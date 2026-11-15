//! Per-link traffic log, grouped like RawTap's raw log: a new entry on a newline, a 30 ms gap,
//! a change of direction, or every 1 KB. One file per link per day:
//!
//! ```text
//! C:\ProgramData\ComTap\traffic\traffic-COM5-2026-11-16.log
//! 2026-11-16 12:01:33.120 RX 30 31 32 33 0D 0A | 0123<CR><LF>
//! 2026-11-16 12:01:40.002 TX 54 0D | T<CR>
//! 2026-11-16 12:05:00.000 -- disconnected: device closed the connection
//! ```
//!
//! Files older than 14 days are deleted, and each link's files are capped at 50 MB in total.
use crate::codec;
use chrono::{DateTime, Local};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const GAP: Duration = Duration::from_millis(30);
const MAX_ENTRY: usize = 1024;
const KEEP_DAYS: i64 = 14;
pub const CAP_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Rx,
    Tx,
}

impl Dir {
    fn tag(self) -> &'static str {
        match self {
            Dir::Rx => "RX",
            Dir::Tx => "TX",
        }
    }
}

struct Pending {
    dir: Dir,
    start: DateTime<Local>,
    last: Instant,
    data: Vec<u8>,
}

struct Inner {
    pending: Option<Pending>,
    file: Option<File>,
    day: String,
    size: u64,
    capped: bool,
}

pub struct Traffic {
    dir: Option<PathBuf>,
    com: String,
    inner: Mutex<Inner>,
}

pub fn file_for(dir: &Path, com: &str, day: &str) -> PathBuf {
    dir.join(format!("traffic-{com}-{day}.log"))
}

impl Traffic {
    pub fn new(dir: Option<PathBuf>, com: &str) -> Traffic {
        if let Some(d) = &dir {
            let _ = fs::create_dir_all(d);
        }
        Traffic {
            dir,
            com: com.to_string(),
            inner: Mutex::new(Inner { pending: None, file: None, day: String::new(), size: 0, capped: false }),
        }
    }

    /// A Traffic that records nothing (tests).
    #[cfg(test)]
    pub fn disabled() -> Traffic {
        Traffic::new(None, "TEST")
    }

    pub fn record(&self, dir: Dir, bytes: &[u8]) {
        if self.dir.is_none() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        let now = Instant::now();
        let split = g.pending.as_ref().is_some_and(|p| p.dir != dir || now.duration_since(p.last) > GAP);
        if split {
            self.flush(&mut g);
        }
        for &b in bytes {
            let p = g.pending.get_or_insert_with(|| Pending { dir, start: Local::now(), last: now, data: Vec::new() });
            p.data.push(b);
            p.last = now;
            if b == b'\n' || p.data.len() >= MAX_ENTRY {
                self.flush(&mut g);
            }
        }
    }

    /// Writes a one-line event (connected, disconnected...).
    pub fn event(&self, msg: &str) {
        if self.dir.is_none() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        self.flush(&mut g);
        let line = format!("{} -- {msg}", Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));
        self.write_line(&mut g, &line);
    }

    /// Ends an entry that has been quiet for longer than GAP.
    pub fn flush_idle(&self) {
        let mut g = self.inner.lock().unwrap();
        if g.pending.as_ref().is_some_and(|p| p.last.elapsed() > GAP) {
            self.flush(&mut g);
        }
    }

    fn flush(&self, g: &mut Inner) {
        let Some(p) = g.pending.take() else { return };
        let line = format!(
            "{} {} {} | {}",
            p.start.format("%Y-%m-%d %H:%M:%S%.3f"),
            p.dir.tag(),
            codec::hex_string(&p.data),
            codec::text_string(&p.data)
        );
        self.write_line(g, &line);
    }

    fn write_line(&self, g: &mut Inner, line: &str) {
        let Some(dir) = &self.dir else { return };
        let day = Local::now().format("%Y-%m-%d").to_string();
        if g.day != day || g.file.is_none() {
            prune(dir, &self.com, &day);
            let path = file_for(dir, &self.com, &day);
            g.file = OpenOptions::new().create(true).append(true).open(&path).ok();
            g.size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            g.capped = false;
            g.day = day;
        }
        if g.capped {
            return;
        }
        let Some(f) = g.file.as_mut() else { return };
        if g.size >= CAP_BYTES {
            let _ = writeln!(f, "{} -- traffic log reached 50 MB today; paused until tomorrow", Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f"));
            g.capped = true;
            return;
        }
        if writeln!(f, "{line}").is_ok() {
            g.size += line.len() as u64 + 1;
        }
    }
}

