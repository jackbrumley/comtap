//! Install, apply, restart and uninstall, each as a fixed list of named steps.
//! The screen shows the list as a checklist; the command line prints it.
use crate::config::{Config, Link};
use crate::{paths, ports, service, status};
use anyhow::{Result, bail};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(not(windows))]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;

#[derive(Debug, Clone, PartialEq)]
pub enum St {
    Pending,
    Running,
    Done(String),
    /// Finished, but something needs attention. Not a failure.
    Warn(String),
    Skip(String),
    Fail(String),
}

/// Receives (step index, new state).
pub type Rep<'a> = &'a mut dyn FnMut(usize, St);

pub struct Plan {
    pub title: &'static str,
    pub steps: &'static [&'static str],
}

pub const RESTART: Plan = Plan {
    title: "Restarting the service",
    steps: &[
        "Check administrator rights",
        "Stop the service",
        "Start the service",
        "Check each device connects",
    ],
};

pub const STOP: Plan = Plan {
    title: "Stopping the service",
    steps: &["Check administrator rights", "Stop the service"],
};

/// Runs one step: reports Running, then the result. A failed step stops the whole plan.
fn step(rep: Rep, i: usize, f: impl FnOnce() -> Result<St>) -> Result<()> {
    rep(i, St::Running);
    match f() {
        Ok(s) => {
            rep(i, s);
            Ok(())
        }
        Err(e) => {
            rep(i, St::Fail(format!("{e:#}")));
            Err(e)
        }
    }
}

fn done(s: impl Into<String>) -> Result<St> {
    Ok(St::Done(s.into()))
}

fn check_admin() -> Result<St> {
    if !service::is_admin() {
        if cfg!(windows) {
            bail!("ComTap is not running as administrator. Open it with \"Run as administrator\".");
        }
        bail!("ComTap is not running as root. Run it with sudo, e.g. `sudo comtap`.");
    }
    done("")
}

fn ports_summary(links: &[Link]) -> String {
    links
        .iter()
        .map(|l| ports::software_path(&l.com))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Waits up to 15 s for the service to report every link connected.
fn check_connects(links: &[Link]) -> Result<St> {
    if links.is_empty() {
        return Ok(St::Skip("no devices".into()));
    }
    let end = Instant::now() + Duration::from_secs(15);
    let started = chrono::Local::now().timestamp();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let st = status::read().filter(|s| s.written_unix >= started);
        let states: Vec<(String, String, Option<String>)> = links
            .iter()
            .map(|l| {
                let s = st
                    .as_ref()
                    .and_then(|s| s.links.iter().find(|x| x.com == l.com));
                (
                    format!("{} ({} ↔ {})", l.name, l.com, l.tcp),
                    s.map(|s| s.state.clone()).unwrap_or_default(),
                    s.and_then(|s| s.last_error.clone()),
                )
            })
            .collect();
        if states.iter().all(|s| s.1 == "connected") {
            return done(format!("{} connected", states.len()));
        }
        if Instant::now() > end {
            let bad: Vec<String> = states
                .iter()
                .filter(|s| s.1 != "connected")
                .map(|s| format!("{}: {}", s.0, s.2.clone().unwrap_or_else(|| s.1.clone())))
                .collect();
            return Ok(St::Warn(format!(
                "Not connected yet. The service keeps retrying. {}",
                bad.join("; ")
            )));
        }
    }
}

fn current_links() -> Vec<Link> {
    Config::load(&paths::installed_config())
        .map(|c| c.links)
        .unwrap_or_default()
}

pub fn restart(rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || service::stop().map(|_| St::Done(String::new())))?;
    step(rep, 2, || service::start().map(|_| St::Done(String::new())))?;
    step(rep, 3, || check_connects(&current_links()))?;
    Ok(())
}

pub fn stop(rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || service::stop().map(|_| St::Done(String::new())))?;
    Ok(())
}

/// Prints each step as it finishes, for the command line.
pub fn cli_reporter(plan: &'static Plan) -> impl FnMut(usize, St) {
    println!("{}", plan.title);
    move |i, st| {
        let name = plan.steps[i];
        let (tag, detail) = match &st {
            St::Pending | St::Running => return,
            St::Done(d) => ("  ok  ", d),
            St::Warn(d) => (" warn ", d),
            St::Skip(d) => (" skip ", d),
            St::Fail(d) => (" FAIL ", d),
        };
        if detail.is_empty() {
            println!("[{tag}] {name}");
        } else {
            println!("[{tag}] {name}: {detail}");
        }
    }
}
