//! The interactive screen: double-click comtap.exe (or the desktop shortcut).
//! Anyone can view. Changes need admin, and ComTap offers to reopen itself elevated.
use crate::config::{Config, Link};
use crate::status::StatusFile;
use crate::traffic::{self, Dir};
use crate::{codec, com0com, paths, service, status};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const SPIN: [&str; 4] = ["|", "/", "-", "\\"];
const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, PartialEq)]
enum Fmt {
    Compact,
    HexText,
    Hex,
    Text,
}

impl Fmt {
    fn next(self) -> Fmt {
        match self {
            Fmt::Compact => Fmt::HexText,
            Fmt::HexText => Fmt::Hex,
            Fmt::Hex => Fmt::Text,
            Fmt::Text => Fmt::Compact,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Fmt::Compact => "compact",
            Fmt::HexText => "hex+text",
            Fmt::Hex => "hex",
            Fmt::Text => "text",
        }
    }
}

const F_NAME: usize = 0;
const F_HOST: usize = 1;
const F_PORT: usize = 2;
const F_COM: usize = 3;
const LABELS: [&str; 4] = ["Name", "Scanner IP / host", "TCP port", "COM port"];

struct Form {
    /// None = adding a new link.
    idx: Option<usize>,
    fields: [String; 4],
    focus: usize,
    coms: Vec<String>,
    err: Option<String>,
    test: Option<Receiver<String>>,
    test_msg: Option<String>,
}

enum Mode {
    Main,
    Edit(Form),
    AdminPrompt,
    ServiceMenu(usize),
    ConfirmDelete(usize),
    ConfirmUninstall,
}

struct Busy {
    what: String,
    rx: Receiver<Result<()>>,
    since: Instant,
}

struct App {
    admin: bool,
    cfg: Config,
    cfg_note: Option<String>,
    sel: usize,
    status: Option<StatusFile>,
    svc: String,
    drv: String,
    installed: bool,
    traffic: Vec<traffic::Line>,
    traffic_len: u64,
    traffic_com: String,
    fmt: Fmt,
    scroll: usize,
    mode: Mode,
    msg: String,
    msg_err: bool,
    busy: Option<Busy>,
    quit: bool,
    last_status: Instant,
    last_svc: Instant,
    last_traffic: Instant,
}

pub fn run() -> Result<()> {
    let mut app = App::new();
    let mut term = ratatui::init();
    let r = app.main_loop(&mut term);
    ratatui::restore();
    r
}

fn load_config() -> (Config, Option<String>) {
    let p = paths::installed_config();
    if !p.exists() {
        return (Config { links: vec![] }, Some("No config yet. Press A to add a scanner.".into()));
    }
    match Config::load(&p) {
        Ok(c) => (c, None),
        Err(e) => (Config { links: vec![] }, Some(format!("Config has an error: {e:#}"))),
    }
}

impl App {
    fn new() -> App {
        let (cfg, cfg_note) = load_config();
        let long_ago = Instant::now() - Duration::from_secs(60);
        let mut app = App {
            admin: service::is_admin(),
            cfg,
            cfg_note,
            sel: 0,
            status: None,
            svc: String::new(),
            drv: String::new(),
            installed: false,
            traffic: vec![],
            traffic_len: 0,
            traffic_com: String::new(),
            fmt: Fmt::HexText,
            scroll: 0,
            mode: Mode::Main,
            msg: String::new(),
            msg_err: false,
            busy: None,
            quit: false,
            last_status: long_ago,
            last_svc: long_ago,
            last_traffic: long_ago,
        };
        if !app.admin {
            app.say("View only. Changes need admin: press F2 to reopen as admin.");
        }
        app
    }

    fn say(&mut self, m: impl Into<String>) {
        self.msg = m.into();
        self.msg_err = false;
    }

    fn fail(&mut self, m: impl Into<String>) {
        self.msg = m.into();
        self.msg_err = true;
    }

