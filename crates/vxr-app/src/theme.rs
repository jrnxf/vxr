//! The UI themes: one set of colour tokens each, for the Hybrid UI and the editor view's own
//! colours (background, ground, grid, bounds frame and selection). Voxels keep their palette
//! colours in every theme.
//!
//! The UI draws with the current theme's tokens ([`th`]), set once per frame from the picked
//! [`ThemeChoice`] by `Hybrid::ui`.

use std::cell::Cell;

use egui::Color32;
use vxr_render::ViewColors;

/// One theme's colour tokens. Each names the shadcn/ui variable it plays the part of.
#[derive(Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    /// A dark theme (egui's dark visuals); light otherwise.
    pub dark: bool,
    /// --card at 94%: the floating panels.
    pub panel: Color32,
    /// --card: menus over other panels.
    pub panel_solid: Color32,
    /// --border.
    pub line: Color32,
    /// --input.
    pub line2: Color32,
    /// --foreground.
    pub text: Color32,
    /// --muted-foreground.
    pub muted: Color32,
    /// Labels, hints and keys, a step quieter than `muted`: --ring.
    pub faint: Color32,
    /// --primary.
    pub accent: Color32,
    /// The primary button under the pointer (shadcn's `bg-primary/90`).
    pub accent_hover: Color32,
    /// Text and icons on `accent`: --primary-foreground.
    pub on_accent: Color32,
    /// An on state's fill: --accent over a panel.
    pub accent_bg: Color32,
    /// A faint fill for a picked row or drop target.
    pub accent_tint: Color32,
    /// An on state's or a focused field's border: --ring.
    pub ring: Color32,
    /// --destructive.
    pub danger: Color32,
    pub danger_bg: Color32,
    pub danger_line: Color32,
    pub ok: Color32,
    /// A warning, between ok and danger.
    pub mid: Color32,
    pub mid_bg: Color32,
    pub mid_line: Color32,
    /// A control under the pointer.
    pub hover: Color32,
    /// Panel shadows.
    pub shadow: Color32,
    /// The shadow under text drawn straight on the view.
    pub text_shadow: Color32,
    /// The colour the translucent fills and borders are made of: white on Neutral, black on
    /// Paper, the foreground on the editor themes.
    base: Color32,
    /// The editor view's own colours.
    pub view: ViewColors,
}

impl Theme {
    /// The translucent fill or border the theme's fills are made of, at `alpha` (0 to 255).
    pub fn tint(&self, alpha: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(self.base.r(), self.base.g(), self.base.b(), alpha)
    }

    /// A backing for chips drawn over the rendered image, at `alpha`: black in a dark theme,
    /// white in a light one.
    pub fn scrim(&self, alpha: u8) -> Color32 {
        if self.dark { Color32::from_black_alpha(alpha) } else { Color32::from_white_alpha(alpha) }
    }
}

