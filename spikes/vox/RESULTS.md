# Test 4: `.vox` round-trip

**Summary**

- **Unedited re-save: pass.** All 568 MagicaVoxel-format files in the corpus came back byte for byte, plus our 21 fixtures. That covers versions 150 and 200, legacy `PACK`/`MATT` files, old render chunks and Teardown's `TDCZ`. The 2 files that are not MagicaVoxel files at all are rejected with an error.
- **Edited re-save: pass.** After setting one voxel, recolouring one palette slot and moving one node, only the chunks those edits had to change differ, in all 589 files. `ogt_vox` reads the same voxels and world transforms as we do on 585 of 589, and `dot_vox` reads the same voxels, palette and scene graph on 588 of 589. Every difference is a known gap in those readers, listed below.
- **MagicaVoxel check: pass on 0.99.6.2, with a stated gap.** 0.99.7.2 exists only for Windows (it crashes at start under Wine on macOS) and no Windows machine was available, so the check used 0.99.6.2, the last Mac build. MagicaVoxel composes transforms parent then child (as we do) and centres models at floor(size/2) (as we do), both shown pixel for pixel. 17 of 21 fixtures re-saved by MagicaVoxel read back the same; the other 4 differ only where 0.99.6.2 lacks a feature or clamps positions.
- **Fuzzing: pass.** One hour of `cargo fuzz` (331,274 runs) found no panic, no runaway allocation and no slow input. Fuzzing found one bug in its first second, an integer overflow while building an error message. It is fixed, and its input is kept as a regression.
- **Property test: pass.** 20,000 random scenes per property, 19,526 of them big enough to split into several 256^3 models: write, read, write is byte-identical and every voxel lands where it started.
- **Biggest finding:** real MagicaVoxel files share scene-graph nodes between parents (linked copies) in 7 of the 35 scene-graph files. A reader that visits each node once loses objects. vxr's scene model needs instancing from the start.

## What was built

`spikes/vox/`, a standalone Cargo project, about 1,700 lines for the codec plus tools and tests.

- **Raw layer** (`src/raw.rs`): the header version as read, every chunk's id, content and children bytes in file order, and any bytes after `MAIN`. Writing it back is byte-identical by construction.
- **Typed chunks** (`src/chunk.rs`): parse and encode for `SIZE`, `XYZI`, `TDCZ`, `RGBA`, `nTRN`, `nGRP`, `nSHP`, `MATL`, `rCAM`, `LAYR`, `rOBJ`, `META`, `NOTE`, `PACK`. DICTs are ordered byte pairs, so key order, duplicate keys and non-UTF-8 names survive. Each typed chunk keeps any bytes it did not parse, so `encode(parse(chunk))` equals the original for every chunk. `IMAP` (256 bytes), `MATT`, `rLIT`, `rAIR`, `rLEN`, `POST`, `rDIS` and unknown chunks stay opaque.
- **Semantic layer** (`src/scene.rs`): models, the palette, materials, layers, cameras, render objects, notes, `META`, and the scene graph flattened to world instances, with MagicaVoxel's `floor(size / 2)` pivot and the version 200 overlap order.
- **Edits** (`src/doc.rs`): set a voxel, recolour a palette slot, move a node. Each re-encodes only its own chunk.
- **Writer** (`src/build.rs`): builds new files in the chunk order MagicaVoxel 0.99.7 uses, and splits scenes bigger than 256^3 into placed models.
- **Tools** (`voxtool`): `roundtrip`, `chunks`, `dump`, `edit-check`, `rules`, `compare`, `fixtures`. Cross-checks: `harness/crosscheck-ogt` (a 54-line C++ harness around `ogt_vox.h`) and `examples/crosscheck_dotvox.rs` (the `dot_vox` 5.2.0 crate).
- **Reader limits for files from strangers:** every count and length is checked against the bytes left before anything is allocated; `TDCZ` models are capped at 256 per axis and 64 Mi expanded voxels per file; scene-graph walks stop at depth 256 or 1,048,576 node visits. `tests/hostile.rs` feeds it the crafted cases behind other readers' bugs (lying and negative sizes, billion-entry counts, zlib bombs, cyclic and exponentially shared graphs, bad `_r`/`_t` strings): all give `Err` or a bounded result.

