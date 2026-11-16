//! Daily log files in C:\ProgramData\ComTap\logs, kept for 14 days. Also echoes to the console when asked.
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

struct State {
    dir: PathBuf,
    day: String,
    file: Option<File>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static ECHO: AtomicBool = AtomicBool::new(true);

pub fn init(dir: PathBuf, echo: bool) {
    ECHO.store(echo, Ordering::Relaxed);
    let _ = fs::create_dir_all(&dir);
    prune(&dir);
    *STATE.lock().unwrap() = Some(State {
        dir,
        day: String::new(),
        file: None,
    });
}

fn prune(dir: &Path) {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(14 * 86400);
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if old && e.file_name().to_string_lossy().starts_with("comtap-") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

pub fn write(msg: &str) {
    let now = chrono::Local::now();
    let line = format!("{} {}", now.format("%Y-%m-%d %H:%M:%S%.3f"), msg);
    if ECHO.load(Ordering::Relaxed) {
        println!("{line}");
    }
    let mut guard = STATE.lock().unwrap();
    let Some(st) = guard.as_mut() else { return };
    let day = now.format("%Y-%m-%d").to_string();
    if st.day != day || st.file.is_none() {
        st.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(st.dir.join(format!("comtap-{day}.log")))
            .ok();
        st.day = day;
    }
    if let Some(f) = st.file.as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::logger::write(&format!($($t)*)) };
}
