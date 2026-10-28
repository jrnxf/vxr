//! vxr's editor in the browser: the same [`vxr_app::Editor`] as the desktop app, drawn on a
//! WebGPU canvas. The page (`www/index.html`) forwards DOM input here and handles the file
//! picker and downloads; everything else, the UI included, is the shared Rust code. The
//! assistant calls its provider from the page with `fetch` (`net`), keys in the tab.
//!
//! Only built for `wasm32`. `crates/vxr-web/build` makes the static site in `dist/`.

#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod web;
