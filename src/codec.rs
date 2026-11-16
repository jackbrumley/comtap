//! Byte formatting (hex / control-char text), the same as RawTap's.

const CTRL: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US",
];

/// Text form of one byte: printable ASCII as-is, control chars as `<CR>`, others as `<0xFF>`.
pub fn byte_text(b: u8) -> String {
    match b {
        0x00..=0x1F => format!("<{}>", CTRL[b as usize]),
        0x7F => "<DEL>".to_string(),
        0x20..=0x7E => (b as char).to_string(),
        _ => format!("<0x{b:02X}>"),
    }
}

pub fn hex_string(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn text_string(data: &[u8]) -> String {
    data.iter().map(|&b| byte_text(b)).collect()
}

/// Parses "30 31 0D" back into bytes; stops at the first thing that isn't a hex pair.
pub fn parse_hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map_while(|t| u8::from_str_radix(t, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let d = b"AB\r\n\x02\xff";
        assert_eq!(text_string(d), "AB<CR><LF><STX><0xFF>");
        assert_eq!(parse_hex(&hex_string(d)), d);
    }
}