    fn main_loop(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.refresh();
            term.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(150))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind != KeyEventKind::Release {
                        self.key(k);
                    }
                }
            }
        }
        Ok(())
    }

    // ---------- data ----------

    fn refresh(&mut self) {
        if let Some(b) = &self.busy {
            if let Ok(r) = b.rx.try_recv() {
                let what = b.what.clone();
                self.busy = None;
                match r {
                    Ok(()) => {
                        let last = crate::logger::recent().last().cloned().unwrap_or_default();
                        self.say(format!("{what}: done. {last}"));
                    }
                    Err(e) => self.fail(format!("{what} failed: {e:#}")),
                }
                let (cfg, note) = load_config();
                self.cfg = cfg;
                self.cfg_note = note;
                self.sel = self.sel.min(self.cfg.links.len().saturating_sub(1));
                self.last_svc = Instant::now() - Duration::from_secs(60);
            }
        }
        if let Mode::Edit(f) = &mut self.mode {
            if let Some(rx) = &f.test {
                if let Ok(m) = rx.try_recv() {
                    f.test_msg = Some(m);
                    f.test = None;
                }
            }
        }
        if self.last_svc.elapsed() > Duration::from_secs(2) {
            self.svc = service::state_text();
            self.drv = service::driver_state_text();
            self.installed = self.svc != "not installed";
            self.last_svc = Instant::now();
        }
        if self.last_status.elapsed() > Duration::from_secs(1) {
            self.status = status::read();
            self.last_status = Instant::now();
        }
        if self.last_traffic.elapsed() > Duration::from_millis(250) {
            self.last_traffic = Instant::now();
            let com = self.cfg.links.get(self.sel).map(|l| l.com.clone()).unwrap_or_default();
            let day = chrono::Local::now().format("%Y-%m-%d").to_string();
            let len = std::fs::metadata(traffic::file_for(&paths::traffic_dir(), &com, &day))
                .map(|m| m.len())
                .unwrap_or(0);
            if com != self.traffic_com || len != self.traffic_len {
                let (lines, len) = traffic::tail(&paths::traffic_dir(), &com, TAIL_BYTES);
                self.traffic = lines;
                self.traffic_len = len;
                self.traffic_com = com;
            }
        }
    }

    /// Live status for a link, if the service wrote it in the last 10 s.
    fn link_status(&self, l: &Link) -> Option<&status::LinkStatus> {
        let st = self.status.as_ref()?;
        if chrono::Local::now().timestamp() - st.written_unix > 10 {
            return None;
        }
        st.links.iter().find(|s| s.com == l.com)
    }

    fn start_busy(&mut self, what: &str, job: impl FnOnce() -> Result<()> + Send + 'static) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
        self.busy = Some(Busy { what: what.to_string(), rx, since: Instant::now() });
        self.mode = Mode::Main;
    }

    fn apply(&mut self, what: &str, new: Config) {
        self.start_busy(what, move || crate::apply_config(&new));
    }

    // ---------- keys ----------

    fn needs_admin(&mut self) -> bool {
        if self.admin {
            return false;
        }
        self.mode = Mode::AdminPrompt;
        true
    }

    fn key(&mut self, k: KeyEvent) {
        if self.busy.is_some() {
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Main) {
            Mode::Main => self.key_main(k),
            Mode::Edit(f) => self.key_edit(f, k),
            Mode::AdminPrompt => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => match crate::relaunch_as_admin() {
                    Ok(()) => self.quit = true,
                    Err(e) => self.fail(format!("Still view only: {e:#}")),
                },
                _ => self.say("Still view only."),
            },
            Mode::ConfirmDelete(i) => {
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    let mut new = self.cfg.clone();
                    new.links.remove(i);
                    self.apply("Delete", new);
                } else {
                    self.say("Not deleted.");
                }
            }
            Mode::ConfirmUninstall => {
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    self.start_busy("Uninstall", || crate::uninstall(false, false));
                } else {
                    self.say("Not uninstalled.");
                }
            }
            Mode::ServiceMenu(i) => self.key_service_menu(i, k),
        }
    }

    fn key_main(&mut self, k: KeyEvent) {
        let n = self.cfg.links.len();
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up if self.sel > 0 => {
                self.sel -= 1;
                self.scroll = 0;
            }
            KeyCode::Down if self.sel + 1 < n => {
                self.sel += 1;
                self.scroll = 0;
            }
            KeyCode::F(2) if !self.admin => self.mode = Mode::AdminPrompt,
            KeyCode::F(8) => self.fmt = self.fmt.next(),
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => self.scroll = 0,
            KeyCode::Home => self.scroll = usize::MAX / 2,
            KeyCode::Char('l') | KeyCode::Char('L') => {
                let _ = std::process::Command::new("explorer").arg(paths::data_dir()).spawn();
            }
            KeyCode::Enter if n > 0 => {
                if !self.needs_admin() {
                    self.mode = Mode::Edit(self.form(Some(self.sel)));
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if !self.needs_admin() {
                    self.mode = Mode::Edit(self.form(None));
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete if n > 0 => {
                if !self.needs_admin() {
                    self.mode = Mode::ConfirmDelete(self.sel);
                }
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if !self.needs_admin() {
                    self.mode = Mode::ServiceMenu(0);
                }
            }
            _ => {}
        }
    }

    fn service_items(&self) -> Vec<&'static str> {
        if !self.installed {
            vec!["Install (com0com, ports, service, desktop shortcut)"]
        } else {
            let run = if self.svc == "Running" { "Stop service" } else { "Start service" };
            vec!["Restart service", run, "Repair virtual ports", "Reinstall / update exe", "Uninstall"]
        }
    }

    fn key_service_menu(&mut self, i: usize, k: KeyEvent) {
        let items = self.service_items();
        match k.code {
            KeyCode::Up => self.mode = Mode::ServiceMenu(i.saturating_sub(1)),
            KeyCode::Down => self.mode = Mode::ServiceMenu((i + 1).min(items.len() - 1)),
            KeyCode::Enter => {
                let item = items[i];
                let cfg_path = paths::installed_config();
                match item {
                    _ if item.starts_with("Install") || item.starts_with("Reinstall") => {
                        if self.cfg.links.is_empty() {
                            self.fail("Add a scanner first (A).");
                        } else {
                            self.start_busy("Install", move || crate::install(Some(cfg_path)));
                        }
                    }
                    "Restart service" => self.start_busy("Restart", crate::restart),
                    "Stop service" => self.start_busy("Stop", service::stop),
                    "Start service" => self.start_busy("Start", service::start),
                    "Repair virtual ports" => {
                        let links = self.cfg.links.clone();
                        self.start_busy("Repair ports", move || {
                            com0com::reconcile(&com0com::Setupc::find()?, &links).map(|_| ())
                        });
                    }
                    "Uninstall" => self.mode = Mode::ConfirmUninstall,
                    _ => {}
                }
            }
            KeyCode::Esc => {}
            _ => self.mode = Mode::ServiceMenu(i),
        }
    }

    fn form(&self, idx: Option<usize>) -> Form {
        let (name, host, port, com) = match idx.and_then(|i| self.cfg.links.get(i)) {
            Some(l) => {
                let (h, p) = l.tcp.rsplit_once(':').unwrap_or((&l.tcp, ""));
                (l.name.clone(), h.to_string(), p.to_string(), l.com.clone())
            }
            None => (format!("Scanner {}", self.cfg.links.len() + 1), String::new(), "51236".into(), String::new()),
        };
        // COM ports offered: this link's own, plus any not used by Windows or another link.
        let in_use: Vec<String> = serialport::available_ports()
            .map(|v| v.into_iter().map(|p| p.port_name.to_ascii_uppercase()).collect())
            .unwrap_or_default();
        let mut coms: Vec<String> = (1..=64)
            .map(|n| format!("COM{n}"))
            .filter(|c| *c == com || (!in_use.contains(c) && !self.cfg.links.iter().any(|l| &l.com == c)))
            .collect();
        if !com.is_empty() && !coms.contains(&com) {
            coms.insert(0, com.clone());
        }
        let com = if com.is_empty() { coms.first().cloned().unwrap_or_else(|| "COM5".into()) } else { com };
        Form { idx, fields: [name, host, port, com], focus: if idx.is_none() { F_HOST } else { F_NAME }, coms, err: None, test: None, test_msg: None }
    }

    fn key_edit(&mut self, mut f: Form, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.say("Not saved.");
                return;
            }
            KeyCode::Up | KeyCode::BackTab => f.focus = (f.focus + 3) % 4,
            KeyCode::Down | KeyCode::Tab => f.focus = (f.focus + 1) % 4,
            KeyCode::Left | KeyCode::Right if f.focus == F_COM && !f.coms.is_empty() => {
                let pos = f.coms.iter().position(|c| *c == f.fields[F_COM]).unwrap_or(0);
                let n = f.coms.len();
                let next = if k.code == KeyCode::Right { (pos + 1) % n } else { (pos + n - 1) % n };
                f.fields[F_COM] = f.coms[next].clone();
            }
            KeyCode::Backspace => {
                f.fields[f.focus].pop();
            }
            KeyCode::Char(c) => {
                let ok = match f.focus {
                    F_PORT => c.is_ascii_digit(),
                    F_HOST => !c.is_whitespace(),
                    F_COM => c.is_ascii_alphanumeric(),
                    _ => true,
                };
                if ok {
                    f.fields[f.focus].push(c);
                }
            }
            KeyCode::F(5) => {
                let addr = format!("{}:{}", f.fields[F_HOST].trim(), f.fields[F_PORT].trim());
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(test_connect(&addr));
                });
                f.test = Some(rx);
                f.test_msg = Some("Testing...".into());
            }
            KeyCode::Enter => {
                let link = Link {
                    name: f.fields[F_NAME].trim().to_string(),
                    tcp: format!("{}:{}", f.fields[F_HOST].trim(), f.fields[F_PORT].trim()),
                    com: f.fields[F_COM].trim().to_string(),
                };
                let mut new = self.cfg.clone();
                match f.idx {
                    Some(i) => new.links[i] = link,
                    None => new.links.push(link),
                }
                match new.validate() {
                    Ok(()) => {
                        if f.idx.is_none() {
                            self.sel = new.links.len() - 1;
                        }
                        let what = if self.installed { "Save and apply" } else { "Save" };
                        self.apply(what, new);
                        return;
                    }
                    Err(e) => f.err = Some(format!("{e:#}")),
                }
            }
            _ => {}
        }
        self.mode = Mode::Edit(f);
    }

    // ---------- drawing ----------

    fn draw(&self, fr: &mut Frame) {
        let links_h = (self.cfg.links.len().max(1) as u16 + 3).min(12);
        let [head, svc, links, tr, msg, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(links_h),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(fr.area());

        // Header
        let right = if self.admin {
            Span::styled(" ADMIN ", Style::new().fg(Color::Black).bg(Color::Green))
        } else {
            Span::styled(" VIEW ONLY · F2 admin ", Style::new().fg(Color::Black).bg(Color::Yellow))
        };
        let title = format!(" ComTap {}  TCP devices as COM ports", env!("CARGO_PKG_VERSION"));
        let [hl, hr] =
            Layout::horizontal([Constraint::Min(10), Constraint::Length(right.width() as u16)]).areas(head);
        fr.render_widget(Paragraph::new(title.bold()), hl);
        fr.render_widget(Paragraph::new(Line::from(right)), hr);

        // Service + driver
        let dot = |s: &str| {
            let c = match s {
                "Running" => Color::Green,
                "not installed" => Color::Yellow,
                _ => Color::Red,
            };
            Span::styled("● ", Style::new().fg(c))
        };
        let files = if com0com::has_embedded() {
            "files inside exe".to_string()
        } else if com0com::locate().is_ok() {
            "files found".to_string()
        } else {
            "files MISSING (see vendor/com0com/README.md)".to_string()
        };
        let svc_lines = vec![
            Line::from(vec![
                "Service  ".into(),
                dot(&self.svc),
                self.svc.clone().into(),
                if self.installed { "  · starts on boot".dark_gray() } else { "  · press S to install".dark_gray() },
            ]),
            Line::from(vec!["Driver   ".into(), dot(&self.drv), format!("com0com {}  · {files}", self.drv).into()]),
        ];
        fr.render_widget(Paragraph::new(svc_lines).block(Block::bordered().title(" Status ")), svc);

        // Links
        let mut rows = Vec::new();
        if self.cfg.links.is_empty() {
            rows.push(Line::from(self.cfg_note.clone().unwrap_or_else(|| "No scanners yet. Press A to add one.".into())).yellow());
        }
        for (i, l) in self.cfg.links.iter().enumerate() {
            let st = self.link_status(l);
            let (state, color) = match st.map(|s| s.state.as_str()) {
                Some("connected") => ("connected", Color::Green),
                Some(s @ ("retrying" | "connecting" | "starting")) => (s, Color::Yellow),
                Some(s) => (s, Color::Red),
                None => ("service not running", Color::DarkGray),
            };
            let last = st.and_then(|s| s.last_rx.clone()).map(|t| format!("  last data {}", &t[11..])).unwrap_or_default();
            let marker = if i == self.sel { "▶ " } else { "  " };
            let mut row = Line::from(vec![
                marker.into(),
                Span::styled("● ", Style::new().fg(color)),
                format!("{:<18} ", trunc(&l.name, 18)).bold(),
                format!("{:<6} ⇄ {:<22} ", l.com, l.tcp).into(),
                Span::styled(state.to_string(), Style::new().fg(color)),
                last.dark_gray(),
            ]);
            if i == self.sel {
                row = row.style(Style::new().add_modifier(Modifier::REVERSED));
            }
            rows.push(row);
            if i == self.sel {
                if let Some(e) = st.and_then(|s| s.last_error.clone()) {
                    rows.push(Line::from(format!("      └ {e}")).red());
                }
            }
        }
        fr.render_widget(Paragraph::new(rows).block(Block::bordered().title(" Scanners ")), links);

        // Traffic
        self.draw_traffic(fr, tr);

        // Message + footer
        let m = if let Some(b) = &self.busy {
            let i = (b.since.elapsed().as_millis() / 150) as usize % 4;
            Line::from(format!(" {} {}... (please wait)", SPIN[i], b.what)).cyan()
        } else if self.msg_err {
            Line::from(format!(" {}", self.msg)).red()
        } else {
            Line::from(format!(" {}", self.msg)).dark_gray()
        };
        fr.render_widget(Paragraph::new(m), msg);
        let keys = " ↑↓ select · Enter edit · A add · D delete · S service · F8 format · PgUp/PgDn scroll · L folder · Q quit";
        fr.render_widget(Paragraph::new(keys.reversed()), foot);

        // Popups
        match &self.mode {
            Mode::Main => {}
            Mode::Edit(f) => self.draw_form(fr, f),
            Mode::AdminPrompt => popup(
                fr,
                " Needs admin ",
                vec![
                    "Changing settings or the service needs administrator rights.".into(),
                    "".into(),
                    "Reopen ComTap as admin?  Y = yes (UAC prompt)   N = no".bold().into(),
                ],
            ),
            Mode::ConfirmDelete(i) => popup(
                fr,
                " Delete scanner ",
                vec![
                    format!("Delete '{}' and its port {}?", self.cfg.links[*i].name, self.cfg.links[*i].com).into(),
                    "".into(),
                    "Y = delete   N = keep".bold().into(),
                ],
            ),
            Mode::ConfirmUninstall => popup(
                fr,
                " Uninstall ",
                vec![
                    "Remove the service, its virtual ports and the desktop shortcut?".into(),
                    "Config, logs and the com0com driver stay.".into(),
                    "".into(),
                    "Y = uninstall   N = cancel".bold().into(),
                ],
            ),
            Mode::ServiceMenu(sel) => {
                let mut lines: Vec<Line> = self
                    .service_items()
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let l = Line::from(format!(" {s} "));
                        if i == *sel { l.reversed() } else { l }
                    })
                    .collect();
                lines.push("".into());
                lines.push("↑↓ choose · Enter run · Esc close".dark_gray().into());
                popup(fr, " Service ", lines);
            }
        }
    }

    fn draw_traffic(&self, fr: &mut Frame, area: Rect) {
        let name = self.cfg.links.get(self.sel).map(|l| l.name.as_str()).unwrap_or("-");
        let inner_w = area.width.saturating_sub(2) as usize;
        let inner_h = area.height.saturating_sub(2) as usize;
        let mut rows: Vec<Line> = Vec::new();
        // Hex dump style: 16 bytes a row if there's room for the text beside it, else 8.
        let per_row = if inner_w >= 12 + 16 * 3 + 2 + 24 { 16 } else { 8 };
        for l in &self.traffic {
            match l {
                traffic::Line::Event { time, msg } => {
                    rows.push(Line::from(format!("{} -- {msg}", &time[..8])).cyan());
                }
                traffic::Line::Data { time, dir, bytes } => {
                    let (tag, color) = match dir {
                        Dir::Rx => ("RX", Color::Green),
                        Dir::Tx => ("TX", Color::Yellow),
                    };
                    let head = |first: bool| {
                        if first {
                            vec![format!("{} ", &time[..8]).dark_gray(), Span::styled(format!("{tag} "), Style::new().fg(color))]
                        } else {
                            vec![Span::raw(" ".repeat(12))]
                        }
                    };
                    match self.fmt {
                        Fmt::Compact => {
                            let hex = codec::hex_string(bytes);
                            let hw = (inner_w.saturating_sub(12)) / 2;
                            let mut s = head(true);
                            s.push(format!("{:<hw$} ", trunc(&hex, hw)).into());
                            s.push(Span::styled(codec::text_string(bytes), Style::new().fg(color)));
                            rows.push(Line::from(s));
                        }
                        Fmt::Text => {
                            let mut s = head(true);
                            s.push(Span::styled(codec::text_string(bytes), Style::new().fg(color)));
                            rows.push(Line::from(s));
                        }
                        Fmt::Hex | Fmt::HexText => {
                            for (i, chunk) in bytes.chunks(if self.fmt == Fmt::Hex { per_row * 2 } else { per_row }).enumerate() {
                                let mut s = head(i == 0);
                                let hex = codec::hex_string(chunk);
                                if self.fmt == Fmt::HexText {
                                    s.push(format!("{:<w$}  ", hex, w = per_row * 3 - 1).into());
                                    s.push(Span::styled(codec::text_string(chunk), Style::new().fg(color)));
                                } else {
                                    s.push(hex.into());
                                }
                                rows.push(Line::from(s));
                            }
                        }
                    }
                }
            }
        }
        if rows.is_empty() {
            rows.push("No traffic today yet. Scan a barcode and it appears here.".dark_gray().into());
        }
        let max_scroll = rows.len().saturating_sub(inner_h);
        let scroll = self.scroll.min(max_scroll);
        let end = rows.len() - scroll;
        let start = end.saturating_sub(inner_h);
        let pos = if scroll == 0 { "following".to_string() } else { format!("scrolled up {scroll} · End to follow") };
        let title = format!(" {name} · traffic · {} (F8) · {pos} ", self.fmt.name());
        let shown: Vec<Line> = rows[start..end].to_vec();
        fr.render_widget(Paragraph::new(shown).block(Block::bordered().title(title)), area);
    }

    fn draw_form(&self, fr: &mut Frame, f: &Form) {
        let mut lines: Vec<Line> = Vec::new();
        for (i, label) in LABELS.iter().enumerate() {
            let focused = i == f.focus;
            let mut val = f.fields[i].clone();
            if focused {
                val.push('_');
            }
            let hint = if i == F_COM { "  ←→ pick a free port".dark_gray() } else { "".into() };
            let v = if focused { Span::styled(format!(" {val:<24}"), Style::new().reversed()) } else { format!(" {val:<24}").into() };
            lines.push(Line::from(vec![format!("{label:>18}:").into(), v, hint]));
        }
        lines.push("".into());
        if let Some(t) = &f.test_msg {
            let l = Line::from(format!("Test: {t}"));
            lines.push(if t.starts_with("OK") { l.green() } else if t.starts_with("Testing") { l.cyan() } else { l.red() });
            if !t.starts_with("OK") && !t.starts_with("Testing") && self.svc == "Running" {
                lines.push("(Some scanners allow only one connection. If ComTap is connected, a test can fail.)".dark_gray().into());
            }
        }
        if let Some(e) = &f.err {
            lines.push(Line::from(e.clone()).red());
        }
        lines.push("".into());
        lines.push("Tab/↑↓ field · F5 test connection · Enter save · Esc cancel".dark_gray().into());
        let title = if f.idx.is_some() { " Edit scanner " } else { " Add scanner " };
        popup(fr, title, lines);
    }
}

