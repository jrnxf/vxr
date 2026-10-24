# .vox format

**Summary**

- Write our own `.vox` codec in the `vxr` workspace. No Rust crate round-trips real MagicaVoxel files: in our test, `dot_vox` drops `rOBJ`, `rCAM`, `NOTE`, `META`, `IMAP` (on write) and every unknown chunk, and `mvox-codec` (the closest fit) is weeks old with 0 stars and rejects Teardown files.
- Build it in two layers. A raw layer keeps every chunk, in order, with dicts as ordered byte pairs, so open-then-save gives back the same bytes. A semantic layer on top handles the scene graph, transforms, materials and palette. Use `ogt_vox` (C, MIT) as the reference for semantics, and `dot_vox` and `vox-rs` as cross-check readers in tests.
- The format has more corners than the official spec covers: version 200 (instance order flipped, `META`), `_frm_t`/`_frm_r` base transforms, IMAP stored as 256 bytes rather than int32, pre-0.99.4 render chunks (`rLIT`, `rAIR`, `rLEN`, `POST`, `rDIS`), legacy `PACK`/`MATT`, and Teardown's `TDCZ`.
- Corpus: `mikelovesrobots/mmmm` (487 files, CC-BY 4.0) for bulk voxels and palettes. Modern scene files come from `vengi`, `bevy_vox_scene`, `vox-rs` and `dot_vox` test data, plus fixtures we make ourselves in MagicaVoxel 0.99.7.2. Licensed files can be committed. Everything else gets fetched by a script and never committed.
- Phase 2 proof: byte-identical re-save across the corpus, semantic equality after edits, a fuzzed reader, and a check in MagicaVoxel itself.

---

## 1. Spec: the documented part

