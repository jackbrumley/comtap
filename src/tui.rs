//! The interactive screen, styled like RawTap: double-click comtap.exe or the desktop shortcut.
//!
//! New user:  Welcome -> device form -> install checklist -> main screen.
//! Installed: main screen. Anyone can look; changes reopen ComTap as admin.
use crate::config::{Config, Link};
use crate::setup::{self, Plan, St};
use crate::status::StatusFile;
use crate::traffic::{self, Dir};
use crate::{codec, com0com, paths, service, status};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const ACCENT: Color = Color::Cyan;
const DIM: Style = Style::new().fg(Color::DarkGray);
const SPIN: [&str; 4] = ["|", "/", "-", "\\"];
const TAIL_BYTES: u64 = 256 * 1024;
// Glyphs that the default console font (Consolas) has. ▶ ⇄ ✔ ✘ show as boxes there.
const MARK: &str = "►";
const LEFT: &str = "◄";
const BOTH: &str = "↔";
const OK: &str = "√";
const BAD: &str = "×";

fn badge(t: &str, bg: Color) -> Span<'static> {
    Span::styled(format!(" {t} "), Style::new().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD))
}

fn title(t: &str) -> Line<'static> {
    Line::from(vec![badge("ComTap", ACCENT), Span::raw(format!(" {t} "))])
}

fn hints(t: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {t} "), DIM))
}

// ---------------------------------------------------------------- state

#[derive(Clone, Copy, PartialEq, Debug)]
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
const LABELS: [&str; 4] = ["Name", "Device IP", "TCP port", "COM port"];
const HELP: [&str; 4] = [
    "A label for this device, shown on this screen and in the logs. For example: Dock 3 scanner.",
    "The device's IP address or network name. ComTap connects to it as a TCP client.",
    "The TCP port the device sends its data on. You find it in the device's network settings.",
    "The COM port your software will open. Only free ports are listed. Use ←→ to change it.",
];

struct Form {
    /// None = a new device.
    idx: Option<usize>,
    fields: [String; 4],
    focus: usize,
    coms: Vec<String>,
    /// True when ComTap isn't installed yet: Enter installs.
    install: bool,
    /// Something ComTap changed for you, e.g. a reserved COM port swapped for a free one.
    note: Option<String>,
    err: Option<String>,
    test: Option<Receiver<(bool, String)>>,
    test_msg: Option<(bool, String)>,
}

/// Where to go when a progress screen is closed.
enum After {
    Main,
    /// Back to the form with these values, if the plan failed.
    FormOnFail { idx: Option<usize>, link: Link, install: bool },
}

enum Msg {
    Step(usize, St),
    Done(Result<(), String>),
}

struct Progress {
    plan: &'static Plan,
    states: Vec<St>,
    rx: Receiver<Msg>,
    result: Option<Result<(), String>>,
    since: Instant,
    after: After,
    success_note: String,
}

enum Screen {
    Welcome,
    Form(Form),
    Progress(Progress),
    Main,
}

enum Overlay {
    None,
    Menu { sel: usize, filter: String },
    Admin { open: String },
    ConfirmDelete(usize),
    ConfirmUninstall,
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Add,
    Edit,
    Delete,
    Install,
    Restart,
    Stop,
    Start,
    Repair,
    Uninstall,
    Folder,
    Format,
    Quit,
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
    screen: Screen,
    overlay: Overlay,
    msg: Option<(bool, String)>,
    quit: bool,
    last_status: Instant,
    last_svc: Instant,
    last_traffic: Instant,
}

pub fn run(open: Option<String>) -> Result<()> {
    let mut app = App::new();
    app.screen = app.initial_screen(open.as_deref());
    let mut term = ratatui::init();
    let r = app.main_loop(&mut term);
    ratatui::restore();
    r
}

fn load_config() -> (Config, Option<String>) {
    let p = paths::installed_config();
    if !p.exists() {
        return (Config { links: vec![] }, None);
    }
    match Config::load(&p) {
        Ok(c) => (c, None),
        Err(e) => (Config { links: vec![] }, Some(format!("The config file has an error: {e:#}"))),
    }
}

impl App {
    fn new() -> App {
        let (cfg, cfg_note) = load_config();
        let long_ago = Instant::now() - Duration::from_secs(60);
        let svc = service::state_text();
        App {
            admin: service::is_admin(),
            cfg,
            cfg_note,
            sel: 0,
            status: None,
            installed: svc != "not installed",
            svc,
            drv: service::driver_state_text(),
            traffic: vec![],
            traffic_len: 0,
            traffic_com: String::new(),
            fmt: Fmt::HexText,
            scroll: 0,
            screen: Screen::Main,
            overlay: Overlay::None,
            msg: None,
            quit: false,
            last_status: long_ago,
            last_svc: Instant::now(),
            last_traffic: long_ago,
        }
    }

    fn initial_screen(&mut self, open: Option<&str>) -> Screen {
        match open {
            Some("add") if self.admin => return Screen::Form(self.form(None)),
            Some(o) if o.starts_with("edit:") && self.admin => {
                let i = o[5..].parse::<usize>().unwrap_or(0);
                if i < self.cfg.links.len() {
                    self.sel = i;
                    return Screen::Form(self.form(Some(i)));
                }
            }
            Some("menu") => self.overlay = Overlay::Menu { sel: 0, filter: String::new() },
            Some("setup") if self.admin => return self.setup_form(),
            _ => {}
        }
        if !self.installed && open.is_none() { Screen::Welcome } else { Screen::Main }
    }

    /// The first device form of a new setup.
    fn setup_form(&self) -> Screen {
        Screen::Form(self.form(if self.cfg.links.is_empty() { None } else { Some(0) }))
    }

    fn say(&mut self, m: impl Into<String>) {
        self.msg = Some((false, m.into()));
    }

    fn fail(&mut self, m: impl Into<String>) {
        self.msg = Some((true, m.into()));
    }

    fn main_loop(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.refresh();
            term.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(120))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind != KeyEventKind::Release {
                        self.key(k);
                    }
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- data

