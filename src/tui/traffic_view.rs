//! Traffic entries as screen rows, in RawTap's raw-log formats and colours.
use super::*;

/// Wraps tokens after a prefix, indenting continuation rows (RawTap's raw log layout).
fn wrap_tokens(
    prefix: String,
    tokens: Vec<String>,
    sep: &str,
    style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let indent = prefix.chars().count();
    let avail = width.saturating_sub(indent).max(8);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for t in tokens {
        let add = if cur.is_empty() {
            t.len()
        } else {
            sep.len() + t.len()
        };
        if !cur.is_empty() && cur.len() + add > avail {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str(sep);
        }
        cur.push_str(&t);
    }
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let p = if i == 0 {
                prefix.clone()
            } else {
                " ".repeat(indent)
            };
            Line::from(vec![Span::styled(p, DIM), Span::styled(r, style)])
        })
        .collect()
}

/// Screen rows for one traffic entry, in RawTap's formats and colours.
pub(super) fn entry_lines(l: &traffic::Line, width: u16, fmt: Fmt) -> Vec<Line<'static>> {
    let w = width as usize;
    match l {
        traffic::Line::Event { time, msg } => {
            let lower = msg.to_lowercase();
            let (mark, style) = if lower.starts_with("connected") {
                (OK, Style::new().fg(Color::Green))
            } else if lower.contains("disconnected")
                || lower.contains("cannot")
                || lower.contains("failed")
                || lower.contains("connect ")
            {
                (BAD, Style::new().fg(Color::LightRed))
            } else {
                ("·", Style::new().fg(Color::Gray))
            };
            let prefix = format!("{time} {mark} ");
            let body = trunc(msg, w.saturating_sub(prefix.chars().count()).max(1));
            vec![Line::from(vec![
                Span::styled(prefix, DIM),
                Span::styled(body, style),
            ])]
        }
        traffic::Line::Data { time, dir, bytes } => {
            let rx = *dir == Dir::Rx;
            let tag = if rx { "RX" } else { "TX" };
            let (hex_style, text_style) = if rx {
                (Style::new().fg(Color::Cyan), Style::new().fg(Color::White))
            } else {
                (
                    Style::new().fg(Color::Yellow),
                    Style::new().fg(Color::LightYellow),
                )
            };
            if fmt == Fmt::Compact {
                let prefix = format!("{time} {tag} ");
                let avail = w.saturating_sub(prefix.len());
                let hex = codec::hex_string(bytes);
                let hw = (avail / 2).max(8);
                let text = codec::text_string(bytes);
                return vec![Line::from(vec![
                    Span::styled(prefix, DIM),
                    Span::styled(format!("{:<hw$}", trunc(&hex, hw)), hex_style),
                    Span::styled(" │ ", DIM),
                    Span::styled(
                        trunc(&text, avail.saturating_sub(hw + 3).max(1)),
                        text_style,
                    ),
                ])];
            }
            let mut out = Vec::new();
            if fmt != Fmt::Text {
                let toks = bytes.iter().map(|b| format!("{b:02X}")).collect();
                out.extend(wrap_tokens(
                    format!("{time} {tag} HEX  "),
                    toks,
                    " ",
                    hex_style,
                    w,
                ));
            }
            if fmt != Fmt::Hex {
                let toks = bytes.iter().map(|&b| codec::byte_text(b)).collect();
                out.extend(wrap_tokens(
                    format!("{time} {tag} TEXT "),
                    toks,
                    "",
                    text_style,
                    w,
                ));
            }
            out
        }
    }
}
