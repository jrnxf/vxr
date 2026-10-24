# Test 5: embed weight

**Summary**

- **Size: pass with lots of room.** The whole one-tag embed (wasm + wasm-bindgen glue + the loader script) is **39.8 KB brotli** against a 150 KB target. About 110 KB is left for the real `.vox` codec, a brick-native scene format and a greedy-mesh fallback.
- **First lit frame: pass on a real iPhone, mid-range Android still pending.** On an iPhone 17 Pro in Safari, the first frame lands **217 ms** after the embed becomes visible at 10 Mbit/s (target 1.0 s) and **1.16 s** on Slow 4G (target 2.5 s). The desktop stand-ins agree: 270 ms and 1.2 s in Chrome with a 4x CPU slowdown. An iPhone 17 Pro is faster than the "mid-range phone" in the target, so the phone rows pass with that caveat until an Android run.
- **Poster: fails with the `poster` attribute, passes with an `<img>` child.** At 10 Mbit/s: iPhone 191 ms vs 70 ms, desktop stand-in 140 ms vs 62 ms. With the attribute, the poster cannot start loading until the loader script has arrived. An `<img>` inside the tag starts loading while the page is still being parsed.
- **Nothing on the iPhone is slow except the network.** wasm compile 1 ms, device 2 ms, pipeline compile 70 ms, parse and upload 2 ms, first GPU frame 2 ms.
- **The `.vox` file is now the heaviest part.** The 145k-voxel island is 137 KB brotli as `.vox`, 3.4x the whole viewer, and it causes most of the wait. The same voxels stored as bricks are **14.5 KB brotli** (measured). The embed should load a brick-native file and keep `.vox` as the fallback.
- **One shader trick matters.** Writing the image to an `rgba8unorm` storage texture added about 65 ms to cold pipeline compile in Chrome and Safari (34 ms in Firefox). Writing to a storage buffer instead cut desktop Chrome's cold first frame from about 130 ms to 55 ms. Tests 1 and 2 should check this too.

Machine: Apple M4 Pro, 16-core GPU, 24 GB, macOS 26. Browsers: Chrome 155, Safari 26.6.2, Firefox 158. Phone: iPhone 17 Pro, iOS 26.6.1, Safari. No Android phone was available, so the mid-range Android rows are **pending** and stay open for Phase 3.

## What was built

`spikes/embed/` is a standalone crate (`cdylib`, `wasm32-unknown-unknown`). It holds the viewer and nothing else:

- `src/vox.rs`: a minimal, bounds-checked `.vox` reader (`SIZE`, `XYZI`, `RGBA`, and `nTRN` translations through `nGRP`/`nSHP`; no rotations). Bad input returns an error code and never panics. It writes straight into a sparse brick map: 8^3 bricks, a 512-bit occupancy mask and one palette byte per voxel.
- `src/shader.wgsl`: a compute ray marcher using a two-level DDA (brick grid, then the voxel bitmask). It adds a sun light with hard shadows (a second ray through the same call site), sky ambient, and a blit pass to the canvas.
- `src/lib.rs`: wgpu 30.0.1 with `default-features = false, features = ["webgpu", "wgsl"]`, so the build is WebGPU-only with no naga or wgpu-core. One device is shared by every embed on the page. Errors are integer codes, so the code adds no formatting of its own.
- `web/voxel-scene.js` (the loader, 2.2 KB brotli): defines `<voxel-scene src poster alt autorotate>` with shadow DOM. It shows the poster at once and fetches the glue and wasm only when the element first becomes visible (IntersectionObserver). It draws only while on screen. Orbit is drag only, with `touch-action: pan-y` and no wheel handler, so the page always keeps its scroll. Without WebGPU, or on any error, the poster stays.

Checked in all three browsers (`results/lazy-*.jsonl`, `results/nogpu-*.jsonl`, `results/gpu-*.jsonl`):

- The wasm is not requested before the embed scrolls into view.
- The embed draws 0 frames per second while off screen and 60 (Safari) or 120 (Chrome, Firefox) while on screen.
- With `navigator.gpu` removed, the poster stays and neither the glue nor the wasm is requested. Screenshots: `shots/embed-live-chrome.png`, `shots/poster-fallback-chrome.png`.

