"""Build round 2's (or 3's) blind judging page: judge2.py <r2-dir> <page-dir> <key.json> [round].

Each result gets a random id. A card shows the tier and the prompt, then the scene before and
after side by side through the same camera twice: the full view, and a close-up on the change
(or the other side, when the change has no small box). Results are shuffled and the model is
hidden. The keyboard flow is round 1's (A, R, N, J, K, moving on to the next unjudged card).
The id to model key goes to <key.json>, outside the page folder.
"""

import json
import os
import random
import shutil
import sys

import prompts

r2, page, keyfile = sys.argv[1:4]
ROUND = sys.argv[4] if len(sys.argv) > 4 else "2"
os.makedirs(os.path.join(page, "img"), exist_ok=True)
info = {pid: (tier, scene, text) for pid, tier, scene, text in prompts.ROUND2}
SCENES = {"new": "an empty scene", "shapes": "a few shapes on a grass plate", "farm": "the farm", "island-tall": "the island"}
results = []
for model in sorted(os.listdir(r2)):
    for pid in sorted(os.listdir(os.path.join(r2, model))):
        d = os.path.join(r2, model, pid)
        if os.path.exists(os.path.join(d, "run.json")):
            results.append((model, pid, d))
rng = random.Random(20261028)
rng.shuffle(results)
key, cards = {}, []
for n, (model, pid, d) in enumerate(results, 1):
    rid = f"c{n:02}"
    key[rid] = {"model": model, "prompt": pid}
    for name in ("before-full", "after-full", "before-second", "after-second"):
        shutil.copy(os.path.join(d, f"{name}.png"), os.path.join(page, "img", f"{rid}-{name}.png"))
    r = json.load(open(os.path.join(d, "run.json")))
    c = r.get("card") or {}
    tier, scene, prompt = info[pid]
    counts = f"+{c.get('added', 0)} added, {c.get('removed', 0)} removed, {c.get('repainted', 0)} repainted, {len(c.get('palette') or [])} palette slots changed"
    if r.get("error"):
        counts += " (the request failed part way)"
    second = "Close-up on the change" if r.get("pairs") == "close" else "From the other side"
    cards.append({"id": rid, "tier": tier, "scene": SCENES[scene], "prompt": prompt, "counts": counts, "second": second})
with open(keyfile, "w") as f:
    json.dump(key, f, indent=1)

