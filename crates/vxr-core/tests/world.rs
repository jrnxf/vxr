//! The world: objects, groups, linked copies and layers read from `.vox`, edited and saved
//! back losslessly.

use std::collections::BTreeMap;

use vxr_core::World;
use vxr_core::scene::vox_to_world;
use vxr_vox::build::{Builder, SceneNode};
use vxr_vox::scene::Rule;
use vxr_vox::{Document, NodeKind, WorldSave, chunk};

/// The generated fixtures, plus the island when it has been built.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> =
        vxr_vox::fixtures::all().into_iter().map(|f| (f.name.to_string(), f.builder.build().write())).collect();
    let island = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    if let Ok(b) = std::fs::read(island) {
        out.push(("island".into(), b));
    }
    out
}

fn fixture(name: &str) -> Vec<u8> {
    vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).unwrap().builder.build().write()
}

fn open(bytes: &[u8]) -> (Document, World) {
    let doc = Document::read(bytes).unwrap();
    let w = World::from_doc(&doc).unwrap();
    (doc, w)
}

fn find(w: &World, name: &str) -> usize {
    (0..w.tree.nodes.len()).find(|&i| w.tree.nodes[i].name.as_deref() == Some(name.as_bytes())).unwrap()
}

/// Names, flags, layers, transforms and kinds in tree order: what a reopened world must keep.
fn outline(w: &World) -> Vec<String> {
    w.tree
        .order()
        .into_iter()
        .map(|i| {
            let n = &w.tree.nodes[i];
            let kind = match &n.kind {
                NodeKind::Group(c) => format!("group of {}", c.len()),
                NodeKind::Object(m) => format!("{} voxels", w.models[*m].voxels),
            };
            let name = n.name.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            format!("{name} hidden={} layer={} {:?} {kind}", n.hidden, n.layer, w.world_transform(i))
        })
        .collect()
}

#[test]
fn every_file_opens_as_the_same_voxels_and_saves_unedited_byte_for_byte() {
    for (name, bytes) in files() {
        let (mut doc, w) = open(&bytes);
        let flat: BTreeMap<[i32; 3], u8> =
            doc.scene().unwrap().world_voxels(Rule::Compose).into_iter().map(|(p, c)| (vox_to_world(p), c)).collect();
        assert_eq!(w.world_voxels(false), flat, "{name}");
        assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Kept, "{name}");
        assert!(doc.write() == bytes, "{name}: unedited save changed the file");
    }
}

#[test]
fn a_world_file_opens_as_separate_objects() {
    let (_, w) = open(&fixture("22-world"));
    let names: Vec<_> =
        w.objects().into_iter().map(|i| String::from_utf8(w.tree.nodes[i].name.clone().unwrap()).unwrap()).collect();
    assert_eq!(names, ["slab", "tree a", "tree b", "tree c", "gem", "lost gem"]);
    // Three linked copies of one model, two of another.
    let a = w.model_of(find(&w, "tree a"));
    assert_eq!(w.model_of(find(&w, "tree b")), a);
    assert_eq!(w.model_of(find(&w, "tree c")), a);
    assert!(w.shown(find(&w, "tree c")));
    assert!(!w.shown(find(&w, "gem")), "gem is on the hidden secret layer");
    assert!(!w.shown(find(&w, "lost gem")), "lost gem is hidden");
    assert_eq!(w.world_voxels(true).len() + 2, w.world_voxels(false).len());
}

