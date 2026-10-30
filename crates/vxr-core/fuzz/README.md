# vxr-core fuzz

One target, `scene_from_vox`: any bytes through `Scene::from_vox` (the whole open path: the
codec, the scene graph tree and the world) must give `Ok` or `Err` without panicking or
allocating without bound, and an accepted file must save again byte for byte and open again.

```sh
# Seeds: the generated fixtures, the codec's regression file, the island and, when
# `spikes/corpus/fetch-corpus` has run, the third-party corpus.
./dev --fuzz cargo run --manifest-path crates/vxr-core/fuzz/Cargo.toml --example seed -- crates/vxr-core/fuzz/corpus/scene_from_vox
./dev --fuzz sh -c 'cd crates/vxr-core/fuzz && cargo fuzz run scene_from_vox -- -max_total_time=3600 -rss_limit_mb=2048'
```

A crash lands in `artifacts/scene_from_vox/`. Turn it into a test beside
`crates/vxr-vox/tests/hostile.rs` (through `Scene::from_vox` in `crates/vxr-core/tests/`).
