//! The world: objects, groups, linked copies and layers read from `.vox`, edited and saved
//! back losslessly.

use std::collections::BTreeMap;

use vxr_core::World;
use vxr_core::scene::vox_to_world;
use vxr_vox::build::Builder;
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
