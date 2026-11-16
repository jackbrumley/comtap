//! The interactive screen, styled like RawTap: double-click comtap.exe or the desktop shortcut.
//!
//! New user:  Welcome -> device form -> install checklist -> main screen.
//! Installed: main screen. Anyone can look; changes reopen ComTap as admin.
use crate::config::{Config, Link};
use crate::setup::{self, Plan, St};
use crate::status::StatusFile;
use crate::traffic::{self, Dir};
use crate::update::UpdateCheck;
use crate::{codec, paths, ports, service, status};
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
const LABELS: [&str; 4] = ["Name", "Device IP", "TCP port", words::PORT_LABEL];
const HELP: [&str; 4] = [
    "A label for this device, shown on this screen and in the logs. For example: Dock 3 scanner.",
    "The device's IP address or network name. ComTap connects to it as a TCP client.",
    "The TCP port the device sends its data on. You find it in the device's network settings.",
    words::PORT_HELP,
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
    FormOnFail {
        idx: Option<usize>,
        link: Link,
        install: bool,
    },
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
    Menu {
        sel: usize,
        filter: String,
    },
    Admin {
        open: String,
    },
    ConfirmDelete(usize),
    ConfirmUninstall,
    /// "Update available": `sel` is the focused button (update_prompt::BUTTONS).
    Update {
        check: UpdateCheck,
        sel: usize,
    },
    /// A message with a Close button (up to date, update failed).
    Notice {
        title: String,
        lines: Vec<String>,
    },
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
    UpdateNow,
    CheckUpdates,
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
    update_rx: Option<Receiver<std::result::Result<UpdateCheck, String>>>,
    /// The latest check's result.
    update: Option<UpdateCheck>,
    /// The running check was asked for by the user.
    update_manual: bool,
    /// The dialog has opened by itself this run.
    update_prompted: bool,
    last_checked: Option<String>,
    /// Set when the user chose Update now: `run` starts the installer after the screen closes.
    start_update: bool,
}

