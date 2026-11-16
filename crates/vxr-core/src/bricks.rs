//! The brick scene file: a brick map and its colours as the embed loads them, about a
//! tenth of the same voxels as `.vox` once compressed. It holds what the viewer draws and
//! nothing else (no scene graph, materials, layers or cameras), so it is an export, never a
//! save: `.vox` stays the file a scene is kept in.
//!
//! Layout, every number little-endian:
//!
//! | Part | Bytes |
//! |---|---|
//! | magic `VXRB` | 4 |
//! | version, 1 | 1 |
//! | size in voxels, x y z, each 1 to 2048 | 3 x u16 |
//! | used palette slots, bit `i` of byte `i / 8` for slot `i` | 32 |
//! | RGBA of each used slot, by slot | 4 each |
//! | brick count | u32 |
//! | each brick's grid cell (x fastest, then y, then z) as the gap to the one before, LEB128 | 1 to 5 each |
//! | each brick's occupancy mask, 16 u32 words, bit `x + 8y + 64z` | 64 each |
//! | the palette slot of each occupied voxel, brick by brick in mask bit order | 1 each |
//!
//! The three brick streams stay apart because each compresses better alone. A map has one
//! encoding: bricks ascend, no brick is empty, no gap is padded, the used slots are exactly
//! the slots a voxel holds, and nothing follows the last voxel. [`decode`] refuses
//! everything else, so a file it accepts encodes back to the same bytes.

use crate::brickmap::{B, BRICK_VOXELS, BrickMap, EMPTY, MASK_WORDS};
use crate::scene::MAX_EXTENT;

pub const MAGIC: [u8; 4] = *b"VXRB";
pub const VERSION: u8 = 1;

const MASK_BYTES: usize = MASK_WORDS * 4;

/// `map` and the colours its voxels use, as a brick scene file.
pub fn encode(map: &BrickMap, palette: &[[u8; 4]; 256]) -> Vec<u8> {
    let bricks = map.live_bricks();
    let mut out = Vec::with_capacity(48 + bricks * (MASK_BYTES + 2) + map.voxels as usize);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    for s in map.size_voxels() {
        out.extend_from_slice(&(s as u16).to_le_bytes());
    }
    let mut used = [0u8; 32];
    for slot in 1..256 {
        if map.counts[slot] != 0 {
            used[slot / 8] |= 1 << (slot % 8);
        }
    }
    out.extend_from_slice(&used);
    for (rgba, &n) in palette.iter().zip(&map.counts).skip(1) {
        if n != 0 {
            out.extend_from_slice(rgba);
        }
    }
    out.extend_from_slice(&(bricks as u32).to_le_bytes());
    let mut next = 0u32;
    for (cell, &b) in map.grid.iter().enumerate() {
        if b != EMPTY {
            let mut gap = cell as u32 - next;
            next = cell as u32 + 1;
            while gap >= 0x80 {
                out.push(gap as u8 | 0x80);
                gap >>= 7;
            }
            out.push(gap as u8);
        }
    }
    for &b in map.grid.iter().filter(|&&b| b != EMPTY) {
        for w in &map.masks[b as usize * MASK_WORDS..(b as usize + 1) * MASK_WORDS] {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
    for &b in map.grid.iter().filter(|&&b| b != EMPTY) {
        let pal = &map.pal[b as usize * BRICK_VOXELS..(b as usize + 1) * BRICK_VOXELS];
        for (w, &bits) in map.masks[b as usize * MASK_WORDS..(b as usize + 1) * MASK_WORDS].iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                out.push(pal[w * 32 + bits.trailing_zeros() as usize]);
                bits &= bits - 1;
            }
        }
    }
    out
}

