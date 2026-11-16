//! Any input: the embed's open path (a brick scene file, or a `.vox` file's shown objects)
//! must return Ok or Err, never panic or allocate without bound, and what it shows must
//! write as a brick scene file that reads back to the same bytes.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vxr_core::bricks;

fuzz_target!(|data: &[u8]| {
    let Ok((map, palette)) = bricks::open(data) else { return };
    let file = bricks::encode(&map, &palette);
    let (again, colors) = bricks::decode(&file).expect("what the embed shows writes as a brick scene file");
    assert_eq!(bricks::encode(&again, &colors), file, "and reads back the same");
});
