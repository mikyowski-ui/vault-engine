//! Pre-rendered backdrop `.bkd` index and `.bkc` chunk files.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::backdrop::{BackdropChunk, BackdropIndex};

/// A valid index: elevation 1, all planes, chunk size 64, 70 by 3 pixels
/// (two chunks), compression 0 (stored) or 1 (zlib).
fn index(compression: u8) -> BackdropIndex {
    let mut bytes = b"VBKD\x00\x01\x02".to_vec();
    bytes.push(compression);
    bytes.extend_from_slice(&[0, 64, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 70, 0, 0, 0, 3, 0, 2, 0, 1, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[1, 1]);
    BackdropIndex::parse(&bytes).expect("fixed index")
}

fuzz_target!(|data: &[u8]| {
    let _ = BackdropIndex::parse(data);
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let _ = BackdropChunk::parse(rest, &index(first & 1), 1);
});
