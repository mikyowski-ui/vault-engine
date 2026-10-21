//! SPEC-002 `.MSG` message files under both text modes and the strict
//! reader.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::{MsgCatalog, TextMode};

fuzz_target!(|data: &[u8]| {
    let _ = MsgCatalog::parse(data);
    let _ = MsgCatalog::parse_with_mode(data, TextMode::WindowsLoose);
    let _ = MsgCatalog::parse_strict(data);
});
