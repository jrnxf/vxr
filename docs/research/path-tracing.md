# Live path-traced preview

**Answer.** Feasible on Apple Silicon if the preview is small and budgeted. Trace a two-level grid (brick map plus 8^3 occupancy bitmasks, the same structure as the engine) with DDA in a WGSL compute pass, at about 640x360, for a fixed slice of each frame (start at 3-4 ms) and accumulate progressively. WebGPU and wgpu have one queue, so there is no async compute: the budget has to come from slicing work across frames. Denoise with a light albedo-demodulated A-trous filter, not full SVGF. On an edit, reset only the pixels it touched. Palette recolor still needs no data rebuild, but it does dirty the accumulated image. Estimated cost on M1: about 0.5-1 sample per pixel per frame at 640x360 within 4 ms. Phones get roughly a third to a half of that.

## 1. Traversal: how to march rays through voxels

| Method | Primary Mrays/s | 1-bounce Mrays/s (ratio) | Edit cost | Source |
|---|---|---|---|---|
| Plain DDA, 1 bit per voxel | 24-33 | 2.6-24 (0.08-0.99x) | O(1) | [VoxelRT](https://github.com/dubiousconst282/VoxelRT) |
| Multi-level DDA | 43-148 | 31-65 (0.44-0.80x) | O(1) | same |
| Brick map (XBrickMap: 8^3 bitmask bricks + sectors) | 89-166 | 72-125 (0.76-0.87x) | ~O(1), sectors may reallocate | same |
| 64-tree (Tree64) | 93-209 | 83-137 (0.66-0.89x) | ~O(log n); chunk trees "fully rebuilt upon changes" | same |
| ESVO octree | 57-137 | 44-86 (0.63-0.80x) | ~O(log n), slow build | same |

- All VoxelRT rows come from one unnamed integrated GPU, in six scenes of about 450k-975k bricks of 8^3. The author "guesstimate[s] at least 5-10x throughput on real hardware" ([VoxelRT README](https://github.com/dubiousconst282/VoxelRT), MIT licence, 146 stars).
- Plain DDA is the base step: a few float ops per voxel step ([Amanatides and Woo 1987](https://www.eecs.yorku.ca/~amana/research/grid.pdf)). On its own it collapses on incoherent bounce rays, at 0.08-0.16x of the primary-ray rate in most scenes ([VoxelRT](https://github.com/dubiousconst282/VoxelRT)). Bounce rays are the whole cost of path tracing.
- Hierarchical DDA fixes this. A multi-level DDA plus compressed bricks gave a 2-3x speedup in a GPU voxel volume path tracer ([Hofmann and Evans, Ray Tracing Gems II](https://cris.fau.de/publications/266030856/)). Layered raw grids keep editing cheap and stay competitive with octrees ([van Wingerden 2015](https://studenttheses.uu.nl/handle/20.500.12932/20460)).
- Brick maps match the best tree on bounce rays and keep O(1) edits. Trees need a rebuild or path copying on each edit ([VoxelRT](https://github.com/dubiousconst282/VoxelRT)). For an editor, the brick map wins.
- Teardown, for comparison, gives each object its own 3D texture of 8-bit palette indices, with two extra mip levels to skip 2^3 and 4^3 empty blocks. A 1-bit world volume at 2^3 voxels per texel serves shadow, AO and reflection rays ([Wittens, Teardown Frame Teardown](https://acko.net/blog/teardown-frame-teardown/)). Its author found ray tracing voxels inside skewed object boxes "a much harder problem than I anticipated", and his new engine uses sparse 8^3 chunks ([voxagon blog, 2024](https://blog.voxagon.se/2024/12/29/year-summary.html)).
- **Pick:** one structure for the editor view and the tracer. A sparse map of 32^3 chunks (as in the prototype) holds 8^3 bricks, each with a 512-bit occupancy mask and a palette-index payload. Use a three-level DDA (chunk, brick, voxel) with the mask test at the brick level. This should line up with the Phase 1 engine pick (topic 1); if that topic picks something else, the tracer follows it.

## 2. Cost per sample: published numbers and estimates

No published voxel path tracer reports rays per second on Apple Silicon or on phones. Searches came up empty, so every per-device number below is an **estimate** scaled from FP32 throughput.

| GPU | FP32 TFLOPS | Source |
|---|---|---|
| M1 (8 core) | 2.6 | [metal-benchmarks](https://github.com/philipturner/metal-benchmarks) |
| M2 (10 core) | 3.6 | same |
| M1 Pro / M1 Max | 5.3 / 10.6 | same |
| M2 Max | 13.6 | same |
| A15 / A16 (iPhone) | 1.7 / 1.8 | same |
| A17 Pro / Adreno 750 | ~2 / 5.7 peak (forum figures) | [techboards thread](https://techboards.net/threads/geekerwan%E2%80%99s-review-of-the-snapdragon-8-gen-3-xiaomi-14.4384/page-3) |

- Anchor points from published work: ESVO cast 61-122 M primary rays/s at 1024x768 on a Quadro FX 5800, a 2008-era GPU ([Laine and Karras 2010](https://www.nvidia.com/docs/IO/88889/laine2010i3d_paper.pdf)). Brick maps on an integrated GPU reach 89-166 M primary and 72-125 M one-bounce rays/s ([VoxelRT](https://github.com/dubiousconst282/VoxelRT)).
- M3 and M4 add hardware ray tracing, and Apple claims M4's is 2x M3's ([Wikipedia, Apple M4](https://en.wikipedia.org/wiki/Apple_M4)). WebGPU and wgpu's portable path do not expose it, and voxel DDA does not use triangles anyway. Assume no help from it.
- **Estimate, M1:** 100-250 M primary rays/s with a brick-map DDA. A MagicaVoxel-style sample (primary hit, 2 bounces, one shadow ray per hit, about 5 rays) comes to roughly 20-50 M samples/s. M2 should be about 1.4x that, M1 Pro about 2x, and M1 or M2 Max about 4-5x (scaled by TFLOPS).
- **Estimate, preview:** 640x360 is 230k pixels. A 4 ms slice on M1 gives 80k-200k samples, so 0.35-0.9 spp per frame, or about 20-50 spp per second at 60 fps. MagicaVoxel-like convergence (a few hundred spp, then denoise) takes about 5-15 s while editing. When input goes idle, raise the slice to about 12 ms and that drops to 2-5 s.
- **Estimate, phones:** iPhone GPUs have about 0.7x M1's peak and throttle under sustained load. The A18 Pro trails the Snapdragon 8 Gen 3 in GPU benchmarks but draws less power ([Notebookcheck](https://www.notebookcheck.net/A18-Pro-vs-Snapdragon-8-Gen-3-GPU-benchmark-results-show-Apple-s-new-chipset-is-not-superior.890529.0.html)). Expect a third to a half of M1 throughput, sustained, in a browser. Mobile ray tracing has long been limited by FP throughput, SIMT divergence on incoherent rays, and bandwidth shared with the CPU ([Lee et al.](https://my.eng.utah.edu/~cs6958/papers/HWRT-seminar/p109-lee.pdf)). For the embed on phones, the default should be lower resolution or path tracing only while idle.
- Teardown shows the cheap end of the range. It traces about 1-2 sparse rays per pixel per effect (2 spp AO, 1 shadow ray, reflections through the coarse volume), runs volumetrics at half resolution, and hides the noise with blur and temporal reprojection ([Wittens](https://acko.net/blog/teardown-frame-teardown/)). That is a fallback look if full path tracing is too slow on phones.

## 3. Materials MagicaVoxel supports, and their cost

- `.vox` `MATL` types are `_diffuse`, `_metal`, `_glass`, `_emit`, with properties `_weight`, `_rough`, `_spec`, `_ior`, `_att`, `_flux`, `_plastic` ([official .vox extension spec](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt)). The app also has cloud media (0.99.4, [BlenderNation](https://www.blendernation.com/2019/05/29/non-blender-magicavoxel-0-99-4-adds-cloud-materials-translucent-shadows-and-more/)), plus blend (weighted Metal/Plastic/Glass/Cloud, "similar to Disney Principled BRDF"), SSS and emissive media (0.99.6.2, [GameFromScratch](https://gamefromscratch.com/magicavoxel-0-99-6-2-released/)).

| Material | Extra cost per sample (estimate) | Notes |
|---|---|---|
| Diffuse | baseline: cosine bounce + 1 shadow ray | cheapest; converges fastest |
| Metal / plastic | about baseline; GGX sample instead of cosine | rough metal converges slower; mirror metal is cheap but adds coherent bounces |
| Glass | 2+ extra rays per glass object crossed (enter, exit, Fresnel split) | skip internal faces between same-material voxels; caustics stay noisy; needs about 4-8 max bounces |
| Emissive | cheap to hit; costly to sample as a light | small emitters are very noisy unless next-event estimation picks emissive voxels (needs a light list) |
| Cloud / SSS / media | highest: many steps per ray through the volume | defer past v1; v1 can load and save them, but render them as diffuse or glass |

- Recommended v1 tracer: diffuse, metal (GGX), glass (dielectric with attenuation), emissive (with a light list for NEE), sky and sun. That covers the four `MATL` types. Cloud, SSS and blend can come later.

## 4. Accumulation and resets

- **Static camera, no edits:** keep a running average (sum plus count) in an RGBA32F buffer. This is plain progressive rendering.
- **Camera moves:** two options.
  1. Reset and show the raw image, with the denoiser covering it. Simple and correct, and fine for a corner view.
  2. Reproject: find each pixel's primary-hit position from the previous frame, and reject the history on a depth or normal mismatch (disocclusion). SVGF does this and drops history on disocclusion ([Schied et al. 2017](https://cg.ivd.kit.edu/publications/2017/svgf/svgf_preprint.pdf)). A voxel tracer has done it per pixel in WGSL-like shading ([voxel-path-tracer](https://github.com/oskarbraten/voxel-path-tracer)).

  Recommendation: reproject during orbit, with the history length capped at about 16 frames so stale light fades. Do a full reset on large jumps such as framing with `.`.
- **Edits:** the edit tool knows the changed voxel box. Steps:
  1. Upload the changed bricks (a few KB) before tracing that frame.
  2. Reset pixels whose stored primary-hit voxel lies inside the dirty box, or whose primary ray now hits something different. Store a hit-voxel id per pixel to test this.
  3. Cap every other pixel's history at about 8 frames for a short while, because indirect light changes across the whole image.

  Research on reusing samples across scene changes exists ([Multi-layer reservoir splatting](https://graphics.cs.utah.edu/research/projects/multi-layer-restir/MultiLayerReservoirSplatting.pdf)). It is not needed for v1.
- **Palette recolor carries over.** The tracer reads a palette index per voxel and looks up a 256-entry palette buffer. A recolor is a 1 KB write with no remesh and no brick rebuild, the same trick as the prototype's palette texture ([decisions](../decisions.md)). The accumulated image does go stale. Accumulate lighting divided by the primary-hit albedo, then multiply the current palette color back in at display (albedo demodulation, as SVGF and the kiss3d denoiser do: [SVGF](https://cg.ivd.kit.edu/publications/2017/svgf/svgf_preprint.pdf), [kiss3d denoise.rs](https://docs.rs/crate/kiss3d/0.45.1/source/src/renderer/raytracer/denoise.rs)). With that, a recolor looks right instantly on directly seen surfaces, and only the color bleed reconverges. Material changes (glass, emit) still need a reset.

## 5. Denoising in wgpu / WGSL

| Option | Cost | Fit |
|---|---|---|
| Edge-aware A-trous (5x5 B-spline, 3-5 iterations, normal and luminance stops, albedo demodulated) | **estimate** 1-2 ms at 640x360 on M1 | best fit; a wgpu/WGSL implementation exists in [kiss3d](https://docs.rs/crate/kiss3d/0.45.1/source/src/renderer/raytracer/denoise.rs) (BSD-3, about 1.8k stars) |
| SVGF (temporal variance + A-trous) | 4.4 ms at 1280x720 on a TITAN X Pascal; 10 ms at 1080p ([paper](https://cg.ivd.kit.edu/publications/2017/svgf/svgf_preprint.pdf)) | **estimate** 5-8 ms at 640x360 on M1 (fewer TFLOPS, much less bandwidth). Built for 1 spp at every frame, which a preview that converges does not need |
| Simple bilateral (one pass) | **estimate** under 0.5 ms | ok as the phone fallback; blurs detail |
| OIDN (CPU, or Metal on macOS 13+) | seconds at 1080p on CPU (old report); no published Metal numbers | native only, no wasm; use it for final renders and export, not the live view ([OIDN](https://github.com/OpenImageDenoise/oidn), Apache-2.0; [Metal support](https://www.phoronix.com/news/Intel-OIDn-Metal-More-ARM64)) |

- No WGSL SVGF port turned up in searches. The CUDA and Vulkan versions ([Q2VKPT A-SVGF](https://github.com/cschied/q2vkpt)) map onto compute passes.
- Tune the filter to the sample count: strong filtering at 1-8 spp, then fade it out. By about 256 spp the raw accumulated image is cleaner than any filter output. This is an estimate, consistent with how MagicaVoxel's sample-count-driven viewer converges ([MagicaVoxel wiki](https://magicavoxel.fandom.com/wiki/Version?oldid=418)).
- For later: a TU Wien thesis gets real-time voxel GI from low-spp path tracing plus temporal/spatial resampling, a light denoiser and a custom TAA. It gives no hardware numbers ([Ott 2025](https://www.cg.tuwien.ac.at/research/publications/2025/ott-rgi/)).

## 6. Keeping editing responsive

- **No async compute.** WebGPU exposes one queue per device ([Arm WebGPU guide](https://learn.arm.com/learning-paths/mobile-graphics-and-gaming/android_webgpu_dawn/5-render-a-simple-3d-object-part-1/)). Multi-queue has been an open investigation since 2020, now at "Milestone 4+" ([gpuweb #1065](https://github.com/gpuweb/gpuweb/issues/1065)). wgpu's `request_device` returns one queue ([wgpu discussion #5525](https://github.com/gfx-rs/wgpu/discussions/5525)). On Metal, "You don't get async compute by using multiple queues" anyway ([gpuweb #1065](https://github.com/gpuweb/gpuweb/issues/1065)). The tracer shares the queue with the editor, and its work runs in order with the editor's.
- **Slicing is the only lever.** Each frame, encode the editor view first, then N tracer dispatches of fixed size (for example, one tile row block or one sample over a pixel subset). Then denoise and composite the corner. Tune N from measured frame time.
- **Measuring:** Chrome supports `timestamp-query`, quantized to 100 us ([Chrome docs](https://developer.chrome.com/docs/web-platform/webgpu/developer-features)). It is not Baseline across browsers ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/GPUQuerySet)). Use GPU timestamps natively, and in the browser fall back to CPU-side frame pacing (`requestAnimationFrame` deltas plus `onSubmittedWorkDone`).
- **Keep dispatches short.** Apple offers no supported way to cancel GPU work once it is running ([Apple forums](https://developer.apple.com/forums/thread/774819)). The macOS watchdog reportedly kills command buffers after about 5 s ([MLX #3302](https://github.com/ml-explore/mlx/issues/3302), anecdotal). Long tracer dispatches would stall the editor frame behind them, so cap each dispatch at about 1 ms (estimate).
- **Budget policy (start values, all estimates):**
  1. While the user is dragging a tool: 2-3 ms for the tracer at 1/2 or 1/4 preview resolution.
  2. During ordinary editing: 4 ms at the full 640x360.
  3. When idle for over 300 ms: 12 ms, until the target spp is reached, then stop tracing.
  4. When the corner is hidden or the tab is off screen: 0 ms (the prototype embed already renders only while on screen).
- Lower-resolution accumulation is the cheapest win. Pixel count drives cost linearly, and the corner view is small. Upscale with a bilinear or edge-aware blit.

## Open risks for Phase 2

1. No real numbers exist for M1-M4 or phones. The Phase 2 spike must measure Mrays/s, primary and bounce, on the brick map, natively and in Chrome and Safari.
2. Editing frame time must not drop when the 4 ms slice is on. That is the test 2 pass/fail.
3. Glass and emissive noise at low spp may look worse than MagicaVoxel until NEE and the light list exist.
4. Safari WebGPU timestamp support is unclear, so the browser budget controller may have to work blind.
5. The phone embed may need the Teardown-style cheap mode (AO + shadows + one bounce) instead of full path tracing.
