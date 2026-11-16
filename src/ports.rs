//! The virtual serial port for each device, on each platform.
//!
//! Windows: a com0com pair. The software opens side A (e.g. COM5); ComTap opens side B.
//! Linux: a pseudo-terminal. The software opens /dev/<name> (e.g. /dev/ttyComTap0), a link
//! to the pty; ComTap holds the other end.
use crate::bridge::Device;
use crate::config::Link;
use anyhow::Result;

#[cfg(windows)]
pub use crate::com0com::Port;
#[cfg(not(windows))]
pub use crate::pty::Port;

/// Checks (Windows: creates or repairs) every link's port before the bridges start.
pub fn prepare(links: &[Link]) -> Result<Vec<Port>> {
    #[cfg(windows)]
    {
        crate::com0com::reconcile(&crate::com0com::Setupc::find()?, links)
    }
    #[cfg(not(windows))]
    {
        crate::pty::prepare(links)
    }
}

/// Opens ComTap's side of the port. Returns the device and a description for the log.
pub fn open(port: &Port) -> std::io::Result<(Box<dyn Device>, String)> {
    #[cfg(windows)]
    {
        windows::open(port)
    }
    #[cfg(not(windows))]
    {
        crate::pty::open(port)
    }
}

/// ComTap's side of the port, for messages: `\\.\CNCB0` on Windows, the link on Linux.
pub fn describe(port: &Port) -> String {
    #[cfg(windows)]
    {
        port.path.clone()
    }
    #[cfg(not(windows))]
    {
        crate::pty::link_path(&port.name).display().to_string()
    }
}

/// Port names to offer in the device form, leaving out `taken` and ones in use on this PC.
pub fn free_ports(taken: &[String], limit: usize) -> Vec<String> {
    #[cfg(windows)]
    {
        crate::com0com::free_ports(taken, limit)
    }
    #[cfg(not(windows))]
    {
        crate::pty::free_ports(taken, limit)
    }
}

/// What the software opens, for messages: "COM5" on Windows, "/dev/ttyComTap0" on Linux.
pub fn software_path(com: &str) -> String {
    #[cfg(windows)]
    {
        com.to_string()
    }
    #[cfg(not(windows))]
    {
        crate::pty::link_path(com).display().to_string()
    }
}

/// Checks a port name from the config and returns it in its standard form.
/// Windows: COM1 to COM255, upper case. Linux: a device name such as ttyComTap0.
pub fn normalize(com: &str) -> Result<String, String> {
    let com = com.trim();
    if cfg!(windows) {
        let up = com.to_ascii_uppercase();
        let n = up.strip_prefix("COM").and_then(|n| n.parse::<u32>().ok());
        if matches!(n, Some(1..=255)) {
            return Ok(up);
        }
        return Err(format!("must look like COM5, got '{com}'"));
    }
    let ok = com.len() <= 32
        && com.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && com
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(com.to_string())
    } else {
        Err(format!(
            "must be a device name such as ttyComTap0 (letters, digits, _ or -), got '{com}'"
        ))
    }
}

#[cfg(windows)]
mod windows {
    use super::Port;
    use crate::bridge::{Device, POLL};
    use std::io::{self, Read, Write};

    /// ComTap's side of a pair. With `open_signal`, its RI pin is on while the software has
    /// the port open.
    struct ComSide {
        port: Box<dyn serialport::SerialPort>,
        open_signal: bool,
    }

    impl Read for ComSide {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.port.read(buf)
        }
    }

    impl Write for ComSide {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.port.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.port.flush()
        }
    }

    impl Device for ComSide {
        fn try_clone_dev(&self) -> io::Result<Box<dyn Device>> {
            Ok(Box::new(ComSide {
                port: self.port.try_clone().map_err(io::Error::other)?,
                open_signal: self.open_signal,
            }))
        }
        fn software_open(&mut self) -> Option<bool> {
            if self.open_signal {
                self.port.read_ring_indicator().ok()
            } else {
                None
            }
        }
    }

    pub fn open(port: &Port) -> io::Result<(Box<dyn Device>, String)> {
        let mut sp = serialport::new(&port.path, 115_200)
            .timeout(POLL)
            .open()
            .map_err(io::Error::other)?;
        // Look like a powered device to the software: it sees DSR, DCD and CTS.
        let _ = sp.write_data_terminal_ready(true);
        let _ = sp.write_request_to_send(true);
        let dev = ComSide {
            port: sp,
            open_signal: port.open_signal,
        };
        Ok((Box::new(dev), port.path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_port_names() {
        if cfg!(windows) {
            assert_eq!(normalize(" com7 ").unwrap(), "COM7");
            assert!(normalize("COM0").is_err());
            assert!(normalize("ttyComTap0").is_err());
        } else {
            assert_eq!(normalize(" ttyComTap0 ").unwrap(), "ttyComTap0");
            assert!(normalize("../etc").is_err());
            assert!(normalize("0tty").is_err());
            assert!(normalize("").is_err());
        }
    }
}
