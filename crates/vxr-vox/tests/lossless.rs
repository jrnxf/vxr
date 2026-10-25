//! `Document::set_world`: saving edits keeps every chunk the edit did not need to touch.

use std::collections::BTreeMap;
use std::path::Path;

use vxr_vox::chunk::{self, Typed};
use vxr_vox::scene::Rule;
use vxr_vox::{Chunk, Document, WorldSave};

fn world(doc: &Document) -> BTreeMap<[i32; 3], u8> {
    doc.scene().unwrap().world_voxels(Rule::Compose)
}

/// Every generated fixture plus the island when it has been built.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> =
        vxr_vox::fixtures::all().into_iter().map(|f| (f.name.to_string(), f.builder.build().write())).collect();
    let island = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    match std::fs::read(&island) {
        Ok(b) => out.push(("island".into(), b)),
        Err(_) => eprintln!("island skipped: run `node prototype/src/build.mjs`"),
    }
    out
}

/// A small deterministic generator so the edits differ per file without a dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// Erase and repaint a few voxels the file already shows. Returns the new world.
fn edit_existing(w: &BTreeMap<[i32; 3], u8>, seed: u64) -> BTreeMap<[i32; 3], u8> {
    let mut rng = Lcg(seed);
    let cells: Vec<[i32; 3]> = w.keys().copied().collect();
    let mut out = w.clone();
    for k in 0..6 {
        let p = cells[rng.next(cells.len())];
        if k % 2 == 0 {
            out.remove(&p);
        } else {
            let c = out.get(&p).copied().unwrap_or(1);
            out.insert(p, if c == 200 { 201 } else { 200 });
        }
    }
    out
}

/// The chunk indices whose bytes differ, for two files with the same chunk count.
fn changed(a: &[Chunk], b: &[Chunk]) -> Vec<usize> {
    assert_eq!(a.len(), b.len());
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

#[test]
fn saving_without_edits_is_byte_identical() {
    for (name, bytes) in files() {
        let mut doc = Document::read(&bytes).unwrap();
        let w = world(&doc);
        assert_eq!(doc.set_world(&w).unwrap(), WorldSave::Kept, "{name}");
        assert_eq!(doc.write(), bytes, "{name}");
    }
}

#[test]
fn editing_existing_voxels_touches_only_those_models() {
    for (seed, (name, bytes)) in files().into_iter().enumerate() {
        let original = Document::read(&bytes).unwrap();
        let w = world(&original);
        if w.is_empty() {
            continue;
        }
        let want = edit_existing(&w, seed as u64 + 1);
        let mut doc = original.clone();
        let how = doc.set_world(&want).unwrap();
        let again = Document::read(&doc.write()).unwrap();
        assert_eq!(world(&again), want, "{name}: reopened world");
        assert_eq!(how, WorldSave::Kept, "{name}");
        // Edited models are re-encoded in place; an edited linked copy (a model behind
        // several instances) gets a new SIZE and XYZI pair and its nSHP points at it.
        let data: Vec<usize> = original.scene().unwrap().models.iter().map(|m| m.data_chunk).collect();
        let (a, b) = (&original.raw.chunks, &again.raw.chunks);
        let added = b.len() - a.len();
        assert_eq!(added % 2, 0, "{name}");
        let last_model = data.iter().max().copied().unwrap_or(0);
        for (k, c) in b[last_model + 1..last_model + 1 + added].iter().enumerate() {
            assert!(c.is(if k % 2 == 0 { chunk::SIZE } else { chunk::XYZI }), "{name}: inserted chunk {k}");
        }
        let b: Vec<Chunk> = b[..=last_model].iter().chain(&b[last_model + 1 + added..]).cloned().collect();
        for i in changed(a, &b) {
            let ok = data.contains(&i) || (added > 0 && a[i].is(chunk::NSHP));
            assert!(ok, "{name}: chunk {i} ({}) changed", vxr_vox::id_str(&a[i].id));
        }
    }
}

#[test]
fn a_shared_shape_node_falls_back_to_rebuilding_only_the_geometry() {
    // Fixture 13 has two nTRN, each with its own nSHP of one model. Point the second nTRN
    // at the first nSHP, as linked copies of a node do.
    let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "13-hidden-node").unwrap();
    let mut original = Document { raw: f.builder.build() };
    let shapes: Vec<i32> = original
        .raw
        .chunks
        .iter()
        .filter_map(|c| match Typed::parse(c).unwrap() {
            Typed::Nshp(n) => Some(n.id),
            _ => None,
        })
        .collect();
    for c in original.raw.chunks.iter_mut() {
        if let Typed::Ntrn(mut n) = Typed::parse(c).unwrap()
            && n.child == shapes[1]
        {
            n.child = shapes[0];
            c.content = n.encode();
        }
    }
    let mut want = world(&original);
    let first = *want.keys().next().unwrap();
    want.remove(&first);
    let mut doc = original.clone();
    assert_eq!(doc.set_world(&want).unwrap(), WorldSave::Rebuilt);
    let again = Document::read(&doc.write()).unwrap();
    assert_eq!(world(&again), want);
    let keep = |d: &Document| d.raw.chunks.iter().filter(|c| !is_geometry(c)).cloned().collect::<Vec<_>>();
    assert_eq!(keep(&again), keep(&original));
}
#[test]
fn voxels_outside_every_model_become_a_new_model_and_nothing_else_moves() {
    for (name, bytes) in files() {
        let original = Document::read(&bytes).unwrap();
        let mut want = world(&original);
        let far = [3000, -3000, 77];
        want.insert(far, 9);
        want.insert([far[0] + 1, far[1], far[2] + 2], 10);
        let mut doc = original.clone();
        let how = doc.set_world(&want).unwrap();
        assert_eq!(how, WorldSave::Kept, "{name}");
        let again = Document::read(&doc.write()).unwrap();
        assert_eq!(world(&again), want, "{name}: reopened world");
        // The original chunks come back in order: the root group (gaining a child) may
        // differ, and the only extra chunks are the new model and its nodes.
        let (a, b) = (&original.raw.chunks, &again.raw.chunks);
        let mut j = 0;
        let mut inserted = Vec::new();
        for c in b {
            if j < a.len() && (a[j] == *c || (a[j].is(chunk::NGRP) && c.is(chunk::NGRP))) {
                j += 1;
            } else {
                inserted.push(c.id);
            }
        }
        assert_eq!(j, a.len(), "{name}: every original chunk is kept in order");
        let ids: Vec<&[u8; 4]> = inserted.iter().collect();
        let had_graph = !original.scene().unwrap().nodes.is_empty();
        let expect: Vec<&[u8; 4]> = if had_graph {
            vec![chunk::SIZE, chunk::XYZI, chunk::NTRN, chunk::NSHP]
        } else {
            let mut v = vec![chunk::SIZE, chunk::XYZI, chunk::NTRN, chunk::NGRP];
            if !original.scene().unwrap().models.is_empty() {
                v.extend([chunk::NTRN, chunk::NSHP]);
            }
            v.extend([chunk::NTRN, chunk::NSHP]);
            v
        };
        assert_eq!(ids, expect, "{name}: inserted chunks");
        for (k, c) in a.iter().enumerate() {
            if !c.is(chunk::NGRP) {
                continue;
            }
            let Typed::Ngrp(g) = Typed::parse(c).unwrap() else { unreachable!() };
            let Some(now) = b.iter().find_map(|c| match Typed::parse(c).unwrap() {
                Typed::Ngrp(n) if n.id == g.id => Some(n),
                _ => None,
            }) else {
                panic!("{name}: group {k} lost")
            };
            assert_eq!(now.children[..g.children.len()], g.children[..], "{name}: group {} children", g.id);
        }
    }
}

