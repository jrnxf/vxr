<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/vxr-lockup-on-dark.svg">
  <img alt="vxr" src="assets/logo/vxr-lockup-on-light.svg" height="64">
</picture>

fast voxel scenes

## Install

```sh
brew install jrnxf/tap/vxr
vxr setup app      # optional: open vxr from Spotlight
vxr setup claude   # optional: let Claude Code edit scenes (or: vxr setup codex)
```

- `vxr` opens the editor, and `vxr scene.vox` opens a file in it. `vxr setup app` adds `~/Applications/vxr.app`, so Spotlight opens it too; it links to Homebrew's copy, so `brew upgrade vxr` updates it.
- `vxr setup claude` (or `vxr setup codex`) registers vxr's two MCP servers with that agent, for every project: `vxr` edits the `.vox` files in `~/vxr` (`--root <dir>` picks another folder), and `vxr-live` edits the scene open in the editor while the assistant's Agents tab allows it. Running it again replaces both, so it is also how to change the folder.
- macOS 13 or later, on Apple silicon or Intel: the release is one universal app, with nothing to compile. Until the first full release, brew installs the latest pre-release (`v0.1.0-pre.N`), and `brew upgrade vxr` moves to the next one.
- The browser editor at https://vxr.jrnxf.co needs nothing installed.

