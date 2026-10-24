# wgpu and WebGPU limits

Research date: 2026-10-24. wgpu version checked: 30.0.1.

## Summary

- WebGPU defaults are 128 MiB per storage binding and 256 MiB per buffer, but nearly every real adapter offers more: ~92% of reports allow at least a 1 GiB storage binding. The exceptions are Android, where only ~36% do, and Linux. Request `adapter.limits()` and design voxel data as many bindings/bricks of 128 MiB or less, so the defaults are the floor and not a wall.
- The browser build should plan for a heap of 2 GB or less: wasm32 caps at 4 GB, Firefox WebGPU uploads break above a 2 GB heap, iOS Safari is reliable only to a few hundred MB, and memory64 is missing in Safari and slower everywhere.
- Threads with shared memory need cross-origin isolation, and a one-tag embed on someone else's page cannot get it (except via Chrome's Document-Isolation-Policy inside a vxr-hosted iframe). So the embed must be single-threaded, push heavy work to the GPU, and use message-passing workers for meshing.
- WebGPU now ships by default in Chrome/Edge (desktop and most Android), Safari 26 (macOS, iOS, iPadOS, visionOS) and Firefox (Windows, Apple Silicon Mac). Linux (only partly supported in Chrome, Nightly-only in Firefox) and older Android are the gaps.
- A WebGL2 fallback works only for a mesh-based viewer: no compute and no storage buffers. It is viable for the embed (greedy meshes) if we want it, not for the editor or the path-traced preview. Rerun and Ruffle ship WebGPU-first with WebGL2 fallback; Bevy picks one at build time.

## 1. Buffer and binding limits

### Defaults (what you get without asking)

| Limit | WebGPU default | wgpu `Limits::default()` | wgpu `downlevel_webgl2_defaults()` |
|---|---|---|---|
| maxStorageBufferBindingSize | 128 MiB | 128 MiB | 0 |
| maxBufferSize | 256 MiB | 256 MiB | 256 MiB |
| maxStorageBuffersPerShaderStage | 8 | 8 | 0 |
| maxUniformBufferBindingSize | 64 KiB | 64 KiB | 16 KiB |
| maxComputeWorkgroupStorageSize | 16 KiB | 16 KiB | 0 |
| maxComputeInvocationsPerWorkgroup | 256 | 256 | - |
| maxTextureDimension3D | 2048 | 2048 | 256 |

Sources: [MDN GPUSupportedLimits](https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits), [wgpu Limits docs](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html), [wgpu-types limits.rs v30.0.1](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-types/src/limits.rs).

Note: `docs/plan.md` says "256 MiB per binding". The binding default is 128 MiB; 256 MiB is the per-buffer default.

### What real adapters expose in browsers

From [web3dsurvey](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize) (share of reports that offer at least the value; no sample size or date given):

| Platform | >= 256 MiB | >= 1 GiB | ~2 GiB |
|---|---|---|---|
| All | 98% | 92% | 16% |
| Windows | 100% | 100% | 99% |
| macOS (all vendors) | 100% | 94% | 62% |
| macOS, Apple GPUs | 100% | 93% | 67% |
| Linux | 86% | 75% | 69% |
| Android | 75% | 36% | - |
| iOS | 100% | 81-82% | ~1% |