#[test]
fn world_edits_save_into_the_file_and_reopen_the_same() {
    let bytes = fixture("22-world");
    let (mut doc, mut w) = open(&bytes);
    let before = doc.scene().unwrap();
    let tree_b = find(&w, "tree b");
    let tree_b_chunk = before.node_chunks[&w.tree.nodes[tree_b].ids.unwrap().0];

    let tree_a = find(&w, "tree a");
    w.move_by(tree_a, [0, 2, -1]);
    w.move_by(find(&w, "trees"), [1, 0, 0]);
    let slab = find(&w, "slab");
    w.set_name(slab, Some("floor"));
    w.set_hidden(find(&w, "grove"), true);
    let gem = find(&w, "gem");
    let copy = w.copy(gem, false).unwrap();
    w.set_name(copy, Some("gem copy"));
    w.move_by(copy, [0, 4, 0]);
    let link = w.copy(find(&w, "tree c"), true).unwrap();
    w.set_name(link, Some("tree d"));
    let pair = w.group(&[slab, copy]).unwrap();
    w.set_name(pair, Some("pair"));
    let shown = w.add_layer("shown");
    w.set_layer(copy, shown);
    w.layer_mut(2).unwrap().hidden = false;
    w.layer_mut(0).unwrap().name = Some(b"earth".to_vec());
    // An edit to the shared tree model shows in all four copies.
    let m = w.model_of(tree_a).unwrap();
    w.models[m].set(1, 4, 1, 4);
    let edited = w.world_voxels(false);
    let edited_outline = outline(&w);

    assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Kept);
    let (doc2, back) = open(&doc.write());
    assert_eq!(back.world_voxels(false), edited);
    assert_eq!(outline(&back), edited_outline);
    assert_eq!(back.model_of(find(&back, "tree d")), back.model_of(find(&back, "tree a")), "still linked");
    assert_ne!(back.model_of(find(&back, "gem copy")), back.model_of(find(&back, "gem")), "a duplicate");
    assert_eq!(back.tree.layers.iter().find(|l| l.id == shown).unwrap().name.as_deref(), Some(&b"shown"[..]));

    // Untouched chunks keep their bytes: palette, materials, tree b's node, the trees layer.
    let after = doc2.scene().unwrap();
    let kept = |id: &[u8; 4]| doc2.raw.chunks.iter().filter(|c| &c.id == id).count();
    assert_eq!(kept(chunk::RGBA), 1);
    let rgba = |d: &Document| d.raw.chunks.iter().find(|c| c.is(chunk::RGBA)).unwrap().content.clone();
    assert_eq!(rgba(&doc2), rgba(&Document::read(&bytes).unwrap()));
    let b_id = w.tree.nodes[tree_b].ids.unwrap().0;
    assert_eq!(
        doc2.raw.chunks[after.node_chunks[&b_id]].content,
        Document::read(&bytes).unwrap().raw.chunks[tree_b_chunk].content
    );
    let layr = |d: &Document, id: i32| {
        d.raw
            .chunks
            .iter()
            .find(|c| c.is(chunk::LAYR) && vxr_vox::chunk::Layr::parse(&c.content).unwrap().id == id)
            .unwrap()
            .content
            .clone()
    };
    assert_eq!(layr(&doc2, 1), layr(&Document::read(&bytes).unwrap(), 1));
}

#[test]
fn ungroup_keeps_objects_in_place() {
    let (mut doc, mut w) = open(&fixture("22-world"));
    let all = w.world_voxels(false);
    let trees = find(&w, "trees");
    assert!(w.ungroup(trees));
    assert!(w.ungroup(find(&w, "grove")));
    assert_eq!(w.world_voxels(false), all);
    assert!(!w.ungroup(0), "the root stays");
    assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Kept);
    let (_, back) = open(&doc.write());
    assert_eq!(back.world_voxels(false), all);
    assert!((0..back.tree.nodes.len()).all(|i| back.tree.nodes[i].name.as_deref() != Some(&b"grove"[..])));
}

#[test]
fn a_file_without_a_scene_graph_is_rebuilt_when_objects_change() {
    let mut b = Builder::new(150);
    b.model([2, 2, 2], vec![[0, 0, 0, 1], [1, 1, 1, 2]]);
    let bytes = b.build().write();
    let (mut doc, mut w) = open(&bytes);
    assert_eq!(w.objects().len(), 1);
    let c = w.copy(w.objects()[0], true).unwrap();
    w.move_by(c, [5, 0, 0]);
    let want = w.world_voxels(false);
    assert_eq!(want.len(), 4);
    assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Rebuilt);
    let (_, back) = open(&doc.write());
    assert_eq!(back.objects().len(), 2);
    assert_eq!(back.world_voxels(false), want);
}

#[test]
fn a_new_layer_goes_past_layers_objects_use_without_a_layr_chunk() {
    let mut b = Builder::new(150);
    let a = b.model([1, 1, 1], vec![[0, 0, 0, 1]]);
    let c = b.model([1, 1, 1], vec![[0, 0, 0, 2]]);
    b.root = vec![SceneNode::shape(a, None, [0, 0, 0]), SceneNode::shape(c, None, [4, 0, 0])];
    let (mut doc, mut w) = open(&b.build().write());
    assert!(w.tree.layers.is_empty());
    assert!(w.objects().iter().all(|&i| w.tree.nodes[i].layer == 0));
    let id = w.add_layer("props");
    assert_eq!(id, 1);
    assert_eq!(w.layers().iter().map(|l| l.id).collect::<Vec<_>>(), [0, 1]);
    w.set_layer_hidden(id, true);
    assert!(w.objects().iter().all(|&i| w.shown(i)), "hiding the new layer hides nothing");
    w.save_into(&mut doc).unwrap();
    let (_, back) = open(&doc.write());
    assert_eq!(back.layers().iter().find(|l| l.id == id).unwrap().name.as_deref(), Some(&b"props"[..]));
    assert!(back.objects().iter().all(|&i| back.shown(i)));
}