/// What a theme is built from; the translucent tokens follow from `base`.
struct Spec {
    name: &'static str,
    dark: bool,
    card: u32,
    fg: u32,
    muted: u32,
    faint: u32,
    accent: u32,
    accent_hover: u32,
    on_accent: u32,
    base: u32,
    danger: u32,
    ok: u32,
    mid: u32,
    view: ViewColors,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

const fn rgba(c: u32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((c >> 16) as u8, (c >> 8) as u8, c as u8, a)
}

const fn bytes(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// `a` mixed toward `b` by `t` 255ths, per channel in sRGB.
const fn mix(a: u32, b: u32, t: u32) -> u32 {
    const fn ch(a: u32, b: u32, t: u32, s: u32) -> u32 {
        let (x, y) = ((a >> s) & 0xff, (b >> s) & 0xff);
        ((x * (255 - t) + y * t + 127) / 255) << s
    }
    ch(a, b, t, 16) | ch(a, b, t, 8) | ch(a, b, t, 0)
}

const fn theme(s: Spec) -> Theme {
    Theme {
        name: s.name,
        dark: s.dark,
        panel: rgba(s.card, 240),
        panel_solid: rgb(s.card),
        line: rgba(s.base, 26),
        line2: rgba(s.base, 38),
        text: rgb(s.fg),
        muted: rgb(s.muted),
        faint: rgb(s.faint),
        accent: rgb(s.accent),
        accent_hover: rgb(s.accent_hover),
        on_accent: rgb(s.on_accent),
        accent_bg: rgba(s.base, 18),
        accent_tint: rgba(s.base, 10),
        ring: rgb(s.faint),
        danger: rgb(s.danger),
        danger_bg: rgba(s.danger, 18),
        danger_line: rgba(s.danger, 70),
        ok: rgb(s.ok),
        mid: rgb(s.mid),
        mid_bg: rgba(s.mid, 20),
        mid_line: rgba(s.mid, 90),
        hover: rgba(s.base, 15),
        shadow: Color32::from_black_alpha(if s.dark { 89 } else { 26 }),
        text_shadow: if s.dark {
            Color32::from_black_alpha(200)
        } else {
            Color32::from_rgba_unmultiplied_const(255, 255, 255, 200)
        },
        base: rgb(s.base),
        view: s.view,
    }
}

/// shadcn/ui's Neutral theme (.dark): every grey is oklch(L 0 0), so the chrome has no hue and
/// the voxels are the only colour on screen.
pub static NEUTRAL: Theme = theme(Spec {
    name: "Neutral",
    dark: true,
    card: 0x171717,
    fg: 0xfafafa,
    muted: 0xa1a1a1,
    faint: 0x737373,
    accent: 0xe5e5e5,
    accent_hover: 0xd4d4d4,
    on_accent: 0x171717,
    base: 0xffffff,
    danger: 0xff6467,
    ok: 0x6ee7a8,
    mid: 0xffd166,
    view: ViewColors::NEUTRAL,
});

/// shadcn/ui's Neutral theme in its light variant (:root): white cards, a near-black primary.
pub static PAPER: Theme = theme(Spec {
    name: "Paper",
    dark: false,
    card: 0xffffff,
    fg: 0x0a0a0a,
    muted: 0x737373,
    faint: 0xa1a1a1,
    accent: 0x171717,
    accent_hover: mix(0x171717, 0xffffff, 26),
    on_accent: 0xfafafa,
    base: 0x000000,
    danger: 0xe7000b,
    // Tailwind's green-700 and yellow-700: shadcn has no success or warning colour, and the
    // dark theme's mint and amber are unreadable on white.
    ok: 0x15803d,
    mid: 0xa16207,
    view: ViewColors {
        sky: [bytes(0xffffff), bytes(0xf5f5f5)],
        tile: bytes(0xe5e5e5),
        base: bytes(0xd4d4d4),
        grid: bytes(0xa1a1a1),
        frame: bytes(0x737373),
        select: bytes(0x171717),
        select_edge: bytes(0xfafafa),
    },
});

/// Gruvbox dark (hard contrast background), from morhetz/gruvbox.
pub static GRUVBOX: Theme = theme(Spec {
    name: "Gruvbox",
    dark: true,
    card: 0x1d2021,   // bg0_h
    fg: 0xebdbb2,     // fg1
    muted: 0xa89984,  // fg4
    faint: 0x928374,  // gray
    accent: 0xfabd2f, // bright yellow
    accent_hover: 0xd79921,
    on_accent: 0x282828,
    base: 0xebdbb2,
    danger: 0xfb4934,
    ok: 0xb8bb26,
    mid: 0xfe8019,
    view: ViewColors {
        sky: [bytes(0x3c3836), bytes(0x282828)], // bg1 to bg0
        tile: bytes(0x32302f),                   // bg0_s
        base: bytes(0x504945),                   // bg2
        grid: bytes(0x7c6f64),                   // bg4
        frame: bytes(0xa89984),
        select: bytes(0xfabd2f),
        select_edge: bytes(0x282828),
    },
});

/// Tokyo Night (night), from folke/tokyonight.nvim.
pub static TOKYO_NIGHT: Theme = theme(Spec {
    name: "Tokyo Night",
    dark: true,
    card: 0x1a1b26,  // bg
    fg: 0xc0caf5,    // fg
    muted: 0xa9b1d6, // fg_dark
    faint: 0x737aa2, // dark5
    accent: 0x7aa2f7,
    accent_hover: mix(0x7aa2f7, 0x1a1b26, 26),
    on_accent: 0x1a1b26,
    base: 0xc0caf5,
    danger: 0xf7768e,
    ok: 0x9ece6a,
    mid: 0xe0af68,
    view: ViewColors {
        sky: [bytes(0x24283b), bytes(0x16161e)], // storm bg to bg_dark
        tile: bytes(0x1f2335),
        base: bytes(0x292e42), // bg_highlight
        grid: bytes(0x414868), // terminal_black
        frame: bytes(0xa9b1d6),
        select: bytes(0x7aa2f7),
        select_edge: bytes(0x1a1b26),
    },
});

/// Kanagawa Wave, from rebelot/kanagawa.nvim.
pub static KANAGAWA: Theme = theme(Spec {
    name: "Kanagawa",
    dark: true,
    card: 0x1f1f28,   // sumiInk3
    fg: 0xdcd7ba,     // fujiWhite
    muted: 0xc8c093,  // oldWhite
    faint: 0x727169,  // fujiGray
    accent: 0x7e9cd8, // crystalBlue
    accent_hover: mix(0x7e9cd8, 0x1f1f28, 26),
    on_accent: 0x1f1f28,
    base: 0xdcd7ba,
    danger: 0xff5d62, // peachRed
    ok: 0x98bb6c,     // springGreen
    mid: 0xe6c384,    // carpYellow
    view: ViewColors {
        sky: [bytes(0x2a2a37), bytes(0x16161d)], // sumiInk4 to sumiInk0
        tile: bytes(0x1f1f28),
        base: bytes(0x363646), // sumiInk5
        grid: bytes(0x54546d), // sumiInk6
        frame: bytes(0xc8c093),
        select: bytes(0x7e9cd8),
        select_edge: bytes(0x1f1f28),
    },
});

/// Nord, from nordtheme.com: Polar Night, Snow Storm, Frost and Aurora.
pub static NORD: Theme = theme(Spec {
    name: "Nord",
    dark: true,
    card: 0x2e3440,  // nord0
    fg: 0xeceff4,    // nord6
    muted: 0xd8dee9, // nord4
    // nord3 is far too dark for text on nord0: nord4 toward nord0.
    faint: mix(0xd8dee9, 0x2e3440, 102),
    accent: 0x88c0d0,       // nord8
    accent_hover: 0x81a1c1, // nord9
    on_accent: 0x2e3440,
    base: 0xeceff4,
    // nord11 is 3:1 on nord0: lifted toward nord6 so error text reads.
    danger: mix(0xbf616a, 0xeceff4, 77),
    ok: 0xa3be8c,  // nord14
    mid: 0xebcb8b, // nord13
    view: ViewColors {
        sky: [bytes(0x434c5e), bytes(0x2e3440)], // nord2 to nord0
        tile: bytes(0x3b4252),                   // nord1
        base: bytes(0x4c566a),                   // nord3
        grid: bytes(0x7b88a1),
        frame: bytes(0xd8dee9),
        select: bytes(0x88c0d0),
        select_edge: bytes(0x2e3440),
    },
});

/// The theme the user picked: a fixed one, or System, which follows the system's light or dark
/// setting (Paper in light mode, Neutral in dark).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    System,
    Neutral,
    Paper,
    Gruvbox,
    TokyoNight,
    Kanagawa,
    Nord,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 7] = [
        ThemeChoice::System,
        ThemeChoice::Neutral,
        ThemeChoice::Paper,
        ThemeChoice::Gruvbox,
        ThemeChoice::TokyoNight,
        ThemeChoice::Kanagawa,
        ThemeChoice::Nord,
    ];

