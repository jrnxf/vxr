//! A `.vox` document: the raw chunks plus edits that re-encode only what they touch.

use crate::chunk::{self, Ntrn, Rgba, Tdcz, Typed, Xyzi};
use crate::default_palette::DEFAULT_PALETTE;
use crate::raw::{Chunk, RawFile};
use crate::scene::Scene;
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Document {
    pub raw: RawFile,
}

impl Document {
    /// Parse the raw layer and check that every known chunk parses semantically.
    pub fn read(bytes: &[u8]) -> Result<Document> {
        let raw = RawFile::parse(bytes)?;
        Scene::from_raw(&raw)?;
        Ok(Document { raw })
    }

    pub fn write(&self) -> Vec<u8> {
        self.raw.write()
    }

    pub fn scene(&self) -> Result<Scene> {
        Scene::from_raw(&self.raw)
    }

    /// Set (or with colour 0, clear) one voxel of model `model`. Touches only that
    /// model's XYZI (or TDCZ) chunk.
    pub fn set_voxel(&mut self, model: usize, p: [u8; 3], color: u8) -> Result<usize> {
        let scene = self.scene()?;
        let m = scene.models.get(model).ok_or_else(|| Error::new(format!("no model {model}")))?;
        if (0..3).any(|k| p[k] as i32 >= m.size[k]) {
            return Err(Error::new(format!("voxel {p:?} outside model size {:?}", m.size)));
        }
        let idx = m.data_chunk;
        let c = &mut self.raw.chunks[idx];
        match Typed::parse(c)? {
            Typed::Xyzi(mut x) => {
                match (x.voxels.iter().rposition(|v| v[..3] == p), color) {
                    (Some(_), 0) => x.voxels.retain(|v| v[..3] != p),
                    (Some(i), col) => x.voxels[i][3] = col,
                    (None, 0) => {}
                    (None, col) => x.voxels.push([p[0], p[1], p[2], col]),
                }
                c.content = Xyzi::encode(&x);
            }
            Typed::Tdcz(t) => {
                let mut dense = t.decode()?;
                let (sx, sy) = (t.x as usize, t.y as usize);
                dense[p[0] as usize + sx * (p[1] as usize + sy * p[2] as usize)] = color;
                c.content = Tdcz::encode_dense(t.x, t.y, t.z, &dense).encode();
            }
            _ => unreachable!("model data chunk is XYZI or TDCZ"),
        }
        Ok(idx)
    }

    /// Recolour palette index `index` (1 to 255). Touches only the RGBA chunk; adds one
    /// (from the default palette) after the last model if the file has none.
    pub fn set_palette(&mut self, index: u8, rgba: [u8; 4]) -> Result<usize> {
        if index == 0 {
            return Err(Error::new("palette index 0 is the empty voxel"));
        }
        let slot = index as usize - 1;
        if let Some(i) = self.raw.chunks.iter().position(|c| c.is(chunk::RGBA)) {
            let Typed::Rgba(mut p) = Typed::parse(&self.raw.chunks[i])? else { unreachable!() };
            if slot >= p.colors.len() {
                return Err(Error::new(format!("RGBA chunk has only {} entries", p.colors.len())));
            }
            p.colors[slot] = rgba;
            self.raw.chunks[i].content = p.encode();
            return Ok(i);
        }
        let mut colors = DEFAULT_PALETTE.to_vec();
        colors[slot] = rgba;
        let at = self
            .raw
            .chunks
            .iter()
            .rposition(|c| c.is(chunk::XYZI) || c.is(chunk::TDCZ))
            .map(|i| i + 1)
            .unwrap_or(self.raw.chunks.len());
        self.raw.chunks.insert(at, Chunk::new(chunk::RGBA, Rgba { colors, extra: vec![] }.encode()));
        Ok(at)
    }

    /// Move transform node `id` to translation `t`: frame 0's `_t`, and `_frm_t` too
    /// if the node has a base transform. Touches only that nTRN chunk.
    pub fn set_translation(&mut self, id: i32, t: [i32; 3]) -> Result<usize> {
        let i = self
            .raw
            .chunks
            .iter()
            .position(|c| c.is(chunk::NTRN) && Ntrn::parse(&c.content).map(|n| n.id == id).unwrap_or(false))
            .ok_or_else(|| Error::new(format!("no nTRN with id {id}")))?;
        let mut n = Ntrn::parse(&self.raw.chunks[i].content)?;
        let v = format!("{} {} {}", t[0], t[1], t[2]);
        if n.frames.is_empty() {
            n.frames.push(Default::default());
        }
        n.frames[0].set("_t", v.clone());
        if n.attrs.has("_frm_t") {
            n.attrs.set("_frm_t", v);
        }
        self.raw.chunks[i].content = n.encode();
        Ok(i)
    }
}
