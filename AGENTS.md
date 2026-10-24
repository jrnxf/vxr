# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## Stack

- Rust core on wgpu (Metal, Vulkan, DX12, WebGPU in the browser). Desktop first; the browser build and the embed come from the same code. No server.
- UI drawn in Rust so the Hybrid UI runs native and in the browser; the UI layer is not picked yet.
- No real build yet. The Phase 2 spikes in `spikes/` build through `spikes/dev <command>` (a Nix dev shell with Rust, the wasm32 target, wasm-bindgen, wasm-opt and twiggy; `spikes/dev --fuzz` adds nightly and cargo-fuzz). Wrap timed GPU runs in `spikes/gpu-lock` so benchmarks never share the GPU.

## Where things live

- `docs/plan.md`: what to research and test next, in order. Ask the owner before starting each phase.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/research/`: Phase 1 desk research, one file per topic.
- `docs/market-research.md`: the market case. Reference only.
- `docs/ui-directions.html`: the UI options that led to Hybrid (direction D). Reference only.
- `spikes/`: Phase 2 risk tests (engine, preview, vox, embed), each with a `RESULTS.md`. Throwaway, but the engine and the `.vox` codec are the starting point for Phase 3.
- `prototype/`: a throwaway three.js editor used to try the feel. Not the real build; mine it for ideas (chunking, greedy meshing, palette-texture recolor, `.vox` I/O), do not extend it.

## Before launch

- [ ] Clean up `spikes/` before the repo goes public: fold what Phase 3 keeps (engine, `.vox` codec) into the real workspace, delete the rest, and decide whether the `phase-2-spikes-data` release (screenshots, raw benchmark data, test files) stays or goes.
