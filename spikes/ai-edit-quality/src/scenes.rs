//! Round 2's scenes, one per tier: a few shapes on a grass plate (simple), a farm (medium) and
//! the island with room above it (large). Each is one model, so it opens in model mode.

use std::collections::BTreeMap;
use std::path::Path;

use vxr_core::Scene;
use vxr_core::scene::world_to_vox;
use vxr_vox::build::{Builder, SceneNode};

type Cells = BTreeMap<[i32; 3], u8>;

/// Slot colours set on top of MagicaVoxel's default palette.
const COLORS: &[(u8, [u8; 3])] = &[
    (1, [94, 166, 74]),    // grass
    (2, [200, 56, 52]),    // red
    (3, [59, 111, 212]),   // blue
    (4, [232, 197, 71]),   // yellow
    (5, [236, 232, 220]),  // white wall
    (6, [92, 64, 51]),     // dark wood
    (7, [138, 106, 79]),   // planks
    (8, [74, 78, 92]),     // dark roof
    (9, [64, 112, 190]),   // water
    (10, [120, 86, 58]),   // soil
    (11, [214, 186, 92]),  // wheat
    (12, [58, 122, 64]),   // leaves
    (13, [70, 92, 160]),   // blue roof
    (14, [156, 204, 236]), // glass
    (15, [168, 150, 120]), // path
];

fn boxed(c: &mut Cells, lo: [i32; 3], hi: [i32; 3], v: u8) {
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                if v == 0 {
                    c.remove(&[x, y, z]);
                } else {
                    c.insert([x, y, z], v);
                }
            }
        }
    }
}

fn ball(c: &mut Cells, at: [i32; 3], r: i32, v: u8) {
    let reach = (r * r) as f32 + r as f32 / 2.0;
    for x in -r..=r {
        for y in -r..=r {
            for z in -r..=r {
                if ((x * x + y * y + z * z) as f32) <= reach {
                    c.insert([at[0] + x, at[1] + y, at[2] + z], v);
                }
            }
        }
    }
}

/// Walls of a box building from `lo` to `hi` (inclusive), with a gable roof along x.
fn house(c: &mut Cells, lo: [i32; 3], hi: [i32; 3], wall: u8, roof: u8) {
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                if x == lo[0] || x == hi[0] || z == lo[2] || z == hi[2] {
                    c.insert([x, y, z], wall);
                }
            }
        }
    }
    let (mut zl, mut zr, mut y) = (lo[2] - 1, hi[2] + 1, hi[1] + 1);
    while zl <= zr {
        for x in lo[0] - 1..=hi[0] + 1 {
            c.insert([x, y, zl], roof);
            c.insert([x, y, zr], roof);
        }
        for z in zl + 1..zr {
            c.insert([lo[0], y, z], wall);
            c.insert([hi[0], y, z], wall);
        }
        zl += 1;
        zr -= 1;
        y += 1;
    }
}

fn tree(c: &mut Cells, at: [i32; 3]) {
    boxed(c, at, [at[0], at[1] + 5, at[2]], 6);
    ball(c, [at[0], at[1] + 8, at[2]], 4, 12);
}

fn shapes() -> Cells {
    let mut c = Cells::new();
    boxed(&mut c, [-16, 0, -16], [15, 0, 15], 1);
    boxed(&mut c, [-12, 1, -4], [-7, 6, 1], 2);
    ball(&mut c, [2, 6, -2], 5, 3);
    boxed(&mut c, [9, 1, -3], [11, 12, -1], 4);
    c
}

