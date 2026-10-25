# Test 3: UI layer (egui vs HTML/CSS)

**Status: in progress.** The egui build is done natively (headless); the HTML/CSS build, the browser builds, click-to-voxel latency, wasm sizes, IME, VoiceOver and the call are still to come.

## What was built so far

- `src/hybrid.rs`: the Hybrid UI in egui 0.37: top bar, tool rail (add, erase, paint, pick; voxel, box, face; mirror, frame), palette panel with the current colour, 64 swatches and used-colour dots, coach card, stats corner, hint bar that rewrites itself for the tool and modifiers, undo cluster, and Cmd-K with a text field and filtered commands. Single-letter keys, Shift erases, Alt picks.
- Every size, colour and font follows the prototype's CSS (`prototype/src/editor.html`), which the mockups were captured from: Inter and JetBrains Mono, Lucide icons. The fonts are fetched by `fetch-fonts` into `fetched/` (pinned versions, sha256 checked, not in git).
- `src/offscreen.rs`: the test 1 renderer (compacted brick map, `trace_h`) plus the UI drawn into an offscreen texture and read back. No window, so it runs with the screen locked.
- `src/bin/egui_shot.rs`: screenshots at 1x and 2x of a 1280x800-point view (the mockup's size) of the island, and the UI's cost.

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
