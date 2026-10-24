# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## First session only: onboarding

This section exists to hand the project over to a new machine. On the first session in this repo:

1. Read `README.md`, `docs/decisions.md`, `docs/plan.md` and `docs/market-research.md` in full. Skim `docs/ui-directions.html` (direction D, Hybrid, is the pick) and `docs/screenshots/`.
2. Summarize back to the owner, in a few lines, what vxr is, what has been decided, and what the first work is (`docs/plan.md`: desk research first, then risk tests on this Mac). Ask before starting any of it.
3. Then clean up this file: delete this whole "First session only" section, and keep only what every future session needs (what vxr is, the stack, where the decisions live, how to build and test once that exists). Commit that cleanup on its own.

## Where things live

- `docs/plan.md`: what to research and test next, in order.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/market-research.md`: the market case. Reference only.
- `docs/ui-directions.html`: the UI options that led to Hybrid. Reference only.
- `prototype/`: a throwaway three.js editor used to try the feel. Not the real build; mine it for ideas (chunking, greedy meshing, palette-texture recolor, `.vox` I/O), do not extend it.
