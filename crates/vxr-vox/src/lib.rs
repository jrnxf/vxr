//! vxr `.vox` codec, lossless in two layers.
//!
//! - [`raw`]: the file as an ordered list of chunks, every byte kept. An unedited
//!   file is written back byte for byte, including chunks we do not understand.
//! - [`chunk`]: typed parse and encode for each known chunk. Every typed chunk keeps
//!   the bytes it did not consume, so `encode(parse(c)) == c` for every chunk.
//! - [`scene`]: the semantic view (models, palette, scene graph, world instances).
//! - [`doc`]: a raw file plus edits that re-encode only the chunks they touch.
//! - [`tree`]: the scene graph as an editable tree of objects and groups, with layers,
//!   written back by [`Document::set_tree`] touching only the chunks whose meaning changed.
//! - [`material`]: palette slot materials (`MATL`) as typed values.
//! - [`build`]: writes new files from scratch (fixtures, property tests).
//!
//! The reader is bounds checked everywhere and returns `Err` on malformed input.

pub mod build;
pub mod bytes;
pub mod chunk;
pub mod default_palette;
pub mod doc;
pub mod fixtures;
pub mod material;
pub mod raw;
pub mod scene;
pub mod tree;

pub use bytes::Dict;
pub use doc::{Document, WorldSave};
pub use material::{Kind, Material};
pub use raw::{Chunk, RawFile};
pub use scene::{Affine, Instance, Scene};
pub use tree::{Kind as NodeKind, LayerInfo, ModelEdits, Tree, TreeNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub msg: String,
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error { msg: msg.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Four-byte chunk id as text, for messages.
pub fn id_str(id: &[u8; 4]) -> String {
    id.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '?' }).collect()
}
