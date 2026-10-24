# Test 2: live path-traced preview

**Result: pass.** A progressive path tracer at 640x360 in a corner of the full-Retina editor view, with a radius-16 brush editing every frame, adds about 2.5 to 3.8 ms to the editor frame natively, in Chrome and in Safari. It accumulates 62 to 283 samples per pixel per second and gives a clean image 0.3 to 1.2 s after the camera stops. Palette recolors show in the next frame with no data rebuild.

- Machine: Apple M4 Pro, 16-core GPU, 24 GB, 3024x1964 Retina. Chrome 155, Safari 26.6.2, Firefox 158 (Firefox is recorded but does not decide pass or fail).
- Background load: macOS reported the GPU 32 to 53% busy with other apps before every timed run (`results/background-gpu-busy.txt`; one probe taken while another spike held the lock is marked and excluded). The spikes took turns on `spikes/gpu-lock`, but this load could not be removed. Treat every time here as an upper bound.
- Scene: the 10M-voxel terrain from test 1 (9,983,668 voxels, solid fill). The editor view is test 1's renderer at 3024x1964 with its default traversal (`trace_h`, `levels: 4`).

## Pass or fail

| Check (from `docs/research/recommendation.md`) | Native | Chrome | Safari | Firefox (recorded only) | Verdict |
|---|---|---|---|---|---|
| Frame time the preview adds while editing (median) | +2.95 ms framed, +3.83 ms close, +2.90 ms half res | +3.01, +2.82, +3.08 ms | +2.54, +2.46, +2.91 ms | +2.84, +3.16, +2.47 ms | **Pass** (4 ms or less) |
| Editor frame with the preview on (median, p95) | 7.2 / 8.8 ms framed, 11.5 / 13.6 ms close | 7.6 / 8.9 ms, 9.1 / 10.5 ms | 6.4 / 8.0 ms, 8.6 / 10.4 ms | 7.0 / 8.7 ms, 10.3 / 12.1 ms | **Pass** (16.7 ms or less in Chrome and Safari) |
| Samples per pixel per second, 640x360, diffuse, one bounce | 237 framed, 62 close | 283, 151 | 243, 135 | 237, 80 | **Pass** (30 native, 20 browsers) |
| Time to a clean image after the camera stops | 0.43 s framed, 1.18 s close | 0.32 s, 0.55 s | 0.43 s, 0.65 s | 0.75 s, 1.38 s | **Pass** (3 s native, 5 s browsers) |
| Palette recolor | 1 frame, no rebuild | 1 frame | 1 frame | 1 frame | **Pass** |

