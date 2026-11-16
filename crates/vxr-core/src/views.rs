//! Saved views: the file's cameras (`rCAM`) as the editor's orbit camera, and back.
//!
//! MagicaVoxel keeps ten camera slots per file (ids 0 to 9) and writes every one, with a
//! `_radius` of 0 for a slot never saved into. vxr shows them as views 1 to 10. A view is
//! the orbit target in world space (y up, a point, not a cell: world cell `c` spans `c` to
//! `c + 1`), the yaw and pitch in radians as `vxr_render::OrbitCamera` has them, the
//! distance and the projection. In the file, `_focus` is the target in the file's z-up
//! axes, `_angle` is pitch, yaw and roll in degrees and `_radius` the distance. vxr has no
//! roll, so a saved view writes 0 and an opened one ignores it. Yaw 0 looks from the front
//! (world +z, the file's -y) and a positive pitch looks down from above.

use vxr_vox::Camera;
use vxr_vox::camera::Mode;

/// The camera slots a file has room for, as MagicaVoxel writes them.
pub const SLOTS: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedView {
    /// The point the view orbits, in world space (y up).
    pub target: [f32; 3],
    /// Radians around +y; 0 looks from +z.
    pub yaw: f32,
    /// Radians above the horizon, -pi/2 to pi/2.
    pub pitch: f32,
    /// Distance from the target, above 0.
    pub distance: f32,
    pub ortho: bool,
}

impl SavedView {
    /// The view a camera slot holds, or None for an empty slot (a `_radius` of 0) or one
    /// that is not finite.
    pub fn from_camera(c: &Camera) -> Option<SavedView> {
        let f = c.focus;
        let finite = f.iter().chain(&c.angle).all(|v| v.is_finite()) && c.radius.is_finite();
        if !finite || c.radius <= 0.0 {
            return None;
        }
        Some(SavedView {
            target: [f[0], f[2], 1.0 - f[1]],
            yaw: c.angle[1].to_radians().rem_euclid(std::f32::consts::TAU),
            pitch: c.angle[0].clamp(-90.0, 90.0).to_radians(),
            distance: c.radius,
            ortho: matches!(c.mode, Mode::Orth | Mode::Iso),
        })
    }

    /// Camera `old` holding this view: its focus, angle, radius and projection replaced, its
    /// field of view set to `fov` degrees, its frustum and id kept. An isometric slot saved
    /// with an orthographic view stays isometric.
    pub fn to_camera(&self, old: Camera, fov: f32) -> Camera {
        let t = self.target;
        let mode = match (self.ortho, old.mode) {
            (true, Mode::Iso) => Mode::Iso,
            (true, _) => Mode::Orth,
            (false, _) => Mode::Pers,
        };
        Camera {
            mode,
            focus: [t[0], 1.0 - t[2], t[1]],
            angle: [self.pitch.to_degrees(), self.yaw.to_degrees().rem_euclid(360.0), 0.0],
            radius: self.distance,
            fov,
            ..old
        }
    }

    fn valid(&self) -> bool {
        let finite = self.target.iter().chain([&self.yaw, &self.pitch, &self.distance]).all(|v| v.is_finite());
        finite && self.distance > 0.0 && self.pitch.abs() <= std::f32::consts::FRAC_PI_2 + 1e-4
    }
}

impl crate::Scene {
    /// The view saved in slot `slot` (0 to 9), if there is one.
    pub fn saved_view(&self, slot: i32) -> Option<SavedView> {
        self.cameras.iter().find(|c| c.id == slot).and_then(SavedView::from_camera)
    }

    /// Every slot with a view in it, in slot order.
    pub fn saved_views(&self) -> Vec<(i32, SavedView)> {
        (0..SLOTS).filter_map(|k| Some((k, self.saved_view(k)?))).collect()
    }

    /// Save `view` into slot `slot` (0 to 9) as one undo step, with a field of view of `fov`
    /// degrees: the file's camera when it has one, else a new camera, which a save writes
    /// as a new `rCAM`. Returns false, changing nothing, for a slot outside 0 to 9 or a view
    /// that is not finite, has no distance or looks past straight up or down.
    pub fn save_view(&mut self, slot: i32, view: SavedView, fov: f32) -> bool {
        self.commit();
        let ok = self.put_view(slot, view, fov);
        self.commit();
        ok
    }

    /// [`Scene::save_view`] inside the open edit (opening one if needed), for a caller that
    /// commits the step itself.
    pub fn put_view(&mut self, slot: i32, view: SavedView, fov: f32) -> bool {
        if !(0..SLOTS).contains(&slot) || !view.valid() || !fov.is_finite() {
            return false;
        }
        let old = self.cameras.iter().find(|c| c.id == slot).copied().unwrap_or(Camera::new(slot));
        self.put_camera_or_add(view.to_camera(old, fov))
    }
}