    fn reload(&mut self) {
        let (cfg, note) = load_config();
        self.cfg = cfg;
        self.cfg_note = note;
        self.sel = self.sel.min(self.cfg.links.len().saturating_sub(1));
        self.svc = service::state_text();
        self.drv = service::driver_state_text();
        self.installed = self.svc != "not installed";
        self.last_svc = Instant::now();
        self.traffic_com.clear();
    }

    fn refresh(&mut self) {
        if let Screen::Progress(p) = &mut self.screen {
            while let Ok(m) = p.rx.try_recv() {
                match m {
                    Msg::Step(i, s) => p.states[i] = s,
                    Msg::Done(r) => p.result = Some(r),
                }
            }
        }
        if let Screen::Form(f) = &mut self.screen {
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

    fn start_plan(
        &mut self,
        plan: &'static Plan,
        after: After,
        success_note: String,
        job: impl FnOnce(setup::Rep) -> Result<()> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let mut rep = move |i: usize, s: St| {
                let _ = tx2.send(Msg::Step(i, s));
            };
            let r = job(&mut rep).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(r));
        });
        self.overlay = Overlay::None;
        self.screen = Screen::Progress(Progress {
            plan,
            states: vec![St::Pending; plan.steps.len()],
            rx,
            result: None,
            since: Instant::now(),
            after,
            success_note,
        });
    }

    // ---------------------------------------------------------------- keys

    fn key(&mut self, k: KeyEvent) {
        let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(event::KeyModifiers::CONTROL);
        match std::mem::replace(&mut self.screen, Screen::Main) {
            Screen::Welcome => self.key_welcome(k),
            Screen::Form(f) => self.key_form(f, k),
            Screen::Progress(p) => self.key_progress(p, k),
            Screen::Main => {
                if ctrl_c {
                    self.quit = true;
                } else {
                    self.key_main(k);
                }
            }
        }
    }

    fn key_welcome(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter if self.admin => self.screen = self.setup_form(),
            KeyCode::Enter => match setup::relaunch_as_admin("setup") {
                Ok(()) => self.quit = true,
                Err(e) => {
                    self.fail(format!("{e:#}"));
                    self.screen = Screen::Welcome;
                }
            },
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            _ => self.screen = Screen::Welcome,
        }
    }

    fn key_progress(&mut self, p: Progress, k: KeyEvent) {
        let Some(result) = &p.result else {
            self.screen = Screen::Progress(p); // still working: ignore keys
            return;
        };
        if !matches!(k.code, KeyCode::Enter | KeyCode::Esc) {
            self.screen = Screen::Progress(p);
            return;
        }
        self.reload();
        match (result, p.after) {
            (Err(e), After::FormOnFail { idx, link, install }) => {
                let mut f = self.form(idx);
                f.fields[F_NAME] = link.name.clone();
                f.fields[F_HOST] = host_of(&link.tcp);
                f.fields[F_PORT] = port_of(&link.tcp);
                if f.coms.contains(&link.com) {
                    f.fields[F_COM] = link.com.clone();
                    f.note = None;
                } else if f.note.is_none() {
                    f.note = Some(format!(
                        "{} is reserved by another device on this PC, so I picked {}. Use ←→ to choose another.",
                        link.com, f.fields[F_COM]
                    ));
                }
                f.install = install && !self.installed;
                f.err = Some(e.clone());
                self.screen = Screen::Form(f);
            }
            (Err(e), _) => {
                self.fail(format!("{} stopped: {e}", p.plan.title));
                self.screen = Screen::Main;
            }
            (Ok(()), _) => {
                self.say(format!("{}: done.", p.plan.title));
                self.screen = Screen::Main;
            }
        }
    }

    fn menu_items(&self) -> Vec<(Act, String, &'static str, bool)> {
        // (action, label, key hint, needs admin)
        let mut v = vec![(Act::Add, "Add a device".to_string(), "A", true)];
        if !self.cfg.links.is_empty() {
            v.push((Act::Edit, "Edit this device".into(), "Enter", true));
            v.push((Act::Delete, "Delete this device".into(), "D", true));
        }
        if !self.installed {
            v.push((Act::Install, "Install ComTap".into(), "", true));
        } else {
            v.push((Act::Restart, "Restart the service".into(), "", true));
            if self.svc == "Running" {
                v.push((Act::Stop, "Stop the service".into(), "", true));
            } else {
                v.push((Act::Start, "Start the service".into(), "", true));
            }
            v.push((Act::Repair, "Repair virtual COM ports".into(), "", true));
            v.push((Act::Install, "Reinstall / update ComTap".into(), "", true));
            v.push((Act::Uninstall, "Uninstall ComTap".into(), "", true));
        }
        v.push((Act::Folder, "Open the log folder".into(), "L", false));
        v.push((Act::Format, format!("Traffic format: {}", self.fmt.name()), "F8", false));
        v.push((Act::Quit, "Quit".into(), "Q", false));
        v
    }

    fn filtered_menu(&self, filter: &str) -> Vec<(Act, String, &'static str, bool)> {
        let f = filter.to_lowercase();
        self.menu_items().into_iter().filter(|i| i.1.to_lowercase().contains(&f)).collect()
    }

    fn open_name(&self, act: Act) -> String {
        match act {
            Act::Add => "add".into(),
            Act::Edit => format!("edit:{}", self.sel),
            _ => "menu".into(),
        }
    }

    fn run_act(&mut self, act: Act, needs_admin: bool) {
        if needs_admin && !self.admin {
            self.overlay = Overlay::Admin { open: self.open_name(act) };
            return;
        }
        self.overlay = Overlay::None;
        match act {
            Act::Add => self.screen = Screen::Form(self.form(None)),
            Act::Edit if !self.cfg.links.is_empty() => self.screen = Screen::Form(self.form(Some(self.sel))),
            Act::Delete if !self.cfg.links.is_empty() => self.overlay = Overlay::ConfirmDelete(self.sel),
            Act::Install => {
                if self.cfg.links.is_empty() {
                    self.screen = Screen::Form(self.form(None));
                } else {
                    let cfg = self.cfg.clone();
                    let note = ports_note(&cfg.links);
                    self.start_plan(&setup::INSTALL, After::Main, note, move |r| setup::install(&cfg, r));
                }
            }
            Act::Restart | Act::Start => self.start_plan(&setup::RESTART, After::Main, String::new(), setup::restart),
            Act::Stop => self.start_plan(&setup::STOP, After::Main, String::new(), setup::stop),
            Act::Repair => self.start_plan(&setup::REPAIR, After::Main, String::new(), setup::repair),
            Act::Uninstall => self.overlay = Overlay::ConfirmUninstall,
            Act::Folder => {
                let _ = std::process::Command::new("explorer").arg(paths::data_dir()).spawn();
            }
            Act::Format => self.fmt = self.fmt.next(),
            Act::Quit => self.quit = true,
            _ => {}
        }
    }

    fn key_main(&mut self, k: KeyEvent) {
        match std::mem::replace(&mut self.overlay, Overlay::None) {
            Overlay::None => {}
            Overlay::Menu { sel, mut filter } => {
                let items = self.filtered_menu(&filter);
                match k.code {
                    KeyCode::Esc | KeyCode::F(1) => {}
                    KeyCode::Up => self.overlay = Overlay::Menu { sel: sel.saturating_sub(1), filter },
                    KeyCode::Down => {
                        self.overlay = Overlay::Menu { sel: (sel + 1).min(items.len().saturating_sub(1)), filter }
                    }
                    KeyCode::Enter => {
                        if let Some(i) = items.get(sel) {
                            self.run_act(i.0, i.3);
                        }
                    }
                    KeyCode::Left | KeyCode::Right if items.get(sel).is_some_and(|i| i.0 == Act::Format) => {
                        self.fmt = self.fmt.next();
                        self.overlay = Overlay::Menu { sel, filter };
                    }
                    KeyCode::Backspace => {
                        filter.pop();
                        self.overlay = Overlay::Menu { sel: 0, filter };
                    }
                    KeyCode::Char(c) => {
                        filter.push(c);
                        self.overlay = Overlay::Menu { sel: 0, filter };
                    }
                    _ => self.overlay = Overlay::Menu { sel, filter },
                }
                return;
            }
            Overlay::Admin { open } => {
                if k.code == KeyCode::Enter {
                    match setup::relaunch_as_admin(&open) {
                        Ok(()) => self.quit = true,
                        Err(e) => self.fail(format!("{e:#}. Still view only.")),
                    }
                }
                return;
            }
            Overlay::ConfirmDelete(i) => {
                if k.code == KeyCode::Enter {
                    let mut new = self.cfg.clone();
                    let gone = new.links.remove(i);
                    self.sel = self.sel.saturating_sub(1);
                    let note = format!("{} ({}) deleted.", gone.name, gone.com);
                    self.start_plan(&setup::APPLY, After::Main, note, move |r| setup::apply(&new, r));
                }
                return;
            }
            Overlay::ConfirmUninstall => {
                if k.code == KeyCode::Enter {
                    self.start_plan(&setup::UNINSTALL, After::Main, String::new(), |r| setup::uninstall(false, false, r));
                }
                return;
            }
        }
        let n = self.cfg.links.len();
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.quit = true,
            KeyCode::F(1) | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.overlay = Overlay::Menu { sel: 0, filter: String::new() }
            }
            KeyCode::F(2) if !self.admin => self.overlay = Overlay::Admin { open: "menu".into() },
            KeyCode::Up if self.sel > 0 => {
                self.sel -= 1;
                self.scroll = 0;
            }
            KeyCode::Down if self.sel + 1 < n => {
                self.sel += 1;
                self.scroll = 0;
            }
            KeyCode::F(8) => self.fmt = self.fmt.next(),
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => self.scroll = 0,
            KeyCode::Home => self.scroll = usize::MAX / 2,
            KeyCode::Char('l') | KeyCode::Char('L') => self.run_act(Act::Folder, false),
            KeyCode::Enter => self.run_act(if n == 0 { Act::Add } else { Act::Edit }, true),
            KeyCode::Char('a') | KeyCode::Char('A') => self.run_act(Act::Add, true),
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete if n > 0 => self.run_act(Act::Delete, true),
            _ => {}
        }
    }

    fn form(&self, idx: Option<usize>) -> Form {
        let (name, host, port, com) = match idx.and_then(|i| self.cfg.links.get(i)) {
            Some(l) => (l.name.clone(), host_of(&l.tcp), port_of(&l.tcp), l.com.clone()),
            None => (format!("Device {}", self.cfg.links.len() + 1), String::new(), String::new(), String::new()),
        };
        let taken: Vec<String> =
            self.cfg.links.iter().enumerate().filter(|(i, _)| Some(*i) != idx).map(|(_, l)| l.com.clone()).collect();
        let mut coms = com0com::free_ports(&taken, 40);
        let mut note = None;
        let mut com = com;
        if !com.is_empty() && !coms.contains(&com) {
            if self.installed && idx.is_some() {
                // An installed device's port is reserved by its own pair: keep it on offer.
                coms.insert(0, com.clone());
            } else {
                let first = coms.first().cloned().unwrap_or_default();
                note = Some(format!(
                    "{com} is reserved by another device on this PC, so I picked {first}. Use ←→ to choose another."
                ));
                com = first;
            }
        }
        if com.is_empty() {
            com = coms.first().cloned().unwrap_or_default();
        }
        Form {
            idx,
            fields: [name, host, port, com],
            focus: if idx.is_none() { F_HOST } else { F_NAME },
            coms,
            install: !self.installed,
            note,
            err: None,
            test: None,
            test_msg: None,
        }
    }

    fn key_form(&mut self, mut f: Form, k: KeyEvent) {
        let back = |app: &mut App| {
            if app.installed || !app.cfg.links.is_empty() {
                app.screen = Screen::Main;
            } else {
                app.screen = Screen::Welcome;
            }
        };
        match k.code {
            KeyCode::Esc => {
                back(self);
                return;
            }
            KeyCode::Up | KeyCode::BackTab => f.focus = (f.focus + 3) % 4,
            KeyCode::Down | KeyCode::Tab => f.focus = (f.focus + 1) % 4,
            KeyCode::Left | KeyCode::Right if f.focus == F_COM && !f.coms.is_empty() => {
                let pos = f.coms.iter().position(|c| *c == f.fields[F_COM]).unwrap_or(0);
                let n = f.coms.len();
                let next = if k.code == KeyCode::Right { (pos + 1) % n } else { (pos + n - 1) % n };
                f.fields[F_COM] = f.coms[next].clone();
                f.note = None;
            }
            KeyCode::Backspace if f.focus != F_COM => {
                f.fields[f.focus].pop();
            }
            KeyCode::Char(c) if f.focus != F_COM => {
                let ok = match f.focus {
                    F_PORT => c.is_ascii_digit() && f.fields[F_PORT].len() < 5,
                    F_HOST => !c.is_whitespace(),
                    _ => true,
                };
                if ok {
                    f.fields[f.focus].push(c);
                    f.test_msg = None;
                }
            }
            KeyCode::F(5) => {
                let addr = format!("{}:{}", f.fields[F_HOST].trim(), f.fields[F_PORT].trim());
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(test_connect(&addr));
                });
                f.test = Some(rx);
                f.test_msg = None;
            }
            KeyCode::Enter => {
                if f.fields[F_HOST].trim().is_empty() {
                    f.err = Some("Enter the device's IP address.".into());
                    f.focus = F_HOST;
                } else if f.fields[F_PORT].trim().is_empty() {
                    f.err = Some("Enter the device's TCP port.".into());
                    f.focus = F_PORT;
                } else if f.fields[F_COM].is_empty() {
                    f.err = Some("No free COM port found.".into());
                } else {
                    let link = Link {
                        name: if f.fields[F_NAME].trim().is_empty() {
                            "Device".into()
                        } else {
                            f.fields[F_NAME].trim().to_string()
                        },
                        tcp: format!("{}:{}", f.fields[F_HOST].trim(), f.fields[F_PORT].trim()),
                        com: f.fields[F_COM].clone(),
                    };
                    let mut new = self.cfg.clone();
                    match f.idx {
                        Some(i) => new.links[i] = link.clone(),
                        None => new.links.push(link.clone()),
                    }
                    match new.validate() {
                        Ok(()) => {
                            if f.idx.is_none() {
                                self.sel = new.links.len() - 1;
                            }
                            let after = After::FormOnFail { idx: f.idx, link, install: f.install };
                            let note = ports_note(&new.links);
                            if f.install {
                                self.start_plan(&setup::INSTALL, after, note, move |r| setup::install(&new, r));
                            } else {
                                self.start_plan(&setup::APPLY, after, note, move |r| setup::apply(&new, r));
                            }
                            return;
                        }
                        Err(e) => f.err = Some(format!("{e:#}")),
                    }
                }
            }
            _ => {}
        }
        self.screen = Screen::Form(f);
    }

    // ---------------------------------------------------------------- drawing

    fn draw(&self, fr: &mut Frame) {
        match &self.screen {
            Screen::Welcome => self.draw_welcome(fr),
            Screen::Form(f) => self.draw_form(fr, f),
            Screen::Progress(p) => self.draw_progress(fr, p),
            Screen::Main => self.draw_main(fr),
        }
    }

    fn draw_welcome(&self, fr: &mut Frame) {
        let mut lines: Vec<Line> = vec![
            "".into(),
            "   ComTap makes a network (TCP) device show up as a COM port, so".into(),
            "   software that only talks serial can read it.".into(),
            "".into(),
            "   Setting up takes three steps:".into(),
            Line::from(vec![Span::styled("     1  ", Style::new().fg(ACCENT)), "Enter the device's IP address and TCP port".into()]),
            Line::from(vec![Span::styled("     2  ", Style::new().fg(ACCENT)), "Pick the COM port your software will open".into()]),
            Line::from(vec![
                Span::styled("     3  ", Style::new().fg(ACCENT)),
                "Install. ComTap then runs in the background and starts with Windows.".into(),
            ]),
            "".into(),
        ];
        if self.admin {
            lines.push(Line::from(vec![
                "   Press ".into(),
                badge("Enter", Color::Green),
                " to start".into(),
            ]));
        } else {
            lines.push("   Setting up needs administrator rights.".into());
            lines.push("".into());
            lines.push(Line::from(vec![
                "   Press ".into(),
                badge("Enter", Color::Green),
                " to continue. Windows asks for permission (UAC).".into(),
            ]));
        }
        if let Some((true, m)) = &self.msg {
            lines.push("".into());
            lines.push(Line::from(Span::styled(format!("   {m}"), Style::new().fg(Color::LightRed))));
        }
        let h = lines.len() as u16 + 3;
        let area = top_box(fr.area(), 80, h);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title("TCP device to COM port"))
            .title_bottom(hints("Enter continue  Esc quit"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
    }

    fn draw_form(&self, fr: &mut Frame, f: &Form) {
        let heading = match (f.install, f.idx) {
            (true, _) => "Set up a device",
            (false, None) => "Add a device",
            (false, Some(_)) => "Edit device",
        };
        let action = if f.install {
            format!(" to install ComTap and create {}", f.fields[F_COM])
        } else {
            " to save and apply".into()
        };
        // Lay out with the full height first, then shrink the box to fit the lines.
        let inner_w = 80.min(fr.area().width).saturating_sub(2);

        let mut lines: Vec<Line> = vec!["".into()];
        let mut cursor = None;
        for (i, label) in LABELS.iter().enumerate() {
            let focused = i == f.focus;
            let label_style =
                if focused { Style::new().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::new() };
            let val_style =
                if focused { Style::new().fg(Color::Black).bg(ACCENT) } else { Style::new().fg(Color::White) };
            let mut spans = vec![
                Span::styled(if focused { format!(" {MARK} ") } else { "   ".into() }, Style::new().fg(ACCENT)),
                Span::styled(format!("{label:<14}"), label_style),
            ];
            let value = &f.fields[i];
            if i == F_COM {
                if focused {
                    spans.push(Span::styled(format!("{LEFT} "), DIM));
                }
                let shown = if value.is_empty() { "none free".to_string() } else { value.clone() };
                spans.push(Span::styled(format!(" {shown} "), val_style));
                if focused {
                    spans.push(Span::styled(format!(" {MARK}"), DIM));
                }
                spans.push(Span::styled(format!("  {} free", f.coms.len()), DIM));
            } else {
                let shown = if value.is_empty() && !focused {
                    match i {
                        F_HOST => "e.g. 192.168.1.50".to_string(),
                        F_PORT => "e.g. 51236".to_string(),
                        _ => String::new(),
                    }
                } else {
                    value.clone()
                };
                let style = if value.is_empty() && !focused { DIM } else { val_style };
                spans.push(Span::styled(format!(" {shown} "), style));
                if focused {
                    // (column, row) inside the box; placed once the box is sized.
                    cursor = Some((3 + 14 + 1 + value.chars().count() as u16, lines.len() as u16));
                }
            }
            lines.push(Line::from(spans));
        }
        if let Some(n) = &f.note {
            lines.push("".into());
            for r in wrap_words(n, inner_w.saturating_sub(6) as usize) {
                lines.push(Line::from(Span::styled(format!("   {r}"), Style::new().fg(Color::Yellow))));
            }
        }
        lines.push("".into());
        lines.push(Line::from(Span::styled(format!("   {}", "─".repeat(inner_w.saturating_sub(6) as usize)), DIM)));
        let mut rows = wrap_words(HELP[f.focus], inner_w.saturating_sub(6) as usize);
        rows.resize(2, String::new());
        for r in rows.into_iter().take(2) {
            lines.push(Line::from(Span::styled(format!("   {r}"), Style::new().fg(Color::Gray))));
        }
        lines.push("".into());
        lines.push(match (&f.test, &f.test_msg) {
            (Some(_), _) => Line::from(Span::styled("   Testing the connection…", Style::new().fg(Color::Yellow))),
            (None, Some((true, m))) => Line::from(Span::styled(format!("   {OK} {m}"), Style::new().fg(Color::Green))),
            (None, Some((false, m))) => {
                Line::from(Span::styled(format!("   {BAD} {m}"), Style::new().fg(Color::LightRed)))
            }
            (None, None) => Line::from(Span::styled("   Press F5 to test the connection to the device.", DIM)),
        });
        if matches!(&f.test_msg, Some((false, _))) && self.svc == "Running" {
            lines.push(Line::from(Span::styled(
                "   (Some devices accept only one connection. If ComTap is connected, a test can fail.)",
                DIM,
            )));
        }
        lines.push("".into());
        lines.push(Line::from(vec!["   Press ".into(), badge("Enter", Color::Green), action.into()]));
        if let Some(e) = &f.err {
            lines.push("".into());
            for r in wrap_words(e, inner_w.saturating_sub(6) as usize) {
                lines.push(Line::from(Span::styled(format!("   {r}"), Style::new().fg(Color::LightRed))));
            }
        }
        lines.push("".into());
        let area = top_box(fr.area(), 80, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title(heading))
            .title_bottom(hints("↑↓ move  ←→ change  type to edit  F5 test  Enter save  Esc back"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
        if let Some((cx, cy)) = cursor.filter(|c| c.1 + 2 < area.height) {
            fr.set_cursor_position((area.x + 1 + cx, area.y + 1 + cy));
        }
    }

    fn draw_progress(&self, fr: &mut Frame, p: &Progress) {
        let w = 84.min(fr.area().width);
        let text_w = w.saturating_sub(12) as usize;
        let mut lines: Vec<Line> = vec!["".into()];
        let spin = SPIN[(p.since.elapsed().as_millis() / 150) as usize % 4];
        for (i, name) in p.plan.steps.iter().enumerate() {
            let (icon, icon_style, name_style, detail, detail_style) = match &p.states[i] {
                St::Pending => (" ", DIM, DIM, "", DIM),
                St::Running => (spin, Style::new().fg(Color::Yellow), Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD), "", DIM),
                St::Done(d) => (OK, Style::new().fg(Color::Green), Style::new(), d.as_str(), DIM),
                St::Warn(d) => ("!", Style::new().fg(Color::Yellow), Style::new().fg(Color::Yellow), d.as_str(), Style::new().fg(Color::Yellow)),
                St::Skip(d) => ("-", DIM, DIM, d.as_str(), DIM),
                St::Fail(d) => (BAD, Style::new().fg(Color::LightRed), Style::new().fg(Color::LightRed), d.as_str(), Style::new().fg(Color::LightRed)),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("   {icon}  "), icon_style.add_modifier(Modifier::BOLD)),
                Span::styled(name.to_string(), name_style),
            ]));
            if !detail.is_empty() {
                for r in wrap_words(detail, text_w) {
                    lines.push(Line::from(Span::styled(format!("      {r}"), detail_style)));
                }
            }
            if matches!(p.states[i], St::Running) && name.starts_with("Create the virtual") {
                let hint = "The first time, Windows can ask to install the com0com driver: click Install.";
                for r in wrap_words(hint, text_w) {
                    lines.push(Line::from(Span::styled(format!("      {r}"), Style::new().fg(Color::Yellow))));
                }
            }
        }
        lines.push("".into());
        match &p.result {
            None => lines.push(Line::from(Span::styled("   Working, please wait…", Style::new().fg(Color::Yellow)))),
            Some(Ok(())) => {
                lines.push(Line::from(Span::styled(
                    "   Done.",
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                )));
                if !p.success_note.is_empty() {
                    lines.push(Line::from(Span::styled(format!("   {}", p.success_note), Style::new().fg(Color::Green))));
                }
                lines.push("".into());
                lines.push(Line::from(vec!["   Press ".into(), badge("Enter", Color::Green), " to continue".into()]));
            }
            Some(Err(_)) => {
                lines.push(Line::from(Span::styled(
                    "   Stopped at the step marked ×. Nothing after it was done.",
                    Style::new().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                )));
                lines.push("".into());
                let back = if matches!(p.after, After::FormOnFail { .. }) { " to go back and fix it" } else { " to go back" };
                lines.push(Line::from(vec!["   Press ".into(), badge("Enter", Color::Green), back.into()]));
            }
        }
        let area = top_box(fr.area(), w, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title(p.plan.title))
            .title_bottom(hints(if p.result.is_some() { "Enter continue" } else { "please wait" }))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_main(&self, fr: &mut Frame) {
        let rows = self.cfg.links.len().max(1) as u16 + 3;
        let [head, devs, tr, msg, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(rows.min(12)),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(fr.area());

        // Header, like RawTap's: name badge, state badge, details.
        let (state, bg, fg) = match self.svc.as_str() {
            "Running" => ("● SERVICE RUNNING", Color::Green, Color::Black),
            "not installed" => ("NOT INSTALLED", Color::Yellow, Color::Black),
            _ => ("○ SERVICE STOPPED", Color::Red, Color::White),
        };
        let mut spans = vec![
            badge("ComTap", ACCENT),
            Span::raw(" "),
            Span::styled(format!(" {state} "), Style::new().fg(fg).bg(bg)),
            Span::styled(format!("  driver com0com {}", self.drv), DIM),
        ];
        let right = if self.admin { badge("ADMIN", Color::Green) } else { badge("VIEW ONLY · F2 admin", Color::Yellow) };
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let gap = (head.width as usize).saturating_sub(used + right.width());
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(right);
        fr.render_widget(Paragraph::new(Line::from(spans)), head);

        // Devices
        let mut lines: Vec<Line> = Vec::new();
        if let Some(n) = &self.cfg_note {
            lines.push(Line::from(Span::styled(format!(" {n}"), Style::new().fg(Color::LightRed))));
        } else if self.cfg.links.is_empty() {
            lines.push(Line::from(Span::styled(" No devices yet. Press A to add one.", Style::new().fg(Color::Yellow))));
        } else if !self.installed {
            lines.push(Line::from(Span::styled(
                " ComTap is not installed. Press F1 and choose Install.",
                Style::new().fg(Color::Yellow),
            )));
        }
        for (i, l) in self.cfg.links.iter().enumerate() {
            let st = self.link_status(l);
            let (state, color) = match st.map(|s| s.state.as_str()) {
                Some("connected") => ("connected", Color::Green),
                Some(s @ ("retrying" | "connecting" | "starting")) => (s, Color::Yellow),
                Some(s) => (s, Color::LightRed),
                None => ("service not running", Color::DarkGray),
            };
            let selected = i == self.sel;
            let mut spans = vec![
                Span::styled(if selected { format!(" {MARK} ") } else { "   ".into() }, Style::new().fg(ACCENT)),
                Span::styled("● ", Style::new().fg(color)),
                Span::styled(
                    format!("{:<20}", trunc(&l.name, 20)),
                    if selected { Style::new().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::new() },
                ),
                Span::styled(format!("{:<6}", l.com), Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {BOTH} "), DIM),
                Span::raw(format!("{:<22} ", l.tcp)),
                Span::styled(format!("{state:<12}"), Style::new().fg(color)),
            ];
            if let Some(s) = st {
                if let Some(t) = &s.last_rx {
                    spans.push(Span::styled(format!("  last data {}", t.get(11..).unwrap_or(t)), DIM));
                }
            }
            lines.push(Line::from(spans));
            if selected {
                if let Some(e) = st.and_then(|s| s.last_error.clone()) {
                    lines.push(Line::from(Span::styled(format!("       {e}"), Style::new().fg(Color::LightRed))));
                }
            }
        }
        let block = Block::bordered().title(" Devices ").border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), devs);

        self.draw_traffic(fr, tr);

        let m = match &self.msg {
            Some((true, m)) => Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::LightRed))),
            Some((false, m)) => Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::Green))),
            None if !self.admin => Line::from(Span::styled(" View only. Changes need admin: press F2.", DIM)),
            None => Line::from(""),
        };
        fr.render_widget(Paragraph::new(m), msg);
        fr.render_widget(
            Paragraph::new(hints("F1 menu  ↑↓ device  Enter edit  A add  D delete  F8 format  PgUp/PgDn scroll  L logs  Q quit")),
            foot,
        );

        match &self.overlay {
            Overlay::None => {}
            Overlay::Menu { sel, filter } => self.draw_menu(fr, *sel, filter),
            Overlay::Admin { .. } => confirm(
                fr,
                " Needs admin ",
                Color::Yellow,
                vec![
                    "This change needs administrator rights.".into(),
                    "ComTap will reopen as admin, and Windows will ask for permission.".into(),
                ],
                "reopen as admin",
            ),
            Overlay::ConfirmDelete(i) => {
                let l = &self.cfg.links[*i];
                confirm(
                    fr,
                    " Delete device ",
                    Color::Yellow,
                    vec![format!("Delete '{}'?", l.name).into(), format!("Its virtual port {} will be removed.", l.com).into()],
                    "delete",
                )
            }
            Overlay::ConfirmUninstall => confirm(
                fr,
                " Uninstall ComTap ",
                Color::Yellow,
                vec![
                    "Remove the service, its virtual COM ports and the desktop shortcut?".into(),
                    "Your settings, logs and the com0com driver stay.".into(),
                ],
                "uninstall",
            ),
        }
    }

    fn draw_menu(&self, fr: &mut Frame, sel: usize, filter: &str) {
        let items = self.filtered_menu(filter);
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(" Filter: ", DIM),
            Span::styled(if filter.is_empty() { "type to search".to_string() } else { filter.to_string() }, if filter.is_empty() { DIM } else { Style::new().fg(Color::White) }),
        ]));
        lines.push("".into());
        for (i, (_, label, key, admin)) in items.iter().enumerate() {
            let focused = i == sel;
            let lock = if *admin && !self.admin { " (admin)" } else { "" };
            let text = format!("{label}{lock}");
            let mut spans = vec![
                Span::styled(if focused { format!(" {MARK} ") } else { "   ".into() }, Style::new().fg(ACCENT)),
                Span::styled(
                    format!("{text:<38}"),
                    if focused { Style::new().fg(Color::Black).bg(ACCENT) } else { Style::new() },
                ),
            ];
            spans.push(Span::styled(format!(" {key:>5}"), DIM));
            lines.push(Line::from(spans));
        }
        if items.is_empty() {
            lines.push(Line::from(Span::styled("   nothing matches", DIM)));
        }
        let area = Rect { y: fr.area().y + 2, ..top_box(fr.area(), 54, lines.len() as u16 + 2) };
        let area = area.intersection(fr.area());
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title("Menu"))
            .title_bottom(hints("↑↓ move  Enter run  ←→ change  Esc close"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_traffic(&self, fr: &mut Frame, area: Rect) {
        let name = self.cfg.links.get(self.sel).map(|l| l.name.as_str()).unwrap_or("-");
        let inner_w = area.width.saturating_sub(2);
        let inner_h = area.height.saturating_sub(2) as usize;
        let mut rows: Vec<Line> = Vec::new();
        for l in &self.traffic {
            rows.extend(entry_lines(l, inner_w, self.fmt));
        }
        if rows.is_empty() {
            rows.push(Line::from(Span::styled(
                "No data today yet. Data from the device and from your software appears here.",
                DIM,
            )));
        }
        let max_scroll = rows.len().saturating_sub(inner_h);
        let scroll = self.scroll.min(max_scroll);
        let end = rows.len() - scroll;
        let start = end.saturating_sub(inner_h);
        let mut block = Block::bordered()
            .title(format!(" Traffic · {name} · {} (F8) ", self.fmt.name()))
            .border_style(Style::new().fg(ACCENT));
        if scroll > 0 {
            block = block.title_top(
                Line::from(Span::styled(" scrolled · End to follow ", Style::new().fg(Color::Yellow))).right_aligned(),
            );
        }
        fr.render_widget(Paragraph::new(rows[start..end].to_vec()).block(block), area);
    }
}

// ---------------------------------------------------------------- helpers

fn ports_note(links: &[Link]) -> String {
    let ports: Vec<&str> = links.iter().map(|l| l.com.as_str()).collect();
    format!("Your software can now open {}.", ports.join(", "))
}

fn host_of(tcp: &str) -> String {
    tcp.rsplit_once(':').map(|(h, _)| h).unwrap_or(tcp).to_string()
}

fn port_of(tcp: &str) -> String {
    tcp.rsplit_once(':').map(|(_, p)| p).unwrap_or("").to_string()
}

fn test_connect(addr: &str) -> (bool, String) {
    let t0 = Instant::now();
    let addrs = match addr.to_socket_addrs() {
        Ok(a) => a.collect::<Vec<_>>(),
        Err(e) => return (false, format!("Cannot find {addr}: {e}")),
    };
    let mut last = format!("Cannot find {addr}");
    for a in addrs {
        match TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
            Ok(_) => return (true, format!("{addr} answered in {} ms", t0.elapsed().as_millis())),
            Err(e) => last = format!("{addr}: {e}"),
        }
    }
    (false, last)
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

/// A box centred across, pinned near the top (like RawTap's setup box).
fn top_box(full: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(full.width);
    let h = h.min(full.height);
    let top = if full.height >= h + 2 { 1 } else { 0 };
    Rect { x: full.x + (full.width - w) / 2, y: full.y + top, width: w, height: h.min(full.height - top) }
}

fn confirm(fr: &mut Frame, heading: &str, color: Color, body: Vec<Line<'static>>, what: &str) {
    let mut lines = vec!["".into()];
    lines.extend(body.into_iter().map(|l| {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(l.spans);
        Line::from(spans)
    }));
    lines.push("".into());
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(30) as u16 + 4).max(44).min(fr.area().width);
    let h = (lines.len() as u16 + 2).min(fr.area().height);
    let a = fr.area();
    let area = Rect { x: a.x + (a.width - w) / 2, y: a.y + (a.height - h) / 2, width: w, height: h };
    fr.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(badge(heading.trim(), color)))
        .title_bottom(Line::from(vec![
            badge("Enter", Color::Green),
            Span::raw(format!(" {what}  ")),
            badge("Esc", Color::Gray),
            Span::raw(" cancel "),
        ]))
        .border_style(Style::new().fg(color));
    fr.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

