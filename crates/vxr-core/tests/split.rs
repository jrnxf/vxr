//! Selection to new object: the selected voxels leave the model being edited and become an
//! object of their own, in place, as one undo step.

use std::collections::BTreeMap;

use proptest::prelude::*;
use vxr_core::Scene;
use vxr_core::history::At;
use vxr_core::scene::SplitError;
use vxr_core::select::Mode;
use vxr_vox::build::Builder;
use vxr_vox::{Document, NodeKind};

fn fixture(name: &str) -> Vec<u8> {
    vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).unwrap().builder.build().write()
}

fn find(s: &Scene, name: &str) -> usize {
    let w = s.world().unwrap();
    (0..w.tree.nodes.len()).find(|&i| w.tree.nodes[i].name.as_deref() == Some(name.as_bytes())).unwrap()
}

/// Object `i`'s voxels by world cell, from the world's copy of its model.
fn object_voxels(s: &Scene, i: usize) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    s.world().unwrap().for_each_object_voxel(i, |p, c| {
        out.insert(p, c);
    });
    out
}

/// The map's voxels by world cell.
fn map_voxels(s: &Scene) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    s.for_each_voxel(|p, c| {
        out.insert(s.to_world(p), c);
    });
    out
}

#[test]
fn a_selection_becomes_an_object_in_place_as_one_undo_step() {
    let bytes = fixture("22-world");
    let mut s = Scene::from_vox(&bytes).unwrap();
    let (palette, materials) = (s.palette, s.materials);
    let all = s.world_voxels(false);
    let slab = find(&s, "slab");
    assert!(s.edit_object(slab));
    let slab_voxels = map_voxels(&s);
    // The slab's left half.
    let mid = s.bounds().lo[0] + s.bounds().size[0] as i32 / 2;
    let mut cells = Vec::new();
    s.for_each_voxel(|p, _| {
        if s.to_world(p)[0] < mid {
            cells.push(p);
        }
    });
    let picked: BTreeMap<[i32; 3], u8> =
        slab_voxels.iter().filter(|(p, _)| p[0] < mid).map(|(&p, &c)| (p, c)).collect();
    assert!(!picked.is_empty() && picked.len() < slab_voxels.len());
    s.select(cells, Mode::Replace);
    let steps = s.history().position();

    let new = s.selection_to_object().unwrap();
    assert_eq!(s.history().position(), steps + 1, "one undo step");
    assert_eq!(s.at(), At::Object(new), "editing the new object");
    assert_eq!(s.selected_count(), picked.len(), "its voxels stay selected");
    assert_eq!(map_voxels(&s), picked, "the map is the new object's model, in place");
    let size: [u32; 3] = std::array::from_fn(|a| {
        let (lo, hi) = (picked.keys().map(|p| p[a]).min().unwrap(), picked.keys().map(|p| p[a]).max().unwrap());
        (hi - lo + 1) as u32
    });
    assert_eq!(s.bounds().size, size, "sized to its voxels");

    let w = s.world().unwrap();
    let parents = w.tree.parents();
    assert_eq!(parents[new], parents[slab], "same parent");
    let kids = w.tree.children(parents[slab].unwrap());
    assert_eq!(kids.iter().position(|&k| k == new), kids.iter().position(|&k| k == slab).map(|k| k + 1));
    assert_eq!(w.tree.nodes[new].layer, w.tree.nodes[slab].layer, "same layer");
    assert!(!w.tree.nodes[new].hidden);
    assert_eq!(w.tree.nodes[new].name.as_deref(), Some(&b"object 1"[..]), "every other object has a name");
    assert_ne!(w.model_of(new), w.model_of(slab));
    let rest: BTreeMap<_, _> = slab_voxels.iter().filter(|(p, _)| p[0] >= mid).map(|(&p, &c)| (p, c)).collect();
    assert_eq!(object_voxels(&s, slab), rest, "the slab lost exactly the selected voxels");
    assert_eq!(object_voxels(&s, new), picked, "the new object has exactly them");
    assert_eq!(s.world_voxels(false), all, "nothing moved in the world");
    assert_eq!((s.palette, s.materials), (palette, materials), "palette and materials are the scene's");

    // Saved and reopened: the same world, two objects where the slab was, and every chunk
    // the split did not need keeps its bytes.
    let saved = s.to_vox().unwrap();
    let again = Scene::from_vox(&saved).unwrap();
    assert_eq!(again.world_voxels(false), all);
    assert_eq!(again.world().unwrap().objects().len(), s.world().unwrap().objects().len());
    let (doc, out) = (Document::read(&bytes).unwrap(), Document::read(&saved).unwrap());
    let file = doc.scene().unwrap();
    let opened = Scene::from_vox(&bytes).unwrap();
    let ow = opened.world().unwrap();
    let parent_grp = file.node_chunks[&ow.tree.nodes[parents[slab].unwrap()].ids.unwrap().1];
    let slab_data = file.models[ow.model_of(slab).unwrap()].data_chunk;
    for (k, c) in doc.raw.chunks.iter().enumerate() {
        if k != parent_grp && k != slab_data {
            assert!(
                out.raw.chunks.iter().any(|o| o.id == c.id && o.content == c.content),
                "chunk {k} ({}) kept",
                vxr_vox::id_str(&c.id)
            );
        }
    }

    // Undo: the slab whole again and its selection, byte for byte; redo: the split again.
    assert!(s.undo());
    assert_eq!(s.at(), At::Object(slab));
    assert_eq!(map_voxels(&s), slab_voxels);
    assert_eq!(s.selected_count(), picked.len());
    assert!(s.to_vox().unwrap() == bytes, "undone: the opened file");
    assert!(s.redo());
    assert_eq!(s.at(), At::Object(new));
    assert_eq!(s.selected_count(), picked.len());
    assert!(s.to_vox().unwrap() == saved, "redone: the same file");

    // Tab to the world and back keeps it; a rename in model mode is its own step.
    assert!(s.show_world());
    assert!(s.pick(Some(new)));
    assert!(s.edit_object(new));
    assert_eq!(map_voxels(&s), picked);
    assert!(s.set_node_name(new, Some("mug")));
    assert_eq!(s.history().position(), steps + 2);
    assert_eq!(s.at(), At::Object(new), "a rename keeps the model open");
    assert_eq!(s.world().unwrap().tree.nodes[new].name.as_deref(), Some(&b"mug"[..]));
    assert!(s.undo());
    assert_eq!(s.world().unwrap().tree.nodes[new].name.as_deref(), Some(&b"object 1"[..]));
    assert!(s.to_vox().unwrap() == saved);
    assert!(s.undo());
    assert_eq!(s.picked(), None, "the pick goes with the object undo takes away");
    assert!(s.to_vox().unwrap() == bytes);
}

