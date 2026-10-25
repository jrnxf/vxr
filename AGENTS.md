# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## Stack

- Rust core on wgpu (Metal, Vulkan, DX12, WebGPU in the browser). Desktop first; the browser build and the embed come from the same code. No server.
- UI drawn in Rust with egui (picked by test 3, see `docs/decisions.md`) so the Hybrid UI runs native and in the browser.
- The real build is the Cargo workspace at the root (Phase 3, in progress). Run everything through `./dev <command>`, a Nix dev shell (`nix/flake.nix`: Rust with the wasm32 target, wasm-bindgen, wasm-opt, twiggy, brotli).
- The Phase 2 spikes in `spikes/` build separately through `spikes/dev <command>` (`spikes/dev --fuzz` adds nightly and cargo-fuzz). Wrap timed GPU runs in `spikes/gpu-lock` so benchmarks never share the GPU.

## Build and test

- Once per checkout: `node prototype/src/build.mjs` (the island `scene.vox`) and `crates/vxr-app/fetch-fonts` (Inter, JetBrains Mono and the Lucide icons into the git-ignored `crates/vxr-app/fonts/`; without them the app falls back to egui's fonts and the icons show as boxes).
- Run the editor: `./dev cargo run --release -p vxr-app -- [scene.vox]`, for example `prototype/demo/scene.vox`. Left-click edits with the tool on the rail, right-drag orbits, Shift+right-drag or middle-drag pans, the wheel zooms, `.` frames, Cmd-Z / Cmd-Shift-Z undo and redo, Cmd-O / Cmd-S open and save, Cmd-K (or `?`) lists every command.
- Headless check of the app: `./dev cargo run --release -p vxr-app -- --shot crates/vxr-app/shots prototype/demo/scene.vox`. It drives every tool through the same pointer path as the window, undoes and redoes, checks that an unedited save is byte-identical and an edited save keeps every non-model chunk (lossless save), saves and reopens, prints `ok` or `FAIL` per step (exit code 1 on failure), and writes `app-1x.png`, `app-2x.png` and `app-2x-edited.png` into the git-ignored `crates/vxr-app/shots/`. It needs no window, so it works with the screen locked.
- `./dev cargo test`: unit and property tests (brick map, undo/redo, editing tools, `.vox` round-trip, GPU renderer). Without the island the island tests skip. `PROPTEST_CASES=<n>` sets the property test size.
- `./dev cargo clippy --all-targets -- -D warnings` and `./dev cargo fmt --check` must be clean.
- `./dev cargo build --target wasm32-unknown-unknown` checks the browser target.
- Browser editor: `crates/vxr-web/build` makes the static site in the git-ignored `crates/vxr-web/dist/` (wasm, page, fonts with their licences, the island as `scene.vox`); serve that folder to try it. It needs WebGPU. `crates/vxr-web/check` (after the build; needs `chrome-devtools-axi` on the PATH) checks it end to end in headless Chrome: it serves `dist/` on 127.0.0.1, runs `check.js` in the page (every tool, the camera gestures, undo/redo, Cmd-K, Open through the file input, Save through the download and its lossless chunks, resize), checks the no-WebGPU message in a second browser without WebGPU, prints `ok` or `FAIL` per step (exit code 1 on failure) and writes 1x and 2x screenshots into the git-ignored `crates/vxr-web/shots/`. Extend `check.js` with each new feature, as `shot.rs` is extended natively. In the page, `window.vxr.state()` returns the editor state as JSON. `crates/vxr-web/deploy` builds and deploys it to https://vxr.jrnxf.co (Cloudflare Workers static assets, `crates/vxr-web/wrangler.jsonc`), only from a clean `main`.
- Fixtures are generated in tests (`vxr_vox::fixtures`), never committed as `.vox` files.

## Where things live

- `crates/vxr-vox`: the `.vox` codec (from `spikes/vox`). `crates/vxr-core`: the editable scene (brick map, undo/redo, `.vox` open and lossless save through `vxr_vox::Document::set_world`), no GPU. `crates/vxr-render`: the GPU renderer (from `spikes/engine`: compacted brick map upload, `trace_h` march, blit, orbit camera); its tests render on the real GPU and compare every pixel with the CPU ray cast, and skip when there is no adapter. `crates/vxr-app`: the desktop editor (binary `vxr`): `editor.rs` (scene, camera, gestures and drawing, no window), `ui.rs` (the Hybrid UI in egui, from `spikes/ui`), `native.rs` (winit window, file dialogs), `shot.rs` (the headless check). `crates/vxr-web`: the same `Editor` on a WebGPU canvas (`src/web.rs`) and its page (`www/index.html`), which forwards input, opens files through a file picker and downloads saves.

- `docs/plan.md`: what to research and test next, in order. Ask the owner before starting each phase.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/research/`: Phase 1 desk research, one file per topic. `docs/research/magicavoxel-gap.md` lists every MagicaVoxel feature with vxr's status and priority; update its row when a feature lands.
- `docs/market-research.md`: the market case. Reference only.
- `ui-directions.html` and the prototype screenshots: the UI options that led to Hybrid (direction D) and the mockups test 3 compares against. Reference only, in `vxr-design-references.zip` on the `design-references` GitHub release, not in git.
- `spikes/`: Phase 2 risk tests (engine, preview, vox, embed), each with a `RESULTS.md`. Throwaway, but the engine and the `.vox` codec are the starting point for Phase 3.
- `prototype/`: run `node prototype/src/build.mjs` once to create `prototype/demo/` (git-ignored), including the island `scene.vox` the spikes load. A throwaway three.js editor used to try the feel. Not the real build; mine it for ideas (chunking, greedy meshing, palette-texture recolor, `.vox` I/O), do not extend it.

## Keep the repo small

- The owner wants a lean repo. Do not commit heavy files (screenshots, videos, recordings, benchmark dumps, large test data, built bundles) when they can be avoided: git history keeps them forever.
- Put them in a zip on a GitHub release (`gh release create` or `gh release upload`) and link the release from the docs, or regenerate them from a script.
- Small files the code needs (a test fixture of a few KB, a poster image for the embed) are fine. Ask before committing anything over about 200 KB.

## Before launch

- [ ] Clean up `spikes/` before the repo goes public: fold what Phase 3 keeps (engine, `.vox` codec) into the real workspace, delete the rest, and decide whether the `phase-2-spikes-data` release (screenshots, raw benchmark data, test files) stays or goes.
