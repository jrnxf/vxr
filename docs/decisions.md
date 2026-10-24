# Decisions

What has been settled for vxr so far, and why. Newest context first within each section.

## Identity

- Name: **vxr**. Description everywhere: **fast voxel scenes** (short, terse, all lowercase).
- Packages: crate `vxr` on crates.io; npm `@jrnxf/vxr` (scoped, so it cannot be squatted). Neither is published yet.
- Repo is private for now.

## Product

- An open-source voxel scene editor with MagicaVoxel's feel and none of its limits, plus a one-tag web embed.
- The wedge, from `docs/market-research.md`: MagicaVoxel is loved but closed source, stalled, Windows-only in its current build, Intel-only on Mac, has no Linux build, and caps models at 256^3. Nobody owns voxel web embeds.
- v1 must keep what MagicaVoxel users love: results in minutes, the hand-sculpting feel, a palette-first workflow, a beautiful built-in renderer.
- v1 must fix the loudest complaints: the 256^3 cap, render being a separate mode, platform gaps, capped undo.
- Out of v1: collaboration, rigging, AI generation, accounts, cloud storage.

## UI: Hybrid (direction D)

Picked from four directions in `docs/ui-directions.html` (A Studio, B Sketch, C Diorama, D Hybrid).

- One small floating toolbar: add, erase, paint, pick, plus a few shapes (voxel, box, face). Single-letter keys for everything, modifiers for quick overrides (Shift erases, Alt picks), and hints near the cursor.
- The palette is always on screen. Editing a palette slot recolors every voxel using it.
- The scene tree appears only once a second object exists, so a single model stays clean.
- A live path-traced preview sits in a corner while you edit, instead of a separate render mode.
- Cmd-K command palette for everything else.
- Camera: MagicaVoxel style. Right-drag orbits, middle-drag or Shift+right-drag pans, wheel zooms toward the cursor, `.` frames the scene.

## Platform and stack

- **Desktop first, browser from the same code.** Desktop gets full GPU and CPU, no browser memory caps, and real file handling. The browser build exists for "try it from a link" and for the embed, which can only live on the web.
- Rust core on **wgpu** (Metal, Vulkan, DX12, and WebGPU in the browser). UI drawn in Rust too (egui or similar) so the same Hybrid UI runs in the window and in the browser. The UI layer choice is still to be confirmed by test 3 below.
- Ship order: desktop editor, then the embed (a viewer, small), then the full editor in the browser.
- The browser build will not be laggy for normal scenes: rendering runs on the GPU at near-native speed. Desktop only pulls ahead at the extreme end (huge scenes, heavy CPU remeshing, browser memory caps).
- **No server.** Rust compiles to WebAssembly and runs on the visitor's machine. The site, editor and embed are static files (free on Cloudflare); the npm package is served free by jsDelivr and unpkg. A server is only needed later for sharing or accounts, and a small Cloudflare Worker would do.
- Build and tune on a Mac with a real GPU. The prototype was built on a machine with no GPU, so its speed was never measured on hardware.

## Phase 1 research picks (pending Phase 2 tests)

From `docs/research/`, summarized in `docs/research/recommendation.md`. Each holds unless its Phase 2 test fails.

- **Engine:** a sparse brick map per object (8^3 bricks, occupancy mask plus one palette byte per voxel), ray marched in compute shaders for both the editor view and the path tracer. No meshes in the core; greedy meshes from the same bricks are the fallback. Replaces the prototype's chunked greedy meshing.
- **Live preview:** progressive path tracing at about 640x360 in a fixed slice of each frame, since wgpu and WebGPU have one queue and no async compute.
- **UI:** egui first, HTML/CSS over the canvas as runner-up; test 3 decides.
- **`.vox`:** our own codec, raw chunks kept in order so an unedited re-save is byte-identical. No Rust crate round-trips real MagicaVoxel files.
- **Browser:** WebGPU is required for the editor and path tracer. The market research's WebGL2 fallback could only ever serve a mesh-based embed; whether to ship one or just a poster is decided after test 5.
- **Embed:** its own crate and wasm target, WebGPU only, single-threaded (a one-tag embed cannot get cross-origin isolation), target 150 KB brotli.

## First work: five tests to cut the risk

Throwaway spikes, measured on the Mac, before the real build.

1. **Engine:** a Rust/wgpu sparse renderer built native and for the browser. Measure FPS, memory and edit latency at 10M and 100M voxels. Settles how big the native advantage really is.
2. **Live render preview:** progressive path tracing in a corner view while editing. Measure the hit to editing frame rate. The riskiest piece of Hybrid.
3. **UI layer:** a Rust-drawn UI (egui) against a web UI over the GPU canvas (Tauri-style shell). Hybrid's polish depends on this.
4. **`.vox` round-trip:** open and re-save a corpus of real MagicaVoxel files (materials, layers, cameras, rotations) and prove nothing changes.
5. **Embed weight:** wasm download size and time to first frame on a phone.

After those, the first real milestone is a sample desktop app: Hybrid toolbar and palette, add/erase/paint, orbit, undo/redo, `.vox` open and save, FPS and voxel-count readout, with the browser build as a second target.

## What the prototype proved

`prototype/` is a throwaway three.js editor (see the README). Findings worth keeping:

- Sparse 32^3 chunks in a map, greedy meshing with per-vertex ambient occlusion, and only dirty chunks remeshed on a per-frame budget: a single edit remeshes 1 to 4 chunks in about 1 to 7 ms.
- Meshing 145k voxels takes about 0.11 s and 3.9M voxels about 0.39 s, on the CPU in JavaScript.
- Recoloring through a palette texture read in the vertex shader makes palette edits instant, with no remesh. Keep this trick.
- `.vox` round-trip of its own files was lossless (0 mismatches over 145,064 voxels); scenes over 256^3 split into 256^3 models and read back intact. It ignores model rotation, `MATL` materials and layers, and was not checked in MagicaVoxel itself.
- The embed as a web component: shadow DOM, its own orbit that never captures page scroll, and rendering only while on screen.
- Island draws 32k triangles in 18 draw calls; the 3.9M-voxel terrain draws 595k triangles in 248 calls. FPS was not measured on real hardware.

Screenshots: `docs/screenshots/`.
