//! SPEC-015 §5 zlib members: a 4-byte little-endian expected size (capped
//! at 1 MiB), then the stream. The decoder must refuse bad streams without
//! panicking, and never produce more than the expected size.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::inflate::{inflate, zlib_decompress};

fuzz_target!(|data: &[u8]| {
    let Some((size, stream)) = data.split_first_chunk::<4>() else {
        return;
    };
    let size = (u32::from_le_bytes(*size) as usize).min(1 << 20);
    if let Ok(output) = zlib_decompress(stream, size) {
        assert_eq!(output.len(), size);
    }
    if let Ok((output, used)) = inflate(stream, size) {
        assert_eq!(output.len(), size);
        assert!(used <= stream.len());
    }
});