Sources: [overall and per platform](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize), [Mac OS](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize/platform/Mac%20OS), [Android](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize/platform/Android), [iOS](https://web3dsurvey.com/webgpu/limits/maxStorageBufferBindingSize/platform/iOS).

- maxBufferSize: 97% of all reports allow at least 1 GiB, and 79% allow ~2 GiB. iOS reaches 1 GiB in 81% of reports. Android reaches 1 GiB in 100% of reports, but most Android devices still bind only 128-512 MiB at a time. ([web3dsurvey maxBufferSize](https://web3dsurvey.com/webgpu/limits/maxBufferSize))
- Browser values top out near 2 GiB (2147483644 / 2147483647), even on GPUs that can do more. Browsers also round limits down to tiers to reduce fingerprinting. ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits), [web3dsurvey](https://web3dsurvey.com/webgpu/limits/maxBufferSize))
- WebGPU returned a working adapter in 83% of web3dsurvey reports: ChromeOS 92%, macOS 90%, Windows 88%, iOS 86%, Android 74%, Linux 17%. ([web3dsurvey](https://web3dsurvey.com/webgpu))

### Native (desktop wgpu)

- On Metal, wgpu sets `max_buffer_size` and `max_storage_buffer_binding_size` both to `MTLDevice.maxBufferLength`. ([wgpu-hal metal/adapter.rs](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-hal/src/metal/adapter.rs))
- Apple only guarantees 256 MB for `maxBufferLength` ([Apple docs](https://developer.apple.com/documentation/metal/mtldevice/maxbufferlength)). Real values scale with RAM: one M4 Max report shows ~80 GiB ([MLX issue](https://upd.dev/ml-explore/mlx/issues/3082)). Estimate: an M-series Mac with 16 GB likely exposes several GiB per buffer. Measure this in Phase 2.
- So the desktop build can have one buffer far larger than the browser's ~2 GiB cap.

### How to request more

- WebGPU: read `adapter.limits` and pass the values you need as `requiredLimits` to `requestDevice()`. A device enforces exactly what it was created with. Requests above what the adapter supports fail. ([W3C WebGPU spec](https://www.w3.org/TR/webgpu/))
- wgpu: start from `Limits::default()` (or `downlevel_defaults()`), raise with `adapter.limits()` through `or_better_values_from` / `or_worse_values_from` to clamp to what the adapter has, check with `check_limits`. Requesting more than the adapter supports panics. ([wgpu Limits docs](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html))
- Chrome 146 (beta notes published 2026-02-25) shipped WebGPU compatibility mode (`featureLevel: "compatibility"`) on OpenGL ES 3.1 Android. In compat mode, vertex shaders get 0 storage buffers (true on ~45% of devices), fragment shaders get 4, and workgroups are capped at 128 invocations. ([Chrome blog](https://developer.chrome.com/blog/new-in-webgpu-146), [WebGPU Fundamentals](https://webgpufundamentals.org/webgpu/lessons/webgpu-compatibility-mode.html)). wgpu does not support compat mode yet: [gfx-rs/wgpu#8124](https://github.com/gfx-rs/wgpu/issues/8124) is open.

### What this means for voxel data (estimates)

- Estimate: 100M voxels at 1 byte of palette index each come to ~100 MB raw, which fits one 128 MiB binding before brick overhead. 1B voxels (~1 GB) do not fit the default, so data must be split across bindings.
- 8 storage buffers per stage x 128 MiB = 1 GiB addressable per shader at defaults, if the data is split into pages. Splitting into pages also keeps Android and iOS working.
- 3D textures are a second route: 2048^3 is allowed by default, and a brick atlas in an `r8uint` 3D texture is bounded by memory, not by the buffer binding limit. Worth testing in Phase 2 engine spike.

## 2. wasm32 memory

- wasm32 can address at most 4 GB. Chrome has allowed the full 4 GB since M83 (2020); before that it allowed 2 GB, and going over 2 GB still has to be enabled at build time. ([V8 blog](https://v8.dev/blog/4gb-wasm-memory))
- Firefox: when the wasm heap is larger than 2 GB, WebGPU `queue.writeBuffer()` / `writeTexture()` throw with views from that heap. The bug is still open (NEW). So Firefox users effectively get a 2 GB cap, for both 4 GB wasm32 and wasm64. ([Bugzilla 2022805](https://bugzilla.mozilla.org/show_bug.cgi?id=2022805))
- iOS Safari: memory available to wasm is unreliable. In reports, a first load gets ~1 GB, reloads get 300-500 MB (one report saw 28 MB), and a failed `memory.grow` kills the tab instead of returning an error. These reports cover iOS 17. They are tracked in a WebKit "MegaBug" and no fix was found. ([WebKit 269777](https://bugs.webkit.org/show_bug.cgi?id=269777), [WebKit 221530](https://bugs.webkit.org/show_bug.cgi?id=221530), [WebAssembly design #1397](https://github.com/WebAssembly/design/issues/1397))
- Shipped apps hit the cap: Rerun's web viewer warns that "the Wasm32 build only has 4GiB of memory" and tells users to switch to the native viewer. ([Rerun source](https://github.com/rerun-io/rerun/blob/main/crates/store_app/re_entity_db/src/rrd_manifest_index/chunk_prioritizer.rs))

### memory64

| Engine | Status |
|---|---|
| Chrome | Shipped in 133 |
| Firefox | Shipped in 134 |
| Safari | Behind a flag (Technology Preview 251); [WebKit 300538](https://bugs.webkit.org/show_bug.cgi?id=300538) still NEW |

Sources: [WebAssembly feature table data](https://github.com/WebAssembly/website/blob/main/features.json), [SpiderMonkey blog](https://spidermonkey.dev/blog/2025/01/15/is-memory64-actually-worth-using.html).

- Browsers cap memory64 at 16 GB through the JS API, and it runs 10% to over 100% slower than wasm32 because bounds checks cannot be elided. ([SpiderMonkey blog](https://spidermonkey.dev/blog/2025/01/15/is-memory64-actually-worth-using.html))
- Rust: `wasm64-unknown-unknown` is Tier 3 and ships no prebuilt std, so it needs `build-std` (nightly). ([rustc platform docs](https://doc.rust-lang.org/rustc/platform-support/wasm64-unknown-unknown.html))
- wasm-bindgen: memory64 support merged 2026-04-27 ([PR #5004](https://github.com/wasm-bindgen/wasm-bindgen/pull/5004), released in 0.2.119). Threads on memory64 merged 2026-10-15 but are not in a release yet ([PR #5378](https://github.com/wasm-bindgen/wasm-bindgen/pull/5378)). Whether wgpu works on wasm64 is unknown; nothing found.
- Verdict: memory64 is not a v1 option. Missing in Safari, slow, nightly-only in Rust, and Firefox's 2 GB WebGPU bug applies anyway.

## 3. Threads in wasm and the one-tag embed

- Threads in wasm (shared memory + atomics) work in all three engines: Chrome 74, Firefox 79, Safari 14.1 / iOS 14.5. They need `SharedArrayBuffer`. ([features.json](https://github.com/WebAssembly/website/blob/main/features.json))
- `SharedArrayBuffer` needs `crossOriginIsolated`: the top-level document must send `COOP: same-origin` plus `COEP: require-corp` (or `credentialless`, which only Chromium has). Every document in the frame chain must also be isolated, and an iframe needs `allow="cross-origin-isolated"`. ([web.dev COOP/COEP](https://web.dev/articles/coop-coep), [MDN crossOriginIsolated](https://developer.mozilla.org/en-US/docs/Web/API/Window/crossOriginIsolated))
- For an embed on someone else's page, this means:
  - We do not control the host page's headers, and COEP would break the host's own third-party embeds, ads and images. An inline `<vxr-scene>` can therefore never count on threads.
  - An iframe served from a vxr origin with COOP/COEP is still not isolated unless the host page is isolated too and delegates the permission. The TAG review notes that this makes SharedArrayBuffer in third-party iframes "very unreliable". ([TAG review #995](https://github.com/w3ctag/design-reviews/issues/995))
- Chrome's Document-Isolation-Policy (DIP) fixes this per frame: a vxr-hosted iframe that sends `Document-Isolation-Policy: isolate-and-credentialless` gets SharedArrayBuffer whatever the host does. DIP shipped on desktop in Chrome 137, and Android was planned for later. The TAG closed its review on 2025-05-26 as "satisfied". Mozilla and WebKit had given no signal by the last record found. ([Chrome blog](https://developer.chrome.com/blog/document-isolation-policy), [TAG review](https://github.com/w3ctag/design-reviews/issues/995), [Mozilla position #1074](https://github.com/mozilla/standards-positions/issues/1074), [WebKit position #399](https://github.com/WebKit/standards-positions/issues/399))
- Shipped Rust engines run single-threaded on the web: Bevy's wasm multithreading issue is still open and on hold ([bevy#4078](https://github.com/bevyengine/bevy/issues/4078)). wgpu marks its web objects as not shareable between threads, and its `fragile-send-sync-non-atomic-wasm` feature only papers over that. ([wgpu Cargo.toml](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu/Cargo.toml))

### Workarounds, best first

1. **Do the heavy work on the GPU.** Use compute shaders for meshing or brick updates, and ray-march bricks directly so no CPU remesh is needed. WebGPU needs no isolation.
2. **Single-threaded main module with a time budget.** The prototype already remeshes dirty chunks on a per-frame budget (1-7 ms per edit, in JS).
3. **Workers without shared memory.** Run separate wasm instances in workers, and send chunk data in and meshes back through transferable `ArrayBuffer`s via `postMessage`. Works everywhere with no headers. Costs a copy per chunk (estimate: small for 32^3 chunks of 32 KB each).
4. **Optional threads where available.** Detect `crossOriginIsolated` (vxr's own site with COOP/COEP, or a DIP iframe in Chrome) and switch to a rayon pool there. Two builds (atomics and non-atomics) roughly double build and test work.
5. **Move the whole renderer into a worker** with OffscreenCanvas, so the host page's main thread stays free. Run this in a Phase 2 spike: whether WebGPU in workers works in every target browser is unverified.

## 4. WebGPU status by browser (as of 2026-10-24)

| Browser | Windows | macOS | Linux | Android | iOS / iPadOS |
|---|---|---|---|---|---|
| Chrome / Edge | 113 (x64); ARM64 behind a flag | 113 | 144 on Intel Gen12+; 147 on NVIDIA 535.183+ under Wayland; others behind a flag | 121 on Android 12+ with ARM / Qualcomm / Intel GPUs; 139 on Imagination with Android 16+; Samsung Xclipse TBD | uses WebKit |
| Safari | - | 26 (macOS Tahoe) | - | - | 26 |
| Firefox | 141 | 145 (Apple Silicon, macOS 26+); 147 on all macOS versions (Apple Silicon); Intel Macs TBD | Nightly only, ship "expected in 2026" | behind a flag | uses WebKit |

Source for the table: [gpuweb Implementation Status wiki](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status) (last edited 2026-10-02).

Dates:
- Chrome 113: first stable WebGPU, May 2023. ([Chrome blog](https://developer.chrome.com/blog/webgpu-release))
- Chrome 144 (Linux): announced around 2026-01-07. ([CodeZine](https://codezine.jp/news/detail/22956), [Chrome WebGPU news](https://developer.chrome.com/docs/web-platform/webgpu/news))
- Safari 26: 2025-09-15. ([WebKit blog](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/))
- Firefox 141: announced 2025-07-15. Firefox's WebGPU is built on wgpu itself. ([Mozilla gfx blog](https://mozillagfx.wordpress.com/2025/07/15/shipping-webgpu-on-windows-in-firefox-141/))
- Firefox 147 (all macOS on Apple Silicon): 2026-01-13. ([Firefox 147 notes](https://www.firefox.com/firefox/147.0/releasenotes/))
- Estimate: Chrome 121 shipped ~January 2024 and Chrome 147 ~April 2026, from Chrome's 4-week release cadence. Not checked against a release page.

Gaps that matter for vxr:
- Linux: only 17% of web3dsurvey Linux reports got a working adapter. Linux users should get the desktop build.
- Android: works on Android 12+ with common GPUs. Older and low-end devices need compat mode, which wgpu does not support yet.
- Firefox on Linux and Android: not shipped.

## 5. Is a WebGL2 fallback in wgpu viable?

- wgpu has a WebGL2 backend (via wgpu-core + GLES). Since v0.19 (January 2024), one wasm binary can include both backends and pick at `Instance` creation. ([wgpu v0.19.0 release](https://github.com/gfx-rs/wgpu/releases/tag/v0.19.0))
- WebGL2 lacks `COMPUTE_SHADERS` and `INDIRECT_EXECUTION` ([DownlevelFlags](https://docs.rs/wgpu/latest/wgpu/struct.DownlevelFlags.html)). Its limits: 0 storage buffers, 0 workgroup memory, 16 KiB uniform bindings, 256^3 3D textures ([wgpu-types limits.rs](https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-types/src/limits.rs)).
- Under memory pressure, wgpu on WebGL2 panics in `Queue::submit`, while WebGPU keeps running. ([gfx-rs/wgpu#5545](https://github.com/gfx-rs/wgpu/issues/5545))
- Verdict:
  - **Editor and live path-traced preview:** not viable. They need storage buffers and compute (DDA ray marching over bricks).
  - **Embed viewer:** viable only as a second render path. It would draw CPU-built greedy meshes with vertex buffers and a palette texture (the prototype's approach, which needs no storage buffers). That means writing and testing two renderers.
  - Estimate: with WebGPU in all major browsers, the fallback mainly serves Linux, older Android and Firefox Android. Decide after Phase 2 test 5 measures embed weight.

## 6. How shipped wgpu apps on the web cope

| App | Stars | Web backend | Threads on web | Memory |
|---|---|---|---|---|
| [Rerun](https://github.com/rerun-io/rerun) | ~11.6k | WebGPU first, WebGL fallback picked automatically; `?renderer=webgl` override ([troubleshooting](https://github.com/rerun-io/rerun/blob/main/docs/content/getting-started/install-rerun/troubleshooting.md)) | single-threaded (no isolation found in source) | caps memory and evicts data; tells users the wasm32 build has 4 GiB ([source](https://github.com/rerun-io/rerun/blob/main/crates/store_app/re_entity_db/src/rrd_manifest_index/chunk_prioritizer.rs)) |
| [Ruffle](https://github.com/ruffle-rs/ruffle) | ~18.7k | default is wgpu-on-WebGL2; WebGPU listed as "in development"; plain WebGL and Canvas as further fallbacks ([config](https://ruffle.rs/js-docs/master/enums/Config.RenderBackend.html)) | single-threaded | small content, not an issue |
| [Bevy](https://github.com/bevyengine/bevy) 0.20 | ~49k | `webgl2` on by default; the `webgpu` feature overrides it at build time, so one binary does one backend ([Cargo.toml](https://github.com/bevyengine/bevy/blob/main/Cargo.toml)) | single-threaded; tracking issue open since 2022 ([#4078](https://github.com/bevyengine/bevy/issues/4078)) | n/a |
| Firefox itself | - | uses wgpu as its WebGPU implementation ([Mozilla](https://mozillagfx.wordpress.com/2025/07/15/shipping-webgpu-on-windows-in-firefox-141/)) | - | - |

Star counts from the GitHub API on 2026-10-24.

What they have in common: none of them uses wasm threads on the web, all treat memory as capped and fall back to native for big data, and the two mature ones (Rerun, Ruffle) keep a WebGL path.

## What this forces on vxr

1. Data layout: pages or bricks of 128 MiB or less in several bindings (or a 3D texture atlas), with limits raised from `adapter.limits()` when available. Never one giant buffer.
2. Browser heap budget: plan for 2 GB or less on desktop browsers and a few hundred MB on iOS. Make the embed stream or decimate, and send big scenes to desktop.
3. The embed is single-threaded. Heavy work goes to the GPU or to message-passing workers. Threads are an optional speed-up on vxr's own isolated site.
4. Editor and preview require WebGPU. A WebGL2 path is an embed-only option to decide later.
5. Linux browser users and old Android are second-class. The desktop build covers Linux.

## Open risks for Phase 2

- The real `maxBufferLength` and per-binding limits on the test Mac, native vs Chrome vs Safari, are unmeasured.
- iOS Safari memory: measure the largest scene the embed survives across reloads.
- Whether a GPU-only edit path (compute meshing or direct brick ray marching) keeps edit latency low enough single-threaded.
- WebGPU in a worker with OffscreenCanvas across Chrome, Safari 26 and Firefox: unverified.
- Firefox 2 GB heap bug ([2022805](https://bugzilla.mozilla.org/show_bug.cgi?id=2022805)): track it, since it constrains every wasm heap design.
