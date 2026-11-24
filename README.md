<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/vxr-lockup-on-dark.svg">
  <img alt="vxr" src="assets/logo/vxr-lockup-on-light.svg" height="64">
</picture>

**vxr is a fast voxel scene editor for macOS and the browser.** It opens and saves MagicaVoxel `.vox` files without loss, path-traces the scene while you edit, and puts a scene on any web page with one tag.

<a href="https://vxr.jrnxf.co"><img alt="The vxr editor with a voxel island open: the tool rail on the left, the live path-traced render in the corner" src="https://github.com/user-attachments/assets/9b22dd7e-a955-464e-95c8-5ff70f3bc415" width="100%"></a>

## Try it

- **In the browser:** open https://vxr.jrnxf.co. Nothing to install. It needs WebGPU (a recent Chrome, Edge or Safari).
- **On a Mac:**

```sh
brew install jrnxf/tap/vxr
vxr                # open the editor
vxr scene.vox      # open a file in it
```

macOS 13 or later, Apple silicon or Intel. There is no Linux or Windows download yet. `vxr setup app` adds vxr to Spotlight.

## What it does

- **Sculpt:** voxel, box, face, line, circle and sphere brushes, drawn live as you drag. Mirror on three axes, select, move, rotate, hollow and fill. Unlimited undo.
- **Large scenes:** one sparse scene up to 2048 voxels on each axis, with objects, groups, linked copies and layers.
- **Render:** a live path-traced view sits in the corner while you edit. Metal, glass, plastic and emissive materials, sun and sky, depth of field, and PNG export up to 4K.
- **Export:** OBJ with a palette texture for a game engine, PNG slices, an isometric sprite, a turntable, and the web embed.
- **Every command by name:** Cmd-K lists each command, and a hint bar explains the tool in your hand.

`docs/controls.md` lists every tool, key and panel.

## MagicaVoxel files

- vxr reads and writes `.vox`. An unedited save is byte for byte the same file.
- A save keeps materials, layers, cameras, names, animation keyframes and any data vxr does not understand.
- A `.vox` model holds at most 256 voxels on an axis. vxr saves a larger object as a group of blocks, and says so when it does.
- `docs/research/magicavoxel-gap.md` lists every MagicaVoxel feature with its status in vxr.

vxr is not affiliated with MagicaVoxel or its author.

## Put a scene on a web page

The Embed command writes a folder with the scene, a poster image and a small viewer. A page shows it with one tag:

```html
<script src="voxel-scene.js" async></script>
<voxel-scene src="island.vxrb" alt="A voxel island">
  <img src="island.png" alt="">
</voxel-scene>
```

The poster shows at once. The viewer loads only when the scene scrolls into view, and a browser without WebGPU keeps the poster.

## Agents and MCP

vxr gives an AI agent the same editing tools you have. Each change is one undo step.

- **In the app:** the assistant (Cmd-J) edits the scene with your own Anthropic, OpenAI or OpenRouter key, or a local model. Each change waits in a review card until you apply it. Keys stay on your machine: in the OS keychain, or in the browser tab unless you choose to remember them in that browser.
- **From a terminal agent:** `vxr pair claude` (or `vxr pair codex`) registers two MCP servers. `vxr` edits the `.vox` files in `~/vxr`. `vxr-live` edits the scene open in the editor, once you allow it in the assistant's Agents tab.
- **By hand, for any MCP client:** `vxr mcp --root <dir>` serves the `.vox` files under a folder on stdio, with no key and no network. `vxr mcp --live` serves the open window. `vxr bridge` connects it to the browser editor.

`vxr --help` lists every command. `docs/research/ai.md` section 2 lists every tool.

## Uninstall

```sh
vxr setup --remove   # first: Homebrew cannot reach what pair and setup wrote
brew uninstall vxr
```

`vxr setup --remove` removes vxr's MCP servers from Claude Code and Codex, the `/vxr` command and `~/Applications/vxr.app`. It leaves what is yours and prints where each is: your scenes in `~/vxr`, the library and settings in `~/Library/Application Support/vxr`, and the keychain items of the service `vxr`.

## Build from source

The build needs [Nix](https://nixos.org/download) with flakes on. Every command runs through `./dev`, which starts a shell with the pinned Rust toolchain. The first run downloads it and takes a few minutes.

```sh
node prototype/src/build.mjs       # once: the island scene the checks load
crates/vxr-app/fetch-fonts         # once: the UI fonts and icons
./dev cargo run --release -p vxr-app -- prototype/demo/scene.vox
./dev cargo test
```

The desktop app builds on macOS today. It builds and passes its tests on Linux in CI, with no download yet. The browser editor (`crates/vxr-web/build`) runs anywhere with WebGPU. [CONTRIBUTING.md](CONTRIBUTING.md) has the rest.

## More

- [docs/controls.md](docs/controls.md): every tool, key and panel.
- [docs/decisions.md](docs/decisions.md): each design decision and why.
- [docs/checks.md](docs/checks.md): what the tests and checks cover.
- [docs/deploy.md](docs/deploy.md) and [docs/release.md](docs/release.md): how the site deploys and how a release is cut.
- [SECURITY.md](SECURITY.md): how to report a security problem.
