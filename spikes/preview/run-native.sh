#!/usr/bin/env bash
# All native test 2 measurements. Run from spikes/preview after `../dev cargo build --release`,
# inside ../gpu-lock so no other spike benchmarks at the same time. The GPU busy level macOS
# reports just before each run (other apps) goes to results/background-gpu-busy.txt.
set -euo pipefail
out=results/native
mkdir -p "$out" shots
b=./target/release/pbench
busy() { echo "native $1: $(./gpu-busy.sh)% $(TZ=Europe/Lisbon date +%H:%M:%S) Lisbon" >> results/background-gpu-busy.txt; }
busy framed
$b --view framed --only edit --only rays --only clean --only recolor --shots shots/framed > "$out/framed.json" 2> "$out/framed.log"
busy close
$b --view close --only edit --only rays --only clean --shots shots/close > "$out/close.json" 2> "$out/close.log"
busy half-res
$b --view framed --only edit --width 1512 --height 982 > "$out/half-res.json" 2> "$out/half-res.log"
