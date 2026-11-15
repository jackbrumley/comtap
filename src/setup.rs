//! Install, apply, restart and uninstall, each as a fixed list of named steps.
//! The screen shows the list as a checklist; the command line prints it.
use crate::config::{Config, Link};
use crate::{com0com, log, paths, service, status};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::time::{Duration, Instant};

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

pub const INSTALL: Plan = Plan {
    title: "Installing ComTap",
    steps: &[
        "Check administrator rights",
        "Save settings",
        "Copy ComTap to Program Files",
        "Unpack the com0com driver",
        "Remove other com0com driver versions",
        "Trust the com0com driver publisher",
        "Create the virtual COM ports",
        "Install the ComTap service (starts at boot)",
        "Start the service",
        "Create the desktop shortcut",
        "Check each device connects",
    ],
};

pub const APPLY: Plan = Plan {
    title: "Saving and applying",
    steps: &[
        "Check administrator rights",
        "Stop the service",
        "Remove virtual ports no longer used",
        "Save settings",
        "Create the virtual COM ports",
        "Start the service",
        "Check each device connects",
    ],
};

pub const RESTART: Plan = Plan {
    title: "Restarting the service",
    steps: &["Check administrator rights", "Stop the service", "Start the service", "Check each device connects"],
};

pub const STOP: Plan = Plan { title: "Stopping the service", steps: &["Check administrator rights", "Stop the service"] };

pub const REPAIR: Plan = Plan {
    title: "Repairing virtual ports",
    steps: &["Check administrator rights", "Stop the service", "Create the virtual COM ports", "Start the service"],
};

pub const UNINSTALL: Plan = Plan {
    title: "Uninstalling ComTap",
    steps: &[
        "Check administrator rights",
        "Stop and remove the service",
        "Remove the virtual COM ports",
        "Remove the desktop shortcut",
    ],
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
        bail!("ComTap is not running as administrator. Open it with \"Run as administrator\".");
    }
    done("")
}

fn ports_summary(links: &[Link]) -> String {
    links.iter().map(|l| l.com.as_str()).collect::<Vec<_>>().join(", ")
}

fn create_ports(links: &[Link]) -> Result<St> {
    if links.is_empty() {
        return Ok(St::Skip("no devices".into()));
    }
    com0com::reconcile(&com0com::Setupc::find()?, links)?;
    done(ports_summary(links))
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
                let s = st.as_ref().and_then(|s| s.links.iter().find(|x| x.com == l.com));
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

/// Full setup with this config. Safe to run again (upgrade, repair).
pub fn install(cfg: &Config, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || {
        if cfg.links.is_empty() {
            bail!("Add at least one device first.");
        }
        cfg.save(&paths::installed_config())?;
        done(paths::installed_config().display().to_string())
    })?;
    step(rep, 2, || {
        service::stop()?;
        let exe = std::env::current_exe()?;
        let dest = paths::installed_exe();
        std::fs::create_dir_all(paths::program_dir())?;
        if std::fs::canonicalize(&exe).ok() == std::fs::canonicalize(&dest).ok() {
            return Ok(St::Skip("already running from there".into()));
        }
        std::fs::copy(&exe, &dest).with_context(|| format!("copying to {}", dest.display()))?;
        done(dest.display().to_string())
    })?;
    step(rep, 3, || {
        com0com::deploy_files(&paths::installed_com0com())?;
        done(paths::installed_com0com().display().to_string())
    })?;
    step(rep, 4, || {
        let other = com0com::other_driver_packages();
        if other.is_empty() {
            return Ok(St::Skip("none found".into()));
        }
        let mut removed = Vec::new();
        for (oem, ver) in other {
            com0com::delete_driver_package(&oem)?;
            log!("Removed com0com driver package {oem} ({ver})");
            removed.push(format!("{ver} ({oem})"));
        }
        done(format!("removed {}", removed.join(", ")))
    })?;
    step(rep, 5, || match com0com::trust_publisher(&paths::installed_com0com()) {
        Ok(()) => done(""),
        Err(e) => Ok(St::Warn(format!("{e:#}. Windows may ask once to install the driver: click Install."))),
    })?;
    step(rep, 6, || create_ports(&cfg.links))?;
    step(rep, 7, || {
        service::install(&paths::installed_exe())?;
        done("automatic start, restarts if it fails")
    })?;
    step(rep, 8, || {
        service::start()?;
        done("")
    })?;
    step(rep, 9, || match create_shortcut(&paths::installed_exe()) {
        Ok(p) => done(p.display().to_string()),
        Err(e) => Ok(St::Warn(format!("{e:#}"))),
    })?;
    step(rep, 10, || check_connects(&cfg.links))?;
    log!("Installed. Software opens {}", ports_summary(&cfg.links));
    Ok(())
}

