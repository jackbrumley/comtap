//! Update checks and the "Update available" dialog (Release Standard section 7, as in RawTap):
//! current → latest with **Later**, **Release notes** and **Update now**. A check the user
//! asked for that finds nothing new says "ComTap is up to date".
//!
//! The dialog opens by itself once a run, on the main screen: at once if nothing else is open,
//! otherwise as soon as the screen is free. U on the main screen, or F1 → Check for updates,
//! checks by hand.
use super::draw::{badge, centered_box, wrap_words};
use super::*;
use crate::update::{self, UpdateCheck};

pub(super) const BUTTONS: [&str; 3] = ["Later", "Release notes", "Update now"];

impl App {
    /// Starts a background update check. `manual`: the user asked, so say the result either way.
    pub(super) fn start_update_check(&mut self, manual: bool) {
        self.update_rx = Some(update::spawn_check());
        self.update_manual = manual;
        if manual {
            self.say("Checking for updates…");
        }
    }

    /// Opens a waiting update dialog once the main screen is free (no form, setup steps or
    /// other dialog), so a check that finishes while the user is busy still offers it.
    pub(super) fn show_update_offer(&mut self) {
        if !matches!(self.screen, Screen::Main) || !matches!(self.overlay, Overlay::None) {
            return;
        }
        if let Some(check) = self.update_offer.take() {
            self.overlay = Overlay::Update { check, sel: 2 };
        }
    }

    /// Picks up a finished update check.
    pub(super) fn poll_update_check(&mut self) {
        let Some(rx) = &self.update_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.update_rx = None;
        self.last_checked = Some(chrono::Local::now().format("%H:%M").to_string());
        match result {
            Ok(check) => {
                let available = check.update_available;
                self.update = Some(check.clone());
                if available && (self.update_manual || !self.update_prompted) {
                    self.update_prompted = true;
                    self.update_offer = Some(check);
                    self.show_update_offer();
                } else if self.update_manual {
                    self.overlay = Overlay::Notice {
                        title: "ComTap is up to date".into(),
                        lines: vec![format!(
                            "You are already on the latest version, {}.",
                            check.current_version
                        )],
                    };
                }
            }
            Err(e) if self.update_manual => self.fail(e),
            Err(_) => {} // background check: already logged
        }
        self.update_manual = false;
    }

    pub(super) fn key_update_prompt(&mut self, check: UpdateCheck, sel: usize, k: KeyEvent) {
        let pressed = match k.code {
            KeyCode::Left | KeyCode::BackTab => {
                self.overlay = Overlay::Update {
                    check,
                    sel: (sel + 2) % 3,
                };
                return;
            }
            KeyCode::Right | KeyCode::Tab => {
                self.overlay = Overlay::Update {
                    check,
                    sel: (sel + 1) % 3,
                };
                return;
            }
            KeyCode::Esc => 0,
            KeyCode::Enter => sel,
            _ => {
                self.overlay = Overlay::Update { check, sel };
                return;
            }
        };
        match pressed {
            1 => {
                if let Err(e) = update::open_url(&check.release_url) {
                    self.fail(e);
                }
                self.overlay = Overlay::Update { check, sel };
            }
            2 => self.begin_update(),
            _ => self.say(format!(
                "Update {} is waiting in the F1 menu.",
                check.latest_version
            )),
        }
    }

    /// Leaves the screen so `run` can start the installer in its own window.
    pub(super) fn begin_update(&mut self) {
        self.start_update = true;
        self.quit = true;
    }

    pub(super) fn draw_update_prompt(&self, fr: &mut Frame, check: &UpdateCheck, sel: usize) {
        let mut buttons = vec![Span::raw("   ")];
        for (i, b) in BUTTONS.iter().enumerate() {
            if i == sel {
                buttons.push(badge(b, ACCENT));
            } else {
                buttons.push(Span::styled(format!(" {b} "), Style::new().fg(Color::Gray)));
            }
            buttons.push(Span::raw("  "));
        }
        let lines: Vec<Line> = vec![
            "".into(),
            Line::from(vec![
                "   ComTap ".into(),
                Span::styled(check.current_version.clone(), Style::new().fg(Color::White)),
                Span::styled(format!(" {MARK} "), DIM),
                Span::styled(
                    check.latest_version.clone(),
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                " is available.".into(),
            ]),
            "".into(),
            words::UPDATE_LINES[0].into(),
            words::UPDATE_LINES[1].into(),
            "".into(),
            Line::from(buttons),
            "".into(),
        ];
        let area = centered_box(fr.area(), 78, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(Line::from(badge("Update available", Color::Green)))
            .title_bottom(Line::from(Span::styled(
                " ←→ choose  Enter select  Esc later ",
                DIM,
            )))
            .border_style(Style::new().fg(Color::Green));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub(super) fn draw_notice(&self, fr: &mut Frame, title: &str, body: &[String]) {
        let w = 78.min(fr.area().width);
        let mut lines: Vec<Line> = vec!["".into()];
        for l in body {
            for r in wrap_words(l, w.saturating_sub(8) as usize) {
                lines.push(Line::from(format!("   {r}")));
            }
        }
        lines.push("".into());
        let area = centered_box(fr.area(), w, lines.len() as u16 + 2);
        fr.render_widget(Clear, area);
        let block = Block::bordered()
            .title(Line::from(badge(title, ACCENT)))
            .title_bottom(Line::from(vec![
                badge("Enter", Color::Green),
                Span::raw(" close "),
            ]))
            .border_style(Style::new().fg(ACCENT));
        fr.render_widget(Paragraph::new(lines).block(block), area);
    }
}
