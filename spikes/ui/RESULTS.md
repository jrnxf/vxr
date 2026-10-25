# Test 3: UI layer (egui vs HTML/CSS)

**Status: done. Call: egui.** Both builds pass every pass line. The egui look is not clearly below the HTML/CSS build: layout, sizes and type match within 1 to 2 px at 1x and 2x, and the differences are backdrop blur (HTML only) and slightly heavier egui text. Under the rule for tonight (egui wins unless its look is clearly below HTML/CSS), egui is the UI layer. **The owner may overturn this in the morning** after looking at the screenshots listed below.

## Summary

| Row (recommendation.md test 3) | egui | HTML/CSS | Pass line |
|---|---|---|---|
| Screenshots at 1x and 2x against the mockups | Same layout and type as HTML within 1 to 2 px; no backdrop blur; egui browser and egui native are pixel-identical in the UI | Same layout; backdrop blur like the mockup | Look judged below |
| Frame time the UI adds, 2560x1600 | Native 0.28 to 0.31 ms; Chrome 0.58 ms (0.08 GPU + 0.5 CPU) | No GPU work in our frame (browser compositor draws the DOM) | 1 ms: **both pass** |
| Click-to-voxel latency | 1 frame (30 of 30, Chrome) | 1 frame (30 of 30, Chrome) | 1 frame: **both pass** |
| Shell wasm size (not a pass line) | 812 KB brotli (4.36 MB raw); 695 KB brotli with simd128 | 70.9 KB brotli | Recorded only |
| Japanese IME (informational) | Composition reaches the egui field (preedit, then commit); glyphs show as boxes until a CJK font is added | Not run (works through the browser's own text field) | Does not count |
| VoiceOver (informational) | Not run; in the browser egui exposes only a canvas | Not run; all 18 buttons have names in Chrome's tree | Does not count |

## What was built

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

Look, first pass: text at 11 to 13 px is crisp at 1x and 2x, with no blur or misalignment. Differences from the mockup so far: no backdrop blur behind panels (egui has none), and the scene background is the test 1 renderer's light sky rather than the mockup's dark gradient, which makes the translucent panels read lighter. The side-by-side with the HTML/CSS build is below.

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

## egui, browser (Chrome, WebGPU)

`src/web_egui.rs` and `web/egui.html`: the same `Viewer` canvas, with `hybrid.rs` drawn by egui-wgpu into the canvas frame after the blit. The page only forwards DOM pointer, wheel, key and IME composition events to egui; every panel, label and Cmd-K is egui. Build with `spikes/ui/build-web egui` (into `web/pkg-egui/`, git-ignored), serve as above and open `/egui.html`. `?shot` and `?cmdk=<text>` work as in the HTML build; `window.bench(n)` times n frames with the UI and n without.

| Row | Result |
|---|---|
| Shell wasm (viewer plus egui, egui-wgpu; opt-level s, fat LTO, wasm-opt -O3; fonts fetched at runtime, not embedded) | 4,362 KB raw, **812 KB brotli**; glue 13.4 KB brotli. That is 741 KB brotli over the HTML build's shell. With `-C target-feature=+simd128`: 2,562 KB raw, 695 KB brotli |
| Click-to-voxel | **1 frame.** 30 of 30 clicks on the island drawn by the next requestAnimationFrame frame, 13.4 ms median and 15.4 ms worst after the event at 60 Hz. The handler took 2.7 to 6.9 ms because it also rescans the whole map for the used-colour dots (a spike shortcut; the app will keep a count per slot) |
| Frame time the UI adds, 1280x800 points at 2x (2560x1600 px), orbiting, 300 frames with and 300 without, three runs | GPU frame without UI 1.10 to 1.19 ms median, with UI 1.18 to 1.26 ms; UI CPU (egui run, tessellate, upload) 0.5 ms median, 0.6 to 0.8 ms p95 (Chrome rounds `performance.now` to 0.1 ms). **UI adds 0.58 ms: pass** (1 ms line). Frames right after an edit cost about 1.2 ms of UI CPU, because the palette's used dots change |
| Look | The UI pixels are identical to the native egui screenshots at 1x and 2x (mean difference 0.02 of 255; the only differing pixels, 0.05%, are in the island, from the GPU's float rounding) |

Screenshots: `shots/eguiweb-1x.png`, `eguiweb-2x.png`, `eguiweb-cmdk-1x.png`, `eguiweb-cmdk-2x.png`.

Where the size goes (twiggy on an unstripped build): egui 0.37 rasterizes glyphs with `vello_cpu`, whose generic SIMD fallback code and blend modes are most of the 4.4 MB. It is not behind a feature flag. The size does not matter for the desktop app and the embed uses no UI; it only slows the first load of the browser editor.

## IME and VoiceOver (informational, about 15 minutes)

- **Japanese IME, egui:** the page keeps a hidden text field focused while an egui text field has focus and forwards `compositionupdate` and `compositionend` to egui as `Ime::Preedit` and `Ime::Commit`. Tested in Chrome by dispatching composition events into Cmd-K's field (an OS IME needs a person at the keyboard): the preedit "にほん" showed as three characters and the commit "日本" replaced it with two (`shots/eguiweb-ime-preedit-1x.png`, `eguiweb-ime-commit-1x.png`). The glyphs drew as empty boxes, because Inter and JetBrains Mono have no Japanese. Fix: add a CJK fallback font (for example Noto Sans JP, several MB, loaded on demand). Also to do: move the hidden field to egui's reported IME rectangle so the OS candidate window sits by the cursor. Native egui (egui-winit) was not checked tonight.
- **Japanese IME, HTML/CSS:** not run. Its Cmd-K field is a real `<input>`, so the browser's IME and system fonts apply.
- **VoiceOver:** not run in either build: the session was unattended and VoiceOver needs a person and the Accessibility permission. What was checked: the HTML build exposes all 18 buttons by name in Chrome's accessibility tree; the egui browser build exposes only the canvas (egui has no web accessibility tree). Native egui can expose its widgets through AccessKit (the `accesskit` feature of egui-winit); not tried.

## The call: egui

Rule applied tonight, from the objective: egui wins unless its look is clearly below the HTML/CSS build; IME and VoiceOver do not count against it.

- **Look.** Compared at 1x and 2x, side by side (`shots/side-2x-*.png`) and against `docs/screenshots/editor-island.png`. The two builds match in layout, sizes, icons and type. HTML is closer to the mockup in two details: backdrop blur behind the panels, and muted text that renders a little dimmer and lighter. egui centres the used-colour dots where the mockup puts them at the bottom, a one-line fix. None of this makes egui clearly below; over plain sky the two are hard to tell apart.
- **Numbers.** Both pass frame cost and click latency. egui costs more wasm (812 KB against 71 KB brotli), which only affects the browser editor's first load.
- **Why egui is the better base anyway:** one Rust UI for desktop and browser, no IPC or input routing between a DOM and the engine, and the egui pass shares the frame we already draw.
- **What egui needs:** a CJK fallback font for IME, the hidden IME field placed at the cursor in the browser, AccessKit on desktop, and (later) a blur pass behind panels if the owner misses it.

**The owner may overturn this in the morning.** The screenshots to compare are `shots/egui-2x.png` (or `eguiweb-2x.png`) against `shots/html-2x.png`, plus the crops in `shots/side-2x-*.png`.
