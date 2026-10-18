/// SPEC-010 section 4.1-4.2 ACM vectors A-H as complete files.
pub fn acm(vector: char) -> Vec<u8> {
    let text = match vector {
        'A' => include_str!("synthetic-A.acm.hex"),
        'B' => include_str!("synthetic-B.acm.hex"),
        'C' => include_str!("synthetic-C.acm.hex"),
        'D' => include_str!("synthetic-D.acm.hex"),
        'E' => include_str!("synthetic-E.acm.hex"),
        'F' => include_str!("synthetic-F.acm.hex"),
        'G' => include_str!("synthetic-G.acm.hex"),
        'H' => include_str!("synthetic-H.acm.hex"),
        _ => panic!("ACM vectors are A-H"),
    };
    let mut bytes = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (address, payload) = line.split_once(':').unwrap();
        assert_eq!(usize::from_str_radix(address, 16).unwrap(), bytes.len());
        bytes.extend(
            payload
                .split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap()),
        );
    }
    bytes
}
