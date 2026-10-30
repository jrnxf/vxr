//! Any input: opening a file must return Ok or Err, never panic or allocate without
//! bound, and a file the editor accepts must save again byte for byte (the lossless
//! save) and open again.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vxr_core::Scene;

fuzz_target!(|data: &[u8]| {
    let Ok(scene) = Scene::from_vox(data) else { return };
    let saved = scene.to_vox();
    assert_eq!(saved, data, "an accepted, unedited file must save byte-identical");
    assert!(Scene::from_vox(&saved).is_ok(), "a saved file opens again");
});
