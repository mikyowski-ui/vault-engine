//! SPEC-003 `.PRO` prototypes of every category and subtype.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::Prototype;

fuzz_target!(|data: &[u8]| {
    if let Ok(prototype) = Prototype::parse(data) {
        let _ = prototype.info();
    }
});
