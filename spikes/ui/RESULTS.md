# Test 3: UI layer (egui vs HTML/CSS)

**Status: in progress.** The egui build is done natively (headless) and the HTML/CSS build is done in Chrome. Still to come: the egui browser build (its frame cost, click-to-voxel latency and shell size), IME, VoiceOver and the call.

## What was built so far

- `src/hybrid.rs`: the Hybrid UI in egui 0.37: top bar, tool rail (add, erase, paint, pick; voxel, box, face; mirror, frame), palette panel with the current colour, 64 swatches and used-colour dots, coach card, stats corner, hint bar that rewrites itself for the tool and modifiers, undo cluster, and Cmd-K with a text field and filtered commands. Single-letter keys, Shift erases, Alt picks.
- Every size, colour and font follows the prototype's CSS (`prototype/src/editor.html`), which the mockups were captured from: Inter and JetBrains Mono, Lucide icons. The fonts are fetched by `fetch-fonts` into `fetched/` (pinned versions, sha256 checked, not in git).
- `src/offscreen.rs`: the test 1 renderer (compacted brick map, `trace_h`) plus the UI drawn into an offscreen texture and read back. No window, so it runs with the screen locked.
- `src/bin/egui_shot.rs`: screenshots at 1x and 2x of a 1280x800-point view (the mockup's size) of the island, and the UI's cost.

- `src/web.rs` and `web/html.html`: the HTML/CSS build. A wasm `Viewer` (the same renderer, compacted bricks, orbit, zoom, pan, single-voxel add, erase, paint and pick) draws on a canvas, and the page draws the same Hybrid UI as DOM over it with the prototype's CSS, the same fonts and the Lucide icon font. Built with `--no-default-features`, so the shell wasm holds no egui. `?shot` freezes the stats like the egui screenshots, `?cmdk=<text>` opens Cmd-K.

Run: `spikes/ui/fetch-fonts`, then from `spikes/ui`: `../dev cargo build --release && ../dev ../gpu-lock target/release/egui_shot`. Screenshots go to `spikes/ui/shots/` (git-ignored).

## egui, native (Metal)

Apple M4 Pro, 16-core GPU, 24 GB. Island scene, 1280x800 points at 2x (2560x1600 px), orbiting, 300 frames with the UI and 300 without, alternating:

| | Median | p95 |
|---|---|---|
| GPU frame without UI | 1.05 ms | 1.42 ms |
| GPU frame with UI | 1.18 to 1.21 ms | 1.48 to 1.50 ms |
| UI CPU (egui run, tessellate, upload) | 0.14 to 0.15 ms | 0.18 to 0.19 ms |
| **UI adds, GPU + CPU** | **0.28 to 0.31 ms** | |

Pass line is 1 ms: **pass**. An empty egui pass already adds 0.04 to 0.07 ms (a full-screen load and store), so egui's own drawing is about 0.1 ms.

How it was timed: GPU timestamps at the start of the march and at the end of the last pass of each frame, resolved once after all 600 frames. Two traps on Metal, kept here so nobody repeats them: resolving right after each frame read the last pass's end stamp one frame late, and the begin stamp of a second render pass fires before the previous pass ends, so a per-pass time for the UI pass came out at 1.1 ms even when the pass drew nothing.

Look, first pass: text at 11 to 13 px is crisp at 1x and 2x, with no blur or misalignment. Differences from the mockup so far: no backdrop blur behind panels (egui has none), and the scene background is the test 1 renderer's light sky rather than the mockup's dark gradient, which makes the translucent panels read lighter. The side-by-side with the HTML/CSS build comes next.

HTML/CSS build: `spikes/ui/build-web` (prints the shell size), then `python3 web/serve.py <port> ../../prototype/demo/scene.vox` from `spikes/ui` and open `http://127.0.0.1:<port>/html.html` in a WebGPU browser. Screenshots were taken with headless-driven Chrome (chrome-devtools-axi) at a 1280x800 viewport with device scale 1 and 2: `shots/html-1x.png`, `html-2x.png`, `html-cmdk-1x.png`, `html-cmdk-2x.png`.

## HTML/CSS, browser (Chrome, WebGPU)

| Row | Result |
|---|---|
| Shell wasm (viewer only: renderer, `.vox` reader, raycast edits; opt-level s, fat LTO, wasm-opt -O3) | 205.9 KB raw, **70.9 KB brotli**; wasm-bindgen glue 11.5 KB brotli |
| Click-to-voxel | **1 frame.** 30 clicks on the island, each a pointerdown on the canvas: the edit is written to the GPU queue inside the event handler (0 to 0.4 ms), and every one of the 30 was drawn by the next requestAnimationFrame frame, 14.6 ms median and 16.2 ms worst after the event at 60 Hz. The clicks were synthetic PointerEvents dispatched in the page, so OS input delay is not included; it is the same for both builds. |
| Frame time the UI adds | Not measured as a number: the DOM is composited by the browser outside the WebGPU frame, so it adds no GPU work to our frame. Its cost is the browser's compositor (backdrop blur on seven panels), which we cannot time with WebGPU timestamps. |
| Button names (accessibility tree) | All 18 buttons have names in Chrome's accessibility tree ("Add", "Erase", "Paint", "Pick color", "Voxel brush", "Undo", palette slots as options). VoiceOver itself: see below. |

The shell is 2.4x the test 5 embed (30 KB) because it carries the full editor renderer, the `.vox` reader and the edit path; it is the base both builds share. The egui build's shell is that plus egui.

## Look, side by side (1x and 2x)

Crops in `shots/side-2x-palette.png`, `side-2x-topleft.png`, `side-2x-bottom.png` (egui left, HTML right; made with a small PIL script, not in git), against the mockup `docs/screenshots/editor-island.png` from the design references.

- **Layout and type are the same.** Every panel lands within 1 to 2 px of the other build: top bar, rail, palette grid, coach card, stats, hint bar, undo cluster, Cmd-K. Inter and JetBrains Mono render crisply in both at 1x and 2x.
- **egui text is a touch heavier and darker** than Chrome's at the same size (egui rasterizes without the browser's gamma and hinting). It reads fine, but next to each other the HTML text is slightly lighter, like the mockup.
- **Backdrop blur:** HTML has it, egui does not. It shows behind the coach card and Cmd-K when they sit over the island; over plain sky the two look identical.
- **Used-colour dots:** the HTML build puts them at the bottom of each swatch, exactly like the mockup (that is how the prototype's CSS lays them out); the egui build centres them. A one-line change in egui.
- Both builds say "Shape" over voxel/box/face where the mockup says "Brush", and both show the test 1 renderer's light sky rather than the mockup's dark three.js backdrop. Same for both, so it does not affect the call.
