//! The brick scene file: every map comes back voxel for voxel, a file has one encoding, and
//! hostile bytes are refused without a panic.

use std::collections::BTreeMap;

use proptest::prelude::*;
use vxr_core::bricks::{decode, encode};
use vxr_core::{BrickMap, Scene};

fn voxels(m: &BrickMap) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    m.for_each(|p, c| {
        out.insert(p, c);
    });
    out
}

fn err(bytes: &[u8]) -> &'static str {
    decode(bytes).err().expect("the file is refused")
}

struct Case(BrickMap, [[u8; 4]; 256]);

impl std::fmt::Debug for Case {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "size {:?}, voxels {:?}", self.0.size_voxels(), voxels(&self.0))
    }
}

/// A map of any size up to the limit on one axis, with sizes that end inside a brick, set
/// and erased in random order so brick ids do not follow the grid and freed slots exist.
fn map() -> impl Strategy<Value = Case> {
    let size = prop_oneof![(1u32..70, 1u32..70, 1u32..70), (1u32..=2048, 1u32..20, 1u32..300)];
    (size, any::<[u8; 3]>(), prop::collection::vec(any::<[u8; 4]>(), 256)).prop_flat_map(
        |((sx, sy, sz), order, pal)| {
            let cell = (0..sx as i32, 0..sy as i32, 0..sz as i32);
            // Runs along x fill whole bricks; single cells leave sparse ones.
            let run = (cell.clone(), 1i32..40, 1u8..=255);
            let dot = (cell, prop_oneof![3 => 1u8..=255, 1 => Just(0u8)]);
            (prop::collection::vec(run, 0..12), prop::collection::vec(dot, 0..300)).prop_map(move |(runs, dots)| {
                let mut m = BrickMap::new([sx, sy, sz]);
                let [sx, sy, sz] = [sx, sy, sz].map(|s| s as i32);
                for ((x, y, z), n, c) in runs {
                    for dx in 0..n {
                        // The run turns the corner instead of stopping at the edge.
                        m.set(
                            (x + dx) % sx,
                            (y + dx * order[0] as i32 % 3) % sy,
                            (z + dx * order[1] as i32 % 3) % sz,
                            c,
                        );
                    }
                }
                for ((x, y, z), c) in dots {
                    m.set(x, y, z, c);
                }
                Case(m, std::array::from_fn(|i| pal[i]))
            })
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    #[test]
    fn a_map_round_trips_and_has_one_encoding(Case(m, pal) in map()) {
        let bytes = encode(&m, &pal);
        let (back, back_pal) = decode(&bytes).unwrap();
        prop_assert_eq!(back.size_voxels(), m.size_voxels());
        prop_assert_eq!(voxels(&back), voxels(&m));
        prop_assert_eq!((back.voxels, back.counts, back.live_bricks()), (m.voxels, m.counts, m.live_bricks()));
        for slot in 0..256 {
            // Only the colours a voxel shows are in the file.
            let want = if slot != 0 && m.counts[slot] != 0 { pal[slot] } else { [0; 4] };
            prop_assert_eq!(back_pal[slot], want, "slot {}", slot);
        }
        prop_assert_eq!(encode(&back, &back_pal), bytes);
    }

    /// A damaged file is refused or, when the damage only changed a colour, a slot or a
    /// position into another valid one, it still encodes back to the same bytes: nothing
    /// is read that the file does not say.
    #[test]
    fn a_damaged_file_is_refused_or_still_canonical(
        Case(m, pal) in map(),
        hits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..4),
        cut in prop::option::of(any::<prop::sample::Index>()),
        tail in prop::collection::vec(any::<u8>(), 0..3),
    ) {
        let good = encode(&m, &pal);
        let mut bytes = good.clone();
        for (at, v) in hits {
            let at = at.index(bytes.len());
            bytes[at] = v;
        }
        if let Some(cut) = cut {
            bytes.truncate(cut.index(bytes.len()));
        }
        bytes.extend(&tail);
        match decode(&bytes) {
            Ok((back, back_pal)) => prop_assert_eq!(encode(&back, &back_pal), bytes),
            Err(_) => prop_assert_ne!(bytes, good),
        }
    }

    /// The file says how long it is, so no shorter or longer file is the same scene.
    #[test]
    fn every_cut_and_every_extra_byte_is_refused(Case(m, pal) in map(), cut in any::<prop::sample::Index>(), extra in any::<u8>()) {
        let bytes = encode(&m, &pal);
        let cut = cut.index(bytes.len());
        prop_assert!(decode(&bytes[..cut]).is_err(), "cut at {} of {}", cut, bytes.len());
        let mut long = bytes;
        long.push(extra);
        prop_assert_eq!(err(&long), "the brick scene file has bytes after its last voxel");
    }
}

/// One voxel of slot 7 at the origin of an 8^3 map: the 45-byte header, slot 7's colour,
/// one brick at gap 0, its mask and its voxel.
fn one_voxel() -> Vec<u8> {
    let mut m = BrickMap::new([8, 8, 8]);
    m.set(0, 0, 0, 7);
    let mut pal = [[0; 4]; 256];
    pal[7] = [10, 20, 30, 255];
    encode(&m, &pal)
}

const SIZE: usize = 5;
const USED: usize = 11;
const COLOUR: usize = 43;
const COUNT: usize = 47;
const GAP: usize = 51;
const MASK: usize = 52;
const VOXEL: usize = 116;

#[test]
fn the_layout_is_the_documented_one() {
    let mut want = b"VXRB\x01\x08\x00\x08\x00\x08\x00".to_vec();
    want.extend([0u8; 32]);
    want[USED] = 1 << 7;
    want.extend([10, 20, 30, 255]);
    want.extend(1u32.to_le_bytes());
    want.push(0);
    want.extend([0u8; 64]);
    want[MASK] = 1;
    want.push(7);
    assert_eq!(one_voxel(), want);
    assert_eq!(want.len(), VOXEL + 1);
}

#[test]
fn hostile_files_are_refused_with_the_reason() {
    let edit = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = one_voxel();
        f(&mut b);
        b
    };
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("empty", vec![], "not a brick scene file"),
        ("a .vox", b"VOX \xc8\0\0\0MAIN".to_vec(), "not a brick scene file"),
        ("version 2", edit(&|b| b[4] = 2), "the brick scene file is a version this viewer does not read"),
        ("size 0", edit(&|b| b[SIZE] = 0), "the brick scene file's size is not 1 to 2048 voxels on each axis"),
        (
            "size 2049",
            edit(&|b| b[SIZE + 2..SIZE + 4].copy_from_slice(&2049u16.to_le_bytes())),
            "the brick scene file's size is not 1 to 2048 voxels on each axis",
        ),
        ("slot 0 used", edit(&|b| b[USED] |= 1), "the brick scene file gives slot 0, the empty cell, a colour"),
        // A count far past the file's length must not be believed (nor allocated for).
        (
            "count",
            edit(&|b| b[COUNT..COUNT + 4].copy_from_slice(&u32::MAX.to_le_bytes())),
            "the brick scene file is cut short",
        ),
        // The only grid cell is 0.
        ("brick outside", edit(&|b| b[GAP] = 1), "the brick scene file has a brick outside its size"),
        (
            "padded gap",
            edit(&|b| b.splice(GAP..=GAP, [0x80, 0]).for_each(drop)),
            "the brick scene file has a brick position it cannot hold",
        ),
        (
            "gap past u32",
            edit(&|b| b.splice(GAP..=GAP, [0xff, 0xff, 0xff, 0xff, 0x1f]).for_each(drop)),
            "the brick scene file has a brick position it cannot hold",
        ),
        ("empty brick", edit(&|b| b[MASK] = 0), "the brick scene file has an empty brick"),
        ("voxel slot 0", edit(&|b| b[VOXEL] = 0), "the brick scene file has a voxel in a slot with no colour"),
        ("voxel slot unlisted", edit(&|b| b[VOXEL] = 8), "the brick scene file has a voxel in a slot with no colour"),
        (
            "unused colour",
            edit(&|b| {
                b[USED] |= 1 << 6;
                b.splice(COLOUR..COLOUR, [1, 2, 3, 4]).for_each(drop);
            }),
            "the brick scene file has a colour no voxel uses",
        ),
        // A 5^3 map keeps cells 5 to 7 of its one brick empty, on every axis.
        (
            "voxel past x",
            edit(&|b| {
                b[SIZE] = 5;
                b[MASK] = 1 << 5;
            }),
            "the brick scene file has a voxel outside its size",
        ),
        (
            "voxel past y",
            edit(&|b| {
                b[SIZE + 2] = 5;
                b[MASK] = 0;
                b[MASK + 5] = 1;
            }),
            "the brick scene file has a voxel outside its size",
        ),
        (
            "voxel past z",
            edit(&|b| {
                b[SIZE + 4] = 5;
                b[MASK] = 0;
                b[MASK + 40] = 1;
            }),
            "the brick scene file has a voxel outside its size",
        ),
    ];
    for (name, bytes, want) in cases {
        assert_eq!(err(&bytes), want, "{name}");
    }
    // The last cell inside a 5^3 map is still accepted.
    let ok = edit(&|b| {
        b[SIZE] = 5;
        b[SIZE + 2] = 5;
        b[SIZE + 4] = 5;
        b[MASK] = 0;
        b[MASK + 4 * 8 + 4] = 1 << 4;
    });
    let (m, _) = decode(&ok).unwrap();
    assert_eq!(voxels(&m), BTreeMap::from([([4, 4, 4], 7)]));
}

/// The reason the format exists: the island's bricks are far smaller than its `.vox`, before
/// any compression. (Compressed, 14.5 KB against 137 KB: `docs/decisions.md`.)
#[test]
fn the_island_as_bricks_is_under_a_third_of_its_vox_and_reads_back() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    let Ok(vox) = std::fs::read(path) else {
        eprintln!("SKIPPED: no island (run `node prototype/src/build.mjs`)");
        return;
    };
    let scene = Scene::from_vox(&vox).unwrap();
    let bytes = encode(&scene.map, &scene.palette);
    let (back, pal) = decode(&bytes).unwrap();
    assert_eq!(back.voxels, 145_064);
    assert_eq!(voxels(&back), voxels(&scene.map));
    assert!((1..256).all(|s| scene.map.counts[s] == 0 || pal[s] == scene.palette[s]));
    assert!(bytes.len() * 3 < vox.len(), "{} B as bricks, {} B as .vox", bytes.len(), vox.len());
    eprintln!("island: {} B as bricks, {} B as .vox", bytes.len(), vox.len());
}
