//! Minimal bounds-checked `.vox` reader for the embed viewer spike.
//! Reads SIZE, XYZI, RGBA and the scene graph (nTRN translations only, no rotations)
//! and writes voxels straight into a brick map. Every read is checked; bad input
//! returns an error code, never panics.

pub const E_HEADER: u32 = 10;
pub const E_TRUNCATED: u32 = 11;
pub const E_SIZE: u32 = 12;
pub const E_TOO_BIG: u32 = 13;
pub const E_EMPTY: u32 = 14;

/// Largest scene the embed accepts, in voxels per axis.
const MAX_AXIS: i32 = 2048;
const MAX_NODES: usize = 1 << 16;
const MAX_VISITS: u32 = 1 << 16;

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn i32(&mut self) -> Result<i32, u32> {
        let s = self.b.get(self.p..self.p + 4).ok_or(E_TRUNCATED)?;
        self.p += 4;
        Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn bytes(&mut self, n: i32) -> Result<&'a [u8], u32> {
        if n < 0 {
            return Err(E_TRUNCATED);
        }
        let s = self.b.get(self.p..self.p + n as usize).ok_or(E_TRUNCATED)?;
        self.p += n as usize;
        Ok(s)
    }
    /// Reads a DICT and returns the value of `_t` if present.
    fn dict_t(&mut self) -> Result<Option<[i32; 3]>, u32> {
        let n = self.i32()?;
        if n < 0 || n > 4096 {
            return Err(E_TRUNCATED);
        }
        let mut t = None;
        for _ in 0..n {
            let kl = self.i32()?;
            let k = self.bytes(kl)?;
            let vl = self.i32()?;
            let v = self.bytes(vl)?;
            if k == b"_t" {
                t = Some(parse3(v));
            }
        }
        Ok(t)
    }
}

/// Parses "x y z" without core::fmt or str::parse.
fn parse3(v: &[u8]) -> [i32; 3] {
    let mut out = [0i32; 3];
    let mut i = 0;
    let mut cur: i32 = 0;
    let mut neg = false;
    let mut have = false;
    for &c in v.iter().chain(core::iter::once(&b' ')) {
        match c {
            b'-' => neg = true,
            b'0'..=b'9' => {
                cur = cur.saturating_mul(10).saturating_add((c - b'0') as i32);
                have = true;
            }
            _ => {
                if have && i < 3 {
                    out[i] = if neg { -cur } else { cur };
                    i += 1;
                }
                cur = 0;
                neg = false;
                have = false;
            }
        }
    }
    out
}

enum Node {
    T { child: i32, t: [i32; 3] },
    G { children: (usize, usize) },
    S { models: (usize, usize) },
}

struct Model {
    size: [i32; 3],
    xyzi: (usize, usize),
}

pub struct Scene {
    pub dims: [u32; 3],
    /// Scene extent in voxels.
    pub ext: [u32; 3],
    pub grid: Vec<u32>,
    pub masks: Vec<u32>,
    pub colors: Vec<u32>,
    pub palette: [u32; 256],
    pub voxels: u32,
}

struct Placed {
    model: usize,
    t: [i32; 3],
}

