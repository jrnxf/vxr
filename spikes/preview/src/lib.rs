//! vxr Phase 2 test 2 spike: a live progressive path-traced preview in a corner of the editor
//! view, on top of the test 1 brick map and ray marcher (`vxr-engine-spike`).
//!
//! - `preview::Preview`: the tracer (reset, sliced trace dispatches, A-trous denoise, resolve,
//!   corner composite).
//! - `bench::Session`: the measurements, shared by the native binary and the browser page.

pub mod bench;
pub mod preview;

#[cfg(target_arch = "wasm32")]
mod web;
