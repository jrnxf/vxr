#!/usr/bin/env bash
# Browser test 2 run: run-browser.sh <chrome|safari|firefox> <name> [steps] [config json]
# Needs `python3 web/serve.py 8742 results` running. Run inside ../gpu-lock.
set -euo pipefail
browser=$1
name=$2
steps=${3:-edit,rays,clean,recolor,half}
config=${4:-"{}"}
rm -f "results/$name.json" "results/$name.log"
echo "$name: $(./gpu-busy.sh)% $(TZ=Europe/Lisbon date +%H:%M:%S) Lisbon" >> results/background-gpu-busy.txt
q=$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$config")
url="http://127.0.0.1:8742/index.html?name=$name&steps=$steps&config=$q"
case $browser in
  chrome) CHROME_DEVTOOLS_AXI_SESSION=preview CHROME_DEVTOOLS_AXI_HEADED=1 chrome-devtools-axi open "$url" > /dev/null ;;
  safari) open -a Safari "$url" ;;
  firefox) open -a Firefox "$url" ;;
esac
for _ in $(seq 1 900); do
  [ -f "results/$name.json" ] && exit 0
  sleep 1
done
echo "timed out waiting for results/$name.json" >&2
exit 1
