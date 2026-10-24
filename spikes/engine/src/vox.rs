//! Minimal bounds-checked `.vox` reader for the spike: SIZE/XYZI models, RGBA palette, and
//! nTRN translations composed down the scene graph. Rotations (`_r`) are counted, not applied.

use std::collections::HashMap;

pub struct VoxModel {
    pub size: [u32; 3],
    /// x, y, z, palette index (MagicaVoxel axes, z up).
    pub voxels: Vec<[u8; 4]>,
}

pub struct VoxScene {
    pub version: u32,
    pub models: Vec<VoxModel>,
    /// RGBA8 per palette index; index 0 unused.
    pub palette: [u32; 256],
    /// (model id, world translation of the model centre).
    pub instances: Vec<(usize, [i32; 3])>,
    pub rotated_nodes: u32,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> Result<(), String> {
        if self.p.checked_add(n).is_none_or(|e| e > self.b.len()) {
            Err(format!("truncated at byte {}", self.p))
        } else {
            Ok(())
        }
    }
    fn u32(&mut self) -> Result<u32, String> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.b[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        Ok(v)
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.need(n)?;
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
    fn dict(&mut self) -> Result<HashMap<String, String>, String> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return Err("dict too large".into());
        }
        let mut m = HashMap::new();
        for _ in 0..n {
            let k = self.string()?;
            let v = self.string()?;
            m.insert(k, v);
        }
        Ok(m)
    }
}

enum Node {
    Trn { child: u32, t: [i32; 3] },
    Grp { children: Vec<u32> },
    Shp { models: Vec<u32> },
}

pub fn parse(bytes: &[u8]) -> Result<VoxScene, String> {
    let mut r = Reader { b: bytes, p: 0 };
    if r.bytes(4)? != b"VOX " {
        return Err("not a .vox file".into());
    }
    let version = r.u32()?;
    let mut models = Vec::new();
    let mut pending_size: Option<[u32; 3]> = None;
    let mut palette = [0u32; 256];
    let mut have_palette = false;
    let mut nodes: HashMap<u32, Node> = HashMap::new();
    let mut rotated = 0;

    // MAIN header, then a flat run of child chunks.
    if r.bytes(4)? != b"MAIN" {
        return Err("missing MAIN".into());
    }
    let main_content = r.u32()? as usize;
    let _main_children = r.u32()?;
    r.bytes(main_content)?;
    while r.p < bytes.len() {
        let id: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        let content = r.u32()? as usize;
        let children = r.u32()? as usize;
        let body = r.bytes(content)?;
        r.bytes(children)?;
        let mut c = Reader { b: body, p: 0 };
        match &id {
            b"SIZE" => {
                let s = [c.u32()?, c.u32()?, c.u32()?];
                if s.iter().any(|&v| v == 0 || v > 256) {
                    return Err(format!("bad model size {s:?}"));
                }
                pending_size = Some(s);
            }
            b"XYZI" => {
                let size = pending_size.take().ok_or("XYZI without SIZE")?;
                let n = c.u32()? as usize;
                let raw = c.bytes(n.checked_mul(4).ok_or("overflow")?)?;
                let mut voxels = Vec::with_capacity(n);
                for v in raw.chunks_exact(4) {
                    if (v[0] as u32) < size[0] && (v[1] as u32) < size[1] && (v[2] as u32) < size[2] && v[3] != 0 {
                        voxels.push([v[0], v[1], v[2], v[3]]);
                    }
                }
                models.push(VoxModel { size, voxels });
            }
            b"RGBA" => {
                let raw = c.bytes(1024)?;
                for i in 0..255 {
                    palette[i + 1] = u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
                }
                have_palette = true;
            }
            b"nTRN" => {
                let nid = c.u32()?;
                let _attr = c.dict()?;
                let child = c.u32()?;
                let _reserved = c.i32()?;
                let _layer = c.i32()?;
                let frames = c.u32()?;
                let mut t = [0; 3];
                for f in 0..frames.min(1024) {
                    let d = c.dict()?;
                    if f == 0 {
                        if let Some(s) = d.get("_t") {
                            let v: Vec<i32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                            if v.len() == 3 {
                                t = [v[0], v[1], v[2]];
                            }
                        }
                        if d.get("_r").is_some_and(|r| r != "4") {
                            rotated += 1;
                        }
                    }
                }
                nodes.insert(nid, Node::Trn { child, t });
            }
            b"nGRP" => {
                let nid = c.u32()?;
                let _attr = c.dict()?;
                let n = c.u32()? as usize;
                if n > 1 << 20 {
                    return Err("group too large".into());
                }
                let mut children = Vec::with_capacity(n);
                for _ in 0..n {
                    children.push(c.u32()?);
                }
                nodes.insert(nid, Node::Grp { children });
            }
            b"nSHP" => {
                let nid = c.u32()?;
                let _attr = c.dict()?;
                let n = c.u32()? as usize;
                if n > 1 << 16 {
                    return Err("shape too large".into());
                }
                let mut ms = Vec::with_capacity(n);
                for _ in 0..n {
                    ms.push(c.u32()?);
                    let _ = c.dict()?;
                }
                nodes.insert(nid, Node::Shp { models: ms });
            }
            b"TDCZ" => return Err("TDCZ (Teardown zlib) not supported by the spike reader".into()),
            _ => {}
        }
    }
    if !have_palette {
        palette = crate::default_palette::DEFAULT_PALETTE;
    }

    let mut instances = Vec::new();
    if nodes.is_empty() {
        for i in 0..models.len() {
            instances.push((i, [0, 0, 0]));
        }
    } else {
        // Walk from node 0, composing translations. Depth-limited against cycles.
        let mut stack = vec![(0u32, [0i32; 3], 0u32)];
        while let Some((id, acc, depth)) = stack.pop() {
            if depth > 64 {
                return Err("scene graph too deep or cyclic".into());
            }
            match nodes.get(&id) {
                Some(Node::Trn { child, t }) => {
                    stack.push((*child, [acc[0] + t[0], acc[1] + t[1], acc[2] + t[2]], depth + 1))
                }
                Some(Node::Grp { children }) => {
                    for &ch in children {
                        stack.push((ch, acc, depth + 1));
                    }
                }
                Some(Node::Shp { models: ms }) => {
                    for &m in ms {
                        if (m as usize) < models.len() {
                            instances.push((m as usize, acc));
                        }
                    }
                }
                None => {}
            }
        }
    }
    Ok(VoxScene { version, models, palette, instances, rotated_nodes: rotated })
}

impl VoxScene {
    pub fn voxel_count(&self) -> u64 {
        self.instances.iter().map(|(m, _)| self.models[*m].voxels.len() as u64).sum()
    }

    /// World voxels in engine axes (y up): (x, y, z, palette index).
    pub fn world_voxels(&self, mut f: impl FnMut(i32, i32, i32, u8)) {
        for (m, t) in &self.instances {
            let model = &self.models[*m];
            let half = [(model.size[0] / 2) as i32, (model.size[1] / 2) as i32, (model.size[2] / 2) as i32];
            for v in &model.voxels {
                let x = t[0] + v[0] as i32 - half[0];
                let y = t[1] + v[1] as i32 - half[1];
                let z = t[2] + v[2] as i32 - half[2];
                // MagicaVoxel z-up right-handed to y-up right-handed: (x, z, -y).
                f(x, z, -y, v[3]);
            }
        }
    }
}