## How it was measured

- `serve.py` serves the files on 127.0.0.1 with `Cache-Control: no-store` and brotli. It simulates the network itself, so all three browsers get the same link: one round trip per request, then the body paced through one shared link.
  - **10 Mbit/s:** 40 ms RTT.
  - **Slow 4G:** 150 ms RTT, 1.6 Mbit/s.
  - Safari sends no `br` over plain http, so the server paces its downloads by the brotli size, as a real https deploy would.
- Chrome also gets a 4x CPU slowdown (`emulate --cpu 4`) in both throttled rows, as the mid-phone stand-in. Safari and Firefox have no CPU throttle, so their throttled rows are network only.
- **10 cold loads per row.** "Cold" means: no HTTP cache; a random constant in the shader on every load so no shader cache can help; and no V8 wasm code cache, since that only applies to modules of 128 KB or more and ours is 86 KB. A warm shader cache cuts desktop Chrome's first frame from 55 ms to 16 ms.
- **iPhone runs** used the same `serve.py` and pages, served from a LAN machine behind a reverse proxy with a trusted certificate, so the phone got real HTTPS, HTTP/2 and brotli over its own Wi-Fi radio. Three copies of the server ran, one per network profile, and each page POSTed every run back (`results/ios-*.jsonl`). Safari on iOS reports timings in whole milliseconds. The iPhone canvas was 740x463 pixels.
- Every timed run on the Mac held `spikes/gpu-lock`. `bench-all.sh` runs the matrix, `probe-run.sh` the compile probe, and `summarize.py` prints the tables.
- Scene: `prototype/demo/scene.vox` (128x128x31, 145,064 voxels, v200), at 1440x900 canvas pixels (720x450 CSS at 2x).
- Each timing is measured from:
  - **poster:** from the parser reaching the tag to the frame after the poster image loads.
  - **first frame:** from the IntersectionObserver callback to the frame after the first frame's GPU work completes.
  - **split rows:** wait for the pipelines before parsing, so each step can be timed alone. The plain rows overlap the steps, as the real embed does.

## Size

All sizes are brotli q11 in bytes (1 KB = 1000 B).

| Build | wasm | wasm + glue + loader |
|---|---|---|
| opt-level z, no LTO, 16 codegen units, wasm-opt -Oz | 45,309 | 55,155 |
| opt-level z, LTO fat, 1 codegen unit, no wasm-opt | 35,979 | 45,825 |
| opt-level z, LTO fat, wasm-opt -Oz | 34,454 | 44,300 |
| opt-level 3, LTO fat, wasm-opt -O3 | 34,345 | 44,191 |
| **opt-level s, LTO fat, wasm-opt -O3 (final)** | **29,955** | **39,801** |

- **LTO is the biggest lever** (-11 KB). Next is `opt-level = "s"`, which beat `"z"` by 4.5 KB, as the research warned it might. wasm-opt is worth 1 to 2 KB; its level (-Oz, -Os, -O3) changes the brotli size by under 0.2 KB.
- `panic = "abort"` changes nothing: `wasm32-unknown-unknown` already aborts. `strip = true` saves 0.8 KB, because wasm-opt drops the name section anyway.
- **Final files:**

  | File | Raw | Brotli |
  |---|---|---|
  | wasm | 85,505 B | 29,955 B |
  | glue | 43,745 B | 7,597 B |
  | loader | 7,461 B | 2,249 B |
  | poster (`web/poster.webp`, 800x500, WebP q0.8) | - | 20,608 B |

  The poster is not part of the 150 KB budget.
- **Where the wasm goes** (twiggy, 89 KB of code and data before wasm-opt, full list in `results/twiggy-top.txt`):

  | Part | Size |
  |---|---|
  | Our code with the wgpu calls inlined into it | 34 KB |
  | Data segments (the shader source is 5.9 KB of it; it could be minified) | 15 KB |
  | wgpu, not inlined | 9.5 KB |
  | dlmalloc | 8.2 KB |
  | `core::fmt`, pulled in by wgpu's panics and `Debug` impls such as `TextureFormat` | 7.1 KB |
  | libm `sin`/`cos` for the camera (could move to JS) | 5.7 KB |
  | js-sys and wasm-bindgen | 4 KB |

