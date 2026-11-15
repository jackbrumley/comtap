//! ComTap: makes TCP devices show up as COM ports, using com0com virtual port pairs,
//! and runs as a self-healing Windows service.
mod bridge;
mod codec;
mod com0com;
mod config;
mod logger;
mod paths;
mod service;
mod status;
mod traffic;
mod tui;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "comtap",
    version,
    about = "Make TCP devices show up as COM ports. Runs as a Windows service.\nRun with no command for the interactive screen."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set everything up and start the service (run as administrator)
    Install {
        /// Config to install (default: comtap_config.json next to comtap.exe, else the installed one)
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Stop and remove the service and its virtual ports (run as administrator)
    Uninstall {
        /// Leave the virtual port pairs in place
        #[arg(long)]
        keep_ports: bool,
        /// Also remove the com0com driver and every com0com pair
        #[arg(long)]
        remove_driver: bool,
    },
    /// Restart the service, e.g. after editing the config (run as administrator)
    Restart,
    /// Show the service and each link
    Status,
    /// Create or repair the virtual ports from the config, then exit (run as administrator)
    Ports {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Run the bridges in this window instead of as a service (for testing)
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Entry point for Windows when it starts the service
    #[command(hide = true)]
    Service,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        None => {
            logger::init(paths::logs_dir(), false);
            tui::run()
        }
        Some(Cmd::Service) => service::dispatch(),
        Some(cmd) => {
            logger::init(paths::logs_dir(), true);
            match cmd {
                Cmd::Install { config } => install(config),
                Cmd::Uninstall { keep_ports, remove_driver } => uninstall(keep_ports, remove_driver),
                Cmd::Restart => restart(),
                Cmd::Status => show_status(),
                Cmd::Ports { config } => ports(config),
                Cmd::Run { config } => {
                    let cfg = paths::resolve_config(config);
                    log!("Running in this window with {} (close the window to stop)", cfg.display());
                    run_all(&cfg, Arc::new(AtomicBool::new(false)))
                }
                Cmd::Service => unreachable!(),
            }
        }
    };
    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn need_admin(what: &str) -> Result<()> {
    if !service::is_admin() {
        bail!("`comtap {what}` needs administrator rights. Open the terminal with \"Run as administrator\".");
    }
    Ok(())
}

/// Runs every link until `stop`. Shared by the service and `comtap run`.
pub fn run_all(cfg_path: &Path, stop: Arc<AtomicBool>) -> Result<()> {
    let cfg = Config::load(cfg_path)?;
    let shared: Vec<status::Shared> = cfg
        .links
        .iter()
        .map(|l| {
            Arc::new(Mutex::new(status::LinkStatus {
                name: l.name.clone(),
                com: l.com.clone(),
                tcp: l.tcp.clone(),
                state: "starting".into(),
                since: status::now(),
                ..Default::default()
            }))
        })
        .collect();
    let writer = status::spawn_writer(shared.clone(), stop.clone());
    if cfg.links.is_empty() {
        log!("No links in {}. Idle until stopped.", cfg_path.display());
        while !stop.load(Ordering::Relaxed) {
            bridge::nap(Duration::from_secs(1), &stop);
        }
        let _ = writer.join();
        return Ok(());
    }

    let traffic: Vec<Arc<traffic::Traffic>> = cfg
        .links
        .iter()
        .map(|l| Arc::new(traffic::Traffic::new(Some(paths::traffic_dir()), &l.com)))
        .collect();
    let flusher = {
        let (traffic, stop) = (traffic.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(traffic::GAP);
                traffic.iter().for_each(|t| t.flush_idle());
            }
        })
    };

    // At boot the driver can come up after us, so keep trying until the ports are ready.
    let devices = loop {
        if stop.load(Ordering::Relaxed) {
            let _ = writer.join();
            let _ = flusher.join();
            return Ok(());
        }
        match com0com::Setupc::find().and_then(|s| com0com::reconcile(&s, &cfg.links)) {
            Ok(d) => break d,
            Err(e) => {
                log!("Virtual ports not ready: {e:#}. Retrying in 30 s.");
                for s in &shared {
                    status::set_state(s, "com-error", Some(format!("{e:#}")));
                }
                bridge::nap(Duration::from_secs(30), &stop);
            }
        }
    };

    let threads: Vec<_> = cfg
        .links
        .iter()
        .zip(devices)
        .zip(shared.iter().cloned())
        .zip(traffic.iter().cloned())
        .map(|(((link, dev), st), tr)| {
            let (link, stop) = (link.clone(), stop.clone());
            std::thread::spawn(move || bridge::run_link(link, dev, stop, st, tr))
        })
        .collect();
    for t in threads {
        let _ = t.join();
    }
    let _ = writer.join();
    let _ = flusher.join();
    Ok(())
}

