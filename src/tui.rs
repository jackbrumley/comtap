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
use form::{F_COM, F_HOST, F_NAME, F_PORT, Form, host_of, port_of};
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
    Toggle,
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
    /// The dialog has been offered by itself this run.
    update_prompted: bool,
    /// An update dialog waiting for the main screen to be free.
    update_offer: Option<UpdateCheck>,
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

/// Opens a folder in the file manager (Explorer, or the desktop's via xdg-open).
fn open_folder(folder: &std::path::Path) -> std::result::Result<(), String> {
    let opener = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open {}: {e}", folder.display()))
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
            update_offer: None,
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
        self.show_update_offer();
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
}

// ---------------------------------------------------------------- helpers

fn ports_note(links: &[Link]) -> String {
    let ports: Vec<&str> = links.iter().map(|l| l.com.as_str()).collect();
    format!("Your software can now open {}.", ports.join(", "))
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

mod device_list;
mod draw;
mod form;
mod keys;
mod traffic_view;
mod update_prompt;
mod words;

#[cfg(test)]
mod tests;