/// Read a brick scene file: the map, and the palette with every unused slot zero. Any
/// bytes give `Ok` or `Err`, never a panic, and memory stays within what the size on each
/// axis (at most 2048) and the file's own length allow. The message says what is wrong
/// with the file.
pub fn decode(bytes: &[u8]) -> Result<(BrickMap, [[u8; 4]; 256]), &'static str> {
    const SHORT: &str = "the brick scene file is cut short";
    let mut r = Reader { bytes, at: 0 };
    if r.take(4).ok_or("not a brick scene file")? != MAGIC {
        return Err("not a brick scene file");
    }
    if r.take(1).ok_or(SHORT)?[0] != VERSION {
        return Err("the brick scene file is a version this viewer does not read");
    }
    let mut size = [0u32; 3];
    for s in &mut size {
        let b = r.take(2).ok_or(SHORT)?;
        *s = u16::from_le_bytes([b[0], b[1]]) as u32;
        if *s == 0 || *s > MAX_EXTENT {
            return Err("the brick scene file's size is not 1 to 2048 voxels on each axis");
        }
    }
    let used = r.take(32).ok_or(SHORT)?;
    let is_used = |slot: usize| used[slot / 8] >> (slot % 8) & 1 != 0;
    if is_used(0) {
        return Err("the brick scene file gives slot 0, the empty cell, a colour");
    }
    let mut palette = [[0u8; 4]; 256];
    for (slot, rgba) in palette.iter_mut().enumerate() {
        if is_used(slot) {
            rgba.copy_from_slice(r.take(4).ok_or(SHORT)?);
        }
    }
    let count = r.take(4).ok_or(SHORT)?;
    let count = u32::from_le_bytes([count[0], count[1], count[2], count[3]]) as usize;
    let dims = size.map(|s| s.div_ceil(B));
    let cells = dims[0] as u64 * dims[1] as u64 * dims[2] as u64;
    // Every brick takes a gap byte, a mask and a voxel at least, so a count the file has no
    // room for is refused before anything is allocated for it.
    if count as u64 > cells || count as u64 * (MASK_BYTES as u64 + 2) > (bytes.len() - r.at) as u64 {
        return Err(SHORT);
    }
    let mut at = Vec::with_capacity(count);
    let mut next = 0u64;
    for _ in 0..count {
        let cell = next + r.gap()? as u64;
        if cell >= cells {
            return Err("the brick scene file has a brick outside its size");
        }
        at.push(cell as usize);
        next = cell + 1;
    }
    let masks = r.take(count * MASK_BYTES).ok_or(SHORT)?;
    let mut map = BrickMap::new(size);
    for (i, &cell) in at.iter().enumerate() {
        let mut mask = [0u32; MASK_WORDS];
        for (w, b) in mask.iter_mut().zip(masks[i * MASK_BYTES..].as_chunks::<4>().0) {
            *w = u32::from_le_bytes(*b);
        }
        let n = mask.iter().map(|w| w.count_ones() as usize).sum::<usize>();
        if n == 0 {
            return Err("the brick scene file has an empty brick");
        }
        let slots = r.take(n).ok_or(SHORT)?;
        if slots.iter().any(|&s| !is_used(s as usize)) {
            return Err("the brick scene file has a voxel in a slot with no colour");
        }
        if !map.fill_brick(cell, &mask, slots) {
            return Err("the brick scene file has a voxel outside its size");
        }
    }
    if r.at != bytes.len() {
        return Err("the brick scene file has bytes after its last voxel");
    }
    if (1..256).any(|slot| is_used(slot) && map.counts[slot] == 0) {
        return Err("the brick scene file has a colour no voxel uses");
    }
    Ok((map, palette))
}

/// What the embed shows of a file from anywhere: a brick scene file as [`decode`] reads
/// it, or a `.vox` file's shown objects in one map (hidden objects and layers left out)
/// with the file's palette. Any bytes give `Ok` or `Err`, never a panic. The map's place in
/// the world is dropped: the viewer frames what it gets.
pub fn open(bytes: &[u8]) -> Result<(BrickMap, [[u8; 4]; 256]), String> {
    if bytes.starts_with(&MAGIC) {
        return decode(bytes).map_err(String::from);
    }
    let doc = vxr_vox::Document::read(bytes).map_err(|e| e.to_string())?;
    let colors = doc.scene().map_err(|e| e.to_string())?;
    let (map, _) = crate::World::from_doc(&doc)?.shown_map()?;
    Ok((map, std::array::from_fn(|i| colors.color(i as u8))))
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }

    /// One LEB128 u32 in its shortest form.
    fn gap(&mut self) -> Result<u32, &'static str> {
        const BAD: &str = "the brick scene file has a brick position it cannot hold";
        let mut v = 0u32;
        for shift in (0..35).step_by(7) {
            let b = self.take(1).ok_or("the brick scene file is cut short")?[0];
            let bits = (b & 0x7f) as u32;
            if shift == 28 && bits > 0xf {
                return Err(BAD);
            }
            v |= bits << shift;
            if b & 0x80 == 0 {
                // A zero last byte after another byte is the same number written longer.
                return if b == 0 && shift != 0 { Err(BAD) } else { Ok(v) };
            }
        }
        Err(BAD)
    }
}
