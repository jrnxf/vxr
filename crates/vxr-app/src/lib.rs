//! vxr's editor app: the Hybrid UI in egui over the brick map renderer.
//!
//! - [`agents`]: terminal agents editing the window through `vxr mcp --live`; `live` is the
//!   desktop app's socket for them, and [`bridge`] the browser's link to `vxr bridge`.
//! - [`assistant`]: the AI assistant's conversation and tool loop, through the review step.
//! - [`editor::Editor`]: the scene on the GPU, the camera, the UI and the editing gestures,
//!   independent of the window. The native host is `src/main.rs`.
//! - [`ui::Hybrid`]: the Hybrid UI (direction D), drawn with egui.
//! - [`color`]: hex, RGB and HSV for the palette editor.
//! - [`files`]: OBJ export files, and a zip to download several files at once.
//! - [`fonts`]: loading the UI fonts.
//! - [`host`]: what the native window and the browser page share: the surface, pointer routing
//!   and Open, Save and the exports, with a `Platform` for what differs.
//! - [`render`]: the Render panel's settings and the PNG render export.
//! - [`review`]: the review step of an AI change, and scripts that drive it.
//! - [`theme`]: the UI themes' colour tokens and the editor view's colours.

pub mod agents;
pub mod assistant;
pub mod bridge;
pub mod color;
pub mod editor;
pub mod files;
pub mod fonts;
pub mod host;
#[cfg(all(unix, not(target_arch = "wasm32")))]
pub mod live;
#[cfg(test)]
mod parity;
pub mod render;
pub mod review;
pub mod theme;
pub mod touch;
pub mod ui;

pub use editor::Editor;
