# Embed weight

**Summary**

- A WebGPU-only wgpu viewer is small: the wgpu wrapper itself is near zero on the web, and one public measurement puts a WebGPU-only wgpu app at 37 KB brotli. The WebGL2 fallback is what is heavy (+1 to 1.7 MB raw, about +1 MB brotli), because it pulls in wgpu-core and naga.
- Target for the `<voxel-scene>` viewer: **150 KB brotli or less** for wasm + JS glue (hard ceiling 300 KB), which lands between three.js core (154 KB br) and model-viewer (229 KB br). Estimate.
- TTFF budget on a mid-range phone: **1.0 s** from "embed is visible" to first lit frame on a 10 Mbit/s link, **2.5 s** on Lighthouse "Slow 4G", poster image shown within 100 ms. Estimate.
- Biggest levers, in order: WebGPU-only build (no `webgl` feature), no fonts/UI/assets in the binary, `opt-level`/LTO/`wasm-opt -Oz`, strip names, brotli q11. Allocator swaps are worth about 10 KB at most.
- Ship WebGL2 as a separate, lazily fetched module (or just the poster) for the ~14% of browsers without WebGPU.

Measured on 2026-10-24 unless noted. "Measured here" means: the published file was downloaded from its official host or npm CDN and compressed locally with `gzip -9` and `brotli -q 11`; wasm sections were split with a small script. No builds were made.

## 1. Real bundle sizes

### Rust/wgpu and other wasm apps (measured here)

