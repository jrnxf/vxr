# Engine and data structure

**Summary**

- Pick: a **sparse brick map** per object - a sparse grid of 8^3 bricks, each with a 512-bit occupancy mask and one `u8` palette index per voxel - ray marched with DDA in compute shaders, for both the editor view and the path tracer. No meshes.
- Why: one structure on the GPU serves both the editor view and the path tracer (WebGPU has no hardware ray tracing). A brush edit rewrites a few hundred bytes per brick it touches. Recolor is still a palette-table write. It traces about as fast as the best sparse trees in the benchmarks we found.
- Memory: about 1.1 to 2.3 B per voxel for solid, hand-made models (estimate). So 10M voxels takes about 23 MB and 100M about 230 MB. 1B (about 2.3 GB) fits a desktop GPU but not a default browser budget, so it needs the compacted-brick variant below.
- Rejected as the core: greedy meshes, because the path tracer needs a volume anyway. SVDAG/HashDAG, because colors take up most of the memory and painting breaks the sharing. ESVO, because it is slower and edits are harder.
- Phase 2 must measure ray-march FPS on integrated and phone GPUs, and how full bricks are in real `.vox` scenes.

## What the sources say

### Greedy meshes (what the prototype did)

| Fact | Number | Source |
|---|---|---|
| Prototype, 32^3 chunks, JS | 1 to 4 chunks per edit in 1 to 7 ms; 3.9M voxels meshed in 0.39 s | `docs/decisions.md` |
| Prototype triangle count | 145k voxels -> 32k tris; 3.9M voxels -> 595k tris in 248 draws | `docs/decisions.md` |
| Binary greedy meshing, 64^3 chunk (62^3 mesh), native C | 50 to 200 us per chunk, 74 us average single-threaded (Ryzen 3800X) | [cgerikj/binary-greedy-meshing](https://github.com/cgerikj/binary-greedy-meshing) |
| vengi (open-source voxel editor) | meshes regions of 62^3 by default with a cubic surface extractor | [vengi MeshState.cpp](https://github.com/vengi-voxel/vengi/blob/master/src/modules/voxel/MeshState.cpp) |
| Goxel | 16^3 blocks, copy-on-write, meshed for display | [Goxel INTERNALS.md](https://github.com/sariug/goxel/blob/master/INTERNALS.md), [goxel.h](https://github.com/guillaumechereau/goxel/blob/master/src/goxel.h) |
| Douglas Dwyer's Octo (Rust, wgpu, wasm + WebGPU) | moved from greedy meshing with LODs (0.2.0) to ray marching in compute shaders (0.5.0) | [octo-release README](https://github.com/DouglasDwyer/octo-release) |

- Mesh memory (estimate, from the prototype's triangle counts): with 4 vertices of 8 B plus 6 indices of 4 B per quad, that is about 56 B per quad, or 4 to 6 B per voxel. With vertex pulling (one 8 B record per quad), it drops to about 0.6 to 0.9 B per voxel. This comes on top of the voxel data itself.
- Edits are cheap: with 64^3 binary meshing, one edit costs well under 1 ms of meshing (estimate: 74 us native, times 1.5 to 2 in wasm according to `docs/plan.md`).
- The problem is the path tracer. WebGPU has no standard hardware ray tracing ([PlayCanvas forum](https://forum.playcanvas.com/t/realistic-fps-with-raytracing-capabilities/24856), [W3C GPU list, 2018](https://lists.w3.org/Archives/Public/public-gpu/2018Dec/0003.html)). wgpu's ray query is native-only and experimental ([wgpu docs](https://wgpu.rs/doc/wgpu/documentation/extensions/ray_tracing/index.html)). So secondary rays need a voxel structure on the GPU anyway, and meshes would mean keeping and editing two representations.
- Per-vertex AO is baked into meshes. A ray-marched view gets AO from the same rays the path tracer uses ([Octo](https://github.com/DouglasDwyer/octo-release) does AO, shadows and emissives this way).

### Sparse brick maps and ray-marched bricks

| Fact | Number | Source |
|---|---|---|
| Teardown storage | one 3D texture per object, 1 byte per voxel = palette index; 256-entry palette per object; 2 extra mips to skip 2^3 and 4^3 empty cells | [Wittens, Teardown Frame Teardown](https://acko.net/blog/teardown-frame-teardown/), [Voxagon blog](https://blog.voxagon.se/2020/12/03/spraycan.html) |
| Teardown render | draws each object's bounding box (36 vertices) and ray marches in the pixel shader | [Wittens](https://acko.net/blog/teardown-frame-teardown/) |
| Teardown shadow volume | 1-bit, whole level, 3504x200x3000 effective in about 262 MB | [Wittens](https://acko.net/blog/teardown-frame-teardown/) |
| XBrickMap cost | 8^3 bits per brick + 12 B per sector; edits about O(1) | [VoxelRT](https://github.com/dubiousconst282/VoxelRT) |
| XBrickMap speed (integrated GPU, primary rays) | 89 to 166 Mrays/s across 6 scenes; 19 iterations per ray on Sponza vs 392 for plain DDA | [VoxelRT](https://github.com/dubiousconst282/VoxelRT) |
| Octo speedups | DDA + bitmask filtering + low-res depth prepass halved frame time | [Dwyer, Voxel Devlog #18](https://www.youtube.com/watch?v=P2bGF6GPmfc) |

- Memory (estimate) for dense 8^3 `u8` bricks: 512 B payload + 64 B mask + about 8 B header = about 584 B per brick. That is 1.14 B per voxel at 100% fill, 2.3 B at 50% and 4.6 B at 25%. A uniform solid brick can collapse to its header (about 8 B), so solid interiors cost almost nothing.
- Variant (estimate): store only occupied voxels, indexed by popcount over the mask. This costs about (72 + n) B per brick, or about 1.6 B per voxel at 25% fill. The cost is an edit that resizes the brick's payload, handled with size-class pools. VoxelRT notes the same reallocation issue at sector level.
- Edit cost (estimate): one voxel edit re-uploads one brick, 584 B. A radius-16 sphere brush touches about 125 bricks, about 73 KB, through `queue.write_buffer`. That is well under 1 ms of CPU work, with no meshing.

### SVO (ESVO) and 64-trees

| Fact | Number | Source |
|---|---|---|
| ESVO node | 4 B | [Laine and Karras 2010](https://www.nvidia.com/docs/IO/88889/laine2010i3d_paper.pdf), [VoxelRT](https://github.com/dubiousconst282/VoxelRT) |
| Bistro, 358M voxels | ESVO 368 MB (1.02 B/voxel); 64-tree with 1 B material per voxel 224 MB (0.62 B/voxel) | [dubiousconst282, sparse 64-trees](https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/) |
| Speed (integrated GPU) | ESVO 57 to 137 Mrays/s; Tree64 93 to 209 Mrays/s | [VoxelRT](https://github.com/dubiousconst282/VoxelRT) |
| Edit | O(log n) path copying; "quite a bit more complicated" for compressed nodes | [VoxelRT](https://github.com/dubiousconst282/VoxelRT), [64-trees post](https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/) |
| Advice | many small trees under a top-level grid for big worlds; a state-of-the-art SVO measured up to 60% slower than simple hierarchical grids | [64-trees post](https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing/) |
| Avoyd (shipped editor) | octree with dedup on "defragment"; worlds up to 262144 a side; up to 65535 materials; GPU path tracer needs Vulkan 1.2 | [Avoyd FAQ](https://www.avoyd.com/faq.html) |
| voxelis (Rust SVO DAG crate) | 32^3 uniform fill: 5.17 ms as single ops, 23 us batched | [voxelis](https://github.com/WildPixelGames/voxelis) |

### SVDAG and HashDAG

| Fact | Number | Source |
|---|---|---|
| SVDAG, Epic Citadel 128K^3 | 19B voxels in 945 MB (geometry only; SVO 5.1 GB); 170 Mrays/s AO, 240 Mrays/s shadows on a GTX 680 | [Kämpe et al. 2013](https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf) |
| Best geometry rates | 0.08 bits/voxel (Kämpe thesis); SSVDAG 0.048 bits/voxel | [Kämpe thesis](https://research.chalmers.se/en/publication/240766), [SSVDAG](https://www.crs4.it/vic/data/papers/i3d2016-symmetry-dags.pdf) |
| Colors dominate | Aokana San Miguel 64K: geometry 2.2 GB vs colors 21 GB; Sponza 32K: 0.8 GB vs 19.7 GB | [Aokana 2025](https://arxiv.org/html/2505.02017v1) |
| HashDAG | interactive carve, fill, copy, paint on 128K^3 Citadel; needs 6 to 8 GB VRAM, CUDA | [Careil et al. 2020](https://diglib.eg.org/handle/10.1111/cgf13916), [HashDAG repo](https://github.com/Phyronnaz/HashDAG) |
| HashDAG colors | edited colors stay uncompressed, so memory grows during a session; live lossy compression adds about 13 ms (about 50%) per frame on an RTX 3070 Ti | [Molenaar and Eisemann 2023](https://diglib.eg.org/handle/10.1111/cgf14757) |
| Later GPU editing | original HashDAG limited by mutex locking and CPU-to-GPU copies; fixed with GPU hash tables | [Molenaar and Eisemann 2024](https://diglib.eg.org/handle/10.2312/pg20241310) |
| Aokana | about 6 ms/frame at about 10B voxels, but no runtime edits | [Aokana 2025](https://arxiv.org/html/2505.02017v1) |

- DAGs win when geometry repeats, at 10B voxels and up. vxr targets 1B at most, and its scenes are hand-painted, which breaks subtree sharing.

### Hybrids

- In a study of 40 formats, the best trade-off came from a raw grid on top with a DAG below, `R(N^3) G(M)`. More generally, flat grids at the top and compact structures at the bottom form the Pareto frontier. Grid levels that are too fine hurt sparse models ([Arbore et al. 2024](https://arxiv.org/abs/2410.14128)).
- The brick map pick follows the same shape: a sparse top grid, with dense or compacted bricks at the bottom. A DAG or 64-tree can later replace the brick level without changing the top level.

## Fit for vxr

| | Greedy mesh | Brick map | ESVO / 64-tree | SVDAG / HashDAG |
|---|---|---|---|---|
| Memory per voxel | 0.6 to 6 B mesh + volume (est.) | 1.1 to 2.3 B solid, 1.6 compacted (est.) | 0.6 to 1.0 B | well under 0.1 B geometry, colors extra |
| Brush edit | remesh 1 to 4 chunks, under 1 ms native | rewrite touched bricks, about 73 KB for r=16 (est.) | path copy, O(log n) | hash insert, colors grow |
| Render cost | scales with triangles | scales with pixels, 89 to 166 Mrays/s iGPU | 57 to 209 Mrays/s iGPU | 170 to 240 Mrays/s (GTX 680) |
| Path tracer | needs a second structure | same structure | same structure | same structure |
| Browser | fine | fine to about 300M voxels at default limits (est.) | fine | CUDA-only reference code |
| Palette recolor | palette texture (proven) | palette buffer, 1 B index | 1 B index in leaves | possible, but paint breaks sharing |

### Browser limits that shape the design

- WebGPU defaults: `maxBufferSize` 256 MiB, `maxStorageBufferBindingSize` **128 MiB** (not 256), `maxTextureDimension3D` 2048 ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits)). The adapter may grant more: 93% of Apple GPUs on macOS report at least 1 GiB per binding ([web3dsurvey](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize/platform/Mac%20OS)).
- Plan (estimate): split the brick pool into several storage buffers of 128 MiB or less, or keep it in an `r8uint` 3D texture atlas, which has no binding-size cap. Request higher limits when the adapter offers them.
- wasm32 caps the CPU-side copy at 4 GB (`docs/plan.md`). At about 2.3 B per voxel, the CPU copy plus the GPU pool puts the browser ceiling near 300M to 500M voxels (estimate). Compacted bricks and collapsed uniform bricks raise it.

### Palette recolor

- Teardown packs every palette into one texture indexed by a 1 B voxel value ([Voxagon](https://blog.voxagon.se/2020/12/03/spraycan.html)). The prototype did the same in its vertex shader (`docs/decisions.md`). With bricks, the shader reads `palette[index]` at hit time. A recolor rewrites one palette entry and resets path-tracer accumulation. No voxel data changes.

### Extras the brick map gives for free

- **Unlimited undo:** copy-on-write bricks keep a snapshot as cheap as the bricks that changed, as Goxel does with 16^3 blocks ([Goxel INTERNALS.md](https://github.com/sariug/goxel/blob/master/INTERNALS.md)). This fixes MagicaVoxel's capped undo.
- **Multi-object `.vox` scenes with rotations:** one brick map per object plus a transform, ray marched inside its bounds, the way Teardown draws each object's box ([Wittens](https://acko.net/blog/teardown-frame-teardown/)).

## The pick

- Each object is a sparse hash grid of 8^3 bricks on the CPU, and that copy is the source of truth. Each brick holds a 512-bit mask plus `u8` palette indices. A uniform brick collapses to one index.
- On the GPU: a brick pool (buffers or a 3D texture) plus a per-object top grid. A compute shader runs DDA over bricks, skips empty space with the masks, and does a low-res depth prepass. The editor view and the path tracer share this structure.
- Edits mark bricks dirty, and dirty bricks are uploaded on a per-frame budget, as the prototype did with chunks.
- Fallbacks: if ray marching is too slow on phones, the embed can greedy-mesh the same bricks, since 8^3 bricks group cleanly into 64^3 binary-meshing chunks. If memory is too high at 1B, swap in compacted bricks, then a 64-tree level.

## Open risks for Phase 2

1. Ray-march frame time at 1080p and Retina on Apple integrated GPUs and phones. All the Mrays/s figures above come from other hardware.
2. Real brick fill factor on public `.vox` scenes. It decides whether 2.3 B per voxel holds.
3. Edit latency end to end in wasm (CPU write, `write_buffer`, next frame) with a large brush.
4. Adapter limits granted in Safari and Firefox, compared with Chrome.
5. Whether ray-marched AO plus the path tracer matches the look of per-vertex AO while editing.
