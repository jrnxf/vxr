# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## Stack

- Rust core on wgpu (Metal, Vulkan, DX12, WebGPU in the browser). Desktop first; the browser build and the embed come from the same code. No server.
- UI drawn in Rust with egui (picked by test 3, see `docs/decisions.md`) so the Hybrid UI runs native and in the browser.
- The real build is the Cargo workspace at the root (Phase 3, in progress). Run everything through `./dev <command>`, a Nix dev shell (`nix/flake.nix`: Rust with the wasm32 target, wasm-bindgen, wasm-opt, twiggy, brotli).
- The Phase 2 spikes in `spikes/` build separately through `spikes/dev <command>` (`spikes/dev --fuzz` adds nightly and cargo-fuzz). Wrap timed GPU runs in `spikes/gpu-lock` so benchmarks never share the GPU.

## Build and test

- `./dev cargo test`: unit and property tests (brick map, undo/redo, `.vox` round-trip). Run `node prototype/src/build.mjs` first so the island tests run instead of skipping. `PROPTEST_CASES=<n>` sets the property test size.
- `./dev cargo clippy --all-targets -- -D warnings` and `./dev cargo fmt --check` must be clean.
- `./dev cargo build --target wasm32-unknown-unknown` checks the browser target.
- Fixtures are generated in tests (`vxr_vox::fixtures`), never committed as `.vox` files.

## Where things live

- `crates/vxr-vox`: the `.vox` codec (from `spikes/vox`). `crates/vxr-core`: the editable scene (brick map, undo/redo, `.vox` open and save), no GPU. `crates/vxr-render`: the GPU renderer (from `spikes/engine`: compacted brick map upload, `trace_h` march, blit, orbit camera); its tests render on the real GPU and compare every pixel with the CPU ray cast, and skip when there is no adapter.

- `docs/plan.md`: what to research and test next, in order. Ask the owner before starting each phase.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/research/`: Phase 1 desk research, one file per topic.
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