#[test]
fn a_model_over_256_saves_as_blocks_in_a_group() {
    let (mut doc, mut w) = open(&fixture("01-single-voxel"));
    let o = w.objects()[0];
    let m = w.model_of(o).unwrap();
    let mut big = vxr_core::BrickMap::new([300, 2, 3]);
    for x in 0..300 {
        big.set(x, 0, 0, 3);
    }
    big.set(299, 1, 2, 4);
    w.models[m] = big;
    w.set_name(o, Some("long"));
    let want = w.world_voxels(false);
    assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Rebuilt);
    let (_, back) = open(&doc.write());
    assert_eq!(back.world_voxels(false), want);
    let long = find(&back, "long");
    let NodeKind::Group(blocks) = &back.tree.nodes[long].kind else { panic!("long is not a group") };
    assert!(blocks.len() >= 2 && blocks.iter().all(|&b| back.model_of(b).is_some()));
}

#[test]
fn every_file_keeps_its_other_chunks_through_world_edits() {
    let geometry = [chunk::SIZE, chunk::XYZI, chunk::TDCZ, chunk::NTRN, chunk::NGRP, chunk::NSHP, chunk::LAYR];
    let others = |d: &Document| -> Vec<_> {
        d.raw.chunks.iter().filter(|c| !geometry.contains(&&c.id)).map(|c| (c.id, c.content.clone())).collect()
    };
    for (name, bytes) in files() {
        let (mut doc, mut w) = open(&bytes);
        let Some(&first) = w.objects().first() else { continue };
        w.move_by(first, [3, 1, -2]);
        let link = w.copy(first, true).unwrap();
        w.move_by(link, [0, 0, 7]);
        w.set_name(link, Some("copy"));
        let want = w.world_voxels(false);
        let save = w.save_into(&mut doc).unwrap();
        let (doc2, back) = open(&doc.write());
        assert_eq!(back.world_voxels(false), want, "{name} ({save:?})");
        assert_eq!(others(&doc2), others(&Document::read(&bytes).unwrap()), "{name}");
        assert_eq!(back.tree.layers, w.tree.layers, "{name}");
    }
}

/// Scene files: the fixtures that fit in a scene, plus the island.
fn scene_files() -> Vec<(String, Vec<u8>)> {
    files().into_iter().filter(|(n, _)| n != "20-far-positions").collect()
}

#[test]
fn scenes_open_as_objects_and_switching_modes_keeps_saves_byte_identical() {
    use vxr_core::history::At;
    for (name, bytes) in scene_files() {
        let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
        let w = s.world().unwrap();
        let objects = w.objects();
        let all = w.world_voxels(false);
        let shown = w.world_voxels(true);
        if objects.len() == 1 {
            assert_eq!(s.at(), At::Object(objects[0]), "{name}: one object opens in model mode");
        } else {
            assert_eq!(s.at(), At::World, "{name}: several objects open in world mode");
            let mut map = BTreeMap::new();
            s.for_each_voxel(|p, c| {
                map.insert(s.to_world(p), c);
            });
            assert_eq!(map, shown, "{name}: the world map shows every shown object");
        }
        for &i in &objects {
            assert!(s.edit_object(i) || s.at() == At::Object(i), "{name}");
            let mut n = 0;
            s.for_each_voxel(|_, _| n += 1);
            let w = s.world().unwrap();
            assert_eq!(n as u64, w.models[w.model_of(i).unwrap()].voxels, "{name}: object {i}'s model");
            assert_eq!(s.world_voxels(false), all, "{name}");
        }
        assert!(s.show_world() || objects.len() <= 1, "{name}");
        assert!(s.to_vox() == bytes, "{name}: still byte-identical");
    }
}

