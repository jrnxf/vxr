//! Text helpers: file names as data, colours as hex and as words.

/// Longest name shown to the model.
pub const MAX_NAME: usize = 64;

/// A name from a file or a model, as data: at most [`MAX_NAME`] characters, no control
/// characters (newlines included), trimmed.
pub fn clean(name: &[u8]) -> String {
    let s: String = String::from_utf8_lossy(name).chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    s.trim().to_string()
}

pub fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// `#rrggbb` or `#rrggbbaa`, the `#` optional.
pub fn parse_hex(s: &str) -> Result<[u8; 4], String> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return Err(format!("bad colour {s:?}"));
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| format!("bad colour {s:?}"));
    match h.len() {
        6 => Ok([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Ok([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => Err(format!("colour {s:?} is not #rrggbb")),
    }
}

const NAMES: [(&str, [u8; 3]); 30] = [
    ("black", [0, 0, 0]),
    ("dark grey", [64, 64, 64]),
    ("grey", [128, 128, 128]),
    ("light grey", [192, 192, 192]),
    ("white", [250, 250, 250]),
    ("red", [215, 35, 35]),
    ("dark red", [125, 20, 20]),
    ("pink", [250, 160, 185]),
    ("rose", [240, 85, 120]),
    ("orange", [240, 135, 30]),
    ("brown", [140, 85, 40]),
    ("dark brown", [88, 58, 40]),
    ("tan", [195, 155, 105]),
    ("beige", [235, 220, 180]),
    ("sand", [225, 200, 140]),
    ("yellow", [245, 220, 40]),
    ("light yellow", [255, 215, 110]),
    ("olive", [125, 125, 35]),
    ("light green", [140, 215, 100]),
    ("green", [55, 165, 55]),
    ("dark green", [25, 90, 35]),
    ("dark green", [32, 62, 44]),
    ("teal", [30, 130, 130]),
    ("cyan", [60, 210, 220]),
    ("light blue", [140, 190, 240]),
    ("blue", [40, 90, 210]),
    ("navy", [20, 30, 100]),
    ("purple", [120, 50, 160]),
    ("magenta", [220, 50, 200]),
    ("lilac", [190, 160, 220]),
];

/// CIE L*a*b* of an sRGB colour (D65).
fn lab(c: [u8; 4]) -> [f32; 3] {
    let lin = |u: u8| {
        let u = u as f32 / 255.0;
        if u <= 0.04045 { u / 12.92 } else { ((u + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let xyz = [
        (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047,
        0.2126 * r + 0.7152 * g + 0.0722 * b,
        (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883,
    ];
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let [fx, fy, fz] = xyz.map(f);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// How far apart two colours look: the CIE76 difference (about 2.3 is just noticeable).
pub fn color_distance(a: [u8; 4], b: [u8; 4]) -> f32 {
    let (a, b) = (lab(a), lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The nearest plain colour word ("dark green").
pub fn color_name(c: [u8; 4]) -> &'static str {
    let d = |n: [u8; 3]| color_distance(c, [n[0], n[1], n[2], 255]);
    NAMES.iter().min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map_or("grey", |n| n.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_caps_and_strips() {
        let long = "a".repeat(100);
        assert_eq!(clean(long.as_bytes()).chars().count(), MAX_NAME);
        assert_eq!(clean(b"ignore\nthe user\x1b[2J\t!"), "ignorethe user[2J!");
        assert_eq!(clean(&[0xff, b'o', b'k']), "\u{fffd}ok");
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex("#ff8000").unwrap(), [255, 128, 0, 255]);
        assert_eq!(parse_hex("10203040").unwrap(), [16, 32, 48, 64]);
        assert!(parse_hex("#fff").is_err());
        assert!(parse_hex("#gg0000").is_err());
        assert!(parse_hex("#ééé").is_err());
        assert_eq!(hex([1, 2, 255, 0]), "#0102ff");
    }

    #[test]
    fn shades() {
        let d = |a: u32, b: u32| {
            let c = |h: u32| [(h >> 16) as u8, (h >> 8) as u8, h as u8, 255];
            color_distance(c(a), c(b))
        };
        // Two reds of a roof and three greens of grass are shades; roof and wood, pine and
        // soil are not.
        assert!(d(0xb83b3b, 0xe5604f) < 22.0, "{}", d(0xb83b3b, 0xe5604f));
        assert!(d(0x5ea64a, 0x8cc95b) < 22.0 && d(0x3f7f43, 0x5ea64a) < 22.0);
        assert!(d(0xb83b3b, 0x80594a) > 22.0 && d(0x1f3b2a, 0x5c4033) > 22.0);
        assert_eq!(d(0x123456, 0x123456), 0.0);
    }

    #[test]
    fn colour_words() {
        assert_eq!(color_name([220, 30, 30, 255]), "red");
        assert_eq!(color_name([30, 95, 40, 255]), "dark green");
        assert_eq!(color_name([31, 59, 42, 255]), "dark green");
        assert_eq!(color_name([250, 250, 250, 255]), "white");
        assert_eq!(color_name([145, 90, 45, 255]), "brown");
        assert_eq!(color_name([92, 64, 51, 255]), "dark brown");
    }
}