models = len({k["model"] for k in key.values()})
page_html = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>vxr test 1 round __ROUND__</title>
<style>
:root { --bg: #f6f6f4; --fg: #1c1c1e; --muted: #6b6b70; --card: #ffffff; --line: #deded9; --ok: #1f7a3d; --no: #b3261e; --chip: #ececea; }
@media (prefers-color-scheme: dark) { :root { --bg: #151517; --fg: #ececee; --muted: #9a9aa2; --card: #1f1f23; --line: #34343a; --ok: #5cc27c; --no: #ef6b61; --chip: #2c2c32; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 15px/1.45 system-ui, -apple-system, sans-serif; }
header { position: sticky; top: 0; z-index: 2; background: var(--bg); border-bottom: 1px solid var(--line); padding: 12px 16px; display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
header h1 { font-size: 16px; margin: 0 auto 0 0; }
button { font: inherit; border: 1px solid var(--line); background: var(--card); color: var(--fg); border-radius: 8px; padding: 6px 12px; cursor: pointer; }
main { max-width: 1680px; margin: 0 auto; padding: 16px; }
.intro { color: var(--muted); max-width: 80ch; }
.card { background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 16px; margin: 20px 0; scroll-margin-top: 64px; }
.card.focus { outline: 2px solid #5b7cfa; }
.top { display: flex; gap: 12px; align-items: baseline; flex-wrap: wrap; }
.id { font-family: ui-monospace, monospace; color: var(--muted); }
.tier { font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: .06em; background: var(--chip); border-radius: 999px; padding: 2px 10px; }
.prompt { font-size: 20px; font-weight: 600; flex: 1 1 500px; }
.meta { color: var(--muted); font-size: 13px; margin: 4px 0 12px; }
.row-label { font-size: 13px; color: var(--muted); margin: 12px 0 6px; }
.pair { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
@media (max-width: 760px) { .pair { grid-template-columns: 1fr; } }
figure { margin: 0; }
figure img { width: 100%; display: block; border-radius: 8px; border: 1px solid var(--line); cursor: zoom-in; }
figcaption { font-size: 13px; font-weight: 600; margin: 0 0 4px; }
.verdict { display: flex; gap: 8px; margin-top: 14px; align-items: center; flex-wrap: wrap; }
.verdict button.on.accept { background: var(--ok); color: #fff; border-color: var(--ok); }
.verdict button.on.reject { background: var(--no); color: #fff; border-color: var(--no); }
.verdict input { flex: 1 1 240px; font: inherit; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--bg); color: var(--fg); }
.verdict input:focus { outline: 2px solid #5b7cfa; }
.keys { color: var(--muted); font-size: 13px; }
#zoom { position: fixed; inset: 0; background: rgba(0,0,0,.88); display: none; align-items: center; justify-content: center; z-index: 5; cursor: zoom-out; }
#zoom img { max-width: 98vw; max-height: 98vh; }
textarea { width: 100%; height: 160px; font-family: ui-monospace, monospace; font-size: 12px; background: var(--card); color: var(--fg); border: 1px solid var(--line); border-radius: 8px; }
</style></head><body>
<header><h1>vxr test 1, round __ROUND__: blind judging</h1><span id="progress"></span><span class="keys">A accept · R reject · N note (Enter to finish) · J/K move</span><button id="next">Next unjudged</button><button id="export">Export verdicts</button></header>
<main>
<p class="intro"><strong>__COUNT__ cards: __PROMPTS__ prompts, each answered by __MODELS__ different models, shuffled.</strong> So every prompt shows up __MODELS__ times, and each card is one model's attempt; you just don't see which model. Judge each card on its own.</p>
<p class="intro">The prompts come in three tiers: simple (an empty scene or a few shapes), medium (a farm) and large (the island). Each card shows the scene before and after, side by side through the same camera: first the whole scene, then a close-up on what changed (or the other side, when the change covers the whole scene). Accept a result if you would apply it as a reasonable answer to the prompt; reject it otherwise. A note is optional. Click a picture to see it full size. Verdicts are kept in this browser until you export them.</p>
<div id="list"></div>
<h2>Export</h2><p class="intro">Export downloads verdicts.json; the same text is below to copy.</p><textarea id="out" readonly></textarea>
</main>
<div id="zoom"><img alt=""></div>
<script>
const CARDS = __DATA__;
const KEY = "vxr-t1-round__ROUND__-verdicts";
let v = {};
try { v = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch (e) { v = {}; }
const save = () => { try { localStorage.setItem(KEY, JSON.stringify(v)); } catch (e) {} render(); };
const esc = s => s.replace(/[&<>"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[c]));
const pair = (c, kind, label) => `<div class="row-label">${label}</div><div class="pair">
  <figure><figcaption>Before</figcaption><img src="img/${c.id}-before-${kind}.png" alt="${c.id} before, ${label}" loading="lazy"></figure>
  <figure><figcaption>After</figcaption><img src="img/${c.id}-after-${kind}.png" alt="${c.id} after, ${label}" loading="lazy"></figure></div>`;
const list = document.getElementById("list");
list.innerHTML = CARDS.map(c => `
<section class="card" id="${c.id}">
  <div class="top"><span class="id">${c.id}</span><span class="tier">${c.tier}</span><span class="prompt">${esc(c.prompt)}</span></div>
  <div class="meta">On ${c.scene} · review card: ${esc(c.counts)}</div>
  ${pair(c, "full", "The whole scene")}
  ${pair(c, "second", c.second)}
  <div class="verdict"><button class="accept" data-id="${c.id}" data-v="accept">Accept</button><button class="reject" data-id="${c.id}" data-v="reject">Reject</button><input data-note="${c.id}" placeholder="Note (optional)"></div>
</section>`).join("");
let cur = 0;
function render() {
  for (const b of document.querySelectorAll(".verdict button")) b.classList.toggle("on", (v[b.dataset.id] || {}).verdict === b.dataset.v);
  for (const i of document.querySelectorAll("input[data-note]")) if (document.activeElement !== i) i.value = (v[i.dataset.note] || {}).note || "";
  const n = CARDS.filter(c => (v[c.id] || {}).verdict).length;
  document.getElementById("progress").textContent = `${n} of ${CARDS.length} judged`;
  document.getElementById("out").value = JSON.stringify({ test: "vxr-ai-test-1-round-__ROUND__", verdicts: v }, null, 1);
}
function nextUnjudged(from) {
  for (let k = 1; k <= CARDS.length; k++) { const i = (from + k) % CARDS.length; if (!(v[CARDS[i].id] || {}).verdict) return i; }
  return Math.min(from + 1, CARDS.length - 1);
}
function focus(i) {
  cur = Math.max(0, Math.min(CARDS.length - 1, i));
  document.querySelectorAll(".card").forEach((e, k) => e.classList.toggle("focus", k === cur));
  document.getElementById(CARDS[cur].id).scrollIntoView({ block: "start" });
}
list.addEventListener("click", e => {
  const b = e.target.closest("button[data-v]");
  if (b) { v[b.dataset.id] = { ...(v[b.dataset.id] || {}), verdict: b.dataset.v }; save(); return; }
  if (e.target.tagName === "IMG") { const z = document.getElementById("zoom"); z.querySelector("img").src = e.target.src; z.style.display = "flex"; }
});
list.addEventListener("input", e => { const id = e.target.dataset.note; if (id) { v[id] = { ...(v[id] || {}), note: e.target.value }; save(); } });
document.getElementById("zoom").onclick = e => { e.currentTarget.style.display = "none"; };
document.getElementById("next").onclick = () => { const i = CARDS.findIndex(c => !(v[c.id] || {}).verdict); if (i >= 0) focus(i); };
document.getElementById("export").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([document.getElementById("out").value], { type: "application/json" }));
  a.download = "verdicts-round__ROUND__.json"; a.click();
};
document.addEventListener("keydown", e => {
  if (e.target.tagName === "INPUT") {
    if (e.key === "Enter" || e.key === "Escape") { e.preventDefault(); e.target.blur(); focus(cur); }
    return;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === "n") { e.preventDefault(); const i = document.querySelector(`input[data-note="${CARDS[cur].id}"]`); i.focus(); return; }
  const id = CARDS[cur].id;
  if (e.key === "a" || e.key === "r") { v[id] = { ...(v[id] || {}), verdict: e.key === "a" ? "accept" : "reject" }; save(); focus(nextUnjudged(cur)); }
  else if (e.key === "j") focus(cur + 1);
  else if (e.key === "k") focus(cur - 1);
  else if (e.key === "Escape") document.getElementById("zoom").style.display = "none";
});
render();
{ const i = CARDS.findIndex(c => !(v[c.id] || {}).verdict); focus(i >= 0 ? i : 0); }
list.addEventListener("focusin", e => { const id = e.target.dataset && e.target.dataset.note; if (id) { const k = CARDS.findIndex(c => c.id === id); if (k >= 0 && k !== cur) { cur = k; document.querySelectorAll(".card").forEach((el, j) => el.classList.toggle("focus", j === cur)); } } });
</script></body></html>
"""
html = (page_html.replace("__ROUND__", ROUND).replace("__DATA__", json.dumps(cards)).replace("__COUNT__", str(len(cards)))
        .replace("__PROMPTS__", str(len({k["prompt"] for k in key.values()}))).replace("__MODELS__", str(models)))
with open(os.path.join(page, "index.html"), "w") as f:
    f.write(html)
print(f"{len(cards)} results; page {os.path.join(page, 'index.html')}; key {keyfile}")
