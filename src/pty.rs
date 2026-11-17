//! Linux virtual serial ports: one pseudo-terminal per device.
//!
//! ComTap holds the pty's master side. The software opens the slave side through a stable
//! link, /dev/<name> (e.g. /dev/ttyComTap0), which ComTap creates when the service starts and
//! removes when it stops. The slave is set to raw mode and given to the serial-port group
//! (dialout, or uucp on Arch), so users in that group can open it.
//!
//! The kernel reports a hang-up on the master while no program has the slave open, which
//! tells ComTap whether the software has the port open.
use crate::bridge::{Device, POLL};
use crate::config::Link;
use anyhow::{Result, bail};
use std::ffi::CString;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A device's port, as named in the config (e.g. ttyComTap0).
#[derive(Debug, Clone)]
pub struct Port {
    pub name: String,
}

/// Where the links go: /dev, or COMTAP_DEV_DIR (for tests without root).
fn dev_dir() -> PathBuf {
    std::env::var_os("COMTAP_DEV_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev"))
}

/// The path the software opens, e.g. /dev/ttyComTap0.
pub fn link_path(name: &str) -> PathBuf {
    dev_dir().join(name)
}

/// True if `path` is free for ComTap's link: missing, or a link (ComTap's, maybe stale).
fn link_free(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) => m.file_type().is_symlink(),
        Err(_) => true,
    }
}

pub fn prepare(links: &[Link]) -> Result<Vec<Port>> {
    for l in links {
        let p = link_path(&l.com);
        if !link_free(&p) {
            bail!(
                "{} already exists and isn't ComTap's link. Pick another port name for '{}'.",
                p.display(),
                l.name
            );
        }
    }
    Ok(links
        .iter()
        .map(|l| Port {
            name: l.com.clone(),
        })
        .collect())
}

pub fn free_ports(taken: &[String], limit: usize) -> Vec<String> {
    (0..256)
        .map(|i| format!("ttyComTap{i}"))
        .filter(|n| !taken.iter().any(|t| t == n) && link_free(&link_path(n)))
        .take(limit)
        .collect()
}

fn last_err() -> io::Error {
    io::Error::last_os_error()
}

fn check(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(last_err()) } else { Ok(r) }
}

/// Waits up to `ms` for `events` on `fd`. Returns the reported events (0 on timeout).
fn poll(fd: RawFd, events: libc::c_short, ms: libc::c_int) -> io::Result<libc::c_short> {
    let mut p = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    // SAFETY: one valid pollfd, count 1.
    let r = unsafe { libc::poll(&mut p, 1, ms) };
    if r < 0 {
        let e = last_err();
        return if e.kind() == ErrorKind::Interrupted {
            Ok(0)
        } else {
            Err(e)
        };
    }
    Ok(if r == 0 { 0 } else { p.revents })
}

/// The master side and the link to the slave; the link goes when the last clone is dropped.
struct Inner {
    master: OwnedFd,
    link: PathBuf,
    slave: PathBuf,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if std::fs::read_link(&self.link).ok().as_deref() == Some(self.slave.as_path()) {
            let _ = std::fs::remove_file(&self.link);
        }
    }
}

#[derive(Clone)]
struct PtyDevice {
    inner: Arc<Inner>,
}

impl PtyDevice {
    fn fd(&self) -> RawFd {
        self.inner.master.as_raw_fd()
    }
}

/// No program has the slave open: the kernel answers reads with EIO.
fn is_hangup(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::EIO)
}

impl Read for PtyDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rev = poll(self.fd(), libc::POLLIN, POLL.as_millis() as libc::c_int)?;
        if rev & libc::POLLIN != 0 {
            // SAFETY: buf is valid for buf.len() bytes.
            let n = unsafe { libc::read(self.fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                return Ok(n as usize);
            }
            let e = last_err();
            if n < 0 && !is_hangup(&e) && e.kind() != ErrorKind::WouldBlock {
                return Err(e);
            }
        }
        if rev != 0 && rev & libc::POLLIN == 0 || rev & libc::POLLHUP != 0 {
            // Hung up (software closed the port): poll returns at once, so pace the loop.
            std::thread::sleep(POLL);
        }
        Err(ErrorKind::TimedOut.into())
    }
}

