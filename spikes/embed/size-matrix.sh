#!/usr/bin/env bash
# Size of wasm (after wasm-bindgen, then wasm-opt) for each build setting. Brotli q11.
set -euo pipefail
cd "$(dirname "$0")"
row() { # label, wasm-opt flag
  local f=pkg/vxr_embed_spike_bg.wasm tmp=target/size-tmp.wasm
  wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/release/vxr_embed_spike.wasm
  for o in none -Oz -Os -O3; do
    if [[ $o == none ]]; then cp $f $tmp; else wasm-opt $o --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue --strip-debug --strip-producers -o $tmp $f; fi
    printf "%-34s wasm-opt %-5s raw %7d br %6d\n" "$1" "$o" "$(wc -c < $tmp)" "$(brotli -q 11 -c $tmp | wc -c)"
  done
}
for ol in z s 3; do
  CARGO_PROFILE_RELEASE_OPT_LEVEL=$ol cargo build -q --release --target wasm32-unknown-unknown
  row "opt-level=$ol lto=fat cu=1"
done
CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build -q --release --target wasm32-unknown-unknown
row "opt-level=z lto=off cu=16"
CARGO_PROFILE_RELEASE_OPT_LEVEL=z CARGO_PROFILE_RELEASE_STRIP=false cargo build -q --release --target wasm32-unknown-unknown
row "opt-level=z lto=fat strip=false"
