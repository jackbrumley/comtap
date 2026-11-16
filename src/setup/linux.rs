//! Linux steps: the binary in /usr/local/bin and a systemd service. The ports are
//! pseudo-terminals the service makes itself when it starts, so there is no driver.
use super::{Plan, Rep, St, check_admin, check_connects, current_links, done, step};
use crate::config::Config;
use crate::{log, paths, service};
use anyhow::{Context, Result, bail};
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

pub const INSTALL: Plan = Plan {
    title: "Installing ComTap",
    steps: &[
        "Check root rights",
        "Save settings",
        "Copy comtap to /usr/local/bin",
        "Install the systemd service (starts at boot)",
        "Start the service",
        "Check each device connects",
    ],
};

pub const APPLY: Plan = Plan {
    title: "Saving and applying",
    steps: &[
        "Check root rights",
        "Stop the service",
        "Save settings",
        "Start the service",
        "Check each device connects",
    ],
};

pub const REPAIR: Plan = Plan {
    title: "Repairing virtual ports",
    steps: &[
        "Check root rights",
        "Stop the service",
        "Start the service (it makes the ports again)",
    ],
};

pub const UNINSTALL: Plan = Plan {
    title: "Uninstalling ComTap",
    steps: &["Check root rights", "Stop and remove the service"],
};

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
        if std::fs::canonicalize(&exe).ok() == std::fs::canonicalize(&dest).ok() {
            return Ok(St::Skip("already running from there".into()));
        }
        // Copy beside it, then rename: the old binary may still be running (this screen).
        let tmp = dest.with_extension("new");
        std::fs::copy(&exe, &tmp).with_context(|| format!("copying to {}", tmp.display()))?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, &dest).with_context(|| format!("replacing {}", dest.display()))?;
        done(dest.display().to_string())
    })?;
    step(rep, 3, || {
        service::install(&paths::installed_exe())?;
        done("starts at boot, restarts if it fails")
    })?;
    step(rep, 4, || {
        service::start()?;
        done("")
    })?;
    step(rep, 5, || check_connects(&cfg.links))?;
    log!(
        "Installed. Software opens {}",
        super::ports_summary(&cfg.links)
    );
    Ok(())
}

/// Saves a new config. If the service is installed, applies it straight away.
pub fn apply(new: &Config, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    let installed = service::is_installed();
    let not_installed = || Ok(St::Skip("not installed yet".into()));
    step(rep, 1, || {
        if !installed {
            return not_installed();
        }
        service::stop()?;
        done("")
    })?;
    step(rep, 2, || {
        new.save(&paths::installed_config())?;
        done("")
    })?;
    step(rep, 3, || {
        if !installed {
            return not_installed();
        }
        service::start()?;
        done("")
    })?;
    step(rep, 4, || {
        if installed {
            check_connects(&new.links)
        } else {
            not_installed()
        }
    })?;
    Ok(())
}

pub fn repair(rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || service::stop().map(|_| St::Done(String::new())))?;
    step(rep, 2, || {
        service::start()?;
        done(super::ports_summary(&current_links()))
    })?;
    Ok(())
}

/// Stops and removes the service; its /dev links go with it. The flags are Windows-only.
pub fn uninstall(_keep_ports: bool, _remove_driver: bool, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || {
        service::delete()?;
        done("")
    })?;
    log!(
        "Uninstalled. Config stays in {}, logs in {}",
        paths::installed_config().display(),
        paths::logs_dir().display()
    );
    Ok(())
}

static RELAUNCH: Mutex<Option<String>> = Mutex::new(None);

/// Marks ComTap to reopen with sudo (opening `open` on the screen) once the screen has closed.
pub fn relaunch_as_admin(open: &str) -> Result<()> {
    *RELAUNCH.lock().unwrap() = Some(open.to_string());
    Ok(())
}

/// The screen has closed: replace this process with `sudo comtap --open <screen>`, in this
/// terminal, so sudo can ask for the password. Returns only if that fails.
pub fn run_pending_relaunch() {
    use std::os::unix::process::CommandExt;
    let Some(open) = RELAUNCH.lock().unwrap().take() else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let e = std::process::Command::new("sudo")
        .arg(exe)
        .args(["--open", &open])
        .exec();
    eprintln!("Could not reopen ComTap with sudo: {e}. Run: sudo comtap");
}