/// Full setup: config, exe, com0com, ports, service, desktop shortcut. Safe to repeat.
pub fn install(config: Option<PathBuf>) -> Result<()> {
    need_admin("install")?;
    std::fs::create_dir_all(paths::data_dir())?;
    std::fs::create_dir_all(paths::program_dir())?;

    // 1. Config: copy the given one in, or keep the installed one, or write a starter and stop.
    let dest_cfg = paths::installed_config();
    let src_cfg = config.unwrap_or_else(|| paths::exe_dir().join("comtap_config.json"));
    if src_cfg.exists() {
        Config::load(&src_cfg)?;
        let same = std::fs::canonicalize(&src_cfg).ok() == std::fs::canonicalize(&dest_cfg).ok();
        if !same {
            std::fs::copy(&src_cfg, &dest_cfg).with_context(|| format!("copying {}", src_cfg.display()))?;
            log!("Config installed: {}", dest_cfg.display());
        }
    } else if !dest_cfg.exists() {
        std::fs::write(&dest_cfg, config::EXAMPLE)?;
        bail!(
            "No config yet. I wrote a starter at {}. Set each scanner's IP:port and COM port, then install again.",
            dest_cfg.display()
        );
    }
    let cfg = Config::load(&dest_cfg)?;
    if cfg.links.is_empty() {
        bail!("Add at least one scanner (link) before installing.");
    }

    // 2. The exe: stop the service so the old copy isn't locked, then copy ourselves in.
    service::stop()?;
    let exe = std::env::current_exe()?;
    let dest_exe = paths::installed_exe();
    let same = std::fs::canonicalize(&exe).ok() == std::fs::canonicalize(&dest_exe).ok();
    if !same {
        std::fs::copy(&exe, &dest_exe).with_context(|| format!("copying to {}", dest_exe.display()))?;
        log!("Installed {}", dest_exe.display());
    }

    // 3. com0com: unpack, trust its publisher, then create the pairs (this installs the driver on first use).
    let c0c = paths::installed_com0com();
    com0com::deploy_files(&c0c)?;
    if let Err(e) = com0com::trust_publisher(&c0c) {
        log!("Warning: could not pre-trust the com0com publisher ({e:#}). Windows may ask once.");
    }
    let setupc = com0com::Setupc::new(c0c);
    com0com::reconcile(&setupc, &cfg.links)?;

    // 4. Service and shortcut.
    service::install(&dest_exe)?;
    service::start()?;
    log!("Service '{}' installed, set to start automatically, and running.", service::NAME);
    match create_shortcut(&dest_exe) {
        Ok(p) => log!("Desktop shortcut: {}", p.display()),
        Err(e) => log!("Warning: could not create the desktop shortcut: {e:#}"),
    }
    for l in &cfg.links {
        log!("{}: your software opens {} <-> {}", l.name, l.com, l.tcp);
    }
    Ok(())
}

