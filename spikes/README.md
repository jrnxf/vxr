# spikes

Throwaway Phase 2 and Phase AI risk tests (see `docs/plan.md` and `docs/research/recommendation.md`). Not the real build.

- `./dev <command>` runs a command in the Nix dev shell from `nix/flake.nix`: stable Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen` 0.2.129, `wasm-opt`, `twiggy`, `brotli`. `./dev --fuzz <command>` uses nightly Rust with `cargo-fuzz`.
- `./gpu-lock <command>` holds a lock while a timed GPU run happens, so benchmarks from different spikes never share the GPU.
- The island scene `prototype/demo/scene.vox` is generated, not committed: run `node prototype/src/build.mjs` from the repo root first.
- Each spike is its own project in `<name>/` (a Cargo project, or a static page for `ai-browser-calls`) and ends with a `RESULTS.md`.
- Screenshots (`*/shots/`), raw benchmark data (`*/results/`) and the `.vox` test files are not in git. They are in `vxr-phase-2-spikes-data.zip` on the `phase-2-spikes-data` GitHub release; unzip it inside `spikes/`. The `.vox` codec spike (`spikes/vox`, its results and the `voxtool` CLI) was folded into `crates/vxr-vox` and deleted; its `RESULTS.md` is in git history.
