//! SPEC-001 `.LST` lists for every consumer, then the ID lookups.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::{LstKind, LstTable};

fuzz_target!(|data: &[u8]| {
    for kind in [
        LstKind::Raw,
        LstKind::Art,
        LstKind::Script,
        LstKind::Prototype,
    ] {
        let Ok(table) = LstTable::parse(data, kind) else {
            continue;
        };
        for id in [0, 1, 2, 0xfff, 0xff_ffff, u32::MAX] {
            let _ = table.art_id(id);
            let _ = table.script_id(id);
            let _ = table.prototype_id(id);
            let _ = table.map_script_reference(id as i32);
        }
    }
});
