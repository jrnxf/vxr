//! Colour conversions for the palette editor: hex, and HSV from `vxr_core::palette`.

pub use vxr_core::palette::Hsv;

/// `#RRGGBB`, upper case.
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Read `RRGGBB` or `RGB`, with or without `#`.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        6 => Some(std::array::from_fn(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())),
        3 => Some(std::array::from_fn(|i| u8::from_str_radix(&s[i..i + 1], 16).unwrap() * 17)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_reads_and_writes() {
        assert_eq!(hex([0x8e, 0xa2, 0xff]), "#8EA2FF");
        assert_eq!(parse_hex("#8ea2ff"), Some([0x8e, 0xa2, 0xff]));
        assert_eq!(parse_hex(" 8EA2FF "), Some([0x8e, 0xa2, 0xff]));
        assert_eq!(parse_hex("#fa0"), Some([255, 170, 0]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
    }
}
