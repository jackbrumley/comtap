//! Keys for every screen except the device form, and the F1 menu: its items and what each one
//! does. The same keys work everywhere: ↑↓ choose, Enter open, Esc back, F1 menu. There are no
//! letter shortcuts, so each action has one place. Ctrl+C quits, as in any terminal app.
use super::devices::{DEVICE_BACK, DEVICE_DELETE, DEVICE_EDIT, DEVICE_ITEMS, DEVICE_WATCH};
use super::*;

impl App {
    pub(super) fn key(&mut self, k: KeyEvent) {
        let ctrl_c =
            k.code == KeyCode::Char('c') && k.modifiers.contains(event::KeyModifiers::CONTROL);
        let screen = std::mem::replace(&mut self.screen, Screen::Main);
        let browsing = matches!(
            screen,
            Screen::Main | Screen::Device { .. } | Screen::Monitor { .. }
        );
        if browsing && ctrl_c {
            self.quit = true;
            return;
        }
        if browsing && !matches!(self.overlay, Overlay::None) {
            self.screen = screen;
            self.key_overlay(k);
            return;
        }
        if browsing && k.code == KeyCode::F(1) {
            self.screen = screen;
            self.overlay = Overlay::Menu {
                sel: 0,
                filter: String::new(),
            };
            return;
        }
        match screen {
            Screen::Welcome => self.key_welcome(k),
            Screen::Form(f) => self.key_form(f, k),
            Screen::Progress(p) => self.key_progress(p, k),
            Screen::Main => self.key_list(k),
            Screen::Device { idx, sel } => self.key_device(idx, sel, k),
            Screen::Monitor { idx } => self.key_monitor(idx, k),
        }
    }

