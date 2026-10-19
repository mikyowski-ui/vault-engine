//! SPEC-001 `.PAL` palettes and their color tables.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::Palette;

fuzz_target!(|data: &[u8]| {
    let _ = Palette::parse(data);
});
