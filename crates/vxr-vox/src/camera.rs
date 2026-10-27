//! Saved cameras: the `rCAM` dict as a typed value and back.
//!
//! An `rCAM` chunk's id is the camera slot it saves. MagicaVoxel writes its mode
//! (`_mode`), the point it orbits (`_focus`, file axes), its pitch, yaw and roll in degrees
//! (`_angle`), its distance from the focus (`_radius`), the orthographic frustum
//! (`_frustum`) and the field of view in degrees (`_fov`). An unknown mode is read as
//! perspective. [`Camera::apply_to`] writes only the keys whose value changed, so an edit
//! of one value leaves the other keys' text, and the keys vxr does not know, as they were.

use crate::bytes::Dict;
use crate::material::{fmt, num};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Pers,
    Free,
    Pano,
    Orth,
    Iso,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Pers, Mode::Free, Mode::Pano, Mode::Orth, Mode::Iso];

    /// The `_mode` value.
    pub fn tag(self) -> &'static str {
        match self {
            Mode::Pers => "pers",
            Mode::Free => "free",
            Mode::Pano => "pano",
            Mode::Orth => "orth",
            Mode::Iso => "iso",
        }
    }
}

/// One saved camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The camera slot, the chunk's id.
    pub id: i32,
    pub mode: Mode,
    /// The point the camera looks at, in MagicaVoxel's z-up file axes. `_focus`.
    pub focus: [f32; 3],
    /// Pitch, yaw and roll in degrees. `_angle`.
    pub angle: [f32; 3],
    /// Distance from the focus. `_radius`.
    pub radius: f32,
    /// Orthographic frustum. `_frustum`.
    pub frustum: f32,
    /// Field of view in degrees. `_fov`.
    pub fov: f32,
}

impl Camera {
    /// A camera in slot `id` with the values a dict without keys reads as.
    pub fn new(id: i32) -> Camera {
        Camera { id, mode: Mode::Pers, focus: [0.0; 3], angle: [0.0; 3], radius: 0.0, frustum: 0.4, fov: 45.0 }
    }

    /// Read an `rCAM` dict. Missing or malformed keys take the values of [`Camera::new`].
    pub fn from_dict(id: i32, d: &Dict) -> Camera {
        let def = Camera::new(id);
        let mode = d.get_str("_mode").map(str::trim).and_then(|m| Mode::ALL.into_iter().find(|k| k.tag() == m));
        Camera {
            id,
            mode: mode.unwrap_or(def.mode),
            focus: vec3(d, "_focus").unwrap_or(def.focus),
            angle: vec3(d, "_angle").unwrap_or(def.angle),
            radius: num(d, "_radius").unwrap_or(def.radius),
            frustum: num(d, "_frustum").unwrap_or(def.frustum),
            fov: num(d, "_fov").unwrap_or(def.fov),
        }
    }

    /// Write the values that differ from what `d` reads as into `d`. Other keys stay.
    pub fn apply_to(&self, d: &mut Dict) {
        let old = Camera::from_dict(self.id, d);
        if old.mode != self.mode {
            d.set("_mode", self.mode.tag());
        }
        let v3 = |v: [f32; 3]| v.map(fmt).join(" ");
        if old.focus != self.focus {
            d.set("_focus", v3(self.focus));
        }
        if old.angle != self.angle {
            d.set("_angle", v3(self.angle));
        }
        for (key, was, now) in
            [("_radius", old.radius, self.radius), ("_frustum", old.frustum, self.frustum), ("_fov", old.fov, self.fov)]
        {
            if was != now {
                d.set(key, fmt(now));
            }
        }
    }

    /// A fresh dict with every key, for a new file.
    pub fn to_dict(&self) -> Dict {
        let v3 = |v: [f32; 3]| v.map(fmt).join(" ");
        Dict::default()
            .with("_mode", self.mode.tag())
            .with("_focus", v3(self.focus))
            .with("_angle", v3(self.angle))
            .with("_radius", fmt(self.radius))
            .with("_frustum", fmt(self.frustum))
            .with("_fov", fmt(self.fov))
    }
}

/// Three numbers separated by spaces, as `_focus` and `_angle` hold them.
fn vec3(d: &Dict, key: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = d.get_str(key)?.split_whitespace().map(|s| s.parse::<f32>().ok()).collect::<Option<_>>()?;
    match v[..] {
        [x, y, z] if v.iter().all(|c| c.is_finite()) => Some([x, y, z]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        Dict::default()
            .with("_mode", "orth")
            .with("_focus", "1.5 -2 3")
            .with("_angle", "30 45 0")
            .with("_radius", "40")
            .with("_frustum", "0.25")
            .with("_fov", "60")
            .with("_extra", "kept")
    }

    #[test]
    fn reads_every_key() {
        let c = Camera::from_dict(2, &dict());
        assert_eq!(
            c,
            Camera {
                id: 2,
                mode: Mode::Orth,
                focus: [1.5, -2.0, 3.0],
                angle: [30.0, 45.0, 0.0],
                radius: 40.0,
                frustum: 0.25,
                fov: 60.0
            }
        );
        assert_eq!(Camera::from_dict(0, &Dict::default()), Camera::new(0));
        let bad = Dict::default().with("_mode", "warp").with("_focus", "1 2").with("_angle", "a b c");
        assert_eq!(Camera::from_dict(0, &bad), Camera::new(0));
        for m in Mode::ALL {
            assert_eq!(Camera::from_dict(0, &Dict::default().with("_mode", m.tag())).mode, m);
        }
    }

    #[test]
    fn writes_only_what_changed() {
        let mut d = dict().with("_radius", "40.0");
        let c = Camera::from_dict(2, &d);
        c.apply_to(&mut d);
        assert_eq!(d, dict().with("_radius", "40.0"), "unchanged: the text stays");
        Camera { radius: 60.0, angle: [10.0, 45.0, 0.0], ..c }.apply_to(&mut d);
        let want = dict().with("_radius", "60").with("_angle", "10 45 0");
        assert_eq!(d, want, "only the edited keys change, in place");
        assert_eq!(Camera::from_dict(2, &d), Camera { radius: 60.0, angle: [10.0, 45.0, 0.0], ..c });
        let full = Camera { mode: Mode::Iso, fov: 30.5, ..c };
        assert_eq!(Camera::from_dict(2, &full.to_dict()), full);
    }
}