#[test]
fn growing_and_cropping_a_model_keeps_every_object_in_place() {
    for (name, bytes) in scene_files() {
        let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
        let all = s.world_voxels(false);
        for i in s.world().unwrap().objects() {
            s.edit_object(i);
            let b = s.bounds();
            let size = b.size.map(|k| (k + 5).min(vxr_core::scene::MAX_EXTENT));
            s.set_bounds([b.lo[0] - 2, b.lo[1] - 3, b.lo[2] - 1], size).unwrap();
            assert_eq!(s.world_voxels(false), all, "{name}: object {i} grown");
            s.show_world();
            assert_eq!(s.world_voxels(false), all, "{name}: object {i} grown, in world mode");
        }
        let grown = s.to_vox();
        let again = vxr_core::Scene::from_vox(&grown).unwrap();
        assert_eq!(again.world_voxels(false), all, "{name}: grown models saved");
        for i in s.world().unwrap().objects() {
            s.edit_object(i);
            let before = s.bounds().size;
            s.crop();
            let w = s.world().unwrap();
            if w.models[w.model_of(i).unwrap()].voxels > 0 {
                assert!(s.bounds().size.iter().zip(before).all(|(a, b)| *a <= b), "{name}: object {i} cropped");
            }
        }
        assert_eq!(s.world_voxels(false), all, "{name}: cropped");
        let again = vxr_core::Scene::from_vox(&s.to_vox()).unwrap();
        assert_eq!(again.world_voxels(false), all, "{name}: cropped models saved");
        // Every step undoes, across modes, back to the opened file.
        while s.undo() {}
        assert!(s.to_vox() == bytes, "{name}: undone to the file");
    }
}

#[test]
fn world_edits_and_model_edits_undo_in_order_across_modes() {
    use vxr_core::history::At;
    let bytes = fixture("22-world");
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    assert_eq!(s.at(), At::World);
    let opened = s.world_voxels(false);
    let w = s.world().unwrap();
    let (slab, tree_a, tree_b) = (find(w, "slab"), find(w, "tree a"), find(w, "tree b"));
    let tree_model = w.model_of(tree_a).unwrap();
    assert_eq!(w.model_of(tree_b), Some(tree_model), "linked copies");

    // Voxels cannot be edited in world mode.
    let mut cell = None;
    s.for_each_voxel(|p, _| cell = Some(p));
    assert_eq!(s.edit([(cell.unwrap(), 0)]), 0);

    // A model edit on tree a shows on every linked copy.
    assert!(s.edit_object(tree_a));
    let mut cells = Vec::new();
    s.for_each_voxel(|p, _| cells.push(p));
    let n = s.map.voxels;
    assert_eq!(s.edit([(cells[0], 0)]), 1);
    assert!(s.show_world());
    let edited = s.world_voxels(false);
    assert_eq!(opened.len() - edited.len(), 3, "one voxel gone from each of three copies");

    // A move, a duplicate and a hide, each one step.
    s.edit_world(|w| w.move_by(slab, [0, 0, 4])).unwrap();
    let copy = s.edit_world(|w| w.copy(tree_b, false)).unwrap().unwrap();
    s.edit_world(|w| w.move_by(copy, [0, 6, 0])).unwrap();
    s.edit_world(|w| w.set_hidden(slab, true)).unwrap();
    let w = s.world().unwrap();
    assert_ne!(w.model_of(copy), Some(tree_model), "a duplicate has its own model");
    let models = w.models.len();
    let done = (s.world_voxels(false), s.world_voxels(true));
    assert!(!done.1.keys().any(|p| opened.contains_key(p) && !done.0.contains_key(p)));
    let saved = s.to_vox();
    let again = vxr_core::Scene::from_vox(&saved).unwrap();
    assert_eq!((again.world_voxels(false), again.world_voxels(true)), done, "saved and reopened");
    assert_eq!(again.world().unwrap().models.len(), models);

    // Undo the four world edits, then the model edit (which goes back into tree a).
    for _ in 0..4 {
        assert!(s.undo());
        assert_eq!(s.at(), At::World);
    }
    assert_eq!(s.world_voxels(false), edited);
    assert_eq!(s.world().unwrap().models.len(), models - 1, "the duplicate's model is gone");
    assert!(s.undo());
    assert_eq!(s.at(), At::Object(tree_a), "undo goes back to the model it edits");
    assert_eq!(s.map.voxels, n);
    assert!(s.to_vox() == bytes, "undone: byte-identical");
    assert!(!s.undo());

    // Redo all of it, across modes again.
    while s.redo() {}
    assert_eq!(s.at(), At::World);
    assert_eq!((s.world_voxels(false), s.world_voxels(true)), done);
    assert!(s.to_vox() == saved, "redone: the same file");
}

