//! Drawing the welcome, progress and browsing screens (list, device page, watch data) and
//! the dialogs over them, in RawTap's style.
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

pub(super) fn title(t: &str) -> Line<'static> {
    Line::from(vec![badge("ComTap", ACCENT), Span::raw(format!(" {t} "))])
}

pub(super) fn hints(t: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {t} "), DIM))
}

impl App {
    // ---------------------------------------------------------------- drawing

    pub(super) fn draw(&self, fr: &mut Frame) {
        match &self.screen {
            Screen::Welcome => self.draw_welcome(fr),
            Screen::Form(f) => self.draw_form(fr, f),
            Screen::Progress(p) => self.draw_progress(fr, p),
            Screen::Main | Screen::Device { .. } | Screen::Monitor { .. } => self.draw_browse(fr),
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
                words::PICK_PORT.into(),
            ]),
            Line::from(vec![
                Span::styled("     3  ", Style::new().fg(ACCENT)),
                words::STARTS.into(),
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
            lines.push(words::SETUP_NEEDS_ADMIN.into());
            lines.push("".into());
            lines.push(Line::from(vec![
                "   Press ".into(),
                badge("Enter", Color::Green),
                words::SETUP_CONTINUE.into(),
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
            if cfg!(windows)
                && matches!(p.states[i], St::Running)
                && name.starts_with("Create the virtual")
            {
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

    /// The list, a device's page or its data: header, body, message line, key line, and any
    /// dialog over them.
    fn draw_browse(&self, fr: &mut Frame) {
        let [head, body, msg, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(fr.area());
        self.draw_header(fr, head);
        let keys = match self.screen {
            Screen::Device { idx, sel } => {
                self.draw_device(fr, body, idx, sel);
                "↑↓ choose  Enter open  Esc back  F1 menu"
            }
            Screen::Monitor { idx } => {
                self.draw_monitor(fr, body, idx);
                "←→ format  PgUp/PgDn scroll  Esc back  F1 menu"
            }
            _ => {
                self.draw_list(fr, body);
                "↑↓ choose  Enter open  F1 menu"
            }
        };
        let m = match &self.msg {
            Some((true, m)) => Line::from(Span::styled(
                format!(" {m}"),
                Style::new().fg(Color::LightRed),
            )),
            Some((false, m)) => {
                Line::from(Span::styled(format!(" {m}"), Style::new().fg(Color::Green)))
            }
            None if !self.admin => Line::from(Span::styled(words::VIEW_ONLY, DIM)),
            None => Line::from(""),
        };
        fr.render_widget(Paragraph::new(m), msg);
        fr.render_widget(Paragraph::new(hints(keys)), foot);
        self.draw_overlay(fr);
    }

    /// Like RawTap's: name badge, service state, driver; the version and admin badge on the right.
    fn draw_header(&self, fr: &mut Frame, head: Rect) {
        let (state, bg, fg) = match self.svc.as_str() {
            "Running" => ("● SERVICE RUNNING", Color::Green, Color::Black),
            "not installed" => ("NOT INSTALLED", Color::Yellow, Color::Black),
            _ => ("○ SERVICE STOPPED", Color::Red, Color::White),
        };
        let mut spans = vec![
            badge("ComTap", ACCENT),
            Span::raw(" "),
            Span::styled(format!(" {state} "), Style::new().fg(fg).bg(bg)),
            Span::styled(format!("  {} {}", words::DRIVER_LABEL, self.drv), DIM),
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
            badge(words::VIEW_ONLY_BADGE, Color::Yellow)
        };
        let mut version = Some(Span::styled(
            concat!("v", env!("CARGO_PKG_VERSION"), " "),
            DIM,
        ));
        let right_width =
            |version: &Option<Span>| right.width() + version.as_ref().map_or(0, Span::width);
        let fits = |spans: &[Span], version: &Option<Span>| {
            spans.iter().map(Span::width).sum::<usize>() + right_width(version)
                < head.width as usize
        };
        // Narrow window: the driver state is the first thing to go, then the version.
        if !fits(&spans, &version) {
            spans.remove(3);
        }
        if !fits(&spans, &version) {
            version = None;
        }
        let used: usize = spans.iter().map(Span::width).sum();
        let gap = (head.width as usize).saturating_sub(used + right_width(&version));
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(version);
        spans.push(right);
        fr.render_widget(Paragraph::new(Line::from(spans)), head);
    }

    fn draw_overlay(&self, fr: &mut Frame) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Menu { sel, filter } => self.draw_menu(fr, *sel, filter),
            Overlay::Admin { .. } => confirm(
                fr,
                words::ADMIN_TITLE,
                Color::Yellow,
                words::ADMIN_LINES.iter().map(|l| Line::from(*l)).collect(),
                words::ADMIN_BUTTON,
            ),
            Overlay::ConfirmDelete(i) => {
                let Some(l) = self.cfg.links.get(*i) else {
                    return;
                };
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
                words::UNINSTALL_LINES
                    .iter()
                    .map(|l| Line::from(*l))
                    .collect(),
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
        for (i, (_, label, admin)) in items.iter().enumerate() {
            let focused = i == sel;
            let lock = if *admin && !self.admin {
                " (admin)"
            } else {
                ""
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if focused {
                        format!(" {MARK} ")
                    } else {
                        "   ".into()
                    },
                    Style::new().fg(ACCENT),
                ),
                Span::styled(
                    format!("{:<44}", format!("{label}{lock}")),
                    if focused {
                        Style::new().fg(Color::Black).bg(ACCENT)
                    } else {
                        Style::new()
                    },
                ),
            ]));
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
            .title_bottom(hints("↑↓ choose  Enter run  Esc close"))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    /// Watch data: one device's traffic, newest at the bottom.
    fn draw_monitor(&self, fr: &mut Frame, area: Rect, idx: usize) {
        let name = self
            .cfg
            .links
            .get(idx)
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
            .title(format!(" Watch data · {name} · {} ", self.fmt.name()))
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
pub(super) fn top_box(full: Rect, w: u16, h: u16) -> Rect {
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
