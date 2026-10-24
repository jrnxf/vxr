#!/usr/bin/env bash
# Build the wasm bench into web/pkg (run from spikes/engine).
set -euo pipefail
../dev cargo build --release --target wasm32-unknown-unknown --lib
../dev wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/vxr_engine_spike.wasm
ls -la web/pkg
