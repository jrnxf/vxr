# Phase AI test 6: big staged changes

**Result: pass.** A 100,000-cell staged change on the island, drawn as a ghost at 3024x1964, takes the editor's render frame from 1.69 to 1.84 ms (median, p95 1.88 ms). Added to the app's full frame, measured at 1.97 ms in the same session, that comes to about 2.1 ms. The ghost overlay itself costs 0.03 to 0.08 ms. The rest is the after state's extra voxels, which the scene would cost anyway once applied. Apply takes 2.2 to 7.2 ms (median) and Discard 1.7 to 3.6 ms, each counted until the next frame is on screen. The worst single repeat in 252 was 9.5 ms. The bar is 50 ms.

- Machine: Apple M4 Pro (16-core GPU, 24 GB), Metal, native. 3024x1964, the full-Retina editor size the app's `--shot` frame time uses.
- Scene: the island (`prototype/demo/scene.vox`): 145,064 voxels, bounds 128x31x128, 412 brick slots.
- Runs: two full runs back to back on a quiet machine (1-minute load average under 5), under `spikes/gpu-lock`. The two runs agree to 0.01 ms on every frame median. The tables below are the first run.

## Pass or fail

| Check (from `docs/plan.md`) | Measured | Verdict |
|---|---|---|
| Frame with a 100k-cell ghost stays within the current budget (about 2 ms without the render corner) | Render frame 1.69 to 1.84 ms median (p95 1.88 ms), default framing; app frame about 1.97 to 2.12 ms; overlay alone +0.03 ms | **Pass** |
| Apply under 50 ms | 2.2 to 7.2 ms median, 9.5 ms worst repeat | **Pass** |
| Discard under 50 ms | 1.7 to 3.6 ms median, 3.8 ms worst repeat | **Pass** |

## Method

`cargo run --release -- <scene.vox> [shots dir]` in this folder, via `../gpu-lock ../../dev`. The program does the following:

- **The staged change** (`src/staged.rs`) follows `docs/research/ai.md`: one open edit on the live scene (`Scene::begin` and `put`). Added and repainted cells go into the map at once, so the view shows the after state. Removed cells stay in the map until Apply, so the ghost can draw what goes. Apply erases them and closes the edit with `Scene::commit`: one undo step. Discard is `Scene::revert_voxels` plus `commit`, which leaves no undo step.
- **The mix:** 60,000 added, 20,000 repainted and 20,000 removed cells. Each kind gets its own third of the island's columns, sorted by x. Removed takes the top voxels of the first third, layer by layer. Repainted takes the middle third from the surface down, each voxel moved to the next palette slot. Added builds about 11 layers on top of the last third. On screen, every hit pixel in the right half is a ghost cell, which is the overlay's worst case.
- **The ghost** (`src/ghost.rs`, `src/march.wgsl`) is a copy of `vxr_render::Renderer` and its march shader with one addition. Beside the selection mask, a second storage buffer holds 2 bits per voxel in the same per-brick-slot layout (32 words per slot): 1 added, 2 repainted, 3 removed. On a hit, the shader looks up the cell's 2 bits and draws:
  - added: tinted green, with a brighter edge line;
  - repainted: its new colour, with an accent edge;
  - removed: dimmed red, with a red edge.
- **The ghost mask** is built on the CPU from the staged cell lists, the way `Scene::selection_mask` is built. It is uploaded with `queue.write_buffer`.
- **The frame** is the editor's GPU work for one frame:
  - the camera uniform with the editor's default guides (ground, grid, bounds frame, 2 px lines);
  - the march at 3024x1964;
  - the blit onto a `Bgra8UnormSrgb` window-sized target;
  - a wait for the GPU.

  It is timed in wall-clock time per frame, as in `shot.rs` `frame_times`. Each state gets 5 runs of 120 frames after 4 warm-up frames. The four states take turns run by run, so clock drift lands on all of them alike. The march pass alone is timed again with GPU timestamps, 5 runs of 60 frames.
- **Views:** framed is the editor's `.` framing and the default view. Iso is the isometric snap. Close is 45% of the framed distance, aimed at the added cells, so the ghost fills most of the view.
- **Apply, Discard and undo:** each is timed from the call through the dirty-brick upload (`GpuMap::upload_dirty`), clearing the ghost, and the next frame drawn and waited for. Each runs 21 times on four changes: the mix and 100k each of added, repainted and removed cells. After every Discard and every undo, the program checks that every voxel and the palette counts match the opened island. It also checks that Apply is exactly one undo step and Discard is none.

## Frame time

CPU + GPU per frame, median and p95 of 600 frames (5 runs x 120):

| View | Before, real renderer | Before, spike renderer (no ghost) | Staged, ghost off (after state only) | Staged, ghost on |
|---|---|---|---|---|
| framed | 1.69 / 1.80 ms | 1.70 / 1.76 ms | 1.80 / 1.85 ms | 1.84 / 1.90 ms |
| iso | 1.58 / 1.63 ms | 1.58 / 1.64 ms | 1.65 / 1.71 ms | 1.69 / 1.74 ms |
| close | 2.72 / 2.78 ms | 2.73 / 2.80 ms | 2.89 / 2.95 ms | 2.96 / 3.02 ms |

March pass alone, GPU timestamps (median):

