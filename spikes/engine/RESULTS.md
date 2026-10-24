# Test 1: engine (sparse brick map, ray marched)

**Verdict: pass, with the compacted layout as the default.** Frame time, edit latency and largest scene pass natively and in Chrome and Safari, with room to spare in the browsers. GPU memory per voxel fails with the dense layout on hollow or thin models (4.3 to 8.9 B). The compacted layout, now built and measured, passes on every scene (1.16 to 2.39 B), for 4 to 12% more frame time.

- The browser draws as fast as native. At 10M voxels, full Retina, orbiting the framed scene: native 5.6 ms, Chrome 5.6 ms, Safari 5.6 ms (median). The native advantage is CPU work: building the 100M scene takes 0.66 s native, 1.3 s in Safari, 2.0 s in Chrome, 2.6 s in Firefox.
- A radius-16 brush costs about 0.25 ms of CPU (brush plus upload of about 57 bricks), so every edit at 10M lands in the next frame.
- 300M voxels load and render in Chrome, Safari and Firefox. The wasm heap grew to 3,968 MiB in all three, so the "Firefox breaks above 2 GB" risk did not show up on this Mac.
- Memory: the dense layout (512 palette bytes per brick) takes 1.2 to 1.7 bytes per voxel on solid or painted scenes but 4.3 to 8.9 on hollow scans and lattices, where bricks are mostly empty. The compacted layout stores palette bytes only for occupied voxels and measures 1.16 to 2.39 on every scene: 18 to 25% less on solid scenes, 62 to 73% less on hollow ones. See "Compacted vs dense".
- Caveat: other apps kept the GPU 50 to 67% busy during every run, so all frame times are upper bounds (see "How it was measured").

## Pass or fail per threshold row

| Metric | Pass line | Measured (median / p95) | Result |
|---|---|---|---|
| Frame time, 10M voxels, full Retina, orbiting | 8.3 ms native, 16.7 ms Chrome and Safari | native 5.6 / 7.2 ms; Chrome 5.6 / 7.5 ms; Safari 5.6 / 7.0 ms | Pass |
| Frame time, 100M voxels | 16.7 ms native, 33 ms Chrome and Safari | native 7.7 / 9.4 ms; Chrome 8.0 / 9.8 ms; Safari 8.4 / 10.7 ms | Pass |
| Edit latency, radius-16 brush, input to visible change | 1 frame native, 2 frames in browsers | native 10M: 360 of 360 edits fit one 120 Hz frame (CPU 0.25 / 0.39 ms, frame 5.5 / 6.7 ms). Chrome and Safari: every edit's CPU plus GPU fits one 60 Hz frame, so 2 frames once the browser's compositor frame is added | Pass |
| GPU memory per voxel on real `.vox` scenes | 3 B or less (target 2.3 B) | Compacted (measured): 1.16 to 2.39 B on all 7 real scenes, 2.18 to 2.39 B on the hollow ones. Dense: 1.15 to 1.67 B on solid or painted models, 4.3 to 8.9 B on hollow or lattice models | Pass with compacted; fail with dense |
| Largest scene that loads without a crash | 100M in Chrome and Safari | 300M in Chrome, Safari and Firefox (503 MB of GPU data). 300M is close to this spike's own cap, not a browser limit | Pass |

Stress view, not a pass line: a close, low camera with the horizon in view (long grazing rays) costs native 8.0 / 9.8 ms at 10M. Its p95 is over 8.3 ms. Chrome and Safari stay at 8.3 to 8.5 / 10.2 ms, well under 16.7 ms.

## What was built

- `src/brickmap.rs`, the CPU side:
  - A dense grid of brick slots over the scene bounds, and 8^3 bricks that each hold a 512-bit occupancy mask and one palette byte per voxel.
  - A sphere brush, dirty tracking, and CPU picking through the same structure.
- `src/brickmap.wgsl`, the GPU side (bind group 1):
  - Two palette layouts behind the same bindings, picked when the map is uploaded (`GpuBrickMap::new_with_layout`). Dense: 512 bytes per brick in up to four 128 MiB pages of 2^18 bricks. Compacted: see "Compacted vs dense". No binding is over 128 MiB.
  - A coarse level stores 64 bits per 4x4x4 bricks (32^3 voxels).
  - Three traversals: `trace2` (brick, then voxel), `trace` (coarse, brick, voxel, nested), and `trace_h` (brick DDA that tests the coarse bits instead of reading the grid, and jumps a whole empty 32^3 cell in one step). `trace_h` is the editor default (`RenderOptions { levels: 4 }`).
