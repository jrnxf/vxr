#!/usr/bin/env bash
# One timing batch. Run it inside ../gpu-lock so no other spike shares the GPU.
#   ./bench-run.sh <chrome|safari|firefox> <label> <none|10mbit|slow4g> <cpu-slowdown> <runs> <page> [split]
# Results land in results/<label>.jsonl, one line per cold load.
set -euo pipefail
cd "$(dirname "$0")"
browser=$1 label=$2 profile=$3 cpu=$4 runs=$5 page=$6 split=${7:-}
base="http://127.0.0.1:8790"
curl -sf -X POST "$base/profile?name=$profile" >/dev/null
rm -f "results/$label.jsonl"
url="$base/$page?runs=$runs&i=0&f=$label&report=1${split:+&split=1}"
case $browser in
  chrome)
    export CHROME_DEVTOOLS_AXI_SESSION=embed CHROME_DEVTOOLS_AXI_HEADED=1
    chrome-devtools-axi emulate --cpu "$cpu" >/dev/null
    chrome-devtools-axi open "$url" >/dev/null 2>&1 || true ;;
  safari) open -a Safari "$url" ;;
  firefox) open -a Firefox "$url" ;;
esac
deadline=$((SECONDS + 60 + runs * 20))
until [[ -f "results/$label.jsonl" && $(wc -l < "results/$label.jsonl") -ge $runs ]]; do
  if (( SECONDS > deadline )); then echo "timeout: $label"; break; fi
  sleep 1
done
[[ $browser == chrome ]] && chrome-devtools-axi emulate --cpu 1 >/dev/null
curl -sf -X POST "$base/profile?name=none" >/dev/null
echo "$label: $(wc -l < "results/$label.jsonl" 2>/dev/null || echo 0) runs"
