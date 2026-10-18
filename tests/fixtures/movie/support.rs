/// Space-separated hex bytes, one or more lines.
fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// SPEC-012 §4.1 Fixture A: the header and a first chunk header.
pub fn header_a() -> Vec<u8> {
    hex(include_str!("fixture-a.hex"))
}
/// Fixture A variants A2-A5, each rejected.
pub fn header_variants() -> Vec<(&'static str, Vec<u8>)> {
    let mut variants = Vec::new();
    let mut a2 = header_a();
    a2[0x18] = 0x34;
    variants.push(("A2 check 4404", a2));
    let mut a3 = header_a();
    a3[0x16..0x1a].copy_from_slice(&[0x01, 0x01, 0x32, 0x11]);
    variants.push(("A3 version 257", a3));
    let mut a4 = header_a();
    a4[19] = 0x20;
    variants.push(("A4 signature byte 19", a4));
    let mut a5 = header_a();
    a5[0x14] = 0x1c;
    variants.push(("A5 header size 28", a5));
    variants
}
/// §4.2 Fixture B: the 26-byte header followed by the chunk walk.
pub fn chunk_walk() -> Vec<u8> {
    [&header_a()[..26], &hex(include_str!("fixture-b.hex"))].concat()
}
/// §4.6 Fixture F subtitles.
pub fn subtitles() -> &'static [u8] {
    include_bytes!("fixture-f.sve")
}
/// §4.7 Fixture G fades.
pub fn fades() -> &'static [u8] {
    include_bytes!("fixture-g.cfg")
}
/// §4.8 Fixture H: the synthetic movie and its expected-hash table.
pub fn synthetic(movie: char) -> (&'static [u8], &'static str, &'static str) {
    match movie {
        'A' => (
            include_bytes!("../../../oracle/SPEC-012/synthetic/synthetic-A.mve"),
            include_str!("../../../oracle/SPEC-012/synthetic/synthetic-A.expected.tsv"),
            include_str!("../../../oracle/SPEC-012/synthetic/synthetic-A.frames.hex"),
        ),
        'B' => (
            include_bytes!("../../../oracle/SPEC-012/synthetic/synthetic-B.mve"),
            include_str!("../../../oracle/SPEC-012/synthetic/synthetic-B.expected.tsv"),
            include_str!("../../../oracle/SPEC-012/synthetic/synthetic-B.frames.hex"),
        ),
        _ => panic!("synthetic movies are A and B"),
    }
}
/// Decoded frames of a `*.frames.hex` file: one buffer per `frame` heading.
pub fn frames(text: &str) -> Vec<Vec<u8>> {
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        if line.starts_with("frame") {
            frames.push(Vec::new());
        } else {
            let row = frames.last_mut().expect("rows follow a frame heading");
            for index in (0..line.len()).step_by(2) {
                row.push(u8::from_str_radix(&line[index..index + 2], 16).unwrap());
            }
        }
    }
    frames
}
