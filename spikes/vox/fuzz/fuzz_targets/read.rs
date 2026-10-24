//! Any input: the reader must return Ok or Err, never panic or allocate without
//! bound. Anything it accepts must re-save byte for byte, and survive the edits.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vox::Document;
use vox::scene::Rule;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::read(data) else { return };
    assert_eq!(doc.write(), data, "accepted input must re-save byte-identical");
    let scene = doc.scene().expect("read already checked the semantic layer");
    let (inst, _) = scene.instances(Rule::Compose);
    let _ = scene.instances(Rule::DotVoxExample);
    let mut d = doc.clone();
    let _ = d.set_palette(1, [1, 2, 3, 4]);
    if !scene.models.is_empty() {
        let _ = d.set_voxel(0, [0, 0, 0], 5);
    }
    if let Some(id) = inst.iter().find_map(|i| i.transform_node) {
        let _ = d.set_translation(id, [1, -2, 3]);
    }
    let w = d.write();
    let again = Document::read(&w).expect("an edited file reads back");
    assert_eq!(again.write(), w, "an edited file re-saves byte-identical");
});
