# Recommendation

**Summary**

- **Engine:** a sparse brick map (8^3 bricks with occupancy masks and `u8` palette indices), ray marched in compute shaders for both the editor view and the path tracer. No meshes in the core. Greedy-meshing the same bricks stays as the fallback for weak GPUs and a WebGL2 embed.
- **UI:** egui first, HTML/CSS over the canvas as the runner-up. The embed uses neither.
- **`.vox`:** our own two-layer codec (raw chunks kept in order, semantics on top). No existing Rust crate round-trips real files.
- **Browser:** WebGPU required for the editor and the path tracer. The embed is single-threaded, WebGPU-only, its own crate, with a poster image for browsers without WebGPU.
- **Biggest unknowns:** ray-march and path-trace speed on real Apple GPUs and phones, egui reaching the Hybrid look, and phone time to first frame. Nobody has published those numbers; Phase 2 measures them.

Sources are in the topic files: [engine](engine.md), [gpu-limits](gpu-limits.md), [path-tracing](path-tracing.md), [ui-layer](ui-layer.md), [vox-format](vox-format.md), [embed-weight](embed-weight.md), [native-vs-browser](native-vs-browser.md).

## Stack picks

| Area | Pick | Why | Fallback if Phase 2 fails |
|---|---|---|---|
| Voxel data | Sparse brick map per object, 8^3 bricks, 512-bit mask + 1 byte per voxel | One GPU structure serves view and path tracer; edits touch only the bricks hit; recolor stays a palette write. About 2.3 B per voxel (estimate) | Compacted bricks (occupied voxels only, about 1.6 B per voxel) for big scenes |
| Rendering | DDA ray marching in WGSL compute, two levels (brick grid, then bitmask) | WebGPU has no hardware ray tracing; brick maps trace about as fast as the best sparse trees | Greedy meshes built from the same bricks |
| GPU memory layout | Brick pool split into bindings of 128 MiB or less, or an `r8uint` 3D texture atlas; limits raised from `adapter.limits()` | The defaults are 128 MiB per binding and 256 MiB per buffer; Android often grants no more | - |
| Live preview | Progressive path tracing at about 640x360 in a fixed per-frame slice (start at 3 to 4 ms), cut into about 1 ms dispatches, albedo-demodulated A-trous denoise, reset only edited pixels | wgpu and WebGPU have one queue, so no async compute | Teardown-style cheap mode (AO, shadows, one bounce) on phones |
| UI | egui | Mature, wgpu-native, same code on desktop and web, shares the render pass | HTML/CSS over the canvas, browser build first |
| `.vox` | Own codec in the workspace, `ogt_vox` as the semantic reference, `dot_vox` and `vox-rs` as cross-check readers | `dot_vox` re-saved only 3 of 15 real files byte for byte and lost all 8 models of a Teardown file | - |
| Embed | Separate crate and wasm target, WebGPU only, WGSL only, no fonts or UI, poster first, load on visibility, one shared device per page | WebGL2 adds about 1 MB brotli; egui adds about 2.9 MB | Lazy-loaded WebGL2 mesh viewer, only if the poster proves not enough |
| Threads | Optional speed-up on vxr's own site only; editor and embed must work single-threaded | A one-tag embed cannot get cross-origin isolation on someone else's page | - |

## Corrections to earlier docs

- `docs/plan.md` said storage buffers default to 256 MiB per binding. The default is **128 MiB per binding**; 256 MiB is the default max buffer size ([gpu-limits](gpu-limits.md)).
- `docs/plan.md` said wasm is "roughly 1.5 to 2x slower". That holds for one thread (1.45 to 1.55x average, up to 2.5x). The bigger gaps are threads (about 5 to 10x for bulk work in a single-threaded embed, estimate) and GPU readback latency ([native-vs-browser](native-vs-browser.md)).
- `docs/market-research.md` recommended web-first with a WebGL2 fallback. `docs/decisions.md` already moved to desktop first. This research adds: the editor and path tracer need WebGPU; WebGL2 could only ever serve a mesh-based embed.

## Riskiest unknowns left

1. **Ray-march and path-trace speed on Apple GPUs and phones.** Every Mrays/s figure comes from other hardware. If it falls short, the core still holds but the view falls back to meshes.
2. **egui reaching the Hybrid look.** No backdrop blur, basic animation, unproven IME. If it fails, the UI moves to HTML/CSS with IPC and input routing.
3. **Phone time to first frame for the embed.** No published numbers for WebGPU device creation, pipeline compile or wasm compile on iOS Safari and mid Android.
4. **`.vox` rotation semantics.** Whether transforms compose down the scene tree is disputed between readers; only MagicaVoxel itself settles it, and its Mac build is Intel-only (Rosetta).
5. **Browser memory.** Firefox breaks above a 2 GB heap; iOS Safari can drop to a few hundred MB after a reload.

## Phase 2 tests, with thresholds

