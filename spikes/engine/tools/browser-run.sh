#!/usr/bin/env bash
# Open the bench page in a browser and wait for its result file.
# tools/browser-run.sh <chrome|safari|firefox> <port> <name> <query> [timeout s]
set -euo pipefail
browser=$1 port=$2 name=$3 query=$4 timeout=${5:-600}
out="results/$name.json"
rm -f "$out"
busy=$(for _ in 1 2 3; do ioreg -r -d 1 -c IOAccelerator | grep -o '"Device Utilization %"=[0-9]*' | head -1 | cut -d= -f2; sleep 0.2; done | awk '{s+=$1} END {printf "%.0f", s/NR}')
url="http://127.0.0.1:$port/index.html?name=$name&$query"
case $browser in
  chrome)
    export CHROME_DEVTOOLS_AXI_SESSION=engine CHROME_DEVTOOLS_AXI_HEADED=1
    for id in $(chrome-devtools-axi pages 2>/dev/null | grep -E '^ +[0-9]+,' | awk -F, '{print $1}' | tr -d ' '); do
      [[ "$id" == 1 ]] || chrome-devtools-axi closepage "$id" >/dev/null 2>&1 || true
    done
    chrome-devtools-axi newpage "$url" >/dev/null 2>&1 || true ;;
  safari) open -a Safari "$url&blank=1" ;;
  firefox) open -a Firefox "$url&blank=1" ;;
esac
for _ in $(seq 1 "$timeout"); do
  [[ -s "$out" ]] && { echo "ok $out (gpu busy before: $busy%)"; exit 0; }
  sleep 1
done
echo "timeout waiting for $out"
exit 1