#[test]
fn the_split_is_refused_when_it_cannot_make_a_new_object() {
    let bytes = fixture("22-world");
    let mut s = Scene::from_vox(&bytes).unwrap();
    assert_eq!(s.selection_to_object(), Err(SplitError::World), "world mode selects objects, not voxels");
    let tree_a = find(&s, "tree a");
    assert!(s.edit_object(tree_a));
    assert_eq!(s.selection_to_object(), Err(SplitError::NothingSelected));
    s.select_all();
    assert_eq!(s.selection_to_object(), Err(SplitError::Everything), "the whole model is the object already");
    let mut one = None;
    s.for_each_voxel(|p, _| one = one.or(Some(p)));
    s.select([one.unwrap()], Mode::Replace);
    let steps = s.history().position();
    assert_eq!(s.selection_to_object(), Err(SplitError::Linked), "tree a shares its model with b and c");
    assert_eq!(s.history().position(), steps);
    assert!(s.to_vox().unwrap() == bytes, "nothing changed");
}

#[test]
fn a_new_scene_becomes_a_world_of_two_objects() {
    let mut s = Scene::new([32, 32, 32]);
    let box_at = |lo: [i32; 3], c: u8| {
        let mut v = Vec::new();
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    v.push(([lo[0] + x, lo[1] + y, lo[2] + z], c));
                }
            }
        }
        v
    };
    s.edit(box_at([2, 0, 2], 3));
    s.edit(box_at([20, 0, 20], 7));
    let (all, bytes, map) = (s.world_voxels(false), s.to_vox().unwrap(), map_voxels(&s));
    s.select_color(7, Mode::Replace);
    assert_eq!(s.selected_count(), 64);

    let new = s.selection_to_object().unwrap();
    assert_eq!(s.at(), At::Object(new));
    let w = s.world().unwrap();
    assert_eq!(w.objects().len(), 2);
    assert_eq!(w.tree.nodes[new].name.as_deref(), Some(&b"object 2"[..]), "the first object shows as object 1");
    assert_eq!(s.world_voxels(false), all);
    assert_eq!(s.selected_count(), 64);
    let saved = s.to_vox().unwrap();
    let again = Scene::from_vox(&saved).unwrap();
    assert_eq!(again.world().unwrap().objects().len(), 2, "saves as two objects");
    assert_eq!(again.world_voxels(false), all);

    // Undo takes the world away again: the new scene's map, file and edits are back.
    assert!(s.undo());
    assert!(s.world().is_none());
    assert_eq!(s.at(), At::Map);
    assert_eq!(map_voxels(&s), map);
    assert_eq!(s.bounds().size, [32, 32, 32]);
    assert_eq!(s.selected_count(), 64);
    assert!(s.to_vox().unwrap() == bytes, "undone: the same file");
    assert!(s.redo());
    assert!(s.to_vox().unwrap() == saved, "redone: the same file");
    assert!(s.undo());
    assert!(s.undo() && s.undo());
    assert_eq!(s.map.voxels, 0, "the edits before it undo as before");
}