/// Saves a new config. If the service is installed, applies it straight away.
pub fn apply(new: &Config, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    let installed = service::is_installed();
    let old = Config::load(&paths::installed_config()).ok();
    let not_installed = || Ok(St::Skip("not installed yet".into()));
    step(rep, 1, || {
        if !installed {
            return not_installed();
        }
        service::stop()?;
        done("")
    })?;
    step(rep, 2, || {
        let Some(old) = &old else { return Ok(St::Skip("nothing to remove".into())) };
        let gone: Vec<Link> =
            old.links.iter().filter(|o| !new.links.iter().any(|n| n.com == o.com)).cloned().collect();
        if gone.is_empty() {
            return Ok(St::Skip("nothing to remove".into()));
        }
        let Ok(setupc) = com0com::Setupc::find() else { return Ok(St::Skip("com0com not set up".into())) };
        com0com::remove_pairs(&setupc, &gone)?;
        done(ports_summary(&gone))
    })?;
    step(rep, 3, || {
        new.save(&paths::installed_config())?;
        done("")
    })?;
    step(rep, 4, || if installed { create_ports(&new.links) } else { not_installed() })?;
    step(rep, 5, || {
        if !installed {
            return not_installed();
        }
        service::start()?;
        done("")
    })?;
    step(rep, 6, || if installed { check_connects(&new.links) } else { not_installed() })?;
    Ok(())
}

fn current_links() -> Vec<Link> {
    Config::load(&paths::installed_config()).map(|c| c.links).unwrap_or_default()
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

pub fn repair(rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || service::stop().map(|_| St::Done(String::new())))?;
    step(rep, 2, || create_ports(&current_links()))?;
    step(rep, 3, || service::start().map(|_| St::Done(String::new())))?;
    Ok(())
}

pub fn uninstall(keep_ports: bool, remove_driver: bool, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || service::delete().map(|_| St::Done(String::new())))?;
    step(rep, 2, || {
        let Ok(setupc) = com0com::Setupc::find() else { return Ok(St::Skip("com0com not set up".into())) };
        if remove_driver {
            setupc.uninstall_driver()?;
            return done("removed the com0com driver and every com0com pair");
        }
        if keep_ports {
            return Ok(St::Skip("kept, as asked".into()));
        }
        let links = current_links();
        com0com::remove_pairs(&setupc, &links)?;
        done(ports_summary(&links))
    })?;
    step(rep, 3, || {
        let _ = std::fs::remove_file(paths::shortcut());
        done("")
    })?;
    log!("Uninstalled. Config, logs and traffic stay in {}", paths::data_dir().display());
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

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// ComTap.lnk on the Public Desktop, so every user on the PC can open the screen.
fn create_shortcut(target: &Path) -> Result<std::path::PathBuf> {
    let lnk = paths::shortcut();
    let script = format!(
        "$ErrorActionPreference='Stop'; $s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); \
         $s.TargetPath={}; $s.WorkingDirectory={}; $s.Description='ComTap: TCP devices as COM ports'; $s.Save()",
        ps_quote(&lnk.display().to_string()),
        ps_quote(&target.display().to_string()),
        ps_quote(&paths::program_dir().display().to_string()),
    );
    com0com::powershell(&script)?;
    Ok(lnk)
}

/// Starts this exe again as administrator (UAC prompt), opening `open` on the screen.
pub fn relaunch_as_admin(open: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    let script = format!(
        "Start-Process -FilePath {} -ArgumentList '--open',{} -Verb RunAs",
        ps_quote(&exe.display().to_string()),
        ps_quote(open)
    );
    if com0com::powershell(&script).is_err() {
        bail!("Windows did not give admin rights (the UAC prompt was declined)");
    }
    Ok(())
}
