Three targets.

- `scene_from_vox`: any bytes through `Scene::from_vox` (the whole open path: the codec, the
  scene graph tree and the world) must give `Ok` or `Err` without panicking or allocating
  without bound, and an accepted file must save again byte for byte and open again.
- `bricks_decode`: any bytes through `vxr_core::bricks::decode` (the brick scene file the
  embed loads) must give `Ok` or `Err` the same way, and an accepted file must encode back
  byte for byte.
- `embed_open`: any bytes through `vxr_core::bricks::open` (what the embed viewer does with
  a file from anywhere: a brick scene file, or a `.vox` file's shown objects) must give
  `Ok` or `Err` the same way, and what it shows must write as a brick scene file that
  reads back the same. Seed it with both corpora.

```sh
# Seeds: the generated fixtures, the codec's regression file, the island and, when
# `spikes/corpus/fetch-corpus` has run, the third-party corpus.
./dev --fuzz cargo run --manifest-path crates/vxr-core/fuzz/Cargo.toml --example seed -- crates/vxr-core/fuzz/corpus/scene_from_vox
./dev --fuzz sh -c 'cd crates/vxr-core/fuzz && cargo fuzz run scene_from_vox -- -max_total_time=3600 -rss_limit_mb=2048'
# The same scenes as brick scene files.
./dev --fuzz cargo run --manifest-path crates/vxr-core/fuzz/Cargo.toml --example seed -- --bricks crates/vxr-core/fuzz/corpus/bricks_decode
./dev --fuzz sh -c 'cd crates/vxr-core/fuzz && cargo fuzz run bricks_decode -- -max_total_time=3600 -rss_limit_mb=2048'
# Both kinds of file through the embed's open path.
./dev --fuzz sh -c 'cd crates/vxr-core/fuzz && cargo fuzz run embed_open corpus/embed_open corpus/scene_from_vox corpus/bricks_decode -- -max_total_time=3600 -rss_limit_mb=2048'
```

A crash lands in `artifacts/<target>/`. Turn it into a test beside
`crates/vxr-vox/tests/hostile.rs` (through `Scene::from_vox` in `crates/vxr-core/tests/`), or
in `crates/vxr-core/tests/bricks.rs` for a brick scene file.
