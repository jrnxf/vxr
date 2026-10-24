#!/usr/bin/env bash
# Average "Device Utilization %" macOS reports for the GPU over ~1 s (background load).
for _ in 1 2 3 4 5; do
  ioreg -r -d 1 -c IOAccelerator | grep -o '"Device Utilization %"=[0-9]*' | head -1 | cut -d= -f2
  sleep 0.2
done | awk '{s += $1} END {printf "%.0f\n", s / NR}'