/// Saves a new config and, if the service is installed, applies it straight away:
/// removes pairs for COM ports no longer used, creates new ones, restarts the service.
pub fn apply_config(new: &Config) -> Result<()> {
    need_admin("save")?;
    let path = paths::installed_config();
    let old = Config::load(&path).ok();
    let installed = service::is_installed();
    if installed {
        service::stop()?;
    }
    if let (Some(old), Ok(setupc)) = (old, com0com::Setupc::find()) {
        let gone: Vec<_> = old.links.into_iter().filter(|o| !new.links.iter().any(|n| n.com == o.com)).collect();
        if !gone.is_empty() {
            com0com::remove_pairs(&setupc, &gone)?;
        }
    }
    new.save(&path)?;
    log!("Config saved: {}", path.display());
    if installed {
        if !new.links.is_empty() {
            com0com::reconcile(&com0com::Setupc::find()?, &new.links)?;
        }
        service::start()?;
        log!("Service restarted with the new config");
    }
    Ok(())
}

pub fn uninstall(keep_ports: bool, remove_driver: bool) -> Result<()> {
    need_admin("uninstall")?;
    service::delete()?;
    log!("Service removed");
    if let Ok(setupc) = com0com::Setupc::find() {
        if remove_driver {
            setupc.uninstall_driver()?;
            log!("com0com driver and all its pairs removed");
        } else if !keep_ports {
            if let Ok(cfg) = Config::load(&paths::installed_config()) {
                com0com::remove_pairs(&setupc, &cfg.links)?;
            }
        }
    }
    let _ = std::fs::remove_file(paths::shortcut());
    log!(
        "Uninstalled. Config, logs and traffic are kept in {}. {} can be deleted.",
        paths::data_dir().display(),
        paths::program_dir().display()
    );
    Ok(())
}

pub fn restart() -> Result<()> {
    need_admin("restart")?;
    service::stop()?;
    service::start()?;
    log!("Service restarted");
    Ok(())
}

fn ports(config: Option<PathBuf>) -> Result<()> {
    need_admin("ports")?;
    let cfg = Config::load(&paths::resolve_config(config))?;
    let setupc = com0com::Setupc::find()?;
    com0com::reconcile(&setupc, &cfg.links)?;
    Ok(())
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// ComTap.lnk on the Public Desktop, so every user on the PC can open the screen.
fn create_shortcut(target: &Path) -> Result<PathBuf> {
    let lnk = paths::shortcut();
    let script = format!(
        "$ErrorActionPreference='Stop'; $s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); \
         $s.TargetPath={}; $s.WorkingDirectory={}; $s.Description='ComTap: TCP devices as COM ports'; $s.Save()",
        ps_quote(&lnk.display().to_string()),
        ps_quote(&target.display().to_string()),
        ps_quote(&paths::program_dir().display().to_string()),
    );
    let out = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(lnk)
}

/// Starts this exe again as administrator (UAC prompt). Err if the user says no.
pub fn relaunch_as_admin() -> Result<()> {
    let exe = std::env::current_exe()?;
    let script = format!("Start-Process -FilePath {} -Verb RunAs", ps_quote(&exe.display().to_string()));
    let out = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output()?;
    if !out.status.success() {
        bail!("not elevated (UAC was declined or failed)");
    }
    Ok(())
}

fn show_status() -> Result<()> {
    println!("Service: {}", service::state_text());
    println!("Driver:  {}", service::driver_state_text());
    let Some(st) = status::read() else {
        println!("No status yet ({} not found).", paths::status_file().display());
        return Ok(());
    };
    let age = chrono::Local::now().timestamp() - st.written_unix;
    if age > 10 {
        println!("Status last written {} ({age} s ago); the service may not be running.", st.written);
    }
    for l in &st.links {
        println!();
        println!("{}  {} <-> {}", l.name, l.com, l.tcp);
        println!("  state     {} since {}", l.state, l.since);
        if let Some(e) = &l.last_error {
            println!("  last err  {e}");
        }
        println!("  last data {}", l.last_rx.as_deref().unwrap_or("none yet"));
        println!("  bytes     {} in from device, {} out to device, {} connects", l.rx_bytes, l.tx_bytes, l.connects);
    }
    Ok(())
}