| View | Before, real | Before, spike | Staged, ghost off | Staged, ghost on |
|---|---|---|---|---|
| framed | 1.37 ms | 1.38 ms | 1.48 ms | 1.51 ms |
| iso | 1.26 ms | 1.27 ms | 1.34 ms | 1.37 ms |
| close | 2.40 ms | 2.42 ms | 2.57 ms | 2.64 ms |

- The spike's copy without a ghost matches the real renderer within 0.02 ms. The one extra uniform word and the empty binding cost nothing measurable.
- The ghost's own cost (ghost on minus ghost off) is 0.03 ms framed, 0.03 ms iso and 0.07 ms close by timestamps. That is one storage-buffer read per hit pixel, like the selection highlight.
- The 0.1 to 0.17 ms between "before" and "ghost off" comes from the 60,000 added voxels and the deeper surface they make. The scene costs that once applied anyway.
- This harness has no egui, so its frame is smaller than the app's. The app's own `--shot` check, run in the same session under the same lock, gave 1.97 ms without the render corner and 2.20 ms with every voxel selected. `docs/decisions.md` records 2.00 to 2.25 ms for that same number. The ghost adds 0.15 ms at the default framing (0.11 ms from the added voxels, 0.03 ms from the overlay), so the app frame comes to about 2.12 ms. That is inside the 2.00 to 2.25 ms the editor has always run at, and under the select-all case.

## Apply, Discard and undo

Median / p95 / max of 21 repeats, until the next frame is drawn. "Scene" is the `vxr_core` call alone.

| Change | Stage (open edit, put) | Ghost mask build | Apply | Discard | Undo of the Apply |
|---|---|---|---|---|---|
| 100k mixed | 3.9 ms | 0.1 ms | 3.6 / 4.8 / 9.5 ms (scene 0.8) | 3.4 / 3.6 / 3.7 ms (scene 0.3) | 3.7 / 3.9 / 5.0 ms |
| 100k added | 4.6 ms | 0.1 ms | 2.2 / 2.3 / 2.3 ms (scene 0.1) | 3.1 / 3.2 / 4.0 ms (scene 0.4) | 2.8 / 3.2 / 3.3 ms |
| 100k repainted | 4.5 ms | 0.1 ms | 2.1 / 2.2 / 2.3 ms (scene 0.1) | 3.5 / 3.7 / 3.9 ms (scene 0.4) | 3.1 / 3.3 / 3.6 ms |
| 100k removed | 0 ms | 0.1 ms | 7.1 / 7.3 / 7.3 ms (scene 4.7) | 1.7 / 1.7 / 1.7 ms (scene 0) | 3.3 / 3.3 / 3.3 ms |

- Most of every total is the frame itself (1.7 to 2.5 ms) plus the upload (0.4 to 1.0 ms of CPU). The scene work is under 1 ms, except Apply of removals. Apply puts 100,000 cells through `Scene::put` and the open edit's `HashMap`, which takes 4.7 ms. The same `put` path makes staging 100k cells take 3.9 to 4.6 ms. The assistant's tool calls spread that cost while the model streams.
- The worst single repeat, 9.5 ms on the mix, was the first Apply of the run (the frame took 8.2 ms while the GPU clocked up).

**Under heavy CPU load:** the first runs of this spike overlapped another job running 48 busy-loop processes on the machine (load average about 80). Medians of up to 60 ms for Apply of 100k removals and p95s near 100 ms were seen there, almost all of it in CPU time. Those runs also used an earlier, buggy split of the cells, so they are not in the tables. They show the bar has headroom on an idle machine, not on a saturated one.

## What it changes for the build

1. **The ghost is a second mask beside the selection.** It uses 2 bits per voxel in the selection mask's per-brick-slot layout, and the march shader tints on a hit. No second trace and no new pass are needed. Build it again whenever brick slots move (`map.fills` changes), as the selection mask already is; 0.1 ms for 100k cells.
2. **Removed cells stay in the map until Apply.** Without that, there is nothing to draw where they were. Two follow-ons for the review step:
   - `vxr-ops` queries (counts, the scene summary, slices) must treat staged removals as gone, so the model sees the after state.
   - The render corner traces the same map, so it shows removed cells as still present. Before/After in the review card should be the ghost view against a plain view. Alternatively, the preview tracer can skip cells the ghost marks removed, which needs the same mask bound to it.
3. **Apply, Discard and Stop need no new history machinery.** The scene's open edit (`begin`, `put`, `revert_voxels`, `commit`) already gives one undo step for Apply and none for Discard. Undo of an Apply restores the scene byte for byte.
4. **Apply of removals is the slowest path** (4.7 ms of scene work for 100k). At the 200k-cell default limit it would be about 10 ms, still far under the bar. If it ever needs to be faster, record removals in the open edit at stage time and only flip the map bits at Apply.
5. **The 200k-cell limit per request in `docs/research/ai.md` can stay.** Every cost here grows with the cell count, and 100k cells use under a fifth of the Apply and Discard bar.

## Files

- `src/main.rs`: the benchmark.
- `src/staged.rs`: the staged change, its ghost mask, and the plan of cells.
- `src/ghost.rs` and `src/march.wgsl`: the renderer copy with the ghost; `src/blit.wgsl` is unchanged.
- Screenshots: pass a shots directory to write `before-framed.png` and `ghost-framed.png`, `ghost-iso.png` and `ghost-close.png` at 3024x1964. They are not committed; `shots/` is git-ignored, and the program regenerates them in a minute.
