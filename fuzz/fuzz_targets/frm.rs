//! SPEC-001 `.FRM` frame archives.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::FrmArchive;

fuzz_target!(|data: &[u8]| {
    let _ = FrmArchive::parse(data);
});
