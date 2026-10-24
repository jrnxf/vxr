//! Crafted inputs from the known reader bugs in other codecs: each must give `Err` or a
//! bounded result, never a panic or a huge allocation.

use vox::build::{Builder, SceneNode};
use vox::bytes::Writer;
use vox::chunk::{self, Ngrp, Nshp, Ntrn, Tdcz};
use vox::raw::{Chunk, RawFile};
use vox::scene::{MAX_VISITS, Rule};
use vox::{Dict, Document};

fn file(chunks: Vec<Chunk>) -> Vec<u8> {
    RawFile { version: 200, main_content: vec![], chunks, trailing: vec![] }.write()
}

fn i32s(v: &[i32]) -> Vec<u8> {
    let mut w = Writer::default();
    for x in v {
        w.i32(*x);
    }
    w.0
}

#[test]
fn truncated_and_lying_sizes() {
    let good = Builder::new(200).build().write();
    for n in 0..good.len() {
        let _ = Document::read(&good[..n]);
    }
    // Chunk content size larger than the file.
    let mut b = file(vec![Chunk::new(chunk::SIZE, i32s(&[1, 1, 1]))]);
    b[24..28].copy_from_slice(&i32::MAX.to_le_bytes());
    assert!(Document::read(&b).is_err());
    // Negative content size.
    b[24..28].copy_from_slice(&(-12i32).to_le_bytes());
    assert!(Document::read(&b).is_err());
}

#[test]
fn negative_and_huge_counts() {
    assert!(Document::read(&file(vec![Chunk::new(chunk::SIZE, i32s(&[-1, 5, 5]))])).is_err());
    // XYZI claiming 2^31-1 voxels in 8 bytes.
    let x = file(vec![Chunk::new(chunk::SIZE, i32s(&[4, 4, 4])), Chunk::new(chunk::XYZI, i32s(&[i32::MAX, 0]))]);
    assert!(Document::read(&x).is_err());
    // A DICT with a billion pairs, a NOTE with a billion strings, a group with a billion children.
    assert!(Document::read(&file(vec![Chunk::new(chunk::META, i32s(&[1 << 30]))])).is_err());
    assert!(Document::read(&file(vec![Chunk::new(chunk::NOTE, i32s(&[1 << 30]))])).is_err());
    assert!(Document::read(&file(vec![Chunk::new(chunk::NGRP, i32s(&[1, 0, 1 << 30]))])).is_err());
    // A string longer than the chunk.
    assert!(Document::read(&file(vec![Chunk::new(chunk::META, i32s(&[1, 1 << 30]))])).is_err());
}

#[test]
fn tdcz_bombs_are_capped() {
    // Declares 1024^3 voxels: rejected before decompressing anything.
    let t = Tdcz { x: 1024, y: 1024, z: 1024, compressed: vec![0x78, 0x9c] };
    let f = file(vec![Chunk::new(chunk::SIZE, i32s(&[1024, 1024, 1024])), Chunk::new(chunk::TDCZ, t.encode())]);
    assert!(Document::read(&f).is_err());
    // A short stream for a 256^3 model: Err, not a partial model.
    let t = Tdcz { x: 256, y: 256, z: 256, compressed: vec![0x78, 0x9c, 0x63, 0x00] };
    let f = file(vec![Chunk::new(chunk::SIZE, i32s(&[256, 256, 256])), Chunk::new(chunk::TDCZ, t.encode())]);
    assert!(Document::read(&f).is_err());
    // Twenty full 256^3 models: over the 64 Mi voxel budget, so the file is rejected.
    let dense = vec![1u8; 256 * 256 * 256];
    let t = Tdcz::encode_dense(256, 256, 256, &dense);
    let mut cs = Vec::new();
    for _ in 0..20 {
        cs.push(Chunk::new(chunk::SIZE, i32s(&[256, 256, 256])));
        cs.push(Chunk::new(chunk::TDCZ, t.encode()));
    }
    assert!(Document::read(&file(cs)).is_err());
}

#[test]
fn cyclic_and_exploding_graphs_are_bounded() {
    let model = || vec![Chunk::new(chunk::SIZE, i32s(&[1, 1, 1])), Chunk::new(chunk::XYZI, i32s(&[1, 0x0100_0000]))];
    let trn = |id, child| Ntrn {
        id,
        attrs: Dict::default(),
        child,
        reserved: -1,
        layer: 0,
        frames: vec![Dict::default()],
        extra: vec![],
    };
    // 0 -> 1 -> 0 -> ...: a cycle.
    let mut cs = model();
    cs.push(Chunk::new(chunk::NTRN, trn(0, 1).encode()));
    cs.push(Chunk::new(chunk::NGRP, Ngrp { id: 1, attrs: Dict::default(), children: vec![0], extra: vec![] }.encode()));
    let doc = Document::read(&file(cs)).unwrap();
    let (inst, warn) = doc.scene().unwrap().instances(Rule::Compose);
    assert!(inst.is_empty() && !warn.is_empty());
    // 20 levels of groups with 4 children each, all the same node: 4^20 paths.
    let mut cs = model();
    cs.push(Chunk::new(chunk::NTRN, trn(0, 1).encode()));
    for level in 0..20 {
        let g = 1 + 2 * level;
        cs.push(Chunk::new(
            chunk::NGRP,
            Ngrp { id: g, attrs: Dict::default(), children: vec![g + 1; 4], extra: vec![] }.encode(),
        ));
        cs.push(Chunk::new(chunk::NTRN, trn(g + 1, g + 2).encode()));
    }
    cs.push(Chunk::new(
        chunk::NSHP,
        Nshp { id: 41, attrs: Dict::default(), models: vec![(0, Dict::default())], extra: vec![] }.encode(),
    ));
    let doc = Document::read(&file(cs)).unwrap();
    let t = std::time::Instant::now();
    let (inst, warn) = doc.scene().unwrap().instances(Rule::Compose);
    assert!(inst.len() <= MAX_VISITS && warn.iter().any(|w| w.contains("budget")));
    assert!(t.elapsed().as_secs() < 10);
}

#[test]
fn bad_rotation_and_translation_strings_warn() {
    let mut b = Builder::new(200);
    let m = b.model([1, 1, 1], vec![[0, 0, 0, 1]]);
    b.root.push(SceneNode::Shape {
        attrs: Dict::default(),
        layer: 0,
        frames: vec![Dict::default().with("_r", "255").with("_t", "1 two 3")],
        models: vec![(m, Dict::default())],
    });
    let doc = Document::read(&b.build().write()).unwrap();
    let (inst, warn) = doc.scene().unwrap().instances(Rule::Compose);
    assert_eq!(inst.len(), 1);
    assert_eq!(warn.len(), 2);
}
