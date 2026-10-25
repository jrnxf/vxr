//! vxr core: the editable voxel scene, independent of the GPU.
//!
//! - [`brickmap`]: the sparse brick map (8^3 bricks with occupancy masks), edits,
//!   dirty tracking for partial GPU uploads, and a CPU ray cast for picking.
//! - [`history`]: undo and redo steps.
//! - [`scene`]: a brick map with its palette and history, and `.vox` open and save.
//! - [`tools`]: the editing tools (add, erase, paint, pick with the voxel, box, face, line
//!   and center brushes, mirror on three axes and axis lock), as presses, drags and releases.

pub mod brickmap;
pub mod history;
pub mod scene;
pub mod tools;

pub use brickmap::BrickMap;
pub use scene::Scene;
/// Palette slot materials, from the `.vox` codec.
pub use vxr_vox::material;
