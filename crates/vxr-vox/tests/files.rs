//! Fixtures are generated in memory from `vxr_vox::fixtures`, so no `.vox` files are
//! committed. The prototype island is checked when `node prototype/src/build.mjs` has run.

use std::path::Path;

use vxr_vox::Document;
use vxr_vox::chunk::Typed;

fn assert_round_trip(name: &str, bytes: &[u8]) {
    let doc = Document::read(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(doc.write(), bytes, "{name}");
    for c in &doc.raw.chunks {
        assert_eq!(Typed::parse(c).unwrap().encode(), c.content, "{name} {:?}", c.id);
    }
}

#[test]
fn fixtures_round_trip() {
    vxr_vox::fixtures::check_all().unwrap();
    for f in vxr_vox::fixtures::all() {
        assert_round_trip(f.name, &f.builder.build().write());
    }
}

#[test]
fn prototype_island_round_trips() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    let Ok(bytes) = std::fs::read(&p) else {
        eprintln!("skipped: run `node prototype/src/build.mjs` to generate {}", p.display());
        return;
    };
    assert_round_trip("island", &bytes);
}

/// Found by cargo-fuzz in Phase 2: a negative chunk size overflowed while building the
/// error message.
#[test]
fn fuzz_regression_size_field_offset_underflow() {
    let bytes = include_bytes!("regressions/01-size-field-offset-underflow");
    let _ = Document::read(bytes).and_then(|d| d.scene());
}