#[test]
fn palette_and_materials_survive_a_voxel_edit() {
    for (name, bytes) in files() {
        let original = Document::read(&bytes).unwrap();
        let w = world(&original);
        if w.is_empty() {
            continue;
        }
        let mut doc = original.clone();
        doc.set_world(&edit_existing(&w, 7)).unwrap();
        let keep = |d: &Document| -> Vec<Chunk> {
            d.raw.chunks.iter().filter(|c| !is_geometry(c)).cloned().collect::<Vec<_>>()
        };
        assert_eq!(keep(&doc), keep(&original), "{name}: non-geometry chunks");
        assert_eq!((doc.raw.version, &doc.raw.trailing), (original.raw.version, &original.raw.trailing), "{name}");
    }
}

fn is_geometry(c: &Chunk) -> bool {
    [chunk::SIZE, chunk::XYZI, chunk::TDCZ, chunk::NTRN, chunk::NGRP, chunk::NSHP].iter().any(|id| c.is(id))
}

#[test]
fn a_voxel_added_inside_a_hidden_model_goes_into_a_shown_one() {
    // 13-hidden-node: a shown cube and a hidden copy at x = 6, both from one 3^3 model. Erase
    // a cell of each, then put them back: the hidden one's cell must not land in the hidden
    // model, or MagicaVoxel would not show it.
    for name in ["13-hidden-node", "12-layers"] {
        let f = vxr_vox::fixtures::all().into_iter().find(|f| f.name == name).unwrap();
        let mut doc = Document::read(&f.builder.build().write()).unwrap();
        let scene = doc.scene().unwrap();
        let (inst, _) = scene.instances(Rule::Compose);
        let hidden_layers: Vec<i32> =
            scene.layers.iter().filter(|l| l.attrs.is_true("_hidden")).map(|l| l.id).collect();
        let hidden = |i: &vxr_vox::scene::Instance| i.hidden || hidden_layers.contains(&i.layer);
        let shown_world = |doc: &Document| {
            let s = doc.scene().unwrap();
            let (inst, _) = s.instances(Rule::Compose);
            let mut out = BTreeMap::new();
            for i in inst.iter().filter(|i| !hidden(i)) {
                let m = &s.models[i.model];
                for v in m.effective_voxels() {
                    let p =
                        vxr_vox::scene::Scene::world_cell(&i.world, m.size, [v[0] as i32, v[1] as i32, v[2] as i32]);
                    out.insert(p, v[3]);
                }
            }
            out
        };
        assert!(inst.iter().any(hidden), "{name} has a hidden model");
        let mut w = world(&doc);
        let shown_before = shown_world(&doc);
        let cell = *w.keys().find(|p| !shown_before.contains_key(*p)).unwrap();
        let c = w.remove(&cell).unwrap();
        doc.set_world(&w).unwrap();
        w.insert(cell, c);
        doc.set_world(&w).unwrap();
        let again = Document::read(&doc.write()).unwrap();
        assert_eq!(world(&again), w, "{name}");
        assert_eq!(shown_world(&again).get(&cell), Some(&c), "{name}: the re-added voxel shows");
    }
}