- The glue is large for glue: 44 KB raw, 7.6 KB brotli. That is wgpu's WebGPU bindings, not our exports.

**Headroom.** 110 KB brotli is left under the 150 KB target. Rough costs of what still has to fit (estimates, not measured):

| Item | Estimated cost (brotli) |
|---|---|
| The full two-layer `.vox` codec | 15 to 30 KB |
| A brick-native scene format | 2 KB |
| A greedy-mesh fallback on WebGPU | 10 to 20 KB |
| MagicaVoxel's default palette | 1 KB |
| Error handling | a few KB |

That totals roughly 70 to 90 KB, which still fits.

A WebGL2 fallback built into the same module does not fit. The research put wgpu's WebGL2 backend at about 1 MB brotli.

## Timing

Medians of 10 cold loads, in ms. "first GPU" in the plain rows includes the pipeline compile, which lands on the first submit.

| Row | Poster after tag | First frame after visible (min-max) | wasm in | Scene file in | Device | Pipelines (split) | First GPU | wasm compile alone |
|---|---|---|---|---|---|---|---|---|
| Chrome, local | 7.7 | 56 (50-115) | 3 | 8 | 4.0 | 36 | 40 | 0.3 |
| Chrome, 10 Mbit/s, 4x CPU | 140 | 270 (261-303) | 114 | 214 | 10 | 40 | 42 | 0.9 |
| Chrome, 10 Mbit/s, 4x CPU, `<img>` child | 62 | 259 (248-268) | 104 | 201 | 9.8 | - | 42 | 0.9 |
| Chrome, Slow 4G, 4x CPU | 631 | 1200 (1190-1207) | 606 | 1146 | 9.6 | - | 40 | 1.1 |
| Chrome, Slow 4G, 4x CPU, `<img>` child | 272 | 1100 (1090-1106) | 506 | 1045 | 9.0 | - | 40 | 0.9 |
| Safari, local | 7.0 | 60 (58-124) | 2 | 6 | 1.0 | 49 | 2 | 0.5 |
| Safari, 10 Mbit/s | 128 | 237 (230-255) | 109 | 222 | 1.0 | - | 2 | 1.0 |
| Safari, Slow 4G | 516 | 1251 (1239-1262) | 560 | 1232 | 2.0 | - | 4 | 1.0 |
| Firefox, local (recorded only) | 15 | 66 (59-75) | 2 | 6 | 2.0 | 82 | 52 | 1.0 |
| Firefox, 10 Mbit/s (recorded only) | 164 | 234 (232-242) | 112 | 215 | 5.0 | - | 12 | 1.0 |
| Firefox, Slow 4G (recorded only) | 650 | 1175 (1157-1183) | 528 | 1154 | 3.0 | - | 8 | 3.0 |
| iPhone Safari, LAN, split | 105 | 110 (75-116) | 11 | 93 | 2 | 70 (45-73) | 2.5 | 0.5 |
| iPhone Safari, 10 Mbit/s | 191 | 217 (209-225) | 120 | 204 | 2 | - | 2 | 1 |
| iPhone Safari, 10 Mbit/s, `<img>` child | 70 | 200 (196-213) | 106 | 191 | 2 | - | 2 | 1 |
| iPhone Safari, Slow 4G | 647 | 1159 (1155-1167) | 533 | 1136 | 2.5 | - | 5 | 2 |

Notes:

- **The scene file is the long pole in every throttled row.** wasm and glue are in by about 110 ms at 10 Mbit/s. The 137 KB `.vox` arrives at about 210 ms, and at about 1.1 s on Slow 4G.
- **With a brick-native file of 14.5 KB, Slow 4G would land near 0.5 to 0.6 s** instead of 1.2 s. This is arithmetic from the rows above, not measured.
- **Parse and upload of 145k voxels:** 1.5 ms on desktop, 7 ms with 4x CPU.
- **Cold pipeline compile, timed alone** with the async calls in `web/compile-probe.html` (`results/probe-*.jsonl`):

  | Browser | Our compute pipeline | Trivial compute shader | Trivial shader + `rgba8unorm` storage texture write |
  |---|---|---|---|
  | Chrome | 31 ms | 8 ms | 75 ms |
  | Safari | 45 ms | 9 ms | 74 ms |
  | Firefox | 33 ms | 7 ms | 41 ms |

  The shadow ray costs nothing extra, because it shares the call site with the primary ray. The march loop itself is about 15 ms of the 31.
