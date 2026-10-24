//! SPEC-010 §4.1-4.2 ACM vectors A-H, plus every column code, the wrapping
//! output rule, and the header and bound errors.
use crate::acm::{AcmHeader, decode, wrap16};

use vault_fixtures::sound;

fn samples(vector: char) -> Vec<i16> {
    decode(&sound::acm(vector)).unwrap().samples
}

#[test]
fn spec010_vectors_a_to_d_decode_exactly() {
    let a = decode(&sound::acm('A')).unwrap();
    assert_eq!(
        a.header,
        AcmHeader {
            samples: 4,
            channels: 1,
            rate: 22050,
            levels: 0,
            rows: 4
        }
    );
    // L = 0: no transform and no +1 bias.
    assert_eq!(a.samples, [300, -400, 0, 100]);
    assert_eq!((a.blocks, a.past_end), (1, 0));
    // B: the level-1 carry persists into the all-zero second block, whose
    // fourth sample is decoded and discarded (N = 7).
    let b = decode(&sound::acm('B')).unwrap();
    assert_eq!(b.samples, [150, 300, 50, -200, -100, 0, 0]);
    assert_eq!(b.blocks, 2);
    // C: format 29 with field 120 (both digits 10) and field 27 (digits 5,
    // 2, the second discarded while its 7 bits are consumed).
    assert_eq!(samples('C'), [-50, -50, 50, 50, 0]);
    // D: format 17 codes 111, 0, 110, 10.
    assert_eq!(samples('D'), [7, 0, 0, -7, 0]);
    assert_eq!(a.pcm(), [44, 1, 112, 254, 0, 0, 100, 0]);
}

#[test]
fn spec010_vectors_e_to_h_edges() {
    // E: a reserved column format ends the stream at its block: nothing is
    // emitted from it (§3.7) and the error is reported.
    let e = decode(&sound::acm('E')).unwrap();
    assert!(e.samples.is_empty() && e.blocks == 0);
    assert!(
        e.failure
            .unwrap()
            .to_string()
            .contains("reserved column format 1")
    );
    // F: format 29 needs T[-5] with k = 0: an error, never stale memory.
    let f = decode(&sound::acm('F')).unwrap();
    assert!(f.samples.is_empty());
    assert!(
        f.failure
            .unwrap()
            .to_string()
            .contains("step-table index -5")
    );
    // The samples of every block before the failing one remain emitted.
    let partial = decode(&file(2, 0, 1, |w| {
        w.put(0, 20);
        w.put(0, 5);
        w.put(0, 20);
        w.put(1, 5);
    }))
    .unwrap();
    assert_eq!((partial.samples.as_slice(), partial.blocks), (&[0][..], 1));
    assert!(partial.failure.is_some());
    // G: the 16-bit step table (step 40000: T[+1] = -25536).
    assert_eq!(samples('G'), [-12768, -25536, 0, 25536]);
    assert_eq!(wrap16(40000), -25536);
    assert_eq!(wrap16(-40000), 25536);
    // H: format 17 looks ahead 3 bits from the final stored bit; the zero
    // byte supplied past the end is not an error.
    let h = decode(&sound::acm('H')).unwrap();
    assert_eq!(h.samples, [7, -7, 0, 0]);
    assert_eq!(h.past_end, 1);
}

/// LSB-first bit writer for synthetic streams.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    bits: u32,
}
impl Writer {
    fn put(&mut self, value: u32, width: u32) {
        for bit in 0..width {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let last = self.bytes.last_mut().unwrap();
            *last |= (((value >> bit) & 1) as u8) << (self.bits % 8);
            self.bits += 1;
        }
    }
    /// Prefix-code bits listed in stream order.
    fn code(&mut self, bits: &[u32]) {
        for bit in bits {
            self.put(*bit, 1);
        }
    }
}
/// A file: header (N, 1 channel, 22050 Hz, L, R), then the body.
fn file(samples: u32, levels: u16, rows: u16, body: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut writer = Writer::default();
    for byte in [0x97, 0x28, 0x03, 0x01] {
        writer.put(byte, 8);
    }
    writer.put(samples, 32);
    writer.put(1, 16);
    writer.put(22050, 16);
    writer.put(u32::from(rows << 4 | levels), 16);
    body(&mut writer);
    writer.bytes
}
/// One column of `rows` values with `k`, `s`, and format `format`.
fn column(rows: u16, k: u32, step: u32, format: u32, codes: impl FnOnce(&mut Writer)) -> Vec<i16> {
    let bytes = file(u32::from(rows), 0, rows, |w| {
        w.put(k, 4);
        w.put(step, 16);
        w.put(format, 5);
        codes(w);
    });
    decode(&bytes).unwrap().samples
}

