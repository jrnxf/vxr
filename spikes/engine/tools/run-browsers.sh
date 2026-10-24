#!/usr/bin/env bash
# Full browser matrix, each run holding the GPU lock only while it runs.
# tools/run-browsers.sh <port> <browser>...
set -uo pipefail
port=$1; shift
enc() { python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$1"; }
full=$(enc '{}')
load=$(enc '{"load_only":true,"upload_test":false}')
for b in "$@"; do
  for s in terrain:10m terrain:100m scene.vox; do
    n="$b-$(echo "$s" | sed 's/:/-/; s/\.vox//')"
    ../gpu-lock tools/browser-run.sh "$b" "$port" "$n" "scene=$s&cfg=$full" 900
  done
  for s in terrain:200m terrain:300m; do
    n="$b-load-$(echo "$s" | sed 's/:/-/')"
    ../gpu-lock tools/browser-run.sh "$b" "$port" "$n" "scene=$s&cfg=$load" 600
  done
  ../gpu-lock tools/browser-run.sh "$b" "$port" "$b-heap" "heap=4096" 300
done
