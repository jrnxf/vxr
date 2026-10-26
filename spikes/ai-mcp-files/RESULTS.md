# Test 5: `vxr mcp` files mode from Claude Code

Phase AI risk test 5 (`docs/plan.md`). Run 2026-10-26 on the M4 Pro, from the owner's Claude Code login (no API key), model `claude-opus-5-5`.

**Result: pass.** In two runs of each session Claude Code built a scene from an empty file, edited a generated fixture with materials, layers and cameras, and edited its own build again, all through the MCP tools with no failed call. Every chunk the edits did not need stayed byte-identical and in place (282 of 290 in the fixture, 19 of 21 in the re-edit). Every result opened in the real editor (`vxr --shot`), and saving it unedited there gave the same bytes. One codec issue showed up along the way: a new scene saved around the origin is split into four objects (below).

## What was built

`vxr-mcp --root <dir> [--log <file>]`: a throwaway stdio MCP server (newline-delimited JSON-RPC) on `vxr_core::Scene`, so it opens and saves through the editor's own lossless path (`World::save_into`, `Document::set_palette`, `set_material`). One tool per operation, 24 in all (9.7 KB of tool JSON):

- Files: `list_files`, `new_scene(path, size)`, `open(path)`, `save(path?)`. Paths stay under `--root` (relative, no `..`, `.vox` only); a save writes a temporary file and renames it, and the first overwrite of a file in a session keeps `<name>.vox.bak`.
- Queries: `scene_summary` (mode, editable bounds, used slots with colour and material, selection, the object tree, layers, cameras; names capped at 64 characters, without control characters, marked as file content) and `slice(axis, index)` as text.
- Edits, one undo step each, with a short result (cells changed, voxel count, a warning when cells fell outside or nothing changed): `place`, `erase`, `box` (add, erase or paint, optional hollow), `line`, `sphere` (or a disc), `paint`, `select`, `fill`, `palette_set`, `material_set`.
- World: `edit_object(node)`, `show_world`, `object_set` (name, hidden, layer), `layer_add`, `layer_set` (name, hidden).
- `camera_set(id, values)`, `undo`, `done(summary)`.

Coordinates are vxr's y-up world cells. A file with several objects opens in world mode, where voxel tools refuse with "call edit_object(node) first", as in the editor.

