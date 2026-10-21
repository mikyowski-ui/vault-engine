//! SPEC-006 `.GAM` variable initializers, as the campaign file and as a
//! map-specific file.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::{GamFile, GamSection};

fuzz_target!(|data: &[u8]| {
    let _ = GamFile::parse(data, GamSection::Campaign);
    let _ = GamFile::parse(data, GamSection::Map);
});
