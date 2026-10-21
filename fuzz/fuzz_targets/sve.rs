//! SPEC-012 `.SVE` subtitle files.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::sve;

fuzz_target!(|data: &[u8]| {
    let _ = sve::parse(data);
});
