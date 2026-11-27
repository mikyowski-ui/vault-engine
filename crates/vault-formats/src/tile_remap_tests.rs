//! `docs/extended-art-namespaces.md` §3.5 and vector E: the `VTLX` sidecar.
use super::*;

/// Vector E's sidecar: local tile index 4000 maps to row 5000.
const VECTOR_E: [u8; 14] = [
    0x56, 0x54, 0x4C, 0x58, 0x00, 0x01, 0x00, 0x01, 0x0F, 0xA0, 0x00, 0x00, 0x13, 0x88,
];

#[test]
fn vector_e_sidecar_parses_and_maps_both_lookups() {
    let remap = TileRemap::parse(&VECTOR_E).unwrap();
    assert_eq!(remap.entries(), [(4000, 5000)]);
    assert_eq!(remap.len(), 1);
    assert!(!remap.is_empty());
    // A floor word 0x0FA0 draws row 5000; 0x0FA1 draws its own index.
    assert_eq!(remap.row(0x0FA0), 5000);
    assert_eq!(remap.row(0x0FA1), 4001);
    assert_eq!(remap.row(1), 1, "the blank tile stays itself");
    assert_eq!(remap.row(0), 0);
    assert_eq!(remap.to_bytes(), VECTOR_E);
    assert_eq!(TileRemap::from_pairs([(4000, 5000)]).unwrap(), remap);
}

#[test]
fn tile_remaps_round_trip_and_the_empty_remap_is_the_identity() {
    let empty = TileRemap::parse(b"VTLX\x00\x01\x00\x00").unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty, TileRemap::default());
    assert_eq!(empty.to_bytes(), b"VTLX\x00\x01\x00\x00");
    for local in [0, 1, 2, 4095] {
        assert_eq!(empty.row(local), u32::from(local));
    }
    let pairs = [(0, 7), (2, 4096), (3, 0), (4094, MAX_ROW), (4095, 12)];
    let remap = TileRemap::from_pairs(pairs).unwrap();
    assert_eq!(TileRemap::parse(&remap.to_bytes()).unwrap(), remap);
    for (local, row) in pairs {
        assert_eq!(remap.row(local), row);
    }
    assert_eq!(remap.row(1), 1);
    assert_eq!(remap.row(5), 5);
    // The most entries: every local index but the blank tile, 4095 of them.
    let full = TileRemap::from_pairs(
        (0..=MAX_LOCAL)
            .filter(|local| *local != BLANK_TILE)
            .map(|local| (local, u32::from(local) + 5000)),
    )
    .unwrap();
    assert_eq!(full.len(), 4095);
    assert_eq!(TileRemap::parse(&full.to_bytes()).unwrap(), full);
}

#[test]
fn tile_remaps_refuse_every_layout_deviation() {
    let entry = |local: u16, row: u32| {
        let mut bytes = local.to_be_bytes().to_vec();
        bytes.extend_from_slice(&row.to_be_bytes());
        bytes
    };
    let sidecar = |count: u16, entries: &[(u16, u32)]| {
        let mut bytes = b"VTLX\x00\x01".to_vec();
        bytes.extend_from_slice(&count.to_be_bytes());
        for (local, row) in entries {
            bytes.extend(entry(*local, *row));
        }
        bytes
    };
    let mut trailing = VECTOR_E.to_vec();
    trailing.push(0);
    let mut wrong_magic = VECTOR_E.to_vec();
    wrong_magic[3] = b'Y';
    let mut version_two = VECTOR_E.to_vec();
    version_two[5] = 2;
    let mut version_zero = VECTOR_E.to_vec();
    version_zero[5] = 0;
    for (name, bytes, needle) in [
        ("empty", Vec::new(), "header"),
        ("short header", b"VTLX\x00\x01\x00".to_vec(), "header"),
        ("magic", wrong_magic, "magic"),
        ("version 2", version_two, "version 2"),
        ("version 0", version_zero, "version 0"),
        ("truncated", VECTOR_E[..13].to_vec(), "need 14 bytes"),
        (
            "count past the entries",
            sidecar(2, &[(4000, 5000)]),
            "need 20",
        ),
        ("trailing", trailing, "trailing"),
        ("count above 4096", sidecar(4097, &[]), "more than 4096"),
        ("blank tile", sidecar(1, &[(1, 5000)]), "blank tile"),
        (
            "local 4096",
            sidecar(1, &[(4096, 5000)]),
            "outside 0..=4095",
        ),
        ("local 0xffff", sidecar(1, &[(0xffff, 5000)]), "outside"),
        ("row", sidecar(1, &[(4000, 0x0100_0000)]), "above 16777215"),
        (
            "repeat",
            sidecar(2, &[(4000, 5000), (4000, 5001)]),
            "does not ascend",
        ),
        (
            "descending",
            sidecar(2, &[(4000, 5000), (3000, 5001)]),
            "does not ascend",
        ),
    ] {
        let error = TileRemap::parse(&bytes).unwrap_err();
        assert_eq!(error.format, "TLX", "{name}");
        assert!(error.message.contains(needle), "{name}: {}", error.message);
    }
    // The second entry's offset is named.
    let error = TileRemap::parse(&sidecar(2, &[(4000, 5000), (4000, 5001)])).unwrap_err();
    assert_eq!(error.offset, 14);
    // The pair constructor refuses the same.
    for pairs in [
        vec![(1, 5)],
        vec![(4096, 5)],
        vec![(2, 0x0100_0000)],
        vec![(3, 1), (2, 1)],
        vec![(3, 1), (3, 2)],
        (0..=4096).map(|local| (local, 0)).collect(),
    ] {
        assert!(TileRemap::from_pairs(pairs.clone()).is_err(), "{pairs:?}");
    }
}