/// Greedy word wrap.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut rows = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        rows.push(cur);
    }
    rows
}

/// Wraps tokens after a prefix, indenting continuation rows (RawTap's raw log layout).
fn wrap_tokens(prefix: String, tokens: Vec<String>, sep: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    let indent = prefix.chars().count();
    let avail = width.saturating_sub(indent).max(8);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for t in tokens {
        let add = if cur.is_empty() { t.len() } else { sep.len() + t.len() };
        if !cur.is_empty() && cur.len() + add > avail {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str(sep);
        }
        cur.push_str(&t);
    }
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let p = if i == 0 { prefix.clone() } else { " ".repeat(indent) };
            Line::from(vec![Span::styled(p, DIM), Span::styled(r, style)])
        })
        .collect()
}

/// Screen rows for one traffic entry, in RawTap's formats and colours.
fn entry_lines(l: &traffic::Line, width: u16, fmt: Fmt) -> Vec<Line<'static>> {
    let w = width as usize;
    match l {
        traffic::Line::Event { time, msg } => {
            let lower = msg.to_lowercase();
            let (mark, style) = if lower.starts_with("connected") {
                (OK, Style::new().fg(Color::Green))
            } else if lower.contains("disconnected") || lower.contains("cannot") || lower.contains("failed")
                || lower.contains("connect ")
            {
                (BAD, Style::new().fg(Color::LightRed))
            } else {
                ("·", Style::new().fg(Color::Gray))
            };
            let prefix = format!("{time} {mark} ");
            let body = trunc(msg, w.saturating_sub(prefix.chars().count()).max(1));
            vec![Line::from(vec![Span::styled(prefix, DIM), Span::styled(body, style)])]
        }
        traffic::Line::Data { time, dir, bytes } => {
            let rx = *dir == Dir::Rx;
            let tag = if rx { "RX" } else { "TX" };
            let (hex_style, text_style) = if rx {
                (Style::new().fg(Color::Cyan), Style::new().fg(Color::White))
            } else {
                (Style::new().fg(Color::Yellow), Style::new().fg(Color::LightYellow))
            };
            if fmt == Fmt::Compact {
                let prefix = format!("{time} {tag} ");
                let avail = w.saturating_sub(prefix.len());
                let hex = codec::hex_string(bytes);
                let hw = (avail / 2).max(8);
                let text = codec::text_string(bytes);
                return vec![Line::from(vec![
                    Span::styled(prefix, DIM),
                    Span::styled(format!("{:<hw$}", trunc(&hex, hw)), hex_style),
                    Span::styled(" │ ", DIM),
                    Span::styled(trunc(&text, avail.saturating_sub(hw + 3).max(1)), text_style),
                ])];
            }
            let mut out = Vec::new();
            if fmt != Fmt::Text {
                let toks = bytes.iter().map(|b| format!("{b:02X}")).collect();
                out.extend(wrap_tokens(format!("{time} {tag} HEX  "), toks, " ", hex_style, w));
            }
            if fmt != Fmt::Hex {
                let toks = bytes.iter().map(|&b| codec::byte_text(b)).collect();
                out.extend(wrap_tokens(format!("{time} {tag} TEXT "), toks, "", text_style, w));
            }
            out
        }
    }
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
                Link { name: "Dock 3 scanner".into(), tcp: "10.1.128.186:51236".into(), com: "COM23".into() },
                Link { name: "Scale".into(), tcp: "10.1.128.190:4001".into(), com: "COM24".into() },
            ],
        };
        app.cfg_note = None;
        app.svc = "Running".into();
        app.drv = "Running".into();
        app.installed = true;
        app.admin = false;
        app.traffic = vec![
            traffic::parse_line("2026-11-16 12:01:20.000 -- connected to 10.1.128.186:51236").unwrap(),
            traffic::parse_line("2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | x").unwrap(),
            traffic::parse_line("2026-11-16 12:01:40.002 TX 54 0D | T<CR>").unwrap(),
            traffic::parse_line("2026-11-16 12:05:00.000 -- disconnected: device closed the connection").unwrap(),
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

    fn progress(app: &App, plan: &'static Plan, states: Vec<St>, result: Option<Result<(), String>>) -> Screen {
        let (_tx, rx) = channel();
        let _ = app;
        Screen::Progress(Progress {
            plan,
            states,
            rx,
            result,
            since: Instant::now(),
            after: After::Main,
            success_note: "Your software can now open COM23.".into(),
        })
    }

    fn install_states(upto: usize, last: St) -> Vec<St> {
        let mut v = vec![St::Pending; setup::INSTALL.steps.len()];
        for s in v.iter_mut().take(upto) {
            *s = St::Done(String::new());
        }
        v[1] = St::Done(r"C:\ProgramData\ComTap\comtap_config.json".into());
        v[4] = St::Skip("none found".into());
        v[upto] = last;
        v
    }

    #[test]
    fn draws_every_screen_at_any_size() {
        let mut app = sample();
        let screens = |app: &App| -> Vec<Screen> {
            vec![
                Screen::Welcome,
                Screen::Form(app.form(None)),
                Screen::Form(app.form(Some(0))),
                progress(app, &setup::INSTALL, install_states(6, St::Running), None),
                progress(app, &setup::INSTALL, install_states(10, St::Done("1 connected".into())), Some(Ok(()))),
                progress(
                    app,
                    &setup::INSTALL,
                    install_states(6, St::Fail("COM5 is reserved by another device".into())),
                    Some(Err("x".into())),
                ),
                Screen::Main,
            ]
        };
        for (w, h) in [(120, 40), (80, 24), (40, 12), (20, 6)] {
            for s in screens(&app) {
                app.screen = s;
                for fmt in [Fmt::Compact, Fmt::HexText, Fmt::Hex, Fmt::Text] {
                    app.fmt = fmt;
                    render(&app, w, h);
                }
            }
            app.screen = Screen::Main;
            for o in 0..4 {
                app.overlay = match o {
                    0 => Overlay::Menu { sel: 2, filter: String::new() },
                    1 => Overlay::Admin { open: "menu".into() },
                    2 => Overlay::ConfirmDelete(1),
                    _ => Overlay::ConfirmUninstall,
                };
                render(&app, w, h);
            }
            app.overlay = Overlay::None;
        }
    }

    #[test]
    #[ignore = "prints screens; run with --ignored --nocapture"]
    fn show_screens() {
        let mut app = sample();
        app.fmt = Fmt::HexText;
        let shots: Vec<(&str, Screen)> = vec![
            ("WELCOME", Screen::Welcome),
            ("FORM", Screen::Form(app.form(None))),
            ("PROGRESS", progress(&app, &setup::INSTALL, install_states(6, St::Running), None)),
            (
                "DONE",
                progress(
                    &app,
                    &setup::INSTALL,
                    {
                        let mut s = install_states(10, St::Done("1 connected".into()));
                        s[2] = St::Done(r"C:\Program Files\ComTap\comtap.exe".into());
                        s[6] = St::Done("COM23".into());
                        s
                    },
                    Some(Ok(())),
                ),
            ),
            ("MAIN", Screen::Main),
        ];
        for (name, s) in shots {
            app.screen = s;
            println!("===== {name}\n{}", render(&app, 100, 30));
        }
        app.screen = Screen::Main;
        app.overlay = Overlay::Menu { sel: 3, filter: String::new() };
        println!("===== MENU\n{}", render(&app, 100, 30));
    }
}
