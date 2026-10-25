//! vxr's editor app: the Hybrid UI in egui over the brick map renderer.
//!
//! - [`editor::Editor`]: the scene on the GPU, the camera, the UI and the editing gestures,
//!   independent of the window. The native host is `src/main.rs`.
//! - [`ui::Hybrid`]: the Hybrid UI (direction D), drawn with egui.
//! - [`fonts`]: loading the UI fonts.

pub mod editor;
pub mod fonts;
pub mod ui;

pub use editor::Editor;
