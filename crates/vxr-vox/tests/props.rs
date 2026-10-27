//! Property tests: random scenes, including ones bigger than 256^3 that split into
//! several models, must write-read-write byte-identical and read back the same voxels.

use std::collections::BTreeMap;

use proptest::prelude::*;
use vxr_vox::build::{Builder, SceneNode};
use vxr_vox::raw::{Chunk, RawFile};
use vxr_vox::scene::{Rule, rotation_from_byte};
use vxr_vox::{Dict, Document};

static CASES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static OVER_256: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn world() -> impl Strategy<Value = BTreeMap<[i32; 3], u8>> {
    // Extents up to 700 per axis, offset into negative space, so most scenes span
    // several 256^3 blocks.
    (1i32..700, 1i32..700, 1i32..400, -300i32..300, 1usize..2500).prop_flat_map(|(sx, sy, sz, off, n)| {
        prop::collection::vec(
            ((0..sx, 0..sy, 0..sz).prop_map(move |(x, y, z)| [x + off, y - off / 2, z]), 1u8..=255),
            1..=n,
        )
        .prop_map(|v| v.into_iter().collect::<BTreeMap<_, _>>())
    })
}

fn palette() -> impl Strategy<Value = Vec<[u8; 4]>> {
    prop::collection::vec(any::<[u8; 4]>(), 256)
}

proptest! {
    // PROPTEST_CASES overrides the default of 1000 cases per property.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1000),
        ..ProptestConfig::default()
    })]

    #[test]
    fn world_scene_round_trips(voxels in world(), pal in palette(), v200 in any::<bool>()) {
        CASES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut b = Builder::new(if v200 { 200 } else { 150 });
        b.palette = Some(pal);
        b.add_world(&voxels);
        let bytes = b.build().write();
        let doc = Document::read(&bytes).unwrap();
        prop_assert_eq!(doc.write(), bytes.clone());
        let scene = doc.scene().unwrap();
        prop_assert!(scene.models.iter().all(|m| m.size.iter().all(|&s| s <= 256)));
        if scene.models.len() > 1 {
            OVER_256.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        prop_assert_eq!(scene.world_voxels(Rule::Compose), voxels);
        // A second generation from the read-back scene is also identical.
        let again = Document::read(&doc.write()).unwrap();
        prop_assert_eq!(again.write(), bytes);
        if CASES.load(std::sync::atomic::Ordering::Relaxed).is_multiple_of(1000) {
            eprintln!("world_scene_round_trips: {} cases, {} split into several models", CASES.load(std::sync::atomic::Ordering::Relaxed), OVER_256.load(std::sync::atomic::Ordering::Relaxed));
        }
    }

    #[test]
    fn rotated_instances_land_where_expected(
        size in (1i32..20, 1i32..20, 1i32..20),
        rot in (0u8..128).prop_filter("valid rotation", |b| rotation_from_byte(*b).is_some()),
        t in (-500i32..500, -500i32..500, -500i32..500),
        seed in any::<u64>(),
    ) {
        // One model with an asymmetric pattern, placed with a random rotation or mirror.
        let mut vs = Vec::new();
        let mut s = seed;
        for x in 0..size.0 { for y in 0..size.1 { for z in 0..size.2 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            if s >> 61 == 0 { vs.push([x as u8, y as u8, z as u8, 1 + (s >> 40) as u8 % 254]); }
        }}}
        let mut b = Builder::new(200);
        let m = b.model([size.0, size.1, size.2], vs.clone());
        let r = rotation_from_byte(rot).unwrap();
        b.root.push(SceneNode::shape(m, Some(r), [t.0, t.1, t.2]));
        let bytes = b.build().write();
        let doc = Document::read(&bytes).unwrap();
        prop_assert_eq!(doc.write(), bytes);
        let scene = doc.scene().unwrap();
        let got = scene.world_voxels(Rule::Compose);
        // Independent placement: centre of voxel v relative to the pivot, rotated, plus t.
        let mut want = BTreeMap::new();
        let piv = [size.0 / 2, size.1 / 2, size.2 / 2];
        for v in &vs {
            let c = [v[0] as f64 + 0.5 - piv[0] as f64, v[1] as f64 + 0.5 - piv[1] as f64, v[2] as f64 + 0.5 - piv[2] as f64];
            let mut w = [0f64; 3];
            for i in 0..3 { w[i] = (0..3).map(|k| r[i][k] as f64 * c[k]).sum::<f64>() + [t.0, t.1, t.2][i] as f64; }
            want.insert([w[0].floor() as i32, w[1].floor() as i32, w[2].floor() as i32], v[3]);
        }
        prop_assert_eq!(got, want);
    }

    #[test]
    fn raw_chunk_lists_round_trip(
        version in any::<i32>(),
        chunks in prop::collection::vec((any::<[u8; 4]>(), prop::collection::vec(any::<u8>(), 0..64), prop::collection::vec(any::<u8>(), 0..8)), 0..40),
        trailing in prop::collection::vec(any::<u8>(), 0..16),
    ) {
        let raw = RawFile {
            version,
            main_content: vec![],
            chunks: chunks.into_iter().map(|(id, content, children)| Chunk { id, content, children }).collect(),
            trailing,
        };
        let bytes = raw.write();
        prop_assert_eq!(RawFile::parse(&bytes).unwrap(), raw);
        // The full reader may reject garbage, but must not panic, and if it accepts the
        // bytes it must give them back unchanged.
        if let Ok(doc) = Document::read(&bytes) {
            prop_assert_eq!(doc.write(), bytes);
        }
    }

    #[test]
    fn edits_stay_stable(voxels in world(), x in any::<u8>(), c in 0u8..=255, slot in 1u8..=255, rgba in any::<[u8; 4]>(), t in any::<[i16; 3]>()) {
        let mut b = Builder::new(200);
        b.add_world(&voxels);
        b.materials.push((1, Dict::default().with("_type", "_metal").with("_rough", "0.3")));
        let mut doc = Document::read(&b.build().write()).unwrap();
        let m0 = doc.scene().unwrap().models[0].size;
        let p = [(x as i32 % m0[0]) as u8, 0, 0];
        let before = doc.raw.chunks.clone();
        let i1 = doc.set_voxel(0, p, c).unwrap();
        let i2 = doc.set_palette(slot, rgba).unwrap();
        let i3 = doc.set_translation(2, [t[0] as i32, t[1] as i32, t[2] as i32]).unwrap();
        for (k, (a, z)) in before.iter().zip(&doc.raw.chunks).enumerate() {
            if a != z { prop_assert!(k == i1 || k == i2 || k == i3, "chunk {} changed", k); }
        }
        let w = doc.write();
        let again = Document::read(&w).unwrap();
        prop_assert_eq!(again.write(), w);
        prop_assert_eq!(again.scene().unwrap().color(slot), rgba);
    }
}

