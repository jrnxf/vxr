#!/usr/bin/env python3
"""Medians (and min/max) per batch from results/*.jsonl. Prints a Markdown table."""
import glob
import json
import os
import statistics
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
COLS = [
    ("poster_after_tag", "poster"),
    ("first_frame_after_visible", "first frame"),
    ("wasm_download_end_after_visible", "wasm in"),
    ("wasm_compile_tail", "compile tail"),
    ("wasm_compile_only", "compile only"),
    ("device", "device"),
    ("pipelines", "pipelines"),
    ("vox_fetch_end_after_visible", "vox in"),
    ("parse_upload", "parse+upload"),
    ("first_gpu", "first GPU"),
    ("steady_gpu_frame", "steady GPU"),
]


def med(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    return statistics.median(xs) if xs else None


def fmt(v):
    return "-" if v is None else ("%.0f" % v if v >= 10 else "%.1f" % v)


def main():
    files = sorted(glob.glob(os.path.join(HERE, "results", "*.jsonl")))
    only = sys.argv[1:]
    print("| batch | n | " + " | ".join(c[1] for c in COLS) + " |")
    print("|---|---|" + "---|" * len(COLS))
    for f in files:
        name = os.path.basename(f)[:-6]
        if only and name not in only:
            continue
        rows = [json.loads(l) for l in open(f) if l.strip()]
        rows = [r for r in rows if "first_frame_after_visible" in r]
        if not rows:
            continue
        errs = sum(1 for r in rows if r.get("error"))
        cells = []
        for key, _ in COLS:
            vals = [r.get(key) for r in rows]
            m = med(vals)
            nums = [v for v in vals if isinstance(v, (int, float))]
            if m is not None and key in ("poster_after_tag", "first_frame_after_visible") and nums:
                cells.append("%s (%s-%s)" % (fmt(m), fmt(min(nums)), fmt(max(nums))))
            else:
                cells.append(fmt(m))
        print("| %s | %d%s | %s |" % (name, len(rows), " (%d err)" % errs if errs else "", " | ".join(cells)))


if __name__ == "__main__":
    main()
