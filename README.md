# vxr

fast voxel scenes

Early stage. The real build is a Rust workspace in `crates/`, and the first desktop editor runs: open a MagicaVoxel `.vox` file, add, erase, paint and pick with the voxel, box and face brushes (and X mirror), orbit, pan and zoom, undo and redo, and save. The palette is editable: all 255 slots, a colour editor with hex, RGB and HSV that recolours every voxel using the slot instantly, and swap and copy of slots as in MagicaVoxel, each one undo step. Saving an opened file is lossless: materials, layers, cameras, names and anything vxr does not understand are kept, and an unedited save is byte for byte the same file. It uses the Hybrid UI (tool rail, palette, hint bar, Cmd-K) drawn with egui, and shows the voxel count and FPS. The same editor runs in the browser with WebGPU (open and save there go through a file picker and a download). See `AGENTS.md` for how to build and run both.

## What is here

- `prototype/` - a throwaway in-browser editor (three.js) used to try the feel: add, erase, paint and pick, box and face brushes, X mirror, palette-first recolor, unlimited undo, `.vox` open and save, and a one-tag `<voxel-scene>` embed.
  - `prototype/demo/` is the built version and is not in git: run `node prototype/src/build.mjs` (no dependencies) to create it, including the starter island `scene.vox`. Serve it with any static server, for example `python3 -m http.server -d prototype/demo`, then open `index.html` (`?scene=terrain` loads a 3.9M-voxel stress scene) or `embed.html`.
  - `prototype/src/` holds the sources and `build.mjs`.
- `docs/market-research.md` - whether there is a market for an open-source voxel editor, and which wedge to take.
- `ui-directions.html` (in `vxr-design-references.zip` on the `design-references` GitHub release) - four UI directions for the editor. The pick is **D, Hybrid**.

## Direction

- A Rust core on wgpu, built both as a native app and as WebAssembly with WebGPU for the browser and the embed.
- MagicaVoxel's feel, without its limits: sparse scenes well past 256^3, lossless `.vox` round-trip, and a scene file that is also a web embed.
