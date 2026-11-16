//! Windows steps: com0com port pairs, the Windows service, the desktop shortcut and PATH.
use super::{Plan, Rep, St, check_admin, check_connects, current_links, done, ports_summary, step};
use crate::config::{Config, Link};
use crate::{com0com, log, paths, service};
use anyhow::{Context, Result, bail};
use std::path::Path;

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
        "Add comtap to the system PATH",
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

pub const REPAIR: Plan = Plan {
    title: "Repairing virtual ports",
    steps: &[
        "Check administrator rights",
        "Stop the service",
        "Create the virtual COM ports",
        "Start the service",
    ],
};

pub const UNINSTALL: Plan = Plan {
    title: "Uninstalling ComTap",
    steps: &[
        "Check administrator rights",
        "Stop and remove the service",
        "Remove the virtual COM ports",
        "Remove the desktop shortcut",
        "Remove comtap from the system PATH",
    ],
};

fn create_ports(links: &[Link]) -> Result<St> {
    if links.is_empty() {
        return Ok(St::Skip("no devices".into()));
    }
    com0com::reconcile(&com0com::Setupc::find()?, links)?;
    done(ports_summary(links))
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
    step(rep, 5, || {
        match com0com::trust_publisher(&paths::installed_com0com()) {
            Ok(()) => done(""),
            Err(e) => Ok(St::Warn(format!(
                "{e:#}. Windows may ask once to install the driver: click Install."
            ))),
        }
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
    step(rep, 10, || match set_on_path(&paths::program_dir(), true) {
        Ok(true) => done("open a new terminal to use it"),
        Ok(false) => done("already there"),
        Err(e) => Ok(St::Warn(format!("{e:#}"))),
    })?;
    step(rep, 11, || check_connects(&cfg.links))?;
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
        let Some(old) = &old else {
            return Ok(St::Skip("nothing to remove".into()));
        };
        let gone: Vec<Link> = old
            .links
            .iter()
            .filter(|o| !new.links.iter().any(|n| n.com == o.com))
            .cloned()
            .collect();
        if gone.is_empty() {
            return Ok(St::Skip("nothing to remove".into()));
        }
        let Ok(setupc) = com0com::Setupc::find() else {
            return Ok(St::Skip("com0com not set up".into()));
        };
        com0com::remove_pairs(&setupc, &gone)?;
        done(ports_summary(&gone))
    })?;
    step(rep, 3, || {
        new.save(&paths::installed_config())?;
        done("")
    })?;
    step(rep, 4, || {
        if installed {
            create_ports(&new.links)
        } else {
            not_installed()
        }
    })?;
    step(rep, 5, || {
        if !installed {
            return not_installed();
        }
        service::start()?;
        done("")
    })?;
    step(rep, 6, || {
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
    step(rep, 2, || create_ports(&current_links()))?;
    step(rep, 3, || service::start().map(|_| St::Done(String::new())))?;
    Ok(())
}

pub fn uninstall(keep_ports: bool, remove_driver: bool, rep: Rep) -> Result<()> {
    step(rep, 0, check_admin)?;
    step(rep, 1, || {
        service::delete().map(|_| St::Done(String::new()))
    })?;
    step(rep, 2, || {
        let Ok(setupc) = com0com::Setupc::find() else {
            return Ok(St::Skip("com0com not set up".into()));
        };
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
    step(rep, 4, || match set_on_path(&paths::program_dir(), false) {
        Ok(true) => done(""),
        Ok(false) => Ok(St::Skip("not there".into())),
        Err(e) => Ok(St::Warn(format!("{e:#}"))),
    })?;
    log!(
        "Uninstalled. Config, logs and traffic stay in {}",
        paths::data_dir().display()
    );
    Ok(())
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

/// Adds `dir` to (or removes it from) the machine PATH, so `comtap` runs from any new terminal.
/// Returns whether PATH changed. Reads and writes the raw registry value, so entries such as
/// `%SystemRoot%\system32` stay unexpanded, then tells running programs that PATH changed.
fn set_on_path(dir: &Path, add: bool) -> Result<bool> {
    const SCRIPT: &str = r#"$ErrorActionPreference='Stop'
$d = __DIR__
$k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment', $true)
$old = $k.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$parts = @($old -split ';' | Where-Object { $_ -ne '' })
$others = @($parts | Where-Object { $_.TrimEnd('\') -ine $d.TrimEnd('\') })
$present = $others.Count -lt $parts.Count
if ((__ADD__) -eq $present) { 'unchanged'; exit }
if (__ADD__) { $new = $others + $d } else { $new = $others }
$k.SetValue('Path', ($new -join ';'), 'ExpandString')
Add-Type -Namespace ComTap -Name Env -MemberDefinition '[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, string l, uint f, uint t, out UIntPtr r);'
$r = [UIntPtr]::Zero
[void][ComTap.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)
'changed'"#;
    let script = SCRIPT
        .replace("__DIR__", &ps_quote(&dir.display().to_string()))
        .replace("__ADD__", if add { "$true" } else { "$false" });
    let out = com0com::powershell(&script)?;
    Ok(out.trim() == "changed")
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

/// Windows reopens as admin straight away (see `relaunch_as_admin`), so nothing waits here.
pub fn run_pending_relaunch() {}
