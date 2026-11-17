//! The device list (the start screen) and a device's page, with each device's state in plain
//! words: what is happening, and what to do about it.
use super::draw::wrap_words;
use super::*;

/// The device page's items, in order.
pub(super) const DEVICE_ITEMS: [&str; 4] =
    ["Watch data", "Edit settings", "Delete this device", "Back"];
pub(super) const DEVICE_WATCH: usize = 0;
pub(super) const DEVICE_EDIT: usize = 1;
pub(super) const DEVICE_DELETE: usize = 2;
pub(super) const DEVICE_BACK: usize = 3;

/// A device's state for people: a short label for the list, its colour, and a sentence or two
/// for its page.
pub(super) struct Plain {
    pub(super) label: String,
    pub(super) color: Color,
    pub(super) explain: String,
}

impl App {
    pub(super) fn plain_state(&self, l: &Link) -> Plain {
        let st = self.link_status(l);
        let error = st
            .and_then(|s| s.last_error.clone())
            .map(|e| format!(" Last error: {e}."))
            .unwrap_or_default();
        let plain = |label: &str, color: Color, explain: String| Plain {
            label: label.into(),
            color,
            explain,
        };
        if !l.enabled {
            return plain(
                "Off",
                Color::DarkGray,
                format!(
                    "Off. ComTap doesn't connect to the device, so its connection is free. {} stays for your software. To turn it on, choose Edit settings.",
                    l.com
                ),
            );
        }
        let Some(s) = st else {
            return if self.installed {
                plain(
                    "Service not running",
                    Color::DarkGray,
                    "The ComTap service isn't running, so nothing is connected. Press F1 and choose Start the service.".into(),
                )
            } else {
                plain(
                    "Not installed",
                    Color::Yellow,
                    "ComTap isn't installed yet. Press F1 and choose Install ComTap.".into(),
                )
            };
        };
        match s.state.as_str() {
            "connected" if s.software_open == Some(false) => plain(
                "Waiting for your software",
                Color::Yellow,
                format!(
                    "Connected to the device, but no software has {} open. Open {} in your software; until then, data from the device is dropped.",
                    l.com, l.com
                ),
            ),
            "connected" => plain(
                "Connected",
                Color::Green,
                format!("Connected. Your software reads the device on {}.", l.com),
            ),
            "starting" | "connecting" => plain(
                "Connecting",
                Color::Yellow,
                "Connecting to the device…".into(),
            ),
            "retrying" => plain(
                "Can't reach the device",
                Color::Yellow,
                format!(
                    "Can't reach the device, so ComTap keeps trying. Check it is on and connected to the network, and that its IP address and TCP port are right (Edit settings).{error}"
                ),
            ),
            "com-error" => plain(
                "Port problem",
                Color::LightRed,
                format!(
                    "ComTap can't open its side of {}. Press F1 and choose {}.{error}",
                    l.com,
                    words::REPAIR_PORTS
                ),
            ),
            other => plain(other, Color::LightRed, error.trim().to_string()),
        }
    }

    /// The start screen: "+ Add a device", then one row per device.
    pub(super) fn draw_list(&self, fr: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = vec!["".into()];
        if let Some(n) = &self.cfg_note {
            lines.push(Line::from(Span::styled(
                format!("   {n}"),
                Style::new().fg(Color::LightRed),
            )));
            lines.push("".into());
        }
        lines.push(self.list_row(
            0,
            Span::styled("+ Add a device", Style::new().fg(ACCENT)),
            vec![],
        ));
        for (i, l) in self.cfg.links.iter().enumerate() {
            let p = self.plain_state(l);
            let selected = self.row == i + 1;
            let dot = if l.enabled { "● " } else { "○ " };
            lines.push(self.list_row(
                i + 1,
                Span::styled(
                    format!("{:<24}", trunc(&l.name, 24)),
                    if selected {
                        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    },
                ),
                vec![
                    Span::styled(format!("{:<12}", l.com), Style::new().fg(Color::White)),
                    Span::styled(dot, Style::new().fg(p.color)),
                    Span::styled(p.label, Style::new().fg(p.color)),
                ],
            ));
        }
        if self.cfg.links.is_empty() {
            lines.push("".into());
            for r in wrap_words(words::NO_DEVICES, area.width.saturating_sub(8) as usize) {
                lines.push(Line::from(Span::styled(format!("   {r}"), DIM)));
            }
        }
        let block = Block::bordered()
            .title(" Devices ")
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn list_row(
        &self,
        row: usize,
        first: Span<'static>,
        rest: Vec<Span<'static>>,
    ) -> Line<'static> {
        let mut spans = vec![Span::styled(
            if self.row == row {
                format!(" {MARK} ")
            } else {
                "   ".into()
            },
            Style::new().fg(ACCENT),
        )];
        spans.push(first);
        spans.extend(rest);
        Line::from(spans)
    }

    /// A device's page: its state in plain words, its settings, and what you can do.
    pub(super) fn draw_device(&self, fr: &mut Frame, area: Rect, idx: usize, sel: usize) {
        let Some(l) = self.cfg.links.get(idx) else {
            return;
        };
        let p = self.plain_state(l);
        let text_w = area.width.saturating_sub(6) as usize;
        let mut lines: Vec<Line> = vec!["".into()];
        for r in wrap_words(&p.explain, text_w) {
            lines.push(Line::from(Span::styled(
                format!("  {r}"),
                Style::new().fg(p.color),
            )));
        }
        lines.push("".into());
        let last = self
            .link_status(l)
            .and_then(|s| s.last_rx.clone())
            .map(|t| t.get(11..).unwrap_or(&t).to_string())
            .unwrap_or_else(|| "none yet".into());
        for (label, value) in [
            (
                "Device",
                format!("{}, TCP port {}", host_of(&l.tcp), port_of(&l.tcp)),
            ),
            (words::PORT_LABEL, ports::software_path(&l.com)),
            ("Last data", last),
        ] {
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<12}"), DIM),
                Span::styled(value, Style::new().fg(Color::White)),
            ]));
        }
        lines.push("".into());
        for (i, item) in DEVICE_ITEMS.iter().enumerate() {
            let focused = i == sel;
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
                    format!(" {item} "),
                    if focused {
                        Style::new().fg(Color::Black).bg(ACCENT)
                    } else {
                        Style::new()
                    },
                ),
            ]));
        }
        let block = Block::bordered()
            .title(format!(" {} ", l.name))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }
}
