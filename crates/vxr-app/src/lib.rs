//! vxr's editor app: the Hybrid UI in egui over the brick map renderer.
//!
//! - [`assistant`]: the AI assistant's conversation and tool loop, through the review step.
//! - [`editor::Editor`]: the scene on the GPU, the camera, the UI and the editing gestures,
//!   independent of the window. The native host is `src/main.rs`.
//! - [`ui::Hybrid`]: the Hybrid UI (direction D), drawn with egui.
//! - [`color`]: hex, RGB and HSV for the palette editor.
//! - [`files`]: OBJ export files, and a zip to download several files at once.
//! - [`fonts`]: loading the UI fonts.
//! - [`render`]: the Render panel's settings and the PNG render export.
//! - [`review`]: the review step of an AI change, and scripts that drive it.

pub mod assistant;
pub mod color;
pub mod editor;
pub mod files;
pub mod fonts;
pub mod render;
pub mod review;
pub mod ui;

pub use editor::Editor;
