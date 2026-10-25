//! SPEC-015 §5 and §8: the zlib vectors CPython's encoder made, and every
//! refusal the decoder applies.
use super::*;

/// Every vector: its name, its regenerated plain data and its stream.
fn vectors() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    vault_fixtures::dat2::zlib_vectors()
        .into_iter()
        .map(|vector| (vector.name, vector.plain, vector.stream))
        .collect()
}

#[test]
fn every_vector_decodes_to_its_plain_data() {
    let vectors = vectors();
    let names: Vec<&str> = vectors.iter().map(|(name, ..)| name.as_str()).collect();
    assert_eq!(
        names,
        ["empty", "text", "words", "noise", "stored", "repeat"]
    );
    for (name, plain, stream) in &vectors {
        let decoded =
            zlib_decompress(stream, plain.len()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&decoded, plain, "{name}");
    }
}

#[test]
fn the_output_is_bounded_by_the_expected_size() {
    for (name, plain, stream) in vectors() {
        if plain.is_empty() {
            continue;
        }
        // One byte short: refused as soon as it would grow past.
        let error = zlib_decompress(&stream, plain.len() - 1).unwrap_err();
        assert!(error.to_string().contains("longer than"), "{name}: {error}");
        // One byte more: the data ends short of it.
        let error = zlib_decompress(&stream, plain.len() + 1).unwrap_err();
        assert!(error.to_string().contains("not"), "{name}: {error}");
    }
}

#[test]
fn the_zlib_wrapper_is_checked_whole() {
    let (_, plain, stream) = vectors().swap_remove(2);
    let refuse = |bytes: &[u8], needle: &str| {
        let error = zlib_decompress(bytes, plain.len()).unwrap_err().to_string();
        assert!(error.contains(needle), "{needle}: {error}");
    };
    let damaged = |at: usize, value: u8| {
        let mut bytes = stream.clone();
        bytes[at] = value;
        bytes
    };
    refuse(&[], "ends early");
    refuse(&damaged(0, 0x77), "not DEFLATE");
    refuse(&damaged(0, 0x88), "larger than 32 KiB");
    refuse(&damaged(1, stream[1] ^ 1), "header check");
    // FDICT set, with a header check that still holds.
    let fdict = (0u8..=255)
        .find(|flg| flg & 0x20 != 0 && (0x7800u16 + u16::from(*flg)).is_multiple_of(31))
        .unwrap();
    refuse(&damaged(1, fdict), "preset dictionary");
    let end = stream.len() - 1;
    refuse(&damaged(end, stream[end] ^ 0xff), "Adler-32");
    refuse(&stream[..stream.len() - 2], "checksum is missing");
    refuse(&[stream.as_slice(), &[0]].concat(), "after the checksum");
    refuse(&stream[..stream.len() / 2], "ends early");
}

#[test]
fn undefined_deflate_cases_are_refused() {
    let refuse = |bytes: &[u8], size: usize, needle: &str| {
        let error = inflate(bytes, size).unwrap_err().to_string();
        assert!(error.contains(needle), "{needle}: {error}");
    };
    // BFINAL 1, BTYPE 3.
    refuse(&[0x07], 0, "block type 3");
    // A stored block whose NLEN is not LEN's complement.
    refuse(&[0x01, 0x05, 0x00, 0x00, 0x00], 5, "LEN and NLEN");
    // A fixed block whose first symbol is length 3 at distance 1: before
    // the start of the output.
    refuse(&[0x03, 0x02, 0x00], 3, "before the start");
    // A stored block, then nothing for the next header.
    refuse(&[0x00, 0x00, 0x00, 0xff, 0xff], 0, "ends early");
    // A valid one-byte stored block decodes and reports its length.
    assert_eq!(
        inflate(&[0x01, 0x01, 0x00, 0xfe, 0xff, b'x'], 1).unwrap(),
        (b"x".to_vec(), 6)
    );
}

#[test]
fn adler32_matches_rfc_1950() {
    // RFC 1950 §9's algorithm: 1 for no data, and the common example.
    assert_eq!(adler32(b""), 1);
    assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    // The modular reduction holds over long runs of 0xFF.
    let long = vec![0xff; 100_000];
    let (mut a, mut b) = (1u64, 0u64);
    for &byte in &long {
        a = (a + u64::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    assert_eq!(adler32(&long), ((b << 16) | a) as u32);
}
