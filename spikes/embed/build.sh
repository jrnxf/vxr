#!/usr/bin/env bash
# Build the embed wasm, run wasm-bindgen and wasm-opt, print raw and brotli q11 sizes.
# Usage: ./build.sh [wasm-opt level, default -O3]
set -euo pipefail
cd "$(dirname "$0")"
OPT="${1:--O3}"
cargo build --release --target wasm32-unknown-unknown
rm -rf pkg && mkdir -p pkg
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/release/vxr_embed_spike.wasm
if [[ "$OPT" != "none" ]]; then
  wasm-opt "$OPT" --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
    --strip-debug --strip-producers -o pkg/vxr_embed_spike_bg.wasm pkg/vxr_embed_spike_bg.wasm
fi
for f in pkg/vxr_embed_spike_bg.wasm pkg/vxr_embed_spike.js; do brotli -q 11 -f -o "$f.br" "$f"; done
brotli -q 11 -f -o pkg/voxel-scene.js.br web/voxel-scene.js
brotli -q 11 -f -o pkg/scene.vox.br ../../prototype/demo/scene.vox
[[ -f web/poster.webp ]] || true
size() { local raw br; raw=$(wc -c < "$1"); br=$(brotli -q 11 -c "$1" | wc -c); printf "%-28s raw %8d  br %7d\n" "$(basename "$1")" "$raw" "$br"; }
size pkg/vxr_embed_spike_bg.wasm
size pkg/vxr_embed_spike.js
[[ -f web/voxel-scene.js ]] && size web/voxel-scene.js
