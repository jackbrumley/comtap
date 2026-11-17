//! The main screen's device list: one row per device with its state, and a hint under the
//! selected one (its last error, "waiting for" software, or how to turn it back on).
use super::*;

impl App {
    pub(super) fn draw_devices(&self, fr: &mut Frame, devs: Rect) {
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
                _ if !l.enabled => ("off".into(), Color::DarkGray),
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
                Span::styled(if l.enabled { "● " } else { "○ " }, Style::new().fg(color)),
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
            if selected && !l.enabled {
                lines.push(Line::from(Span::styled(
                    format!(
                        "       Off: ComTap doesn't connect to it, and {} stays for your software. Space turns it on.",
                        l.com
                    ),
                    DIM,
                )));
            } else if selected && unopened {
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
    }
}
