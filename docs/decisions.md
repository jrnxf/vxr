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

Picked from four directions in `ui-directions.html` (in the `design-references` GitHub release) (A Studio, B Sketch, C Diorama, D Hybrid).

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

## Engine, preview, `.vox` and embed (settled by Phase 2)

Phase 1 picks from `docs/research/recommendation.md`, now confirmed or changed by the Phase 2 spikes in `spikes/` (each has a `RESULTS.md`). Measured on an Apple M4 Pro (16-core GPU) with other apps using the GPU, so times are upper bounds.

- **Engine: sparse brick map, ray marched, compacted layout by default.** 8^3 bricks with an occupancy mask; palette bytes stored only for occupied voxels (per-brick offset plus popcount). Measured 1.2 to 2.4 B per voxel on every scene tried; the dense layout used 4.3 to 8.9 B on hollow models and failed the 3 B line. Compacted costs 3 to 15% frame time. Full Retina, median: 10M voxels 4.7 ms native, 4.9 Chrome, 5.0 Safari; 100M voxels 6.9 / 7.1 / 7.4 ms. Radius-16 edits land in the next frame. Browsers loaded 300M voxels. Traversal: `trace_h` (brick steps plus a coarse 64-bit occupancy word that skips empty 32^3 cells).
- **The browser draws as fast as native.** Native wins only on CPU work (building a 100M scene: 0.66 s native, 1.3 to 2.0 s in browsers). This backs "desktop first, browser from the same code".
- **Live preview: progressive path tracing at 640x360 in the corner, passes everywhere.** It adds 2.5 to 3.8 ms per frame and comes clean in 0.3 to 1.2 s. Budget all preview work (trace, denoise, composite) at 3 ms per frame while editing and 12 ms after 300 ms idle; the idle boost is required. Write the preview into the editor image instead of a second full-screen pass. No 480x270 or pause-while-editing fallback needed.
- **UI:** egui first, HTML/CSS over the canvas as runner-up; test 3 decides. Not run yet; it needs the owner to judge the look.
- **`.vox`: our own two-layer codec (raw chunks plus semantics), about 1,700 lines.** 568 of 568 real MagicaVoxel files re-save byte for byte; edits change only the chunks they must; 1 hour of fuzzing found no crash after one fixed bug. MagicaVoxel (0.99.6.2) confirms that transforms compose parent then child and that pivots are floor(size/2). The scene model needs instancing (shared nodes) from the start. MagicaVoxel clamps positions to about +-1000, so warn when exporting wider scenes. 0.99.7.x-only behaviour (version 200 overlap order, `_frm_t`) is untested: it ships only for Windows.
- **Browser:** WebGPU is required for the editor and path tracer.
- **Embed: its own crate, WebGPU only, poster-only fallback (no WebGL2).** 39.8 KB brotli for wasm, glue and loader (target 150 KB). On an iPhone 17 Pro the scene shows 217 ms after scrolling into view at 10 Mbit/s and 1.16 s on Slow 4G. The snippet puts the poster as an `<img>` inside the tag (70 ms; the `poster=` attribute alone missed the 100 ms line). The embed loads a brick-format scene file with `.vox` as the fallback: the island is 14.5 KB as bricks against 137 KB as `.vox`. A built-in WebGL2 fallback would be about 25 times the viewer. Mid-range Android is not measured.

### Open going into Phase 3

- Growing the compacted pool on the GPU instead of repacking on the CPU: today a full pool costs one hitch of 54 ms native and about 100 ms in browsers at 100M voxels.
- The native close, low view sits near the 120 fps line; a depth prepass is the likely fix.
- Not measured: M1-class Macs (estimate about 15 ms at 10M, full Retina), mid-range Android, the iOS reload memory test, MagicaVoxel 0.99.7.x.
- Longer multi-core fuzzing before the embed reads files from strangers.
- Licence (still open from `docs/market-research.md`).

## What the prototype proved

`prototype/` is a throwaway three.js editor (see the README). Findings worth keeping:

- Sparse 32^3 chunks in a map, greedy meshing with per-vertex ambient occlusion, and only dirty chunks remeshed on a per-frame budget: a single edit remeshes 1 to 4 chunks in about 1 to 7 ms.
- Meshing 145k voxels takes about 0.11 s and 3.9M voxels about 0.39 s, on the CPU in JavaScript.
- Recoloring through a palette texture read in the vertex shader makes palette edits instant, with no remesh. Keep this trick.
- `.vox` round-trip of its own files was lossless (0 mismatches over 145,064 voxels); scenes over 256^3 split into 256^3 models and read back intact. It ignores model rotation, `MATL` materials and layers, and was not checked in MagicaVoxel itself.
- The embed as a web component: shadow DOM, its own orbit that never captures page scroll, and rendering only while on screen.
- Island draws 32k triangles in 18 draw calls; the 3.9M-voxel terrain draws 595k triangles in 248 calls. FPS was not measured on real hardware.

Screenshots: `vxr-design-references.zip` on the `design-references` GitHub release.
