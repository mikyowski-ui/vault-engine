//! SPEC-006 `.GCD` player sheets.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::GcdFile;

fuzz_target!(|data: &[u8]| {
    let _ = GcdFile::parse(data);
});
