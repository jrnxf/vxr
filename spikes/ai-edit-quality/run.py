"""Run the batches: run.py <out-dir> <t1|t2|t4|r2|r3> [--models a,b] [--only id,id] [--jobs n] [--cap usd].

- t1: the 20 edit prompts (island or a new scene), with render_view, screenshots for judging.
- t2: the 10 placement prompts on the island, once without render_view ("text") and once
  with it ("vision").
- t4: the 20 edit prompts on the files with planted instructions.
- r2: round 2 of test 1, the 18 tiered prompts on the `vxr-ops scenes` files, with before and
  after pictures for judging.
- r3: round 3 of test 1, round 2's prompts and scenes on the `vxr-ops` crate (`vxr-ops serve3`:
  point query, named parts, limits per turn, no render_view), the two default models.

Results go to <out-dir>/<test>/.../<prompt-id>/ (run.json, transcript.json, result.vox,
changes.json, views and shots). A run whose run.json exists is skipped, so a batch resumes.
Spend is the sum of every run.json of the round (rounds 2 and 3 are <out-dir>/r2 and r3, round
1 the rest);
no run starts once it passes the cap.
"""

import argparse
import concurrent.futures
import glob
import json
import os
import sys
import threading

import loop
import prompts

HERE = os.path.dirname(os.path.abspath(__file__))
BINARY = os.path.join(HERE, "target", "release", "vxr-ops")
CAP = [14.0]
DEFAULT = ["claude-opus-5-5", "claude-sonnet-5-5", "gpt-6.1-sol"]
lock = threading.Lock()


ROUNDS = ("r2", "r3")
R3_MODELS = ["claude-opus-5-5", "gpt-6.1-sol"]


def spent(out, test=None):
    total = 0.0
    want = test if test in ROUNDS else "r1"
    for p in glob.glob(os.path.join(out, "**", "run.json"), recursive=True):
        seg = p[len(out):].split(os.sep)[1]
        if (seg if seg in ROUNDS else "r1") != want:
            continue
        with open(p) as f:
            total += json.load(f).get("cost", 0.0)
    return total


def jobs(args, files):
    t = args.test
    if t in ROUNDS:
        for pid, _, scene, text in prompts.ROUND2:
            path = "new" if scene == "new" else os.path.join(files, f"{scene}.vox")
            for m in args.models:
                yield (os.path.join(args.out, t, m, pid), m, path, text, t == "r2", False, pid)
        return
    if t in ("t1", "t4"):
        for pid, scene, text in prompts.EDITS:
            if t == "t1":
                path = os.path.join(files, "island.vox") if scene == "island" else "new"
            else:
                path = os.path.join(files, "island-injected.vox" if scene == "island" else "empty-injected.vox")
            for m in args.models:
                yield (os.path.join(args.out, t, m, pid), m, path, text, True, t == "t1", pid)
    else:
        for cond in ("text", "vision"):
            for pid, text in prompts.PLACEMENTS:
                for m in args.models:
                    yield (os.path.join(args.out, t, cond, m, pid), m, os.path.join(files, "island.vox"), text, cond == "vision", False, pid)


def one(job, out):
    path, model, scene, text, vision, shots, pid = job
    if os.path.exists(os.path.join(path, "run.json")):
        return None
    with lock:
        s = spent(out, path[len(out):].split(os.sep)[1])
        if s > CAP[0]:
            print(f"skip {path}: spend ${s:.2f} is over the ${CAP[0]} cap", flush=True)
            return None
    test = path.split(os.sep)[-3]
    r = loop.run(BINARY, model, scene, text, path, vision=vision, shots=shots, pairs=test in ROUNDS,
                 server="serve3" if test == "r3" else "serve")
    c = r.get("card") or {}
    print(f"{pid:18} {model:18} {'vis' if vision else 'txt'} calls {r['calls']:2} err {r['tool_errors']} "
          f"{r['wall_s']:5.1f}s ${r['cost']:.3f} done {r['done']} +{c.get('added')} -{c.get('removed')} ~{c.get('repainted')} "
          f"pal {len(c.get('palette') or [])}{' ERROR ' + r['error'][:200] if r['error'] else ''}", flush=True)
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("test", choices=["t1", "t2", "t4", "r2", "r3"])
    ap.add_argument("--models", default=None)
    ap.add_argument("--only", default="")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--files", default=None, help="folder with the scene files")
    ap.add_argument("--cap", type=float, default=CAP[0], help="stop starting runs past this spend (USD)")
    args = ap.parse_args()
    args.models = args.models.split(",") if args.models else (R3_MODELS if args.test == "r3" else DEFAULT)
    CAP[0] = args.cap
    files = args.files or os.path.join(args.out, "files")
    todo = [j for j in jobs(args, files) if not args.only or j[6] in args.only.split(",")]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as ex:
        for f in [ex.submit(one, j, args.out) for j in todo]:
            try:
                f.result()
            except Exception as e:  # noqa: BLE001
                print(f"run failed: {e}", file=sys.stderr, flush=True)
    print(f"spend so far ${spent(args.out, args.test):.2f}")


if __name__ == "__main__":
    main()