- `src/march.wgsl`: one primary ray per pixel in an 8x8 compute workgroup. It does directional and ambient shading and can also write a G-buffer (depth and normal plus palette index) for the test 2 path tracer. `src/gpu.rs` blits the result to any target.
- `src/vox.rs`: a minimal bounds-checked `.vox` reader. It reads SIZE, XYZI, RGBA (with MagicaVoxel's default palette as the fallback) and `nTRN` translations composed down the scene graph. It counts rotations but does not apply them. It does not read Teardown's `TDCZ`.
- Two binaries:
  - `bench` does the native measurements and screenshots.
  - `view` is an interactive window with MagicaVoxel-style orbit, pan and zoom toward the cursor, click to add and Shift-click to erase, `.` to frame, and H for a traversal heat map.
- The same library compiles to wasm (`src/web.rs`, `web/index.html`). The page runs the identical measurements through wgpu's WebGPU backend.

## How it was measured

- **Machine:** Apple M4 Pro, 16-core GPU, 24 GB, 3024x1964 Retina at 120 Hz.
- **Software:**
  - Native: wgpu 30.0.1 on Metal.
  - Browsers: Chrome 155, Safari 26.6.2, Firefox 158. Firefox was installed during this test at the owner's request; its numbers are recorded only.
  - The browser build uses wasm-bindgen 0.2.129.
- **A frame:**
  - The ray-march compute pass at 3024x1964, plus a blit into a 3024x1964 BGRA target.
  - Rendered offscreen, so the display's vsync does not cap it.
  - Timed with GPU timestamps around both passes, one frame in flight at a time. Wall-clock throughput from batches of 30 back-to-back frames agrees within about 0.5 ms.
- **Orbit:** a full 360 degree turn in 180 frames. Terrain numbers pool 3 runs (540 frames) natively and in Chrome and Safari, and 2 runs in Firefox.
- **Two cameras:**
  - Framed: MagicaVoxel's `.`, with the scene's box centred and filling 90% of the screen, 34 degrees above the horizon.
  - Close: 15 degrees above the horizon at a quarter of that distance, with the horizon in view.
- **Edits:** 120 radius-16 brushes per run, alternating add and erase at the surface point under a moving screen position.
  - CPU time covers the brush and the upload of the touched bricks. GPU time is the next frame's.
  - An edit counts as "within one frame" when CPU plus GPU time fits the frame budget: 8.33 ms native at 120 Hz, 16.7 ms in browsers.
  - A readback confirmed the first edit's frame changes on screen: 7,995 pixels at 10M and 672 at 100M.
- **Background load:**
  - Before each run, macOS reported the GPU 50 to 67% busy with other apps, mostly a messaging app and browser pages left open by another spike.
  - The spikes took turns on a GPU lock, but this load could not be removed. Every frame time here is measured under it and is an upper bound; an idle machine will be faster.
- **Timer resolution:** Safari and Firefox clamp `performance.now()` to 1 ms, so their per-edit CPU times are whole milliseconds; use their means.
- **Firefox caveat:** Firefox's timestamps and its work-done callbacks disagree (by wall clock, half resolution looked slower than full), so its frame times are rough.

## Scenes

| Scene | Voxels | Notes |
|---|---|---|
| Island | 145,064 | `prototype/demo/scene.vox`, 128x31x128, v200 |
| Terrain 10M | 9,983,668 | Prototype terrain generator, 655x655 footprint, **solid fill**: every voxel from y = 0 up to the surface or sea level counts, the hidden interior included. 25,508 bricks |
| Terrain 100M | 100,026,399 | Same generator, 2040x2040 footprint, 253,147 bricks |
| Terrain 200M, 300M | 199,904,781 and 299,998,382 | Only for the largest-scene probe |
| monu3 (largest public scene) | 848,469 | MagicaVoxel's biggest official sample, 112x120x112 |
| monu6, menger, nature, dragon, teapot | 28k to 365k | Only for memory per voxel |

Fetched into `fetched/` (git-ignored, never committed) from `ephtracy/voxel-model` at commit `3bff8feaa7d94a6237f2c07dc4ca3c3d268ef06c` (MIT). Each URL is `https://raw.githubusercontent.com/ephtracy/voxel-model/3bff8feaa7d94a6237f2c07dc4ca3c3d268ef06c/<path>`.

| Path | sha256 |
|---|---|
| `vox/monument/monu3.vox` | `09a48ddb48927fdab8a2015e668aa99e2465fd2d6c02d7d81c5feb8b5a44dcf3` |
| `vox/monument/monu6.vox` | `643c3e27f5237f917034424161256b2504dc0797bafb7adade63da4fc8433e82` |
| `vox/procedure/menger.vox` | `f63f56086f0cc31af0cd6acd4039272a652e96a8ec77d44e62eb57d14298d633` |
| `vox/procedure/nature.vox` | `0b94623cecb68393d8ec523cc2856511a7503c710c0afff480f990b6b96d4051` |
| `vox/scan/dragon.vox` | `f87c20c30b41716c29bcdb4b778236a51ddf34572d167cdf88b2220adbf12b69` |
| `vox/scan/teapot.vox` | `2288242b9bbabf704f2b9d5f317204dc6e9b5daad4d37b5e25d8214cb988b0b7` |
| `MagicaVoxel-file-format-vox.txt` (default palette source) | not hashed; only its 256-entry palette is copied into `src/default_palette.rs` |

## Numbers

### Frame time (ms, median / p95)

| Scene and view | Native | Chrome | Safari | Firefox (recorded only) |
|---|---|---|---|---|
| 10M, framed | 5.64 / 7.21 | 5.64 / 7.47 | 5.59 / 7.00 | 6.18 / 9.32 |
| 10M, close | 8.03 / 9.75 | 8.52 / 10.16 | 8.26 / 10.13 | 8.62 / 10.09 |
| 10M, framed, half resolution (1512x982) | 1.82 / 2.70 | 1.84 / 2.82 | 1.83 / 2.69 | 2.35 / 5.01 |
| 100M, framed | 7.72 / 9.40 | 8.00 / 9.83 | 8.35 / 10.68 | 8.63 / 10.43 |
| 100M, close | 10.05 / 11.30 | 10.49 / 12.19 | 11.41 / 12.88 | 11.35 / 12.45 |
| 100M, framed, half resolution | 2.67 / 3.97 | 2.69 / 3.80 | 2.82 / 3.70 | 3.25 / 6.14 |
| Island, framed (one run in browsers) | 2.10 / 2.94 | 1.64 / 2.10 | 1.61 / 2.02 | 2.69 / 10.31 |
| monu3, framed | 0.87 / 1.61 | - | - | - |

M1 stand-in:
- Half resolution has a quarter of the pixels and costs about a third of full resolution.
- For an M1 with an 8-core GPU at full Retina, expect very roughly 2.5 to 3x the full-resolution numbers: about 15 ms at 10M framed. That is a 60 fps editor, or 120 fps at half resolution. This is an estimate from GPU core counts and memory bandwidth, not measured.

### Edits (radius-16 brush, about 9,000 to 16,000 voxels changed, 57 bricks uploaded median, 80 max)

| | CPU ms, steady (median / p95 / max) | Next frame GPU ms (median / p95) | Edits within one frame |
|---|---|---|---|
| Native 10M | 0.25 / 0.39 / 0.75 | 5.5 / 6.7 | 100% (360 edits, 120 Hz budget) |
| Native 100M | 0.26 / 0.43 / 0.64 | 7.3 / 8.1 | 92%; the rest take 2 frames because the frame itself is near 8.3 ms |
| Chrome 10M / 100M | 0.2 / 0.3 / 0.4 | 5.5 / 6.7 and 6.9 / 8.3 | 100% (60 Hz budget) |
| Safari 10M / 100M | mean 0.12 and 0.15 (1 ms clock) | 5.1 / 6.0 and 7.6 / 8.1 | 100% |
| Firefox 10M / 100M | mean 0.84 and 0.93 (1 ms clock) | 6.7 / 12.6 and 8.2 / 16.0 | 100% and 94% |

- **First edit vs steady:**
  - Natively the first edit costs 0.38 to 0.53 ms of CPU, against a steady median of 0.25 ms, because the caches are cold. It still lands in the same frame.
  - In browsers the first edit is within the 1 ms timer noise.
- **The 31 ms outlier from phase 1 was not the brush:**
  - It did not come back in 6 reruns of that exact 10-edit run, or in 1,440 edits across the final native runs (max 0.75 ms).
  - Under synthetic full CPU load (24 busy processes on 12 cores), edits reached 6.3 ms.
  - The 31 ms edit ran while the other spikes were compiling wgpu in parallel. It was the edit thread being descheduled.
  - A real editor should keep the edit and upload on a thread that is not starved by background work.
- **Recolor:** one 4 KB palette write, below timer resolution (under 0.01 ms natively). No voxel data changes.

### GPU memory per voxel

Measured for both layouts; details in "Compacted vs dense" below.

| Scene | Voxels | Brick fill | Dense used (B/voxel) | Compacted used (B/voxel) |
|---|---|---|---|---|
| Island | 145,064 | 69% | 1.67 | 1.24 |
| monu3 | 848,469 | 99% | 1.15 | 1.16 |
| monu6 | 365,232 | 76% | 1.50 | 1.21 |
| menger | 160,000 | 26% | 4.30 | 1.63 |
| nature | 75,835 | 15% | 7.69 | 2.15 |
| dragon (scan) | 40,265 | 15% | 7.54 | 2.18 |
| teapot (scan) | 28,411 | 13% | 8.86 | 2.39 |
| Terrain 10M | 9,983,668 | 76% | 1.51 | 1.23 |
| Terrain 100M | 100,026,399 | 77% | 1.49 | 1.22 |

"Used" is everything the scene occupies on the GPU without pool headroom: grid, coarse level, masks, and the palette bytes (dense: 512 per brick; compacted: the heap blocks plus 8 bytes of offset and prefix per brick).

### Loading, upload and limits

| | Native | Chrome | Safari | Firefox |
|---|---|---|---|---|
| Build 10M / 100M scene on the CPU | 67 ms / 655 ms | 209 ms / 1,975 ms | 149 ms / 1,346 ms | 283 ms / 2,594 ms |
| Upload 100M scene (149 MB) | 34 ms | 51 ms | 52 ms | 128 ms |
| Upload 256 MB brick pool (two 128 MiB buffers) | 6.4 GB/s | 6.8 GB/s | 7.5 GB/s | 2.6 GB/s |
| 200M / 300M scene: loads and renders | not tried (no browser cap) | yes / yes, build 4.0 s / 6.1 s | yes / yes, build 2.7 s / 4.0 s | yes / yes, build 6.5 s / 8.9 s |
| wasm heap reached (128 MiB steps) | - | 3,968 MiB | 3,968 MiB | 3,968 MiB |

Adapter limits granted (the spike requests everything the adapter offers):

| Limit | Native Metal | Chrome 155 | Safari 26.6 | Firefox 158 |
|---|---|---|---|---|
| maxBufferSize | 4 GiB | 4 GiB | 2 GiB | 2 GiB |
| maxStorageBufferBindingSize | 4 GiB | 4 GiB | 2 GiB | 2 GiB |
| maxStorageBuffersPerShaderStage | 29 | 10 | 44 | 9 |
| maxStorageTexturesPerShaderStage | 32 | 8 | 44 | 32 |
| maxBindGroups | 8 | 4 | 11 | 8 |
| maxUniformBufferBindingSize | 4 GiB | 64 KiB | 2 GiB | 2 GiB |
| maxTextureDimension3D | 2048 | 2048 | 2048 | 2048 |
| timestamp-query | yes | yes | yes | yes |

Safari exposes GPU timestamps, which the path-tracing research had left open.

## Compacted vs dense

**How the compacted layout works**
- Same grid, coarse level and masks as dense. The pages hold a heap instead of 512 bytes per brick: each live brick owns one 16-byte-aligned block with palette bytes for its occupied voxels only, in local index order.
- After the masks, each brick has two words: the block's byte offset, and the set-bit counts of its first 4, 8 and 12 mask words (9 bits each).
- A voxel's byte is at offset + (set mask bits below it). With the prefix word the shader needs at most 4 popcounts. A first version without it looped over up to 15 mask words and cost 25 to 31% more frame time; the prefix brought that down to the numbers below.
- Edits re-pack each touched brick (a few microseconds). A brick whose block is too small, or less than half used, moves to a new block from a free list per block size, or from the top of the heap. When the heap is full (headroom is an eighth of the heap, at least 64 KiB), everything is re-packed into a bigger heap.
- Shaders need no change: `palette_value` in `BRICKMAP_WGSL` reads both layouts, picked by a flag in the map uniform. Same bindings, so the test 2 path tracer gets it for free.
- Checked pixel for pixel: dense and compacted render identical frames before and after 300 mixed edits (add, erase, paint, radius 2 to 16, whole-brick fills, heap rebuilds forced) on the island, the teapot, the dragon and the 10M terrain.

**How these runs were measured**
- Dense and compacted runs alternate, so both see the same background load.
- Frame times are the median run of 3: each run's median and p95, then the median over runs. One run in two configurations hit a background GPU burst (98% busy) and is excluded by that.
- The background GPU load was 30 to 45% for these runs (60% at most in browsers), lower than for the first tables. So the dense numbers here are about 20% lower than the dense numbers above. Compare within this section only.

**Frame time** (ms, median / p95, 3024x1964)

| Scene and view | Native dense | Native compacted | Chrome dense | Chrome compacted | Safari dense | Safari compacted |
|---|---|---|---|---|---|---|
| 10M framed | 4.23 / 5.26 | 4.68 / 5.77 | 4.52 / 5.64 | 4.92 / 6.03 | 4.60 / 5.54 | 5.04 / 5.88 |
| 10M close | 6.20 / 6.97 | 6.37 / 7.24 | 6.68 / 7.67 | 6.88 / 7.73 | 6.38 / 7.25 | 6.76 / 7.58 |
| 10M framed, half resolution | 1.41 / 1.91 | 1.58 / 2.05 | 1.57 / 2.16 | 1.70 / 2.16 | 1.52 / 2.08 | 1.75 / 2.30 |
| 100M framed | 6.12 / 7.35 | 6.86 / 8.29 | 6.42 / 7.93 | 7.08 / 8.78 | 6.70 / 7.73 | 7.43 / 8.83 |
| 100M close | 8.12 / 8.64 | 8.76 / 9.33 | 8.72 / 9.44 | 9.37 / 9.96 | 8.48 / 9.19 | 9.73 / 10.35 |

- Compacted costs 3 to 15% more per frame: about 0.2 to 0.5 ms at 10M and 0.6 to 1.3 ms at 100M. Every row still passes its line: 8.3 ms native and 16.7 ms in browsers at 10M; 16.7 and 33 ms at 100M.
- Firefox (2 runs each, recorded only) was slow and noisy this time: 10M framed 11.9 / 17.0 dense and 13.7 / 18.5 compacted; 100M framed 14.6 / 21.3 and 16.4 / 23.2. That is about 2x its earlier runs; its timers are unreliable (see "How it was measured").

**Edits, radius-16 brush, alternating add and erase** (357 steady edits per row over 3 runs)

| | Dense CPU ms (median / p95 / max) | Compacted CPU ms (median / p95 / max) | Compacted blocks moved per edit | Within one frame, compacted |
|---|---|---|---|---|
| Native 10M | 0.21 / 0.30 / 0.40 | 0.32 / 0.46 / 0.81 | 32 | 100% (120 Hz) |
| Native 100M | 0.21 / 0.29 / 0.39 | 0.32 / 0.47 / 0.79 | 30 | 100% (120 Hz) |
| Chrome 10M / 100M | 0.2 / 0.3 / 0.4 | 0.2 / 0.3 / 0.4 (mean 0.22) | 30 | 100% (60 Hz) |
| Safari 10M / 100M | mean 0.15 / 0.14 (1 ms clock) | mean 0.13 / 0.14 (1 ms clock) | 30 | 100% (60 Hz) |

- The first edit costs 0.46 to 0.55 ms natively with compacted against 0.27 to 0.44 ms with dense; it still lands in the same frame.
- Of the 57 bricks a brush touches, about 30 change size enough to move to a new block. Each move costs a free-list pop and one extra 8-byte write.
- One dense native 10M run shows 87% of edits within a frame: that run's frames were slowed by the same background burst. The other dense runs show 100%.

**Worst cases for compacted edits** (120 edits each, native unless noted)

| Case | Dense CPU ms (median / p95 / max) | Compacted CPU ms (median / p95 / max) | Notes |
|---|---|---|---|
| Fill a mostly empty brick completely (from 16 to 27 voxels to 512), teapot, dragon, nature, terrain 10M | 0.02 to 0.03 / under 0.11 / under 0.17 | 0.02 to 0.05 / under 0.08 / under 0.30 | Every fill moves its block. All 120 edits in one frame |
| Add-only brushes on hollow models (teapot, dragon, nature) | 0.06 / 0.17 / 0.32 | 0.06 / 0.21 to 0.34 / 0.50 | The heap filled 1 to 2 times per model and was re-packed; on these small models a re-pack takes under 0.5 ms |
| Add-only brushes, terrain 10M (55 blocks moved per edit) | 0.19 / 0.27 / 2.1 (one brick-pool rebuild) | 0.34 / 0.46 / 8.3 (one heap re-pack) | The heap-full re-pack is the one edit of 120 that missed a frame |
| Same, Chrome and Safari, teapot | 0.1 / 0.2 / 0.4 | 0.1 / 0.2 / 0.5 | 1 ms clock in Safari; every edit in one frame |

- A heap-full re-pack costs the same as uploading the whole compacted pool: 54 ms at 100M natively, 85 ms in Safari, 104 ms in Chrome. With an eighth of the heap as headroom, that happens after about 15 MB of net growth at 100M.
- Dense has the same kind of hitch when its brick pool runs out (28 ms at 100M natively).
- The fix for both: grow the buffers on the GPU (copy into a bigger buffer, or add a page) instead of re-packing on the CPU. Not built.

**Upload of the full pool** (pack on the CPU + queue the writes, and total until the GPU has it)

| | 10M dense | 10M compacted | 100M dense | 100M compacted |
|---|---|---|---|---|
| Native | 2.0 + total 5.3 ms (15.1 MB) | 4.4 + total 7.3 ms (12.3 MB) | 19.8 + total 28 ms (149 MB) | 45 + total 54 ms (122 MB) |
| Chrome | total 8 ms | total 13 ms | total 47 ms | total 104 ms |
| Safari | total 19 ms | total 14 ms | total 50 ms | total 85 ms |
| Firefox | total 45 ms | total 105 ms | total 149 ms | total 176 ms |

- Compacted uploads 18% fewer bytes, but packing costs CPU, so the full upload takes about 2x as long at 100M. That is still a one-time cost at load, under 0.2 s in every browser.
- Raw upload throughput for a 256 MB pool in these runs: native 8.0 to 8.7 GB/s, Chrome 8.0 to 9.0, Safari 8.1 to 8.7, Firefox 1.9 to 2.6.

**Which should be the default: compacted.**
- It is the only layout that passes the memory row on hollow models: 2.2 to 2.4 B against 7.5 to 8.9 B per voxel, a quarter of the memory. It also saves 18 to 25% on solid scenes.
- Memory is what limits big scenes in browsers and on phones; 4 to 12% of frame time is not limiting on this Mac.
- What it costs: about 0.1 ms more CPU per edit, a 2x slower one-time pack at load, and the heap-full re-pack hitch until heap growth moves to the GPU.
- Keep dense only as an option for now. Drop it if phones show no frame-time reason to keep it.

## Bug found by test 2: `trace_h` stalled after edits (fixed)

**Symptom**
- Test 2 saw `trace_h` primary rays in the framed 10M view drop from 773 to 242 Mrays/s after 240 brush edits, natively only.
- Plain `trace`, Chrome and Safari did not slow down.
- Reproduced with test 2's own bench, built against this library in a separate target directory: 786 Mrays/s before the edits, 244 to 247 after, in 4 runs.

**Cause: rays stuck in a loop**
- Capping the traversal at 256 steps instead of 4,096 brought the after-edit speed back to 773 Mrays/s. So a few rays were running to the 4,096-step cap.
- The loop is in `trace_h`'s jump across an empty 32^3 coarse cell. After the jump it re-derived the brick position on the other two axes by rounding the hit position, and it set t to the cell's exit time even if that was behind the current t.
- With rounding error, the ray could land back in a cell it had already left. Two empty cells could then hand it back and forth until the step cap.
- Edits matter because they carve empty coarse cells next to surfaces. Native only, because Metal compiles with fast math, so the rounding differs from Dawn's and WebKit's.
- The coarse occupancy words themselves were correct after `upload_dirty`. No barrier was missing.

**Fix** (in `brickmap.wgsl`; no API change)
- On the jump, t never goes back: `t = max(t, exit)`.
- The brick coordinates on the two other axes are clamped to the cell being left. The ray leaves through the face of the jump axis first, so on the other axes it is still inside that cell.

**After the fix**
- Test 2's bench: trace_h 685 to 712 Mrays/s before the edits, 651 to 670 after. That matches `trace`'s own run-to-run spread; the absolute numbers are lower than the first repro because of background load.
- The close view goes from 468 to 431, as before the bug.
- Editor view natively, framed 10M, before and after 300 edits (median / p95 ms):

  | Traversal and layout | Before 300 edits | After 300 edits |
  |---|---|---|
  | `trace_h`, dense | 4.25 / 5.31 | 3.68 / 4.55 |
  | `trace_h`, compacted | 4.62 / 5.82 | 4.08 / 4.85 |
  | `trace`, dense | 5.05 / 6.41 | 3.63 / 4.45 |
  | `trace`, compacted | 5.48 / 7.01 | 3.98 / 4.81 |

  With one secondary ray per pixel from the hit (`--secondary`), `trace_h` goes from 13.2 to 13.0 ms (dense) and from 13.6 to 13.0 ms (compacted). The edited terrain is cheaper to draw, so nothing slows down after edits.
- The fix costs nothing measurable: the framed 10M dense median is 4.25 ms against 4.23 ms in the comparison runs before it.
- The frame-time tables above were measured before the fix, and stay valid because of that.
- `trace` and `trace_h` now render identical pixels on the island and the teapot, and differ on 11 of 5.9M pixels on the 10M terrain (silhouette ties), before and after 300 edits.
- My own editor-view runs never hit this bug: its rays never landed on the bad boundaries at 3024x1964 with my camera. Test 2's pixel-centre rays at 640x360 did.

## Optimizations tried

Each tuning comparison below was run back to back in one session; the final numbers above came later and include every kept change.

- **Clip rays to the occupied bricks' bounds:** kept.
- **Traversal** (ms, wall-clock throughput, medians):

  | | 10M framed | 10M close | 100M framed | 100M close |
  |---|---|---|---|---|
  | Two-level (brick, voxel) | 6.96 | 11.31 | 9.31 | 12.46 |
  | Three-level, nested | 7.62 | 7.86 | 11.53 | 12.07 |
  | Hybrid `trace_h` (kept) | 6.12 | 8.19 | 9.27 | 10.71 |

  The nested three-level loop helps long grazing rays but costs short ones, probably from register pressure. The hybrid keeps the brick loop and only reads the coarse bits.
- **Native: wgpu's shader runtime checks off** (buffer bounds checks and the counter naga adds to every loop, via `create_shader_module_trusted`). This is about 10% faster: 10M framed 5.87 to 5.27 ms, close 8.63 to 7.74 ms. It is safe here because the shader indexes only engine-written data and every loop is bounded. Browsers always keep their own checks.
- **Workgroup shape** (10M framed / close, GPU ms): 8x8 6.11 / 9.09, 8x4 6.01 / 8.93, 16x8 6.59 / 9.74, 16x16 6.35 / 9.27, 32x4 7.42 / 9.98. Kept 8x8.
- **Edits:** touched bricks upload in merged runs of consecutive ids, about 46 writes per brush.
- **Not tried:**
  - A low-resolution depth prepass to start primary rays near the surface. This is the obvious next win for grazing views.
  - The compacted layout on the GPU.
  - Temporal anti-aliasing.

## Visual notes

- The stripes on snow slopes in the framed terrain shots are sub-pixel voxel aliasing: one ray per pixel and roughly one voxel per pixel at that distance. The editor will want temporal anti-aliasing or a coarser look at distance.
- The traversal heat map (`shots/terrain-10m-close-heat.png`) shows the cost sitting at the horizon, where rays graze the terrain.

## What it changes for vxr

1. Keep the brick map, ray marched, for the editor view. This Mac does not need the greedy-mesh fallback; it remains a fallback for weak GPUs.
2. Make compacted bricks the default layout, not only a big-scene option. Measured: 1.16 to 2.39 B per voxel on every scene against 4.3 to 8.9 B with dense on hollow models, for 4 to 12% more frame time. Hollow models are common: scans, shells, anything hand-painted on the surface. Grow its heap on the GPU instead of re-packing on the CPU when it fills.
3. Native is not faster at drawing. It is faster at CPU work (scene build 2 to 4x), and it has no browser memory caps. Desktop-first stays right for files, memory and CPU-heavy operations, not for frame rate.
4. Use the hybrid traversal (`trace_h`, with the empty-cell jump fix) and the coarse 64-bit occupancy level.
5. On native, build trusted shaders without wgpu's runtime checks, for about 10%.
6. Plan bindings for 8 storage buffers per stage: Firefox gives 9, Chrome 10, and Android often 8. The march uses 6.

## Still open

- **Phones and M1-class Macs:** not measured; the M1 numbers above are an estimate.
- **Compacted heap growth:** a full heap is re-packed on the CPU, a 54 ms (native) to 104 ms (Chrome) hitch at 100M. Growing on the GPU is the fix; not built. The CPU copy of the scene still keeps 512 bytes per brick; only the GPU copy is compacted.
- **10M close view natively:** p95 is 9.8 ms against 8.3. A depth prepass or lower internal resolution while orbiting should fix it; not tried.
- **`trace_h` robustness:** the stall above came from floating-point ties at cell boundaries. A traversal fuzz test that shoots rays from exact voxel corners and faces, across edits, would catch the next one. Not built.
- **Background load:** the first tables were taken with the GPU 50 to 67% busy with other apps, the compacted comparison at 30 to 45%. Rerun `tools/run-final.sh` and `tools/run-compact.sh` on an idle machine for clean numbers.
- **Presentation path:** the window or canvas present path (vsync, compositor) is not timed. Edit latency in frames comes from CPU plus GPU time against the frame budget, plus the readback check.
- **The spike's `.vox` reader:** it ignores rotations and `TDCZ`. Test 4 owns the real codec.
- **Scene cap:** this spike caps scenes at four 128 MiB palette pages (1,048,576 bricks, about 400M solid voxels). It was not pushed past 300M.

## Files and how to rerun

- **Sources:**
  - `src/` holds the library: `brickmap.rs`, `brickmap.wgsl`, `march.wgsl`, `gpu.rs`, `scene.rs`, `vox.rs`, `camera.rs`, `bench.rs`, `web.rs`.
  - `src/bin/` holds `bench.rs` and `view.rs`.
  - `web/` holds the browser page and its local server.
- **Tools:**
  - `tools/build-web.sh`
  - `tools/browser-run.sh`
  - `tools/run-browsers.sh` (first matrix, probes)
  - `tools/run-final.sh` (pooled runs)
  - `tools/run-compact.sh` (dense vs compacted, interleaved)
  - `tools/compare.py` (prints the dense vs compacted tables)
  - `tools/summarize.py`
  - `tools/pool.py`
- **Data and images:**
  - `results/`: raw JSON per run. `final-*` files are the pooled runs; `cmp-*`, `worst-*` and `mem-*` are the dense vs compacted runs; the others are the first browser matrix and the probes.
  - `shots/`: 3024x1964 screenshots of the 10M and 100M terrains (framed and close), the island and monu3, the heat map, and Chrome rendering the 100M terrain after its edit run.

```
cd spikes/engine
../dev cargo build --release
./target/release/bench terrain:10m > out.json        # all native measurements for one scene
./target/release/bench terrain:10m --layout compact  # same with the compacted layout
./target/release/bench fetched/teapot.vox --check    # dense vs compacted and trace vs trace_h, pixel for pixel, after 300 edits
./target/release/bench terrain:10m --drift 300       # framed frame time before and after 300 edits (add --secondary, --levels, --layout)
./target/release/bench --limits                      # adapter limits
./target/release/view terrain:10m                    # interactive window
tools/build-web.sh
python3 web/serve.py 8731 results ../../prototype/demo/scene.vox fetched/monu3.vox &
tools/run-final.sh 8731                              # 3 native + browser reps, takes the GPU lock per run
python3 tools/pool.py native-10m results/final-native-terrain-10m-r*.json
```
