"""Print the headline numbers from a browser or native results file: summarize.py <file.json>..."""
import json
import sys


def edit_line(tag, e):
    off, on = e["off"], e["on"]
    f = lambda x, k: round(x[k]["median"], 3)
    return (f"{tag}: off {f(off, 'frame_gpu_ms')} / wall {f(off, 'wall_ms_per_frame')} | on {f(on, 'frame_gpu_ms')} "
            f"p95 {round(on['frame_gpu_ms']['p95'], 3)} / wall {f(on, 'wall_ms_per_frame')} p95 {round(on['wall_ms_per_frame']['p95'], 3)} | "
            f"added gpu {round(e['added_ms_median'], 3)} wall {round(e.get('added_wall_ms_median', float('nan')), 3)} | "
            f"trace {f(on, 'trace_gpu_ms')} denoise {f(on, 'denoise_gpu_ms')} spp/frame {f(on, 'spp_per_frame')} spp/s {round(on['spp_per_s_at_vsync'], 1)}")


for path in sys.argv[1:]:
    d = json.load(open(path))
    print("==", path, d.get("user_agent", d.get("platform")))
    if "error" in d:
        print("  error", d["error"])
        continue
    a = d.get("adapter", {})
    print("  timestamps", a.get("timestamp_query"), "maxStorageBuffersPerShaderStage", a.get("maxStorageBuffersPerShaderStage"), d.get("calibration") or d.get("notes"))
    for view in ["framed", "close"]:
        v = d.get(view)
        if not v:
            continue
        if "edit" in v:
            print("  " + edit_line(view, v["edit"]))
        if "flat_out" in v:
            fo = v["flat_out"]
            print(f"  flat out: full sample {round(fo['full_sample_gpu_ms']['median'], 3)} ms, {round(fo['spp_per_gpu_second'], 1)} spp/GPU-s, denoise+resolve {round(fo['denoise_resolve_gpu_ms']['median'], 3)}")
        if "rays" in v:
            r = v["rays"]
            print(f"  rays: primary {round(r['primary']['mrays_per_s'])} Mrays/s, bounce {round(r['bounce']['mrays_per_s'])} Mrays/s ({r['primary']['timing']})")
        if "clean" in v:
            c = v["clean"]
            print(f"  clean: idle boost {c['idle_boost']['clean_at_ms']} ms, fixed slice {c['fixed_slice']['clean_at_ms']} ms")
        if "recolor" in v:
            print(f"  recolor: {v['recolor']['frames_until_99pct_updated']} frame(s), {v['recolor']['fraction_updated_per_frame']}")
    if "half_res" in d:
        print("  " + edit_line("half res", d["half_res"]))