Test machine: Apple M4 Pro (16-core GPU), 24 GB, 3024x1964 Retina. It is faster than the M1-class Macs many users have, so thresholds keep about 2x headroom, and each test also reports numbers with the GPU workload scaled down where noted. Browsers: native, Chrome stable, Safari 26. Firefox numbers are recorded but do not decide pass or fail.

Each spike lives in `spikes/<name>/` and ends with a `RESULTS.md`: what was measured, the numbers, pass or fail, and what it changes.

### 1. Engine (brick map, ray marched)

Scenes: the prototype island (145k voxels), a generated 10M-voxel terrain, a generated 100M-voxel terrain, and one large public `.vox` scene.

| Metric | Pass |
|---|---|
| Frame time, editor view at full Retina, 10M voxels, orbiting | 8.3 ms or less native (120 fps); 16.7 ms or less in Chrome and Safari |
| Frame time, 100M voxels | 16.7 ms or less native; 33 ms or less in Chrome and Safari |
| Edit latency, radius-16 brush, input to visible change | 1 frame native; 2 frames in browsers (CPU write + upload included) |
| GPU memory per voxel on real `.vox` scenes | 3 B or less (target 2.3 B) |
| Largest scene that loads in each browser without a crash | 100M voxels in Chrome and Safari on the Mac |
| Also record | Adapter limits granted (native, Chrome, Safari, Firefox), upload throughput for a 256 MB brick pool, frame time at half resolution as a stand-in for M1 |

Fail on frame time: switch the editor view to greedy meshes built from the same bricks, keep the brick map for the path tracer.

### 2. Live render preview

Run on the 10M-voxel scene while a scripted brush edits continuously.

| Metric | Pass |
|---|---|
| Editor frame time with the preview on vs off | Adds 4 ms or less, and stays at or under 16.7 ms in Chrome and Safari |
| Samples per pixel per second at 640x360 (diffuse, one bounce) | 30 or more native; 20 or more in browsers |
| Time to a clean image after the camera stops | 3 s or less native, 5 s or less in browsers, judged against a 1024-spp reference (no visible noise at 1x) |
| Palette recolor | Updates the preview within 1 frame with no data rebuild |
| Also record | Mrays/s primary and one-bounce, denoise cost per frame, whether Safari exposes GPU timestamps |

Fail: drop the preview to 480x270 or a cheaper look, and retest. If still failing, the preview becomes "pauses while editing, resumes on idle".

### 3. UI layer (egui vs HTML/CSS)

Build the Hybrid toolbar, palette panel, hint bar and Cmd-K in both, in the browser build first, then egui native.

| Check | Pass |
|---|---|
| Screenshots vs the Hybrid mockups at 1x and 2x | Owner judges it matches; no blurry or misaligned text at 11 to 13 px |
| Frame time the UI adds to the 3D view | 1 ms or less |
| Click to voxel latency | 1 frame |
| Japanese IME in the Cmd-K field | Composition shows inline; Tab and Enter pick candidates without leaking to the app |
| VoiceOver on toolbar and palette | Every control has a name and role on desktop; browser recorded, not a blocker |
| Editor shell wasm size without the engine | Recorded for both; not a pass line for the editor |

Decision rule: egui wins unless the owner judges its look clearly below the HTML/CSS build, or IME fails. The owner judges the look; this is the one test that needs them.

### 4. `.vox` round-trip

| Check | Pass |
|---|---|
| Unedited re-save | Byte-identical on 100% of the corpus (committed set plus fetched set, including v200 and Teardown files) |
| Edited re-save (set one voxel, recolor one slot, move one node) | Only the chunks the edit had to change differ; `dot_vox` and `ogt_vox` read the same voxels and world transforms |
| MagicaVoxel check | About 20 fixtures, including nested rotated nodes and odd/even sizes, look the same in MagicaVoxel 0.99.7.2 after our re-save; MagicaVoxel's re-save reads back the same |
| Fuzzing | 1 hour of `cargo-fuzz` with no panics and no unbounded allocation |
| Property test | Random scenes, including ones over 256^3, write-read-write byte-identical |

### 5. Embed weight

Viewer only: `.vox` parse, brick upload, ray march, orbit, poster. Its own crate.

| Metric | Pass |
|---|---|
| wasm + JS glue, brotli q11 | 150 KB or less (hard ceiling 300 KB) |
| Poster visible after the tag is parsed | 100 ms or less |
| First lit frame after the embed becomes visible, mid-range phone, 10 Mbit/s | 1.0 s or less (2.5 s on Lighthouse Slow 4G) |
| Also record | Device creation, pipeline compile and wasm compile time on one iPhone and one mid Android; largest scene iOS Safari survives across 3 reloads; twiggy size profile |

Fail on size: profile with twiggy and cut. Fail on time: precompile pipelines on idle, shrink shaders, or show the poster longer.

## What to decide after Phase 2

- WebGL2 fallback for the embed, or poster only (test 5 numbers plus WebGPU share at that time).
- egui or HTML/CSS (test 3).
- Licence (still open from `docs/market-research.md`).
