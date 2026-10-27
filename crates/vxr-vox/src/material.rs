//! Palette slot materials: the `MATL` dict as a typed value and back.
//!
//! A `MATL` chunk's id is the palette index it applies to (1 to 255). vxr renders five
//! types, MagicaVoxel's diffuse, metal, plastic (`_metal` with `_plastic` set), glass and
//! emissive. Newer types are read as the closest of those (`_blend` by its weights, `_media` as diffuse), and a slot vxr does not
//! edit keeps its dict byte for byte. [`Material::apply_to`] changes only the keys of the
//! type being written, so the keys vxr does not know survive an edit too.

use crate::bytes::Dict;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Diffuse,
    Metal,
    Glass,
    Emit,
    Plastic,
}

impl Kind {
    /// Every type, in the order of their numbers (the browser's `material(slot)` type).
    pub const ALL: [Kind; 5] = [Kind::Diffuse, Kind::Metal, Kind::Glass, Kind::Emit, Kind::Plastic];
    /// The order the Material tab shows them in, MagicaVoxel's.
    pub const SHOWN: [Kind; 5] = [Kind::Diffuse, Kind::Metal, Kind::Plastic, Kind::Glass, Kind::Emit];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Diffuse => "Diffuse",
            Kind::Metal => "Metal",
            Kind::Glass => "Glass",
            Kind::Emit => "Emit",
            Kind::Plastic => "Plastic",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Kind::Diffuse => "_diffuse",
            Kind::Metal | Kind::Plastic => "_metal",
            Kind::Glass => "_glass",
            Kind::Emit => "_emit",
        }
    }
}

/// One palette slot's material. Every field is kept whatever the type, so switching the
/// type back and forth in the editor loses nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub kind: Kind,
    /// Surface roughness, 0 (mirror) to 1. `_rough`.
    pub rough: f32,
    /// How much of a metal reflects like metal (the rest is diffuse), 0 to 1. `_metal`.
    pub metal: f32,
    /// Refractive index of glass, 1 to 3. `_ri`, or `_ior` + 1.
    pub ior: f32,
    /// How much light passes through glass, 0 to 1. `_trans`.
    pub trans: f32,
    /// Emission, 0 to 1 (`_emit`), times 2 to the power `flux` (`_flux`, 0 to 4).
    pub emit: f32,
    pub flux: f32,
    /// How much plastic's clear coat reflects, 0 (matte) to 1. `_spec`.
    pub spec: f32,
    /// How deeply glass takes on its colour the more of it light crosses, 0 to 1. `_att`.
    pub att: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            kind: Kind::Diffuse,
            rough: 0.1,
            metal: 1.0,
            ior: 1.3,
            trans: 0.8,
            emit: 1.0,
            flux: 1.0,
            spec: 1.0,
            att: 0.0,
        }
    }
}

