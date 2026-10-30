//! Cameras in a saved file: `Document::set_camera` touches only its own `rCAM` chunk.

use std::path::Path;

use vxr_vox::chunk;
use vxr_vox::{Chunk, Document};

/// Every generated fixture plus the island when it has been built.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> =
        vxr_vox::fixtures::all().into_iter().map(|f| (f.name.to_string(), f.builder.build().write())).collect();
    let island = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prototype/demo/scene.vox");
    match std::fs::read(&island) {
        Ok(b) => out.push(("island".into(), b)),
        Err(_) => eprintln!("island skipped: run `node prototype/src/build.mjs`"),
    }
    out
}

/// The chunk indices whose bytes differ, for two files with the same chunk count.
fn changed(a: &[Chunk], b: &[Chunk]) -> Vec<usize> {
    assert_eq!(a.len(), b.len());
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

/// The `15-camera` fixture (three cameras) with render settings and a note after them, so
/// the cameras sit between other chunks as MagicaVoxel writes them.
fn cameras_file() -> Vec<u8> {
    let mut b = vxr_vox::fixtures::all().into_iter().find(|f| f.name == "15-camera").unwrap().builder;
    b.render_objects = vec![vxr_vox::Dict::default().with("_type", "_setting").with("_ground", "1")];
    b.note = Some(vec![b"row 0".to_vec()]);
    b.build().write()
}

#[test]
fn setting_a_camera_touches_only_its_chunk() {
    use vxr_vox::{Camera, CameraMode};
    let bytes = cameras_file();
    let original = Document::read(&bytes).unwrap();
    let cams = original.cameras();
    assert_eq!(cams.iter().map(|c| c.id).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!((cams[1].mode, cams[1].angle, cams[1].radius), (CameraMode::Orth, [90.0, 0.0, 0.0], 30.0));

    // Unchanged: byte-identical.
    let mut doc = original.clone();
    for c in &cams {
        doc.set_camera(c).unwrap();
    }
    assert_eq!(doc.write(), bytes);

    // One value of camera 1: only its rCAM changes, and in it only `_radius`.
    let i = doc.set_camera(&Camera { radius: 60.0, ..cams[1] }).unwrap();
    assert_eq!(changed(&original.raw.chunks, &doc.raw.chunks), [i]);
    let before = chunk::IdDict::parse(chunk::RCAM, &original.raw.chunks[i].content).unwrap();
    let after = chunk::IdDict::parse(chunk::RCAM, &doc.raw.chunks[i].content).unwrap();
    assert_eq!(after.id, 1);
    assert_eq!(after.dict, before.dict.clone().with("_radius", "60"));
    assert_eq!(doc.camera(1), Some(Camera { radius: 60.0, ..cams[1] }));
    assert_eq!(Document::read(&doc.write()).unwrap().cameras(), [cams[0], Camera { radius: 60.0, ..cams[1] }, cams[2]]);

    // Setting it back gives the file's bytes.
    doc.set_camera(&cams[1]).unwrap();
    assert_eq!(doc.write(), bytes);

    // A camera the file lacks is added after the last rCAM, before the note.
    let i = doc.set_camera(&Camera { mode: CameraMode::Iso, ..Camera::new(5) }).unwrap();
    let last = original.raw.chunks.iter().rposition(|c| c.is(chunk::RCAM)).unwrap();
    assert_eq!(i, last + 1);
    assert_eq!(doc.camera(5).map(|c| c.mode), Some(CameraMode::Iso));
    let mut rest = doc.raw.chunks.clone();
    rest.remove(i);
    assert_eq!(rest, original.raw.chunks, "every other chunk in place");
}

#[test]
fn every_camera_id_reads_as_one_camera() {
    for (name, bytes) in files() {
        let doc = Document::read(&bytes).unwrap();
        let n = doc.raw.chunks.iter().filter(|c| c.is(chunk::RCAM)).count();
        assert_eq!(doc.cameras().len(), n, "{name}: no duplicate ids in the fixtures");
    }
}
