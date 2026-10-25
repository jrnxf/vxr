use std::collections::BTreeMap;

use proptest::prelude::*;
use vxr_core::Scene;
use vxr_core::scene::{vox_to_world, world_to_vox};
use vxr_vox::build::Builder;
use vxr_vox::scene::Rule;

fn world_voxels(s: &Scene) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    s.for_each_voxel(|p, c| {
        out.insert(s.to_world(p), c);
    });
    out
}

#[test]
fn undo_and_redo_restore_every_step() {
    let mut s = Scene::new([32, 32, 32]);
    assert_eq!(s.edit([([1, 1, 1], 3), ([2, 1, 1], 3)]), 2);
    assert_eq!(s.edit([([1, 1, 1], 9)]), 1, "paint");
    assert_eq!(s.edit([([2, 1, 1], 0)]), 1, "erase");
    assert_eq!(s.edit([([1, 1, 1], 9)]), 0, "no change, no step");
    s.set_color(9, [1, 2, 3, 255]);
    let done = (world_voxels(&s), s.palette);

    assert!(s.undo());
    assert_eq!(s.palette[9], vxr_vox::default_palette::DEFAULT_PALETTE[8]);
    assert!(s.undo() && s.undo());
    assert_eq!((s.map.get(1, 1, 1), s.map.get(2, 1, 1)), (3, 3));
    assert!(s.undo());
    assert_eq!(s.map.voxels, 0);
    assert!(!s.undo() && !s.history().can_undo());

    while s.redo() {}
    assert_eq!((world_voxels(&s), s.palette), done);

    s.undo();
    s.edit([([5, 5, 5], 1)]);
    assert!(!s.history().can_redo(), "a new edit drops the redo steps");
}

#[test]
fn undo_of_overlapping_cells_in_one_step_restores_the_first_value() {
    let mut s = Scene::new([8, 8, 8]);
    s.edit([([0, 0, 0], 4)]);
    s.edit([([0, 0, 0], 5), ([0, 0, 0], 6)]);
    s.undo();
    assert_eq!(s.map.get(0, 0, 0), 4);
    s.redo();
    assert_eq!(s.map.get(0, 0, 0), 6);
}

#[test]
fn axes_follow_magicavoxel_z_up() {
    // One voxel at file (1, 2, 3) in a 4x4x4 model placed at the origin.
    let mut b = Builder::new(200);
    let m = b.model([4, 4, 4], vec![[1, 2, 3, 7]]);
    b.root.push(vxr_vox::build::SceneNode::shape(m, None, [0, 0, 0]));
    let s = Scene::from_vox(&b.build().write()).unwrap();
    // The pivot is floor(size / 2) = 2, so the file cell is (-1, 0, 1); vxr y-up is (-1, 1, 0).
    assert_eq!(world_voxels(&s).into_iter().collect::<Vec<_>>(), vec![([-1, 1, 0], 7)]);
    assert_eq!(world_to_vox(vox_to_world([4, -5, 6])), [4, -5, 6]);
}

#[test]
fn every_vox_fixture_opens_and_saves_with_the_same_voxels_and_palette() {
    for f in vxr_vox::fixtures::all() {
        let bytes = f.builder.build().write();
        let file = vxr_vox::Document::read(&bytes).unwrap().scene().unwrap();
        let expect: BTreeMap<_, _> =
            file.world_voxels(Rule::Compose).into_iter().map(|(p, c)| (vox_to_world(p), c)).collect();
        if expect.keys().any(|p| p.iter().any(|&x| x.abs() > 1000)) {
            continue; // 20-far-positions spans more than MAX_EXTENT on purpose
        }
        let s = Scene::from_vox(&bytes).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        assert_eq!(world_voxels(&s), expect, "{}", f.name);
        let again = Scene::from_vox(&s.to_vox()).unwrap();
        assert_eq!(world_voxels(&again), expect, "{} after save", f.name);
        assert_eq!(again.palette, s.palette, "{}", f.name);
    }
}

#[test]
fn too_large_a_scene_is_an_error_not_an_allocation() {
    let mut b = Builder::new(200);
    let mut w = BTreeMap::new();
    w.insert([0, 0, 0], 1);
    w.insert([5000, 0, 0], 1);
    b.add_world(&w);
    assert!(Scene::from_vox(&b.build().write()).is_err());
}

#[test]
fn prototype_island_opens_and_saves() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    let Ok(bytes) = std::fs::read(&p) else {
        eprintln!("skipped: run `node prototype/src/build.mjs` to generate {}", p.display());
        return;
    };
    let s = Scene::from_vox(&bytes).unwrap();
    assert!(s.map.voxels > 10_000);
    let again = Scene::from_vox(&s.to_vox()).unwrap();
    assert_eq!(world_voxels(&again), world_voxels(&s));
    assert_eq!(again.palette, s.palette);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(200),
        ..ProptestConfig::default()
    })]

    /// Random edits saved and reopened land on the same world cells, and undoing them all
    /// gives back an empty scene.
    #[test]
    fn edits_survive_save_and_undo_to_empty(
        edits in prop::collection::vec(prop::collection::vec(((0i32..300, 0i32..40, 0i32..300), 0u8..=255), 1..40), 1..20),
    ) {
        let mut s = Scene::new([300, 40, 300]);
        for e in &edits {
            s.edit(e.iter().map(|&((x, y, z), c)| ([x, y, z], c)));
        }
        let before = world_voxels(&s);
        let total: u32 = s.map.counts[1..].iter().sum();
        prop_assert_eq!(total as u64, s.map.voxels);
        prop_assert_eq!(before.len() as u64, s.map.voxels);
        if !before.is_empty() {
            let again = Scene::from_vox(&s.to_vox()).unwrap();
            prop_assert_eq!(world_voxels(&again), before);
        }
        while s.undo() {}
        prop_assert_eq!(s.map.voxels, 0);
    }
}
