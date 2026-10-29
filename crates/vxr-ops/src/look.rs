//! `look`: a picture of the scene for a model that takes images, cast on the CPU through the
//! brick map ([`vxr_core::BrickMap::raycast`]) and shaded as the editor view shades it
//! (`march.wgsl`: palette colour, sky and sun, here with shadows), so it needs no GPU and
//! looks the same in `vxr mcp` files mode, the desktop app and the browser. Staged removals
//! are empty, as in every view. It is not the path tracer: materials other than emit are not
//! shown, which the Render export is for.

use serde_json::json;
use vxr_core::Scene;
use vxr_vox::Kind;

use crate::args::{Cell, Look, LookView};
use crate::session::{Error, Session, invalid};

type V3 = [f32; 3];

/// The picture's longer side when the call gives none, and its limits.
pub const DEFAULT_SIZE: u32 = 512;
pub const MIN_SIZE: u32 = 64;
pub const MAX_SIZE: u32 = 1024;
/// The largest picture, in bytes: as base64 it still fits the live socket's and the bridge's
/// 4 MiB message.
pub const MAX_PNG: usize = 2 << 20;

/// The editor view's sun (`Renderer`'s), toward the front-right-top.
const SUN: V3 = [0.4207, 0.7479, 0.3272];
/// Pitch of the isometric view (`vxr_render::camera::ISO_PITCH`).
const ISO_PITCH: f32 = 0.615_479_7;
/// Background and ground, sRGB.
const BACKGROUND: [u8; 3] = [36, 38, 43];
const GROUND: [u8; 3] = [64, 67, 74];
const GRID: [u8; 3] = [84, 88, 96];
/// The selection's tint, sRGB.
const SELECTED: [u8; 3] = [255, 140, 26];

/// A camera in map cell space (voxel (x, y, z) fills [x, x + 1) and so on, y up), as
/// `vxr_render::OrbitCamera` gives it: where rays start, the view's basis, the projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The eye in perspective; in orthographic, the middle of the plane rays start on.
    pub eye: V3,
    pub fwd: V3,
    pub right: V3,
    pub up: V3,
    pub ortho: bool,
    /// Half the view's height: in voxels in orthographic, as the tangent of half the
    /// vertical field of view in perspective.
    pub half: f32,
    /// Width over height.
    pub aspect: f32,
}

impl Camera {
    /// The ray through a pixel centre (origin top left) of a `w` x `h` picture.
    fn ray(&self, px: f32, py: f32, w: f32, h: f32) -> (V3, V3) {
        let sx = (px / w * 2.0 - 1.0) * self.half * self.aspect;
        let sy = (1.0 - py / h * 2.0) * self.half;
        if self.ortho {
            (add(self.eye, add(mul(self.right, sx), mul(self.up, sy))), self.fwd)
        } else {
            (self.eye, norm(add(self.fwd, add(mul(self.right, sx), mul(self.up, sy)))))
        }
    }
}

/// View direction, right and up for a yaw and pitch, as `OrbitCamera::basis`.
fn basis(yaw: f32, pitch: f32) -> (V3, V3, V3) {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let fwd = [-cp * sy, -sp, -cp * cy];
    let up = [-sp * sy, cp, -sp * cy];
    (fwd, cross(fwd, up), up)
}

fn angles(v: LookView) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    match v {
        LookView::Front | LookView::Current => (0.0, 0.0),
        LookView::Back => (PI, 0.0),
        LookView::Left => (-FRAC_PI_2, 0.0),
        LookView::Right => (FRAC_PI_2, 0.0),
        LookView::Top => (0.0, FRAC_PI_2),
        LookView::Bottom => (0.0, -FRAC_PI_2),
        LookView::Iso => (FRAC_PI_4, ISO_PITCH),
    }
}

/// An orthographic camera looking from `view` that frames the map box `lo..hi` (corners of
/// cells), a little margin round it.
pub fn framing(view: LookView, lo: V3, hi: V3) -> Camera {
    let (yaw, pitch) = angles(view);
    let (fwd, right, up) = basis(yaw, pitch);
    let mid = mul(add(lo, hi), 0.5);
    let (mut hw, mut hh) = (0.5f32, 0.5f32);
    for i in 0..8 {
        let c = [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ];
        let d = sub(c, mid);
        hw = hw.max(dot(d, right).abs());
        hh = hh.max(dot(d, up).abs());
    }
    let margin = 1.08;
    let aspect = (hw / hh).clamp(0.5, 2.0);
    let half = (hh.max(hw / aspect)) * margin;
    let back = len(sub(hi, lo)) + 4.0;
    Camera { eye: sub(mid, mul(fwd, back)), fwd, right, up, ortho: true, half, aspect }
}