#[test]
fn objects_are_picked_dragged_duplicated_linked_and_deleted_in_world_mode() {
    let bytes = fixture("22-world");
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    let w = s.world().unwrap();
    let (slab, tree_a) = (find(w, "slab"), find(w, "tree a"));
    let (objects, models) = (w.objects().len(), w.models.len());
    let slab_voxels = {
        let mut n = 0;
        w.for_each_object_voxel(slab, |_, _| n += 1);
        n
    };

    // A slab voxel picks the slab, and picking shows its voxels selected.
    let mut cell = None;
    s.for_each_voxel(|p, _| {
        if cell.is_none() && s.object_at(p) == Some(slab) {
            cell = Some(p);
        }
    });
    let cell = cell.unwrap();
    assert!(s.pick(Some(slab)));
    assert_eq!(s.selected_count(), slab_voxels);
    assert!(!s.pick(Some(0)), "the root is not an object");

    // A drag through three positions is one undo step, and the pick survives the reload.
    let before = s.world_voxels(false);
    for d in [[1, 0, 0], [1, 0, 1], [0, 0, 1]] {
        s.put_world(|w| w.move_by(slab, d)).unwrap();
    }
    assert_eq!(s.picked(), Some(slab));
    assert_eq!(s.selected_count(), slab_voxels);
    s.commit();
    let moved = s.world_voxels(false);
    assert_eq!(moved.len(), before.len());
    assert!(s.undo());
    assert_eq!(s.world_voxels(false), before, "one step for the whole drag");
    assert!(s.redo());
    assert_eq!(s.world_voxels(false), moved);

    // Duplicate sits beside the original with its own model; a linked copy shares it.
    let dup = s.edit_world(|w| w.duplicate(slab, false)).unwrap().unwrap();
    let link = s.edit_world(|w| w.duplicate(tree_a, true)).unwrap().unwrap();
    let w = s.world().unwrap();
    assert_eq!(w.objects().len(), objects + 2);
    assert_eq!(w.models.len(), models + 1);
    assert_eq!(w.model_of(link), w.model_of(tree_a));
    let (lo, size) = w.placement(slab).unwrap();
    let (dlo, dsize) = w.placement(dup).unwrap();
    assert_eq!((dlo, dsize), ([lo[0] + size[0] as i32 + 1, lo[1], lo[2]], size));
    assert!(s.pick(Some(dup)));
    assert_eq!(s.selected_count(), slab_voxels);

    // Deleting the picked duplicate drops the pick; undo brings it back.
    assert!(s.edit_world(|w| w.remove(dup)).unwrap());
    assert_eq!(s.picked(), None);
    assert_eq!(s.selected_count(), 0);
    assert!(s.world().unwrap().object_at(s.to_world(cell)).is_none_or(|i| i != dup));

    // Saved and reopened: the moved slab, the linked copy, no duplicate.
    let saved = s.to_vox();
    let again = vxr_core::Scene::from_vox(&saved).unwrap();
    assert_eq!(again.world_voxels(false), s.world_voxels(false));
    let w2 = again.world().unwrap();
    assert_eq!(w2.objects().len(), objects + 1);
    let links = w2.objects().iter().filter(|&&i| w2.model_of(i) == w2.model_of(find(w2, "tree a"))).count();
    assert_eq!(links, 4, "three linked trees and the new linked copy");

    // Everything undone is the original file again.
    while s.undo() {}
    assert!(s.to_vox() == bytes);
}

#[test]
fn every_shown_voxel_belongs_to_an_object() {
    for (name, bytes) in scene_files() {
        let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
        if !s.show_world() && s.at() != vxr_core::history::At::World {
            continue;
        }
        let mut missing = 0;
        s.for_each_voxel(|p, _| missing += s.object_at(p).is_none() as usize);
        assert_eq!(missing, 0, "{name}");
    }
}

