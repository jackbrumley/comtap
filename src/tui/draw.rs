//! Drawing every screen, in RawTap's style.
use super::traffic_view::entry_lines;
use super::*;

pub(super) fn badge(t: &str, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {t} "),
        Style::new()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}

fn title(t: &str) -> Line<'static> {
    Line::from(vec![badge("ComTap", ACCENT), Span::raw(format!(" {t} "))])
}

fn hints(t: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {t} "), DIM))
}

impl App {
    // ---------------------------------------------------------------- drawing

    pub(super) fn draw(&self, fr: &mut Frame) {
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
            Line::from(vec![
                Span::styled("     1  ", Style::new().fg(ACCENT)),
                "Enter the device's IP address and TCP port".into(),
            ]),
            Line::from(vec![
                Span::styled("     2  ", Style::new().fg(ACCENT)),
                "Pick the COM port your software will open".into(),
            ]),
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
            lines.push(Line::from(Span::styled(
                format!("   {m}"),
                Style::new().fg(Color::LightRed),
            )));
        }
        let h = lines.len() as u16 + 3;
        let area = top_box(fr.area(), 80, h);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title("TCP device to COM port"))
            .title_bottom(hints("Enter continue  Esc quit"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
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

    fn draw_progress(&self, fr: &mut Frame, p: &Progress) {
        let w = 84.min(fr.area().width);
        let text_w = w.saturating_sub(12) as usize;
        let mut lines: Vec<Line> = vec!["".into()];
        let spin = SPIN[(p.since.elapsed().as_millis() / 150) as usize % 4];
        for (i, name) in p.plan.steps.iter().enumerate() {
            let (icon, icon_style, name_style, detail, detail_style) = match &p.states[i] {
                St::Pending => (" ", DIM, DIM, "", DIM),
                St::Running => (
                    spin,
                    Style::new().fg(Color::Yellow),
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    "",
                    DIM,
                ),
                St::Done(d) => (
                    OK,
                    Style::new().fg(Color::Green),
                    Style::new(),
                    d.as_str(),
                    DIM,
                ),
                St::Warn(d) => (
                    "!",
                    Style::new().fg(Color::Yellow),
                    Style::new().fg(Color::Yellow),
                    d.as_str(),
                    Style::new().fg(Color::Yellow),
                ),
                St::Skip(d) => ("-", DIM, DIM, d.as_str(), DIM),
                St::Fail(d) => (
                    BAD,
                    Style::new().fg(Color::LightRed),
                    Style::new().fg(Color::LightRed),
                    d.as_str(),
                    Style::new().fg(Color::LightRed),
                ),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {icon}  "),
                    icon_style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(name.to_string(), name_style),
            ]));
            if !detail.is_empty() {
                for r in wrap_words(detail, text_w) {
                    lines.push(Line::from(Span::styled(format!("      {r}"), detail_style)));
                }
            }
            if matches!(p.states[i], St::Running) && name.starts_with("Create the virtual") {
                let hint =
                    "The first time, Windows can ask to install the com0com driver: click Install.";
                for r in wrap_words(hint, text_w) {
                    lines.push(Line::from(Span::styled(
                        format!("      {r}"),
                        Style::new().fg(Color::Yellow),
                    )));
                }
            }
        }
        lines.push("".into());
        match &p.result {
            None => lines.push(Line::from(Span::styled(
                "   Working, please wait…",
                Style::new().fg(Color::Yellow),
            ))),
            Some(Ok(())) => {
                lines.push(Line::from(Span::styled(
                    "   Done.",
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                )));
                if !p.success_note.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("   {}", p.success_note),
                        Style::new().fg(Color::Green),
                    )));
                }
                lines.push("".into());
                lines.push(Line::from(vec![
                    "   Press ".into(),
                    badge("Enter", Color::Green),
                    " to continue".into(),
                ]));
            }
            Some(Err(_)) => {
                lines.push(Line::from(Span::styled(
                    "   Stopped at the step marked ×. Nothing after it was done.",
                    Style::new()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push("".into());
                let back = if matches!(p.after, After::FormOnFail { .. }) {
                    " to go back and fix it"
                } else {
                    " to go back"
                };
                lines.push(Line::from(vec![
                    "   Press ".into(),
                    badge("Enter", Color::Green),
                    back.into(),
                ]));
            }
        }
        let area = top_box(fr.area(), w, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title(p.plan.title))
            .title_bottom(hints(if p.result.is_some() {
                "Enter continue"
            } else {
                "please wait"
            }))
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
        if let Some(u) = self.update.as_ref().filter(|u| u.update_available) {
            spans.push(Span::styled(
                format!("  update {} (F1)", u.latest_version),
                Style::new().fg(Color::Green),
            ));
        }
        let right = if self.admin {
            badge("ADMIN", Color::Green)
        } else {
            badge("VIEW ONLY · F2 admin", Color::Yellow)
        };
        // Narrow window: the driver state is the first thing to go.
        if spans.iter().map(|s| s.width()).sum::<usize>() + right.width() + 1 > head.width as usize
        {
            spans.remove(3);
        }
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let gap = (head.width as usize).saturating_sub(used + right.width());
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(right);
        fr.render_widget(Paragraph::new(Line::from(spans)), head);

        // Devices
        let mut lines: Vec<Line> = Vec::new();
        if let Some(n) = &self.cfg_note {
            lines.push(Line::from(Span::styled(
                format!(" {n}"),
                Style::new().fg(Color::LightRed),
            )));
        } else if self.cfg.links.is_empty() {
            lines.push(Line::from(Span::styled(
                " No devices yet. Press A to add one.",
                Style::new().fg(Color::Yellow),
            )));
        } else if !self.installed {
            lines.push(Line::from(Span::styled(
                " ComTap is not installed. Press F1 and choose Install.",
                Style::new().fg(Color::Yellow),
            )));
        }
        for (i, l) in self.cfg.links.iter().enumerate() {
            let st = self.link_status(l);
            let unopened =
                st.is_some_and(|s| s.state == "connected" && s.software_open == Some(false));
            let (state, color) = match st.map(|s| s.state.as_str()) {
                Some("connected") if unopened => (format!("waiting for {}", l.com), Color::Yellow),
                Some("connected") => ("connected".into(), Color::Green),
                Some(s @ ("retrying" | "connecting" | "starting")) => (s.into(), Color::Yellow),
                Some(s) => (s.into(), Color::LightRed),
                None => ("service not running".into(), Color::DarkGray),
            };
            let selected = i == self.sel;
            let mut spans = vec![
                Span::styled(
                    if selected {
                        format!(" {MARK} ")
                    } else {
                        "   ".into()
                    },
                    Style::new().fg(ACCENT),
                ),
                Span::styled("● ", Style::new().fg(color)),
                Span::styled(
                    format!("{:<20}", trunc(&l.name, 20)),
                    if selected {
                        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    },
                ),
                Span::styled(
                    format!("{:<6}", l.com),
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {BOTH} "), DIM),
                Span::raw(format!("{:<22} ", l.tcp)),
                Span::styled(format!("{state:<12}"), Style::new().fg(color)),
            ];
            if let Some(s) = st {
                if let Some(t) = &s.last_rx {
                    spans.push(Span::styled(
                        format!("  last data {}", t.get(11..).unwrap_or(t)),
                        DIM,
                    ));
                }
            }
            lines.push(Line::from(spans));
            if selected && unopened {
                lines.push(Line::from(Span::styled(
                    format!(
                        "       No software has {} open. Data from the device is dropped until one does.",
                        l.com
                    ),
                    Style::new().fg(Color::Yellow),
                )));
            } else if selected {
                if let Some(e) = st.and_then(|s| s.last_error.clone()) {
                    lines.push(Line::from(Span::styled(
                        format!("       {e}"),
                        Style::new().fg(Color::LightRed),
                    )));
                }
            }
        }
        let block = Block::bordered()
            .title(" Devices ")
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), devs);

        self.draw_traffic(fr, tr);

        let m = match &self.msg {
            Some((true, m)) => Line::from(Span::styled(
                format!(" {m}"),
                Style::new().fg(Color::LightRed),
            )),
            Some((false, m)) => {
                Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::Green)))
            }
            None if !self.admin => Line::from(Span::styled(
                " View only. Changes need admin: press F2.",
                DIM,
            )),
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
                    vec![
                        format!("Delete '{}'?", l.name).into(),
                        format!("Its virtual port {} will be removed.", l.com).into(),
                    ],
                    "delete",
                )
            }
            Overlay::Update { check, sel } => self.draw_update_prompt(fr, check, *sel),
            Overlay::Notice { title, lines } => self.draw_notice(fr, title, lines),
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
            Span::styled(
                if filter.is_empty() {
                    "type to search".to_string()
                } else {
                    filter.to_string()
                },
                if filter.is_empty() {
                    DIM
                } else {
                    Style::new().fg(Color::White)
                },
            ),
        ]));
        lines.push("".into());
        for (i, (_, label, key, admin)) in items.iter().enumerate() {
            let focused = i == sel;
            let lock = if *admin && !self.admin {
                " (admin)"
            } else {
                ""
            };
            let text = format!("{label}{lock}");
            let mut spans = vec![
                Span::styled(
                    if focused {
                        format!(" {MARK} ")
                    } else {
                        "   ".into()
                    },
                    Style::new().fg(ACCENT),
                ),
                Span::styled(
                    format!("{text:<38}"),
                    if focused {
                        Style::new().fg(Color::Black).bg(ACCENT)
                    } else {
                        Style::new()
                    },
                ),
            ];
            spans.push(Span::styled(format!(" {key:>5}"), DIM));
            lines.push(Line::from(spans));
        }
        if items.is_empty() {
            lines.push(Line::from(Span::styled("   nothing matches", DIM)));
        }
        let area = Rect {
            y: fr.area().y + 2,
            ..top_box(fr.area(), 54, lines.len() as u16 + 2)
        };
        let area = area.intersection(fr.area());
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(title("Menu"))
            .title_bottom(hints("↑↓ move  Enter run  ←→ change  Esc close"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_traffic(&self, fr: &mut Frame, area: Rect) {
        let name = self
            .cfg
            .links
            .get(self.sel)
            .map(|l| l.name.as_str())
            .unwrap_or("-");
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
                Line::from(Span::styled(
                    " scrolled · End to follow ",
                    Style::new().fg(Color::Yellow),
                ))
                .right_aligned(),
            );
        }
        fr.render_widget(Paragraph::new(rows[start..end].to_vec()).block(block), area);
    }
}

/// A box centred both ways.
pub(super) fn centered_box(full: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(full.width);
    let h = h.min(full.height);
    Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + (full.height - h) / 2,
        width: w,
        height: h,
    }
}

/// A box centred across, pinned near the top (like RawTap's setup box).
fn top_box(full: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(full.width);
    let h = h.min(full.height);
    let top = if full.height >= h + 2 { 1 } else { 0 };
    Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + top,
        width: w,
        height: h.min(full.height - top),
    }
}

fn confirm(fr: &mut Frame, heading: &str, color: Color, body: Vec<Line<'static>>, what: &str) {
    let mut lines = vec!["".into()];
    lines.extend(body.into_iter().map(|l| {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(l.spans);
        Line::from(spans)
    }));
    lines.push("".into());
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(30) as u16 + 4)
        .max(44)
        .min(fr.area().width);
    let h = (lines.len() as u16 + 2).min(fr.area().height);
    let a = fr.area();
    let area = Rect {
        x: a.x + (a.width - w) / 2,
        y: a.y + (a.height - h) / 2,
        width: w,
        height: h,
    };
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
    fr.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// Greedy word wrap.
pub(super) fn wrap_words(text: &str, width: usize) -> Vec<String> {
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