pub(crate) fn num(d: &Dict, key: &str) -> Option<f32> {
    d.get_str(key).and_then(|s| s.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
}

/// A number as MagicaVoxel writes it: at most three decimals, no trailing zeros.
pub(crate) fn fmt(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

impl Material {
    /// Read a `MATL` dict. Missing keys take the defaults.
    pub fn from_dict(d: &Dict) -> Material {
        let def = Material::default();
        let metal = num(d, "_metal").unwrap_or(def.metal).clamp(0.0, 1.0);
        let trans = num(d, "_trans").unwrap_or(def.trans).clamp(0.0, 1.0);
        let kind = match d.get_str("_type").map(str::trim) {
            Some("_metal") if num(d, "_plastic").is_some_and(|v| v != 0.0) => Kind::Plastic,
            Some("_metal") => Kind::Metal,
            Some("_glass") => Kind::Glass,
            Some("_emit") => Kind::Emit,
            Some("_blend") if num(d, "_metal").unwrap_or(0.0) >= 0.5 => Kind::Metal,
            Some("_blend") if num(d, "_trans").unwrap_or(0.0) >= 0.5 => Kind::Glass,
            _ => Kind::Diffuse,
        };
        let ior = num(d, "_ri").or_else(|| num(d, "_ior").map(|v| v + 1.0)).unwrap_or(def.ior);
        Material {
            kind,
            rough: num(d, "_rough").unwrap_or(def.rough).clamp(0.0, 1.0),
            metal,
            ior: ior.clamp(1.0, 3.0),
            trans,
            emit: num(d, "_emit").unwrap_or(def.emit).clamp(0.0, 1.0),
            flux: num(d, "_flux").unwrap_or(def.flux).clamp(0.0, 4.0),
            spec: num(d, "_spec").unwrap_or(def.spec).clamp(0.0, 1.0),
            att: num(d, "_att").unwrap_or(def.att).clamp(0.0, 1.0),
        }
    }

    /// Write this material into `d`: the type and that type's keys. Other keys stay.
    pub fn apply_to(&self, d: &mut Dict) {
        d.set("_type", self.kind.tag());
        if !d.has("_weight") {
            d.set("_weight", "1");
        }
        match self.kind {
            Kind::Diffuse => {}
            Kind::Metal => {
                d.set("_rough", fmt(self.rough));
                d.set("_metal", fmt(self.metal));
                if d.has("_plastic") {
                    d.set("_plastic", "0");
                }
            }
            Kind::Plastic => {
                d.set("_plastic", "1");
                d.set("_rough", fmt(self.rough));
                d.set("_spec", fmt(self.spec));
            }
            Kind::Glass => {
                d.set("_rough", fmt(self.rough));
                d.set("_ior", fmt(self.ior - 1.0));
                d.set("_ri", fmt(self.ior));
                d.set("_trans", fmt(self.trans));
                d.set("_att", fmt(self.att));
            }
            Kind::Emit => {
                d.set("_emit", fmt(self.emit));
                d.set("_flux", fmt(self.flux));
            }
        }
    }

    /// A fresh dict for this material.
    pub fn to_dict(&self) -> Dict {
        let mut d = Dict::default();
        self.apply_to(&mut d);
        d
    }

    /// Light given off, in multiples of the slot's colour (0 unless emissive).
    pub fn radiance(&self) -> f32 {
        if self.kind == Kind::Emit { self.emit * self.flux.exp2() * 2.0 } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_type() {
        let d = Dict::default().with("_type", "_glass").with("_rough", "0.2").with("_ior", "0.5").with("_trans", "0.9");
        let m = Material::from_dict(&d);
        assert_eq!((m.kind, m.rough, m.ior, m.trans), (Kind::Glass, 0.2, 1.5, 0.9));
        let d = Dict::default().with("_type", "_emit").with("_emit", "0.5").with("_flux", "2");
        assert_eq!(Material::from_dict(&d).radiance(), 0.5 * 4.0 * 2.0);
        let d = Dict::default().with("_type", "_blend").with("_metal", "0.7");
        assert_eq!(Material::from_dict(&d).kind, Kind::Metal);
        assert_eq!(Material::from_dict(&Dict::default().with("_type", "_media")).kind, Kind::Diffuse);
        let d = Dict::default().with("_type", "_metal").with("_plastic", "1").with("_spec", "0.4");
        assert_eq!((Material::from_dict(&d).kind, Material::from_dict(&d).spec), (Kind::Plastic, 0.4));
        let d = Dict::default().with("_type", "_glass").with("_att", "0.7");
        assert_eq!(Material::from_dict(&d).att, 0.7);
        assert_eq!(Material::from_dict(&Dict::default()), Material::default());
    }

    #[test]
    fn round_trips_and_keeps_other_keys() {
        for kind in Kind::ALL {
            let m = Material {
                kind,
                rough: 0.25,
                metal: 0.75,
                ior: 1.45,
                trans: 0.5,
                emit: 0.4,
                flux: 3.0,
                spec: 0.6,
                att: 0.3,
            };
            let mut d = Dict::default().with("_d", "0.5").with("_weight", "0.8");
            m.apply_to(&mut d);
            assert_eq!(d.get_str("_d"), Some("0.5"));
            assert_eq!(d.get_str("_weight"), Some("0.8"));
            let back = Material::from_dict(&d);
            assert_eq!(back.kind, kind);
            match kind {
                Kind::Diffuse => {}
                Kind::Metal => assert_eq!((back.rough, back.metal), (0.25, 0.75)),
                Kind::Glass => assert_eq!((back.rough, back.ior, back.trans, back.att), (0.25, 1.45, 0.5, 0.3)),
                Kind::Emit => assert_eq!((back.emit, back.flux), (0.4, 3.0)),
                Kind::Plastic => assert_eq!((back.rough, back.spec), (0.25, 0.6)),
            }
        }
        // Plastic back to metal clears the flag, so MagicaVoxel reads it as metal too.
        let mut d = Material { kind: Kind::Plastic, ..Material::default() }.to_dict();
        assert_eq!((d.get_str("_type"), d.get_str("_plastic")), (Some("_metal"), Some("1")));
        Material { kind: Kind::Metal, ..Material::default() }.apply_to(&mut d);
        assert_eq!(Material::from_dict(&d).kind, Kind::Metal);
        assert_eq!(fmt(0.0), "0");
        assert_eq!(fmt(1.0), "1");
        assert_eq!(fmt(0.1 + 0.2), "0.3");
    }
}
