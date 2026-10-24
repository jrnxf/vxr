"""Print the headline numbers from bench result JSON files: python3 summarize.py results/*.json"""
import json
import sys


def g(d, *path):
    for p in path:
        if d is None:
            return None
        d = d.get(p) if isinstance(d, dict) else None
    return d


def f(v, nd=2):
    return "-" if v is None else f"{v:.{nd}f}"


for path in sys.argv[1:]:
    try:
        d = json.load(open(path))
    except Exception as e:  # noqa: BLE001
        print(path, "unreadable", e)
        continue
    if "error" in d:
        print(f"{path}: ERROR {d['error']}")
        continue
    up = d.get("upload", {})
    print(f"== {path}  [{d.get('platform', '?')}] {d.get('scene')}  voxels={up.get('voxels')}  load={d.get('load_avg', '')} gpu_busy_before={d.get('gpu_busy_before_pct')}")
    for k in ["orbit_framed", "orbit_close", "orbit_framed_half_res"]:
        o = d.get(k)
        if not o:
            continue
        print(
            f"  {k:24s} gpu med {f(g(o, 'gpu_ms', 'median'))} p95 {f(g(o, 'gpu_ms', 'p95'))}"
            f" | wall med {f(g(o, 'wall_ms_per_frame_batched', 'median'))} p95 {f(g(o, 'wall_ms_per_frame_batched', 'p95'))}"
        )
    e = d.get("edits")
    if e:
        print(
            f"  edits n={e.get('n')} first cpu {f(g(e, 'first_edit', 'brush_ms'))}+{f(g(e, 'first_edit', 'upload_ms'))} ms"
            f" | steady cpu med {f(g(e, 'steady', 'cpu_edit_ms', 'median'))} p95 {f(g(e, 'steady', 'cpu_edit_ms', 'p95'))} max {f(g(e, 'steady', 'cpu_edit_ms', 'max'))}"
            f" | frame gpu med {f(g(e, 'steady', 'frame_gpu_ms', 'median'))} | latency frames p95 {g(e, 'all', 'latency_frames', 'p95')} max {g(e, 'all', 'latency_frames', 'max')}"
            f" within1={g(e, 'all', 'edits_within_1_frame')} budget {e.get('budget_ms')} visible {e.get('visible')}"
        )
    print(
        f"  memory B/voxel used {f(up.get('bytes_per_voxel_used'))} alloc {f(up.get('bytes_per_voxel_allocated'))}"
        f" compacted-est {f(up.get('bytes_per_voxel_compacted_estimate'))} | scene upload {f(up.get('upload_ms'), 1)} ms ({f(up.get('upload_mb'), 1)} MB)"
        f" build {f(d.get('scene_build_ms'), 0)} ms"
    )
    u = d.get("upload_256mb")
    if u:
        print(f"  256 MB upload: med {f(g(u, 'ms', 'median'), 1)} ms = {f(u.get('mb_per_s_median'), 0)} MB/s")
    if d.get("frames"):
        print(f"  load-only frames: {d['frames']}")