/// `update_failed`: the installer failed and started this version again (`--update-failed`).
pub fn run(open: Option<String>, update_failed: bool) -> Result<()> {
    let mut app = App::new();
    app.screen = app.initial_screen(open.as_deref());
    if update_failed {
        app.overlay = Overlay::Notice {
            title: "Update failed".into(),
            lines: vec![crate::update::update_failed_message()],
        };
    }
    app.start_update_check(false);
    let mut term = ratatui::init();
    let r = app.main_loop(&mut term);
    ratatui::restore();
    r?;
    if app.start_update {
        println!("{}", words::UPDATE_STARTING);
        crate::update::run_update().map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

fn load_config() -> (Config, Option<String>) {
    let p = paths::installed_config();
    if !p.exists() {
        return (Config { links: vec![] }, None);
    }
    match Config::load(&p) {
        Ok(c) => (c, None),
        Err(e) => (
            Config { links: vec![] },
            Some(format!("The config file has an error: {e:#}")),
        ),
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
            update_rx: None,
            update: None,
            update_manual: false,
            update_prompted: false,
            last_checked: None,
            start_update: false,
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
            Some("menu") => {
                self.overlay = Overlay::Menu {
                    sel: 0,
                    filter: String::new(),
                }
            }
            Some("setup") if self.admin => return self.setup_form(),
            _ => {}
        }
        if !self.installed && open.is_none() {
            Screen::Welcome
        } else {
            Screen::Main
        }
    }

    /// The first device form of a new setup.
    fn setup_form(&self) -> Screen {
        Screen::Form(self.form(if self.cfg.links.is_empty() {
            None
        } else {
            Some(0)
        }))
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
        self.poll_update_check();
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
            let com = self
                .cfg
                .links
                .get(self.sel)
                .map(|l| l.com.clone())
                .unwrap_or_default();
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
        let ctrl_c =
            k.code == KeyCode::Char('c') && k.modifiers.contains(event::KeyModifiers::CONTROL);
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
            v.push((Act::Install, "Reinstall ComTap".into(), "", true));
            v.push((Act::Uninstall, "Uninstall ComTap".into(), "", true));
        }
        if let Some(u) = self.update.as_ref().filter(|u| u.update_available) {
            v.push((
                Act::UpdateNow,
                format!("Update to {}", u.latest_version),
                "",
                false,
            ));
        }
        let checked = match (&self.update_rx, &self.last_checked) {
            (Some(_), _) => " (checking…)".to_string(),
            (None, Some(t)) => format!(" (last checked {t})"),
            (None, None) => String::new(),
        };
        v.push((
            Act::CheckUpdates,
            format!("Check for updates{checked}"),
            "",
            false,
        ));
        v.push((Act::Folder, "Open the log folder".into(), "L", false));
        v.push((
            Act::Format,
            format!("Traffic format: {}", self.fmt.name()),
            "F8",
            false,
        ));
        v.push((Act::Quit, "Quit".into(), "Q", false));
        v
    }

    fn filtered_menu(&self, filter: &str) -> Vec<(Act, String, &'static str, bool)> {
        let f = filter.to_lowercase();
        self.menu_items()
            .into_iter()
            .filter(|i| i.1.to_lowercase().contains(&f))
            .collect()
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
            self.overlay = Overlay::Admin {
                open: self.open_name(act),
            };
            return;
        }
        self.overlay = Overlay::None;
        match act {
            Act::Add => self.screen = Screen::Form(self.form(None)),
            Act::Edit if !self.cfg.links.is_empty() => {
                self.screen = Screen::Form(self.form(Some(self.sel)))
            }
            Act::Delete if !self.cfg.links.is_empty() => {
                self.overlay = Overlay::ConfirmDelete(self.sel)
            }
            Act::Install => {
                if self.cfg.links.is_empty() {
                    self.screen = Screen::Form(self.form(None));
                } else {
                    let cfg = self.cfg.clone();
                    let note = ports_note(&cfg.links);
                    self.start_plan(&setup::INSTALL, After::Main, note, move |r| {
                        setup::install(&cfg, r)
                    });
                }
            }
            Act::Restart | Act::Start => {
                self.start_plan(&setup::RESTART, After::Main, String::new(), setup::restart)
            }
            Act::Stop => self.start_plan(&setup::STOP, After::Main, String::new(), setup::stop),
            Act::Repair => {
                self.start_plan(&setup::REPAIR, After::Main, String::new(), setup::repair)
            }
            Act::Uninstall => self.overlay = Overlay::ConfirmUninstall,
            Act::UpdateNow => self.begin_update(),
            Act::CheckUpdates => self.start_update_check(true),
            Act::Folder => {
                let _ = std::process::Command::new("explorer")
                    .arg(paths::data_dir())
                    .spawn();
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
                    KeyCode::Up => {
                        self.overlay = Overlay::Menu {
                            sel: sel.saturating_sub(1),
                            filter,
                        }
                    }
                    KeyCode::Down => {
                        self.overlay = Overlay::Menu {
                            sel: (sel + 1).min(items.len().saturating_sub(1)),
                            filter,
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(i) = items.get(sel) {
                            self.run_act(i.0, i.3);
                        }
                    }
                    KeyCode::Left | KeyCode::Right
                        if items.get(sel).is_some_and(|i| i.0 == Act::Format) =>
                    {
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
                    self.start_plan(&setup::APPLY, After::Main, note, move |r| {
                        setup::apply(&new, r)
                    });
                }
                return;
            }
            Overlay::Update { check, sel } => {
                self.key_update_prompt(check, sel, k);
                return;
            }
            Overlay::Notice { title, lines } => {
                if !matches!(k.code, KeyCode::Enter | KeyCode::Esc) {
                    self.overlay = Overlay::Notice { title, lines };
                }
                return;
            }
            Overlay::ConfirmUninstall => {
                if k.code == KeyCode::Enter {
                    self.start_plan(&setup::UNINSTALL, After::Main, String::new(), |r| {
                        setup::uninstall(false, false, r)
                    });
                }
                return;
            }
        }
        let n = self.cfg.links.len();
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.quit = true,
            KeyCode::F(1) | KeyCode::Char('s') | KeyCode::Char('S') => {
                self.overlay = Overlay::Menu {
                    sel: 0,
                    filter: String::new(),
                }
            }
            KeyCode::F(2) if !self.admin => {
                self.overlay = Overlay::Admin {
                    open: "menu".into(),
                }
            }
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
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete if n > 0 => {
                self.run_act(Act::Delete, true)
            }
            _ => {}
        }
    }

    fn form(&self, idx: Option<usize>) -> Form {
        let (name, host, port, com) = match idx.and_then(|i| self.cfg.links.get(i)) {
            Some(l) => (
                l.name.clone(),
                host_of(&l.tcp),
                port_of(&l.tcp),
                l.com.clone(),
            ),
            None => (
                format!("Device {}", self.cfg.links.len() + 1),
                String::new(),
                String::new(),
                String::new(),
            ),
        };
        let taken: Vec<String> = self
            .cfg
            .links
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != idx)
            .map(|(_, l)| l.com.clone())
            .collect();
        let mut coms = ports::free_ports(&taken, 40);
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
                let pos = f
                    .coms
                    .iter()
                    .position(|c| *c == f.fields[F_COM])
                    .unwrap_or(0);
                let n = f.coms.len();
                let next = if k.code == KeyCode::Right {
                    (pos + 1) % n
                } else {
                    (pos + n - 1) % n
                };
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
                            let after = After::FormOnFail {
                                idx: f.idx,
                                link,
                                install: f.install,
                            };
                            let note = ports_note(&new.links);
                            if f.install {
                                self.start_plan(&setup::INSTALL, after, note, move |r| {
                                    setup::install(&new, r)
                                });
                            } else {
                                self.start_plan(&setup::APPLY, after, note, move |r| {
                                    setup::apply(&new, r)
                                });
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
}

// ---------------------------------------------------------------- helpers

fn ports_note(links: &[Link]) -> String {
    let ports: Vec<&str> = links.iter().map(|l| l.com.as_str()).collect();
    format!("Your software can now open {}.", ports.join(", "))
}

fn host_of(tcp: &str) -> String {
    tcp.rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(tcp)
        .to_string()
}

fn port_of(tcp: &str) -> String {
    tcp.rsplit_once(':')
        .map(|(_, p)| p)
        .unwrap_or("")
        .to_string()
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
            Ok(_) => {
                return (
                    true,
                    format!("{addr} answered in {} ms", t0.elapsed().as_millis()),
                );
            }
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

mod draw;
mod traffic_view;
mod update_prompt;
mod words;

#[cfg(test)]
mod tests;
