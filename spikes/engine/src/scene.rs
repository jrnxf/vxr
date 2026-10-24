//! Scene sources: generated terrain (solid fill, ported from the prototype) and `.vox` files.

use crate::brickmap::BrickMap;
use crate::vox;

pub struct Scene {
    pub name: String,
    pub map: BrickMap,
    /// RGBA8 (little endian, r in the low byte) per palette index; index 0 unused.
    pub palette: [u32; 256],
    /// How the scene was built, for the results.
    pub notes: Vec<String>,
}

fn hash2(x: i32, z: i32, seed: i32) -> f32 {
    let mut h = x.wrapping_mul(374761393).wrapping_add(z.wrapping_mul(668265263)).wrapping_add(seed.wrapping_mul(2147483647));
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1274126177);
    ((h ^ ((h as u32) >> 16) as i32) as u32) as f32 / 4294967296.0
}

fn value_noise(x: f32, z: f32, seed: i32) -> f32 {
    let (xi, zi) = (x.floor(), z.floor());
    let (xf, zf) = (x - xi, z - zi);
    let (xi, zi) = (xi as i32, zi as i32);
    let u = xf * xf * (3.0 - 2.0 * xf);
    let w = zf * zf * (3.0 - 2.0 * zf);
    let a = hash2(xi, zi, seed);
    let b = hash2(xi + 1, zi, seed);
    let c = hash2(xi, zi + 1, seed);
    let d = hash2(xi + 1, zi + 1, seed);
    a + (b - a) * u + (c - a) * w + (a - b - c + d) * u * w
}