fn farm() -> Cells {
    let mut c = Cells::new();
    boxed(&mut c, [-32, 0, -32], [31, 1, 31], 1);
    // A path from the front edge to the farmhouse door.
    boxed(&mut c, [10, 1, -14], [12, 1, 31], 15);
    // The red barn with a dark roof and a wide door on its front (+z) side.
    house(&mut c, [-26, 2, -26], [-12, 11, -14], 2, 8);
    boxed(&mut c, [-21, 2, -14], [-17, 7, -14], 0);
    // The white farmhouse with a blue roof, a door and windows.
    house(&mut c, [5, 2, -24], [17, 8, -15], 5, 13);
    boxed(&mut c, [11, 2, -15], [11, 4, -15], 0);
    for x in [7, 15] {
        boxed(&mut c, [x, 5, -15], [x + 1, 6, -15], 14);
    }
    // A pond.
    for x in -6..=6 {
        for z in -6..=6 {
            if x * x + z * z <= 36 {
                c.insert([18 + x, 1, 16 + z], 9);
            }
        }
    }
    // A wheat field in rows, fenced.
    for z in 4..=26 {
        let v = if z % 3 == 0 { 10 } else { 11 };
        boxed(&mut c, [-28, 2, z], [-8, 2, z], v);
    }
    for x in (-30..=-6).step_by(3) {
        boxed(&mut c, [x, 2, 2], [x, 4, 2], 7);
        boxed(&mut c, [x, 2, 28], [x, 4, 28], 7);
    }
    for z in (2..=28).step_by(3) {
        boxed(&mut c, [-30, 2, z], [-30, 4, z], 7);
        boxed(&mut c, [-6, 2, z], [-6, 4, z], 7);
    }
    boxed(&mut c, [-30, 4, 2], [-6, 4, 2], 7);
    boxed(&mut c, [-30, 4, 28], [-6, 4, 28], 7);
    boxed(&mut c, [-30, 4, 2], [-30, 4, 28], 7);
    boxed(&mut c, [-6, 4, 2], [-6, 4, 28], 7);
    tree(&mut c, [-2, 2, -28]);
    tree(&mut c, [26, 2, -6]);
    c
}

/// Write `cells` as one model whose bounds are `lo` and `size` (world cells), with the colours.
fn save(path: &Path, cells: &Cells, lo: [i32; 3], size: [u32; 3]) -> Result<(), String> {
    let mut palette = Scene::new([1, 1, 1]).palette;
    for &(slot, [r, g, b]) in COLORS {
        palette[slot as usize] = [r, g, b, 255];
    }
    let hi: [i32; 3] = std::array::from_fn(|a| lo[a] + size[a] as i32 - 1);
    let (a, b) = (world_to_vox(lo), world_to_vox(hi));
    let vlo: [i32; 3] = std::array::from_fn(|k| a[k].min(b[k]));
    let vsize: [i32; 3] = std::array::from_fn(|k| (a[k] - b[k]).abs() + 1);
    let voxels: Vec<[u8; 4]> = cells
        .iter()
        .map(|(p, &v)| {
            let q = world_to_vox(*p);
            [(q[0] - vlo[0]) as u8, (q[1] - vlo[1]) as u8, (q[2] - vlo[2]) as u8, v]
        })
        .collect();
    let mut builder = Builder::new(200);
    builder.palette = Some((0..256).map(|k| palette[(k + 1) % 256]).collect());
    let m = builder.model(vsize, voxels);
    let t: [i32; 3] = std::array::from_fn(|k| vlo[k] + vsize[k] / 2);
    builder.root = vec![SceneNode::shape(m, None, t)];
    let bytes = builder.build().write();
    // It must open as one model with these bounds and voxels.
    let s = Scene::from_vox(&bytes)?;
    if s.bounds().lo != lo || s.bounds().size != size || s.world_voxels(false) != *cells {
        return Err(format!("{}: reads back different", path.display()));
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    println!("{}: {} voxels, bounds from {lo:?} size {size:?}", path.display(), cells.len());
    Ok(())
}

pub fn write(island: &Path, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    save(&dir.join("shapes.vox"), &shapes(), [-24, 0, -24], [48, 40, 48])?;
    save(&dir.join("farm.vox"), &farm(), [-36, 0, -36], [72, 48, 72])?;
    let bytes = std::fs::read(island).map_err(|e| format!("{}: {e}", island.display()))?;
    let mut s = Scene::from_vox(&bytes)?;
    let b = s.bounds();
    s.set_bounds(b.lo, [b.size[0], 64, b.size[2]]).ok_or("cannot grow the island")?;
    let out = s.to_vox();
    let r = Scene::from_vox(&out)?;
    println!("island-tall.vox: {} voxels, bounds from {:?} size {:?}", r.map.voxels, r.bounds().lo, r.bounds().size);
    std::fs::write(dir.join("island-tall.vox"), out).map_err(|e| e.to_string())
}
