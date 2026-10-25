# MagicaVoxel gap list

Every MagicaVoxel 0.99.6 / 0.99.7 feature we could find in public sources, with vxr's status and a priority. Desk research only: MagicaVoxel was not launched. Keep the table current as each feature lands.

Status as of 2026-10-25 (Phase 3, after the browser build was checked end to end).

## How to read it

- **Status**: done (works natively and in the browser build, with tests and `--shot` steps), partial (some of it, or native only), missing.
- **Priority**: P1 is needed before a MagicaVoxel user could switch, P2 is expected soon after, P3 is nice to have or out of scope for now.
- **Why**: which reason people love MagicaVoxel it serves (see `docs/market-research.md`, "Why it is loved"): **minutes** (results in minutes), **sculpt** (the hand-sculpting feel), **palette** (palette-first work), **render** (a beautiful built-in renderer), **lossless** (lossless `.vox`). A dash means it serves none of them directly.
- **Build order**: the numbers in brackets, such as [1], are the Part 3 order in the current run (1 lossless save, 2 palette editing, 3 preview, render and materials, 4 line and center brushes, 5 selection and transform, 6 model tools, 7 camera and view, 8 world editor, 9 export).

MagicaVoxel versions: 0.99.7.2 (July 2025) is the current build and is Windows-only. The Mac build stopped at 0.99.6.2 (September 2020) [S1]. A user switching from the Mac build expects the 0.99.6 set; a Windows user expects 0.99.7's palette groups, layer sorting and animation timeline.

## Sources