    /// The id it is saved under (and its `theme-<id>` hit and `window.vxr` name).
    pub fn id(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Neutral => "neutral",
            ThemeChoice::Paper => "paper",
            ThemeChoice::Gruvbox => "gruvbox",
            ThemeChoice::TokyoNight => "tokyo-night",
            ThemeChoice::Kanagawa => "kanagawa",
            ThemeChoice::Nord => "nord",
        }
    }

    pub fn from_id(id: &str) -> Option<ThemeChoice> {
        Self::ALL.into_iter().find(|c| c.id() == id.trim())
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            c => c.resolve(true).name,
        }
    }

    /// The theme it shows, with the system in dark mode or not.
    pub fn resolve(self, dark: bool) -> &'static Theme {
        match self {
            ThemeChoice::System if dark => &NEUTRAL,
            ThemeChoice::System => &PAPER,
            ThemeChoice::Neutral => &NEUTRAL,
            ThemeChoice::Paper => &PAPER,
            ThemeChoice::Gruvbox => &GRUVBOX,
            ThemeChoice::TokyoNight => &TOKYO_NIGHT,
            ThemeChoice::Kanagawa => &KANAGAWA,
            ThemeChoice::Nord => &NORD,
        }
    }
}

thread_local! {
    static CURRENT: Cell<&'static Theme> = const { Cell::new(&NEUTRAL) };
}