- **Steady GPU time per 1440x900 frame:** 2 ms in Chrome, 0.9 ms in Safari. Firefox shows 8 ms, but its work-done promise seems to resolve on frame ticks, so treat Firefox's GPU numbers as upper bounds.
- **iPhone:** steady GPU time per 740x463 frame is 1.6 to 1.8 ms. The LAN row is the split page with no simulated throttle, so its poster time (105 ms, `poster=` attribute) is the phone's floor for the attribute form; the `<img>` child form came in at 70 ms even at 10 Mbit/s.
- **Safari** exposes WebGPU on http://127.0.0.1 and works without changes. Its very first launch showed a 249 ms pipeline step (Metal warm-up), and later cold loads settled at 49 ms.

## Pass or fail

| Metric | Threshold | Result | Verdict |
|---|---|---|---|
| wasm + JS glue, brotli q11 | 150 KB or less (ceiling 300 KB) | 37.6 KB, 39.8 KB with the loader | **Pass** |
| Poster visible after the tag is parsed | 100 ms or less | iPhone: 70 ms with an `<img>` child at 10 Mbit/s; 191 ms with `poster=` (105 ms even on the LAN). Desktop: 7 to 15 ms; stand-in at 10 Mbit/s + 4x CPU: 62 ms `<img>`, 140 ms `poster=` | **Pass with the `<img>` child, fail with `poster=` alone** |
| First lit frame after visible, mid-range phone, 10 Mbit/s | 1.0 s or less | iPhone 217 ms. Stand-in: 270 ms Chrome (4x CPU), 237 ms Safari, 234 ms Firefox | **Pass on iPhone (faster than mid-range). Android pending** |
| Same on Lighthouse Slow 4G | 2.5 s or less | iPhone 1.16 s. Stand-in: 1.20 s Chrome (4x CPU), 1.25 s Safari, 1.18 s Firefox | **Pass on iPhone (faster than mid-range). Android pending** |
| Device creation, pipeline compile, wasm compile on one iPhone and one mid Android | Record | iPhone: device 2 ms, pipelines 70 ms (45 to 73), wasm compile 1 ms. Desktop: device 1 to 10 ms, pipelines 36 to 49 ms (Firefox 82), wasm compile 0.3 to 3 ms | **iPhone recorded. Android pending** |
| Largest scene iOS Safari survives across 3 reloads | Record | Not run. The spike reads only `.vox` and caps scenes at 2048 per axis; it needs a large-scene file first | **Pending** |
| twiggy size profile | Record | Above and in `results/twiggy-top.txt` | Done |

**The stand-ins are not phone results.** This Mac's GPU is far faster than a phone's, and the desktop network simulation does not model radio wake-up or a phone's TLS setup. On the iPhone, pipeline compile did grow (70 ms vs 36 to 49 ms on the Mac), but it overlaps the scene download, so it never set the first frame.

## What it changes for vxr

1. **The embed loads a brick-native scene file, with `.vox` as the fallback.** The island drops from 137 KB to 14.5 KB brotli. On slow links that roughly halves the time to first frame. The editor can write this file next to the `.vox` on save.
2. **The embed snippet puts the poster in an `<img>` child:** `<voxel-scene src="a.vxr"><img src="a.webp" alt="..."></voxel-scene>`. The poster `<img>` inside the tag starts loading during HTML parsing and passes the 100 ms poster line at 10 Mbit/s, where the attribute alone does not. Keep `poster=` working, but document the child form.
3. **Answer to the WebGL2 question: go poster-only for v1.** The poster works in all three browsers when WebGPU is missing. That path costs the 2.2 KB loader plus the poster image and nothing else. A WebGL2 fallback built into the module would cost about 25x the current viewer. If one is ever needed, it has to be a separate module loaded only when WebGPU is missing.
4. **Shader output goes through a storage buffer, not a storage texture,** wherever cold compile time matters: the embed, and likely the editor's first frame too.
5. **The 150 KB target holds with room to spare,** so there is no need for nightly-only tricks such as `build-std` with `panic_immediate_abort`. The ~7 KB of `core::fmt` comes from wgpu itself.

