//! Copies bytes both ways between ComTap's side of a virtual port pair and a TCP device,
//! and keeps reconnecting until told to stop.
use crate::com0com::Port;
use crate::config::Link;
use crate::log;
use crate::status::{Shared, now, set_state};
use crate::traffic::{Dir, Traffic};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);

/// Anything ComTap can bridge to TCP: the virtual COM port, or a socket in tests.
pub trait Device: Read + Write + Send {
    fn try_clone_dev(&self) -> io::Result<Box<dyn Device>>;
    /// Whether the software has the COM port open; None when ComTap can't tell.
    fn software_open(&mut self) -> Option<bool> {
        None
    }
}

/// ComTap's side of a pair. With `open_signal`, its RI pin is on while the software has the port open.
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

impl Device for TcpStream {
    fn try_clone_dev(&self) -> io::Result<Box<dyn Device>> {
        Ok(Box::new(self.try_clone()?))
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

/// A write to the COM side that went nowhere because the software isn't reading.
/// A timed-out serial write reports 0 bytes, which `write_all` turns into WriteZero.
fn is_dropped_write(e: &io::Error) -> bool {
    is_timeout(e) || e.kind() == ErrorKind::WriteZero
}

/// Watches whether the software has the COM port open, and reports changes.
struct OpenWatch {
    name: String,
    com: String,
    st: Shared,
    traffic: Arc<Traffic>,
    last: Option<bool>,
    checked: Option<Instant>,
}

impl OpenWatch {
    fn new(link: &Link, st: &Shared, traffic: &Arc<Traffic>) -> OpenWatch {
        OpenWatch {
            name: link.name.clone(),
            com: link.com.clone(),
            st: st.clone(),
            traffic: traffic.clone(),
            last: None,
            checked: None,
        }
    }

    fn check(&mut self, dev: &mut dyn Device) {
        if self
            .checked
            .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.checked = Some(Instant::now());
        let open = dev.software_open();
        self.st.lock().unwrap().software_open = open;
        if open == self.last {
            return;
        }
        self.last = open;
        let msg = match open {
            Some(true) => format!("{} opened by software", self.com),
            Some(false) => format!(
                "{} is not open in any software; data from the device is dropped until it is",
                self.com
            ),
            None => return,
        };
        log!("[{}] {msg}", self.name);
        self.traffic.event(&msg);
    }
}

/// Sleeps up to `dur`, waking early on stop.
pub fn nap(dur: Duration, stop: &AtomicBool) {
    let end = Instant::now() + dur;
    while !stop.load(Ordering::Relaxed) && Instant::now() < end {
        std::thread::sleep(POLL.min(end.saturating_duration_since(Instant::now())));
    }
}

/// Runs one link for ever: opens the COM side, then keeps the TCP side connected.
pub fn run_link(link: Link, com: Port, stop: Arc<AtomicBool>, st: Shared, traffic: Arc<Traffic>) {
    let device_path = com.path;
    let mut last_err = String::new();
    while !stop.load(Ordering::Relaxed) {
        let opened = serialport::new(&device_path, 115_200).timeout(POLL).open();
        let mut port = match opened {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("cannot open {device_path}: {e}");
                if msg != last_err {
                    log!("[{}] {msg}. Retrying every 5 s.", link.name);
                    traffic.event(&msg);
                    last_err = msg.clone();
                }
                set_state(&st, "com-error", Some(msg));
                nap(Duration::from_secs(5), &stop);
                continue;
            }
        };
        last_err.clear();
        // Look like a powered device to the software: it sees DSR, DCD and CTS.
        let _ = port.write_data_terminal_ready(true);
        let _ = port.write_request_to_send(true);
        log!(
            "[{}] Opened {device_path} (software side {})",
            link.name,
            link.com
        );
        let dev = ComSide {
            port,
            open_signal: com.open_signal,
        };
        if let Err(e) = run_with_device(&link, Box::new(dev), &stop, &st, &traffic) {
            let msg = format!("{device_path} failed: {e}");
            log!("[{}] {msg}. Reopening.", link.name);
            traffic.event(&msg);
            set_state(&st, "com-error", Some(msg));
            nap(Duration::from_secs(1), &stop);
        }
    }
    traffic.event("stopped");
    set_state(&st, "stopped", None);
}

fn connect(addr: &str) -> io::Result<TcpStream> {
    let mut last = io::Error::new(ErrorKind::NotFound, format!("{addr} did not resolve"));
    for sa in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&sa, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn tune(tcp: &TcpStream) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(POLL));
    // Keepalive spots a scanner that lost power or network within about 30 s, even when idle.
    let ka = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(10))
        .with_interval(Duration::from_secs(5));
    let _ = socket2::SockRef::from(tcp).set_tcp_keepalive(&ka);
}