#[test]
fn groups_are_picked_moved_duplicated_grouped_and_ungrouped_in_world_mode() {
    let bytes = fixture("22-world");
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    let w = s.world().unwrap();
    let (trees, slab) = (find(w, "trees"), find(w, "slab"));
    let members = w.objects_under(trees);
    assert!(members.len() >= 2, "the trees group holds several objects");
    let count = |w: &World, nodes: &[usize]| {
        let mut n = 0;
        for &i in nodes {
            if w.shown(i) {
                w.for_each_object_voxel(i, |_, _| n += 1);
            }
        }
        n
    };
    let group_voxels = count(w, &members);

    // Picking a group shows every voxel under it selected.
    assert!(s.pick(Some(trees)));
    assert_eq!(s.selected_count(), group_voxels);

    // Moving the group moves every object under it by the same offset, as one undo step.
    let before: Vec<_> = members.iter().map(|&i| s.world().unwrap().placement(i).unwrap().0).collect();
    s.edit_world(|w| w.move_by(trees, [2, 0, -1])).unwrap();
    let w = s.world().unwrap();
    for (&i, lo) in members.iter().zip(&before) {
        assert_eq!(w.placement(i).unwrap().0, [lo[0] + 2, lo[1], lo[2] - 1]);
    }
    assert_eq!(s.picked(), Some(trees));
    assert!(s.undo());
    let w = s.world().unwrap();
    assert!(members.iter().zip(&before).all(|(&i, lo)| w.placement(i).unwrap().0 == *lo));

    // A duplicate of the group copies every object under it.
    let objects = s.world().unwrap().objects().len();
    let dup = s.edit_world(|w| w.duplicate(trees, true)).unwrap().unwrap();
    assert_eq!(s.world().unwrap().objects().len(), objects + members.len());
    assert!(s.pick(Some(dup)));
    assert_eq!(s.selected_count(), group_voxels);
    assert!(s.undo());
    assert_eq!(s.picked(), None, "the undone copy drops the pick");

    // Grouping an object wraps it in a new group in its place; ungrouping puts it back.
    let world_before = s.world_voxels(false);
    let order_before = s.world().unwrap().tree.order().len();
    let g = s.edit_world(|w| w.group(&[slab])).unwrap().unwrap();
    let w = s.world().unwrap();
    assert_eq!(w.tree.children(g), &[slab]);
    assert_eq!(w.tree.order().len(), order_before + 1);
    assert_eq!(s.world_voxels(false), world_before, "grouping moves nothing");
    assert!(s.pick(Some(g)));
    assert!(s.edit_world(|w| w.ungroup(g)).unwrap());
    assert_eq!(s.picked(), None, "the ungrouped group leaves the tree");
    assert_eq!(s.world().unwrap().tree.order().len(), order_before);

    // Ungrouping a moved group keeps its objects where they show, through save and reopen.
    s.edit_world(|w| w.move_by(trees, [0, 3, 0])).unwrap();
    let moved = s.world_voxels(false);
    assert!(s.edit_world(|w| w.ungroup(trees)).unwrap());
    assert_eq!(s.world_voxels(false), moved);
    let again = vxr_core::Scene::from_vox(&s.to_vox()).unwrap();
    assert_eq!(again.world_voxels(false), moved);

    while s.undo() {}
    assert!(s.to_vox() == bytes);
}

#[test]
fn layers_are_shown_hidden_named_added_and_assigned_in_world_mode() {
    let bytes = fixture("22-world");
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    let w = s.world().unwrap();
    let (slab, gem, trees) = (find(w, "slab"), find(w, "gem"), find(w, "trees"));
    let layers = |s: &vxr_core::Scene| {
        s.world()
            .unwrap()
            .layers()
            .into_iter()
            .map(|l| (l.id, l.name.map(|n| String::from_utf8(n).unwrap()).unwrap_or_default(), l.hidden))
            .collect::<Vec<_>>()
    };
    assert_eq!(layers(&s), [(0, "ground".into(), false), (1, "trees".into(), false), (2, "secret".into(), true)]);
    assert!(!s.world().unwrap().shown(gem), "the gem is on the hidden secret layer");

    // Showing the secret layer shows the gem, as one undo step.
    let voxels = s.world_voxels(true).len();
    s.edit_world(|w| w.set_layer_hidden(2, false)).unwrap();
    assert!(s.world().unwrap().shown(gem));
    assert_eq!(s.world_voxels(true).len(), voxels + 1);
    assert!(s.undo());
    assert!(!s.world().unwrap().shown(gem));
    assert!(s.redo());

    // Renaming a layer and moving the slab onto the hidden secret layer hides it.
    s.edit_world(|w| w.set_layer_name(0, Some("floor"))).unwrap();
    s.edit_world(|w| w.set_layer_hidden(2, true)).unwrap();
    s.edit_world(|w| w.put_on_layer(slab, 2)).unwrap();
    assert!(!s.world().unwrap().shown(slab));
    // A group goes onto a layer with every node under it.
    s.edit_world(|w| w.put_on_layer(trees, 0)).unwrap();
    let w = s.world().unwrap();
    assert!(w.objects_under(trees).iter().all(|&i| w.tree.nodes[i].layer == 0));
    // A new layer, and a layer an object is on without a LAYR entry.
    let id = s.edit_world(|w| w.add_layer("props")).unwrap();
    assert_eq!(id, 3);
    s.edit_world(|w| w.put_on_layer(gem, 7)).unwrap();
    assert_eq!(layers(&s).last().unwrap(), &(7, String::new(), false));
    s.edit_world(|w| w.set_layer_name(7, Some("seven"))).unwrap();

    // Everything survives save and reopen.
    let want = layers(&s);
    let again = vxr_core::Scene::from_vox(&s.to_vox()).unwrap();
    assert_eq!(layers(&again), want);
    assert_eq!(outline(again.world().unwrap()), outline(s.world().unwrap()));
    assert_eq!(again.world_voxels(true), s.world_voxels(true));

    while s.undo() {}
    assert!(s.to_vox() == bytes);
}

