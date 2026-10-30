//! A `.vox` document: the raw chunks plus edits that re-encode only what they touch.

use crate::camera::Camera;
use crate::chunk::{self, IdDict, Rgba, Typed};
use crate::default_palette::DEFAULT_PALETTE;
use crate::material::Material;
use crate::raw::{Chunk, RawFile};
use crate::scene::Scene;
use crate::{Error, Result};

/// How [`Document::set_tree`] saved the objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldSave {
    /// The tree and model edits were patched into the file's own chunks; every chunk they
    /// did not touch keeps its bytes.
    Kept,
    /// The edits could not be placed in the file's chunks (a file without a scene graph, a
    /// model over 256 on an axis), so the models and scene graph were rebuilt from the
    /// tree. Every chunk that is not a model, a scene graph node or a layer is untouched.
    Rebuilt,
}

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

    /// The material of palette index `index` (1 to 255): its `MATL` dict, or the default.
    pub fn material(&self, index: u8) -> Material {
        self.raw
            .chunks
            .iter()
            .filter(|c| c.is(chunk::MATL))
            .filter_map(|c| IdDict::parse(chunk::MATL, &c.content).ok())
            .find(|m| m.id == index as i32)
            .map_or_else(Material::default, |m| Material::from_dict(&m.dict))
    }

    /// Set the material of palette index `index` (1 to 255). Touches only that index's
    /// `MATL` chunk, and in it only the keys of the material's type; adds a chunk after the
    /// last `MATL` (or the palette) if the file has none for it.
    pub fn set_material(&mut self, index: u8, m: &Material) -> Result<usize> {
        if index == 0 {
            return Err(Error::new("palette index 0 is the empty voxel"));
        }
        for (i, c) in self.raw.chunks.iter_mut().enumerate() {
            if !c.is(chunk::MATL) {
                continue;
            }
            let mut d = IdDict::parse(chunk::MATL, &c.content)?;
            if d.id != index as i32 {
                continue;
            }
            m.apply_to(&mut d.dict);
            c.content = d.encode();
            return Ok(i);
        }
        let chunks = &self.raw.chunks;
        let at = chunks
            .iter()
            .rposition(|c| c.is(chunk::MATL))
            .or_else(|| chunks.iter().rposition(|c| c.is(chunk::RGBA) || c.is(chunk::IMAP)))
            .map_or(chunks.len(), |i| i + 1);
        let d = IdDict { id: index as i32, dict: m.to_dict(), extra: vec![] };
        self.raw.chunks.insert(at, Chunk::new(chunk::MATL, d.encode()));
        Ok(at)
    }

    /// The saved cameras, in file order. A camera id that appears twice is read from its
    /// first `rCAM`, as [`Document::camera`] and [`Document::set_camera`] see it.
    pub fn cameras(&self) -> Vec<Camera> {
        let mut out: Vec<Camera> = Vec::new();
        for d in self.raw.chunks.iter().filter(|c| c.is(chunk::RCAM)) {
            if let Ok(d) = IdDict::parse(chunk::RCAM, &d.content)
                && !out.iter().any(|c| c.id == d.id)
            {
                out.push(Camera::from_dict(d.id, &d.dict));
            }
        }
        out
    }

    /// The saved camera `id`, if the file has one.
    pub fn camera(&self, id: i32) -> Option<Camera> {
        self.raw
            .chunks
            .iter()
            .filter(|c| c.is(chunk::RCAM))
            .filter_map(|c| IdDict::parse(chunk::RCAM, &c.content).ok())
            .find(|d| d.id == id)
            .map(|d| Camera::from_dict(id, &d.dict))
    }

    /// Set saved camera `c.id`. Touches only that camera's `rCAM` chunk, and in it only the
    /// keys whose value changed; adds a chunk after the last `rCAM` (or the render settings,
    /// materials or palette) if the file has none for it.
    pub fn set_camera(&mut self, c: &Camera) -> Result<usize> {
        for (i, ch) in self.raw.chunks.iter_mut().enumerate() {
            if !ch.is(chunk::RCAM) {
                continue;
            }
            let mut d = IdDict::parse(chunk::RCAM, &ch.content)?;
            if d.id != c.id {
                continue;
            }
            c.apply_to(&mut d.dict);
            ch.content = d.encode();
            return Ok(i);
        }
        let chunks = &self.raw.chunks;
        let at = chunks
            .iter()
            .rposition(|c| c.is(chunk::RCAM))
            .or_else(|| {
                chunks.iter().rposition(|c| [chunk::ROBJ, chunk::MATL, chunk::RGBA, chunk::IMAP].contains(&&c.id))
            })
            .map_or(chunks.len(), |i| i + 1);
        let d = IdDict { id: c.id, dict: c.to_dict(), extra: vec![] };
        self.raw.chunks.insert(at, Chunk::new(chunk::RCAM, d.encode()));
        Ok(at)
    }
}
