//! SPEC-010 ACM: header validation and the full bounded sample decode.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::acm;

fuzz_target!(|data: &[u8]| {
    let _ = acm::decode(data);
});
