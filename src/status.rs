//! Live state of each link. The service writes it to status.json every 2 s; `comtap status` reads it.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinkStatus {
    pub name: String,
    pub com: String,
    pub tcp: String,
    /// starting, connecting, connected, retrying, com-error, stopped
    pub state: String,
    pub since: String,
    pub last_error: Option<String>,
    /// Last time data came from the device.
    pub last_rx: Option<String>,
    /// Bytes from the device to the COM port.
    pub rx_bytes: u64,
    /// Bytes from the COM port to the device.
    pub tx_bytes: u64,
    pub connects: u64,
    /// Whether software has the COM port open; None when ComTap can't tell.
    #[serde(default)]
    pub software_open: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusFile {
    pub written: String,
    pub written_unix: i64,
    pub pid: u32,
    pub links: Vec<LinkStatus>,
}

pub type Shared = Arc<Mutex<LinkStatus>>;

pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn set_state(st: &Shared, state: &str, err: Option<String>) {
    let mut s = st.lock().unwrap();
    if s.state != state {
        s.state = state.to_string();
        s.since = now();
    }
    if err.is_some() || state == "connected" {
        s.last_error = err;
    }
}

pub fn spawn_writer(links: Vec<Shared>, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            write_once(&links);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    })
}

fn write_once(links: &[Shared]) {
    let file = StatusFile {
        written: now(),
        written_unix: chrono::Local::now().timestamp(),
        pid: std::process::id(),
        links: links.iter().map(|l| l.lock().unwrap().clone()).collect(),
    };
    let path = crate::paths::status_file();
    let tmp = path.with_extension("json.tmp");
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn read() -> Option<StatusFile> {
    let text = std::fs::read_to_string(crate::paths::status_file()).ok()?;
    serde_json::from_str(&text).ok()
}