## Still open

- **A mid-range Android phone** (the rows marked pending above). The iPhone runs are done. How to run them:
  - Build with `spikes/dev ./build.sh` and serve with `python3 serve.py 8790` from `spikes/embed/`.
  - **Pages to run on each phone:**
    - `bench.html?runs=10&f=<name>` (real first frame)
    - `bench.html?runs=10&split=1&f=<name>` (each step alone)
    - `bench-child.html?runs=10&f=<name>`
  - Each page chains its own reloads, then shows medians and every run on screen, with a Copy button. Add `&report=1` to also POST each run to the bench server.
  - WebGPU needs a secure context, so a plain LAN `http://` address does not work on a phone. Options:

  | Option | Works for | Steps | Tradeoffs |
  |---|---|---|---|
  | Android over USB | Android only | Run `adb reverse tcp:8790 tcp:8790`, then open `http://localhost:8790/...` in Chrome on the phone | localhost counts as a secure context. Needs adb and USB debugging. All local, and `chrome://inspect` gives remote DevTools and profiling. The network is the server's simulation, not the phone's radio |
  | Local HTTPS on the LAN | iPhone and Android | Make a local CA and certificate (for example with mkcert), wrap `serve.py` in TLS (a few lines with Python's `ssl`), bind it to the LAN address, and install and fully trust the CA on the phone | No third party involved. Trusting a local CA on a phone is security-sensitive, so remove it afterwards. Binding to the LAN instead of 127.0.0.1 needs the owner's OK |
  | Preview deploy | iPhone and Android | Upload `web/`, `pkg/` and the scene to an unlisted preview URL | Real https, brotli, CDN and radio. It publishes the files, so the owner decides. There is no bench server, so read the results on screen. To hold the phone at 10 Mbit/s, use iOS Developer settings, Network Link Conditioner, or Chrome remote throttling on Android |

  - The iPhone runs used the second option without a new CA: the server sat behind an existing LAN reverse proxy whose certificate the phone already trusts.
  - Record wasm compile, device and pipeline times from the split runs, and first frame from the plain runs, on a mid Android.
- **iOS reload memory.** Needs a large scene served in a brick-native file (test 1's 10M and 100M terrains would do). Then reload it 3 times in iOS Safari.
- **Error handling.** An invalid shader only logs a browser warning, so the embed would still fade the poster out to a blank canvas. The real embed must check compilation info or use error scopes before it hides the poster.
- **Rotations.** The reader ignores `_r` and the default MagicaVoxel palette (it uses a gray ramp when `RGBA` is missing). The real codec covers both.
- **Poster sharpness.** The poster is 800x500, which is soft on a 2x screen wider than 400 CSS px. A larger poster costs bytes on the poster path, which is the one with a 100 ms target.

## Files

- `src/`: viewer crate (`lib.rs`, `vox.rs`, `shader.wgsl`).
- `web/`:
  - `voxel-scene.js`: the loader.
  - `index.html`: demo, plus the `?nogpu` check.
  - `bench.html` and `bench-child.html`: timing pages.
  - `lazy.html`: lazy-load and off-screen check.
  - `compile-probe.html`: shader compile probe.
  - `poster.html`: makes `poster.webp`.
- Scripts:
  - `build.sh`: build, wasm-bindgen, wasm-opt, brotli, sizes.
  - `size-matrix.sh`: the size table.
  - `serve.py`: bench server.
  - `bench-run.sh`, `bench-all.sh`, `probe-run.sh`: timing runs.
  - `summarize.py`: medians.
- `results/`: raw runs, one JSON line per cold load. `shots/`: screenshots.