#[test]
fn a_selection_over_256_saves_and_reopens() {
    let mut s = Scene::new([300, 4, 4]);
    s.edit((0..300).map(|x| ([x, 0, 1], 1 + (x % 3) as u8)));
    let all = s.world_voxels(false);
    s.select((0..280).map(|x| [x, 0, 1]), Mode::Replace);
    let new = s.selection_to_object().unwrap();
    assert_eq!(s.bounds().size, [280, 1, 1]);
    let again = Scene::from_vox(&s.to_vox().unwrap()).unwrap();
    assert_eq!(again.world_voxels(false), all, "the model over 256 saves as blocks");
    let w = again.world().unwrap();
    assert!(w.objects().len() >= 3, "the 280 voxel row is two blocks beside the rest");
    assert!(
        w.tree
            .nodes
            .iter()
            .any(|n| n.name == s.world().unwrap().tree.nodes[new].name && matches!(n.kind, NodeKind::Group(_)))
    );
}

/// A file of one model holding `voxels` (model cells, inside 12^3).
fn one_model(voxels: &BTreeMap<[i32; 3], u8>) -> Vec<u8> {
    let mut b = Builder::new(200);
    let v = voxels.iter().map(|(p, &c)| [p[0] as u8, p[1] as u8, p[2] as u8, c]).collect();
    let m = b.model([12, 12, 12], v);
    b.root.push(vxr_vox::build::SceneNode::shape(m, None, [3, -2, 5]));
    b.build().write()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        ..ProptestConfig::default()
    })]

    /// Random selections of random models: the world's voxels are the same after a split
    /// (and saved and reopened), and undo gives back the file's bytes.
    #[test]
    fn a_split_keeps_every_world_voxel_and_undoes_to_the_file(
        voxels in prop::collection::btree_map((0i32..12, 0i32..12, 0i32..12), 1u8..=255, 2..120),
        pick in prop::collection::vec(any::<bool>(), 120),
        new_scene in any::<bool>(),
    ) {
        let voxels: BTreeMap<[i32; 3], u8> = voxels.into_iter().map(|((x, y, z), c)| ([x, y, z], c)).collect();
        let mut s = if new_scene {
            let mut s = Scene::new([12, 12, 12]);
            s.edit(voxels.iter().map(|(&p, &c)| (p, c)));
            s
        } else {
            Scene::from_vox(&one_model(&voxels)).unwrap()
        };
        let (all, bytes) = (s.world_voxels(false), s.to_vox().unwrap());
        let mut cells = Vec::new();
        s.for_each_voxel(|p, _| cells.push(p));
        let chosen: Vec<_> = cells.iter().zip(&pick).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
        s.select(chosen.iter().copied(), Mode::Replace);
        match s.selection_to_object() {
            Ok(new) => {
                prop_assert_eq!(s.at(), At::Object(new));
                prop_assert_eq!(s.selected_count(), chosen.len());
                prop_assert_eq!(s.world().unwrap().objects().len(), 2);
                prop_assert_eq!(s.world_voxels(false), all.clone());
                let again = Scene::from_vox(&s.to_vox().unwrap()).unwrap();
                prop_assert_eq!(again.world_voxels(false), all);
                prop_assert!(s.undo());
                prop_assert!(s.to_vox().unwrap() == bytes);
            }
            Err(e) => {
                let want = if chosen.is_empty() { SplitError::NothingSelected } else { SplitError::Everything };
                prop_assert_eq!(e, want);
                prop_assert!(chosen.is_empty() || chosen.len() == cells.len());
            }
        }
    }

    /// The same inside an open edit (an AI change, applied at once): random groups of a
    /// random model, some voxels painted first, become objects with the map staying on the
    /// model; the edit closes as one undo step that keeps every world voxel and undoes to the
    /// file, and putting the edit back instead leaves the file as it was.
    #[test]
    fn objects_made_inside_an_edit_keep_every_world_voxel(
        voxels in prop::collection::btree_map((0i32..12, 0i32..12, 0i32..12), 1u8..=255, 3..120),
        group in prop::collection::vec(0u8..3, 120),
        new_scene in any::<bool>(),
        revert in any::<bool>(),
    ) {
        let voxels: BTreeMap<[i32; 3], u8> = voxels.into_iter().map(|((x, y, z), c)| ([x, y, z], c)).collect();
        let mut s = if new_scene {
            let mut s = Scene::new([12, 12, 12]);
            s.edit(voxels.iter().map(|(&p, &c)| (p, c)));
            s
        } else {
            Scene::from_vox(&one_model(&voxels)).unwrap()
        };
        let bytes = s.to_vox().unwrap();
        let mut cells = Vec::new();
        s.for_each_voxel(|p, v| cells.push((p, v)));
        // Group 0 stays; groups 1 and 2 become objects. The first voxel is repainted in the
        // same edit.
        let (at, start) = (s.at(), s.history().position());
        s.begin();
        s.put(cells[0].0, 7);
        cells[0].1 = 7;
        let expect = map_voxels(&s);
        let mut made = Vec::new();
        for g in 1..3u8 {
            let part: Vec<_> = cells.iter().zip(&group).filter(|(_, k)| **k == g).map(|(c, _)| *c).collect();
            match s.put_object(&part, Some(&format!("part {g}"))) {
                Ok(n) => made.push((n, part.len())),
                Err(e) => {
                    let left = cells.len() - made.iter().map(|m| m.1).sum::<usize>();
                    prop_assert!(part.is_empty() || part.len() >= left, "{:?}", e);
                }
            }
        }
        if made.is_empty() {
            s.revert_voxels();
            s.commit();
            prop_assert!(s.to_vox().unwrap() == bytes);
            return Ok(());
        }
        prop_assert!(matches!(s.at(), At::Object(_)), "the map stays on the model");
        if revert {
            s.revert_voxels();
            s.commit();
            prop_assert_eq!(s.at(), at);
            prop_assert_eq!(s.history().position(), start);
            prop_assert!(s.to_vox().unwrap() == bytes, "put back: the file as it was");
            return Ok(());
        }
        let position = s.history().position();
        s.commit();
        prop_assert_eq!(s.history().position(), position + 1, "one undo step");
        prop_assert_eq!(s.world().unwrap().objects().len(), 1 + made.len());
        for (n, k) in &made {
            prop_assert_eq!(object_voxels(&s, *n).len(), *k);
        }
        prop_assert_eq!(s.world_voxels(false), expect.clone());
        let saved = s.to_vox().unwrap();
        prop_assert_eq!(Scene::from_vox(&saved).unwrap().world_voxels(false), expect);
        prop_assert!(s.undo());
        prop_assert_eq!(s.at(), at);
        prop_assert!(s.to_vox().unwrap() == bytes, "undone: the file");
        prop_assert!(s.redo());
        prop_assert!(s.to_vox().unwrap() == saved, "redone: the same file");
    }
}
