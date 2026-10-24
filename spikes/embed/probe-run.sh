#!/usr/bin/env bash
# Shader compile probe in one browser. Run inside ../gpu-lock.
#   ./probe-run.sh <chrome|safari|firefox> <label>
set -euo pipefail
cd "$(dirname "$0")"
browser=$1 label=$2
rm -f "results/$label.jsonl"
url="http://127.0.0.1:8790/compile-probe.html?n=5&report=1&f=$label"
case $browser in
  chrome) CHROME_DEVTOOLS_AXI_SESSION=embed CHROME_DEVTOOLS_AXI_HEADED=1 chrome-devtools-axi open "$url" >/dev/null 2>&1 || true ;;
  safari) open -a Safari "$url" ;;
  firefox) open -a Firefox "$url" ;;
esac
deadline=$((SECONDS + 120))
until [[ -f "results/$label.jsonl" ]]; do (( SECONDS > deadline )) && { echo "timeout: $label"; exit 1; }; sleep 1; done
echo "$label done"
