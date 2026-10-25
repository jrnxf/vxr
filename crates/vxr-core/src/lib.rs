//! vxr core: the editable voxel scene, independent of the GPU.
//!
//! - [`brickmap`]: the sparse brick map (8^3 bricks with occupancy masks), edits,
//!   dirty tracking for partial GPU uploads, and a CPU ray cast for picking.
//! - [`history`]: undo and redo steps.
//! - [`scene`]: a brick map with its palette and history, and `.vox` open and save.
//! - [`tools`]: the editing tools (add, erase, paint, pick with the voxel, box and face
//!   brushes, and mirror), as presses, drags and releases on a scene.

pub mod brickmap;
pub mod history;
pub mod scene;
pub mod tools;

pub use brickmap::BrickMap;
pub use scene::Scene;