#[test]
fn spec010_every_column_code() {
    // Format 0: no payload; 3-16: T[v - 2^(f-1)].
    assert_eq!(column(2, 0, 9, 0, |_| {}), [0, 0]);
    assert_eq!(
        column(2, 4, 3, 5, |w| {
            w.put(31, 5);
            w.put(0, 5);
        }),
        [45, -48]
    );
    assert_eq!(
        column(1, 15, 1, 16, |w| w.put(0xffff, 16)),
        [32767],
        "16-bit fields with k = 15"
    );
    // 17: two zeros (a single zero when one row remains), one zero, +/-1.
    assert_eq!(
        column(3, 1, 7, 17, |w| w.code(&[1, 1, 1, 1, 0, 0])),
        [7, 0, 0]
    );
    assert_eq!(column(3, 1, 7, 17, |w| w.code(&[0, 1, 1, 0])), [0, 0, -7]);
    // 18: one zero, or +/-1 from bit 1.
    assert_eq!(
        column(3, 1, 5, 18, |w| w.code(&[1, 1, 0, 1, 0])),
        [5, 0, -5]
    );
    // 19: base-3 digits low first; v >= 27 gives three T[-1].
    assert_eq!(column(3, 1, 2, 19, |w| w.put(5, 5)), [2, 0, -2]);
    assert_eq!(column(3, 1, 2, 19, |w| w.put(30, 5)), [-2, -2, -2]);
    // 20: (bit3, bit2) selects T[-2], T[-1], T[+1], T[+2].
    let twenty = column(5, 2, 3, 20, |w| {
        w.code(&[1, 1, 0, 0]);
        w.code(&[1, 1, 0, 1]);
        w.code(&[1, 0]);
        w.code(&[1, 1, 1, 1]);
        w.code(&[0]);
    });
    assert_eq!(twenty, [-6, 3, 0, 6, 0]);
    // 21: one zero, or (bit2, bit1).
    assert_eq!(
        column(3, 2, 3, 21, |w| w.code(&[1, 0, 0, 1, 1, 0, 0])),
        [-6, -3, 0]
    );
    // 22: base-5 digits; v >= 125 gives three T[-2].
    assert_eq!(
        column(3, 2, 1, 22, |w| w.put(4 + 5 * 3 + 25, 7)),
        [2, 1, -1]
    );
    assert_eq!(column(2, 2, 1, 22, |w| w.put(127, 7)), [-2, -2]);
    // 23: 0 two zeros, 10 one zero, 110 +/-1 by bit 3, 111 (bit4, bit3).
    let twenty_three = column(4, 2, 1, 23, |w| {
        w.code(&[1, 1, 0, 1]);
        w.code(&[1, 1, 1, 0, 0]);
        w.code(&[1, 1, 1, 1, 1]);
        w.code(&[1, 0]);
    });
    assert_eq!(twenty_three, [1, -3, 3, 0]);
    // 24: 0 one zero, 10 +/-1 by bit 2, 11 (bit3, bit2) -> -3, -2, 2, 3.
    let twenty_four = column(3, 2, 1, 24, |w| {
        w.code(&[1, 0, 0]);
        w.code(&[1, 1, 1, 0]);
        w.code(&[0]);
    });
    assert_eq!(twenty_four, [-1, -2, 0]);
    // 26: two zeros, one zero, or (bit4, bit3, bit2) -> -4..-1, 1..4.
    let twenty_six = column(3, 3, 1, 26, |w| {
        w.code(&[1, 1, 0, 0, 0]);
        w.code(&[1, 1, 1, 1, 1]);
        w.code(&[0]);
    });
    assert_eq!(twenty_six, [-4, 4, 0]);
    // 27: one zero, or (bit3, bit2, bit1).
    assert_eq!(column(2, 3, 1, 27, |w| w.code(&[1, 0, 0, 1, 0])), [1, 0]);
    // 29: base-11 digits; v >= 121 gives two T[-5].
    assert_eq!(column(2, 3, 1, 29, |w| w.put(10 + 11 * 2, 7)), [5, -3]);
    assert_eq!(column(2, 3, 1, 29, |w| w.put(125, 7)), [-5, -5]);
    // Reserved codes fail; so does any index outside the filled table.
    for reserved in [1, 2, 25, 28, 30, 31] {
        let bytes = file(1, 0, 1, |w| {
            w.put(0, 20);
            w.put(reserved, 5);
        });
        assert!(
            decode(&bytes).unwrap().failure.is_some(),
            "format {reserved}"
        );
    }
    let bytes = file(1, 0, 1, |w| {
        w.put(1, 4);
        w.put(1, 16);
        w.put(4, 5);
        w.put(0, 4);
    });
    assert!(
        decode(&bytes).unwrap().failure.is_some(),
        "format 4 with k = 1 reads T[-8]"
    );
}

#[test]
fn spec010_lifting_output_wraps_instead_of_saturating() {
    // L = 1, R = 2: column 0 is T[+1] = 32767 then 0; column 1 is T[-1]
    // then 0. Lifting gives 98301 in row 1; +1 and the shift give 49151,
    // which wraps to -16385.
    let bytes = file(4, 1, 2, |w| {
        w.put(1, 4);
        w.put(32767, 16);
        w.put(3, 5);
        w.put(5, 3);
        w.put(4, 3);
        w.put(3, 5);
        w.put(3, 3);
        w.put(4, 3);
    });
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.samples, [16384, -16385, -16383, 16384]);
    assert_eq!(decoded.wrapped, [(1, 49151)]);
}

#[test]
fn spec010_header_errors_and_safe_bounds() {
    let valid = sound::acm('A');
    let mut signature = valid.clone();
    signature[0] = 0x98;
    assert!(decode(&signature).is_err());
    let mut version = valid.clone();
    version[3] = 2;
    assert!(decode(&version).is_err());
    assert!(decode(&valid[..13]).is_err());
    // N = 0 emits nothing for any R, reading no block.
    let empty = file(0, 0, 0, |_| {});
    let decoded = decode(&empty).unwrap();
    assert!(decoded.samples.is_empty() && decoded.blocks == 0);
    // R = 0 with samples to emit would never terminate: rejected.
    assert!(decode(&file(1, 0, 0, |w| w.put(0, 32))).is_err());
    // A block header at or past the end of the file ends decoding there.
    let short = decode(&file(1, 0, 1, |_| {})).unwrap();
    assert!(short.samples.is_empty() && short.failure.is_some());
    // Bounds on decoded samples and block size.
    assert!(decode(&file(u32::MAX, 0, 1, |w| w.put(0, 32))).is_err());
    assert!(decode(&file(1, 15, 4095, |w| w.put(0, 32))).is_err());
}
