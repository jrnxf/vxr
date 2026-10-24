"""Pool repeated runs of one scene on one platform: python3 pool.py <label> files...
Prints median and p95 over all frames of all runs, per view."""
import json
import sys


def q(v, p):
    s = sorted(v)
    return s[round((len(s) - 1) * p)] if s else float("nan")


label, files = sys.argv[1], sys.argv[2:]
runs = [json.load(open(f)) for f in files]
runs = [r for r in runs if "error" not in r]
out = {"label": label, "runs": len(runs), "files": files}
for view in ["orbit_framed", "orbit_close", "orbit_framed_half_res"]:
    for key in ["gpu_ms_all", "wall_ms_all"]:
        v = [x for r in runs for x in (r.get(view) or {}).get(key, [])]
        if v:
            out[f"{view}.{key}"] = {"n": len(v), "median": round(q(v, 0.5), 2), "p95": round(q(v, 0.95), 2)}
cpu = [x for r in runs for x in (r.get("edits") or {}).get("per_edit_cpu_ms", [])[1:]]
lat = [x for r in runs for x in (r.get("edits") or {}).get("per_edit_latency_frames", [])]
fg = [x for r in runs for x in (r.get("edits") or {}).get("per_edit_frame_gpu_ms", [])[1:]]
if cpu:
    out["edit_cpu_ms_steady"] = {"n": len(cpu), "median": round(q(cpu, 0.5), 3), "p95": round(q(cpu, 0.95), 3), "max": round(max(cpu), 3), "mean": round(sum(cpu) / len(cpu), 3)}
    out["edit_frame_gpu_ms"] = {"median": round(q(fg, 0.5), 2), "p95": round(q(fg, 0.95), 2)} if fg else None
    out["edit_latency_frames"] = {"p95": q(lat, 0.95), "max": max(lat), "share_within_1": round(sum(1 for x in lat if x <= 1) / len(lat), 3)}
    out["edit_first"] = [(r.get("edits") or {}).get("first_edit") for r in runs]
print(json.dumps(out))
