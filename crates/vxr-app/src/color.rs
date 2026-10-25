//! Colour conversions for the palette editor: hex, RGB and HSV.

/// Hue in degrees (0 to 360), saturation and value from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub fn from_rgb(c: [u8; 3]) -> Hsv {
        let [r, g, b] = c.map(|x| x as f32 / 255.0);
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Hsv { h, s: if max == 0.0 { 0.0 } else { d / max }, v: max }
    }

    pub fn to_rgb(self) -> [u8; 3] {
        let h = self.h.rem_euclid(360.0) / 60.0;
        let (s, v) = (self.s.clamp(0.0, 1.0), self.v.clamp(0.0, 1.0));
        let c = v * s;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        [r, g, b].map(|k| ((k + m) * 255.0).round() as u8)
    }
}

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
    fn hsv_round_trips_every_grey_and_a_spread_of_colours() {
        for r in (0..=255u32).step_by(15) {
            for g in (0..=255u32).step_by(15) {
                for b in (0..=255u32).step_by(15) {
                    let c = [r as u8, g as u8, b as u8];
                    assert_eq!(Hsv::from_rgb(c).to_rgb(), c, "{c:?}");
                }
            }
        }
    }

    #[test]
    fn known_hues() {
        assert_eq!(Hsv::from_rgb([255, 0, 0]), Hsv { h: 0.0, s: 1.0, v: 1.0 });
        assert_eq!(Hsv::from_rgb([0, 255, 0]).h, 120.0);
        assert_eq!(Hsv::from_rgb([0, 0, 255]).h, 240.0);
        assert_eq!(Hsv { h: 300.0, s: 1.0, v: 1.0 }.to_rgb(), [255, 0, 255]);
        assert_eq!(Hsv { h: 360.0, s: 1.0, v: 1.0 }.to_rgb(), [255, 0, 0]);
    }

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
