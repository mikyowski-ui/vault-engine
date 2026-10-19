//! SPEC-008 LZSS: `0x40` block streams and the automap count-terminated
//! form. The first four bytes choose the requested output length.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::dat::{decode_blocks, decode_until};

/// Four block sizes keep the length large enough for multi-block streams
/// without spending the run on allocation.
const MAX_LENGTH: usize = 4 * vault_formats::dat::BLOCK_SIZE;

fuzz_target!(|data: &[u8]| {
    let Some((prefix, body)) = data.split_first_chunk::<4>() else {
        return;
    };
    let length = u32::from_le_bytes(*prefix) as usize % (MAX_LENGTH + 1);
    let _ = decode_blocks(body, length);
    let _ = decode_until(body, length);
});
