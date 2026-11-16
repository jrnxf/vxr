//! Saved views: a view saved into a camera slot reads back as the same view, from the scene
//! and from the saved file, adds a camera only where the file has none, and undoes.

use std::f32::consts::{FRAC_PI_2, TAU};

use proptest::prelude::*;
use vxr_core::Scene;
use vxr_core::views::SavedView;
use vxr_vox::Camera;
use vxr_vox::camera::Mode;

fn close(a: SavedView, b: SavedView) -> bool {
    let near = |x: f32, y: f32, tol: f32| (x - y).abs() <= tol * (1.0 + x.abs().max(y.abs()));
    // The file keeps six decimals, so a yaw near a full turn may read back as 0.
    let dyaw = (a.yaw - b.yaw).rem_euclid(TAU);
    a.target.iter().zip(b.target).all(|(x, y)| near(*x, y, 1e-5))
        && dyaw.min(TAU - dyaw) < 1e-4
        && near(a.pitch, b.pitch, 1e-4)
        && near(a.distance, b.distance, 1e-5)
        && a.ortho == b.ortho
}

fn view() -> impl Strategy<Value = SavedView> {
    (prop::array::uniform3(-3000.0f32..3000.0), 0.0f32..TAU, -FRAC_PI_2..=FRAC_PI_2, 0.5f32..9000.0, any::<bool>())
        .prop_map(|(target, yaw, pitch, distance, ortho)| SavedView { target, yaw, pitch, distance, ortho })
}

fn camera_file() -> Vec<u8> {
    vxr_vox::fixtures::all().into_iter().find(|f| f.name == "15-camera").unwrap().builder.build().write()
}

proptest! {
    // Each case saves and opens a file, so the default count is lower than the codec's.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    /// Any view saved into any slot of a new scene or an opened file reads back as itself,
    /// in the scene and after a save and an open, and the file's other cameras keep their values.
    #[test]
    fn a_saved_view_reads_back(v in view(), slot in 0..vxr_core::views::SLOTS, opened in any::<bool>()) {
        let mut s = if opened { Scene::from_vox(&camera_file()).unwrap() } else { Scene::new([16, 16, 16]) };
        let before = s.cameras.clone();
        prop_assert!(s.save_view(slot, v, 45.0));
        prop_assert!(close(s.saved_view(slot).unwrap(), v), "in the scene: {:?}", s.saved_view(slot));
        let back = Scene::from_vox(&s.to_vox().unwrap()).unwrap();
        prop_assert!(close(back.saved_view(slot).unwrap(), v), "from the file: {:?}", back.saved_view(slot));
        for c in before.iter().filter(|c| c.id != slot) {
            prop_assert_eq!(back.cameras.iter().find(|k| k.id == c.id), Some(c));
        }
    }
}

#[test]
fn a_new_slot_is_added_and_undone_and_a_file_slot_is_edited_in_place() {
    let v = SavedView { target: [4.0, 8.0, -2.5], yaw: 1.0, pitch: 0.4, distance: 30.0, ortho: true };

    // A new scene has no cameras: saving adds one, undo removes it, redo adds it again.
    let mut s = Scene::new([16, 16, 16]);
    assert!(s.saved_views().is_empty());
    assert!(s.save_view(3, v, 45.0));
    assert_eq!(s.cameras.iter().map(|c| c.id).collect::<Vec<_>>(), [3]);
    assert_eq!(s.cameras[0].mode, Mode::Orth);
    let empty = Scene::new([16, 16, 16]).to_vox().unwrap();
    assert!(s.undo());
    assert!(s.cameras.is_empty() && s.to_vox().unwrap() == empty, "undone: no rCAM written");
    assert!(s.redo());
    assert_eq!(vxr_vox::Document::read(&s.to_vox().unwrap()).unwrap().cameras(), s.cameras);

    // An opened file: slot 1 is edited in place, its frustum and id kept, roll set to 0.
    let bytes = camera_file();
    let mut s = Scene::from_vox(&bytes).unwrap();
    let ids: Vec<i32> = s.cameras.iter().map(|c| c.id).collect();
    let old = s.cameras[1];
    assert!(s.save_view(old.id, SavedView { ortho: false, ..v }, 45.0));
    assert_eq!(s.cameras.iter().map(|c| c.id).collect::<Vec<_>>(), ids, "no camera added");
    let new = s.cameras[1];
    assert_eq!((new.mode, new.frustum, new.angle[2]), (Mode::Pers, old.frustum, 0.0));
    let saved = s.to_vox().unwrap();
    let (a, b) = (vxr_vox::RawFile::parse(&bytes).unwrap(), vxr_vox::RawFile::parse(&saved).unwrap());
    assert_eq!(a.chunks.len(), b.chunks.len());
    assert_eq!((0..a.chunks.len()).filter(|&i| a.chunks[i] != b.chunks[i]).count(), 1, "only its rCAM changed");
    assert!(s.undo() && s.to_vox().unwrap() == bytes, "undone: the file's bytes");

    // An isometric slot stays isometric when an orthographic view is saved into it.
    let mut s = Scene::from_vox(&bytes).unwrap();
    s.set_camera(Camera { mode: Mode::Iso, ..s.cameras[0] });
    assert!(s.save_view(s.cameras[0].id, v, 45.0));
    assert_eq!(s.cameras[0].mode, Mode::Iso);
}

#[test]
fn empty_slots_read_as_none_and_bad_views_are_refused() {
    // MagicaVoxel writes every slot; one never saved into has radius 0.
    let mut s = Scene::from_vox(&camera_file()).unwrap();
    let n = s.saved_views().len();
    s.set_camera(Camera { radius: 0.0, ..s.cameras[0] });
    assert_eq!(s.saved_views().len(), n - 1);
    assert_eq!(s.saved_view(s.cameras[0].id), None);

    let v = SavedView { target: [0.0; 3], yaw: 0.0, pitch: 0.0, distance: 10.0, ortho: false };
    let mut s = Scene::new([8, 8, 8]);
    for (slot, bad, fov) in [
        (-1, v, 45.0),
        (10, v, 45.0),
        (0, SavedView { distance: 0.0, ..v }, 45.0),
        (0, SavedView { distance: f32::NAN, ..v }, 45.0),
        (0, SavedView { target: [0.0, f32::INFINITY, 0.0], ..v }, 45.0),
        (0, SavedView { pitch: 2.0, ..v }, 45.0),
        (0, SavedView { yaw: f32::NAN, ..v }, 45.0),
        (0, v, f32::NAN),
    ] {
        assert!(!s.save_view(slot, bad, fov), "{slot} {bad:?} {fov}");
    }
    assert!(s.cameras.is_empty() && !s.undo(), "nothing saved, no undo step");
}
