//! Image import: a PNG read from a stranger becomes voxels one cell thick, upright, each
//! pixel matched to the palette, as one undo step that grows the bounds when it must.

use std::collections::BTreeMap;

use proptest::prelude::*;
use vxr_core::Scene;
use vxr_core::image::{self, Image};

/// A PNG of `w` x `h` RGBA pixels (top row first), written as RGBA or, when every pixel is
/// opaque and `rgb` is set, as RGB.
fn png(w: u32, h: u32, px: &[[u8; 4]], rgb: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    let rgb = rgb && px.iter().all(|p| p[3] == 255);
    e.set_color(if rgb { png::ColorType::Rgb } else { png::ColorType::Rgba });
    let mut wr = e.write_header().unwrap();
    let data: Vec<u8> = px.iter().flat_map(|p| if rgb { p[..3].to_vec() } else { p.to_vec() }).collect();
    wr.write_image_data(&data).unwrap();
    drop(wr);
    out
}

fn voxels(s: &Scene) -> BTreeMap<[i32; 3], u8> {
    let mut out = BTreeMap::new();
    s.for_each_voxel(|p, v| {
        out.insert(s.to_world(p), v);
    });
    out
}

proptest! {
    /// Pixels in palette colours land on exactly their cells (bottom row on the lowest layer)
    /// with the first slot of their colour, transparent pixels stay empty, the bounds grow
    /// to hold the image, and one undo takes back the voxels and the bounds.
    #[test]
    fn an_image_in_palette_colours_round_trips(
        w in 1u32..12, h in 1u32..12,
        picks in prop::collection::vec((1u8..=255, any::<bool>()), 144),
        palette in prop::collection::vec(prop::sample::select(vec![0u8, 60, 128, 200, 255]), 255 * 3),
        at in prop::option::of(prop::array::uniform3(-6i32..14)),
        rgb in any::<bool>(),
    ) {
        let mut s = Scene::new([8, 8, 8]);
        for i in 1..256 {
            s.palette[i] = [palette[(i - 1) * 3], palette[(i - 1) * 3 + 1], palette[(i - 1) * 3 + 2], 255];
        }
        let n = (w * h) as usize;
        let px: Vec<[u8; 4]> = picks[..n].iter().map(|&(slot, solid)| {
            let c = s.palette[slot as usize];
            [c[0], c[1], c[2], if solid { 255 } else { 127 }]
        }).collect();
        let img = image::decode(&png(w, h, &px, rgb)).unwrap();
        prop_assert_eq!(&img, &Image { width: w, height: h, pixels: px.clone() });

        let before = s.bounds();
        let Ok(done) = s.import_image(&img, at) else {
            prop_assert!(px.iter().all(|p| p[3] < 128));
            return Ok(());
        };
        let first = |c: [u8; 4]| (1..256).find(|&i| s.palette[i][..3] == c[..3]).unwrap() as u8;
        let mut want = BTreeMap::new();
        for (i, p) in px.iter().enumerate().filter(|(_, p)| p[3] == 255) {
            let (x, y) = ((i as u32 % w) as i32, (h - 1 - i as u32 / w) as i32);
            want.insert([done.at[0] + x, done.at[1] + y, done.at[2]], first(*p));
        }
        prop_assert_eq!(voxels(&s), want.clone());
        prop_assert_eq!(done.voxels, want.len());
        if let Some(at) = at {
            prop_assert_eq!(done.at, at);
        }
        let b = s.bounds();
        let inside = |p: &[i32; 3]| (0..3).all(|a| p[a] >= b.lo[a] && p[a] < b.lo[a] + b.size[a] as i32);
        prop_assert!(want.keys().all(inside));
        prop_assert_eq!(done.grew, b != before);
        prop_assert!(s.undo());
        prop_assert_eq!(s.map.voxels, 0);
        prop_assert_eq!(s.bounds(), before);
    }
}

#[test]
fn the_default_place_stands_in_front_of_the_model() {
    // 20 wide into an empty 8 cube: centred, halfway back, the bounds grow 6 each way on x.
    let mut s = Scene::new([8, 8, 8]);
    let b = s.bounds();
    let img = Image { width: 20, height: 5, pixels: vec![[255, 0, 0, 255]; 100] };
    let done = s.import_image(&img, None).unwrap();
    assert_eq!(done.at, [b.lo[0] - 6, b.floor, b.lo[2] + 4]);
    assert!(done.grew);
    let nb = s.bounds();
    assert_eq!((nb.lo, nb.size, nb.floor), ([b.lo[0] - 6, b.lo[1], b.lo[2]], [20, 8, 8], b.floor));
    assert_eq!((done.voxels, done.slots), (100, 1));
    // Again: one cell in front of the first, inside the bounds.
    let small = Image { width: 2, height: 1, pixels: vec![[255, 0, 0, 255]; 2] };
    let done = s.import_image(&small, None).unwrap();
    assert_eq!((done.at, done.grew, done.voxels), ([b.lo[0] + 3, b.floor, b.lo[2] + 5], false, 2));
    // A voxel on the bounds' front layer, high up: the next one stands in front of it, and
    // the bounds grow one layer toward the viewer.
    s.edit([(s.to_map([b.lo[0], b.lo[1] + 7, b.lo[2] + 7]), 1)]);
    let done = s.import_image(&small, None).unwrap();
    assert_eq!((done.at, done.grew), ([b.lo[0] + 3, b.floor, b.lo[2] + 8], true));
    assert_eq!(s.bounds().size, [20, 8, 9]);
}