#[test]
fn big_sparse_scene_over_256_on_every_axis() {
    // 900 x 700 x 600 with a few thousand voxels, including the far corners.
    let mut voxels = BTreeMap::new();
    let mut s: u64 = 7;
    for _ in 0..5000 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        voxels.insert(
            [(s >> 20) as i32 % 900 - 450, (s >> 30) as i32 % 700, (s >> 40) as i32 % 600],
            1 + (s >> 56) as u8 % 250,
        );
    }
    for c in [[-450, 0, 0], [449, 699, 599], [-450, 699, 0]] {
        voxels.insert(c, 9);
    }
    let mut b = Builder::new(200);
    b.add_world(&voxels);
    let bytes = b.build().write();
    let doc = Document::read(&bytes).unwrap();
    assert_eq!(doc.write(), bytes);
    let scene = doc.scene().unwrap();
    assert!(scene.models.len() >= 4 * 3 * 3, "split into {} models", scene.models.len());
    assert_eq!(scene.world_voxels(Rule::Compose), voxels);
}

#[test]
fn blocks_are_cut_from_the_voxels_low_corner() {
    // Centred on the origin: one model, not one per side of each axis.
    let cube = |lo: i32, n: i32| {
        let mut v = BTreeMap::new();
        for x in lo..lo + n {
            for y in lo..lo + n {
                for z in [lo, lo + n - 1] {
                    v.insert([x, y, z], 1 + (x.rem_euclid(200)) as u8);
                }
            }
        }
        v
    };
    for (lo, n, models) in [(-16, 32, 1), (-128, 256, 1), (1000, 256, 1), (-128, 257, 8)] {
        let voxels = cube(lo, n);
        let mut b = Builder::new(200);
        b.add_world(&voxels);
        let bytes = b.build().write();
        let scene = Document::read(&bytes).unwrap().scene().unwrap();
        assert_eq!(scene.models.len(), models, "{n}^3 at {lo}");
        assert_eq!(scene.world_voxels(Rule::Compose), voxels, "{n}^3 at {lo}");
    }
}
