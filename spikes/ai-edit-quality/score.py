"""Score test 2: score.py <island-dump.json> <run-dir>... prints one line per run and a table.

The dump is `vxr-ops dump island.vox` ([x, y, z, slot] per voxel). A run's marker is the
cells it added in a magenta slot (any added cells if none is magenta). Each prompt's check
reads the island's own voxels: what the marker rests on, or which wall it touches.
"""

import collections
import json
import os
import sys

CHIMNEY, ROOF, WALL, PLANK, WATER, PINE, LEAF = {5}, {11, 12}, {58, 59}, {55, 56}, {37, 39, 40}, {25, 26}, {25, 26, 27, 28, 29}


def load(dump):
    d = {}
    for x, y, z, s in json.load(open(dump)):
        d[(x, y, z)] = s
    return d


def features(d):
    f = {}
    f["chimney_top"] = {p for p, s in d.items() if s in CHIMNEY and p[1] >= 18 and p[0] in (-3, -2)
                        and (p[0], p[1] + 1, p[2]) not in d}
    top = max(p[1] for p, s in d.items() if s in LEAF and p[1] > 12)
    peak = [p for p, s in d.items() if s in LEAF and p[1] == top]
    f["tree_top"] = {p for p, s in d.items() if s in LEAF and p[1] >= top - 1
                     and any(abs(p[0] - q[0]) <= 2 and abs(p[2] - q[2]) <= 2 for q in peak)}
    planks = {p for p, s in d.items() if s in PLANK and p[1] == 8 and p[0] >= 25}
    f["dock_max_x"] = max(p[0] for p in planks)
    f["dock_z"] = (min(p[2] for p in planks), max(p[2] for p in planks))
    f["dock_end"] = {p for p in planks if p[0] >= f["dock_max_x"] - 3}
    walls = {p for p, s in d.items() if s in WALL and 13 <= p[1] <= 17 and -11 <= p[0] <= -1 and 3 <= p[2] <= 11}
    f["wall_west"] = {p for p in walls if p[0] == -11}
    f["wall_back"] = {p for p in walls if p[2] == 3}
    f["ridge"] = {p for p, s in d.items() if s in ROOF and p[1] == 23 and -9 <= p[0] <= -3}
    f["water"] = {p for p, s in d.items() if s in WATER and p[1] == 6}
    ptop = max(p[1] for p, s in d.items() if s in PINE)
    f["pine"] = next(p for p, s in d.items() if s in PINE and p[1] == ptop)
    return f


def magenta(h):
    r, g, b = int(h[1:3], 16), int(h[3:5], 16), int(h[5:7], 16)
    return r > 180 and b > 180 and g < 100


def marker(run):
    """The marker's cells (magenta ones, else every added cell), how many of them replaced a
    voxel of the island, and how many other cells changed."""
    ch = json.load(open(os.path.join(run, "changes.json")))
    m = [c for c in ch["cells"] if c[4] != 0 and magenta(ch["palette"][str(c[4])])]
    if not m:
        m = [c for c in ch["cells"] if c[3] == 0 and c[4] != 0]
    embedded = sum(c[3] != 0 for c in m)
    return [tuple(c[:3]) for c in m], embedded, len(ch["cells"]) - len(m)