Early stage. The real build is a Rust workspace in `crates/`, and the first desktop editor runs: open a MagicaVoxel `.vox` file, add, erase, paint and pick with the voxel, box, face, line and center (circle or sphere) brushes, drawn live as you drag (the voxel brush in sizes up to 16; a box's height follows the pointer after its base is drawn, until a click places it), with mirror on X, Y and Z and an axis lock, select by box, by colour or by the face or connected volume clicked (or invert the selection) and then move, copy, paste, delete, fill, rotate, flip or loop the selection (or the whole model), hollow it to a shell or fill the space it encloses, resize the model's bounds or crop them to the voxels, orbit, pan and zoom in perspective or orthographic, snap to isometric or the six side views with the ground, its voxel grid and the model bounds drawn (each can be hidden), undo and redo, save, and start a new blank scene (Cmd-N, which asks first when there are unsaved edits). A file with several objects opens as separate objects: Tab switches between the world of every object and editing one object's model (an edit to a linked copy shows on every copy), and each model's size is saved as its bounds. In the world, a click picks an object, a drag or the arrows move it, and it can be duplicated, linked (an instance sharing its model) or deleted. Once a second object exists a scene tree shows beside the tool rail: every object and group, indented under its groups, with each model's size; a row click picks the object or group (or edits the object, in model mode), the row's eye or H hides and shows it, and a double-click renames it. A picked group moves, copies and deletes as one, Cmd-G puts the picked object into a new group and Cmd-Shift-G ungroups. Under the tree in the world, a LAYERS list shows, hides, names and adds layers, and a click puts the picked object on one. The palette is editable: all 255 slots, a colour editor with hex, RGB and HSV that recolours every voxel using the slot instantly, and swap and copy of slots as in MagicaVoxel, each one undo step. Saving an opened file is lossless: materials, layers, cameras, names and anything vxr does not understand are kept, and an unedited save is byte for byte the same file. A live path-traced render sits in the corner while you edit: it keeps sampling in the background, updates only around an edit, and recolours instantly; a click on it fills the whole view with it, and Escape puts it back. A Render panel sets the sun, sky (a gradient, or a physical sky that follows the sun's height, reddens at sunset and shows the sun disk), exposure, gamma, vignette, depth of field and sample count, and exports a PNG up to 4K while you keep editing (a download in the browser). The Export menu in the top bar also writes an OBJ mesh of what the view shows (one model, or every shown object in the world), merged into rectangles of one colour and textured by a 256x1 palette PNG, ready for a game engine (one zip download in the browser). PNG slices put every layer of it side by side in one image. Each palette slot can be diffuse, metal, plastic (a glossy coat with its specular), glass (with attenuation, deeper colour through thicker glass) or emissive, with roughness and the type's settings; the render shows it, and it is saved in MagicaVoxel's `MATL` chunks so MagicaVoxel reads it back. It uses the Hybrid UI (tool rail, palette, hint bar, Cmd-K) drawn with egui, and shows the voxel count and FPS. An assistant (Cmd-J, or Cmd-K "Ask AI") edits the scene through tool calls with your own Anthropic, OpenAI, OpenRouter or local model key, and every change waits in a review card until you apply it as one undo step; keys stay on your machine (the OS keychain, or the browser tab unless you choose to remember them there, encrypted). The same editor runs in the browser with WebGPU (open and save there go through a file picker and a download). See `AGENTS.md` for how to build and run both.

## What is here

- `prototype/` - a throwaway in-browser editor (three.js) used to try the feel: add, erase, paint and pick, box and face brushes, X mirror, palette-first recolor, unlimited undo, `.vox` open and save, and a one-tag `<voxel-scene>` embed.
  - `prototype/demo/` is the built version and is not in git: run `node prototype/src/build.mjs` (no dependencies) to create it, including the starter island `scene.vox`. Serve it with any static server, for example `python3 -m http.server -d prototype/demo`, then open `index.html` (`?scene=terrain` loads a 3.9M-voxel stress scene) or `embed.html`.
  - `prototype/src/` holds the sources and `build.mjs`.
- `docs/market-research.md` - whether there is a market for an open-source voxel editor, and which wedge to take.
- `docs/research/ai.md` - research and a proposal for AI in vxr: a bring-your-own-key assistant that edits the scene through tool calls with a review step, and a `vxr mcp` server for terminal agents. The assistant (`docs/decisions.md` "Phase AI build") and `vxr mcp` files and live modes, with `vxr bridge` for live mode in the browser (`crates/vxr-mcp`), are built.
- `vxr mcp --root <dir>` - an MCP server on stdio that lets a terminal agent open, edit and save the `.vox` files under `<dir>` (lossless, no key, no network). `vxr setup claude` registers it on `~/vxr`; by hand, for Claude Code: `claude mcp add vxr -- vxr mcp --root <dir>`; for other clients `{"mcpServers": {"vxr": {"command": "vxr", "args": ["mcp", "--root", "<dir>"]}}}`.
- `vxr mcp --live` - the same server editing the scene open in the desktop app, each call one undo step listed under Agent edits in the assistant panel's Agents tab with Undo and Stop (or, with the Agents tab's "Review every agent change" on, a preview that waits for Apply or Discard). It connects over a local socket only while the assistant panel's Agents tab (Cmd-K "Terminal agents (MCP)…") allows it. `vxr setup claude` registers it as `vxr-live`; by hand, for Claude Code: `claude mcp add vxr-live -- vxr mcp --live`. Not on Windows yet.
- `vxr bridge` - live mode for the browser editor at https://vxr.jrnxf.co. Run it in a terminal and type the pairing code it prints into the Agents tab of the page's assistant panel (Cmd-K "Terminal agents (MCP)…"); then `vxr-live` edits the scene in that tab as it does the desktop app. It listens on 127.0.0.1 only and answers only the vxr.jrnxf.co page. Chrome and Edge ask once to let the page reach apps on this device; allow it. Firefox should work the same way; Safari may not let a web page reach it, and the panel says so.
- `ui-directions.html` (in `vxr-design-references.zip` on the `design-references` GitHub release) - four UI directions for the editor. The pick is **D, Hybrid**.

## Direction

- A Rust core on wgpu, built both as a native app and as WebAssembly with WebGPU for the browser and the embed.
- MagicaVoxel's feel, without its limits: sparse scenes well past 256^3, lossless `.vox` round-trip, and a scene file that is also a web embed.

## Deploy

- The browser editor at https://vxr.jrnxf.co deploys itself: Cloudflare Workers Builds builds every push to `main` and deploys it, and builds every other branch as a preview version that does not go live (the `Workers Builds: vxr` check on a pull request). Both run `crates/vxr-web/cloudflare-build`, which installs the same tools as `nix/`, pinned by version and sha256, and runs `crates/vxr-web/build` with them.
- `crates/vxr-web/deploy` still deploys by hand from a clean `main`, through the nix dev shell.

## Release

- Pushing a tag `v<version>` runs `.github/workflows/release.yml`: it fails unless the tag's commit is on `main` and passes `.github/workflows/ci.yml` (which also runs on every pull request and push to `main`), then builds `vxr.app` with one universal binary (`crates/vxr-app/bundle`), publishes the tag's GitHub release with `vxr-<version>-universal-apple-darwin.tar.gz` and its `.sha256`, and points `Formula/vxr.rb` in jrnxf/homebrew-tap at it.
- Development builds are tagged `v0.1.0-pre.N` and published as pre-releases; the build stamps the tag's version in, so no commit is needed. The formula follows them until the first full release (`v0.1.0`, which Cargo.toml must say), then only full releases.
- `crates/vxr-app/bundle <out-dir>` builds the same `vxr.app` for this Mac from a checkout.
