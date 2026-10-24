//! Round-trip every committed fixture, the prototype scene, and (when it has been
//! fetched with corpus/fetch-corpus) the whole third-party corpus.

use std::path::Path;

use vox::Document;
use vox::chunk::Typed;

fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vox")) {
            out.push(p);
        }
    }
}

#[test]
fn fixtures_round_trip() {
    vox::fixtures::check_all().unwrap();
}

#[test]
fn corpus_round_trips_byte_for_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut fs = vec![root.join("../../prototype/demo/scene.vox")];
    files(&root.join("fixtures"), &mut fs);
    files(&root.join("../corpus/fetched"), &mut fs);
    let (mut ok, mut rejected) = (0, 0);
    for f in &fs {
        let bytes = std::fs::read(f).unwrap();
        let Ok(doc) = Document::read(&bytes) else {
            // Only the two known non-MagicaVoxel files may be rejected.
            let name = f.file_name().unwrap().to_string_lossy();
            assert!(name == "not_a.vox" || name == "slab6_vox_test.vox", "rejected {}", f.display());
            rejected += 1;
            continue;
        };
        assert_eq!(doc.write(), bytes, "{}", f.display());
        for c in &doc.raw.chunks {
            assert_eq!(Typed::parse(c).unwrap().encode(), c.content, "{} {:?}", f.display(), c.id);
        }
        ok += 1;
    }
    eprintln!("{ok} files byte-identical, {rejected} rejected as not MagicaVoxel");
}
