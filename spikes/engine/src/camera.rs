//! MagicaVoxel-style orbit camera: orbit around a target, pan, zoom toward the cursor.

use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians around +y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
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
        let mut c = Self { target, yaw: 0.6, pitch: 0.6, distance: radius / (fov_y * 0.5).sin(), fov_y };
        c.fit(lo, hi, aspect, 0.9);
        c
    }

    /// Keep yaw and pitch; move the target in the view plane and the distance so the box's
    /// projected rectangle is centred and spans `fill` of the screen on its tighter axis.
    pub fn fit(&mut self, lo: Vec3, hi: Vec3, aspect: f32, fill: f32) {
        let corners: Vec<Vec3> = (0..8)
            .map(|i| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }))
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
            let eye = self.eye();
            let fwd = (self.target - eye).normalize();
            let right = fwd.cross(Vec3::Y).normalize();
            let up = right.cross(fwd);
            let half_h = self.distance * (self.fov_y * 0.5).tan();
            self.target += right * cx * half_h * aspect + up * cy * half_h;
            self.distance *= (ext / fill).clamp(0.5, 2.0);
        }
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cp * sy, sp, cp * cy) * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y);
        let proj = glam::camera::rh::proj::directx::perspective(self.fov_y, aspect, 0.5, 1.0e5);
        proj * view
    }

    /// Ray through a pixel (pixel centre coords, origin top left).
    pub fn ray(&self, px: f32, py: f32, w: f32, h: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(w / h).inverse();
        let ndc_x = px / w * 2.0 - 1.0;
        let ndc_y = 1.0 - py / h * 2.0;
        let near = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        (self.eye(), (far - near).normalize())
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.55, 1.55);
    }

    pub fn pan(&mut self, dx: f32, dy: f32, viewport_h: f32) {
        let eye = self.eye();
        let fwd = (self.target - eye).normalize();
        let right = fwd.cross(Vec3::Y).normalize();
        let up = right.cross(fwd);
        let scale = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_h;
        self.target += (-right * dx + up * dy) * scale;
    }

    /// Zoom toward the point under the cursor at depth `hit_t` (or the target if none).
    pub fn zoom(&mut self, steps: f32, cursor_dir: Option<(Vec3, f32)>) {
        let factor = 0.88f32.powf(steps);
        if let Some((dir, t)) = cursor_dir {
            let eye = self.eye();
            let p = eye + dir * t;
            self.target = p + (self.target - p) * factor;
        }
        self.distance = (self.distance * factor).max(1.0);
    }
}
