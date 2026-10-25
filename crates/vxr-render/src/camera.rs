//! MagicaVoxel-style orbit camera: orbit around a target, pan, zoom toward the cursor, in
//! perspective or orthographic projection, with snap views along the axes and isometric.

use glam::{Mat4, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// How far behind the target orthographic rays start, in voxels: outside any model (bounds go
/// up to 2048 a side, so the farthest corner is under 3548 from the centre) at any zoom.
pub const ORTHO_BACK: f32 = 4096.0;

/// Pitch of the isometric view: the cube diagonal points at the eye (atan of 1 / sqrt 2).
pub const ISO_PITCH: f32 = 0.615_479_7;

/// A view to snap to: looking at the target from one side, or isometric.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// From +z.
    Front,
    /// From -z.
    Back,
    /// From -x.
    Left,
    /// From +x.
    Right,
    Top,
    Bottom,
    /// Orthographic, from the front-right-top corner (+x, +y, +z).
    Iso,
}

impl View {
    pub const ALL: [View; 7] = [View::Front, View::Back, View::Left, View::Right, View::Top, View::Bottom, View::Iso];

    pub fn name(self) -> &'static str {
        match self {
            View::Front => "Front",
            View::Back => "Back",
            View::Left => "Left",
            View::Right => "Right",
            View::Top => "Top",
            View::Bottom => "Bottom",
            View::Iso => "Isometric",
        }
    }

    /// Yaw and pitch of the view.
    pub fn angles(self) -> (f32, f32) {
        match self {
            View::Front => (0.0, 0.0),
            View::Back => (PI, 0.0),
            View::Left => (-FRAC_PI_2, 0.0),
            View::Right => (FRAC_PI_2, 0.0),
            View::Top => (0.0, FRAC_PI_2),
            View::Bottom => (0.0, -FRAC_PI_2),
            View::Iso => (FRAC_PI_4, ISO_PITCH),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians around +y.
    pub yaw: f32,
    /// Radians above the horizon, -pi/2 to pi/2 (straight down at pi/2).
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    /// Orthographic projection: parallel rays, the view `2 * half_height()` voxels tall.
    pub ortho: bool,
}

impl OrbitCamera {
    /// Frame an axis-aligned box (voxel units), like MagicaVoxel's `.` key, for a 3024x1964
    /// (Retina laptop) view. Use `framing_aspect` when the view has another shape.
    pub fn framing(lo: Vec3, hi: Vec3) -> Self {
        Self::framing_aspect(lo, hi, 3024.0 / 1964.0)
    }

    /// Frame a box so its projection is centred and fills 90% of the tighter screen axis.
    pub fn framing_aspect(lo: Vec3, hi: Vec3, aspect: f32) -> Self {
        let target = (lo + hi) * 0.5;
        let radius = (hi - lo).length() * 0.5;
        let fov_y = 45f32.to_radians();
        let mut c = Self { target, yaw: 0.6, pitch: 0.6, distance: radius / (fov_y * 0.5).sin(), fov_y, ortho: false };
        c.fit(lo, hi, aspect, 0.9);
        c
    }

    /// Keep yaw and pitch; move the target in the view plane and the distance so the box's
    /// projected rectangle is centred and spans `fill` of the screen on its tighter axis.
    pub fn fit(&mut self, lo: Vec3, hi: Vec3, aspect: f32, fill: f32) {
        let corners: Vec<Vec3> = (0..8)
            .map(|i| {
                Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                )
            })
            .collect();
        for _ in 0..32 {
            let vp = self.view_proj(aspect);
            let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for c in &corners {
                let p = vp.project_point3(*c);
                x0 = x0.min(p.x);
                x1 = x1.max(p.x);
                y0 = y0.min(p.y);
                y1 = y1.max(p.y);
            }
            let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
            let ext = ((x1 - x0) * 0.5).max((y1 - y0) * 0.5);
            let (_, right, up) = self.basis();
            let half_h = self.half_height();
            self.target += right * cx * half_h * aspect + up * cy * half_h;
            self.distance *= (ext / fill).clamp(0.5, 2.0);
        }
    }

    /// The orbit position, `distance` from the target. Perspective rays start here.
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    /// View direction, right and up. Up comes from the yaw, so it stays defined looking
    /// straight down or up (it equals the usual cross with +y everywhere else).
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let fwd = -Vec3::new(cp * sy, sp, cp * cy);
        let up = Vec3::new(-sp * sy, cp, -sp * cy);
        (fwd, fwd.cross(up), up)
    }

    /// Half the view's height in voxels at the target: what the perspective view spans there,
    /// and the orthographic view everywhere, so switching keeps the model's size on screen.
    pub fn half_height(&self) -> f32 {
        self.distance * (self.fov_y * 0.5).tan()
    }

    /// Where the view matrix sits: the eye in perspective, `ORTHO_BACK` behind the target in
    /// orthographic (rays start on its near plane).
    pub fn view_eye(&self) -> Vec3 {
        if self.ortho { self.target - self.basis().0 * ORTHO_BACK } else { self.eye() }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let (_, _, up) = self.basis();
        let view = glam::camera::rh::view::look_at_mat4(self.view_eye(), self.target, up);
        let proj = if self.ortho {
            let (hh, hw) = (self.half_height(), self.half_height() * aspect);
            glam::camera::rh::proj::directx::orthographic(-hw, hw, -hh, hh, 0.0, 2.0 * ORTHO_BACK)
        } else {
            glam::camera::rh::proj::directx::perspective(self.fov_y, aspect, 0.5, 1.0e5)
        };
        proj * view
    }

    /// Ray through a pixel (pixel centre coords, origin top left): from the eye in
    /// perspective, from the near plane along the view direction in orthographic.
    pub fn ray(&self, px: f32, py: f32, w: f32, h: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(w / h).inverse();
        let ndc_x = px / w * 2.0 - 1.0;
        let ndc_y = 1.0 - py / h * 2.0;
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        (if self.ortho { near } else { self.eye() }, (far - near).normalize())
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-FRAC_PI_2, FRAC_PI_2);
    }

    pub fn pan(&mut self, dx: f32, dy: f32, viewport_h: f32) {
        let (_, right, up) = self.basis();
        let scale = 2.0 * self.half_height() / viewport_h;
        self.target += (-right * dx + up * dy) * scale;
    }

    /// Zoom toward `toward`, the point under the cursor (or the target if none).
    pub fn zoom(&mut self, steps: f32, toward: Option<Vec3>) {
        let factor = 0.88f32.powf(steps);
        if let Some(p) = toward {
            self.target = p + (self.target - p) * factor;
        }
        self.distance = (self.distance * factor).max(1.0);
    }

    /// Look from one side or isometric, keeping the target and zoom. Isometric turns
    /// orthographic on; the side views keep the projection.
    pub fn snap(&mut self, v: View) {
        (self.yaw, self.pitch) = v.angles();
        if v == View::Iso {
            self.ortho = true;
        }
    }

    /// The view the camera is at, if it is at one (isometric counts only in orthographic,
    /// from any of the four upper corners).
    pub fn view(&self) -> Option<View> {
        let close = |a: f32, b: f32| {
            let d = (a - b).rem_euclid(2.0 * PI);
            d.min(2.0 * PI - d) < 1e-3
        };
        let iso_yaw = (0..4).any(|k| close(self.yaw, FRAC_PI_4 + k as f32 * FRAC_PI_2));
        if self.ortho && (self.pitch - ISO_PITCH).abs() < 1e-3 && iso_yaw {
            return Some(View::Iso);
        }
        View::ALL[..6].iter().copied().find(|v| {
            let (y, p) = v.angles();
            (self.pitch - p).abs() < 1e-3 && (p.abs() == FRAC_PI_2 || close(self.yaw, y))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_matches_look_at_and_holds_at_the_poles() {
        for (yaw, pitch) in [(0.6, 0.6), (2.5, -0.3), (-1.2, 1.5), (0.0, FRAC_PI_2), (1.0, -FRAC_PI_2)] {
            let c = OrbitCamera { target: Vec3::ZERO, yaw, pitch, distance: 10.0, fov_y: 0.8, ortho: false };
            let (f, r, u) = c.basis();
            assert!((f - (c.target - c.eye()).normalize()).length() < 1e-5);
            assert!(f.dot(r).abs() < 1e-5 && f.dot(u).abs() < 1e-5 && r.dot(u).abs() < 1e-5);
            assert!((r.length() - 1.0).abs() < 1e-5 && r.y.abs() < 1e-5, "right stays level");
            assert!(c.view_proj(1.5).is_finite());
        }
    }

    #[test]
    fn orthographic_rays_are_parallel_and_keep_the_scale() {
        let mut c = OrbitCamera::framing(Vec3::ZERO, Vec3::splat(32.0));
        c.ortho = true;
        let (o0, d0) = c.ray(0.5, 0.5, 400.0, 300.0);
        let (o1, d1) = c.ray(399.5, 299.5, 400.0, 300.0);
        assert!((d0 - d1).length() < 1e-5 && (d0 - c.basis().0).length() < 1e-5);
        // The view is 2 * half_height tall at any depth.
        let (_, _, up) = c.basis();
        let (top, _) = c.ray(200.0, 0.0, 400.0, 300.0);
        let (bottom, _) = c.ray(200.0, 300.0, 400.0, 300.0);
        assert!(((top - bottom).dot(up) - 2.0 * c.half_height()).abs() < 1e-2);
        assert!((o0 - o1).length() > 1.0);
        // Rays start behind anything in the bounds.
        assert!((c.target - o0).dot(d0) > 3000.0);
    }

    #[test]
    fn snap_views_are_recognised() {
        let mut c = OrbitCamera::framing(Vec3::ZERO, Vec3::splat(32.0));
        assert_eq!(c.view(), None);
        for v in View::ALL {
            c.snap(v);
            assert_eq!(c.view(), Some(v), "{v:?}");
        }
        assert!(c.ortho, "isometric turns orthographic on");
        c.snap(View::Iso);
        c.ortho = false;
        assert_eq!(c.view(), None, "isometric needs orthographic");
        c.ortho = true;
        c.yaw += FRAC_PI_2 * 3.0;
        assert_eq!(c.view(), Some(View::Iso), "from any upper corner");
        c.orbit(5.0, 0.0);
        assert_eq!(c.view(), None);
        c.snap(View::Top);
        c.orbit(0.0, 50.0);
        assert_eq!(c.pitch, FRAC_PI_2, "orbit stops at straight down");
    }
}
