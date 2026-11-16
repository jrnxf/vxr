//! vxr's embed viewer: what the `<voxel-scene>` element (`web/voxel-scene.js`) loads when
//! it first comes on screen. It reads a brick scene file or a `.vox` file through
//! [`vxr_core::bricks::open`], uploads it as the editor does ([`vxr_render::GpuMap`]) and
//! ray marches it with the editor's `trace_h` (`brickmap.wgsl`), with a sun, a hard shadow
//! and a sky. WebGPU only: with no WebGPU the element keeps its poster.
//!
//! Only built for `wasm32`. `crates/vxr-embed/build` makes the files to serve in `dist/`
//! and fails when they pass the size budget.

#[cfg(target_arch = "wasm32")]
mod viewer;
