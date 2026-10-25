//! vxr Phase 2 test 3 spike: the Hybrid UI (direction D) drawn in egui over the test 1 renderer.
//!
//! `hybrid::Hybrid` holds the UI state and draws the top bar, tool rail, palette, coach card,
//! stats, hint bar, undo cluster and Cmd-K. Sizes, colours and fonts follow the prototype's CSS
//! (`prototype/src/editor.html`), which the mockups were captured from, so the HTML/CSS build and
//! this one can be compared pixel for pixel.

pub mod hybrid;

#[cfg(not(target_arch = "wasm32"))]
pub mod offscreen;
