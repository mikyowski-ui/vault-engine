//! `VTLX` tile remap sidecars (`docs/extended-art-namespaces.md` §3.5): the
//! parser, then the writer, whose bytes must equal the input.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::TileRemap;

fuzz_target!(|data: &[u8]| {
    let Ok(remap) = TileRemap::parse(data) else {
        return;
    };
    // The layout has one encoding, so a parsed sidecar writes back exactly.
    assert_eq!(remap.to_bytes(), data);
    // Every local index answers, listed or not.
    for local in [0, 1, 4095] {
        let _ = remap.row(local);
    }
});