pub fn read(b: &[u8]) -> Result<Scene, u32> {
    if b.len() < 20 || &b[0..4] != b"VOX " || &b[8..12] != b"MAIN" {
        return Err(E_HEADER);
    }
    let mut r = Rd { b, p: 12 };
    let main_n = r.i32()?;
    let _main_m = r.i32()?;
    r.bytes(main_n)?;

    let mut size: Option<[i32; 3]> = None;
    let mut models: Vec<Model> = Vec::new();
    let mut nodes: Vec<(i32, Node)> = Vec::new();
    let mut ids: Vec<i32> = Vec::new(); // flat storage for group children and shape model ids
    let mut palette = [0u32; 256];
    let mut have_palette = false;

    while r.p + 12 <= b.len() {
        let id = r.bytes(4)?;
        let n = r.i32()?;
        let m = r.i32()?;
        let body = r.bytes(n)?;
        r.bytes(m)?;
        let mut c = Rd { b: body, p: 0 };
        match id {
            b"SIZE" => {
                let s = [c.i32()?, c.i32()?, c.i32()?];
                if s.iter().any(|&v| v < 1 || v > 256) {
                    return Err(E_SIZE);
                }
                size = Some(s);
            }
            b"XYZI" => {
                let s = size.take().ok_or(E_SIZE)?;
                let count = c.i32()?;
                if count < 0 || count as usize > (body.len() - 4) / 4 {
                    return Err(E_TRUNCATED);
                }
                // Offset of this chunk body in the file, so voxels are read in place later.
                let off = body.as_ptr() as usize - b.as_ptr() as usize;
                models.push(Model { size: s, xyzi: (off + 4, off + 4 + count as usize * 4) });
            }
            b"RGBA" => {
                let p = c.bytes(1024)?;
                for i in 0..255 {
                    palette[i + 1] = u32::from_le_bytes([p[i * 4], p[i * 4 + 1], p[i * 4 + 2], p[i * 4 + 3]]);
                }
                have_palette = true;
            }
            b"nTRN" => {
                let nid = c.i32()?;
                c.dict_t()?;
                let child = c.i32()?;
                c.i32()?; // reserved
                c.i32()?; // layer
                let frames = c.i32()?;
                let mut t = [0; 3];
                for f in 0..frames.clamp(0, 4096) {
                    if let Some(v) = c.dict_t()? {
                        if f == 0 {
                            t = v;
                        }
                    }
                }
                nodes.push((nid, Node::T { child, t }));
            }
            b"nGRP" => {
                let nid = c.i32()?;
                c.dict_t()?;
                let k = c.i32()?;
                if k < 0 || k as usize > body.len() / 4 {
                    return Err(E_TRUNCATED);
                }
                let start = ids.len();
                for _ in 0..k {
                    ids.push(c.i32()?);
                }
                nodes.push((nid, Node::G { children: (start, ids.len()) }));
            }
            b"nSHP" => {
                let nid = c.i32()?;
                c.dict_t()?;
                let k = c.i32()?;
                if k < 0 || k as usize > body.len() / 8 {
                    return Err(E_TRUNCATED);
                }
                let start = ids.len();
                for _ in 0..k {
                    ids.push(c.i32()?);
                    c.dict_t()?;
                }
                nodes.push((nid, Node::S { models: (start, ids.len()) }));
            }
            _ => {}
        }
        if nodes.len() > MAX_NODES {
            return Err(E_TOO_BIG);
        }
    }
    if models.is_empty() {
        return Err(E_EMPTY);
    }

    // Place models by composing translations from node 0 down.
    let mut placed: Vec<Placed> = Vec::new();
    if nodes.iter().any(|(id, _)| *id == 0) {
        let mut stack: Vec<(i32, [i32; 3], u32)> = Vec::new();
        stack.push((0, [0; 3], 0));
        let mut visits = 0;
        while let Some((nid, o, depth)) = stack.pop() {
            visits += 1;
            if visits > MAX_VISITS || depth > 64 {
                return Err(E_TOO_BIG);
            }
            let Some((_, node)) = nodes.iter().find(|(id, _)| *id == nid) else { continue };
            match node {
                Node::T { child, t } => {
                    let o2 = [o[0].saturating_add(t[0]), o[1].saturating_add(t[1]), o[2].saturating_add(t[2])];
                    stack.push((*child, o2, depth + 1));
                }
                Node::G { children } => {
                    for &c in &ids[children.0..children.1] {
                        stack.push((c, o, depth + 1));
                    }
                }
                Node::S { models: ms } => {
                    for &mi in &ids[ms.0..ms.1] {
                        if mi >= 0 && (mi as usize) < models.len() {
                            placed.push(Placed { model: mi as usize, t: o });
                        }
                    }
                }
            }
        }
    } else {
        for i in 0..models.len() {
            let s = models[i].size;
            placed.push(Placed { model: i, t: [s[0] / 2, s[1] / 2, s[2] / 2] });
        }
    }
    if placed.is_empty() {
        return Err(E_EMPTY);
    }

    // World bounds. A model's corner is t - floor(size / 2).
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for p in &placed {
        let s = models[p.model].size;
        for a in 0..3 {
            let c = p.t[a].saturating_sub(s[a] / 2);
            lo[a] = lo[a].min(c);
            hi[a] = hi[a].max(c.saturating_add(s[a]));
        }
    }
    let mut dims = [0u32; 3];
    let mut ext3 = [0u32; 3];
    for a in 0..3 {
        let ext = hi[a].saturating_sub(lo[a]);
        if ext <= 0 || ext > MAX_AXIS {
            return Err(E_TOO_BIG);
        }
        dims[a] = ((ext + 7) / 8) as u32;
        ext3[a] = ext as u32;
    }

    let cells = dims[0] as usize * dims[1] as usize * dims[2] as usize;
    let mut s = Scene {
        dims,
        ext: ext3,
        grid: vec![0u32; cells],
        masks: Vec::new(),
        colors: Vec::new(),
        palette,
        voxels: 0,
    };
    if !have_palette {
        // Neutral ramp. The real viewer needs MagicaVoxel's default palette here (1 KB).
        for i in 1..256u32 {
            let g = i;
            s.palette[i as usize] = 0xff00_0000 | g << 16 | g << 8 | g;
        }
    }

    for p in &placed {
        let m = &models[p.model];
        let base = [
            p.t[0] - m.size[0] / 2 - lo[0],
            p.t[1] - m.size[1] / 2 - lo[1],
            p.t[2] - m.size[2] / 2 - lo[2],
        ];
        let data = &b[m.xyzi.0..m.xyzi.1];
        for v in data.chunks_exact(4) {
            if v[3] == 0 || v[0] as i32 >= m.size[0] || v[1] as i32 >= m.size[1] || v[2] as i32 >= m.size[2] {
                continue;
            }
            let w = [base[0] + v[0] as i32, base[1] + v[1] as i32, base[2] + v[2] as i32];
            s.set(w[0] as u32, w[1] as u32, w[2] as u32, v[3]);
        }
    }
    Ok(s)
}

impl Scene {
    fn set(&mut self, x: u32, y: u32, z: u32, ci: u8) {
        let d = self.dims;
        let cell = ((x >> 3) + d[0] * ((y >> 3) + d[1] * (z >> 3))) as usize;
        let Some(slot) = self.grid.get_mut(cell) else { return };
        if *slot == 0 {
            *slot = (self.masks.len() / 16) as u32 + 1;
            self.masks.resize(self.masks.len() + 16, 0);
            self.colors.resize(self.colors.len() + 128, 0);
        }
        let bi = (*slot - 1) as usize;
        let l = ((x & 7) + 8 * ((y & 7) + 8 * (z & 7))) as usize;
        let mw = &mut self.masks[bi * 16 + (l >> 5)];
        if *mw & (1 << (l & 31)) == 0 {
            self.voxels += 1;
        }
        *mw |= 1 << (l & 31);
        let cw = &mut self.colors[bi * 128 + (l >> 2)];
        let sh = (l & 3) * 8;
        *cw = (*cw & !(0xff << sh)) | (ci as u32) << sh;
    }
}
