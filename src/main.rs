//! ComTap: makes TCP devices show up as COM ports, using com0com virtual port pairs,
//! and runs as a self-healing Windows service.
mod bridge;
mod codec;
mod com0com;
mod config;
mod logger;
mod paths;
mod service;
mod setup;
mod status;
mod traffic;
mod tui;
mod update;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
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
    /// Screen to open on start (used when ComTap reopens itself as admin)
    #[arg(long, hide = true)]
    open: Option<String>,
    /// Set by the updater when the install failed: says so, with the update log's path
    #[arg(long, hide = true)]
    update_failed: bool,
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
    /// Show the service and each device
    Status,
    /// Create or repair the virtual ports from the config (run as administrator)
    Ports,
    /// Update to the latest release (opens the installer in a new window)
    Update,
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
            tui::run(cli.open, cli.update_failed)
        }
        Some(Cmd::Service) => service::dispatch(),
        Some(cmd) => {
            logger::init(paths::logs_dir(), false);
            match cmd {
                Cmd::Install { config } => install_cli(config),
                Cmd::Uninstall { keep_ports, remove_driver } => {
                    setup::uninstall(keep_ports, remove_driver, &mut setup::cli_reporter(&setup::UNINSTALL))
                }
                Cmd::Restart => setup::restart(&mut setup::cli_reporter(&setup::RESTART)),
                Cmd::Status => show_status(),
                Cmd::Update => update::run_update().map_err(anyhow::Error::msg).map(|()| {
                    println!("The installer is running in a new window. Windows will ask for admin permission.");
                }),
                Cmd::Ports => setup::repair(&mut setup::cli_reporter(&setup::REPAIR)),
                Cmd::Run { config } => {
                    logger::init(paths::logs_dir(), true);
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

/// `comtap install`: picks the config (given, next to the exe, or installed), then installs.
fn install_cli(config: Option<PathBuf>) -> Result<()> {
    let installed = paths::installed_config();
    let src = config.unwrap_or_else(|| {
        let beside = paths::exe_dir().join("comtap_config.json");
        if beside.exists() {
            beside
        } else {
            installed.clone()
        }
    });
    if !src.exists() {
        let _ = std::fs::create_dir_all(paths::data_dir());
        if std::fs::write(&installed, config::EXAMPLE).is_ok() {
            bail!(
                "No config yet. I wrote a starter at {}. Set each device's IP:port and COM port, then install again. \
                 Or run comtap with no command for the setup screen.",
                installed.display()
            );
        }
        bail!("No config found. Run comtap with no command for the setup screen.");
    }
    let cfg = Config::load(&src)?;
    setup::install(&cfg, &mut setup::cli_reporter(&setup::INSTALL))
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
        log!("No devices in {}. Idle until stopped.", cfg_path.display());
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

fn show_status() -> Result<()> {
    println!("Service: {}", service::state_text());
    println!("Driver:  {}", service::driver_state_text());
    let Some(st) = status::read() else {
        println!(
            "No status yet ({} not found).",
            paths::status_file().display()
        );
        return Ok(());
    };
    let age = chrono::Local::now().timestamp() - st.written_unix;
    if age > 10 {
        println!(
            "Status last written {} ({age} s ago); the service may not be running.",
            st.written
        );
    }
    for l in &st.links {
        println!();
        println!("{}  {} <-> {}", l.name, l.com, l.tcp);
        println!("  state     {} since {}", l.state, l.since);
        match l.software_open {
            Some(true) => println!("  software  has {} open", l.com),
            Some(false) => println!(
                "  software  none has {} open (device data is dropped)",
                l.com
            ),
            None => {}
        }
        if let Some(e) = &l.last_error {
            println!("  last err  {e}");
        }
        println!("  last data {}", l.last_rx.as_deref().unwrap_or("none yet"));
        println!(
            "  bytes     {} in from device, {} out to device, {} connects",
            l.rx_bytes, l.tx_bytes, l.connects
        );
    }
    Ok(())
}