#[test]
fn colours_off_the_palette_go_to_the_nearest_slot() {
    let mut p = [[0u8, 0, 0, 255]; 256];
    p[1] = [0, 0, 0, 255];
    p[2] = [255, 255, 255, 255];
    p[3] = [200, 30, 30, 255];
    p[4] = [30, 30, 200, 255];
    p[5..].fill([128, 128, 128, 255]);
    for (rgb, slot) in [
        ([250, 10, 10], 3),
        ([10, 10, 250], 4),
        ([20, 20, 20], 1),
        ([240, 240, 230], 2),
        ([120, 130, 125], 5),
        ([0, 0, 0], 1),
    ] {
        assert_eq!(image::nearest(&p, rgb), slot, "{rgb:?}");
    }
}

/// A PNG built chunk by chunk, so a test can break one: an 8-bit indexed 4 x 1 image whose
/// `PLTE` is `plte`, its pixels slots 0 to 3.
fn indexed(plte: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = png_crc(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &[0, 0, 0, 4, 0, 0, 0, 1, 8, 3, 0, 0, 0]);
    chunk(&mut out, b"PLTE", plte);
    // Filter byte 0 and the slots 0, 1, 2, 3, stored (zlib with no compression).
    let raw = [0u8, 0, 1, 2, 3];
    let mut z = vec![0x78, 0x01, 1, raw.len() as u8, 0, !(raw.len() as u8), 0xff];
    z.extend(raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend((b << 16 | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn png_crc(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in data {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Found by the fuzz target `image_decode`: the `png` decoder panics on a `PLTE` that is not
/// a whole number of colours. Both readers of PNGs from strangers refuse it instead.
#[test]
fn a_broken_palette_chunk_is_refused_not_a_panic() {
    let good: Vec<u8> = (0..12).collect();
    let img = image::decode(&indexed(&good)).unwrap();
    assert_eq!(img.pixels, [[0, 1, 2, 255], [3, 4, 5, 255], [6, 7, 8, 255], [9, 10, 11, 255]]);
    assert_eq!(vxr_core::palette::parse(&indexed(&good)).unwrap()[3], [9, 10, 11, 255]);
    for len in [13, 4, 0, 771] {
        let why = format!("the PNG does not read (its palette is {len} bytes, not 1 to 256 colors)");
        let bad = indexed(&vec![7; len]);
        assert_eq!(image::decode(&bad).unwrap_err(), why);
        assert_eq!(vxr_core::palette::parse(&bad).unwrap_err(), why);
    }
}

#[test]
fn files_that_cannot_import_are_refused_and_change_nothing() {
    let wide = png(1025, 1, &vec![[9, 9, 9, 255]; 1025], true);
    let mut cut = png(4, 4, &[[9, 9, 9, 255]; 16], true);
    cut.truncate(cut.len() - 20);
    for (bytes, why) in
        [(b"GIF89a".to_vec(), "it is not a PNG image"), (wide, "the image is 1025×1 pixels: at most 1024 on a side")]
    {
        assert_eq!(image::decode(&bytes).unwrap_err(), why);
    }
    assert!(image::decode(&cut).unwrap_err().starts_with("the PNG does not read"));

    let mut s = Scene::new([8, 8, 8]);
    let clear = Image { width: 2, height: 1, pixels: vec![[255, 0, 0, 127]; 2] };
    assert_eq!(s.import_image(&clear, None).unwrap_err(), "every pixel of the image is transparent");
    // Past the side limit of a model: the bounds would grow to 2049.
    let red = Image { width: 1, height: 1, pixels: vec![[255, 0, 0, 255]] };
    assert!(s.import_image(&red, Some([2048, 0, 0])).is_err());
    assert_eq!((s.map.voxels, s.history().can_undo(), s.bounds()), (0, false, Scene::new([8, 8, 8]).bounds()));

    let world = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "22-world").unwrap().builder.build().write();
    let mut s = Scene::from_vox(&world).unwrap();
    assert!(s.world().is_some());
    let shown = voxels(&s);
    assert_eq!(s.import_image(&red, None).unwrap_err(), "the world shows: edit an object to import an image into it");
    assert_eq!(voxels(&s), shown);
}
