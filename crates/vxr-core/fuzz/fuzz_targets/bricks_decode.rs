//! Any input: reading a brick scene file (what the embed loads from strangers) must return
//! Ok or Err, never panic or allocate without bound, and a file it accepts must encode back
//! byte for byte, so nothing is read that the file does not say.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vxr_core::bricks;

fuzz_target!(|data: &[u8]| {
    let Ok((map, palette)) = bricks::decode(data) else { return };
    assert_eq!(bricks::encode(&map, &palette), data, "an accepted file has one encoding");
});