- [S1] MagicaVoxel site and full update log, 0.96 to 0.99.7.2: [ephtracy.github.io mv_main](https://ephtracy.github.io/index.html?page=mv_main), source [mv_main.html](https://github.com/ephtracy/ephtracy.github.io/blob/master/mv_main.html). Release downloads: [GitHub releases](https://github.com/ephtracy/ephtracy.github.io/releases).
- [S2] The `.vox` format and its scene, layer, material, camera and note chunks: [MagicaVoxel-file-format-vox.txt](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox.txt), [MagicaVoxel-file-format-vox-extension.txt](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt). Our own notes: `docs/research/vox-format.md`.
- [S3] Community voxel shaders (679 stars, last push February 2025): [lachlanmcdonald/magicavoxel-shaders](https://github.com/lachlanmcdonald/magicavoxel-shaders).
- [S4] vengi's feature list, for comparison: [vengi docs/voxedit/Features.md](https://github.com/vengi-voxel/vengi/blob/master/docs/voxedit/Features.md).
- [S5] `docs/market-research.md` for the loved list and complaints.

Feature names below follow the update log [S1] unless another source is given.

## Editing modes and brushes

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Attach, erase, paint actions | Attach/Erase/Paint on every brush [S1 0.98.1] | done (add, erase, paint; Shift erases, Alt picks) | P1 | sculpt |
| Pick colour from a voxel | Alt-click; Alt+drag picks from the screen [S1 0.99] | done (pick tool and Alt) | P1 | palette |
| Voxel brush (V) with size | Cube, sphere, square, disc, surface; keys 1-9 set size; Ctrl+Alt+drag resizes [S1 0.97.1, 0.99.5.1] | partial (single voxel and drag-on-plane; no size, no sphere or surface shape) | P1 | sculpt |
| Face brush (F) | Extrude or peel a connected face by dragging, several modes [S1 0.97] | done (click extrudes or peels the whole face, paint fills it) | P1 | sculpt, minutes |
| Box brush (B) | Drag a rectangle, resizable with a gizmo, Enter commits [S1 0.99.5.1] | partial (drag a rectangle; no gizmo or height drag) | P1 | minutes |
| Line brush (L) | Line, square and circle in one geometry mode since 0.99.6.2 [S1 0.99.6.2] | done (L: drag a line on the drag plane, shown live) | P1 | sculpt |
| Center brush (C) | Square or circle from a center point, even diameters [S1 0.97, 0.97.1] | done (C: circle on the drag plane from its center, C again for a sphere; no square or even diameters) | P1 | sculpt |
| Pattern brush (P) | Stamp a pattern made from a selection, rotate and offset it [S1 0.98.1] | missing | P2 | minutes |
| Mirror X, Y, Z | Per-axis mirror on every brush [S1 0.97] | done (M toggles; X, Y, Z chips in the axis flyout; planes in `docs/decisions.md`) | P1 | sculpt |
| Axis lock | Brush option "axis" [S1 0.97, 0.99.5.1] | done (K cycles X, Y, Z, off: drags stay on the plane across that axis) | P1 | sculpt |
| Diagonal symmetry | Command `dia [axis]` [S1 0.98.1] | missing | P3 | - |
| Region options | Brushes act on selected or unselected voxels only [S1 0.98.1, 0.99.5.1] | missing (selection is in [5]; brushes ignore it so far) | P2 | sculpt |
| Replace colour and remove colour brushes | Act on every voxel of one colour [S1 0.98.1] | missing (paint face covers part) | P2 | palette |
| Sculpt brushes | Applied to selected or unselected regions [S1 0.99.5.1] | missing | P3 | sculpt |
| SDF brush | Box brush plus a voxel shader [S1 0.99.5.1] | missing (see Shaders) | P3 | - |
| Scale and wrap transform brushes | Live point-sampled scale; live wrap and crop with mirroring [S1 0.99.6.2] | missing | P3 | - |
| Rotate transform brush | Axis, screen and sphere rotate with 5 and 15 degree snaps [S1 0.99.6.2] | missing (90 degree rotate is [5]) | P3 | - |
| Undo and redo | Up to 100 compressed steps [S1 0.99.5] | done (unlimited, one step per stroke) | P1 | minutes |

## Selection and transform

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Box select | Shift adds, Shift+Alt subtracts [S1 0.98.1] | done [5] (a screen box through every depth; Shift adds, Alt subtracts) | P1 | sculpt |
| Rect (screen) select | Select voxels in a screen rectangle [S1 0.98.1] | done [5] (the select tool's box) | P2 | - |
| Region select: same volume, face, colour | [S1 0.98.1]; select voxels by colour from the palette menu [S1 0.99.5] | partial [5] (colour: click a voxel, or Cmd-K Select color; face and volume missing) | P1 | palette |
| Deselect, invert, select all | Edit->Select [S1 0.98.1] | partial [5] (Cmd-A, Escape, the flyout's x; invert missing) | P1 | - |
| Copy, paste, delete, fill selection | [S1 0.98.1] | done [5] (Cmd-C, Cmd-X, Cmd-V at the pointer, Backspace, fill with the current colour) | P1 | minutes |
| Move selection | Move brush on selected voxels [S1 0.98.1]; Ctrl / Ctrl+Shift / Ctrl+Shift+Alt for 1, 2 or normal-axis moves [S1 0.99.7.2] | done [5] (drag on the pressed face's plane, live; arrows and Shift-Up/Down nudge) | P1 | sculpt |
| Flip, rotate 90, loop (shift) | Transform tools on the selection around its local center [S1 0.98.1] | done [5] (selection, or the whole model with nothing selected) | P1 | minutes |
| Scale and repeat | Console commands `scale`, `repeat` [S1 0.98] | missing | P3 | - |
| Shear | Command `shear z 0.2 0.2` [S1 0.99.6.2] | missing | P3 | - |
| Mask and texture with a pattern model | Modify->Mask/Texture [S1 0.99.6.2] | missing | P3 | - |
| Export only the selection | [S1 0.98.1] | missing | P2 | - |

## Model tools

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Model size up to 256^3 | Canvas size per object [S1 0.99.5] | done, and past it: one sparse map up to 2048 voxels per axis, split into 256^3 models on save | P1 | - |
| Resize model bounds | Crop tool resizes the canvas live [S1 0.99.5.1] | done [6] (Resize… in the selection flyout opens a Bounds panel with X, Y and Z fields; keeps the bottom and the centre; one undo step; saved as the object's model size [8]) | P2 | - |
| Crop to content and fit | Fit sizes several objects [S1 0.99.5.1] | partial [6] (Crop in the selection flyout and Cmd-K fits the bounds to the voxels; saved as the object's model size [8]; fitting several objects at once waits for the world UI) | P2 | - |
| Hollow and fill | Edit tools (also in vengi [S4]) | done [6] (selection flyout and Cmd-K: Hollow keeps the voxels seen from outside, Fill inside fills enclosed space; the selection or the whole model) | P2 | sculpt |
| Boolean operations | Groups and objects [S1 0.99.6.2] | missing | P3 | - |
| Statistics | `log`: model count, scene size, voxels per colour [S1 0.99.6.2] | partial (voxel count in the header; per-colour count kept in the brick map, not shown) | P3 | - |
| Auto-save and crash recovery | Every 25 steps to `cache/`, recover from `backup.vox` [S1 0.99, FAQ] | missing | P2 | - |

## Palette

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| 256-slot palette, recolour by editing a slot | Core workflow [S1, S5] | done (all 255 slots in a scrolling grid; Edit recolours every voxel of the slot live, one undo step per edit) [2] | P1 | palette |
| Colour picker: HSV, RGB, hex | [S1 0.99.4.2] | done (saturation/value square, hue bar, hex, RGB and HSV fields) [2] | P1 | palette |
| Swap colours | Ctrl+drag swaps; Alt+drag swapped without changing the model in 0.97.2 [S1 0.99.2, 0.97.2] | done (Swap with… button, Cmd-K, or Cmd/Ctrl-drag; voxels move with the colour so the scene keeps its look) [2] | P1 | palette |
| Duplicate (copy) a colour | Ctrl+Shift+drag [S1 0.99.2] | done (Copy to… button, Cmd-K, or Cmd/Ctrl+Shift-drag) [2] | P1 | palette |
| Multi-select colours, group HSV/RGB edits | Drag selects; group edits since 0.99.7.1 [S1 0.99.5, 0.99.7.1] | missing | P2 | palette |
| Copy and paste colours between palettes | [S1 0.99.7.1] | missing | P2 | palette |
| Sort palette | `pal sort [+-hsvrgb]` [S1 0.99.2] | missing | P3 | palette |
| Mask unused colours | `pal mask` [S1 0.97.1]; vxr shows dots on used colours | partial (used dots) | P3 | palette |
| Select voxels by colour and colours by voxels | Palette menu [S1 0.99.5] | partial [5] (voxels by colour; colours by voxels missing) | P1 | palette |
| Palette notes | A note per palette row, saved in NOTE [S1 0.99.5.1, S2] | missing (kept by lossless save [1]) | P3 | lossless |
| Import and export palettes | IO panel, text and image [S1 0.99.5.1] | missing | P2 | palette |

## Materials

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Per-slot material | Matter->Sel per palette slot [S1 0.96.2] | done [3]: the colour editor's Material tab and Cmd-K "Edit material", one undo step per change; swap and copy carry it | P1 | render |
| Diffuse, metal, glass, emissive | Plus plastic, roughness, IOR, attenuation, emission power [S1 0.96-0.98.2] | partial [3]: the four types with roughness, metalness, transparency, refractive index (reflection only: glass is thin, no bending), emission and power, path traced in the corner and export; no plastic, specular or attenuation | P1 | render |
| Blend, SSS, media (cloud) | Unified material system [S1 0.99.6.2] | missing | P3 | render |
| Saved to `.vox` MATL | [S1 0.98, S2] | done [3]: edits change only that slot's MATL keys of its type; unknown keys and unedited slots keep their bytes; new scenes write MATL for non-diffuse slots | P1 | lossless, render |
| Copy and paste materials | Ctrl+C/V [S1 0.99] | missing | P3 | - |

## Render

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Interactive path tracer | Progressive GPU path tracing [S1] | partial [3]: the live corner runs in the app and the browser (640x360, A-trous denoise, 3 ms per frame while editing, 12 ms idle); no full-view render mode or click-to-swap yet | P1 | render |
| Sun direction and colour, sky | Atmospheric sky with Rayleigh and Mie, sun disk [S1 0.99.1, 0.99.2] | partial [3]: Render panel sun direction, height, colour and strength, sky light and a sky backdrop toggle over a gradient sky; no physical sky or sun disk | P1 | render |
| Exposure, gamma, tone mapping, vignette | Post->E, Camera->G, ACES [S1 0.97.3, 0.98.1] | partial [3]: exposure in stops (no restart) and ACES tone mapping; no gamma or vignette controls | P1 | render |
| Depth of field, bokeh | Click to set focus; bladed bokeh [S1 0.98.1, 0.99.1] | partial [3]: thin-lens depth of field with blur and focus sliders (round bokeh); no click to focus or bladed bokeh | P2 | render |
| Bloom | [S1 0.98.2, 0.99.1] | missing | P3 | render |
| Fog and volumetrics | [S1 0.97.5, 0.99.4.1] | missing | P3 | render |
| Image-based lighting (`.hdr`) | Sky->IBL [S1 0.99.4.1] | missing | P3 | render |
| Denoiser | Intel OIDN plugin [S1 0.99.4.2] | done [3]: A-trous in the live corner | P1 | render |
| Export PNG at a chosen size | Photo mode up to 12000x12000 [S1 0.99.3] | done [3]: 720p to 4K (as the GPU allows) at 64 to 4,096 spp, traced in the background while editing; download in the browser | P1 | render, minutes |
| Turntable and animation render | [S1 0.99.3, 0.99.7.0] | missing | P3 | - |
| Voxel shapes (Lego, marching cubes, clay) | [S1 0.96, 0.99.2] | missing | P3 | - |
| Editor display modes | AO and constant shading, background fading, grid/edge colours [S1 0.99.5.1] | partial (one lit shading with the mockup background) | P3 | - |

## Camera and view

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Orbit, pan, zoom | Right-drag, middle-drag or Space+drag, wheel or Z+drag [S1 0.99, FAQ] | done (right-drag orbit, Shift+right or middle pan, wheel zoom) | P1 | minutes |
| Frame the model | Hotkey 4 recenters [S1 0.99] | done (`.`) | P1 | minutes |
| Set rotation center on a voxel | X+click [S1 0.99.3] | missing | P2 | sculpt |
| Perspective, orthographic, isometric | Camera modes [S1 0.97.1] | done [7] (View flyout, O, 0) | P1 | - |
| View cube and axis snap views | 26 standard views, hotkey 5 for face views [S1 0.99] | partial [7] (six side views and isometric on 0 to 6 and the View flyout; no 26-view cube) | P2 | - |
| Grid and ground | Frame grids with spacing, infinite ground [S1 0.98.1, 0.97.5] | done [7] (a voxel grid on the ground inside the bounds and a ground that fades into the background; View flyout, G, Cmd-K; no grid spacing setting) | P2 | minutes |
| Bounding box display | Select on any of the six bounding box faces [S1 0.98.1] | partial [7] (the bounds frame is drawn, faint where voxels hide it; View flyout, Cmd-K; no face selection on it) | P2 | - |
| Save and load views | 7/8, F5/F7, camera slots [S1 0.99, 0.99.6.2] | missing (cameras kept by lossless save [1]) | P3 | - |
| Camera control panel | FOV, position, pitch/yaw/roll [S1 0.99.6.2, 0.99.6.4] | missing | P3 | - |
| Free (fly) camera | Pers/Free modes [S1 0.97.5, 0.99.6.4] | missing | P3 | - |

## World editor

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Multiple objects per scene | Tab switches model and world editors [S1 0.99] | done: files open as separate objects (world mode with several, model mode with one); Tab and Cmd-K switch between editing one model (edits reach every linked copy) and the world, undo follows edits across modes, and saving writes models, nodes and layers losslessly; in world mode a click picks an object, a drag or the arrows move it, Cmd-D duplicates it, Cmd-Shift-D makes a linked copy and Backspace deletes it; a scene tree beside the rail lists every object and group from the second object; groups are picked from the tree, moved, copied, made (Cmd-G) and taken apart (Cmd-Shift-G); layers are listed under the tree, shown, hidden, named, added and assigned [8] | P1 | lossless |
| Move, rotate, flip objects | Gizmo, explicit position, 90 degree rotate gizmo [S1 0.99, 0.99.7.0, 0.99.7.2] | partial [8] (move objects and groups: drag on the face plane, arrows and Shift-Up/Down, one undo step each; rotate and flip objects missing) | P2 | - |
| Groups | Enter and leave with Tab or double-click [S1 0.99, 0.99.5.1] | done [8] (a group row picks the group; a drag on one of its objects, the arrows, Cmd-D, Cmd-Shift-D, Backspace, H and rename act on the whole group; Cmd-G wraps the picked node in a new group, Cmd-Shift-G ungroups in place; Tab edits its first object; one undo step each, saved losslessly; grouping several objects at once waits for multi-select) | P2 | - |
| References (instances) and duplicates | Shift+drag; Ref-R converts [S1 0.99, 0.99.5.1] | done [8] (Cmd-D duplicate with its own model, Cmd-Shift-D linked copy sharing it, placed beside the original; Cmd-K; saved losslessly) | P2 | - |
| Layers: show, hide, name, colour, sort | Layer panel [S1 0.99.6.4, 0.99.7.1] | partial [8] (a LAYERS list under the scene tree in world mode: every layer with its object count; the eye shows and hides it, a double-click or Cmd-K renames, + or Cmd-K adds one, a click puts the picked object or group on it, Cmd-K hides the picked object's layer; one undo step each, saved as `LAYR` and nTRN layer ids. Layer colour is kept losslessly but not editable, and sort is a Windows-only 0.99.7 feature: both wait for a user asking) | P2 | - |
| Scene outline panel | Rename, hide, reorder, multi-select [S1 0.99.6.4] | partial [8] (beside the rail from the second object: objects and groups indented, model sizes, linked copies marked; a row click picks an object or group or edits an object, the eye and H hide and show, a double-click or Cmd-K renames, one undo step each; reorder and multi-select missing) | P2 | - |
| Pattern library | Drag assets in, match palettes [S1 0.99.6.4] | missing | P3 | minutes |
| Copy and paste transforms | [S1 0.99.7.0] | missing | P3 | - |

## Files and export

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Open `.vox` (all versions, scene graph) | [S2] | done (every instance flattened, hidden ones included) | P1 | lossless |
| Save `.vox` | [S2] | done [1]: lossless. An unedited save is byte-identical; edits re-encode only the edited models (plus the root group for new models), and every other chunk (materials, layers, cameras, names, notes, unknown chunks) keeps its bytes. An edited linked copy is split into its own model | P1 | lossless |
| Choose what is saved | IO panel [S1 0.99.5.1] | missing | P3 | - |
| Recover from a corrupted file | IO->File->Recover [S1 0.99.7.2] | missing | P3 | lossless |
| OBJ with palette texture | Color-based simplification [S1 0.96.1] | missing [9] | P2 | - |
| PLY (marching cubes) | [S1 0.97.1] | missing | P3 | - |
| Qubicle `.qb` | [S1 0.97.2] | missing | P3 | - |
| Slices, cubes, point clouds | [S1 0.99.3]; `o slice` exports a volume as one image [S1 0.98.1] | missing [9] (PNG slices) | P2 | - |
| Isometric sprites | Export->iso [S1 0.96.1] | missing | P3 | - |
| Baked AO mesh | Export->bake [S1 0.97.2] | missing | P3 | - |
| Import images | JPEG import, drag in sprites [S1 0.99.5, 0.99.7.2] | missing | P3 | - |
| Web embed | none | partial: the `<voxel-scene>` embed is proven in `spikes/embed`; the app's Embed command shows "not built yet" | P2 | - |
| Runs in a browser, on Apple Silicon and Linux | Windows and Intel Mac only [S5] | done (native wgpu app plus the WebGPU browser build) | P1 | - |

## Shaders (out of scope tonight)

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Voxel shaders (`xs`) | GLSL scripts that generate or change a volume, 16 arguments, iterations, frames [S1 0.97.1, 0.99.6.2] | missing | P2 | minutes |
| Voxel shader brush | Run a shader interactively as a brush [S1 0.99.6.2] | missing | P2 | sculpt |
| Community shader library | About 100 shaders in [S3] | missing | P3 | - |

Design note: the community shaders are GLSL fragments that read `voxel(pos)` and `palette()` and return a palette index per voxel [S3]. vxr could run them as WGSL compute passes over the brick map's dense bricks (one dispatch per touched brick, results written back as one undo step). A small GLSL-to-WGSL shim (naga reads GLSL) would let the existing library run unchanged. Arguments map to sliders in a Shader panel, and a "shader brush" runs one inside the box brush's bounds. The same edit API the AI work needs (Part 4) is a better first target than a GPU shader runtime, so this waits for that design.

## Animation (out of scope tonight)

| Feature | MagicaVoxel | vxr | Priority | Why |
|---|---|---|---|---|
| Frame-based animation | Up to 24 frames in 0.98, a timeline with 0-360 range since 0.99.7 [S1 0.98, 0.99.7.0, 0.99.7.1] | missing (frames of an opened file are kept by lossless save [1]) | P3 | - |
| Object keyframes | `nTRN` frames with `_f` [S2] | missing | P3 | - |
| Onion skin, playback, import sprites | [S1 0.99.7.0, 0.99.7.2] | missing | P3 | - |

Design note: `.vox` animation is a list of `nSHP` models with `_f` frame indices and `nTRN` keyframes [S2]. The lossless save keeps them untouched. A first editable version would add a frame strip under the viewport where each frame is a separate brick map sharing the palette (cheap: bricks are shared copy-on-write until edited), with Cmd-K entries for add, duplicate and delete frame, and play. Object keyframes belong with the world editor [8].

## What vxr has that MagicaVoxel does not

- Native Apple Silicon, Linux and the browser from one build [S5].
- One sparse map well past 256^3 per model (2048 per axis today).
- Unlimited undo; a command palette (Cmd-K) listing every command; a hint bar that explains the current tool.
- Open source, and a web embed in progress.

## Takeaways

1. Lossless save [1] was the one blocking gap for anyone with existing files. It is done: saving an opened MagicaVoxel scene keeps its materials, layers, cameras and names.
2. The palette [2] and the renderer [3] were the two loved features vxr lacked most visibly. Palette editing is done (picker, live recolour, swap and copy); multi-select, sorting and palette import/export remain. The live path-traced corner, the Render panel (sun, sky, exposure, depth of field, samples), PNG export and per-slot materials (diffuse, metal, glass, emissive, saved as MATL) are in; refraction, plastic and the newer blend and cloud types remain.
3. Brushes [4] and selection [5] are in (line, center circle and sphere, mirror on X, Y and Z, axis lock; box and colour select, move, copy, paste, delete, fill, rotate, flip and loop), which closes the sculpting gap apart from region options and face or volume select. Next, the world editor [8] matters most for MagicaVoxel scenes with many objects.
