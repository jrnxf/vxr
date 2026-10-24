#!/usr/bin/env bash
# Final runs: 3 native reps per scene, then 3 Chrome and Safari reps and 2 Firefox reps for the
# terrains. Each run takes the GPU lock on its own so other spikes get turns in between.
set -uo pipefail
port=$1
enc() { python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$1"; }
for rep in 1 2 3; do
  for spec in terrain:10m terrain:100m ../../prototype/demo/scene.vox fetched/monu3.vox; do
    n=$(echo "$spec" | sed 's|.*/||; s|:|-|; s|\.vox||')
    ../gpu-lock ./target/release/bench "$spec" > "results/final-native-$n-r$rep.json" 2>/dev/null
    echo "native $n r$rep done"
  done
done
full=$(enc '{}')
for rep in 1 2 3; do
  for b in chrome safari firefox; do
    [[ $b == firefox && $rep == 3 ]] && continue
    for s in terrain:10m terrain:100m; do
      n="final-$b-$(echo "$s" | sed 's/:/-/')-r$rep"
      ../gpu-lock tools/browser-run.sh "$b" "$port" "$n" "scene=$s&cfg=$full" 900
    done
  done
done
