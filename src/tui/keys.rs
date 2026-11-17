//! Keys for the welcome, progress and main screens, and the F1 menu: its items and what each
//! one does.
use super::*;

impl App {
    pub(super) fn key(&mut self, k: KeyEvent) {
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
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
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

    pub(super) fn menu_items(&self) -> Vec<(Act, String, &'static str, bool)> {
        // (action, label, key hint, needs admin)
        let mut v = vec![(Act::Add, "Add a device".to_string(), "A", true)];
        if !self.cfg.links.is_empty() {
            v.push((Act::Edit, "Edit this device".into(), "Enter", true));
            let toggle = match self.cfg.links.get(self.sel) {
                Some(l) if !l.enabled => "Turn this device on",
                _ => "Turn this device off",
            };
            v.push((Act::Toggle, toggle.into(), "Space", true));
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
            "U",
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

    pub(super) fn filtered_menu(&self, filter: &str) -> Vec<(Act, String, &'static str, bool)> {
        let f = filter.to_lowercase();
        self.menu_items()
            .into_iter()
            .filter(|i| i.1.to_lowercase().contains(&f))
            .collect()
    }

    /// Turns a device off or on and applies it, like saving the device. Off keeps the device and
    /// its port; the service just doesn't connect to it.
    pub(super) fn toggle_device(&mut self, i: usize) {
        let mut new = self.cfg.clone();
        let link = &mut new.links[i];
        link.enabled = !link.enabled;
        let note = if link.enabled {
            format!(
                "{} is on: ComTap connects to {} again.",
                link.name, link.tcp
            )
        } else {
            format!(
                "{} is off: ComTap no longer connects to {}. {} stays for your software.",
                link.name, link.tcp, link.com
            )
        };
        self.start_plan(&setup::APPLY, After::Main, note, move |r| {
            setup::apply(&new, r)
        });
    }

    pub(super) fn open_name(&self, act: Act) -> String {
        match act {
            Act::Add => "add".into(),
            Act::Edit => format!("edit:{}", self.sel),
            _ => "menu".into(),
        }
    }

    pub(super) fn run_act(&mut self, act: Act, needs_admin: bool) {
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
            Act::Toggle if !self.cfg.links.is_empty() => self.toggle_device(self.sel),
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
            Act::Format => self.fmt = self.fmt.next(),
            Act::Quit => self.quit = true,
            _ => {}
        }
    }

    pub(super) fn key_main(&mut self, k: KeyEvent) {
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
            KeyCode::Char('u') | KeyCode::Char('U') => self.run_act(Act::CheckUpdates, false),
            KeyCode::Enter => self.run_act(if n == 0 { Act::Add } else { Act::Edit }, true),
            KeyCode::Char('a') | KeyCode::Char('A') => self.run_act(Act::Add, true),
            KeyCode::Char(' ') | KeyCode::Char('e') | KeyCode::Char('E') if n > 0 => {
                self.run_act(Act::Toggle, true)
            }
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete if n > 0 => {
                self.run_act(Act::Delete, true)
            }
            _ => {}
        }
    }
}