Sources: [MagicaVoxel-file-format-vox.txt](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox.txt) and [MagicaVoxel-file-format-vox-extension.txt](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt). The repo got an MIT `LICENSE` in [April 2026](https://github.com/ephtracy/voxel-model/commits/master). The spec text was last edited in [September 2021](https://github.com/ephtracy/voxel-model/commits/master/MagicaVoxel-file-format-vox.txt).

**Container (RIFF style, little endian)**

- Header: `"VOX "` then an int32 version (150 or 200, see section 2).
- Chunk: 4-byte id, int32 content size N, int32 children size M, N content bytes, M child bytes.
- `MAIN` is the root and has only children. Every other chunk is a flat child of `MAIN`.
- The spec says sizes are `int` (signed). `ogt_vox` reads them as unsigned ([opengametools #82](https://github.com/jpaver/opengametools/issues/82)). Reject negative or overflowing sizes.

**Types used inside chunks** ([extension spec](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt))

- STRING: int32 byte length, then bytes with no terminator. The encoding is not specified.
- DICT: int32 pair count, then STRING key and STRING value for each pair. **Every value is text**, including numbers: `_t` is `"x y z"` and `_r` is the decimal byte, such as `"17"` ([voxel-model #34](https://github.com/ephtracy/voxel-model/issues/34), [#24](https://github.com/ephtracy/voxel-model/issues/24)).
- ROTATION: one byte. Bits 0-1 give the column of the non-zero entry in row 1, bits 2-3 the same for row 2, and bits 4, 5 and 6 give the signs of rows 1, 2 and 3. Row 3's column is whichever one is left. This encodes all 48 signed permutation matrices: 24 rotations and 24 mirror images.

**Chunks**

| Chunk | Content | Notes |
|---|---|---|
| `PACK` | int32 model count | Only for the old frame animation of 0.98.2. Deprecated, but old files still carry it ([#43](https://github.com/ephtracy/voxel-model/issues/43)). |
| `SIZE` | int32 x, y, z | z is up ("gravity direction"). Always comes right before its `XYZI`. |
| `XYZI` | int32 N, then N x (u8 x, y, z, colour index) | Model id = order of appearance, starting at 0 ([#46](https://github.com/ephtracy/voxel-model/issues/46)). |
| `RGBA` | 256 x (u8 r, g, b, a) | Entry i in the file is palette index i+1. The 256th entry is unused (index 0 means empty) ([#21](https://github.com/ephtracy/voxel-model/issues/21)). If the chunk is missing, use the default palette from the spec. |
| `nTRN` | id, DICT attrs (`_name`, `_hidden`), child id, reserved (-1), layer id, frame count, then a DICT per frame (`_r`, `_t`, `_f`) | Transform node. |
| `nGRP` | id, DICT attrs, child count, child ids | Group node. |
| `nSHP` | id, DICT attrs, model count, then per model (model id, DICT with `_f`) | Shape node. More than one model means animation frames. |
| `MATL` | int32 material id, DICT | `_type` (`_diffuse`, `_metal`, `_glass`, `_emit`, plus newer types), `_weight`, `_rough`, `_spec`, `_ior`, `_att`, `_flux`, `_plastic`. |
| `LAYR` | int32 id, DICT (`_name`, `_hidden`), int32 reserved (-1) | |
| `rOBJ` | DICT | Render settings. The author has said these are left undocumented on purpose because they change often ([#19](https://github.com/ephtracy/voxel-model/issues/19)). |
| `rCAM` | int32 id, DICT (`_mode`, `_focus`, `_angle`, `_radius`, `_frustum`, `_fov`) | |
| `NOTE` | int32 count, then STRINGs | Names for palette rows. |
| `IMAP` | "int32 x 256" in the spec, but **actually 256 bytes** | See section 2. |

**Scene graph**: the root is node 0, a `nTRN`. Its child is a `nGRP`, whose children are `nTRN`s, and each `nTRN` points to a `nGRP` or a `nSHP`. To place a model, compose every transform from the root down to its `nSHP` ([#46](https://github.com/ephtracy/voxel-model/issues/46)).

## 2. Undocumented corners

**Version 150 and version 200**

- MagicaVoxel kept writing 150 for years so older versions could still open its files ([ephtracy in #19](https://github.com/ephtracy/voxel-model/issues/19)). Version 0.99.7.0 and later write 200 ([opengametools #40](https://github.com/jpaver/opengametools/issues/40), [voxel-model #65](https://github.com/ephtracy/voxel-model/issues/65)). The change is not documented ([#63](https://github.com/ephtracy/voxel-model/issues/63)).
- What we know changed with 200:
  1. **Instance order flipped.** When instances overlap, the one that wins differs: in version 200 files the lower instance wins, in older files the higher one wins ([ogt_vox.h notes](https://github.com/jpaver/opengametools/blob/master/src/ogt_vox.h), [opengametools #40](https://github.com/jpaver/opengametools/issues/40)). Writing groups before instances breaks this order ([#84](https://github.com/jpaver/opengametools/issues/84)).
  2. **`META` chunk** (since 0.99.7.2): a DICT with `_anim_range` set to `"start end"` ([opengametools #76](https://github.com/jpaver/opengametools/issues/76)). The [0.99.7.2 release notes](https://softantenna.com/softwares/6915-magica-voxel) say the animation range is now saved in `META`. In the files we inspected, `META` comes first under `MAIN`, before the first `SIZE`.
- `ogt_vox` rejects any version other than 150 or 200 ([ogt_vox.h](https://github.com/jpaver/opengametools/blob/master/src/ogt_vox.h)). We should read any version, warn on unknown ones, and write back the version we read.

**Animation: `_f`, `_loop`, `_frm_t`, `_frm_r`**

- Each `nTRN` frame DICT carries `_f` (a keyframe index) next to `_t` and `_r`. Each `nSHP` model entry carries `_f` to pick which model shows at which frame ([extension spec](https://github.com/ephtracy/voxel-model/blob/master/MagicaVoxel-file-format-vox-extension.txt)).
- `_loop` (0/1) in node attributes marks a looping track. Frames between keys are interpolated, and outside the range they loop or clamp ([ogt_vox looping](https://github.com/jpaver/opengametools/blob/master/src/ogt_vox.h), [opengametools #34](https://github.com/jpaver/opengametools/issues/34)).
- 0.99.7.2 added `_frm_t` and `_frm_r` to the **node attribute DICT**. They hold the node's base translation and rotation, separate from its keyframes ([vengi #583](https://github.com/vengi-voxel/vengi/issues/583), [opengametools #89](https://github.com/jpaver/opengametools/issues/89)). We found `_frm_t` in real files: `deer.vox` in [bevy_vox_scene/assets](https://github.com/oliver-dew/bevy_vox_scene/tree/main/assets) and `test_meta_chunk.vox` in [vox-rs/testdata](https://github.com/zeozeozeo/vox-rs/tree/main/testdata).
- The prototype reads only frame 0, and it places every model in a `nSHP`. An animated file would show all of its frames stacked on top of each other.

**Rotations and pivots**

- Rotation and flip are stored as one matrix. You can tell a mirror from a rotation by its determinant (row1 x row2 compared with row3), but the axis of the flip cannot be recovered: the same pose can come from different sequences of steps ([ephtracy in #23](https://github.com/ephtracy/voxel-model/issues/23)). Store the matrix, not "rotation + flip".
- A model's pivot is `floor(size / 2)` in voxel-corner space. Rotation applies around the pivot, then `_t` is added ([ogt_vox "model pivots"](https://github.com/jpaver/opengametools/blob/master/src/ogt_vox.h)). Readers keep getting this wrong for odd and even sizes ([vengi #110](https://github.com/vengi-voxel/vengi/issues/110), [vengi #69](https://github.com/vengi-voxel/vengi/issues/69)).
- Whether rotations compose down the tree is disputed. `ogt_vox` composes transforms from parent to child. In 2025 `dot_vox` changed its example to say a transform node stores a *global* orientation ([dot_vox #53](https://github.com/dust-engine/dot_vox/pull/53)). Settle this in Phase 2 with a nested, rotated fixture opened in MagicaVoxel.

**Palette index map (`IMAP`)**

- It is 256 **bytes**, not int32s. Two PRs to fix the spec were closed without merging ([#54](https://github.com/ephtracy/voxel-model/pull/54), [#57](https://github.com/ephtracy/voxel-model/pull/57)).
- It records the palette display order after the user drags colours around. Voxel indices still index `RGBA` directly. Do not remap voxels through it ([aardappel and others in #19](https://github.com/ephtracy/voxel-model/issues/19), [dot_vox docs](https://github.com/dust-engine/dot_vox/blob/master/src/dot_vox_data.rs)). `NOTE` names follow the display order ([#19](https://github.com/ephtracy/voxel-model/issues/19)).

**Materials (`MATL`, old `MATT`)**

- MagicaVoxel writes all 256 `MATL` chunks even when nothing uses them, which adds a fixed amount to every file ([#32](https://github.com/ephtracy/voxel-model/issues/32)). Our test files bear this out: every modern file has exactly 256.
- Keys found in real files (our scan of the test corpora in section 4): `_type`, `_weight`, `_rough`, `_spec`, `_ior`, `_ri` (the refractive index, `_ior` + 1), `_att`, `_flux`, `_emit`, `_ldr`, `_plastic`, `_alpha`, `_trans`, `_d`, `_g`, `_media`, `_media_type`, `_metal`, `_glow`, `_g0`, `_g1`, `_gw`, `_spec_p`. The spec documents fewer than half of them. Newer types include `_blend` and `_media` (cloud) ([ogt_vox](https://github.com/jpaver/opengametools/blob/master/src/ogt_vox.h), [opengametools #64](https://github.com/jpaver/opengametools/issues/64), [#66](https://github.com/jpaver/opengametools/issues/66)). Treat the key set as open-ended and keep keys you do not know.
- `MATT` (before 0.99) is binary: int32 id, int32 type (0 diffuse, 1 metal, 2 glass, 3 emissive), float weight, u32 property bits (plastic, roughness, specular, IOR, attenuation, power, glow, isTotalPower), then one float per set bit ([original 2016 spec](https://github.com/ephtracy/voxel-model/blob/8e53f38989/MagicaVoxel-file-format-vox.txt)). `ephtracy/voxel-model/vox/anim/deer.vox` still contains 255 of them (our scan).

**Layers, render objects, cameras**

- The number of `LAYR` chunks grew across versions: 8, then 16, then 32 in our samples. `LAYR` can carry `_color` as `"r g b"` ([opengametools #31](https://github.com/jpaver/opengametools/issues/31)). An instance's layer id can be -1 ([opengametools #60](https://github.com/jpaver/opengametools/issues/60)).
- `rOBJ` is a list of DICTs keyed by `_type` (`_bounce`, `_env`, `_inf` (sun), `_uni`, `_ibl`, `_atm`, `_fog_uni`, `_lens`, `_film`, `_bloom`, `_ground`, `_bg`, `_edge`, `_grid`, `_setting`). A parsed dump is in [#31](https://github.com/ephtracy/voxel-model/issues/31). There are 11 to 15 per file in our samples.
- `rCAM`: 10 per file in modern samples. `_mode` is one of `pers`, `free`, `pano`, `orth`, `iso` ([vox-rs codec](https://github.com/zeozeozeo/vox-rs/blob/main/src/codec.rs)).
- Files from 0.99.0 to about 0.99.3 carry render chunks that were replaced later: `rLIT`, `rAIR`, `rLEN`, `POST`, `rDIS`. You can see them in [dot_vox's own test files](https://github.com/dust-engine/dot_vox/tree/master/src/resources) (our scan). No spec describes them, so a lossless codec has to carry them through as opaque chunks.

**Size limits**

- One model is at most 256 on each axis, because `XYZI` stores coordinates as u8. Some readers fail at exactly 256 but work at 255 ([vengi #71](https://github.com/vengi-voxel/vengi/issues/71)). Include a 256^3 fixture.
- The scene can be bigger through many models. Before 0.99 the limit was 126^3 ([#4](https://github.com/ephtracy/voxel-model/issues/4)). MagicaVoxel itself seems to clamp world positions to about [-1000, 999] ([#66](https://github.com/ephtracy/voxel-model/issues/66), a user report). So a vxr scene larger than about 2000 voxels across can be saved, but MagicaVoxel may not show it correctly. That is an estimate to confirm in Phase 2.
- Saving a big vxr scene means splitting it into 256^3 models placed by `nTRN` (the prototype already does this). Any re-split has to keep the pivot rule above.

**Other writers and quirks**

- Teardown files use `TDCZ`, which is a zlib-compressed `XYZI`, in place of `XYZI` ([opengametools #79](https://github.com/jpaver/opengametools/issues/79), [Teardown-Converter](https://github.com/TTFH/Teardown-Converter/blob/a75787ac6ca943a9838bf2fc746fe08aaab3c51f/src/vox_writer.cpp#L115C1-L128C2)). Our scan of `vengi/data/tests/teardown.vox` confirms this: 8 `SIZE` + `TDCZ` pairs.
- voxedit.io writes `nTRN` with a reserved id other than -1, and MagicaVoxel opens those files anyway ([opengametools #52](https://github.com/jpaver/opengametools/issues/52)). Warn on this, do not fail.
- Re-saving can stack an extra root group each time if the reader adds its own root ([vengi #189](https://github.com/vengi-voxel/vengi/issues/189)). Map MagicaVoxel's root `nTRN` and `nGRP` to our root instead of wrapping them.
- MagicaVoxel writes node ids 0..n in emission order, and layer ids 0..n. We checked all 16 scene-graph files in our sample. Other tools may not, so do not depend on it.
- Crafted `SIZE` values have caused heap overflows in `ogt_vox` ([opengametools #87](https://github.com/jpaver/opengametools/issues/87)). The embed will load files from strangers, so the reader must be bounds-checked and fuzzed.
- About "#264": `ephtracy/voxel-model` has only 67 issues. The closest match is [vengi #264](https://github.com/vengi-voxel/vengi/issues/264) (open), which asks to import every animation frame. It fits the `_f` notes above.

## 3. Rust crates

**Metadata** (from crates.io and GitHub, October 2026)

| Crate | Version, last release | Downloads | Stars | Licence | Reads | Writes |
|---|---|---|---|---|---|---|
| [dot_vox](https://crates.io/crates/dot_vox) | 5.2.0, Jan 2026 | 257k | 69 | MIT | SIZE, XYZI, RGBA, MATL, nTRN, nGRP, nSHP, LAYR, IMAP | SIZE, XYZI, nTRN, nGRP, nSHP, RGBA, MATL, LAYR |
| [mvox-codec](https://crates.io/crates/mvox-codec) + [mvox](https://crates.io/crates/mvox) | 0.2.1, Oct 2026 | about 250 | 0 ([tyleo/tyt](https://github.com/tyleo/tyt) monorepo) | MIT | everything, unknown chunks kept verbatim | everything except PACK |
| [vox-rs](https://crates.io/crates/vox-rs) | 0.2.0, Apr 2026 | 72 | 2 | Unlicense | everything incl. META, MATT; flattens the scene | its own rebuilt scene |
| [vox-format](https://crates.io/crates/vox-format) | 0.1.0, Jul 2021 | 7.5k | 9 | MIT | models, palette | PACK, models, palette ("work-in-progress") |
| [create_vox](https://crates.io/crates/create_vox) | 1.0.1, Aug 2022 | 11.7k | 2 | MIT | basic | basic, no MATL |
| [vox_writer](https://crates.io/crates/vox_writer) | 0.1.9, Aug 2023 | 12.6k | n/a | n/a | none | basic |

`bevy_vox_scene` (51 stars, active) reads through `dot_vox` and writes nothing ([Cargo.toml](https://github.com/oliver-dew/bevy_vox_scene/blob/main/Cargo.toml)). The best reference implementation is not in Rust: [ogt_vox](https://github.com/jpaver/opengametools) (C++, MIT, 424 stars, pushed Oct 2026), which `vengi` uses ([vengi #74](https://github.com/vengi-voxel/vengi/issues/74)). `vox-rs` is a port of it.

**Round-trip test (ours).** For each crate we read a file, wrote it back, and compared chunks by id and exact content. Corpus: 15 files from the sources in section 4, covering v150, v200, legacy, Teardown and animation. Code: a throwaway harness, not committed.

| Crate | Byte-identical | What changes or is lost |
|---|---|---|
| dot_vox | 3 of 15 (only old single-model files) | **Drops** `rOBJ`, `rCAM`, `NOTE`, `META`, `PACK`, `MATT`, the old render chunks and `TDCZ`. **Reads `IMAP` but never writes it.** DICTs are `HashMap`s, so key order changes in `MATL` and `nTRN` (values are kept). On the Teardown file it silently loses all 8 models and leaves `nSHP` pointing at nothing. |
| mvox-codec | 1 of 15 | Values are kept. Key order changes: known `MATL` and frame keys get moved ahead of the rest. Drops `PACK`. Writes alpha 0 in the unused 256th `RGBA` slot. **Rejects the Teardown file** ("two SIZE chunks without an intervening XYZI"). |
| vox-rs | 0 of 15 | Rebuilds the file from scratch: flattens groups, moves voxels into dense storage, keeps only the sun from `rOBJ`, drops `IMAP` and the old render chunks. Fails on the Teardown file. |

Code-level findings behind those results:

- **dot_vox**: `Dict = HashMap<String, String>` ([parser.rs](https://github.com/dust-engine/dot_vox/blob/master/src/parser.rs)). Unknown chunks are logged at debug level and thrown away. A chunk that fails to parse, such as one with a non-UTF-8 name string, becomes `Chunk::Invalid` and is dropped without an error. The writer renumbers nodes and layers by their position in the list ([dot_vox_data.rs](https://github.com/dust-engine/dot_vox/blob/master/src/dot_vox_data.rs)). Voxel index 0 becomes index 1 through `saturating_sub(1)` ([model.rs](https://github.com/dust-engine/dot_vox/blob/master/src/model.rs)). Its own round-trip test compares a synthetic placeholder with itself, not real files ([lib.rs tests](https://github.com/dust-engine/dot_vox/blob/master/src/lib.rs)). The `rOBJ`/`rCAM` PR has been open since 2022 ([#30](https://github.com/dust-engine/dot_vox/pull/30)), and so has fuzzing ([#41](https://github.com/dust-engine/dot_vox/pull/41)). Releases come slowly: 5.1.1 in 2023, 5.2.0 in 2026.
- **mvox-codec**: the data model is the right one. Dicts are `Vec<(String, String)>`, every type has an `extra` dict for unknown keys, and unknown chunks are kept ([mvox_file.rs](https://github.com/tyleo/tyt/tree/main/projects/voxel-formats/mvox/src/roots), [from_mvox_file_bytes.rs](https://github.com/tyleo/tyt/blob/main/projects/voxel-formats/mvox-codec/src/from_mvox_file_bytes.rs)). But the writer uses a fixed chunk order and puts unknown chunks last ([to_mvox_file_bytes.rs](https://github.com/tyleo/tyt/blob/main/projects/voxel-formats/mvox-codec/src/to_mvox_file_bytes.rs)), materials store parsed `f32`s, and the whole project is one person's few-week-old monorepo.
- **vox-rs**: a good semantic oracle (animation sampling, flattened instances), but it is a scene converter by design ([README](https://github.com/zeozeozeo/vox-rs/blob/main/README.md)).

**Recommendation: write our own.** Fork nothing. Reasons:

1. No crate keeps chunk order and every chunk. Fixing `dot_vox` would mean changing its public data model (HashMap to ordered, opaque chunks, explicit ids). That is a fork in all but name, owned by someone else.
2. The codec is small. Our estimate is about 1,500 lines plus tests: about 2 to 4 agent-hours to write, and about a day of wall clock with fixture work and a MagicaVoxel check. `ogt_vox.h` covers the full semantics in about 3k lines of C++.
3. `.vox` I/O sits on the main user path for vxr (decisions: "`.vox` open and save" in the first app). Owning it also lets us carry vxr-only data in our own chunk, which MagicaVoxel skips.
4. Design: **raw layer** = header version + ordered `Vec<Chunk { id: [u8; 4], content: Vec<u8>, children: Vec<u8> }>`, with DICTs as ordered `Vec<(Vec<u8>, Vec<u8>)>` so non-UTF-8 strings survive. **Semantic layer** = a scene graph, a 4x4 or 3x3 integer transform per frame, palette + IMAP + NOTE, and materials as ordered dicts with typed getters. On save, re-emit untouched chunks byte for byte and regenerate only what was edited.
5. Keep `dot_vox`, `mvox-codec` and `vox-rs` (or `ogt_vox` through a C build) as dev-dependencies for differential tests. They help find bugs but are not the target.

## 4. Test corpora

| Source | Files | Contents | Licence | Use |
|---|---|---|---|---|
| [mikelovesrobots/mmmm](https://github.com/mikelovesrobots/mmmm) | 487 .vox, 42.7 MB (GitHub tree API) | v150, one model each, `SIZE XYZI RGBA` | [CC-BY 4.0](https://github.com/mikelovesrobots/mmmm/blob/master/LICENSE) | Bulk voxel and palette checks. Commit allowed, with credit. |
| [ephtracy/voxel-model `vox/`](https://github.com/ephtracy/voxel-model/tree/master/vox) | about 45 | Mostly legacy v150: single model, or `PACK` + `MATT` (anim). No scene graph. Includes `menger`, `monu3` (3.4 MB) | Repo has been MIT since 2026, but [#22](https://github.com/ephtracy/voxel-model/issues/22) (asking for a licence) is still open, and the characters and monuments are fan art of other games (per the folder readmes) | Local testing only. Do not commit. |
| MagicaVoxel download's `vox/` folder | about a dozen (estimate) | v200 samples such as `castle`, `doom`, `chr_knight` ([#65](https://github.com/ephtracy/voxel-model/issues/65)) | None stated | Local testing only. |
| [vengi `data/tests`](https://github.com/vengi-voxel/vengi/tree/master/data/tests) | 19 .vox | `teardown.vox` (TDCZ), `animation.0.99.7.1.vox`, `test-transform.vox`, `test_material.vox` (3 MB), `splitobjects`, `8ontop` | Code is MIT. Data licences are per-file and mostly unstated ([data/tests/README](https://github.com/vengi-voxel/vengi/blob/master/data/tests/README.md)) | Fetch by script. Do not commit. |
| [bevy_vox_scene `assets`](https://github.com/oliver-dew/bevy_vox_scene/tree/main/assets) | 5 | `deer.vox` (v200, META, `_frm_t`, 32 layers), `study.vox` (1 MB, 78 nodes), `cloud.vox` (cloud media) | Repo MIT | Fetch by script. Ask before committing. |
| [vox-rs `testdata`](https://github.com/zeozeozeo/vox-rs/tree/main/testdata) and [dot_vox `resources`](https://github.com/dust-engine/dot_vox/tree/master/src/resources) | 8 + 6 | `test_groups` (44 nodes), `test_multiple_model_scene` (149 nodes, IMAP), `test_meta_chunk`, old 0.99.0 render chunks | Unlicense / MIT, but some are MagicaVoxel samples | Fetch by script. |
| OpenGameArt and itch.io packs | 12 to 363 per pack | Artist files, versions vary | CC0 ([voxel creature pack 1](https://opengameart.org/content/voxel-creature-pack1), [55 voxel creatures](https://opengameart.org/content/55-voxel-creatures), [modular bathroom](https://opengameart.org/content/modular-bathroom-voxel-art), [FuzzyManStudios medieval, 363 models](https://fuzzymanstudios.itch.io/voxel-art); check the licence bundled in each archive, [comment](https://opengameart.org/node/157057)) | Commit allowed. |
| Teardown game assets | thousands (estimate) | MagicaVoxel files with game materials, some `TDCZ` | Proprietary. We found no redistribution terms ([modding docs](https://get-teardown.readthedocs.io/en/latest/mods/creating-your-own-assets.html)) | Local testing only, never commit. vengi's `teardown.vox` covers `TDCZ`. |
| **Our own fixtures** | about 20 (estimate) | Made in MagicaVoxel 0.99.7.2 for this purpose: all 48 `_r` values, nested rotated groups, hidden nodes and layers, layer colours, keyframes with `_loop` and `META`, every material type, edited cameras, a reordered palette (IMAP) with NOTE names, a 256^3 model, overlapping instances | CC0, ours | Commit. This is the core of the test. |

Plan: a `fetch-corpus` script with URL + sha256 for each third-party file, downloading into a git-ignored directory. Only CC0, CC-BY (credited) and our own files get committed.

## 5. What the prototype handles and skips

From `prototype/src/core.js` (`readVox` and `writeVox`):

- **Reads**: `SIZE`/`XYZI` pairs, `RGBA` (with the correct +1 shift), and `nTRN`/`nGRP`/`nSHP` with translation from frame 0 only. Pivot is `floor(size / 2)`, which matches ogt_vox. Files without a scene graph are placed at the origin. It converts z-up to y-up.
- **Skips**: `_r` rotation, frames after 0 and `_f` (every model in a `nSHP` gets drawn), `_hidden`, `LAYR`, `MATL`/`MATT`, `IMAP`, `NOTE`, `rOBJ`, `rCAM`, `META`, `PACK`, `TDCZ`. Unknown chunks are skipped safely.
- **Writes**: version 200 with no `META`. Splits the scene into 256^3 models under one group, one `LAYR`, `RGBA` (last slot 0). No `MATL`, so MagicaVoxel falls back to its defaults.
- **Lossless claim**: it only covers voxels and palette, and only for its own files (decisions.md). For real files it is lossy in all of the skipped areas above.

## 6. How to prove lossless round-trip in Phase 2

Define "lossless" at three levels and test all three:

1. **Unedited re-save is byte-identical.** For every corpus file: `read -> write` returns the same bytes. Pass: 100% of the corpus, including legacy, Teardown and v200.
2. **Edited re-save keeps everything it did not touch.** Apply a scripted edit (set one voxel, recolour one palette slot, move one node). Then compare the two files as a canonical chunk list (ordered DICT pairs, chunk order kept). Pass: the only differences are the chunks the edit had to change. Also check that `dot_vox` and `ogt_vox` read the voxels and world transforms the same before and after the edit.
3. **MagicaVoxel agrees.** Open our fixtures and our edited re-saves in MagicaVoxel 0.99.7.2. Screenshot each from a fixed camera and compare with screenshots of the originals. Then re-save in MagicaVoxel and confirm our semantic layer reads the same scene back. MagicaVoxel's Mac build is Intel-only (decisions.md), so this needs Rosetta or a Windows machine. Roughly 1 to 2 hours of manual work for about 20 fixtures (estimate).

Also:

- **Fuzz** the reader with `cargo-fuzz`, starting from the corpus as seeds. Pass: no panics and no unbounded allocation after a 1-hour run (estimate of a sensible budget), and malformed input returns `Err`.
- **Property test**: random scenes (including models over 256^3 that get split) go through write, read, write. Pass: the two outputs are byte-identical, and the voxels match exactly.

**Open risks for Phase 2**

- Whether rotations compose down the tree (dot_vox #53 against ogt_vox). This can only be settled in MagicaVoxel itself.
- Pivot and flip off-by-one on even and odd sizes.
- MagicaVoxel's world clamp of about ±1000 for scenes larger than it supports.
- Unknown keys in future MagicaVoxel versions. The raw layer covers these.
- String encoding of non-ASCII names is not specified. Keep them as bytes.
