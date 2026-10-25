//! vxr core: the editable voxel scene, independent of the GPU.
//!
//! - [`brickmap`]: the sparse brick map (8^3 bricks with occupancy masks), edits,
//!   dirty tracking for partial GPU uploads, and a CPU ray cast for picking.
//! - [`export`]: OBJ export, greedy quads textured by a 256x1 palette image.
//! - [`history`]: undo and redo steps.
//! - [`scene`]: a brick map with its palette and history, and `.vox` open and save.
//! - [`model`]: hollow and fill, of the selection or the whole model, and resize and crop
//!   of the model bounds.
//! - [`select`]: the selection (box and colour), and move, copy, paste, delete, fill,
//!   rotate, flip and loop of the selection or the whole model.
//! - [`world`]: several objects per scene (objects, groups, linked copies, layers), read
//!   from and saved losslessly into `.vox`.
//! - [`tools`]: the editing tools (add, erase, paint, pick with the voxel, box, face, line
//!   and center brushes, mirror on three axes and axis lock), as presses, drags and releases.

pub mod brickmap;
pub mod export;
pub mod history;
pub mod model;
pub mod scene;
pub mod select;
pub mod tools;
pub mod world;

pub use brickmap::BrickMap;
pub use scene::Scene;
/// Palette slot materials, from the `.vox` codec.
pub use vxr_vox::material;
pub use world::World;
