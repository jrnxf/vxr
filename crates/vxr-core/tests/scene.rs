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
fn a_dragged_colour_is_one_undo_step() {
    let mut s = Scene::new([8, 8, 8]);
    s.edit([([0, 0, 0], 4)]);
    let original = s.palette[4];
    for g in 0..=255u8 {
        s.put_color(4, [10, g, 30, 255]);
    }
    assert_eq!(s.palette[4], [10, 255, 30, 255], "the colour shows while it is dragged");
    s.commit();
    assert!(s.undo());
    assert_eq!(s.palette[4], original, "one undo returns the colour from before the drag");
    assert_eq!(s.map.get(0, 0, 0), 4, "the voxel edit before it is a separate step");
    assert!(s.redo());
    assert_eq!(s.palette[4], [10, 255, 30, 255]);

    let five = s.palette[5];
    s.put_color(5, [1, 1, 1, 255]);
    s.put_color(5, five);
    s.commit();
    assert!(s.undo() && s.palette[4] == original, "a colour dragged back to where it started adds no step");
}

#[test]
fn swapping_two_colours_keeps_the_look_and_undoes_in_one_step() {
    let mut s = Scene::new([8, 8, 8]);
    s.edit([([0, 0, 0], 3), ([1, 0, 0], 3), ([2, 0, 0], 7)]);
    s.set_color(3, [200, 0, 0, 255]);
    s.set_color(7, [0, 0, 200, 255]);
    let look = |s: &Scene| -> BTreeMap<[i32; 3], [u8; 4]> {
        let mut out = BTreeMap::new();
        s.for_each_voxel(|p, c| {
            out.insert(p, s.palette[c as usize]);
        });
        out
    };
    let before = (look(&s), s.palette, world_voxels(&s));
    assert_eq!(s.swap_colors(3, 7), 3);
    assert_eq!(look(&s), before.0, "every voxel shows the same colour");
    assert_eq!((s.palette[3], s.palette[7]), ([0, 0, 200, 255], [200, 0, 0, 255]));
    assert_eq!((s.map.get(0, 0, 0), s.map.get(2, 0, 0)), (7, 3));
    assert_eq!((s.map.counts[3], s.map.counts[7]), (1, 2));
    assert!(s.undo());
    assert_eq!((s.palette, world_voxels(&s)), (before.1, before.2));
    assert!(s.redo());
    assert_eq!(look(&s), before.0);
    assert_eq!(s.swap_colors(3, 3), 0);
    assert_eq!(s.swap_colors(0, 3), 0, "slot 0 is empty and never swapped");

    // Swapping with an unused slot moves the colour and its voxels there.
    let n = s.swap_colors(7, 100);
    assert_eq!((n, s.map.counts[7], s.map.counts[100]), (2, 0, 2));
    assert_eq!(look(&s), before.0);
}

#[test]
fn copying_a_colour_recolours_the_target_slot() {
    let mut s = Scene::new([8, 8, 8]);
    s.edit([([0, 0, 0], 3), ([1, 0, 0], 7)]);
    let old = s.palette[7];
    s.copy_color(3, 7);
    assert_eq!(s.palette[7], s.palette[3]);
    assert_eq!(s.map.get(1, 0, 0), 7, "voxels keep their slot");
    assert!(s.undo());
    assert_eq!(s.palette[7], old);
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

/// The generated fixtures that fit in a scene, plus the island when it has been built.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = vxr_vox::fixtures::all()
        .into_iter()
        .filter(|f| f.name != "20-far-positions")
        .map(|f| (f.name.to_string(), f.builder.build().write()))
        .collect();
    let island = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    if let Ok(b) = std::fs::read(island) {
        out.push(("island".into(), b));
    }
    out
}

#[test]
fn saving_an_opened_file_without_edits_writes_it_back_byte_for_byte() {
    for (name, bytes) in files() {
        let s = Scene::from_vox(&bytes).unwrap();
        assert!(s.to_vox() == bytes, "{name}");
    }
}

