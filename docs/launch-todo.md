# Launch to-do

What is left before launch, and the owner decisions still open. Items 1 to 3 of the launch list (the embed, export for sharing and the P2 rows a switching user will hit) are done. Tick a box when its work lands, and delete this file when every box is ticked.

## Work not done yet

### 4. Test audit: remove filler, add what guards real behaviour

Verify each point before acting on it.

- [ ] Remove or merge filler tests: the duplicated `a_missing_cli_says_how_to_add_by_hand` in `crates/vxr-app/src/pair.rs` and `setup.rs` (fold the near-identical code too), `neutral_keeps_the_shipped_tokens` and the default check in `theme.rs`, the easing-curve tests in `ui/account.rs`, the `send_budget` comparison in `courier.rs`, and the two one-literal wrappers in `crates/vxr-render/tests/render.rs`.
- [ ] Loud skips: GPU tests without an adapter and island tests without the island must say they skipped. Keep CI's `VXR_REQUIRE_GPU=1` and add the same hard requirement for the island.
- [ ] A floor on the `--shot` step count in CI: fail when the `ok` count drops below the last known count (1102 now; the browser check has 618).
- [ ] Direct tests, each proven by breaking the code: `line_cells`, `ball_cells` and `box_cells` in `crates/vxr-core/src/tools.rs` (property tests), `history.rs` (random edit, undo and redo against a model), `room_for` and `split_error` at the size limits, `world_blocks` at the 256 boundary, and `validate` and `set_layers` in `crates/vxr-vox/src/tree.rs` against hostile trees.
- [ ] `unwrap` and `expect` on input from files, sockets or provider JSON: `crates/vxr-ai/src/chat.rs`, `crates/vxr-core/src/world.rs`, `scene.world().expect(...)` in `crates/vxr-ops/src/session.rs` and the readback in `crates/vxr-render/src/gpu.rs`. Return an error where outside input can reach it. Otherwise leave a one-line comment that says why it cannot.
- [ ] A long fuzz run (about 20 minutes across cores) on the open path. Add each finding as a regression test.

### 5. CI covers what is checked by hand today

- [ ] Run the browser checks (`crates/vxr-web/check`, `crates/vxr-embed/check`) in CI if headless Chrome with WebGPU runs on the runner. Otherwise write down what blocks it. They need wasm-bindgen-cli, binaryen and Chrome.
- [ ] A fuzz smoke run of about a minute on the open path.

### 6. Platforms

- [ ] Linux: the desktop app builds and passes its tests and `--shot` on Linux, plus a script beside `crates/vxr-app/bundle` that makes a tarball (binary, `.desktop` file, icon). Not wired into `release.yml`.
- [ ] Windows: list what blocks a build (`cfg(unix)` paths, live mode). Fix what is safe to fix blind.

### 7. First five minutes

- [ ] Two or three starter scenes built by code (never committed as `.vox`), offered from the library and the landing page. They show materials, the path tracer and a scene past 256 cells.
- [ ] Just-in-time tips: once per tip, dismissible, off in the settings, never over the canvas centre.
- [ ] Every error a user can see says what happened and what to do next (open, save, export, sign-in, assistant).
- [ ] Accessibility: every control reachable by keyboard, visible focus, tooltips with the shortcut, readable text sizes in both themes.

### 8. Repo and docs a stranger can read

- [x] Rewrite `README.md` for a stranger: one sentence on what vxr is, a screenshot that links to the site, install, short feature bullets, the agent and MCP story, links to `docs/`. Deploy and Release moved to `docs/deploy.md` and `docs/release.md`.
- [x] Add `CONTRIBUTING.md` that points at `AGENTS.md`, with `SECURITY.md` and a bug report template.
- [ ] Bring `docs/controls.md`, `docs/checks.md` and the "What vxr has that MagicaVoxel does not" list up to date.
- [ ] Move what the real crates still need out of `spikes/` and `prototype/` (for example the island generator in `prototype/src/build.mjs`), then list what is safe to delete.

### 9. Performance (ongoing)

- [ ] The pool repack hitch at 100M voxels, and the close, low view near the 120 fps line (a depth prepass is the likely fix). Measure before and after with the `info` frame-time lines.

## Release wiring

- [ ] Wire the Linux tarball into `release.yml` once it exists.

## Open owner decisions

Settled answers go into `docs/decisions.md`.

### Project

1. Licence: the repo is public and has no licence file.
2. Issues 64, 65 and 67 on the Mac app's library sync.
3. A WebGL2 fallback for the embed (v1 shows only the poster without WebGPU).
4. Hosted share links served from the cloud library (accounts and abuse).
5. The gap list's platform row now says "partial" (no Linux download, no Windows build). Confirm the wording. Should sort palette, image import and saved views move from P3 to P2?

### Embed

6. The brick scene file extension. `.vxrb` is a placeholder (the viewer reads the magic, so any name works).
7. The embed draws glass and emissive voxels as plain colour. Is that fine for v1?
8. The embed's look comes from the spike: sky, sun, 12 px corners, 16:10, no zoom gesture. The island fills only about 16 percent of the first frame.
9. Poster: 1280x800 PNG, the Render panel's light and samples, a dark backdrop against the viewer's light sky.
10. Files mode (`vxr mcp --root`) refuses `export` kind embed because it has no GPU for the poster. Build a CPU poster, or keep the refusal?

### Export

11. Turntable: 8, 16, 32 or 64 frames (32 by default), a turn around the camera's target, the Export menu row starts at once, a 250 million pixel limit in the browser.
12. Animated turntable: APNG only, always written beside the frames, one turn in 4 seconds, an endless loop.
13. Isometric sprite: 2:1 pixel art, 4 pixels per voxel, flat shades.
14. Export selection: one sticky switch in the Export menu. It does nothing while nothing is selected, and files get a `-selection` suffix. Render, turntable and embed never take only the selection.

### Editing

15. Region option ("Brush inside only"): off at first, limits erase and paint only, not saved between sessions. The agent option is refused for mode add and with nothing selected.
16. Replace and remove colour: commands on the current colour, not brushes. They limit to the selection when one exists. Agents use `paint` and `erase` with `from_slot`.
17. Rotate and flip objects: a group turns around the middle of its box, not its origin. Cmd-K only. A new `transform_object` tool.
18. Palette group edits: saturation and value scale by percent. Shift-click multi-select only. A new `palette_adjust` tool.
19. Sort palette is Cmd-K only and ascending only. Palette text export is Lospec `.hex`.
20. `palette_swap` and Cmd-drag swap remap only the model being edited, so other objects show swapped colours. `Scene::remap_slots` can fix it in a few lines. Fix it, or keep it?
21. Palette import: a short palette replaces only slots 1..N. The agent tool takes text only. No drag-and-drop.
22. Pattern brush: key T, stamps the clipboard, no rotate or offset option, no hover ghost.
23. Scene tree reorder: no keyboard shortcut, world mode only, no auto-scroll while dragging.
24. Image import: placed upright in front of the model, not in a new object. Refused in the world view. PNG only, up to 1024 px. Colours go to the nearest slot.
25. Click to focus: a one-click pick, never turns depth of field on, `focus_on` is live mode only.
26. Saved views: the angle mapping is not checked against MagicaVoxel yet. Open a file with saved cameras there to check. Roll is ignored and the file's fov is not applied.
27. Scene tree multi-select: Cmd-click only, no Shift range. Only Cmd-G, H and Backspace act on several rows. Picks are not undo steps.