- `fixture <out.vox>`: the file the edit session starts from. It is `vxr_vox::fixtures`' `22-world` (a slab, a group of three linked copies of one tree model with a nested group, a gem on a hidden layer, a hidden object, three layers) plus what MagicaVoxel writes next to it: 255 `MATL` chunks (the slab's grey metal, the gem glass, orange emissive), three `rOBJ`, three `rCAM` and a `NOTE`. 290 chunks, 22,464 bytes.
- `chunkdiff <before> <after> --expect <key>...`: keys every chunk (`MATL 8`, `rCAM 0`, `LAYR 1`, `nTRN 14` by node id, `XYZI #0` by model order, `RGBA`, ...), reports each as identical, changed, removed or added, checks that identical chunks keep their order, opens the result with `Scene::from_vox`, and fails on any change not expected.
- `run <out-dir> [build|edit|reedit|shots]`: runs each session with `claude -p`, with only the vxr server and its tools (`--mcp-config <file> --strict-mcp-config --tools "" --allowedTools mcp__vxr --setting-sources project --no-session-persistence --output-format stream-json --verbose`), then `summary.py` (turns, time, tokens, tool calls from the stream and the server's call log), `chunkdiff`, and `vxr --shot` on each result.

## The sessions

- **build**: "Create a new file house.vox with a scene of 32 x 24 x 32 voxels and build a small cottage on a patch of grass: a grass base, wooden walls with a door opening and two windows, a pitched red roof, a chimney, glass in the windows, an emissive lamp beside the door, and a tree next to the house. Choose and set your own colours. Check your work with slice where it helps. Save, then call done."
- **edit** on the fixture: paint a 2 x 2 red square in the middle of the slab; remove the top voxel of the tree model the three linked copies share; make slot 8 plastic with roughness 0.4; rename layer trees to forest and show layer secret; put gem on layer ground and unhide lost gem; move camera 0 out to radius 60; "change nothing else"; save.
- **reedit** on the file the build session wrote: make the roof dark blue (its slot's colour) and the lamp brighter (its material); save.

## Numbers

| Session | Run | Tool calls | Turns | Wall time | Errors | Tokens out | Cost as reported |
|---|---|---|---|---|---|---|---|
| build | 1 | 31 (15 `box`, 8 `palette_set`, 2 `material_set`, 1 `slice`, ...) | 32 | 56.2 s | 0 | 4,734 | $0.36 |
| build | 2 | 46 (28 `box`, 8 `palette_set`, 2 `slice`, ...) | 47 | 63.8 s | 0 | 6,163 | $0.26 |
| edit | 1 | 18 (3 `slice`, 2 `edit_object`, 2 `scene_summary`, ...) | 19 | 20.0 s | 0 | 1,875 | $0.32 |
| edit | 2 | 16 | 17 | 17.9 s | 0 | 1,675 | $0.14 |
| reedit | 1 | 7 | 8 | 26.7 s | 0 | 894 | $0.25 |
| reedit | 2 | 7 | 8 | 14.3 s | 0 | 826 | $0.08 |

The cost is what Claude Code reports; on a subscription it is counted against the plan, not billed. Most input is cached reads of the system prompt and the tool list. The server itself spent at most 21 ms on a whole session's calls; the time is the model's.

Chunks (`chunkdiff`):

| Session | Run | Identical, in order | Changed (all expected) | Unexpected |
|---|---|---|---|---|
| edit | 1 and 2 | 282 of 290 | `XYZI #0` (slab), `XYZI #1` (tree model, so all three copies), `MATL 8`, `LAYR 1`, `LAYR 2`, `nTRN 14` (gem's layer), `nTRN 16` (lost gem's hidden flag), `rCAM 0` | 0 |
| reedit | 1 and 2 | 19 of 21 | `RGBA`, the lamp's `MATL` | 0 |

Both edit runs wrote the same 22,460 bytes. The other 254 `MATL`, the other cameras, `rOBJ`, `NOTE`, `LAYR 0`, every other node, the gem model and every `SIZE` kept their bytes. The plastic material keeps the slot's old `_metal` key and adds `_plastic` and `_spec`, as `Material::apply_to` writes it.

Opening in vxr (`vxr --shot <dir> <file>`, all six results): each opened (2,476 and 1,880 voxels for the two houses, 102 for the world), rendered at 1x and 2x, and its unedited save was byte-identical. The shot run's next step, an add drag, then fails on all of them, because they open in world mode, where a click picks an object; the check assumes an editable model, so only its first steps apply here. The screenshots show the cottage as asked (walls with a door, two glass windows, a stepped roof, a chimney, a lit lamp, a tree) and, in the world, the red square, the forest layer, both gems shown and the trees one voxel shorter.

## Against the bar

- The agent finishes the build: yes, 2 of 2 (and 4 of 4 edit sessions), with `done` called and the file saved each time.
- Every untouched chunk stays byte-identical: yes, with the order kept.
- The result opens in vxr: yes, every file.

## What it changes for the build

- **Fix the new-scene save before `vxr mcp`.** A new scene is written through `Builder::add_world`, and `world_blocks` cuts models on a 256 grid fixed at 0. A new scene is centred on the origin across the ground, so it becomes four objects even at 32^3: the houses reopen as "object 1" to "object 4" in world mode. The editor's own Save does the same today. An agent that reopens its build has to find the right block with `edit_object`; the reedit sessions got away with palette and material edits. Cutting blocks from the voxels' low corner would keep anything up to 256^3 as one model.
- **Cameras need a codec edit.** `vxr-core` neither reads nor edits `rCAM`. The spike kept edited camera dicts beside the scene, rewrote just those chunks at save and kept its own undo for them. The build should add `Document::set_camera` next to `set_material`, and put cameras in `Scene` so undo and the summary cover them.
- **The tool shape held.** One tool per operation, model and world mode made explicit (a voxel edit in world mode refuses with what to call instead), node ids from `scene_summary`, and a short result per edit were enough: no call failed, and the agent found the linked tree model, the layers and the hidden object from the summary alone. Text slices were the only view; on these tasks the agent used them to check its work and needed nothing more, which says nothing yet about `render_view` (test 2).
- **Keep the paths and names rules.** Root-relative paths only, temporary file and rename, a `.bak` once per session, names capped and marked as file content cost little and never got in the way.
- **Claude Code needs nothing special**: an MCP config naming the binary and its `--root` is the whole setup (`claude mcp add` writes the same entry; not tried here). In `-p` runs `--allowedTools mcp__vxr` allows every vxr tool; an interactive user approves each call, as `docs/research/ai.md` expects.
- **For a CI-style check of `vxr mcp`**, `--shot` would want an open-only mode (or a world-mode path), since its tool drive assumes a model.

Not covered: other agents and models (only Claude Code on Opus 5.5), live mode, undo called by the agent (a hand-driven smoke test undid a world edit and a voxel edit back to the file's bytes), and the owner's judgement of the build's looks.

## Rerun

```
crates/vxr-app/fetch-fonts                # once, for the shots' UI fonts
spikes/ai-mcp-files/run <out-dir>         # build, edit, reedit, shots; or name the steps
```

It needs a logged-in `claude` on the PATH and writes everything (streams, call logs, `.vox` files, screenshots) under `<out-dir>`; none of it is in git.
