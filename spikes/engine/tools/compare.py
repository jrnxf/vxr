"""Dense vs compacted side by side from results/cmp-* and results/worst-* files."""
import glob
import json
import statistics as st


def load(f):
    try:
        return json.load(open(f))
    except ValueError:
        return {"error": "unreadable or still running"}


def q(v, p):
    s = sorted(v)
    return s[round((len(s) - 1) * p)] if s else float("nan")


def pooled(files, view, key="gpu_ms_all"):
    """Median run: the median over runs of each run's median and p95, so one run hit by a
    background GPU burst does not move the result. Outlier runs are listed."""
    per = []
    for f in files:
        v = (load(f).get(view) or {}).get(key, [])
        if v:
            per.append((q(v, 0.5), q(v, 0.95), f))
    if not per:
        return None
    med = st.median([p[0] for p in per])
    out = [p[2].split("-r")[-1].split(".")[0] for p in per if p[0] > med * 1.25]
    return (st.median([p[0] for p in per]), st.median([p[1] for p in per]), len(per), out)


def fmt(t):
    if not t:
        return "-"
    flag = f" (slow runs: r{','.join(t[3])})" if t[3] else ""
    return f"{t[0]:.2f} / {t[1]:.2f} n{t[2]}{flag}"


print("FRAME TIME (gpu ms, median run: median / p95)")
for plat in ["native", "chrome", "safari", "firefox"]:
    for sc in ["terrain-10m", "terrain-100m"]:
        for view in ["orbit_framed", "orbit_close", "orbit_framed_half_res"]:
            row = []
            for L in ["dense", "compact"]:
                fs = sorted(glob.glob(f"results/cmp-{plat}-{sc}-{L}-r*.json"))
                row.append(fmt(pooled(fs, view)))
            print(f"  {plat:8s} {sc:13s} {view:22s} dense {row[0]:>28s} | compact {row[1]:>28s}")

print("EDITS (radius 16, alternating; steady CPU ms median / p95 / max; frame gpu median; within 1 frame)")
for plat in ["native", "chrome", "safari", "firefox"]:
    for sc in ["terrain-10m", "terrain-100m"]:
        for L in ["dense", "compact"]:
            fs = sorted(glob.glob(f"results/cmp-{plat}-{sc}-{L}-r*.json"))
            ds = [d for d in (load(f) for f in fs) if "error" not in d]
            ed = [d.get("edits") for d in ds if d.get("edits")]
            if not ed:
                continue
            cpu = [x for e in ed for x in e["per_edit_cpu_ms"][1:]]
            fg = [x for e in ed for x in e["per_edit_frame_gpu_ms"][1:]]
            lat = [x for e in ed for x in e["per_edit_latency_frames"]]
            re = [e["reallocs_per_edit"]["median"] for e in ed]
            rb = sum(e["rebuilds"] for e in ed)
            first = [e["first_edit"] for e in ed]
            print(
                f"  {plat:8s} {sc:13s} {L:8s} n={len(cpu)} cpu {q(cpu,.5):.3f} / {q(cpu,.95):.3f} / {max(cpu):.3f} mean {st.mean(cpu):.3f}"
                f" | frame {q(fg,.5):.2f} | within1 {sum(1 for x in lat if x <= 1)/len(lat):.3f} | reallocs/edit {re} rebuilds {rb}"
                f" | first cpu {[round((f['brush_ms'] or 0)+(f['upload_ms'] or 0),2) for f in first]}"
            )

print("FULL POOL UPLOAD (pack+queue ms, total upload ms, MB) and 256 MB throughput")
for plat in ["native", "chrome", "safari", "firefox"]:
    for sc in ["terrain-10m", "terrain-100m"]:
        for L in ["dense", "compact"]:
            fs = sorted(glob.glob(f"results/cmp-{plat}-{sc}-{L}-r*.json"))
            ds = [d for d in (load(f) for f in fs) if "error" not in d]
            if not ds:
                continue
            u = [d["upload"] for d in ds]
            tp = [d.get("upload_256mb", {}).get("mb_per_s_median") for d in ds]
            tp = [t for t in tp if t]
            print(
                f"  {plat:8s} {sc:13s} {L:8s} pack+queue {st.median([x['build_and_queue_ms'] for x in u]):.1f} total {st.median([x['upload_ms'] for x in u]):.1f} ms"
                f" ({u[0]['upload_mb']:.1f} MB, {u[0]['bytes_per_voxel_used']:.2f} B/voxel) 256MB {st.median(tp) if tp else 0:.0f} MB/s"
                f" busy {[d.get('gpu_busy_before_pct') for d in ds]}"
            )

print("WORST CASE EDITS")
for f in sorted(glob.glob("results/worst-*.json")):
    d = load(f)
    if "error" in d:
        print(" ", f, d["error"])
        continue
    e, fl = d.get("edits") or {}, d.get("fill_edits") or {}
    print(
        f"  {f.split('/')[-1]:38s} add-only brushes: cpu {e['steady']['cpu_edit_ms']['median']:.3f} / {e['steady']['cpu_edit_ms']['p95']:.3f} / {e['steady']['cpu_edit_ms']['max']:.3f}"
        f" reallocs/edit {e['reallocs_per_edit']['median']:.0f} rebuilds {e['rebuilds']} within1 {e['all']['edits_within_1_frame']}/{e['n']}"
        f" B/v after {e['bytes_per_voxel_used_after']:.2f} heap {e['heap_after']}"
    )
    print(
        f"  {'':38s} brick fills n={fl['n']} (from {fl['voxels_before_fill']['median']:.0f} voxels): cpu {fl['cpu_edit_ms']['median']:.3f} / {fl['cpu_edit_ms']['p95']:.3f} / {fl['cpu_edit_ms']['max']:.3f}"
        f" upload {fl['upload_ms']['median']:.3f} reallocs/edit {fl['reallocs_per_edit']['median']:.0f} rebuilds {fl['rebuilds']} latency max {fl['latency_frames']['max']}"
    )