"Framed" is the whole scene in view (MagicaVoxel's `.`, the editor default). "Close" is 30% of that distance looking 25 degrees down, so terrain fills 89% of the preview instead of 33%. That makes it the heavy case. "Half res" is the editor at 1512x982, the M1 stand-in.

Two caveats on the frame-time row:

- **Native close view, p95.** The p95 frame with the preview is 5.0 ms slower than the p95 without it, so the tail goes over 4 ms. The median passes by 0.17 ms. With a 2.5 ms budget the close view adds 3.38 ms median and 4.24 ms at p95, at 43 spp/s.
- **Two ways to measure.** The table uses GPU timestamps: from the start of the frame's first pass to the end of the editor's blit. Wall-clock time per frame, with 8 frames submitted back to back, says the preview adds slightly less: 2.6 to 3.0 ms natively, 2.4 to 3.0 ms in Chrome and Safari. Firefox's wall clock is unusable: every frame takes 13 ms whatever the work, so for Firefox only its timestamps count.

## Also recorded

**Ray throughput at 640x360.** Primary rays start at pixel centres. Bounce rays are cosine-weighted and start from the primary hits. Each test runs 24 dispatches back to back, 3 times.

| | Primary, framed | Bounce, framed | Primary, close | Bounce, close |
|---|---|---|---|---|
| Native, `trace_h`, scene not edited | 773 Mrays/s | 193 | 507 | 197 |
| Native, `trace_h`, after 240 brush edits | 242 | 190 | 490 | 195 |
| Native, `trace` (three levels), after edits | 579 | 160 | 465 | 159 |
| Chrome, `trace_h`, after edits | 879 | 193 | 703 | 282 |
| Safari, `trace_h`, after edits | 834 | 208 | 748 | 301 |
| Firefox, `trace_h`, after edits | 359 | 130 | 655 | 247 |

- A full sample is a primary ray, a sun shadow ray, one bounce ray and a sun shadow ray from the bounce hit, so up to 4 rays. The browser rows ran after the editing runs (576 brush edits).
- With the tracer running flat out, one full sample of the whole preview takes:
  - Native: 0.97 ms framed, 2.33 ms close (1,034 and 430 spp per GPU-second).
  - Chrome: 0.85 ms framed.
  - Safari: 1.02 ms framed.
  - Firefox: 2.56 ms framed. It is slower when frames run one at a time.
- `trace_h` (the engine default) beats `trace` for full samples both before and after edits (`results/native/traversal-*.log`):
  - Framed: 1.19 against 1.22 ms before, 1.16 against 1.21 ms after.
  - Close: 2.10 against 2.44 ms before, 2.10 against 2.45 ms after.
- Native `trace_h` has one oddity. After the edits, its primary rays in the framed view (two thirds of them miss into the sky) drop to a third of their earlier speed, while `trace`, Chrome and Safari do not slow down. Full samples are not affected, because bounce and shadow rays dominate them. Passed to test 1 to look at.

**Denoise cost per frame** (initial pass, 4 A-trous passes and resolve, at 640x360):

| | Framed | Close |
|---|---|---|
| Native | 0.33 ms | 0.65 ms |
| Chrome | 0.26 ms | 0.46 ms |
| Safari | 0.31 ms | 0.55 ms |
| Firefox | 0.33 ms | 0.84 ms |

**GPU timestamps:** Safari 26.6.2 does expose `timestamp-query`. So do Chrome 155 and Firefox 158. Chrome rounds its timestamps to steps of about 65 microseconds.

**Half resolution (M1 stand-in):** the editor frame drops to 1.8 ms natively, and the preview still adds 2.9 ms (4.7 ms total). The preview's cost does not depend on editor resolution.

**Without the idle boost:** if the budget stays at 3 ms forever, the native close view takes 4.7 s to come clean (fails the 3 s line) and Firefox's close view never got there in 12 s. Raising the budget to 12 ms once input has been idle for 300 ms is required, not optional.

## How it was measured

**Preview.**
- Diffuse path tracing with a sun and a sky. The sun is sampled directly (next-event estimation) through a small cone for soft shadows. One bounce, lit by the sun or by the sky.
- Radiance is accumulated divided by the albedo of the primary hit, and multiplied by the current palette at display. So a recolor is just a palette write.
- Code: `src/trace.wgsl`, `src/denoise.wgsl`, `src/preview.rs`. It uses test 1's brick map, `trace_h` and its bind group unchanged.

**Budget.**
- While editing, the preview gets 3.0 ms of GPU per frame: its trace slice is 3.0 ms minus the measured denoise and composite time.
- The slice is cut into dispatches of about 1 ms of rows each, and continues where the last frame stopped.
- When input has been idle for 300 ms, the budget rises to 12 ms.
- The controller keeps a running average of GPU time per row.
- 3.0 ms rather than the recommendation's 3 to 4 ms: at 3.5 ms the close view added 3.99 ms median, and 4.6 ms at p95.

**Denoise.** 5x5 B-spline A-trous, 4 passes (steps 1, 2, 4, 8). Weights stop at:
- a change of face orientation;
- a plane distance over about 1 voxel;
- a luminance difference scaled by each pixel's variance (as SVGF does), so the filter fades out by itself as samples pile up.

**Edits** (reset only the touched pixels):
- The preview keeps its own pixel-centre primary hit (distance and face) for every pixel. It uses neither the editor's G-buffer (a different resolution and aspect) nor reprojection.
- After an edit, a pixel restarts if its primary ray or its sun shadow ray crosses the edited box (the brush sphere plus 1 voxel).
- Pixels within 32 voxels of the edit keep at most 16 samples, so the changed indirect light fades in.
- A camera move restarts the whole preview.
- Screenshots taken during and after editing show no stale pixels (`shots/preview-while-editing-close.png` against `shots/preview-after-editing-settled-close.png`).

**Composite.**
- A compute pass writes the preview into the editor's colour texture before the editor's blit.
- The first version drew it in a second render pass over the full 3024x1964 target. That cost about 2 ms extra in the close view: each tile of the target is loaded and stored again on a tiled GPU. The real editor should draw its corner views inside the pass that already draws the frame.

**Frame order and timing.**
- Each frame runs: preview passes, editor march, composite, editor blit.
- The preview goes first because GPUs run passes with no data dependency side by side. A preview pass that overlapped the march could not be timed on its own, and in Safari that sent the budget controller to almost zero samples.
- Frame time is measured from the first pass of the frame to the end of the blit.
- Editing runs: 48 warm-up frames, then 240 measured frames, with the brush editing every frame (about 0.25 ms of CPU including the upload).

**Clean image.**
- The reference is 1,024 samples per pixel, not denoised.
- The metric is PSNR on 8-bit sRGB over terrain pixels only. Sky pixels are noise-free and would inflate it: the whole-image PSNR of the framed view reads about 5 dB higher.
- The line is **38 dB**, chosen by eye on 3x crops (`shots/*/`):
  - 33 dB: blotches in shadowed grass.
  - 36 dB: faint blotches at 3x, hard to see at 1x.
  - 38 dB: indistinguishable from the reference at 3x.
- A run passes at the first of 5 checks in a row (one every 100 ms) at or above the line.
- Time runs on a simulated 120 Hz clock: each frame counts as the larger of its GPU time and 8.33 ms.
- The preview image is cleared at the restart, so a pixel without a sample cannot pass by showing an older image.

**Browsers.**
- The same Rust code is built to wasm (wasm-bindgen 0.2.129) and served on 127.0.0.1.
- Chrome was driven by `chrome-devtools-axi`. Safari and Firefox were opened with `open -a`, and each page posted its results to a local server.
- Timed frames render offscreen, the same as natively. The last frame is presented to the page's canvas as a visual check (`shots/chrome-canvas.png`, `shots/safari-window.png`).

## What it changes for vxr

- **The live corner preview holds on this Mac,** natively and in Chrome and Safari. The fallbacks in the recommendation (480x270, or pausing while editing) are not needed here.
- **Budget the whole preview, not just the trace.** Use 3 ms while editing, rising to 12 ms after 300 ms idle. The idle boost is what makes heavy views come clean in time.
- **Draw the preview into the editor's frame,** not in an extra pass over the whole window.
- **Keep the preview's own per-pixel primary hit** for edit resets. A simple box test per pixel kept edits responsive with no stale pixels. No reprojection is needed for edits.
- **Use `trace_h` for the tracer too.** It gives faster full samples, and it is the editor default.
- **Firefox works,** with the same numbers by GPU timestamps, but it is slow when frames run one at a time (2.6 ms per sample against 0.85 to 1.0 elsewhere).

## Still open

- **Real M1-class and phone numbers.** On a GPU with about a third of this one's speed (estimate), the same 3 ms buys about a third of the samples: roughly 80 spp/s framed and 20 close. The close view would then sit near the 30 spp/s line, and clean times would roughly triple.
- **Native close-view tail.** The p95 frame-time increase is over 4 ms. Rerun on an idle machine before reading much into it.
- **Native `trace_h` primary-ray slowdown after edits.** It costs nothing in full samples, but the editor view may feel it.
- **Materials.** Metal, glass and emissive are not in this spike. Glass and small emitters will converge more slowly than diffuse.

## Files

- `src/`: the tracer (`preview.rs`, `trace.wgsl`, `denoise.wgsl`, `composite_cs.wgsl`), the measurements shared by native and browser (`bench.rs`), the native runner (`bin/pbench.rs`) and the browser entry point (`web.rs`).
- `run-native.sh` (inside `../gpu-lock`) runs every native measurement. `run-browser.sh <chrome|safari|firefox> <name>` runs the browser version; it needs `python3 web/serve.py 8742 results` running.
- Build:
  - Native: `../dev cargo build --release`.
  - Browser: `../dev cargo build --release --lib --target wasm32-unknown-unknown`, then `../dev wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/vxr_preview_spike.wasm`.
- `results/`: raw JSON for every run and the background GPU load log. `summarize.py <results json>` prints the headline numbers of a browser run.
- `shots/`: 1,024-spp references, raw 1-spp frames, denoised images at the moment they passed (`framed/`, `close/`), editor frames with the preview, and the Chrome and Safari canvases.
