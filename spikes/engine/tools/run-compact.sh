#!/usr/bin/env bash
# Dense vs compacted layouts, interleaved so both see the same background load.
# Each run takes the GPU lock on its own.
set -uo pipefail
port=$1
enc() { python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$1"; }
# Worst-case edits on hollow models and the terrain: add-only brushes plus whole-brick fills.
for spec in fetched/teapot.vox fetched/dragon.vox fetched/nature.vox terrain:10m; do
  n=$(echo "$spec" | sed 's|.*/||; s|:|-|; s|\.vox||')
  for L in dense compact; do
    ../gpu-lock ./target/release/bench "$spec" --layout $L --add-only --fill 120 --frames 60 --no-half --no-upload > "results/worst-native-$n-$L.json" 2>/dev/null
    echo "worst native $n $L done"
  done
done
for rep in 1 2 3; do
  for spec in terrain:10m terrain:100m; do
    n=$(echo "$spec" | sed 's|:|-|')
    for L in dense compact; do
      ../gpu-lock ./target/release/bench "$spec" --layout $L > "results/cmp-native-$n-$L-r$rep.json" 2>/dev/null
      echo "native $n $L r$rep done"
    done
  done
done
for rep in 1 2 3; do
  for b in chrome safari firefox; do
    [[ $b == firefox && $rep == 3 ]] && continue
    for s in terrain:10m terrain:100m; do
      for L in dense compact; do
        cfg=$(enc "{\"layout\":\"$L\"}")
        n="cmp-$b-$(echo "$s" | sed 's/:/-/')-$L-r$rep"
        ../gpu-lock tools/browser-run.sh "$b" "$port" "$n" "scene=$s&cfg=$cfg" 900
      done
    done
  done
done
for b in chrome safari; do
  for L in dense compact; do
    cfg=$(enc "{\"layout\":\"$L\",\"add_only\":true,\"fill_edits\":120,\"frames\":60,\"half\":false,\"upload_test\":false}")
    ../gpu-lock tools/browser-run.sh "$b" "$port" "worst-$b-teapot-$L" "scene=teapot.vox&cfg=$cfg" 600
  done
done