fn fbm(x: f32, z: f32, seed: i32, oct: i32) -> f32 {
    let (mut s, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for i in 0..oct {
        s += amp * value_noise(x * f, z * f, seed + i * 17);
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    s / norm
}

// Terrain palette slots.
const STONE_D: u8 = 1;
const STONE: u8 = 2;
const STONE_L: u8 = 3;
const ROCK_L: u8 = 4;
const DIRT: u8 = 5;
const SAND: u8 = 6;
const SAND_D: u8 = 7;
const GRASS_D: u8 = 8;
const GRASS: u8 = 9;
const GRASS_L: u8 = 10;
const SNOW: u8 = 11;
const WATER: u8 = 12;
const WATER_D: u8 = 13;
const SHORE: u8 = 14;

fn rgba(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xff << 24
}

pub fn terrain_palette() -> [u32; 256] {
    let mut p = [rgba(255, 0, 255); 256];
    p[0] = 0;
    p[STONE_D as usize] = rgba(96, 96, 104);
    p[STONE as usize] = rgba(122, 122, 130);
    p[STONE_L as usize] = rgba(150, 148, 150);
    p[ROCK_L as usize] = rgba(140, 128, 112);
    p[DIRT as usize] = rgba(120, 86, 58);
    p[SAND as usize] = rgba(222, 204, 150);
    p[SAND_D as usize] = rgba(196, 176, 120);
    p[GRASS_D as usize] = rgba(70, 120, 56);
    p[GRASS as usize] = rgba(96, 154, 66);
    p[GRASS_L as usize] = rgba(124, 178, 82);
    p[SNOW as usize] = rgba(240, 244, 250);
    p[WATER as usize] = rgba(64, 128, 196);
    p[WATER_D as usize] = rgba(40, 92, 160);
    p[SHORE as usize] = rgba(110, 170, 210);
    // Brush colours used by the edit tests.
    p[200] = rgba(230, 70, 60);
    p[201] = rgba(250, 200, 60);
    p
}

/// Prototype terrain heights for a `side` x `side` footprint.
pub fn terrain_height(x: i32, z: i32) -> i32 {
    let (xf, zf) = (x as f32, z as f32);
    let n = ((fbm(xf / 110.0, zf / 110.0, 101, 5) - 0.28) / 0.44).clamp(0.0, 1.0);
    let ridge = 1.0 - (fbm(xf / 70.0, zf / 70.0, 202, 4) * 2.0 - 1.0).abs();
    let mount = ((n - 0.45).max(0.0) / 0.55).powf(1.5);
    (3.0 + n * 16.0 + mount * (30.0 + ridge * ridge * 40.0)).floor() as i32
}

pub const SEA: i32 = 10;

/// Count the voxels a footprint would produce without building it (solid fill up to
/// max(height, sea level)).
pub fn terrain_count(side: u32) -> u64 {
    let mut n = 0u64;
    for z in 0..side as i32 {
        for x in 0..side as i32 {
            n += (terrain_height(x, z).max(SEA) + 1) as u64;
        }
    }
    n
}

/// Footprint side giving close to `target` voxels (mean column is about 38 tall).
pub fn terrain_side_for(target: u64) -> u32 {
    let mut side = ((target as f64 / 38.0).sqrt() as u32).max(8);
    for _ in 0..3 {
        let n = terrain_count(side);
        side = ((side as f64) * (target as f64 / n as f64).sqrt()).round() as u32;
    }
    side
}

/// Solid-fill terrain: every voxel from y = 0 up to the surface (or sea level) is set.
pub fn terrain(side: u32) -> Scene {
    let mut max_h = SEA;
    for z in (0..side as i32).step_by(4) {
        for x in (0..side as i32).step_by(4) {
            max_h = max_h.max(terrain_height(x, z));
        }
    }
    let mut map = BrickMap::new([side, (max_h + 16) as u32, side]);
    for z in 0..side as i32 {
        for x in 0..side as i32 {
            let h = terrain_height(x, z);
            let top = h.max(SEA);
            for y in 0..=top {
                let c = if y > h {
                    if y == SEA {
                        if SEA - h <= 1 {
                            SHORE
                        } else if SEA - h <= 3 {
                            WATER
                        } else {
                            WATER_D
                        }
                    } else {
                        WATER_D
                    }
                } else if y == h {
                    if h <= SEA + 1 {
                        SAND
                    } else if h > 52 {
                        SNOW
                    } else if h > 34 {
                        if hash2(x, z, 8) < 0.3 { STONE_L } else { ROCK_L }
                    } else if h > 26 {
                        GRASS_D
                    } else if hash2(x, z, 9) < 0.12 {
                        GRASS_L
                    } else {
                        GRASS
                    }
                } else if y > h - 3 && h <= 34 {
                    if h <= SEA + 1 { SAND_D } else { DIRT }
                } else if hash2(x, y * 31 + z, 2) < 0.5 {
                    STONE
                } else {
                    STONE_D
                };
                map.set(x, y, z, c);
            }
        }
    }
    Scene {
        name: format!("terrain {side}x{side}"),
        map,
        palette: terrain_palette(),
        notes: vec![format!("solid fill, footprint {side}x{side}, max height {}", max_h)],
    }
}

pub fn from_vox(name: &str, bytes: &[u8]) -> Result<Scene, String> {
    let v = vox::parse(bytes)?;
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    v.world_voxels(|x, y, z, _| {
        lo = [lo[0].min(x), lo[1].min(y), lo[2].min(z)];
        hi = [hi[0].max(x), hi[1].max(y), hi[2].max(z)];
    });
    if lo[0] == i32::MAX {
        return Err("no voxels".into());
    }
    let size = [(hi[0] - lo[0] + 1) as u32, (hi[1] - lo[1] + 1) as u32, (hi[2] - lo[2] + 1) as u32];
    if size.iter().any(|&s| s > 16384) {
        return Err(format!("scene too large {size:?}"));
    }
    let mut map = BrickMap::new(size);
    v.world_voxels(|x, y, z, c| {
        map.set(x - lo[0], y - lo[1], z - lo[2], c);
    });
    let mut notes = vec![format!(
        "vox v{}, {} models, {} instances, size {}x{}x{}",
        v.version,
        v.models.len(),
        v.instances.len(),
        size[0],
        size[1],
        size[2]
    )];
    if v.rotated_nodes > 0 {
        notes.push(format!("{} rotated nodes drawn unrotated (spike reader)", v.rotated_nodes));
    }
    Ok(Scene { name: name.to_string(), map, palette: v.palette, notes })
}
