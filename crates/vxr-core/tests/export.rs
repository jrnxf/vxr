//! OBJ export: greedy quads cover exactly the visible faces with the right colours, wound
//! to face outwards, and the OBJ text refers only to vertices, UVs and normals it defines.

use std::collections::{BTreeMap, HashMap};

use proptest::prelude::*;
use vxr_core::export::{Quad, Sprite, obj, quads, slices, sprite};
use vxr_core::{BrickMap, Scene};

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

/// Every visible face of `voxels`: (axis, positive, plane, u, v) to its colour.
fn faces(voxels: &BTreeMap<[i32; 3], u8>) -> HashMap<(usize, bool, i32, i32, i32), u8> {
    let mut out = HashMap::new();
    for (&p, &c) in voxels {
        for axis in 0..3 {
            for positive in [false, true] {
                let mut n = p;
                n[axis] += if positive { 1 } else { -1 };
                if !voxels.contains_key(&n) {
                    out.insert((axis, positive, p[axis] + positive as i32, p[(axis + 1) % 3], p[(axis + 2) % 3]), c);
                }
            }
        }
    }
    out
}

/// The quads must tile the visible faces exactly, each with the colour of the voxel behind.
fn check_cover(voxels: &BTreeMap<[i32; 3], u8>, qs: &[Quad]) -> Result<(), String> {
    let want = faces(voxels);
    let mut got = HashMap::new();
    for q in qs {
        for u in q.u[0]..q.u[1] {
            for v in q.v[0]..q.v[1] {
                if got.insert((q.axis, q.positive, q.k, u, v), q.index).is_some() {
                    return Err(format!("overlap at {q:?}"));
                }
            }
        }
        // Counter-clockwise from outside: (c1 - c0) x (c3 - c0) points along the normal.
        let c = q.corners();
        let d = |a: [i32; 3], b: [i32; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let (e1, e2) = (d(c[0], c[1]), d(c[0], c[3]));
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let s = if q.positive { 1 } else { -1 };
        if (0..3).any(|a| if a == q.axis { n[a] * s <= 0 } else { n[a] != 0 }) {
            return Err(format!("wound inwards: {q:?}"));
        }
    }
    if got != want {
        return Err(format!("{} faces covered, {} visible", got.len(), want.len()));
    }
    Ok(())
}

fn cells(list: &[([i32; 3], u8)]) -> BTreeMap<[i32; 3], u8> {
    list.iter().copied().collect()
}

#[test]
fn simple_shapes_merge_into_the_fewest_quads() {
    let one = cells(&[([0, 0, 0], 3)]);
    let q = quads(&one);
    assert_eq!(q.len(), 6);
    check_cover(&one, &q).unwrap();
    assert_eq!(obj(&one, &[[0; 4]; 256], "one").vertices, 8);

    // A 3x3x3 cube of one colour: one quad per side.
    let mut cube = BTreeMap::new();
    for x in 0..3 {
        for y in 0..3 {
            for z in 0..3 {
                cube.insert([x, y, z], 7);
            }
        }
    }
    let q = quads(&cube);
    assert_eq!(q.len(), 6);
    assert!(q.iter().all(|q| q.area() == 9));
    check_cover(&cube, &q).unwrap();

    // Two colours in a bar: the two ends, and each long side split by colour.
    let bar = cells(&[([0, 0, 0], 2), ([1, 0, 0], 2), ([2, 0, 0], 5)]);
    let q = quads(&bar);
    assert_eq!(q.len(), 2 + 4 * 2);
    check_cover(&bar, &q).unwrap();

    // An L: faces on the same plane but split by the corner's hidden face.
    let l = cells(&[([0, 0, 0], 1), ([1, 0, 0], 1), ([0, 1, 0], 1)]);
    check_cover(&l, &quads(&l)).unwrap();
}

#[test]
fn obj_text_is_centred_textured_and_well_formed() {
    let mut palette = [[0u8; 4]; 256];
    for (i, c) in palette.iter_mut().enumerate() {
        *c = [i as u8, 255 - i as u8, 7, 255];
    }
    let v = cells(&[([10, 4, -3], 2), ([11, 4, -3], 9)]);
    let o = obj(&v, &palette, "bar");
    assert!(o.obj.contains("mtllib bar.mtl\n"));
    assert!(o.mtl.contains("map_Kd bar.png\n"));
    // Bottom centre on the origin: x from -1 to 1, y from 0 to 1, z from -0.5 to 0.5.
    let vs: Vec<[f32; 3]> = o
        .obj
        .lines()
        .filter_map(|l| l.strip_prefix("v "))
        .map(|l| {
            let n: Vec<f32> = l.split(' ').map(|x| x.parse().unwrap()).collect();
            [n[0], n[1], n[2]]
        })
        .collect();
    assert_eq!(vs.len(), o.vertices);
    for a in 0..3 {
        let lo = vs.iter().map(|p| p[a]).fold(f32::MAX, f32::min);
        let hi = vs.iter().map(|p| p[a]).fold(f32::MIN, f32::max);
        assert_eq!((lo, hi), [(-1.0, 1.0), (0.0, 1.0), (-0.5, 0.5)][a]);
    }
    // Texel i - 1 holds index i's colour, and each UV sits on its colour's texel centre.
    assert_eq!(o.texture.len(), 256 * 4);
    assert_eq!(o.texture[4..8], palette[2]);
    let vts: Vec<f32> = o
        .obj
        .lines()
        .filter_map(|l| l.strip_prefix("vt "))
        .map(|l| l.split(' ').next().unwrap().parse().unwrap())
        .collect();
    let mut texels: Vec<usize> = vts.iter().map(|u| (u * 256.0) as usize).collect();
    texels.sort();
    assert_eq!(texels, [1, 8]);
}

#[test]
fn every_file_exports_a_mesh_covering_its_visible_faces() {
    for (name, bytes) in files() {
        let Ok(scene) = Scene::from_vox(&bytes) else { continue };
        let voxels = scene.world_voxels(true);
        let qs = quads(&voxels);
        check_cover(&voxels, &qs).unwrap_or_else(|e| panic!("{name}: {e}"));
        let o = obj(&voxels, &scene.palette, "m");
        let (nv, nt) = (o.vertices, o.obj.lines().filter(|l| l.starts_with("vt ")).count());
        let fs: Vec<&str> = o.obj.lines().filter_map(|l| l.strip_prefix("f ")).collect();
        assert_eq!(fs.len(), qs.len(), "{name}");
        for f in fs {
            for c in f.split(' ') {
                let i: Vec<usize> = c.split('/').map(|x| x.parse().unwrap()).collect();
                assert!(i[0] >= 1 && i[0] <= nv && i[1] >= 1 && i[1] <= nt && i[2] >= 1 && i[2] <= 6, "{name}: {c}");
            }
        }
        // Greedy merging pays: never more quads than faces, far fewer on the island.
        let area: i64 = qs.iter().map(Quad::area).sum();
        assert!(qs.len() as i64 <= area, "{name}");
        if name == "island" {
            assert!(qs.len() * 3 < area as usize, "island: {} quads for {area} faces", qs.len());
        }
    }
}

#[test]
fn slices_lay_the_layers_side_by_side_from_the_bottom() {
    let mut palette = [[0, 0, 0, 255]; 256];
    palette[3] = [200, 10, 20, 255];
    palette[7] = [1, 2, 3, 255];
    // A 3x2x4 model: one voxel on the bottom layer, one on the top, one outside it.
    let s = slices([3, 2, 4], [([2, 0, 1], 3), ([0, 1, 3], 7), ([3, 0, 0], 3)], &palette);
    assert_eq!((s.width, s.height, s.layers), (6, 4, 2));
    let px = |x: usize, y: usize| &s.rgba[(y * 6 + x) * 4..(y * 6 + x) * 4 + 4];
    assert_eq!(px(2, 1), [200, 10, 20, 255]);
    assert_eq!(px(3, 3), [1, 2, 3, 255]);
    assert_eq!(s.rgba.chunks(4).filter(|p| p[3] != 0).count(), 2);
}

#[test]
fn slices_hold_every_voxel_of_every_fixture_and_the_island() {
    for (name, bytes) in files() {
        let Ok(scene) = Scene::from_vox(&bytes) else { continue };
        let size = scene.bounds().size;
        let mut cells = Vec::new();
        scene.for_each_voxel(|p, c| cells.push((p, c)));
        let s = slices(size, cells, &scene.palette);
        assert_eq!((s.width, s.height), (size[0] * size[1], size[2]), "{name}");
        let size = size.map(|n| n as i32);
        let mut n = 0;
        scene.for_each_voxel(|p, c| {
            let at = ((p[2] * size[0] * size[1] + p[1] * size[0] + p[0]) * 4) as usize;
            let [r, g, b, _] = scene.palette[c as usize];
            assert_eq!(s.rgba[at..at + 4], [r, g, b, 255], "{name} {p:?}");
            n += 1;
        });
        assert_eq!(s.rgba.chunks(4).filter(|p| p[3] != 0).count(), n, "{name}");
    }
}

/// A sprite as text, one letter per pixel: `.` transparent, `T` the colour itself (a top
/// face), `R` four fifths of it (a right face), `L` three fifths (a left face).
fn letters(s: &Sprite, colour: [u8; 3]) -> Vec<String> {
    let shade = |n: u16| colour.map(|v| (v as u16 * n / 5) as u8);
    s.rgba
        .chunks(s.width as usize * 4)
        .map(|row| {
            row.chunks(4)
                .map(|p| match p {
                    [0, 0, 0, 0] => '.',
                    [r, g, b, 255] if [*r, *g, *b] == shade(5) => 'T',
                    [r, g, b, 255] if [*r, *g, *b] == shade(4) => 'R',
                    [r, g, b, 255] if [*r, *g, *b] == shade(3) => 'L',
                    _ => '?',
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_sprite_draws_each_voxel_as_a_pixel_art_cube() {
    let mut palette = [[0, 0, 0, 255]; 256];
    palette[9] = [200, 100, 50, 255];
    let one = |at: [i32; 3]| {
        let mut m = BrickMap::new([16, 16, 16]);
        m.set(at[0], at[1], at[2], 9);
        m
    };
    // One voxel, wherever it is in the map: the image is the voxel's own box.
    let s = sprite(&one([5, 9, 2]), &palette, 1).unwrap();
    assert_eq!(letters(&s, [200, 100, 50]), ["TTTT", "LLRR", "LLRR"]);
    let s = sprite(&one([0, 0, 15]), &palette, 2).unwrap();
    assert_eq!(
        letters(&s, [200, 100, 50]),
        ["..TTTT..", "TTTTTTTT", "LLTTTTRR", "LLLLRRRR", "LLLLRRRR", "LLLLRRRR", "..LLRR.."]
    );
    // An L of three voxels: one step along x goes two pixels right and one down, one along
    // z two left and one down, and the nearer voxel hides the faces behind it. The image is
    // the box of the voxels, so the empty near corner leaves the last row clear.
    let mut m = one([3, 0, 3]);
    m.set(4, 0, 3, 9);
    m.set(3, 0, 4, 9);
    let s = sprite(&m, &palette, 1).unwrap();
    assert_eq!(letters(&s, [200, 100, 50]), ["..TTTT..", "TTTTTTTT", "LLRRLLRR", "LLRRLLRR", "........"]);
    // A column of two: one voxel up is two pixels up, and the upper one hides the top below.
    let mut m = one([3, 0, 3]);
    m.set(3, 1, 3, 9);
    let s = sprite(&m, &palette, 1).unwrap();
    assert_eq!(letters(&s, [200, 100, 50]), ["TTTT", "LLRR", "LLRR", "LLRR", "LLRR"]);
    assert!(sprite(&BrickMap::new([16, 16, 16]), &palette, 1).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(256),
        ..ProptestConfig::default()
    })]

    /// The sprite is what a painter makes: every voxel stamped as the one-voxel cube, the
    /// farthest first, each at its place by the three steps.
    #[test]
    fn a_sprite_is_every_cube_painted_back_to_front(
        // Cells on both sides of a brick edge (8) on every axis.
        cells in prop::collection::btree_map([5..11i32, 6..11i32, 4..11i32], 1..4u8, 1..60),
        scale in 1..4u32,
    ) {
        let mut palette = [[0, 0, 0, 255]; 256];
        palette[1] = [250, 20, 40, 255];
        palette[2] = [30, 240, 60, 255];
        palette[3] = [15, 25, 235, 255];
        let mut m = BrickMap::new([16, 16, 16]);
        for (p, c) in &cells {
            m.set(p[0], p[1], p[2], *c);
        }
        let got = sprite(&m, &palette, scale).unwrap();

        let lo = [0, 1, 2].map(|a| cells.keys().map(|p| p[a]).min().unwrap());
        let hi = [0, 1, 2].map(|a| cells.keys().map(|p| p[a]).max().unwrap() + 1);
        let s = scale as i32;
        let (w, h) = (2 * s * (hi[0] - lo[0] + hi[2] - lo[2]), s * (hi[0] - lo[0] + hi[2] - lo[2]) + 2 * s * (hi[1] - lo[1]) - 1);
        prop_assert_eq!((got.width as i32, got.height as i32), (w, h));
        let mut want = vec![0u8; (w * h * 4) as usize];
        let mut order: Vec<_> = cells.iter().collect();
        order.sort_by_key(|(p, _)| p[0] + p[1] + p[2]);
        for (p, c) in order {
            let mut one = BrickMap::new([8, 8, 8]);
            one.set(0, 0, 0, *c);
            let stamp = sprite(&one, &palette, scale).unwrap();
            let [x, y, z] = [0, 1, 2].map(|a| p[a] - lo[a]);
            // The stamp's top left corner: the box's left edge is the far end of z, its top the top layer.
            let left = 2 * s * (x - z) + 2 * s * (hi[2] - lo[2] - 1);
            let top = s * (x + z) - 2 * s * y + 2 * s * (hi[1] - lo[1] - 1);
            for j in 0..stamp.height as i32 {
                for i in 0..stamp.width as i32 {
                    let px = &stamp.rgba[((j * stamp.width as i32 + i) * 4) as usize..][..4];
                    if px[3] != 0 {
                        want[(((top + j) * w + left + i) * 4) as usize..][..4].copy_from_slice(px);
                    }
                }
            }
        }
        prop_assert!(got.rgba == want, "scale {} cells {:?}", scale, cells);
    }
}