## Corpus

Fetched by `spikes/corpus/fetch-corpus` into `spikes/corpus/fetched/` (git-ignored). Every URL is pinned to a commit SHA and checked against a sha256 in `spikes/corpus/manifest.tsv`: 569 `.vox` files (61 MB) plus the `ogt_vox.h` header. With the prototype island, the corpus is 570 files.

| Source | Files | What it covers |
|---|---|---|
| [mikelovesrobots/mmmm](https://github.com/mikelovesrobots/mmmm) | 487 | v150 single models, many without `RGBA` (default palette) |
| [ephtracy/voxel-model `vox/`](https://github.com/ephtracy/voxel-model/tree/master/vox) | 44 | Legacy v150: `PACK` + `MATT` animations, `monu3` (3.4 MB) |
| [vengi `data/tests`](https://github.com/vengi-voxel/vengi/tree/master/data/tests) | 19 | `teardown.vox` (`TDCZ`), `animation.0.99.7.1`, `test-transform`, `test_material`; plus `slab6_vox_test.vox`, a different format that shares the extension |
| [vox-rs `testdata`](https://github.com/zeozeozeo/vox-rs/tree/main/testdata) | 8 | Scene graphs with 44 and 149 nodes, `IMAP`, `META` |
| [dot_vox `src/resources`](https://github.com/dust-engine/dot_vox/tree/master/src/resources) | 6 | Old 0.99.0 render chunks; `not_a.vox`, a deliberately broken file |
| [bevy_vox_scene `assets`](https://github.com/oliver-dew/bevy_vox_scene/tree/main/assets) | 5 | v200, `META`, `_frm_t`, 32 layers, cloud media |
| `prototype/demo/scene.vox` (committed) | 1 | The prototype's v200 island |
| `spikes/vox/fixtures/` (ours, meant to be committed) | 21 | See "MagicaVoxel check" |

Only 8 of the 570 are version 200 (7 fetched plus the prototype island), and 21 more are our fixtures. MagicaVoxel's own sample scenes ship only inside its download, with no licence stated, so they are not in the corpus.

## Results

| Check | Pass line | Result | Verdict |
|---|---|---|---|
| Unedited re-save | Byte-identical on 100% of the corpus, incl. v200 and Teardown | 568 of 568 MagicaVoxel files, 21 of 21 fixtures; 2 non-MagicaVoxel files rejected | **Pass** |
| Edited re-save | Only the chunks the edit had to change differ; `dot_vox` and `ogt_vox` read the same voxels and world transforms | 589 of 589 touched only their chunks; `ogt_vox` agrees on 585, `dot_vox` on 588 (voxels, palette, graph), all gaps explained | **Pass** |
| MagicaVoxel check | About 20 fixtures look the same in MagicaVoxel 0.99.7.2 after our re-save; MagicaVoxel's re-save reads back the same | Run on 0.99.6.2 (no 0.99.7.2 on macOS). Composition and pivots match pixel for pixel; 17 of 21 re-saves identical through `compare`, 4 differ only on 0.99.6.2 limits | **Pass on 0.99.6.2; 0.99.7.2-only behaviour untested** |
| Fuzzing | 1 hour of `cargo-fuzz`, no panics, no unbounded allocation | 1 h, 331,274 runs, 0 crashes, 0 out-of-memory, 0 timeouts; 1 earlier bug fixed | **Pass** |
| Property test | Random scenes incl. over 256^3: write-read-write byte-identical | 20,000 cases per property, all pass | **Pass** |

### 1. Unedited re-save

`voxtool roundtrip ../corpus/fetched ../../prototype/demo/scene.vox fixtures` (full output in `logs/roundtrip.txt`):

| Source | Files | Byte-identical | Rejected |
|---|---|---|---|
| mmmm | 487 | 487 | 0 |
| voxel-model | 44 | 44 | 0 |
| vengi | 19 | 18 | 1 (`slab6_vox_test.vox`, not a MagicaVoxel file) |
| vox-rs | 8 | 8 | 0 |
| dot_vox | 6 | 5 | 1 (`not_a.vox`, magic `DOOD`) |
| bevy_vox_scene | 5 | 5 | 0 |
| prototype island | 1 | 1 | 0 |
| our fixtures | 21 | 21 | 0 |

By version: 561 of 561 at 150, 28 of 28 at 200 (8 corpus files plus our fixtures).

The typed layer is checked too: every one of the 11,656 chunks re-encodes to its original bytes, including 6,912 `MATL`, 765 `MATT`, 534 `nTRN`, 8 `TDCZ` and 24 old render chunks. No chunk had bytes past the fields we parse. Reading and re-saving all 64 MB takes 0.2 s on one core.

### 2. Edited re-save

`voxtool edit-check` applies three edits to every file: flip the colour of model 0's first voxel (or add one), set palette slot 1 to a new colour, and move the transform node above the first shape by (+3, -2, +1).

- 589 files edited: 589 voxel edits, 589 recolours (46 of them had no `RGBA` chunk, so one was added), 56 node moves (files without a scene graph have no node to move).
- In all 589, the only chunks that differ are the edited `XYZI`/`TDCZ`, `RGBA` and `nTRN`. Our semantic diff agrees: at most one voxel and one palette entry change, only instances under the moved node move, and materials, layers and cameras are untouched. Each edited file also re-saves byte-identical.

Third-party readers on the originals and on the edited files (`logs/crosscheck-*.txt`):

| Reader | Same as ours | Differences, all explained |
|---|---|---|
| `ogt_vox` (C++, pinned at opengametools `17b41f9`) | 585 of 589, before and after the edits | `teardown.vox`: upstream `ogt_vox` skips `TDCZ` (only vengi's patched copy reads it). 3 legacy `PACK` animations (`deer`, `horse`, `T-Rex` from voxel-model): with no scene graph and several models, `ogt_vox` places nothing; we show frame 0. |
| `dot_vox` 5.2.0 | Voxels, palette, scene graph and layers on 588 of 589 | `teardown.vox`: `dot_vox` drops `TDCZ`, so it sees no models. Separately, 47 files differ only in the unused 256th palette entry, where the two default palettes disagree on alpha; no voxel can use it. |
| `dot_vox`, world transforms | 53 of 56 with a scene graph (our composition rule applied to its graph) | `deer.vox` and `test_meta_chunk.vox` carry `_frm_t`, which we and `ogt_vox` use as the base transform and `dot_vox` ignores; plus fixture 21, which tests exactly that. |

**Rotation composition.** `dot_vox` computes no world transforms itself. Its `traverse_graph` example adds translations without rotating them by the parent, multiplies rotations, and resets rotation to identity at any node without `_r` (the "global orientation" reading from dot_vox #53). `ogt_vox` composes parent then child as full transforms, and so do we. The two rules disagree on 5 real files and 3 fixtures (`logs/rules.txt`). Evidence favours composing: on vengi's `test-transform.vox`, composing leaves 136 of 621,096 cells claimed by two objects, while dot_vox's rule puts 22,668 cells inside other objects. The MagicaVoxel check settles it with fixtures 07 and 08.

### 3. MagicaVoxel check

**Version.** MagicaVoxel 0.99.7.x ships only for Windows. Under Wine 11.18 on macOS it crashes at start (a null call, most likely a missing OpenGL entry point; macOS stops at OpenGL 4.1). No Windows machine was available. The check ran on **0.99.6.2**, the last macOS build (Intel, under Rosetta). It predates version 200, `META`, `_frm_t`/`_frm_r` and some material keys, so those can only be confirmed in 0.99.7.x.

**Method.** Each fixture was opened in MagicaVoxel and screenshotted, then saved from MagicaVoxel into `fixtures/magicavoxel/` and compared with `voxtool compare`. Saving twice gave byte-identical files, so no stray input changed them. Screenshots are in `shots/`.

**Transform composition: MagicaVoxel composes parent then child.** For 07 and 08, a nested file was shown next to two flat files, one placed by composing and one by `dot_vox`'s rule, with two reference voxels so the camera framing is the same. The nested file and the composed flat file are pixel-identical apart from the file name in the title; the `dot_vox` placement lands elsewhere (`shots/mv-07-*.png`). This settles the open question: compose, as `ogt_vox` and our codec do.

**Pivots: floor(size/2), at odd, even and mixed sizes.** For 02, 03 and 04, the marker was shown next to the same voxels inside a model one bigger on each axis (padding on one side only), translated so floor(size/2) puts every voxel in the same cell. Any other rounding would shift them. In all three pairs every voxel lands on the same pixels; only the selection outline differs (`shots/mv-pivot-*.png`).

**Re-saves through `compare`:**

| Fixture | Result |
|---|---|
| 01 to 13, 15 to 17, 19 | Same voxels, palette, materials, layers and world transforms (17 files). Includes all 24 rotations and 24 mirrors, 255^3 and 256^3 models, the 13-model split terrain, layers, a hidden node, the camera and the palette `IMAP`/`NOTE` |
| 14-materials | 0.99.6.2 drops keys it does not know: `_media_type _cloud`, `_spec`, `_trans`. A version gap, not a codec problem |
| 18-overlap-v200 | 0.99.6.2 shows the second instance winning the overlap (the version 150 rule) and saves as version 150 without reordering, so 32 overlap cells change colour. Confirms 0.99.6.2 does not know version 200; the version 200 rule itself needs 0.99.7.x |
| 20-far-positions | MagicaVoxel clamps positions: the markers at x = -1300 and 1200 move to the edge (-1300 became -1000). Confirms the estimate in `docs/research/vox-format.md`: scenes wider than about 2,000 voxels do not survive a trip through MagicaVoxel |
| 21-frm-base-transform | 0.99.6.2 ignores `_frm_t` (added in 0.99.7.2) and places the marker by its first keyframe |

Visual notes: 16 shows the palette in the reversed `IMAP` order with voxel colours unchanged, as expected.

### 4. Fuzzing

Target `fuzz/fuzz_targets/read.rs`: read any input. If the reader accepts it, re-saving must give the same bytes; then flatten the scene graph under both composition rules, apply the three edits, read the edited file back, and check that it re-saves byte-identical too. Built with nightly Rust, AddressSanitizer and overflow checks (`cargo fuzz run -O -a`). Seeds: the 407 corpus files under 64 KB.

Flags: `-max_total_time=3600 -rss_limit_mb=2048 -malloc_limit_mb=512 -max_len=65536 -timeout=10`.

| | |
|---|---|
| Runs | 331,274 in 3,609 s, plus 285,493 in a 3-minute warm-up before it |
| Coverage | 2,979 edges at start, 3,061 at the end; 577 inputs in the working set |
| Crashes, out-of-memory, timeouts | 0, 0, 0. Slowest input under 1 s |
| Peak memory | 1,146 MB with AddressSanitizer (limit 2,048 MB); no single allocation over 512 MB |

The run is slow, about 91 inputs a second, because inputs go up to 64 KB and each one is parsed five times under AddressSanitizer.

Bug found and fixed: in the first second of fuzzing, a negative chunk size inside `MAIN` hit `attempt to subtract with overflow` in `src/raw.rs`, while building the error message's offset. A release build would have printed a wrong offset, not crashed. It is fixed, the input is in `fuzz/regressions/`, and it now returns `Err`.

Two changes came after the fuzz build and are not in the fuzzed binary: the `TDCZ` budget went from 256 Mi down to 64 Mi voxels, and a lint fix touched `rotation_to_byte`, which only the writer uses. The read path is otherwise the same code.

### 5. Property test

`tests/props.rs`, run with `PROPTEST_CASES=20000` (default 1000):

- Random world-space scenes, up to 700 x 700 x 400 with up to 2,500 voxels, offset into negative space, random palette, version 150 or 200: split into models, write, read, write. Byte-identical every time, and the world voxels read back equal the input. 19,526 of the 20,000 split into more than one model.
- One model under a random one of the 48 `_r` matrices and a random translation: every voxel lands where an independent float computation of pivot plus rotation puts it.
- Random raw chunk lists with random ids and contents: the raw layer always round-trips; the full reader either rejects the bytes or gives them back unchanged.
- Random edits on random scenes: only the edited chunks change, and the result re-saves byte-identical.
- A fixed 900 x 700 x 600 scene with its far corners set: 36 models, all voxels back in place.

## What it changes for vxr

- **Keep the two-layer codec.** It met every automated pass line at about 1,700 lines. It goes into the real workspace as the `.vox` module. The raw layer is what makes "open and save never loses anything" cheap.
- **Instancing is part of the format.** 7 of the 35 real scene-graph files reuse a node under several parents. vxr's scene model needs shared models from day one, and its `.vox` writer must keep the sharing instead of copying voxels.
- **Compose transforms parent then child**, as `ogt_vox` does. MagicaVoxel 0.99.6.2 confirms it pixel for pixel. Use `_frm_t`/`_frm_r` as the base pose when present.
- **Warn when a scene is wider than about 2,000 voxels** on `.vox` export: MagicaVoxel clamps positions to about [-1000, 999].
- **Keep voxel indices raw; `IMAP` is display order only.** `ogt_vox` remaps voxel indices through `IMAP`, contrary to the advice in `docs/research/vox-format.md`. Colours come out the same either way, so both readings agree on what you see, but re-saving through `ogt_vox` changes the indices. Ours keeps them.
- **`TDCZ` layout**, which no spec describes: int32 x, y, z, then a zlib stream of x*y*z colour bytes, x fastest, ended with a sync flush instead of a final block. Neither `dot_vox` nor upstream `ogt_vox` reads it. Edits re-compress it.
- **Version 200 overlap order** is implemented (the lower instance wins from version 200, the higher one before). Fixtures 18 and 19 check it in MagicaVoxel.
- **Do not use `ogt_vox` as a writer** for vxr files: it loses `_media_type _cloud` and reorders palettes.
- **Legacy `PACK` animations** (no scene graph, several models): show frame 0. `ogt_vox` shows nothing for them.

## Still open

- MagicaVoxel 0.99.7.x behaviour, which needs a Windows machine: the version 200 overlap rule (18), `_frm_t` against keyframes (21), newer material keys (14), and re-saves written as version 200.
- Few version 200 files: 8 in the corpus. MagicaVoxel-made fixtures from the check should be added once they exist.
- The `TDCZ` voxel order (x fastest) matches vengi's reader and Teardown-Converter's writer, but no third-party reader in this test decodes `TDCZ`, so it is not cross-checked.
- Fuzzing went shallow: 331,274 runs in an hour on one core. Before the reader faces files from strangers in the embed, run it on several cores overnight with a lighter target (read only, smaller inputs). There is no coverage report beyond libFuzzer's edge count, because the nightly toolchain in the dev shell has no `llvm-tools`.

## How to run

From `spikes/vox/`:

```sh
../corpus/fetch-corpus                     # 570 files into ../corpus/fetched/
../dev cargo build --release
./target/release/voxtool roundtrip ../corpus/fetched ../../prototype/demo/scene.vox fixtures
./target/release/voxtool edit-check target/edited ../corpus/fetched
harness/build-ogt && harness/crosscheck-ogt ../corpus/fetched target/edited
../dev cargo run --release --example crosscheck_dotvox -- ../corpus/fetched target/edited
PROPTEST_CASES=20000 ../dev cargo test --release
cd fuzz && ../../dev --fuzz cargo fuzz run -O -a read corpus seeds -- -max_total_time=3600 -rss_limit_mb=2048 -malloc_limit_mb=512 -max_len=65536
```

`fuzz/seeds/` is filled by copying the corpus files under 64 KB; `fuzz/regressions/` holds the inputs of fixed crashes.

## Credits

The bulk of the corpus is [Mini Mike's Metro Minis](https://github.com/mikelovesrobots/mmmm) by Mike Judge, licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). We only read the files; none are changed or committed. The other fetched files belong to their projects (vengi, bevy_vox_scene, vox-rs, dot_vox, ephtracy/voxel-model) and are not committed. The default palette and the `ogt_vox` reference reader come from [opengametools](https://github.com/jpaver/opengametools) (MIT).
