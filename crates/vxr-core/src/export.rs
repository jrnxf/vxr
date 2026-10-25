//! OBJ export with a palette texture: every visible voxel face, merged into rectangles of
//! one colour (greedy meshing), textured by a 256x1 image of the palette.
//!
//! Axes are vxr's (y-up, right-handed, one unit per voxel), which is also OBJ's convention.
//! The mesh sits on the origin: the voxels' bounds are centred on X and Z and their bottom
//! is at Y = 0, so a prop drops onto the ground in a game engine. Texel `i - 1` holds the
//! colour of voxel index `i`, as in MagicaVoxel's palette images and OBJ export, and every
//! vertex of a quad takes the UV of its texel's centre.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

/// Texture width: one texel per palette slot (index 1 to 255, texel 255 unused).
pub const TEXTURE_W: u32 = 256;

/// An OBJ export: the `.obj` text, its `.mtl` and the palette texture's RGBA texels.
pub struct Obj {
    pub obj: String,
    pub mtl: String,
    /// `TEXTURE_W` x 1 RGBA texels.
    pub texture: Vec<u8>,
    pub quads: usize,
    pub vertices: usize,
}

/// One merged face: on the plane `axis = k`, facing `+axis` or `-axis`, covering `u0..u1`
/// and `v0..v1` on the next two axes (`(axis + 1) % 3` and `(axis + 2) % 3`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quad {
    pub axis: usize,
    pub positive: bool,
    pub k: i32,
    pub u: [i32; 2],
    pub v: [i32; 2],
    pub index: u8,
}

impl Quad {
    /// Its four corners, counter-clockwise seen from the side it faces.
    pub fn corners(&self) -> [[i32; 3]; 4] {
        let at = |u: i32, v: i32| {
            let mut p = [0; 3];
            p[self.axis] = self.k;
            p[(self.axis + 1) % 3] = u;
            p[(self.axis + 2) % 3] = v;
            p
        };
        let [u0, u1] = self.u;
        let [v0, v1] = self.v;
        // e_u x e_v = e_axis, so this order is counter-clockwise seen from +axis.
        let ccw = [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)];
        if self.positive { ccw } else { [ccw[0], ccw[3], ccw[2], ccw[1]] }
    }

    pub fn area(&self) -> i64 {
        (self.u[1] - self.u[0]) as i64 * (self.v[1] - self.v[0]) as i64
    }
}

/// One plane's visible faces: cell (u, v) to its colour.
type Plane = HashMap<(i32, i32), u8>;

/// Every visible face of `voxels` (cell to palette index, 0 is empty), merged greedily into
/// rectangles of one colour. A face is visible when the cell beside it is empty.
pub fn quads(voxels: &BTreeMap<[i32; 3], u8>) -> Vec<Quad> {
    // Visible faces by (axis, side, plane): cell (u, v) on that plane to its colour.
    let mut planes: BTreeMap<(usize, bool, i32), Plane> = BTreeMap::new();
    for (&p, &c) in voxels {
        if c == 0 {
            continue;
        }
        for axis in 0..3 {
            for positive in [false, true] {
                let mut n = p;
                n[axis] += if positive { 1 } else { -1 };
                if voxels.get(&n).is_some_and(|&c| c != 0) {
                    continue;
                }
                let k = p[axis] + positive as i32;
                let (u, v) = (p[(axis + 1) % 3], p[(axis + 2) % 3]);
                planes.entry((axis, positive, k)).or_default().insert((u, v), c);
            }
        }
    }
    let mut out = Vec::new();
    for ((axis, positive, k), mut cells) in planes {
        let mut order: Vec<(i32, i32)> = cells.keys().copied().collect();
        order.sort_by_key(|&(u, v)| (v, u));
        for (u0, v0) in order {
            let Some(c) = cells.remove(&(u0, v0)) else { continue };
            let mut u1 = u0 + 1;
            while cells.get(&(u1, v0)) == Some(&c) {
                cells.remove(&(u1, v0));
                u1 += 1;
            }
            let mut v1 = v0 + 1;
            while (u0..u1).all(|u| cells.get(&(u, v1)) == Some(&c)) {
                for u in u0..u1 {
                    cells.remove(&(u, v1));
                }
                v1 += 1;
            }
            out.push(Quad { axis, positive, k, u: [u0, u1], v: [v0, v1], index: c });
        }
    }
    out
}

/// The OBJ export of `voxels` coloured by `palette` (RGBA by voxel index). `name` names the
/// object and the `.mtl` and `.png` files the `.obj` refers to (`<name>.mtl`, `<name>.png`).
pub fn obj(voxels: &BTreeMap<[i32; 3], u8>, palette: &[[u8; 4]; 256], name: &str) -> Obj {
    let quads = quads(voxels);
    // Bottom centre of the bounds, doubled so it stays an integer.
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    for p in voxels.keys() {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a] + 1);
        }
    }
    let pivot2 = if voxels.is_empty() { [0; 3] } else { [lo[0] + hi[0], 2 * lo[1], lo[2] + hi[2]] };
    let coord = |p: [i32; 3], a: usize| (2 * p[a] - pivot2[a]) as f32 / 2.0;

    let mut obj = String::new();
    let _ = writeln!(obj, "# vxr OBJ export: {} voxels, {} quads", voxels.len(), quads.len());
    let _ = writeln!(obj, "mtllib {name}.mtl");
    let _ = writeln!(obj, "o {name}");
    let mut vertex: HashMap<[i32; 3], usize> = HashMap::new();
    let mut faces = Vec::with_capacity(quads.len());
    for q in &quads {
        let ids = q.corners().map(|p| {
            let n = vertex.len() + 1;
            *vertex.entry(p).or_insert_with(|| {
                let _ = writeln!(obj, "v {} {} {}", coord(p, 0), coord(p, 1), coord(p, 2));
                n
            })
        });
        faces.push(ids);
    }
    // One UV per used colour, at its texel's centre.
    let mut uv: BTreeMap<u8, usize> = BTreeMap::new();
    for q in &quads {
        let n = uv.len() + 1;
        uv.entry(q.index).or_insert(n);
    }
    let mut uvs: Vec<(u8, usize)> = uv.iter().map(|(&i, &n)| (i, n)).collect();
    uvs.sort_by_key(|&(_, n)| n);
    for (i, _) in uvs {
        let _ = writeln!(obj, "vt {} 0.5", (i as f32 - 0.5) / TEXTURE_W as f32);
    }
    for (a, s) in [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)] {
        let mut n = [0; 3];
        n[a] = s;
        let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], n[2]);
    }
    let _ = writeln!(obj, "usemtl palette");
    for (q, ids) in quads.iter().zip(&faces) {
        let (t, n) = (uv[&q.index], 1 + 2 * q.axis + !q.positive as usize);
        let _ = writeln!(obj, "f {0}/{t}/{n} {1}/{t}/{n} {2}/{t}/{n} {3}/{t}/{n}", ids[0], ids[1], ids[2], ids[3]);
    }

    let mtl =
        format!("# vxr OBJ export\nnewmtl palette\nKa 0 0 0\nKd 1 1 1\nKs 0 0 0\nd 1\nillum 1\nmap_Kd {name}.png\n");
    let mut texture = Vec::with_capacity(TEXTURE_W as usize * 4);
    for t in 0..TEXTURE_W as usize {
        texture.extend_from_slice(&palette[(t + 1) % 256]);
    }
    Obj { obj, mtl, texture, quads: quads.len(), vertices: vertex.len() }
}
