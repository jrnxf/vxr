# Plan: research and risk tests

What to do first on vxr, in order. Read `docs/decisions.md` before this; it holds what is already settled.

## What we already know

- **Product:** a voxel scene editor with MagicaVoxel's feel and none of its limits, plus a one-tag web embed. Desktop first; the browser build comes from the same code.
- **Stack direction:** Rust core on wgpu (Metal, Vulkan, DX12, WebGPU). UI drawn in Rust so the same Hybrid UI runs native and in the browser. No server.
- **UI:** Hybrid. Floating toolbar (add, erase, paint, pick; voxel, box, face shapes), palette always on screen, scene tree only when a second object exists, live path-traced preview in a corner, Cmd-K, MagicaVoxel camera.
- **From the throwaway prototype (`prototype/`, JS + three.js):**
  - Sparse 32^3 chunks in a map, greedy meshing with per-vertex ambient occlusion, only dirty chunks remeshed on a frame budget. A single edit remeshes 1 to 4 chunks in about 1 to 7 ms.
  - Meshing 145k voxels takes about 0.11 s; 3.9M voxels about 0.39 s, single-threaded JS.
  - Palette recolor through a palette texture read in the vertex shader is instant, with no remesh.
  - Its own `.vox` files round-trip with 0 mismatches; scenes over 256^3 split into 256^3 models. It ignores rotations (`_r`), `MATL` materials and layers, and was never checked against MagicaVoxel itself.
  - FPS was never measured on a real GPU (it was built on a machine without one).
- **Browser vs desktop:** rendering through WebGPU runs near native speed. The browser falls behind on CPU-heavy work (wasm is roughly 1.5 to 2x slower, threads are harder) and on memory (wasm32 caps at 4 GB; GPU storage buffers default to 256 MiB per binding). Normal scenes should not feel laggy in either.

## Phase 1: desk research (no GPU needed)

Answer each with sources. Write findings to `docs/research/<topic>.md`, one file per topic, and a short summary at the top of each.

1. **Engine and data structure.** Compare sparse voxel storage and rendering for 10M to 1B voxels: chunked greedy meshes, sparse brick maps, SVO, SVDAG, ray-marched bricks. Use real numbers from papers, talks and engines (Teardown, Douglas Dwyer's work, Rust voxel engines). Pick one for vxr and say why.
2. **wgpu and WebGPU limits.** Buffer and binding limits, wasm32 memory, threads in wasm (SharedArrayBuffer needs cross-origin isolation: what that means for a one-tag embed on someone else's page), and current WebGPU status in Safari, Firefox and on Linux. How do existing wgpu apps cope?
3. **Live path-traced preview.** Progressive voxel path tracing on GPU: DDA ray marching, temporal accumulation, denoisers. Expected cost per sample on Apple Silicon from published numbers. How to keep editing responsive (time budget, lower-res accumulation, separate queue).
4. **UI layer.** egui against iced, Slint, Dioxus, Bevy UI, and a web UI over a wgpu canvas (Tauri-style). Judge text quality, IME, accessibility, theming, wasm size, and real shipped apps. Hybrid needs polish, not a debug look.
5. **`.vox` format.** Full spec plus undocumented corners: `MATL`, `nTRN` rotations and frames, layers, cameras, palette index maps (`IMAP`), multi-model scenes. Rust crates (`dot_vox` and others): coverage, maintenance, licence, lossless round-trip or not. Find a public corpus of real `.vox` files for testing.
6. **Embed weight.** wasm bundle sizes of comparable wgpu apps (triangle, egui demo, small engines), shrinking tricks (`wasm-opt`, LTO, `panic = "abort"`, trimmed features), and time-to-first-frame data.
7. **Native vs browser speed.** Published benchmarks of wasm vs native for CPU-heavy work, and WebGPU vs native wgpu for GPU-bound rendering.
8. **Recommendation.** Stack picks, the riskiest unknowns left, and the Phase 2 tests below rewritten with exact metrics and pass/fail thresholds.

## Phase 2: risk tests on the Mac (real GPU)

Throwaway spikes under `spikes/<name>/`. Each ends with a short `RESULTS.md`: what was measured, numbers, pass or fail, and what it changes.

1. **Engine:** a sparse renderer built native and for the browser. Measure FPS, memory and edit latency at 10M and 100M voxels. Tells us how big the native advantage really is.
2. **Live render preview:** progressive path tracing in a corner view while editing. Measure the hit to editing frame rate. The riskiest piece of Hybrid.
3. **UI layer:** the Phase 1 pick against the runner-up, building the Hybrid toolbar and palette in both. Judge feel, text and wasm size.
4. **`.vox` round-trip:** open and re-save the public corpus; prove nothing changes, including rotations, materials and layers. Check results open correctly in MagicaVoxel.
5. **Embed weight:** wasm download size and time to first frame on a phone, for the embed viewer alone.

## Phase 3: first sample app

Once the tests pass: a desktop app with the Hybrid toolbar and palette, add/erase/paint/pick, orbit camera, undo/redo, `.vox` open and save, and an FPS and voxel-count readout, with the browser build as a second target.

## Working rules

- Ask the owner before starting each phase, and report results in plain language with the numbers.
- Keep `docs/decisions.md` current: when research or a test changes a decision, update it there.
- Repo is private but written as if public: no internal hostnames, local paths or machine names in commits or docs.