/// The theme the UI is drawing with.
pub fn th() -> &'static Theme {
    CURRENT.with(Cell::get)
}

/// Draw with `theme` from now on (on this thread).
pub fn set(theme: &'static Theme) {
    CURRENT.with(|c| c.set(theme));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 relative luminance of an opaque colour.
    fn luminance(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn themes() -> impl Iterator<Item = &'static Theme> {
        ThemeChoice::ALL[1..].iter().map(|c| c.resolve(true))
    }

    #[test]
    fn body_text_meets_wcag_aa_in_every_theme() {
        for t in themes() {
            for (role, c) in [("text", t.text), ("muted", t.muted), ("danger", t.danger), ("ok", t.ok), ("mid", t.mid)]
            {
                let r = contrast(c, t.panel_solid);
                assert!(r >= 4.5, "{} {role} is {r:.2}:1 on its panels", t.name);
            }
            let r = contrast(t.on_accent, t.accent);
            assert!(r >= 4.5, "{} text on its accent is {r:.2}:1", t.name);
        }
    }

    #[test]
    fn labels_stay_legible() {
        // Faint is for labels and key hints, not body text. Paper's is shadcn's light --ring.
        for t in themes() {
            let r = contrast(t.faint, t.panel_solid);
            assert!(r >= 2.5, "{} faint is {r:.2}:1 on its panels", t.name);
        }
    }

    #[test]
    fn system_follows_light_and_dark() {
        assert_eq!(ThemeChoice::System.resolve(true).name, "Neutral");
        assert_eq!(ThemeChoice::System.resolve(false).name, "Paper");
        assert_eq!(ThemeChoice::Nord.resolve(false).name, "Nord", "a picked theme ignores the system");
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn ids_round_trip() {
        for c in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_id(c.id()), Some(c));
        }
        assert_eq!(ThemeChoice::from_id("tokyo-night\n"), Some(ThemeChoice::TokyoNight));
        assert_eq!(ThemeChoice::from_id("solarized"), None);
    }

    #[test]
    fn neutral_keeps_the_shipped_tokens() {
        assert_eq!(NEUTRAL.panel, Color32::from_rgba_unmultiplied(23, 23, 23, 240));
        assert_eq!(NEUTRAL.line, Color32::from_rgba_unmultiplied(255, 255, 255, 26));
        assert_eq!(NEUTRAL.text, Color32::from_rgb(0xfa, 0xfa, 0xfa));
        assert_eq!(NEUTRAL.tint(64), Color32::from_white_alpha(64));
        assert_eq!(NEUTRAL.view, ViewColors::NEUTRAL);
    }

    #[test]
    fn the_editor_themes_have_hue_and_neutral_has_none() {
        let hue = |c: Color32| c.r().max(c.g()).max(c.b()) - c.r().min(c.g()).min(c.b());
        for t in [&NEUTRAL, &PAPER] {
            assert_eq!(hue(t.panel_solid) + hue(t.text) + hue(t.accent), 0, "{}", t.name);
        }
        for t in [&GRUVBOX, &TOKYO_NIGHT, &KANAGAWA, &NORD] {
            assert!(hue(t.accent) > 40, "{}", t.name);
        }
    }
}
