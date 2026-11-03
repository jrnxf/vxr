# vxr

fast voxel scenes: an open-source voxel scene editor (Rust, wgpu) with a one-tag web embed.

## Stack

- Rust core on wgpu (Metal, Vulkan, DX12, WebGPU in the browser). Desktop first; the browser build and the embed come from the same code. No server for the editor: the only server code is the optional account and cloud library, a Cloudflare Worker in TypeScript (`crates/vxr-web/worker/`) behind `/api/*` and `/account`, with D1 and R2.
- UI drawn in Rust with egui (picked by test 3, see `docs/decisions.md`) so the Hybrid UI runs native and in the browser.
- The real build is the Cargo workspace at the root (Phase 3, in progress). Run everything through `./dev <command>`, a Nix dev shell (`nix/flake.nix`: Rust with the wasm32 target, wasm-bindgen, wasm-opt, twiggy, brotli). `./dev --fuzz <command>` adds nightly and cargo-fuzz for the open-path fuzz target in `crates/vxr-core/fuzz` (its README has the commands).
- The Phase 2 spikes in `spikes/` build separately through `spikes/dev <command>`. Wrap timed GPU runs in `spikes/gpu-lock` so benchmarks never share the GPU.

## Build and test

- Once per checkout: `node prototype/src/build.mjs` (the island `scene.vox`) and `crates/vxr-app/fetch-fonts` (Geist, Geist Mono, Inter Regular for a few symbols and the Lucide icons into the git-ignored `crates/vxr-app/fonts/`; without them the app falls back to egui's fonts and the icons show as boxes).
- Run the editor: `./dev cargo run --release -p vxr-app -- [scene.vox]`, for example `prototype/demo/scene.vox`. Every tool, key and panel is in `docs/controls.md`.
- Headless check of the app: `./dev cargo run --release -p vxr-app -- --shot crates/vxr-app/shots prototype/demo/scene.vox`. It drives every tool and panel through the same pointer path as the window, prints `ok` or `FAIL` per step (exit code 1 on failure), prints frame times as `info` lines and writes screenshots into the git-ignored `crates/vxr-app/shots/`. It needs no window, so it works with the screen locked. What it covers: `docs/checks.md`.
- `./dev cargo test`: unit and property tests; the GPU tests skip when there is no adapter and the island tests skip without the island. `PROPTEST_CASES=<n>` sets the property test size. What they cover: `docs/checks.md`.
- `./dev cargo clippy --all-targets -- -D warnings` and `./dev cargo fmt --check` must be clean.
- `./dev cargo build --target wasm32-unknown-unknown` checks the browser target.
- Browser editor: `crates/vxr-web/build` makes the static site in the git-ignored `crates/vxr-web/dist/`; serve that folder to try it. It needs WebGPU. `crates/vxr-web/check` (after the build; needs `chrome-devtools-axi` on the PATH) checks it end to end in headless Chrome through `check.js`, prints `ok` or `FAIL` per step (exit code 1 on failure) and writes screenshots into the git-ignored `crates/vxr-web/shots/`. The page API it drives (`window.vxr.state()`, `hit(name)`, `color(slot)`, `material(slot)`) and what it covers: `docs/checks.md`.
- Extend `shot.rs` and `check.js` with each new feature, and add it to `docs/controls.md` and `docs/checks.md`.
- The Worker and the account page: `npm ci --prefix crates/vxr-web` once, then `npm --prefix crates/vxr-web run typecheck` and `npm --prefix crates/vxr-web test` (vitest in workerd with local D1 and R2; no test calls Turnstile or Resend). To try sign-in, copy `crates/vxr-web/.dev.vars.example` to `.dev.vars` and run `npx wrangler d1 migrations apply vxr --local` and `npx wrangler dev --local` in `crates/vxr-web` after the build; emailed links print to its log. Never point these at the real D1, R2 or secrets.
- `crates/vxr-web/deploy` builds and deploys it to https://vxr.jrnxf.co (Cloudflare Workers static assets and the Worker, `crates/vxr-web/wrangler.jsonc`), only from a clean `main`.
- Fixtures are generated in tests (`vxr_vox::fixtures`), never committed as `.vox` files.

## Where things live

- `crates/vxr-vox`: the `.vox` codec; `material.rs` maps `MATL` dicts to typed materials, `tree.rs` reads the scene graph as an editable tree of objects, groups and layers (`Document::tree`, and `set_tree` writing back only what changed).
- `crates/vxr-core`: the editable scene, no GPU. Brick map, palette and materials, undo/redo; editing tools in `tools.rs`, selection and transform in `select.rs`, hollow, fill inside, resize and crop in `model.rs`, OBJ export and PNG slices in `export.rs`, the world of objects (groups, linked copies, layers) in `world.rs`. Lossless `.vox` save goes through `vxr_vox::Document::set_tree` (via `World::save_into`). `Scene` holds an opened file's world; its map is one object's model or the composed world (`edit_object`, `show_world`, `edit_world`).
- `crates/vxr-render`: the GPU renderer (brick map upload, `trace_h` march, camera and snap views in `camera.rs`, guides in `march.wgsl`); `preview.rs` is the live path-traced corner, also used for export; `gpu.rs` has the non-blocking `Readback`. Its tests compare every GPU pixel with the CPU ray cast.
- `crates/vxr-app`: the desktop editor (binary `vxr`): `editor.rs` (scene, camera, gestures, drawing; no window), `ui.rs` (the Hybrid UI in egui), `render.rs` (Render panel and PNG export), `files.rs` (re-exports `vxr_core::files`), `native.rs` (winit window, file dialogs), `shot.rs` (the headless check).
- `crates/vxr-web`: the same `Editor` on a WebGPU canvas (`src/web.rs`) and its page (`www/index.html`, `www/app.js`).

- `assets/logo/`: the logo as SVG (the cube, the wordmark and the lockup, each for dark and light backgrounds, and the app icon); `docs/decisions.md` "Logo" has the colours and sizes.

- `docs/plan.md`: what to research and test next, in order. Ask the owner before starting each phase.
- `docs/decisions.md`: every settled decision and why. Update it when a decision changes; do not duplicate it here.
- `docs/research/`: Phase 1 desk research, one file per topic. `docs/research/magicavoxel-gap.md` lists every MagicaVoxel feature with vxr's status and priority; update its row when a feature lands. `docs/research/ai.md` is the AI research and proposal (assistant and `vxr mcp`); its mocks and screenshots live in the git-ignored `.scratch/ai-mocks/` and are never committed.
- `docs/market-research.md`: the market case. Reference only.
- `ui-directions.html` and the prototype screenshots: the UI options that led to Hybrid (direction D) and the mockups test 3 compares against. Reference only, in `vxr-design-references.zip` on the `design-references` GitHub release, not in git.
- `spikes/`: Phase 2 risk tests (engine, preview, vox, embed) and Phase AI risk tests (`ai-<name>`), each with a `RESULTS.md`. Throwaway, but the engine and the `.vox` codec are the starting point for Phase 3.
- `prototype/`: run `node prototype/src/build.mjs` once to create `prototype/demo/` (git-ignored), including the island `scene.vox` the spikes load. A throwaway three.js editor used to try the feel. Not the real build; mine it for ideas (chunking, greedy meshing, palette-texture recolor, `.vox` I/O), do not extend it.

## Keep the UI and the agent tools in step

- One editing logic, two front doors. The logic lives in `vxr-core` (`tools.rs`, `select.rs`, `model.rs`, `world.rs`, `export.rs`). The UI reaches it through `ui::Action` in `crates/vxr-app/src/ui.rs` (its Cmd-K row in `COMMANDS`), run in `editor.rs`. Agents reach it through one typed struct per tool in `crates/vxr-ops/src/args.rs` (listed in its `ops!` table, run in `session.rs`), whose generated schema serves the in-app assistant (`crates/vxr-app/src/assistant.rs`, every tool but `look` that works in the scene's mode, `vxr_ops::tools_for`), `vxr mcp` files mode (`crates/vxr-mcp/src/files.rs`) and `vxr mcp --live` (`crates/vxr-mcp/src/live.rs`, answered in `crates/vxr-app/src/agents.rs` and `live.rs`, and in the browser through `bridge.rs`). Tools that need the app or the file system rather than the scene (`render_settings`, `export`, `save`, `new_scene`, `open`, `list_files`, `edit_object`, `show_world`, `review_status`) are the own tools in `files.rs` and `live.rs`.
- A new or changed user-facing editing action ships in the same PR in both: the UI control with its Cmd-K command, and the `vxr-ops` tool calling the same `vxr-core` function (no logic only in `editor.rs` or `session.rs`), offered in files mode, live mode and to the assistant. A new tool with no UI gets its UI in that PR too.
- Cover both sides: the UI in `shot.rs` and `check.js`; the tool in `crates/vxr-ops/src/tests.rs`, `crates/vxr-mcp/src/tests.rs` (files mode) and `live_tests.rs` (live mode over the socket), and, when the window runs it, `agent_call` in `shot.rs` and `check-agent.js` in `crates/vxr-web/check`. Update the tool list in `docs/research/ai.md` section 2 and any tool names in `vxr_ops::SYSTEM_PROMPT` and the `INSTRUCTIONS` of `crates/vxr-mcp/src/protocol.rs` and `live.rs`.
- UI only, on purpose: the camera (orbit, pan, zoom, frame, projection, snap views; `look` takes a view to see from but never moves the user's camera), the view guides, the render corner and full render view, choosing a tool or brush and its size, mirror and axis lock (tools take explicit cells and shapes), flyouts and panels, themes, the touch layout, and the assistant, Agents tab and review card. Agent only, on purpose: the queries (`scene_summary`, `get_voxels`, `slice`, `find_parts`, `look`), `review_status` and `done`.

## Keep the repo small

- The owner wants a lean repo. Do not commit heavy files (screenshots, videos, recordings, benchmark dumps, large test data, built bundles) when they can be avoided: git history keeps them forever.
- Put them in a zip on a GitHub release (`gh release create` or `gh release upload`) and link the release from the docs, or regenerate them from a script.
- Small files the code needs (a test fixture of a few KB, a poster image for the embed) are fine. Ask before committing anything over about 200 KB.

## Before launch

- [ ] Clean up `spikes/` before the repo goes public: fold what Phase 3 keeps (engine, `.vox` codec) into the real workspace, delete the rest, and decide whether the `phase-2-spikes-data` release (screenshots, raw benchmark data, test files) stays or goes.
