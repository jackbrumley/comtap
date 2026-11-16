//! The ComTap service on Linux: a systemd unit that runs `comtap run` as root at boot and
//! restarts it if it fails. Same functions as the Windows service module.
use crate::paths;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const NAME: &str = "comtap";

const UNIT: &str = "[Unit]
Description=ComTap (TCP device to virtual serial port)
Documentation=https://brumley.info/comtap/
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=__EXE__ run
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
";

/// The unit file's contents for this binary.
pub fn unit_text(exe: &Path) -> String {
    UNIT.replace("__EXE__", &exe.display().to_string())
}

pub fn is_admin() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

fn systemctl(args: &[&str]) -> Result<String> {
    let out = Command::new("systemctl")
        .args(args)
        .output()
        .context("running systemctl")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "systemctl {}: {}",
            args.join(" "),
            err.lines().next().unwrap_or("failed").trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Writes the unit (or updates it) and enables it, so it starts at boot.
pub fn install(exe: &Path) -> Result<()> {
    let unit = paths::systemd_unit();
    std::fs::write(&unit, unit_text(exe)).with_context(|| format!("writing {}", unit.display()))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", NAME])?;
    Ok(())
}

pub fn start() -> Result<()> {
    systemctl(&["start", NAME])?;
    // Started is not the same as running: give a crash on start a moment to show.
    std::thread::sleep(Duration::from_millis(500));
    if state_text() != "Running" {
        bail!("the comtap service did not start; see `journalctl -u comtap`");
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    if is_installed() {
        systemctl(&["stop", NAME])?;
    }
    Ok(())
}

pub fn delete() -> Result<()> {
    if !is_installed() {
        return Ok(());
    }
    stop()?;
    let _ = systemctl(&["disable", NAME]);
    std::fs::remove_file(paths::systemd_unit())?;
    systemctl(&["daemon-reload"])?;
    Ok(())
}

pub fn is_installed() -> bool {
    paths::systemd_unit().exists()
}

/// "Running", "Stopped", "Failed", ... or "not installed".
pub fn state_text() -> String {
    if !is_installed() {
        return "not installed".into();
    }
    // is-active prints the state and exits non-zero unless active, so read stdout directly.
    let out = Command::new("systemctl").args(["is-active", NAME]).output();
    match out {
        Ok(o) => match String::from_utf8_lossy(&o.stdout).trim() {
            "active" => "Running".into(),
            "inactive" => "Stopped".into(),
            "failed" => "Failed".into(),
            "activating" => "Starting".into(),
            "deactivating" => "Stopping".into(),
            other => other.to_string(),
        },
        Err(e) => format!("unknown ({e})"),
    }
}

/// Linux needs no driver: the ports are pseudo-terminals.
pub fn driver_state_text() -> String {
    "pty".into()
}

static SIGNALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::Relaxed);
}

/// A stop flag that turns true on SIGTERM (systemctl stop) or SIGINT (Ctrl+C), so the
/// bridges close cleanly and remove their /dev links.
pub fn stop_on_signals() -> Arc<AtomicBool> {
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::spawn(move || {
        while !SIGNALLED.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        s.store(true, Ordering::Relaxed);
    });
    stop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_the_installed_binary_and_restarts() {
        let u = unit_text(Path::new("/usr/local/bin/comtap"));
        assert!(u.contains("ExecStart=/usr/local/bin/comtap run\n"));
        assert!(u.contains("Restart=always"));
        assert!(u.contains("WantedBy=multi-user.target"));
    }
}
