//! vxr Phase 2 test 1 spike: a sparse brick map ray marched in WGSL compute.
//!
//! Library API (also used by the test 2 preview spike):
//! - `scene::terrain(side)` / `scene::from_vox(name, bytes)` build a `Scene` (brick map + palette).
//! - `brickmap::BrickMap`: CPU bricks; `set`, `sphere` brush, `raycast`, dirty tracking.
//! - `gpu::Gpu::new`, `gpu::brickmap_layout`, `gpu::GpuBrickMap` (bind group 1 for any ray
//!   pass, `upload_dirty` after edits, `set_palette` to recolor). `GpuBrickMap::new` is the
//!   dense layout; `new_with_layout(.., MapLayout::Compact)` stores palette bytes only for
//!   occupied voxels. Shaders need no change: `palette_value` in `BRICKMAP_WGSL` reads both.
//! - `gpu::Renderer`: editor-view march into its own textures (`color`, and with
//!   `RenderOptions { gbuffer: true, .. }` also `depth` and `info`), then `blit` to any view.
//! - `gpu::BRICKMAP_WGSL`: bindings + `trace(ro, d, t_min, t_max) -> Hit` for other shaders.
//! - `camera::OrbitCamera`: MagicaVoxel-style orbit, pan and zoom.

pub mod bench;
pub mod brickmap;
pub mod camera;
mod default_palette;
pub mod gpu;
pub mod scene;
pub mod vox;

#[cfg(all(target_arch = "wasm32", feature = "bench-web"))]
mod web;
