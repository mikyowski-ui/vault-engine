//! SPEC-014 `.RIX` splash images.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::rix::Splash;

fuzz_target!(|data: &[u8]| {
    let _ = Splash::parse(data);
});
