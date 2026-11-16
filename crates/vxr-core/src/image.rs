//! Image import, as MagicaVoxel's drag-in sprites: a PNG becomes voxels one cell thick,
//! standing upright as the front view shows the picture (x right, y up), each pixel's colour
//! matched to the nearest palette colour. Pixels under half opaque stay empty. The PNG is
//! from a stranger: its size is checked before anything is allocated.

use std::collections::HashMap;

use crate::Scene;
use crate::history::At;

/// The longest side of an image to import, in pixels (each pixel is one voxel).
pub const MAX_SIDE: u32 = 1024;

/// The most the PNG decoder may allocate: a [`MAX_SIDE`] square at 16 bits per channel.
const MAX_DECODE_BYTES: usize = (MAX_SIDE * MAX_SIDE * 8) as usize;

/// A decoded image: `pixels` row by row from the top, as RGBA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

/// What [`Scene::import_image`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Imported {
    /// Voxels the image set (pixels whose cell already held the colour are not counted).
    pub voxels: usize,
    /// The world cell of the image's bottom-left pixel.
    pub at: [i32; 3],
    /// How many palette slots the pixels matched.
    pub slots: usize,
    /// Whether the model bounds grew to hold the image.
    pub grew: bool,
}

/// Refuse a PNG whose palette chunk the decoder would trip on: `png` 0.18 panics on a
/// `PLTE` whose length is not a whole number of colours (the fuzz target `image_decode`
/// found it), so every `PLTE` is checked to be 1 to 256 colours of 3 bytes first.
pub(crate) fn check_chunks(bytes: &[u8]) -> Result<(), String> {
    let mut at = 8;
    while let Some(head) = bytes.get(at..at + 8) {
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        match &head[4..] {
            b"PLTE" if len == 0 || !len.is_multiple_of(3) || len > 768 => {
                return Err(format!("the PNG does not read (its palette is {len} bytes, not 1 to 256 colors)"));
            }
            b"IEND" => break,
            _ => {}
        }
        at = at.saturating_add(12).saturating_add(len);
    }
    Ok(())
}

/// Read a PNG image to import. Refuses a file that is not a PNG, does not read, or is over
/// [`MAX_SIDE`] on a side (from its header, before decoding).
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if !bytes.starts_with(b"\x89PNG") {
        return Err("it is not a PNG image".into());
    }
    let fits = |w: u32, h: u32| match w.max(h) {
        0 => Err("the image has no pixels".to_string()),
        s if s > MAX_SIDE => Err(format!("the image is {w}×{h} pixels: at most {MAX_SIDE} on a side")),
        _ => Ok(()),
    };
    if let (Some(b"IHDR"), Some(size)) = (bytes.get(12..16), bytes.get(16..24)) {
        let at = |i: usize| u32::from_be_bytes([size[i], size[i + 1], size[i + 2], size[i + 3]]);
        fits(at(0), at(4))?;
    }
    check_chunks(bytes)?;
    let bad = |e: png::DecodingError| format!("the PNG does not read ({e})");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder.set_limits(png::Limits { bytes: MAX_DECODE_BYTES });
    let mut reader = decoder.read_info().map_err(bad)?;
    let (width, height) = reader.info().size();
    fits(width, height)?;
    let mut px = vec![0; reader.output_buffer_size().ok_or("the PNG is too large")?];
    let info = reader.next_frame(&mut px).map_err(bad)?;
    let n = info.color_type.samples();
    let pixels: Vec<[u8; 4]> = px[..info.buffer_size()]
        .chunks_exact(n)
        .map(|p| match n {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    if pixels.len() != (width * height) as usize {
        return Err("the PNG is cut short".into());
    }
    Ok(Image { width, height, pixels })
}

/// The palette slot (1 to 255) whose colour looks nearest to `rgb` (a weighted RGB distance,
/// heavier on red where the colour is red and on blue where it is dark); the lowest slot
/// wins a tie, so an exact colour always finds its first slot.
pub fn nearest(palette: &[[u8; 4]; 256], rgb: [u8; 3]) -> u8 {
    let far = |c: &[u8; 4]| {
        let mean = (c[0] as i64 + rgb[0] as i64) / 2;
        let d: [i64; 3] = std::array::from_fn(|i| c[i] as i64 - rgb[i] as i64);
        (((512 + mean) * d[0] * d[0]) >> 8) + 4 * d[1] * d[1] + (((767 - mean) * d[2] * d[2]) >> 8)
    };
    (1..=255u8).min_by_key(|&s| far(&palette[s as usize])).expect("255 slots")
}

/// The image's voxels: pixel (x, y) at offset [x, height - 1 - y] from the bottom-left, with
/// its nearest slot. Pixels under half opaque are left out.
pub fn cells(img: &Image, palette: &[[u8; 4]; 256]) -> Vec<([i32; 2], u8)> {
    let mut slot = HashMap::new();
    let w = img.width as usize;
    img.pixels
        .iter()
        .enumerate()
        .filter(|(_, p)| p[3] >= 128)
        .map(|(i, p)| {
            let s = *slot.entry([p[0], p[1], p[2]]).or_insert_with(|| nearest(palette, [p[0], p[1], p[2]]));
            ([(i % w) as i32, (img.height as usize - 1 - i / w) as i32], s)
        })
        .collect()
}

/// Where [`Scene::place_image`] puts an image: its voxels and the bounds that hold them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The world cell of the image's bottom-left pixel.
    pub at: [i32; 3],
    /// The model bounds that hold the image (the bounds now, when they already do): low
    /// world cell and size.
    pub lo: [i32; 3],
    pub size: [u32; 3],
    /// World cells and their slots, one per opaque pixel.
    pub cells: Vec<([i32; 3], u8)>,
    /// How many palette slots the pixels matched.
    pub slots: usize,
}

