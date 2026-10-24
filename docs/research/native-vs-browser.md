# Native vs browser speed

Summary:

- GPU-bound work (ray marching, path tracing, shading) runs at about the same speed in Chrome and Safari as native wgpu on the same Mac, since both end up on Metal. Estimate: within 1.0 to 1.15x.
- Single-threaded CPU work in wasm averages 1.45 to 1.55x slower than native on large C programs, peaks at 2.1 to 2.5x, and is near 1.0 to 1.15x on tight numeric loops. The 2019 data is x86 only, with no Safari numbers.
- The big CPU gap is threads, not codegen. The one-tag embed will usually run on one thread, so bulk work like meshing a whole scene will be about 5 to 10x slower than native on an 8 to 12 core Mac (estimate).
- The browser falls behind on GPU round trips (readback, many submits) and in Firefox, where each dispatch costs about 1 ms and a synced readback about 100 ms.
- The plan's line "wasm is roughly 1.5 to 2x slower" is about right for one thread but misses the bigger cost: threads, and GPU sync latency.

## 1. Check of the claim in plan.md

`docs/plan.md` says: "wasm is roughly 1.5 to 2x slower, threads are harder".

- The evidence backs "1.5x" as a mean for large single-threaded code: 1.55x in Chrome and 1.45x in Firefox on SPEC CPU ([Jangda et al., USENIX ATC 2019](https://www.usenix.org/conference/atc19/presentation/jangda)).
- "2x" is closer to the worst case than the typical one. Peaks were 2.5x in Chrome and 2.08x in Firefox, and 7 of 15 SPEC benchmarks came within 1.5x ([paper PDF](https://www.usenix.org/system/files/atc19-jangda.pdf)).
- Tight array kernels, which greedy meshing resembles, run closer to native: 1.00 to 1.15x in Chromium and 1.4 to 1.5x in Firefox for one Rust matrix workload ([dyna3 benchmarks](https://code.studioinfinity.org/StudioInfinity/dyna3/wiki/Language-benchmarks)). Seven PolyBench kernels came within 10% of native on V8 ([Jangda et al.](https://ar5iv.labs.arxiv.org/html/1901.09056), citing earlier work).
- "Threads are harder" undersells it. For the embed, threads are usually not available at all (section 3), which costs a multiple of the core count, not 1.5x.

Suggested rewording: "Single-threaded wasm is about 1.1 to 1.5x slower than native (up to 2.5x worst case). The bigger gaps are threads, which the embed usually cannot use, and GPU readback latency."

## 2. CPU: wasm vs native, one thread

| Source | Workload | Browser / runtime | Slowdown vs native |
|---|---|---|---|
| [Jangda 2019](https://www.usenix.org/conference/atc19/presentation/jangda) | SPEC CPU 2006/2017, x86-64 | Chrome 74 | 1.55x mean, 2.5x peak |
| [Jangda 2019](https://www.usenix.org/conference/atc19/presentation/jangda) | SPEC CPU, x86-64 | Firefox 66 | 1.45x mean, 2.08x peak |
| [dyna3 wiki](https://code.studioinfinity.org/StudioInfinity/dyna3/wiki/Language-benchmarks) | Rust matrix kernel | Chromium / Firefox | 1.00-1.15x / 1.4-1.5x |
| [Trifecta zlib-rs](https://trifectatech.org/blog/fastest-wasm-zlib/) | zlib decompress, SIMD128 vs SSE4.2 vs AVX2 | runtime not named | 294 ms vs 257 vs 218 ms (1.14x vs SSE, 1.35x vs AVX2) |
| [Frank Denis 2026](https://00f.net/2026/06/23/webassembly-runtimes-2026/) | libsodium crypto, x86-64 | Node (V8) | 7.95x (outlier: 64-bit multiply heavy, not a browser) |

Why wasm is slower, from the [Jangda counters](https://ar5iv.labs.arxiv.org/html/1901.09056):

- About 2x more loads and stores, from reserved registers and a weaker register allocator.
- About 1.7x more branches: stack-overflow checks per call, loop entry jumps, indirect-call checks.
- About 1.8x more instructions retired and 2 to 2.8x more L1 instruction-cache misses.
- Some of this is inherent to wasm (safety checks, reserved registers), some is fixable engine work.

Caveats:

- All the browser data is from 2019, x86-64, Chrome and Firefox. No peer-reviewed study since then compares Chrome, Firefox and Safari against native on the same workloads. I found none.
- Apple Silicon is arm64 with 31 general registers, so the register-pressure part of the gap should be smaller than on x86 (estimate, not measured).
- 32-bit wasm on a 64-bit host skips bounds checks entirely by reserving 4 GB of address space with guard pages ([SpiderMonkey](https://spidermonkey.dev/blog/2025/01/15/is-memory64-actually-worth-using.html)). Memory64, the only way past the 4 GB cap, brings explicit checks back and costs 10% to over 100% depending on workload (same source). So breaking the 4 GB cap in the browser has a speed price.
- Engines tier up: V8 runs a fast baseline compiler (Liftoff) first and swaps in optimized code later ([v8.dev](https://v8.dev/blog/liftoff)); Safari does the same with BBQ then OMG ([WebKit](https://webkit.org/blog/7691/webassembly/)). The first seconds after load can be slower than steady state, which matters for embed time to first frame.

### SIMD128

- wasm SIMD is fixed at 128 bits. Against 128-bit native SSE4.2 it was about 14% slower; against 256-bit AVX2 about 35% slower ([Trifecta](https://trifectatech.org/blog/fastest-wasm-zlib/)).
- Apple Silicon's NEON is also 128-bit ([Arm](https://developer.arm.com/Architectures/Neon)), so on a Mac wasm SIMD loses no width to native. Estimate: the SIMD gap on Mac is close to the SSE4.2 case, about 1.1 to 1.2x.
- SIMD over scalar wasm: 1.36x in one HPC benchmark ([Exploring Wasm in HPC](https://arxiv.org/pdf/2301.03982)), 1.7 to 4.5x for TensorFlow.js kernels ([InfoQ](https://www.infoq.com/articles/webassembly-simd-multithreading-performance-gains/)). Worth turning on (`-C target-feature=+simd128`); every current browser supports it.

## 3. CPU: threads

- wasm threads need `SharedArrayBuffer`, which needs the page to be cross-origin isolated (COOP and COEP headers) ([web.dev](https://web.dev/articles/webassembly-threads)). A host page that embeds vxr with one tag will rarely send those headers, so the embed should plan for one thread. The full editor on vxr's own site can set them.
- When threads work, they scale: wasm-bindgen-rayon's Mandelbrot demo went from 273 ms to 87 ms, about 3.1x ([wasm-bindgen-rayon](https://github.com/RReverser/wasm-bindgen-rayon)); Emscripten pthread benchmarks show 3.2 to 3.9x on four cores ([summary](https://expertbeacon.com/webassembly-threads-bringing-multi-threaded-performance-to-the-web/)) and good scaling on pure math ([web.dev, mimalloc and WasmFS](https://web.dev/articles/scaling-multithreaded-webassembly-applications)).
- Costs: Rust wasm threads still need nightly and `--target web` ([wasm-bindgen-rayon docs](https://docs.rs/wasm-bindgen-rayon)); the main thread cannot block on `Atomics.wait`, so locks there must spin ([wasm_safe_mutex](https://docs.rs/wasm_safe_mutex)); the default allocator scales badly (dlmalloc 2660 ms vs mimalloc 1466 ms in one test, [web.dev](https://web.dev/articles/scaling-multithreaded-webassembly-applications)).

## 4. GPU: browser WebGPU vs native wgpu

Shader time. Chrome (Dawn) and Safari both translate WGSL to MSL and run it on Metal, just as native wgpu does on macOS. The GPU does the same work. Differences:

- Browsers always inject shader bounds checks (robustness). Dawn measured about 11% on its robustness benchmark from removing one class of checks on D3D12 ([Dawn commit](https://dawn.googlesource.com/dawn/+/455e4b80f6b8d065660ced359bf836f72c656f33)). Native wgpu can turn them off for trusted shaders via `ShaderRuntimeChecks` ([wgpu docs](https://wgpu.rs/doc/wgpu/struct.ShaderRuntimeChecks.html)). A ray marcher indexes buffers on every step, so expect the browser to pay a few percent to about 15% here (estimate).
- For compute, the WebGPU gap to vendor-native APIs comes mostly from features WebGPU lacks (tensor cores, subgroups in some cases), not from the browser. On Apple GPUs, llama.cpp's Metal backend beat its WebGPU backend by about 2x in prefill and 1.5x in decode ([Llamas on the Web, 2026](https://arxiv.org/abs/2605.20706)). That is WebGPU vs hand-tuned Metal; vxr uses wgpu in both builds, so it does not pay this gap.

Per-call and dispatch overhead. From [Maczan 2026](https://arxiv.org/html/2604.02344), per-dispatch cost in microseconds. "Sequential" isolates the dispatch; "single-op" includes one GPU-to-CPU sync.

| Implementation | Hardware | Sequential | Single-op (with sync) |
|---|---|---|---|
| wgpu-native, Metal | Apple M2 | 71 | 48 |
| Safari 26.2 | Apple M2 | 32 | 248 |
| Firefox 147 | Apple M2 | 1039 | 103,490 |
| wgpu-native, Vulkan | RTX 5090 | 36 | 36 |
| Dawn native, Vulkan | RTX 5090 | 24 | 497 |
| Chrome, Vulkan (Linux) | RTX 5090 | 33 | 2071 |
| Chrome, D3D12 (Windows) | RTX 2000 laptop | 59 | 2729 |

Reading it:

- Chrome and Safari match native on throughput of queued GPU work. The cost shows up on every CPU-GPU round trip: about 0.25 ms in Safari, 2 to 3 ms in Chrome, versus 0.05 to 0.5 ms native.
- Firefox costs about 1 ms per dispatch and about 100 ms per synced op on every platform tested, which points at the browser, not the GPU (same paper). Mozilla's own notes for Firefox 141 list "unbuffered inter-process communication" and "latency from relying on interval timers" as known bottlenecks ([Mozilla Gfx via Simon Willison](https://simonwillison.net/2025/Jul/16/webgpu-firefox)). No Chrome-on-Mac row exists in the paper.
- JS call overhead per draw in Chrome was about 0.3 ms per 10K draws after a 40% cut ([Chrome 114](https://developer.chrome.com/blog/new-in-webgpu-114)). From wasm, every wgpu call also crosses into JS; render bundles exist to cut that cost ([VTK merge request](https://gitlab.kitware.com/vtk/vtk/-/merge_requests/10110)). At the prototype's 248 draws per frame this is negligible; at 10K+ draws it is a few ms per frame (estimate).

Uploads. In Chrome, WebGPU commands are serialized and sent to a separate GPU process ([dawn_wire](https://dawn.googlesource.com/dawn/+/HEAD/docs/dawn/overview.md)), so `writeBuffer` data from wasm memory takes at least one extra copy. An old 2022 test found `writeBuffer` 5 to 10x slower than WebGL for about 1M matrices ([Orillusion](https://forum.orillusion.com/post/69)); Chrome has since added a path that writes straight into host-visible buffers with no pending GPU work, skipping the extra copy ([Chrome 126 notes](https://developer.chrome.com/blog/new-in-webgpu-126)). I found no current GB/s number for any browser vs native. Small per-edit brick uploads (KB to a few MB) should not be noticed; whole-scene uploads of hundreds of MB on load are unmeasured.

Readback. `mapAsync` is async by design ([gpuweb minutes](https://github.com/gpuweb/gpuweb/wiki/Minutes-2018-01-24)). Safari has an open bug where `mapAsync` waits for unrelated command buffers ([WebKit 272804](https://bugs.webkit.org/show_bug.cgi?id=272804)). Combined with the single-op numbers above, any feature that reads GPU results back every frame (GPU picking, GPU-side meshing returned to CPU, progressive render readback) will cost 0.25 to 3 ms in Chrome and Safari and about 100 ms in Firefox.

## 5. Browsers compared on a Mac

| | Chrome | Safari | Firefox |
|---|---|---|---|
| WebGPU on macOS | since 113, Dawn on Metal ([web.dev](https://web.dev/blog/webgpu-supported-major-browsers)) | since Safari 26, Sept 2025 (same) | since 147 on Apple Silicon, Jan 2026 ([alternativeto](https://alternativeto.net/news/2026/1/firefox-147-brings-apple-silicon-webgpu-support-improved-video-playback-and-pip-and-more/)), wgpu inside |
| Dispatch overhead | near native (Linux/Windows data only) | near native on M2 | about 15x native |
| Synced round trip | 2-3 ms (Linux/Windows) | 0.25 ms on M2 | about 100 ms on M2 |
| wasm CPU speed | 1.55x mean (2019, x86) | no published data | 1.45x mean (2019, x86) |
| LLM decode, same app | 30-51 tok/s | 30-51 tok/s | 9-10 tok/s ([Maczan](https://arxiv.org/html/2604.02344)) |

Safari looks like the best browser target on Mac for GPU round trips. Its wasm CPU speed is unmeasured in any study I found.

## 6. What this means for vxr

All factors are estimates built from the sources above, for an Apple Silicon Mac.

| vxr workload | Browser vs desktop | Feel |
|---|---|---|
| Remesh 1-4 chunks after an edit (1-7 ms in the JS prototype) | 1.1-1.5x slower, one thread | Same. Stays inside a frame. |
| Mesh or brick-build a whole large scene on load | Editor with threads: 1.5-3x slower. Embed, one thread: 5-10x slower | Falls behind. A load that takes 1 s native may take 5-10 s in the embed. |
| GPU ray marching / progressive path tracing | 1.0-1.15x (bounds checks) | Same in Chrome and Safari. Firefox fine only if work is batched into few submits. |
| Upload edited bricks to the GPU | Extra copy through GPU process; small cost | Same for edits. Big initial uploads unmeasured. |
| Anything that reads GPU results back per frame (picking, stats) | 0.25-3 ms per round trip; Firefox about 100 ms | Falls behind; Firefox unusable. Do picking on the CPU against the sparse structure. |
| Scenes needing over 4 GB of CPU memory | Needs Memory64: 1.1-2x slower again, if it works at all | Falls behind. Desktop only, in practice. |

Design rules that follow:

- Keep the per-frame path GPU-resident: no per-frame `mapAsync`, few `submit` calls, indirect or bundled draws.
- Pick under the cursor with a CPU ray cast, never a GPU readback.
- Make meshing and brick building chunked and incremental, so one thread can keep up in the embed; parallelize with rayon on desktop and on the isolated editor page.
- Build wasm with `simd128` on; on Mac it costs nothing in width.

## 7. Open risks for Phase 2 (test 1, engine)

- No published Safari wasm CPU numbers, and no wasm vs native numbers on arm64. Measure the vxr mesher native, in Chrome and in Safari on the Mac.
- No Chrome-on-Mac dispatch or round-trip numbers. Measure submit and `mapAsync` latency in Chrome, Safari and Firefox.
- Upload throughput for large brick pools (100s of MB) in each browser is unmeasured.
- Whether the embed can get threads on a host page (COEP `credentialless`, or a cross-origin iframe) belongs to research item 2; if not, single-thread bulk build time sets the embed's scene size limit.
- Firefox's 1 ms per dispatch may improve (Mozilla named the IPC fix as planned work); recheck before deciding how far to support it.