| App | What it is | Raw | gzip -9 | brotli 11 | Notes |
|---|---|---|---|---|---|
| wgpu examples, WebGPU build ([wgpu.rs/examples](https://wgpu.rs/examples/)) | ~30 examples in one module, WebGPU backend | 7.7 MB | 2.7 MB | 2.1 MB | Code section only 2.1 MB raw / 440 KB br; 3.8 MB is embedded example data, 1.7 MB is the name section |
| wgpu examples, WebGL2 build (same site) | Same examples, GL backend | 16.9 MB | 5.1 MB | 3.8 MB | Code section 7.7 MB raw / 1.5 MB br: the GL path adds ~5.6 MB raw / ~1 MB br of code |
| egui demo ([egui.rs](https://www.egui.rs/)) | Full widget gallery, eframe | 11.2 MB | 4.2 MB | 2.9 MB | Code 8.8 MB, data 2.3 MB (fonts); JS glue 20 KB br |
| Bevy `3d_scene`, WebGPU ([bevy.org examples](https://bevy.org/examples-webgpu/3d-rendering/3d-scene/)) | Default Bevy features | 22.9 MB | 7.2 MB | 5.0 MB | Example builds are not size-tuned |
| Bevy `2d_shapes`, WebGPU (same) | Default Bevy features | 26.3 MB | 8.5 MB | 5.7 MB | A "minimal" Bevy app is still MB-class |
| Ruffle 0.7.1 ([npm](https://www.npmjs.com/package/@ruffle-rs/ruffle)) | Flash player, wgpu renderer | 13.6 MB | 4.8 MB | 3.3 MB | Two variants (with and without newer wasm extensions); one loads. jsDelivr serves it at 3.95 MB br, 18% bigger than brotli 11 |
| Rerun web viewer 0.40 ([npm](https://www.npmjs.com/package/@rerun-io/web-viewer)) | wgpu + egui data viewer | 52 MB | 16 MB | 12.2 MB (q9) | Includes a 9 MB name section |
| Spline `hana-ui.wasm` ([npm](https://www.npmjs.com/package/@splinetool/runtime)) | One of Spline's wasm modules | 3.2 MB | 1.0 MB | 787 KB | Which of Spline's wasm modules load for a given scene was not checked |

Published measurements by others:

- WebGPU-only wgpu (`default-features = false, features = ["webgpu", "wgsl"]`): **37 KB brotli** for a small grid benchmark. The same code with wgpu's WebGL2 fallback: **561 KB brotli (1.77 MB raw)**, about 1.7 MB of which is the GL backend. First frame at 20 Mbit/40 ms: 114 ms WebGPU-only vs 328 ms with the fallback built in; three.js 151 ms. Single unreviewed source. [openagents#12237](https://github.com/OpenAgentsInc/openagents/issues/12237)
- egui demo: 5.9 MiB on glow vs 8.3 MiB on wgpu with WebGL (+2.4 MiB), blamed on naga transpiling WGSL to GLSL. An almost empty project went 4.29 MB to 8.82 MB. [egui#5889](https://github.com/emilk/egui/issues/5889)
- A wgpu maintainer: "if you are building for web and only include the webgpu backend the size should drop to near-zero." Native cargo-bloat of the examples: wgpu_core 1.9 MiB, naga 1.6 MiB, wgpu_hal 0.6 MiB. [wgpu#9075](https://github.com/gfx-rs/wgpu/issues/9075)
- Why: in wgpu's [Cargo.toml](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/Cargo.toml), `wgpu-core` is optional and only the `webgl` feature pulls it in on wasm; the `webgpu` backend calls the browser's WebGPU directly. `spirv` and `glsl` input features pull in naga frontends. WGSL straight to the browser needs neither.
- Dawn (C++) WebGPU init-only test under Emscripten: 24 KB raw, 9.5 KB gzip, 7.9 KB brotli. Shows the API binding itself is tiny. [Dawn commit](https://dawn.googlesource.com/dawn/+/73275c1cfcad51a4369e4db690b41c78452d5cdc%5E)

### JS baselines (measured here, from jsDelivr)

| Package | File | Raw | gzip -9 | brotli 11 |
|---|---|---|---|---|
| three 0.186.1 | `three.core.min.js` + `three.module.min.js` (WebGL renderer) | 789 KB | 188 KB | 154 KB |
| three 0.186.1 | + `three.webgpu.min.js` (instead of module) | 1.2 MB | 301 KB | 241 KB |
| @google/model-viewer 4.4.0 | `model-viewer.min.js` (three bundled) | 1.0 MB | 282 KB | 229 KB |
| @google/model-viewer 4.4.0 | `model-viewer-module.min.js` (three as peer) | 464 KB | 140 KB | 116 KB |
| @splinetool/viewer 2.0.111 | `spline-viewer.js` | 3.4 MB | 971 KB | 772 KB (+ wasm modules on demand) |
| @splinetool/runtime 2.0.111 | `runtime.standalone.webgpu.min.js` | 1.7 MB | 486 KB | 391 KB |
| vox-viewer 1.4.0 | `vox-viewer.js` | 6 KB | 2 KB | 1 KB, but depends on three, lit and model-viewer ([npm](https://www.npmjs.com/package/vox-viewer)), so real weight is model-viewer class |

Takeaway: the embed market's bar is "about the size of three.js". A WebGPU-only Rust viewer can beat it; an egui- or Bevy-based viewer cannot (2.9 to 5+ MB brotli).

## 2. Shrinking tricks and their measured effect

| Trick | Measured effect | Source |
|---|---|---|
| WebGPU-only wgpu (no `webgl`) | 561 KB br -> 37 KB br on one app; ~1 MB br of code in the wgpu examples (measured here) | [openagents#12237](https://github.com/OpenAgentsInc/openagents/issues/12237), table above |
| WGSL only, no `spirv`/`glsl` features | Avoids naga frontends; naga is ~1.2 to 1.6 MiB native .text | [wgpu#9075](https://github.com/gfx-rs/wgpu/issues/9075) |
| `wasm-opt -Oz` | "another 15-20%" on code size | [Rust and WebAssembly book](https://rustwasm.github.io/book/reference/code-size.html) |
| Strip names/debug (`strip = true`, `wasm-opt --strip-debug`) | 42.7 KB -> 31.8 KB (-26%) on a small crate; name sections are 1.7 to 9 MB in the shipped apps above | [Rillence](https://rillence.com/articles/rust-wasm-bundle-size-cargo-browser/), measured here |
| `opt-level = "z"` / `"s"` | Not always smaller: on one small crate `s` was +2.2 KB and `z` +5.1 KB vs `opt-level = 3`. Measure both | [Rillence](https://rillence.com/articles/rust-wasm-bundle-size-cargo-browser/), [book](https://rustwasm.github.io/book/reference/code-size.html) |
| LTO, `codegen-units = 1` | ~0 on a small crate; ~1 MB raw on Warp's large app | [Rillence](https://rillence.com/articles/rust-wasm-bundle-size-cargo-browser/), [Warp](https://www.warp.dev/blog/reducing-wasm-binary-size) |
| `panic = "abort"` | 0 bytes in one measurement; sources that claim more give no data. Note wasm32-unknown-unknown already aborts on panic | [Rillence](https://rillence.com/articles/rust-wasm-bundle-size-cargo-browser/) |
| Keep assets out of the binary | Warp: 21.4 MB to 8 MB (gzip) mostly by fetching images on the web instead of bundling | [Warp](https://www.warp.dev/blog/reducing-wasm-binary-size) |
| Smaller allocator | dlmalloc is "around ten kilobytes"; wee_alloc saves most of that but is unmaintained (RUSTSEC-2022-0054, repo archived 2025), advisory recommends the default allocator; talc is general-purpose, no wasm size data found | [book](https://rustwasm.github.io/book/reference/code-size.html), [RUSTSEC-2022-0054](https://rustsec.org/advisories/RUSTSEC-2022-0054) |
| Avoid `core::fmt` and panicking paths | In the book's twiggy example, `fmt` and panic machinery are among the top items of a 47 KB module | [book](https://rustwasm.github.io/book/reference/code-size.html) |
| Brotli 11 vs CDN default | Ruffle: jsDelivr's brotli 3.95 MB vs 3.34 MB at q11 (-15%), measured here. Brotli is ~25 to 35% smaller than gzip -9 across all files above | measured here |

Tooling: `twiggy top` / `twiggy dominators` on the final `.wasm` to see what dominates ([book](https://rustwasm.github.io/book/reference/code-size.html)). Glue: wasm-bindgen JS for the WebGPU bindings was 15 KB br for the wgpu examples and 20 KB br for egui (measured here), so budget ~15 to 20 KB for it, or trim with only the `web-sys` features used.

Ideas not measured: `-Zbuild-std` with `panic_immediate_abort` (nightly, often cited as the next big cut), and avoiding `serde`/`nom`-style generic-heavy crates in the `.vox` parser.

## 3. Time to first frame

There is little published phone data. What exists:

- **Network dominates on phones.** Lighthouse "Slow 4G" is 150 ms RTT, 1.6 Mbit/s down, plus a 4x CPU slowdown ([Lighthouse throttling](https://github.com/GoogleChrome/lighthouse/blob/main/docs/throttling.md)). At that speed 150 KB takes ~0.75 s, 3 MB takes ~15 s (arithmetic).
- **No shared CDN cache.** Chrome (since 86), Safari and Firefox partition the HTTP cache by top-level site, so a visitor pays the download again on every host site, even from jsDelivr ([Chrome blog](https://developer.chrome.com/blog/http-cache-partitioning)). Size matters per site, not once.
- **Wasm compile is fast and overlaps download.** V8's Liftoff baseline compiles "tens of megabytes per second" on desktop and streams while downloading ([V8 pipeline](https://v8.dev/docs/wasm-compilation-pipeline)); Firefox's baseline did 30 to 60 MB/s on desktop ([Mozilla](https://hacks.mozilla.org/?p=31712)). Lowest tiers run ~50 ns per code byte per core on a fast desktop ([wingolog](https://wingolog.org/archives/2020/04/14/understanding-webassembly-code-generation-throughput)). Estimate: a 500 KB raw module compiles in ~10 to 100 ms on a mid phone. JSC's BBQ is slower than V8's or SpiderMonkey's baseline (same source); no iPhone numbers found.
- **Code caching** only applies to `compileStreaming`/`instantiateStreaming`, and only for modules of at least 128 KB ([V8 code caching](https://v8.dev/blog/wasm-code-caching)). Serve as `application/wasm` (jsDelivr does, with brotli and `immutable`, checked here).
- **WebGPU device acquisition is the hidden cost.** Desktop Chrome on Linux: `requestAdapter` ~60 ms + `requestDevice` ~125 ms, ~240 ms to renderer ready vs ~30 ms for WebGL; first frame then waits on synchronous pipeline compilation ([luxar#3172](https://github.com/royerlab/luxar/issues/3172)). Another project: median 27 ms idle, 94 ms median and 309 ms max under load ([underglass#203](https://github.com/RaananW/underglass/pull/203)). Phone numbers: none found.
- **Shader cost**: cold vs warm WGSL compilation differs a lot; no ms figures published for phones ([Hugo Daniel](https://hugodaniel.com/posts/webgpu-diagnostics/)). vxr's viewer needs ~1 to 3 small pipelines, so this should be small, but must be measured.

### Budget for vxr (estimate)

| Stage | Mid phone, 10 Mbit/s, 4G RTT | Slow 4G (Lighthouse) |
|---|---|---|
| Poster image painted (inline or tiny WebP) | < 100 ms after element upgrade | < 300 ms |
| Fetch wasm + glue (150 KB br) + `.vox` (~50 KB) | ~350 ms | ~1.5 s |
| Compile (streams with fetch) | ~0 to 100 ms extra | hidden by download |
| `requestAdapter` + `requestDevice` (start in parallel with fetch) | 100 to 300 ms, overlapped | overlapped |
| Mesh + pipelines + first frame | 50 to 150 ms | 100 to 300 ms |
| **First lit frame after visible** | **<= 1.0 s** | **<= 2.5 s** |

## 4. Lazy loading

- **Poster first.** model-viewer shows a `poster` image (or `slot="poster"` element) until the model is revealed; `loading="auto"` equals `lazy`, which loads "when it is near the viewport"; `reveal="manual"` waits for a call ([model-viewer docs](https://modelviewer.dev/docs/)). vxr should copy this API shape: `poster`, `loading`, `reveal`.
- **Load on visibility** with IntersectionObserver (97% global support, [caniuse data](https://github.com/Fyrd/caniuse)), with a root margin so loading starts a screen before the embed scrolls in. The prototype already renders only while on screen (`docs/decisions.md`).
- **Start everything at once** when triggered: wasm fetch, `.vox` fetch and `navigator.gpu.requestAdapter()` in parallel; luxar's fix was exactly this ([luxar#3172](https://github.com/royerlab/luxar/issues/3172)). Use `createRenderPipelineAsync`.
- **One module per page.** Many `<voxel-scene>` tags on one page should share one wasm instance and one `GPUDevice`. Estimate: needed to avoid paying adapter and device setup per tag.
- **Fallback.** WebGPU has 85.8% full global support (plus 3.7% partial) per caniuse data on 2026-10-24 ([caniuse](https://github.com/Fyrd/caniuse)); Safari ships it on iOS 26 and macOS 26, Chrome on Android 12+ with Qualcomm/ARM GPUs, Firefox on Windows and Apple Silicon Mac only ([web.dev](https://web.dev/blog/webgpu-supported-major-browsers)). Without WebGPU, show the poster, then fetch a separate WebGL2 module only if the host opts in (expect ~600 KB to 1 MB br, estimate from the numbers above).

## Open risks for Phase 2

1. No public phone numbers for WebGPU adapter/device creation or first pipeline compile, on iOS Safari or Android Chrome. Measure on one iPhone and one mid Android.
2. Whether vxr's real viewer (wgpu + `.vox` parser + greedy mesher + orbit + wasm-bindgen) actually fits 150 KB br. Run twiggy on the spike.
3. If the viewer shares code with the editor, egui or other editor crates can leak in; the viewer must be its own crate and wasm target.
4. iOS wasm compile speed (JSC BBQ) and memory limits for larger scenes.
5. WebGL2 fallback cost and whether it is worth shipping at all versus poster only.