#[test]
fn a_world_change_inside_an_open_edit_reverts_and_commits_with_its_colours() {
    let bytes = fixture("22-world");
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    let w = s.world().unwrap();
    let (slab, tree_a) = (find(w, "slab"), find(w, "tree a"));
    let (before, models) = (s.world_voxels(false), w.models.len());

    // A move, a duplicate and a colour in one open edit, then put back: nothing to commit.
    s.begin();
    s.put_world(|w| w.move_by(slab, [0, 0, 4])).unwrap();
    s.put_world(|w| w.duplicate(tree_a, false)).unwrap().unwrap();
    s.put_color(9, [1, 2, 3, 255]);
    assert_ne!(s.world_voxels(false), before);
    s.revert_voxels();
    assert_eq!(s.world_voxels(false), before);
    assert_eq!(s.world().unwrap().models.len(), models, "the duplicate's model is gone");
    assert_eq!(s.palette[9], [1, 2, 3, 255], "colours stay in the open edit");
    s.put_color(9, vxr_core::Scene::from_vox(&bytes).unwrap().palette[9]);
    s.commit();
    assert_eq!(s.history().position(), 0);

    // Again, committed: one named step for the world and the colour, undone to the bytes.
    s.begin();
    s.put_world(|w| w.move_by(slab, [0, 0, 4])).unwrap();
    s.put_color(9, [1, 2, 3, 255]);
    s.commit_named("AI: move the slab");
    assert_eq!(s.history().position(), 1);
    assert_eq!(s.history().undo_label(), Some("AI: move the slab"));
    let moved = s.world_voxels(false);
    assert_ne!(moved, before);
    assert!(s.undo());
    assert!(s.to_vox() == bytes, "one undo puts back the world and the colour");
    assert!(s.redo());
    assert_eq!((s.world_voxels(false), s.palette[9]), (moved, [1, 2, 3, 255]));
}

/// A 40 x 40 x 2 slab with small blocks around and over it: one before the slab in the tree
/// (it covers the slab), one after (the slab covers it), a turned one, and a group of two
/// overlapping blocks and a hidden one.
fn crowded_world() -> Vec<u8> {
    let mut b = Builder::new(200);
    let fill = |size: [i32; 3], c: u8| -> Vec<[u8; 4]> {
        (0..size[0] * size[1] * size[2])
            .map(|i| [(i % size[0]) as u8, (i / size[0] % size[1]) as u8, (i / (size[0] * size[1])) as u8, c])
            .collect()
    };
    let slab = b.model([40, 40, 2], fill([40, 40, 2], 1));
    let cube = b.model([6, 6, 6], fill([6, 6, 6], 2));
    let bar = b.model([5, 3, 2], fill([5, 3, 2], 3));
    let tile = b.model([4, 4, 4], fill([4, 4, 4], 4));
    let turn = [[0, -1, 0], [1, 0, 0], [0, 0, 1]];
    b.root.push(SceneNode::shape(tile, None, [4, 4, 0]).named("top"));
    b.root.push(SceneNode::shape(slab, None, [0, 0, 0]).named("slab"));
    b.root.push(SceneNode::shape(cube, None, [-8, 0, 2]).named("under"));
    b.root.push(SceneNode::shape(bar, Some(turn), [12, -6, 1]).named("turned"));
    b.root.push(
        SceneNode::group(
            None,
            [-10, 10, 3],
            vec![
                SceneNode::shape(tile, None, [0, 0, 0]).named("one"),
                SceneNode::shape(bar, None, [2, 1, 0]).named("two"),
                SceneNode::shape(cube, None, [0, 0, 0]).named("ghost").hidden(),
            ],
        )
        .named("pair"),
    );
    b.build().write()
}

/// The map's voxels by world cell.
fn map_cells(s: &vxr_core::Scene) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    s.for_each_voxel(|p, c| {
        out.insert(s.to_world(p), c);
    });
    out
}