fn test_connect(addr: &str) -> String {
    let t0 = Instant::now();
    let addrs = match addr.to_socket_addrs() {
        Ok(a) => a.collect::<Vec<_>>(),
        Err(e) => return format!("cannot resolve {addr}: {e}"),
    };
    let mut last = format!("{addr} did not resolve");
    for a in addrs {
        match TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
            Ok(_) => return format!("OK, {addr} answered in {} ms", t0.elapsed().as_millis()),
            Err(e) => last = format!("{addr}: {e}"),
        }
    }
    last
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn popup(fr: &mut Frame, title: &str, lines: Vec<Line>) {
    let a = fr.area();
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16 + 4).max(30).min(a.width);
    let h = (lines.len() as u16 + 2).min(a.height);
    let r = Rect { x: a.x + (a.width - w) / 2, y: a.y + (a.height - h) / 2, width: w, height: h };
    fr.render_widget(Clear, r);
    fr.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(Block::bordered().title(title.to_string()).bold()),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sample() -> App {
        let mut app = App::new();
        app.cfg = Config {
            links: vec![
                Link { name: "Line 1 scanner".into(), tcp: "192.168.1.50:51236".into(), com: "COM5".into() },
                Link { name: "Line 2 scanner".into(), tcp: "192.168.1.51:51236".into(), com: "COM6".into() },
            ],
        };
        app.svc = "Running".into();
        app.drv = "Running".into();
        app.installed = true;
        app.traffic = vec![
            traffic::parse_line("2026-11-16 12:01:20.000 -- connected to 192.168.1.50:51236").unwrap(),
            traffic::parse_line("2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | x").unwrap(),
            traffic::parse_line("2026-11-16 12:01:40.002 TX 54 0D | T<CR>").unwrap(),
        ];
        app
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| app.draw(f)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn draws_every_mode_at_any_size() {
        let mut app = sample();
        for (w, h) in [(120, 32), (80, 24), (40, 12), (20, 6)] {
            for fmt in [Fmt::Compact, Fmt::HexText, Fmt::Hex, Fmt::Text] {
                app.fmt = fmt;
                render(&app, w, h);
            }
            for m in 0..5 {
                app.mode = match m {
                    0 => Mode::Edit(app.form(Some(0))),
                    1 => Mode::AdminPrompt,
                    2 => Mode::ServiceMenu(1),
                    3 => Mode::ConfirmDelete(1),
                    _ => Mode::ConfirmUninstall,
                };
                render(&app, w, h);
            }
            app.mode = Mode::Main;
        }
        app.fmt = Fmt::HexText;
        println!("{}", render(&app, 110, 22));
        app.mode = Mode::Edit(app.form(None));
        println!("{}", render(&app, 110, 22));
    }
}
