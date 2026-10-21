//! SPEC-008 §2.8 automap database: the offset table, then every raster the
//! table can address (each an LZSS decode to an exact count).
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::AutomapDb;

fuzz_target!(|data: &[u8]| {
    let Ok(db) = AutomapDb::parse(data.to_vec()) else {
        return;
    };
    for map in 0..160 {
        for elevation in 0..3 {
            let _ = db.raster(map, elevation);
        }
    }
});
