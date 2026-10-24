#!/usr/bin/env bash
# Full timing matrix, 10 cold loads per row, each batch under the shared GPU lock.
set -uo pipefail
cd "$(dirname "$0")"
L=../gpu-lock
N=${N:-10}
# Chrome: desktop baseline, then mid-phone stand-ins (network + 4x CPU slowdown).
$L ./bench-run.sh chrome c-none          none   1 $N bench.html
$L ./bench-run.sh chrome c-none-split    none   1 $N bench.html split
$L ./bench-run.sh chrome c-10mbit        10mbit 4 $N bench.html
$L ./bench-run.sh chrome c-10mbit-split  10mbit 4 $N bench.html split
$L ./bench-run.sh chrome c-10mbit-child  10mbit 4 $N bench-child.html
$L ./bench-run.sh chrome c-slow4g        slow4g 4 $N bench.html
$L ./bench-run.sh chrome c-slow4g-child  slow4g 4 $N bench-child.html
# Safari and Firefox: no CPU throttling available; network is simulated by the server.
$L ./bench-run.sh safari s-none          none   1 $N bench.html
$L ./bench-run.sh safari s-none-split    none   1 $N bench.html split
$L ./bench-run.sh safari s-10mbit        10mbit 1 $N bench.html
$L ./bench-run.sh safari s-slow4g        slow4g 1 $N bench.html
$L ./bench-run.sh firefox f-none         none   1 $N bench.html
$L ./bench-run.sh firefox f-none-split   none   1 $N bench.html split
$L ./bench-run.sh firefox f-10mbit       10mbit 1 $N bench.html
$L ./bench-run.sh firefox f-slow4g       slow4g 1 $N bench.html
echo ALL DONE