/// The picture's size for a longer side of `size` pixels.
fn dims(size: u32, aspect: f32) -> (u32, u32) {
    if aspect >= 1.0 {
        (size, ((size as f32 / aspect).round() as u32).max(1))
    } else {
        (((size as f32 * aspect).round() as u32).max(1), size)
    }
}

impl Session {
    /// The user's view for `look` with view current, or None where there is none.
    pub fn set_camera(&mut self, cam: Option<Camera>) {
        self.camera = cam;
    }

    /// The picture the last `look` made, as PNG bytes.
    pub fn take_image(&mut self) -> Option<Vec<u8>> {
        self.image.take()
    }

    pub(crate) fn look(&mut self, scene: &Scene, a: &Look) -> Result<String, Error> {
        let size = a.size.unwrap_or(DEFAULT_SIZE);
        if !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(invalid(format!("size {size}: give {MIN_SIZE} to {MAX_SIZE} pixels")));
        }
        let b = scene.bounds();
        let (mut lo, mut hi, mut voxels) = ([i32::MAX; 3], [i32::MIN; 3], 0usize);
        self.for_each(scene, |p, _| {
            voxels += 1;
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k] + 1);
            }
        });
        if voxels == 0 {
            (lo, hi) = ([0; 3], b.size.map(|s| s as i32));
        }
        let cam = match a.view {
            LookView::Current => self.camera.ok_or_else(|| {
                invalid(
                    "view current is the user's view in the vxr window, which only live mode has: pick a named view",
                )
            })?,
            v => framing(v, lo.map(|c| c as f32), hi.map(|c| c as f32)),
        };
        let (w, h) = dims(size, cam.aspect);
        let rgb = self.draw(scene, &cam, w, h);
        let png = encode(w, h, &rgb).map_err(invalid)?;
        if png.len() > MAX_PNG {
            return Err(invalid(format!(
                "the picture is {} KiB, over the {} KiB a picture may be: ask for a smaller size",
                png.len() >> 10,
                MAX_PNG >> 10
            )));
        }
        let name = match a.view {
            LookView::Front => "front",
            LookView::Back => "back",
            LookView::Left => "left",
            LookView::Right => "right",
            LookView::Top => "top",
            LookView::Bottom => "bottom",
            LookView::Iso => "iso",
            LookView::Current => "current",
        };
        let mut out = json!({
            "view": name,
            "width": w,
            "height": h,
            "projection": if cam.ortho { "orthographic" } else { "perspective" },
            "voxels": voxels,
            "note": "the picture is the editor view's shading (palette colours, sun and shadows, the ground, the selection tinted orange), not the path-traced render: materials other than emit do not show",
        });
        // A side view's picture spans whole cells: say which, so the model can point at them.
        let world = |m: V3| [m[0] + b.lo[0] as f32, m[1] + b.lo[1] as f32, m[2] + b.lo[2] as f32];
        let span = |axis: usize, sign: f32, along: f32| world(cam.eye)[axis] + sign * along;
        if cam.ortho && !matches!(a.view, LookView::Iso | LookView::Current) {
            let (hw, hh) = (cam.half * cam.aspect, cam.half);
            let ax = |v: V3| v.iter().position(|c| c.abs() > 0.5).unwrap();
            let (ra, ua) = (ax(cam.right), ax(cam.up));
            let (rs, us) = (cam.right[ra].signum(), cam.up[ua].signum());
            let names = ["x", "y", "z"];
            let left = span(ra, -rs, hw);
            let right = span(ra, rs, hw);
            let top = span(ua, us, hh);
            let bottom = span(ua, -us, hh);
            out["across"] = json!({"axis": names[ra], "left_edge": round(left), "right_edge": round(right)});
            out["down"] = json!({"axis": names[ua], "top_edge": round(top), "bottom_edge": round(bottom)});
            out["pixels_per_voxel"] = json!(round(h as f32 / (2.0 * hh)));
            out["cells"] = json!(format!(
                "a pixel at column c shows {} = floor({} {} c / {}), and at row r {} = floor({} {} r / {})",
                names[ra],
                round(left),
                if rs > 0.0 { "+" } else { "-" },
                round(w as f32 / (2.0 * hw)),
                names[ua],
                round(top),
                if us > 0.0 { "-" } else { "+" },
                round(h as f32 / (2.0 * hh)),
            ));
        }
        self.image = Some(png);
        Ok(out.to_string())
    }

    /// The picture as sRGB rows.
    fn draw(&self, scene: &Scene, cam: &Camera, w: u32, h: u32) -> Vec<u8> {
        let b = scene.bounds();
        let size = b.size.map(|s| s as f32);
        let floor = (b.floor - b.lo[1]) as f32;
        let lin = |c: [u8; 3]| c.map(|v| (v as f32 / 255.0).powf(2.2));
        let (bg, ground, grid, tint) = (lin(BACKGROUND), lin(GROUND), lin(GRID), lin(SELECTED));
        // Grid lines only where a voxel is several pixels wide.
        let ppv = h as f32 / (2.0 * cam.half);
        let lines = cam.ortho && ppv >= 6.0;
        let mut out = Vec::with_capacity((w * h * 3) as usize);
        for py in 0..h {
            for px in 0..w {
                let (ro, d) = cam.ray(px as f32 + 0.5, py as f32 + 0.5, w as f32, h as f32);
                let hit = self.cast(scene, ro, d, 1e6);
                // The ground plane under the model, seen from above.
                let tg = if d[1] < -1e-6 && ro[1] > floor { (floor - ro[1]) / d[1] } else { f32::INFINITY };
                let col = match hit {
                    Some((t, v, n)) if t <= tg => {
                        let slot = self.get(scene, v) as usize;
                        let c = scene.palette[slot];
                        let albedo = lin([c[0], c[1], c[2]]);
                        let p = add(add(ro, mul(d, t)), mul(n, 1e-3));
                        let lit = if scene.materials[slot].kind == Kind::Emit {
                            1.0
                        } else {
                            let diff = dot(n, SUN).max(0.0);
                            let shade = if diff > 0.0 && self.cast(scene, p, SUN, 1e6).is_some() { 0.0 } else { 1.0 };
                            0.32 + 0.10 * n[1] + 0.85 * diff * shade
                        };
                        let mut c = albedo.map(|a| a * lit);
                        // A darker line on each voxel's edges, so cells can be counted.
                        let q = add(ro, mul(d, t));
                        let edge = (0..3)
                            .filter(|&k| n[k] == 0.0)
                            .any(|k| (q[k] - q[k].floor()).min(q[k].ceil() - q[k]) * ppv < 0.6);
                        if lines && edge {
                            c = mul(c, 0.8);
                        }
                        if self.selection.contains(&v) {
                            c = mix(c, mul(tint, lit + 0.2), 0.45);
                        }
                        c
                    }
                    _ if tg.is_finite() => {
                        let g = add(ro, mul(d, tg));
                        if (0.0..=size[0]).contains(&g[0]) && (0.0..=size[2]).contains(&g[2]) {
                            let p = [g[0], floor + 1e-3, g[2]];
                            let lit = if self.cast(scene, p, SUN, 1e6).is_some() { 0.55 } else { 1.0 };
                            let f = [g[0].fract(), g[2].fract()];
                            let edge = f.iter().any(|&f| f.min(1.0 - f) * ppv < 0.75);
                            mul(if lines && edge { grid } else { ground }, lit)
                        } else {
                            bg
                        }
                    }
                    _ => bg,
                };
                out.extend(col.map(|c| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8));
            }
        }
        out
    }

    /// The first voxel along a ray from `ro` within `max_t`, staged removals passed through:
    /// its distance, cell and the normal of the face hit.
    fn cast(&self, scene: &Scene, ro: V3, d: V3, max_t: f32) -> Option<(f32, Cell, V3)> {
        let mut t0 = 0.0;
        loop {
            let o = add(ro, mul(d, t0));
            let h = scene.map.raycast(o, d, max_t - t0)?;
            let n = h.normal.map(|c| c as f32);
            if !self.removed.contains(&h.voxel) {
                return Some((t0 + h.t, h.voxel, n));
            }
            // Past the removed cell: where the ray leaves it.
            let exit = (0..3)
                .filter(|&k| d[k].abs() > 1e-9)
                .map(|k| {
                    let edge = h.voxel[k] as f32 + if d[k] > 0.0 { 1.0 } else { 0.0 };
                    (edge - o[k]) / d[k]
                })
                .fold(f32::INFINITY, f32::min);
            t0 += exit.max(h.t) + 1e-4;
            if t0 >= max_t {
                return None;
            }
        }
    }
}

/// RGB rows as a PNG file.
fn encode(w: u32, h: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().and_then(|mut wr| wr.write_image_data(rgb)).map_err(|e| e.to_string())?;
    Ok(out)
}

fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    a.map(|c| c * s)
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> V3 {
    mul(a, 1.0 / len(a))
}
fn mix(a: V3, b: V3, k: f32) -> V3 {
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}