impl Write for PtyDevice {
    /// Returns Ok(0) when the software isn't reading, as a timed-out serial write does.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let rev = poll(self.fd(), libc::POLLOUT, POLL.as_millis() as libc::c_int)?;
        if rev & libc::POLLOUT == 0 {
            return Ok(0);
        }
        // SAFETY: buf is valid for buf.len() bytes.
        let n = unsafe { libc::write(self.fd(), buf.as_ptr().cast(), buf.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = last_err();
        if is_hangup(&e) || e.kind() == ErrorKind::WouldBlock {
            Ok(0)
        } else {
            Err(e)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Device for PtyDevice {
    fn try_clone_dev(&self) -> io::Result<Box<dyn Device>> {
        Ok(Box::new(self.clone()))
    }

    fn software_open(&mut self) -> Option<bool> {
        let rev = poll(self.fd(), 0, 0).ok()?;
        Some(rev & libc::POLLHUP == 0)
    }
}

/// The serial-port group's id: dialout, or uucp on Arch.
fn serial_gid() -> Option<libc::gid_t> {
    for name in ["dialout", "uucp"] {
        let c = CString::new(name).ok()?;
        // SAFETY: c is a valid C string; getgrnam returns null or a static record.
        let g = unsafe { libc::getgrnam(c.as_ptr()) };
        if !g.is_null() {
            // SAFETY: g is non-null.
            return Some(unsafe { (*g).gr_gid });
        }
    }
    None
}

/// Makes a pty in raw mode and links /dev/<name> to its slave.
pub fn open(port: &Port) -> io::Result<(Box<dyn Device>, String)> {
    let link = link_path(&port.name);
    if !link_free(&link) {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("{} exists and isn't ComTap's link", link.display()),
        ));
    }
    // SAFETY: plain libc calls; every fd is wrapped in an OwnedFd straight away.
    let master = unsafe {
        let fd = check(libc::posix_openpt(
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        ))?;
        OwnedFd::from_raw_fd(fd)
    };
    let m = master.as_raw_fd();
    // SAFETY: m is a valid pty master.
    unsafe {
        check(libc::grantpt(m))?;
        check(libc::unlockpt(m))?;
    }
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: name is a writable buffer of the given length.
    let r = unsafe { libc::ptsname_r(m, name.as_mut_ptr(), name.len()) };
    if r != 0 {
        return Err(io::Error::from_raw_os_error(r));
    }
    // SAFETY: ptsname_r wrote a NUL-terminated string.
    let slave_c = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_owned();
    let slave = PathBuf::from(slave_c.to_string_lossy().into_owned());

    // Raw mode on the slave (no echo, no line editing, no CR/LF changes); it stays set while
    // ComTap holds the master, whoever opens the slave later.
    // SAFETY: slave_c is a valid path; the fd is owned and closed at the end of the block.
    unsafe {
        let s = OwnedFd::from_raw_fd(check(libc::open(
            slave_c.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        ))?);
        let mut t: libc::termios = std::mem::zeroed();
        check(libc::tcgetattr(s.as_raw_fd(), &mut t))?;
        libc::cfmakeraw(&mut t);
        check(libc::tcsetattr(s.as_raw_fd(), libc::TCSANOW, &t))?;
        // Users in the serial-port group can open it (only root can change this).
        if let Some(gid) = serial_gid() {
            if libc::fchown(s.as_raw_fd(), libc::uid_t::MAX, gid) == 0 {
                libc::fchmod(s.as_raw_fd(), 0o660);
            }
        }
    }
    // SAFETY: m is valid; switch the master to non-blocking.
    unsafe {
        let fl = check(libc::fcntl(m, libc::F_GETFL))?;
        check(libc::fcntl(m, libc::F_SETFL, fl | libc::O_NONBLOCK))?;
    }

    if std::fs::symlink_metadata(&link).is_ok() {
        std::fs::remove_file(&link)?; // a stale link from a run that didn't stop cleanly
    }
    std::os::unix::fs::symlink(&slave, &link)?;
    let desc = format!("{} -> {}", link.display(), slave.display());
    let dev = PtyDevice {
        inner: Arc::new(Inner {
            master,
            link,
            slave,
        }),
    };
    Ok((Box::new(dev), desc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::{Mutex, MutexGuard};

    /// Tests run in parallel and COMTAP_DEV_DIR is process-wide: each test holds this lock
    /// while it uses its own folder.
    static DEV_DIR: Mutex<()> = Mutex::new(());

    fn own_dev_dir(test: &str) -> (MutexGuard<'static, ()>, PathBuf) {
        let guard = DEV_DIR.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("comtap-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: tests in this module are the only users of COMTAP_DEV_DIR, and each sets it
        // while holding DEV_DIR.
        unsafe { std::env::set_var("COMTAP_DEV_DIR", &dir) };
        (guard, dir)
    }

    #[test]
    fn pty_reports_software_open_and_carries_data() {
        let (_guard, dir) = own_dev_dir("pty");
        let (mut dev, desc) = open(&Port {
            name: "ttyComTapTest".into(),
        })
        .unwrap();
        let link = dir.join("ttyComTapTest");
        assert!(desc.contains("ttyComTapTest"));
        assert!(std::fs::read_link(&link).unwrap().starts_with("/dev/pts"));

        // Nobody has it open yet; writes go nowhere and reads time out.
        assert_eq!(dev.software_open(), Some(false));
        let e = dev.read(&mut [0u8; 16]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::TimedOut);

        // The software opens the link: data flows both ways, unchanged (raw mode).
        let mut sw = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&link)
            .unwrap();
        assert_eq!(dev.software_open(), Some(true));
        dev.write_all(b"AB\r\n").unwrap();
        let mut got = [0u8; 4];
        sw.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"AB\r\n");
        sw.write_all(b"T\r").unwrap();
        let mut back = Vec::new();
        while back.len() < 2 {
            let mut b = [0u8; 8];
            if let Ok(n) = dev.read(&mut b) {
                back.extend_from_slice(&b[..n]);
            }
        }
        assert_eq!(back, b"T\r");

        // The software closes it: reported closed again.
        drop(sw);
        assert_eq!(dev.software_open(), Some(false));

        // Dropping ComTap's side removes the link.
        drop(dev);
        assert!(std::fs::symlink_metadata(&link).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A device that is off keeps its link, takes what the software sends without blocking it,
    /// and removes the link when the service stops.
    #[test]
    fn a_device_that_is_off_keeps_its_link() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_guard, dir) = own_dev_dir("off");
        let link = dir.join("ttyComTapOff");
        let stop = Arc::new(AtomicBool::new(false));
        let st: crate::status::Shared = Arc::new(Mutex::new(Default::default()));
        let hold = {
            let (stop, st) = (stop.clone(), st.clone());
            std::thread::spawn(move || {
                crate::bridge::hold_port(
                    Link {
                        name: "off".into(),
                        tcp: "127.0.0.1:9".into(),
                        com: "ttyComTapOff".into(),
                        enabled: false,
                    },
                    Port {
                        name: "ttyComTapOff".into(),
                    },
                    stop,
                    st,
                )
            })
        };
        let start = std::time::Instant::now();
        while std::fs::read_link(&link).is_err() {
            assert!(start.elapsed().as_secs() < 5, "the link never appeared");
            std::thread::sleep(POLL);
        }
        assert_eq!(st.lock().unwrap().state, "off");
        let mut sw = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&link)
            .unwrap();
        // More than a pty buffers: it only completes because the held port reads it.
        sw.write_all(&[b'x'; 64 * 1024]).unwrap();
        drop(sw);
        stop.store(true, Ordering::Relaxed);
        hold.join().unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
