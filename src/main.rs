//! ComTap: makes TCP devices show up as COM ports, using com0com virtual port pairs,
//! and runs as a self-healing Windows service.
mod bridge;
mod com0com;
mod config;
mod logger;
mod paths;
mod service;
mod status;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use config::Config;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "comtap", version, about = "Make TCP devices show up as COM ports. Runs as a Windows service.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set everything up and start the service (run as administrator)
    Install {
        /// Config to install (default: comtap_config.json next to comtap.exe)
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
        Cmd::Service => service::dispatch(),
        Cmd::Install { config } => install(config),
        Cmd::Uninstall { keep_ports, remove_driver } => uninstall(keep_ports, remove_driver),
        Cmd::Restart => restart(),
        Cmd::Status => show_status(),
        Cmd::Ports { config } => ports(config),
        Cmd::Run { config } => {
            logger::init(paths::logs_dir(), true);
            let cfg = paths::resolve_config(config);
            log!("Running in this window with {} (close the window to stop)", cfg.display());
            run_all(&cfg, Arc::new(AtomicBool::new(false)))
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

    // At boot the driver can come up after us, so keep trying until the ports are ready.
    let devices = loop {
        if stop.load(Ordering::Relaxed) {
            let _ = writer.join();
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
        .map(|((link, dev), st)| {
            let (link, stop) = (link.clone(), stop.clone());
            std::thread::spawn(move || bridge::run_link(link, dev, stop, st))
        })
        .collect();
    for t in threads {
        let _ = t.join();
    }
    let _ = writer.join();
    Ok(())
}

fn install(config: Option<PathBuf>) -> Result<()> {
    need_admin("install")?;
    std::fs::create_dir_all(paths::data_dir())?;
    std::fs::create_dir_all(paths::program_dir())?;
    logger::init(paths::logs_dir(), true);

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
            "No config yet. I wrote a starter at {}. Set each scanner's IP:port and COM port, then run `comtap install` again.",
            dest_cfg.display()
        );
    }
    let cfg = Config::load(&dest_cfg)?;

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

    // 4. Service.
    service::install(&dest_exe)?;
    service::start()?;
    log!("Service '{}' installed, set to start automatically, and running.", service::NAME);
    for l in &cfg.links {
        println!("  {}: your software opens {}  <->  {}", l.name, l.com, l.tcp);
    }
    println!("Check it with: comtap status");
    Ok(())
}

fn uninstall(keep_ports: bool, remove_driver: bool) -> Result<()> {
    need_admin("uninstall")?;
    logger::init(paths::logs_dir(), true);
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
    println!(
        "Done. Config and logs are kept in {}. {} can be deleted.",
        paths::data_dir().display(),
        paths::program_dir().display()
    );
    Ok(())
}

fn restart() -> Result<()> {
    need_admin("restart")?;
    service::stop()?;
    service::start()?;
    println!("Service restarted.");
    show_status()
}

fn ports(config: Option<PathBuf>) -> Result<()> {
    need_admin("ports")?;
    logger::init(paths::logs_dir(), true);
    let cfg = Config::load(&paths::resolve_config(config))?;
    let setupc = com0com::Setupc::find()?;
    com0com::reconcile(&setupc, &cfg.links)?;
    Ok(())
}

fn show_status() -> Result<()> {
    println!("Service: {}", service::state_text());
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
