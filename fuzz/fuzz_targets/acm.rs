//! SPEC-010 ACM: header validation and the full bounded sample decode.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::acm::{self, AcmHeader};

fuzz_target!(|data: &[u8]| {
    // The decoder's own bounds (64 MiB of samples, 4 Mi block elements)
    // are too generous for a fuzzing loop; skip inputs whose untrusted
    // header would have one block or the output allocate more than that.
    if let Ok(header) = AcmHeader::parse(data)
        && (header.samples > 1 << 20 || usize::from(header.rows) * header.columns() > 1 << 18)
    {
        return;
    }
    let _ = acm::decode(data);
});