    pub(super) fn key_welcome(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter if self.admin => self.screen = self.setup_form(),
            KeyCode::Enter => match setup::relaunch_as_admin("setup") {
                Ok(()) => self.quit = true,
                Err(e) => {
                    self.fail(format!("{e:#}"));
                    self.screen = Screen::Welcome;
                }
            },
            KeyCode::Esc => self.quit = true,
            _ => self.screen = Screen::Welcome,
        }
    }

    pub(super) fn key_progress(&mut self, p: Progress, k: KeyEvent) {
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
                f.enabled = link.enabled;
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
            // An edited device: back on its page, to see it come up.
            (Ok(()), After::FormOnFail { idx: Some(i), .. }) if i < self.cfg.links.len() => {
                self.say(format!("{}: done.", p.plan.title));
                self.screen = Screen::Device { idx: i, sel: 0 };
            }
            (Ok(()), _) => {
                self.say(format!("{}: done.", p.plan.title));
                self.screen = Screen::Main;
            }
        }
    }

    /// The device list: "+ Add a device", then one row per device.
    fn key_list(&mut self, k: KeyEvent) {
        let rows = self.cfg.links.len() + 1;
        match k.code {
            KeyCode::Up => self.row = self.row.saturating_sub(1),
            KeyCode::Down => self.row = (self.row + 1).min(rows - 1),
            KeyCode::Home => self.row = 0,
            KeyCode::End => self.row = rows - 1,
            KeyCode::Enter if self.row == 0 => {
                if self.need_admin("add") {
                    return;
                }
                self.screen = Screen::Form(self.form(None));
            }
            KeyCode::Enter => {
                self.screen = Screen::Device {
                    idx: self.row - 1,
                    sel: DEVICE_WATCH,
                }
            }
            _ => {}
        }
    }

    /// A device's page: Watch data, Edit settings, Delete this device, Back.
    fn key_device(&mut self, idx: usize, sel: usize, k: KeyEvent) {
        if idx >= self.cfg.links.len() {
            return; // the device went away (config reloaded): back to the list
        }
        let mut sel = sel;
        match k.code {
            KeyCode::Up => sel = sel.saturating_sub(1),
            KeyCode::Down => sel = (sel + 1).min(DEVICE_ITEMS.len() - 1),
            KeyCode::Esc => return,
            KeyCode::Enter => match sel {
                DEVICE_WATCH => {
                    self.scroll = 0;
                    self.screen = Screen::Monitor { idx };
                    return;
                }
                DEVICE_EDIT => {
                    if !self.need_admin(&format!("edit:{idx}")) {
                        self.screen = Screen::Form(self.form(Some(idx)));
                        return;
                    }
                }
                DEVICE_DELETE => {
                    if !self.need_admin(&format!("device:{idx}")) {
                        self.overlay = Overlay::ConfirmDelete(idx);
                    }
                }
                DEVICE_BACK => return,
                _ => {}
            },
            _ => {}
        }
        self.screen = Screen::Device { idx, sel };
    }

    /// Watching a device's data: ←→ changes the format, PgUp/PgDn scrolls, Esc goes back.
    fn key_monitor(&mut self, idx: usize, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.screen = Screen::Device {
                    idx,
                    sel: DEVICE_WATCH,
                };
                return;
            }
            KeyCode::Right => self.fmt = self.fmt.next(),
            KeyCode::Left => self.fmt = self.fmt.prev(),
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::End => self.scroll = 0,
            KeyCode::Home => self.scroll = usize::MAX / 2,
            _ => {}
        }
        self.screen = Screen::Monitor { idx };
    }

    /// Without admin rights, offers to reopen as admin at `open` and returns true.
    fn need_admin(&mut self, open: &str) -> bool {
        if self.admin {
            return false;
        }
        self.overlay = Overlay::Admin { open: open.into() };
        true
    }

    /// Keys while a dialog or the menu is open over the list, a device page or its data.
    fn key_overlay(&mut self, k: KeyEvent) {
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
                        if let Some(&(act, _, needs_admin)) = items.get(sel) {
                            self.run_act(act, needs_admin);
                        }
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
            }
            Overlay::Admin { open } => {
                if k.code == KeyCode::Enter {
                    match setup::relaunch_as_admin(&open) {
                        Ok(()) => self.quit = true,
                        Err(e) => self.fail(format!("{e:#}. Still view only.")),
                    }
                }
            }
            Overlay::ConfirmDelete(i) => {
                if k.code == KeyCode::Enter {
                    let mut new = self.cfg.clone();
                    let gone = new.links.remove(i);
                    self.row = self.row.min(new.links.len());
                    let note = format!("{} ({}) deleted.", gone.name, gone.com);
                    self.start_plan(&setup::APPLY, After::Main, note, move |r| {
                        setup::apply(&new, r)
                    });
                }
            }
            Overlay::Update { check, sel } => self.key_update_prompt(check, sel, k),
            Overlay::Notice { title, lines } => {
                if !matches!(k.code, KeyCode::Enter | KeyCode::Esc) {
                    self.overlay = Overlay::Notice { title, lines };
                }
            }
            Overlay::ConfirmUninstall => {
                if k.code == KeyCode::Enter {
                    self.start_plan(&setup::UNINSTALL, After::Main, String::new(), |r| {
                        setup::uninstall(false, false, r)
                    });
                }
            }
        }
    }

    /// The F1 menu, in the App Standard's order: actions, then Updates and Debug last.
    /// (action, label, needs admin)
    pub(super) fn menu_items(&self) -> Vec<(Act, String, bool)> {
        let mut v = Vec::new();
        if !self.admin {
            v.push((Act::Admin, words::REOPEN_AS_ADMIN.to_string(), false));
        }
        if !self.installed {
            v.push((Act::Install, "Install ComTap".into(), true));
        } else {
            v.push((Act::Restart, "Restart the service".into(), true));
            if self.svc == "Running" {
                v.push((Act::Stop, "Stop the service".into(), true));
            } else {
                v.push((Act::Start, "Start the service".into(), true));
            }
            v.push((Act::Repair, words::REPAIR_PORTS.into(), true));
            v.push((Act::Install, "Reinstall ComTap".into(), true));
            v.push((Act::Uninstall, "Uninstall ComTap".into(), true));
        }
        v.push((Act::Quit, "Quit".into(), false));
        if let Some(u) = self.update.as_ref().filter(|u| u.update_available) {
            v.push((
                Act::UpdateNow,
                format!("Update to {}", u.latest_version),
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
            false,
        ));
        v.push((Act::Folder, "Open the log folder".into(), false));
        v
    }

    pub(super) fn filtered_menu(&self, filter: &str) -> Vec<(Act, String, bool)> {
        let f = filter.to_lowercase();
        self.menu_items()
            .into_iter()
            .filter(|i| i.1.to_lowercase().contains(&f))
            .collect()
    }

    pub(super) fn run_act(&mut self, act: Act, needs_admin: bool) {
        if needs_admin && self.need_admin("menu") {
            return;
        }
        self.overlay = Overlay::None;
        match act {
            Act::Admin => {
                self.overlay = Overlay::Admin {
                    open: "menu".into(),
                }
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
                if let Err(e) = open_folder(&paths::logs_dir()) {
                    self.fail(e);
                }
            }
            Act::Quit => self.quit = true,
        }
    }
}
