//! The device form: adding or editing a device (name, IP, TCP port, the port the software
//! opens), its keys, the connection test, and drawing it.
use super::draw::{badge, hints, title, top_box, wrap_words};
use super::*;

pub(super) const F_NAME: usize = 0;
pub(super) const F_HOST: usize = 1;
pub(super) const F_PORT: usize = 2;
pub(super) const F_COM: usize = 3;
pub(super) const LABELS: [&str; 4] = ["Name", "Device IP", "TCP port", words::PORT_LABEL];
pub(super) const HELP: [&str; 4] = [
    "A label for this device, shown on this screen and in the logs. For example: Dock 3 scanner.",
    "The device's IP address or network name. ComTap connects to it as a TCP client.",
    "The TCP port the device sends its data on. You find it in the device's network settings.",
    words::PORT_HELP,
];

pub(super) struct Form {
    /// None = a new device.
    pub(super) idx: Option<usize>,
    pub(super) fields: [String; 4],
    pub(super) focus: usize,
    pub(super) coms: Vec<String>,
    /// True when ComTap isn't installed yet: Enter installs.
    pub(super) install: bool,
    /// Something ComTap changed for you, e.g. a reserved COM port swapped for a free one.
    pub(super) note: Option<String>,
    pub(super) err: Option<String>,
    pub(super) test: Option<Receiver<(bool, String)>>,
    pub(super) test_msg: Option<(bool, String)>,
}

impl App {
    pub(super) fn form(&self, idx: Option<usize>) -> Form {
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

    pub(super) fn key_form(&mut self, mut f: Form, k: KeyEvent) {
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
                        // Editing keeps the device on or off; a new device starts on.
                        enabled: f
                            .idx
                            .and_then(|i| self.cfg.links.get(i))
                            .is_none_or(|l| l.enabled),
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

    pub(super) fn draw_form(&self, fr: &mut Frame, f: &Form) {
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
            let label_style = if focused {
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            let val_style = if focused {
                Style::new().fg(Color::Black).bg(ACCENT)
            } else {
                Style::new().fg(Color::White)
            };
            let mut spans = vec![
                Span::styled(
                    if focused {
                        format!(" {MARK} ")
                    } else {
                        "   ".into()
                    },
                    Style::new().fg(ACCENT),
                ),
                Span::styled(format!("{label:<14}"), label_style),
            ];
            let value = &f.fields[i];
            if i == F_COM {
                if focused {
                    spans.push(Span::styled(format!("{LEFT} "), DIM));
                }
                let shown = if value.is_empty() {
                    "none free".to_string()
                } else {
                    value.clone()
                };
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
                let style = if value.is_empty() && !focused {
                    DIM
                } else {
                    val_style
                };
                spans.push(Span::styled(format!(" {shown} "), style));
                if focused {
                    // (column, row) inside the box; placed once the box is sized.
                    cursor = Some((
                        3 + 14 + 1 + value.chars().count() as u16,
                        lines.len() as u16,
                    ));
                }
            }
            lines.push(Line::from(spans));
        }
        if let Some(n) = &f.note {
            lines.push("".into());
            for r in wrap_words(n, inner_w.saturating_sub(6) as usize) {
                lines.push(Line::from(Span::styled(
                    format!("   {r}"),
                    Style::new().fg(Color::Yellow),
                )));
            }
        }
        lines.push("".into());
        lines.push(Line::from(Span::styled(
            format!("   {}", "─".repeat(inner_w.saturating_sub(6) as usize)),
            DIM,
        )));
        let mut rows = wrap_words(HELP[f.focus], inner_w.saturating_sub(6) as usize);
        rows.resize(2, String::new());
        for r in rows.into_iter().take(2) {
            lines.push(Line::from(Span::styled(
                format!("   {r}"),
                Style::new().fg(Color::Gray),
            )));
        }
        lines.push("".into());
        lines.push(match (&f.test, &f.test_msg) {
            (Some(_), _) => Line::from(Span::styled(
                "   Testing the connection…",
                Style::new().fg(Color::Yellow),
            )),
            (None, Some((true, m))) => Line::from(Span::styled(
                format!("   {OK} {m}"),
                Style::new().fg(Color::Green),
            )),
            (None, Some((false, m))) => Line::from(Span::styled(
                format!("   {BAD} {m}"),
                Style::new().fg(Color::LightRed),
            )),
            (None, None) => Line::from(Span::styled(
                "   Press F5 to test the connection to the device.",
                DIM,
            )),
        });
        if matches!(&f.test_msg, Some((false, _))) && self.svc == "Running" {
            lines.push(Line::from(Span::styled(
                "   (Some devices accept only one connection. If ComTap is connected, a test can fail.)",
                DIM,
            )));
        }
        lines.push("".into());
        lines.push(Line::from(vec![
            "   Press ".into(),
            badge("Enter", Color::Green),
            action.into(),
        ]));
        if let Some(e) = &f.err {
            lines.push("".into());
            for r in wrap_words(e, inner_w.saturating_sub(6) as usize) {
                lines.push(Line::from(Span::styled(
                    format!("   {r}"),
                    Style::new().fg(Color::LightRed),
                )));
            }
        }
        lines.push("".into());
        let area = top_box(fr.area(), 80, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title(heading))
            .title_bottom(hints(
                "↑↓ move  ←→ change  type to edit  F5 test  Enter save  Esc back",
            ))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
        if let Some((cx, cy)) = cursor.filter(|c| c.1 + 2 < area.height) {
            fr.set_cursor_position((area.x + 1 + cx, area.y + 1 + cy));
        }
    }
}

pub(super) fn host_of(tcp: &str) -> String {
    tcp.rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(tcp)
        .to_string()
}

pub(super) fn port_of(tcp: &str) -> String {
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
