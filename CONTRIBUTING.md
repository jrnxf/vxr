# Contributing to vxr

Thank you for looking. This file is the short path. [AGENTS.md](AGENTS.md) is the full reference: it is written for coding agents, and it is correct for people too.

## Before you start

- For a bug, open an issue with the bug template. It asks for your `vxr --version`, macOS version and GPU.
- For a feature or a larger change, open an issue first and describe it. It saves you work if the answer is no.
- For a security problem, do not open an issue. See [SECURITY.md](SECURITY.md).

## Build and run

You need [Nix](https://nixos.org/download) with flakes on. Every command runs through `./dev`.

```sh
node prototype/src/build.mjs       # once per checkout: the island scene the checks load
crates/vxr-app/fetch-fonts         # once per checkout: the UI fonts and icons
./dev cargo run --release -p vxr-app -- prototype/demo/scene.vox
```

- The desktop app runs on macOS. Linux builds and passes its tests in CI. Windows does not build yet.
- The browser editor: `crates/vxr-web/build`, then serve `crates/vxr-web/dist/`. It needs WebGPU.

## Before you open a pull request

Run these four. CI runs them too.

```sh
./dev cargo fmt --check
./dev cargo clippy --all-targets -- -D warnings
./dev cargo test
./dev cargo run --release -p vxr-app -- --shot crates/vxr-app/shots prototype/demo/scene.vox
```

- The last one drives every tool and panel with no window, and prints `ok` or `FAIL` for each step.
- For the Worker or the site (`crates/vxr-web`): `npm ci`, then `npm run typecheck`, `npm run lint`, `npm run format:check` and `npm test` in that folder.
- [docs/checks.md](docs/checks.md) says what each check covers.

## Rules that are easy to miss

- **One editing logic, two front doors.** A new editing action needs its UI control and its agent tool in the same pull request. AGENTS.md "Keep the UI and the agent tools in step" has the list of files.
- **Add a check with the change.** A bug fix comes with a test that fails without it.
- **Keep the repo small.** Do not commit screenshots, videos or `.vox` files. Tests generate their fixtures.
- **Update the docs your change touches:** `docs/controls.md`, `docs/checks.md` and `docs/decisions.md`.

## Where things live

- `crates/vxr-vox`: the `.vox` codec.
- `crates/vxr-core`: the scene, the tools and the exports. No GPU.
- `crates/vxr-render`: the GPU renderer and the path tracer.
- `crates/vxr-app`: the desktop editor and its UI.
- `crates/vxr-web`: the browser editor, the site and the account Worker.
- `crates/vxr-embed`: the `<voxel-scene>` viewer.
- `crates/vxr-ops`, `crates/vxr-mcp`, `crates/vxr-ai`: the agent tools, the MCP server and the assistant's model clients.
- `spikes/` and `prototype/` are throwaway experiments from before the real build. Do not extend them.
