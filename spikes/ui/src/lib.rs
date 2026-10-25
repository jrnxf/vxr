//! vxr Phase 2 test 3 spike: the Hybrid UI (direction D) over the test 1 renderer, twice.
//!
//! `hybrid::Hybrid` (feature `egui`) holds the UI state and draws the top bar, tool rail,
//! palette, coach card, stats, hint bar, undo cluster and Cmd-K in egui. `web::Viewer` (wasm)
//! is the canvas viewer the HTML/CSS build (`web/html.html`) draws its DOM UI over. Sizes,
//! colours and fonts follow the prototype's CSS (`prototype/src/editor.html`), which the mockups
//! were captured from, so the two builds can be compared pixel for pixel.

#[cfg(feature = "egui")]
pub mod hybrid;

#[cfg(all(feature = "egui", not(target_arch = "wasm32")))]
pub mod offscreen;

#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(all(feature = "egui", target_arch = "wasm32"))]
pub mod web_egui;