impl Scene {
    /// Where `img` goes in the model being edited, changing nothing: one cell thick, upright
    /// in the x-y plane with its bottom-left pixel at world cell `at`, or by default centred
    /// across the bounds, standing on the ground, one cell in front of the nearest voxel to
    /// the front view (halfway back in an empty model, so it never hides inside the model);
    /// and the bounds grown to hold it. Refuses when the world shows or when no pixel is opaque.
    pub fn place_image(&self, img: &Image, at: Option<[i32; 3]>) -> Result<Placed, String> {
        if self.at() == At::World {
            return Err("the world shows: edit an object to import an image into it".into());
        }
        let cells = cells(img, &self.palette);
        if cells.is_empty() {
            return Err("every pixel of the image is transparent".into());
        }
        let b = self.bounds();
        let (w, h) = (img.width as i32, img.height as i32);
        let at = at.unwrap_or_else(|| {
            let mut front = None;
            self.for_each_voxel(|p, _| front = front.max(Some(p[2])));
            let z = front.map_or(b.lo[2] + b.size[2] as i32 / 2, |z| self.origin[2] + z + 1);
            [b.lo[0] + (b.size[0] as i32 - w).div_euclid(2), b.floor, z]
        });
        let hi = [at[0] + w, at[1] + h, at[2] + 1];
        let lo: [i32; 3] = std::array::from_fn(|a| b.lo[a].min(at[a]));
        let size: [u32; 3] = std::array::from_fn(|a| ((b.lo[a] + b.size[a] as i32).max(hi[a]) - lo[a]) as u32);
        let mut used: Vec<u8> = cells.iter().map(|c| c.1).collect();
        used.sort_unstable();
        used.dedup();
        let cells = cells.into_iter().map(|([x, y], s)| ([at[0] + x, at[1] + y, at[2]], s)).collect();
        Ok(Placed { at, lo, size, cells, slots: used.len() })
    }

    /// Put `img` into the model being edited where [`Scene::place_image`] says, growing the
    /// bounds when it must, as one undo step. Refuses, changing nothing, as that does, and
    /// when the grown model would pass the size limit.
    pub fn import_image(&mut self, img: &Image, at: Option<[i32; 3]>) -> Result<Imported, String> {
        let p = self.place_image(img, at)?;
        let b = self.bounds();
        let grew = p.lo != b.lo || p.size != b.size;
        self.commit();
        if grew {
            self.put_bounds(p.lo, p.size)?;
        }
        for &(q, s) in &p.cells {
            self.put(self.to_map(q), s);
        }
        Ok(Imported { voxels: self.commit(), at: p.at, slots: p.slots, grew })
    }
}
