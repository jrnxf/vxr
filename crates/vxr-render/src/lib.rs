//! vxr renderer: the brick map on the GPU, ray marched in WGSL compute.
//!
//! - [`gpu::Gpu`]: adapter and device with full limits (and timestamps when offered).
//! - [`map::GpuMap`]: the compacted brick map on the GPU (palette bytes for occupied voxels
//!   only), bind group 1 of any ray pass. `upload_dirty` after edits, `set_palette` to recolor.
//! - [`renderer::Renderer`]: the editor view. `march` traces one ray per pixel with `trace_h`
//!   into its own textures, with the ground, grid and bounds frame (`Guides`) when asked; `blit`
//!   draws the result onto any target.
//! - [`preview::Preview`]: the live path-traced preview (640x360, progressive, denoised),
//!   budgeted per frame and written into a corner of the editor image.
//! - [`camera::OrbitCamera`]: MagicaVoxel-style orbit, pan and zoom, perspective or
//!   orthographic, with snap views.
//!
//! Render space is the map's cell space: voxel (x, y, z) fills [x, x + 1) and so on, y up.

pub mod camera;
pub mod gpu;
pub mod map;
pub mod preview;
pub mod renderer;

pub use camera::{OrbitCamera, View};
pub use gpu::Gpu;
pub use map::{GpuMap, brickmap_layout};
pub use preview::{EditBox, Lighting, Preview};
pub use renderer::{Guides, Renderer};