#[test]
fn open_edit_save_keeps_every_untouched_chunk() {
    for (name, bytes) in files() {
        let mut s = Scene::from_vox(&bytes).unwrap();
        let mut cells = Vec::new();
        s.for_each_voxel(|p, _| cells.push(p));
        let n = cells.len();
        s.edit([(cells[0], 0), (cells[n / 2], 33), (cells[n - 1], 0)]);
        s.set_color(33, [1, 2, 3, 255]);
        let expect = (world_voxels(&s), s.palette);

        let saved = s.to_vox();
        let again = Scene::from_vox(&saved).unwrap();
        assert_eq!((world_voxels(&again), again.palette), expect, "{name}");

        let before = vxr_vox::Document::read(&bytes).unwrap();
        let after = vxr_vox::Document::read(&saved).unwrap();
        let file = before.scene().unwrap();
        let mut allowed: Vec<usize> = file.models.iter().map(|m| m.data_chunk).collect();
        allowed.extend(before.raw.chunks.iter().position(|c| c.is(vxr_vox::chunk::RGBA)));
        // Linked copies split off as new models, which shifts the chunks after the last
        // model and repoints their nSHP. Compare everything else in order.
        let (a, b) = (&before.raw.chunks, &after.raw.chunks);
        let extra = b.len() - a.len();
        let last = file.models.iter().map(|m| m.data_chunk).max().unwrap();
        for (i, c) in a.iter().enumerate() {
            let now = if i <= last { &b[i] } else { &b[i + extra] };
            let ok = now == c || allowed.contains(&i) || (extra > 0 && c.is(vxr_vox::chunk::NSHP));
            assert!(ok, "{name}: chunk {i} ({}) changed", vxr_vox::id_str(&c.id));
        }
    }
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

#[test]
fn dirty_box_covers_every_changed_voxel() {
    let mut s = Scene::new([64, 64, 64]);
    s.map.track_dirty(true);
    assert_eq!(s.map.take_dirty_box(), None);
    s.edit([([3, 4, 5], 1), ([10, 2, 7], 2)]);
    assert_eq!(s.map.take_dirty_box(), Some(([3, 2, 5], [10, 4, 7])));
    assert_eq!(s.map.take_dirty_box(), None, "taking it clears it");
    // Setting a voxel to the value it has is no change.
    s.edit([([3, 4, 5], 1)]);
    assert_eq!(s.map.take_dirty_box(), None);
    // Undo reverts the whole first edit, so the box covers both voxels again.
    assert!(s.undo());
    assert_eq!(s.map.take_dirty_box(), Some(([3, 2, 5], [10, 4, 7])));
}

#[test]
fn materials_undo_swap_and_copy_with_their_slot() {
    use vxr_vox::{Kind, Material};
    let mut s = Scene::new([8, 8, 8]);
    s.edit([([0, 0, 0], 3), ([1, 0, 0], 7)]);
    let metal = Material { kind: Kind::Metal, rough: 0.3, ..Material::default() };
    // A dragged slider is one step.
    s.put_material(3, Material { rough: 0.9, ..metal });
    s.put_material(3, metal);
    s.commit();
    assert_eq!(s.materials[3], metal);
    assert!(s.undo());
    assert_eq!(s.materials[3], Material::default());
    assert!(s.redo());
    assert_eq!(s.materials[3], metal);

    s.swap_colors(3, 7);
    assert_eq!((s.materials[3], s.materials[7]), (Material::default(), metal), "the material moves with the voxels");
    assert!(s.undo());
    assert_eq!(s.materials[3], metal);

    s.copy_color(3, 9);
    assert_eq!(s.materials[9], metal);
    assert!(s.undo());
    assert_eq!(s.materials[9], Material::default());
}

#[test]
fn materials_open_and_save_as_matl() {
    use vxr_vox::fixtures::{CYAN, ORANGE, YELLOW};
    use vxr_vox::{Kind, Material};
    let bytes =
        vxr_vox::fixtures::all().into_iter().find(|f| f.name == "14-materials").unwrap().builder.build().write();
    let mut s = Scene::from_vox(&bytes).unwrap();
    assert_eq!(s.materials[YELLOW as usize].kind, Kind::Metal);
    assert_eq!((s.materials[CYAN as usize].kind, s.materials[CYAN as usize].ior), (Kind::Glass, 1.5));
    assert_eq!(s.materials[ORANGE as usize].kind, Kind::Emit);
    assert!(s.to_vox() == bytes, "unedited: byte-identical");

    // Edit one slot: only its MATL chunk changes, and the other keys in it stay.
    let glass = Material { kind: Kind::Glass, rough: 0.05, ior: 1.6, trans: 0.7, ..Material::default() };
    s.set_material(YELLOW, glass);
    let saved = s.to_vox();
    assert_eq!(Scene::from_vox(&saved).unwrap().materials, s.materials);
    let (a, b) = (vxr_vox::RawFile::parse(&bytes).unwrap(), vxr_vox::RawFile::parse(&saved).unwrap());
    assert_eq!(a.chunks.len(), b.chunks.len());
    let changed: Vec<usize> = (0..a.chunks.len()).filter(|&i| a.chunks[i] != b.chunks[i]).collect();
    assert_eq!(changed.len(), 1);
    let d = vxr_vox::chunk::IdDict::parse(vxr_vox::chunk::MATL, &b.chunks[changed[0]].content).unwrap();
    assert_eq!((d.id, d.dict.get_str("_type"), d.dict.get_str("_spec")), (YELLOW as i32, Some("_glass"), Some("1")));
    assert!(s.undo());
    assert!(s.to_vox() == bytes, "undone: byte-identical again");

    // A new scene writes MATL chunks for the slots that are not plain diffuse.
    let mut n = Scene::new([8, 8, 8]);
    n.edit([([0, 0, 0], 5)]);
    n.set_material(5, Material { kind: Kind::Emit, emit: 0.5, flux: 2.0, ..Material::default() });
    let back = Scene::from_vox(&n.to_vox()).unwrap();
    assert_eq!(back.materials, n.materials);
}
