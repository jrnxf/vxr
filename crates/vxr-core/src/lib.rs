//! vxr core: the editable voxel scene, independent of the GPU.
//!
//! - [`brickmap`]: the sparse brick map (8^3 bricks with occupancy masks), edits,
//!   dirty tracking for partial GPU uploads, and a CPU ray cast for picking.
//! - [`history`]: undo and redo steps.
//! - [`scene`]: a brick map with its palette and history, and `.vox` open and save.

pub mod brickmap;
pub mod history;
pub mod scene;

pub use brickmap::BrickMap;
pub use scene::Scene;