/// Reads and drops whatever the software sends while the device is offline, for `dur`.
/// Returns Err if the COM side itself fails.
fn drain(
    dev: &mut dyn Device,
    dur: Duration,
    stop: &AtomicBool,
    watch: &mut OpenWatch,
) -> io::Result<()> {
    let end = Instant::now() + dur;
    let mut buf = [0u8; 1024];
    while !stop.load(Ordering::Relaxed) && Instant::now() < end {
        watch.check(dev);
        match dev.read(&mut buf) {
            Ok(0) => std::thread::sleep(POLL),
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Keeps `link.tcp` connected and bridged to `dev` until stop (Ok) or the device fails (Err).
pub fn run_with_device(
    link: &Link,
    mut dev: Box<dyn Device>,
    stop: &AtomicBool,
    st: &Shared,
    traffic: &Arc<Traffic>,
) -> io::Result<()> {
    let mut backoff = BACKOFF_START;
    let mut last_err = String::new();
    let mut watch = OpenWatch::new(link, st, traffic);
    watch.check(dev.as_mut());
    while !stop.load(Ordering::Relaxed) {
        set_state(st, "connecting", None);
        let tcp = match connect(&link.tcp) {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("connect {}: {e}", link.tcp);
                if msg != last_err {
                    log!("[{}] {msg}. Retrying.", link.name);
                    traffic.event(&msg);
                    last_err = msg.clone();
                }
                set_state(st, "retrying", Some(msg));
                drain(dev.as_mut(), backoff, stop, &mut watch)?;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        last_err.clear();
        backoff = BACKOFF_START;
        tune(&tcp);
        st.lock().unwrap().connects += 1;
        set_state(st, "connected", None);
        log!("[{}] Connected to {}", link.name, link.tcp);
        traffic.event(&format!("connected to {}", link.tcp));

        let reason = session(link, dev.as_mut(), tcp, stop, st, traffic, &mut watch)?;
        if !stop.load(Ordering::Relaxed) {
            log!("[{}] Disconnected from {}: {reason}", link.name, link.tcp);
            traffic.event(&format!("disconnected: {reason}"));
            set_state(st, "retrying", Some(format!("disconnected: {reason}")));
            drain(dev.as_mut(), BACKOFF_START, stop, &mut watch)?;
        }
    }
    Ok(())
}

/// Bridges one TCP connection. Returns why it ended, or Err if the COM side failed.
fn session(
    link: &Link,
    dev: &mut dyn Device,
    tcp: TcpStream,
    stop: &AtomicBool,
    st: &Shared,
    traffic: &Arc<Traffic>,
    watch: &mut OpenWatch,
) -> io::Result<String> {
    let done = Arc::new(AtomicBool::new(false));
    let dev_failed: Arc<std::sync::Mutex<Option<io::Error>>> = Arc::default();

    // Device (TCP) -> COM, on its own thread.
    let reader = {
        let mut tcp_r = tcp.try_clone()?;
        let mut dev_w = dev.try_clone_dev()?;
        let done = done.clone();
        let dev_failed = dev_failed.clone();
        let st = st.clone();
        let name = link.name.clone();
        let com = link.com.clone();
        let traffic = traffic.clone();
        std::thread::spawn(move || -> String {
            let mut buf = [0u8; 4096];
            let mut dropped = 0u64;
            let reason = loop {
                if done.load(Ordering::Relaxed) {
                    break String::from("stopping");
                }
                match tcp_r.read(&mut buf) {
                    Ok(0) => break String::from("device closed the connection"),
                    Ok(n) => {
                        {
                            let mut s = st.lock().unwrap();
                            s.rx_bytes += n as u64;
                            s.last_rx = Some(now());
                        }
                        traffic.record(Dir::Rx, &buf[..n]);
                        // No software has the port open: nowhere to send it (OpenWatch logs this).
                        if dev_w.software_open() == Some(false) {
                            continue;
                        }
                        match dev_w.write_all(&buf[..n]) {
                            Ok(()) => {}
                            // The software isn't reading: drop rather than stall or reconnect.
                            Err(e) if is_dropped_write(&e) => {
                                dropped += n as u64;
                                if dropped == n as u64 || dropped % 4096 < n as u64 {
                                    log!(
                                        "[{name}] {com} software isn't reading; dropped {dropped} bytes so far"
                                    );
                                }
                            }
                            Err(e) => {
                                *dev_failed.lock().unwrap() = Some(e);
                                break String::from("COM write failed");
                            }
                        }
                    }
                    Err(e) if is_timeout(&e) => {}
                    Err(e) => break format!("{e}"),
                }
            };
            done.store(true, Ordering::Relaxed);
            reason
        })
    };

    // COM -> device (TCP), here.
    let mut tcp_w = tcp;
    let mut buf = [0u8; 4096];
    let mut my_reason = None;
    while !done.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        watch.check(dev);
        match dev.read(&mut buf) {
            Ok(0) => std::thread::sleep(POLL),
            Ok(n) => match tcp_w.write_all(&buf[..n]) {
                Ok(()) => {
                    st.lock().unwrap().tx_bytes += n as u64;
                    traffic.record(Dir::Tx, &buf[..n]);
                }
                Err(e) => {
                    my_reason = Some(format!("send failed: {e}"));
                    break;
                }
            },
            Err(e) if is_timeout(&e) => {}
            Err(e) => {
                *dev_failed.lock().unwrap() = Some(e);
                break;
            }
        }
    }
    done.store(true, Ordering::Relaxed);
    let _ = tcp_w.shutdown(Shutdown::Both);
    let their_reason = reader
        .join()
        .unwrap_or_else(|_| String::from("reader thread panicked"));
    if let Some(e) = dev_failed.lock().unwrap().take() {
        return Err(e);
    }
    Ok(my_reason.unwrap_or(if stop.load(Ordering::Relaxed) {
        String::from("stopping")
    } else {
        their_reason
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Two connected sockets: one stands in for ComTap's COM side, the other for the software.
    fn fake_com() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (b, _) = l.accept().unwrap();
        a.set_read_timeout(Some(POLL)).unwrap();
        b.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (a, b)
    }

    fn read_exact_timeout(s: &mut TcpStream, n: usize) -> Vec<u8> {
        let mut out = vec![0u8; n];
        s.read_exact(&mut out).unwrap();
        out
    }

    #[test]
    fn bridges_both_ways_and_reconnects() {
        let scanner = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = Link {
            name: "test".into(),
            tcp: scanner.local_addr().unwrap().to_string(),
            com: "COM99".into(),
        };
        let (com_side, mut software) = fake_com();
        let stop = Arc::new(AtomicBool::new(false));
        let st: Shared = Arc::default();

        let t = {
            let (link, stop, st) = (link.clone(), stop.clone(), st.clone());
            std::thread::spawn(move || {
                run_with_device(
                    &link,
                    Box::new(com_side),
                    &stop,
                    &st,
                    &Arc::new(Traffic::disabled()),
                )
            })
        };

        // First connection: a barcode reaches the software, and a trigger reaches the scanner.
        let (mut conn, _) = scanner.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        conn.write_all(b"0123456789\r\n").unwrap();
        assert_eq!(read_exact_timeout(&mut software, 12), b"0123456789\r\n");
        software.write_all(b"T\r").unwrap();
        assert_eq!(read_exact_timeout(&mut conn, 2), b"T\r");

        // Scanner drops the connection (power cycle); ComTap reconnects by itself.
        drop(conn);
        let (mut conn2, _) = scanner.accept().unwrap();
        conn2.write_all(b"ABC\r\n").unwrap();
        assert_eq!(read_exact_timeout(&mut software, 5), b"ABC\r\n");

        let s = st.lock().unwrap().clone();
        assert_eq!(s.connects, 2);
        assert_eq!(s.rx_bytes, 17);
        assert_eq!(s.tx_bytes, 2);

        stop.store(true, Ordering::Relaxed);
        t.join().unwrap().unwrap();
    }

    #[test]
    fn retries_until_device_appears() {
        // Reserve a port, then free it, so the first connects fail.
        let addr = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let link = Link {
            name: "late".into(),
            tcp: addr.to_string(),
            com: "COM98".into(),
        };
        let (com_side, mut software) = fake_com();
        let stop = Arc::new(AtomicBool::new(false));
        let st: Shared = Arc::default();
        let t = {
            let (link, stop, st) = (link.clone(), stop.clone(), st.clone());
            std::thread::spawn(move || {
                run_with_device(
                    &link,
                    Box::new(com_side),
                    &stop,
                    &st,
                    &Arc::new(Traffic::disabled()),
                )
            })
        };
        // Windows takes about 2 s to report a refused connection on localhost.
        let end = Instant::now() + Duration::from_secs(10);
        while st.lock().unwrap().state != "retrying" {
            assert!(Instant::now() < end, "never reached retrying");
            std::thread::sleep(Duration::from_millis(50));
        }

        let scanner = TcpListener::bind(addr).unwrap();
        let (mut conn, _) = scanner.accept().unwrap();
        conn.write_all(b"LATE\r\n").unwrap();
        assert_eq!(read_exact_timeout(&mut software, 6), b"LATE\r\n");

        stop.store(true, Ordering::Relaxed);
        t.join().unwrap().unwrap();
    }

    /// A COM side whose software can be open or closed, and can stop reading.
    #[derive(Clone, Default)]
    struct FakeCom {
        open: Arc<AtomicBool>,
        stalled: Arc<AtomicBool>,
        got: Arc<std::sync::Mutex<Vec<u8>>>,
    }

    impl Read for FakeCom {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(POLL);
            Err(ErrorKind::TimedOut.into())
        }
    }

    impl Write for FakeCom {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            assert!(self.open.load(Ordering::Relaxed), "wrote to a closed port");
            if self.stalled.load(Ordering::Relaxed) {
                return Ok(0); // what a timed-out serial write reports
            }
            self.got.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Device for FakeCom {
        fn try_clone_dev(&self) -> io::Result<Box<dyn Device>> {
            Ok(Box::new(self.clone()))
        }
        fn software_open(&mut self) -> Option<bool> {
            Some(self.open.load(Ordering::Relaxed))
        }
    }

    #[test]
    fn closed_or_stalled_software_drops_data_and_stays_connected() {
        let scanner = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = Link {
            name: "idle".into(),
            tcp: scanner.local_addr().unwrap().to_string(),
            com: "COM97".into(),
        };
        let com = FakeCom::default();
        let stop = Arc::new(AtomicBool::new(false));
        let st: Shared = Arc::default();
        let t = {
            let (link, stop, st, com) = (link.clone(), stop.clone(), st.clone(), com.clone());
            std::thread::spawn(move || {
                run_with_device(
                    &link,
                    Box::new(com),
                    &stop,
                    &st,
                    &Arc::new(Traffic::disabled()),
                )
            })
        };
        let (mut conn, _) = scanner.accept().unwrap();
        let wait_rx = |n: u64| {
            let end = Instant::now() + Duration::from_secs(5);
            while st.lock().unwrap().rx_bytes < n {
                assert!(Instant::now() < end, "data never arrived");
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(300));
        };

        // No software has the port open: scans are dropped, nothing is written.
        conn.write_all(b"one\r\n").unwrap();
        wait_rx(5);
        assert_eq!(st.lock().unwrap().software_open, Some(false));

        // Software opens the port but doesn't read: writes go nowhere, still no reconnect.
        com.open.store(true, Ordering::Relaxed);
        com.stalled.store(true, Ordering::Relaxed);
        conn.write_all(b"two\r\n").unwrap();
        wait_rx(10);

        // Software reads again: data flows on the same connection.
        com.stalled.store(false, Ordering::Relaxed);
        conn.write_all(b"three\r\n").unwrap();
        wait_rx(17);
        assert_eq!(com.got.lock().unwrap().as_slice(), b"three\r\n");
        let s = st.lock().unwrap().clone();
        assert_eq!(s.connects, 1, "reconnected: {:?}", s.last_error);
        assert_eq!(s.software_open, Some(true));

        stop.store(true, Ordering::Relaxed);
        t.join().unwrap().unwrap();
    }
}
