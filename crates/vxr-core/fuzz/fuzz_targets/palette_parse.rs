//! Any input: reading a palette file to import (a PNG or text from a stranger) must return
//! Ok or Err, never panic or allocate without bound, and what it accepts is 1 to 255 colours.
#![no_main]
use libfuzzer_sys::fuzz_target;
use vxr_core::palette;

fuzz_target!(|data: &[u8]| {
    let Ok(colors) = palette::parse(data) else { return };
    assert!((1..=255).contains(&colors.len()), "{} colours", colors.len());
});