#[test]
fn a_drag_tick_leaves_the_map_as_a_full_compose_of_the_world() {
    let bytes = crowded_world();
    let mut s = vxr_core::Scene::from_vox(&bytes).unwrap();
    assert!(s.world().is_some() && s.picked().is_none());
    let w = s.world().unwrap();
    let (top, under, turned, pair) = (find(w, "top"), find(w, "under"), find(w, "turned"), find(w, "pair"));
    let start = map_cells(&s);
    assert_eq!(start, s.world().unwrap().world_voxels(true));

    // Each tick moves an object or group. The paths cross the slab both ways, so cells it
    // covered come back and cells it covers are hidden, the turned bar and the pair (which
    // carries a hidden block that must stay out) move, and some ticks push past the box the
    // objects fill. While that box holds, the map is patched in place and `reshapes` stays.
    let ticks = [
        (under, [3, 0, 0]),
        (under, [3, 0, 0]),
        (under, [3, 0, -2]),
        (under, [-6, 0, 2]),
        (top, [-2, 0, 0]),
        (top, [0, -1, 0]),
        (turned, [-3, 0, 2]),
        (turned, [0, 1, 0]),
        (pair, [4, 0, 0]),
        (pair, [0, 0, -5]),
        (pair, [0, -2, 0]),
        (pair, [0, 30, 0]),
        (pair, [0, -30, 0]),
        (under, [-30, 0, 0]),
        (under, [30, 0, 0]),
    ];
    let hull = |m: &BTreeMap<[i32; 3], u8>| -> ([i32; 3], [i32; 3]) {
        (
            std::array::from_fn(|a| m.keys().map(|p| p[a]).min().unwrap()),
            std::array::from_fn(|a| m.keys().map(|p| p[a]).max().unwrap()),
        )
    };
    let mut patched = 0;
    for (n, (node, step)) in ticks.into_iter().enumerate() {
        let (reshapes, before) = (s.reshapes(), hull(&map_cells(&s)));
        assert!(s.pick(Some(node)));
        s.move_in_world(node, step).unwrap();
        let shown = s.world().unwrap().world_voxels(true);
        let map = map_cells(&s);
        let off: Vec<_> = map.iter().filter(|(p, c)| shown.get(*p) != Some(c)).map(|(p, _)| p).collect();
        let gone: Vec<_> = shown.iter().filter(|(p, c)| map.get(*p) != Some(c)).map(|(p, _)| p).collect();
        assert!(
            off.is_empty() && gone.is_empty(),
            "tick {n} (node {node}): {} stale or wrong like {:?}, {} missing like {:?}",
            off.len(),
            off.first(),
            gone.len(),
            gone.first()
        );
        if hull(&shown) == before {
            assert_eq!(s.reshapes(), reshapes, "tick {n}: the box held, the map should be patched in place");
            patched += 1;
        }
        // The pick follows the moved voxels: picking again on the settled map selects the same count.
        let kept: std::collections::BTreeSet<_> = s.selection().collect();
        assert!(s.pick(Some(node)));
        assert_eq!(kept, s.selection().collect(), "tick {n}: the selection did not move with the object");
    }
    assert!((8..ticks.len()).contains(&patched), "{patched} of {} ticks patched in place", ticks.len());

    // The whole drag is one undo step.
    s.commit();
    assert_eq!(s.history().position(), 1);
    assert!(s.undo());
    assert_eq!(map_cells(&s), start);
    assert!(s.to_vox() == bytes);
}

#[test]
fn editing_one_models_voxels_re_encodes_only_that_models_chunk() {
    let bytes = fixture("22-world");
    let (mut doc, mut w) = open(&bytes);
    let slab = w.model_of(find(&w, "slab")).unwrap();
    let [sx, sy, sz] = w.models[slab].size_voxels().map(|s| s as i32);
    let cell = (0..sx)
        .flat_map(|x| (0..sy).flat_map(move |y| (0..sz).map(move |z| (x, y, z))))
        .find(|&(x, y, z)| w.models[slab].get(x, y, z) != 0)
        .expect("the slab has voxels");
    assert!(w.models[slab].set(cell.0, cell.1, cell.2, 0));
    assert_eq!(w.save_into(&mut doc).unwrap(), WorldSave::Kept);
    let (a, b) = (Document::read(&bytes).unwrap().raw.chunks, doc.raw.chunks);
    assert_eq!(a.len(), b.len());
    let changed: Vec<_> = (0..a.len()).filter(|&i| a[i] != b[i]).map(|i| a[i].id).collect();
    assert_eq!(changed, [*chunk::XYZI], "only the slab's voxel chunk differs");
}
