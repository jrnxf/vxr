# MagicaVoxel check fixtures

21 small `.vox` files written by our codec (`voxtool fixtures fixtures`), each aimed at one corner of the format. They are ours and free to commit (CC0). `EXPECTED.md` says what each should look like.

Three of them answer the open question of how transforms combine down the scene graph: `07-nested-rotation`, `08-nested-three-levels` and `21-frm-base-transform`. `EXPECTED.md` gives the placement for both readings (ogt_vox composes parent then child; dot_vox's example does not), so one look in MagicaVoxel settles it.

## Checklist (MagicaVoxel 0.99.7.2)

About 1 to 2 hours by hand. Paths are relative to `spikes/vox/`.

1. Open each fixture in MagicaVoxel and compare it with its line in `EXPECTED.md`. Take a screenshot of the default view, plus the render tab for `14-materials`.
2. For `07`, `08` and `21`, write down which of the two placements MagicaVoxel shows (corner position and the direction of the red bar).
3. For `17-animation`, play the animation; for `20-far-positions`, note whether any marker is clamped.
4. Save each fixture from MagicaVoxel under the same file name into `fixtures/magicavoxel/` (create it).
5. Run `../dev cargo run --release --bin voxtool -- compare fixtures fixtures/magicavoxel`. It compares world voxels, palette, materials, layers and world transforms per file. Chunk bytes are expected to differ, since MagicaVoxel rewrites `MATL`, `LAYR`, `rOBJ` and `rCAM` its own way.
6. Optional: run `voxtool edit-check target/edited ../corpus/fetched` and open a few edited real files in MagicaVoxel next to their originals (`vengi__test-transform.vox`, `bevy_vox_scene__study.vox`, `bevy_vox_scene__deer.vox`). Only one voxel, palette slot 1 and one node should differ.
7. Record what you saw in `RESULTS.md` under "MagicaVoxel check".
