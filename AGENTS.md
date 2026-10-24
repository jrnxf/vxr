# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## Stack

- Rust core on wgpu (Metal, Vulkan, DX12, WebGPU in the browser). Desktop first; the browser build and the embed come from the same code. No server.
- UI drawn in Rust so the Hybrid UI runs native and in the browser; the UI layer is not picked yet.
- No build or tests exist yet. Add how to build and test here once they do.

## Where things live

- `docs/plan.md`: what to research and test next, in order. Ask the owner before starting each phase.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/research/`: Phase 1 desk research, one file per topic.
- `docs/market-research.md`: the market case. Reference only.
- `docs/ui-directions.html`: the UI options that led to Hybrid (direction D). Reference only.
- `prototype/`: a throwaway three.js editor used to try the feel. Not the real build; mine it for ideas (chunking, greedy meshing, palette-texture recolor, `.vox` I/O), do not extend it.
