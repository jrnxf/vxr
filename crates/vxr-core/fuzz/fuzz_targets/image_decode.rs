//! Any input: reading an image to import (a PNG from a stranger) must return Ok or Err,
//! never panic or allocate without bound, and what it accepts has one pixel per cell of a
//! picture at most `MAX_SIDE` on a side.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vxr_core::image;

fuzz_target!(|data: &[u8]| {
    let Ok(img) = image::decode(data) else { return };
    assert!(img.width.max(img.height) <= image::MAX_SIDE, "{}x{}", img.width, img.height);
    assert_eq!(img.pixels.len(), (img.width * img.height) as usize);
});