/// Deletes this link's files older than KEEP_DAYS, then the oldest until the total is under the cap.
fn prune(dir: &Path, com: &str, today: &str) {
    let prefix = format!("traffic-{com}-");
    let mut files: Vec<(String, PathBuf, u64)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let day = name.strip_prefix(&prefix)?.strip_suffix(".log")?.to_string();
            Some((day, e.path(), e.metadata().map(|m| m.len()).unwrap_or(0)))
        })
        .collect();
    files.sort();
    let cutoff = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map(|d| (d - chrono::Duration::days(KEEP_DAYS)).format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    for (day, path, size) in &files {
        let too_old = day.as_str() < cutoff.as_str();
        let over = total > CAP_BYTES && day.as_str() < today;
        if (too_old || over) && fs::remove_file(path).is_ok() {
            total -= size;
        }
    }
}

/// One parsed line of a traffic file, for the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Data { time: String, dir: Dir, bytes: Vec<u8> },
    Event { time: String, msg: String },
}

pub fn parse_line(s: &str) -> Option<Line> {
    // "YYYY-MM-DD HH:MM:SS.mmm XX rest"
    if s.len() < 27 {
        return None;
    }
    let time = s.get(11..23)?.to_string();
    let rest = s.get(24..)?;
    if let Some(msg) = rest.strip_prefix("-- ") {
        return Some(Line::Event { time, msg: msg.to_string() });
    }
    let dir = match rest.get(..3)? {
        "RX " => Dir::Rx,
        "TX " => Dir::Tx,
        _ => return None,
    };
    let hex = rest[3..].split(" | ").next().unwrap_or("");
    Some(Line::Data { time, dir, bytes: codec::parse_hex(hex) })
}

/// The last `max_bytes` of today's file for this link, as parsed lines.
pub fn tail(dir: &Path, com: &str, max_bytes: u64) -> (Vec<Line>, u64) {
    use std::io::{Read, Seek, SeekFrom};
    let day = Local::now().format("%Y-%m-%d").to_string();
    let Ok(mut f) = File::open(file_for(dir, com, &day)) else { return (Vec::new(), 0) };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // probably cut in half
    }
    (lines.filter_map(parse_line).collect(), len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("comtap-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn groups_like_rawtap_and_parses_back() {
        let dir = tmp("group");
        let t = Traffic::new(Some(dir.clone()), "COM9");
        t.record(Dir::Rx, b"0123\r\n4567\r\n"); // two entries, split on newline
        t.record(Dir::Rx, b"AB"); // pending...
        t.record(Dir::Tx, b"T\r"); // direction change ends "AB"
        std::thread::sleep(GAP * 2);
        t.flush_idle(); // gap ends "T\r"
        t.event("disconnected: test");
        let (lines, _) = tail(&dir, "COM9", 1 << 20);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(matches!(&lines[0], Line::Data { dir: Dir::Rx, bytes, .. } if bytes == b"0123\r\n"));
        assert!(matches!(&lines[1], Line::Data { bytes, .. } if bytes == b"4567\r\n"));
        assert!(matches!(&lines[2], Line::Data { bytes, .. } if bytes == b"AB"));
        assert!(matches!(&lines[3], Line::Data { dir: Dir::Tx, bytes, .. } if bytes == b"T\r"));
        assert!(matches!(&lines[4], Line::Event { msg, .. } if msg == "disconnected: test"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prunes_old_files() {
        let dir = tmp("prune");
        fs::create_dir_all(&dir).unwrap();
        fs::write(file_for(&dir, "COM9", "2020-01-01"), "old").unwrap();
        fs::write(file_for(&dir, "COM8", "2020-01-01"), "other link").unwrap();
        let t = Traffic::new(Some(dir.clone()), "COM9");
        t.event("start");
        assert!(!file_for(&dir, "COM9", "2020-01-01").exists());
        assert!(file_for(&dir, "COM8", "2020-01-01").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