def check(pid, cells, embedded, d, f):
    """(correct, why). None for p02: the tallest tree reaches the island's top bound (y 30), so
    no marker fits on it and the prompt cannot be answered."""
    if pid == "p02-tallest-tree":
        return None, "no room above the tallest tree (it reaches the top bound)"
    if not cells:
        return False, "no marker"
    if embedded:
        return False, f"{embedded} of its cells replaced island voxels (sunk into what it should sit on)"
    cs = set(cells)
    bottom = min(c[1] for c in cells)
    base = [c for c in cells if c[1] == bottom]
    under = {(c[0], bottom - 1, c[2]) for c in base}
    support = {p for p in under if p in d and p not in cs}
    rests = bool(support)
    xs, zs = [c[0] for c in cells], [c[2] for c in cells]
    cx, cz = sum(xs) / len(xs), sum(zs) / len(zs)
    touches = lambda wall, dx, dz: any((c[0] + dx, c[1], c[2] + dz) in wall for c in cs)
    if pid == "p01-chimney":
        return bool(support & f["chimney_top"]), f"rests on {sorted(support)[:2]}"
    if pid == "p02-tallest-tree":
        return bool(support & f["tree_top"]), f"bottom y {bottom}, tree top y {max(p[1] for p in f['tree_top'])}"
    if pid == "p03-dock-end":
        return bool(support & f["dock_end"]), f"bottom y {bottom}, x {min(xs)}..{max(xs)}, dock ends at x {f['dock_max_x']}"
    if pid == "p04-door":
        # The ground in front of the door tops out at y 14; the roof's eave above is at y 18.
        ok = rests and 12 <= cz <= 15.5 and -9 <= cx <= -4 and bottom <= 16
        return ok, f"centre x {cx:.1f} z {cz:.1f}, bottom y {bottom}, rests {rests}"
    if pid == "p05-ridge":
        return bool(support & f["ridge"]), f"bottom y {bottom}, centre x {cx:.1f} z {cz:.1f}"
    if pid == "p06-left-of-house":
        return rests and touches(f["wall_west"], 1, 0), f"x {min(xs)}..{max(xs)}, z centre {cz:.1f}, bottom y {bottom}, rests {rests}"
    if pid == "p07-behind-house":
        return rests and touches(f["wall_back"], 0, 1), f"z {min(zs)}..{max(zs)}, x centre {cx:.1f}, bottom y {bottom}, rests {rests}"
    if pid == "p08-off-dock":
        dz = f["dock_z"]
        ok = bool(support & f["water"]) and f["dock_max_x"] < min(xs) <= f["dock_max_x"] + 6 and dz[0] - 2 <= cz <= dz[1] + 2
        return ok, f"x {min(xs)}..{max(xs)} (dock ends {f['dock_max_x']}), z centre {cz:.1f}, bottom y {bottom}"
    if pid == "p09-by-pine":
        x0, _, z0 = f["pine"]
        ok = rests and min(xs) > x0 and cx - x0 <= 8 and abs(cz - z0) <= 3
        return ok, f"pine at x {x0} z {z0}; marker centre x {cx:.1f} z {cz:.1f}, rests {rests}"
    if pid == "p10-sea-front":
        ok = bool(support) and support <= f["water"] and cz > 12 and abs(cx + 6) <= 8
        return ok, f"centre x {cx:.1f} z {cz:.1f}, bottom y {bottom}, on water {bool(support) and support <= f['water']}"
    raise ValueError(pid)


def main():
    d = load(sys.argv[1])
    f = features(d)
    rows = collections.defaultdict(dict)
    for run in sys.argv[2:]:
        if not os.path.exists(os.path.join(run, "run.json")):
            continue
        pid = os.path.basename(run.rstrip("/"))
        cond = os.path.basename(os.path.dirname(os.path.dirname(run.rstrip("/"))))
        model = os.path.basename(os.path.dirname(run.rstrip("/")))
        cells, embedded, other = marker(run)
        ok, why = check(pid, cells, embedded, d, f)
        r = json.load(open(os.path.join(run, "run.json")))
        if ok is not None:
            rows[(model, cond)][pid] = ok
        mark = "n/a " if ok is None else "ok  " if ok else "MISS"
        print(f"{model:18} {cond:6} {pid:18} {mark} marker {len(cells)} cells, other changes {other}; "
              f"calls {r['calls']} ({r['by_tool'].get('render_view', 0)} views) {r['wall_s']:.1f}s ${r['cost']:.3f}; {why}")
    for (model, cond), v in sorted(rows.items()):
        print(f"{model} {cond}: {sum(v.values())} of {len(v)} correct (p02 left out)")


if __name__ == "__main__":
    main()
