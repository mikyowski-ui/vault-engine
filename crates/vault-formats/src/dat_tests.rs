//! SPEC-008 §4.1-4.2 archives and LZSS, and §3.2 database precedence.
use crate::{
    DatArchive, DatProvider, Database, LocalDirProvider, Vfs,
    dat::{DatTables, Storage, decode_blocks, decode_until},
};
use std::{io::Cursor, sync::Arc};

fn archive_bytes() -> Vec<u8> {
    crate::fixture_assets::sparse(include_str!(
        "../../../tests/fixtures/archive/synthetic.dat.hex"
    ))
}

#[test]
fn spec008_fixture_complete_archive_tables_and_extraction() {
    let bytes = archive_bytes();
    assert_eq!(bytes.len(), 96);
    let archive = DatArchive::parse(bytes.clone()).unwrap();
    assert_eq!(
        (
            archive.root.count,
            archive.root.capacity,
            archive.root.data_size,
            archive.root.opaque
        ),
        (1, 1, 0, 0)
    );
    let directory = &archive.directories[0];
    assert_eq!(directory.name, b".");
    assert_eq!(
        (
            directory.prefix.count,
            directory.prefix.capacity,
            directory.prefix.data_size
        ),
        (2, 2, 16)
    );
    let pack = &directory.files[0];
    assert_eq!(
        (
            pack.name.as_slice(),
            pack.attributes,
            pack.offset,
            pack.length,
            pack.packed
        ),
        (b"PACK.TXT".as_slice(), 0x40, 0x55, 9, 8)
    );
    let plain = &directory.files[1];
    assert_eq!(
        (
            plain.name.as_slice(),
            plain.attributes,
            plain.offset,
            plain.length,
            plain.packed
        ),
        (b"PLAIN.TXT".as_slice(), 0x20, 0x5d, 3, 3)
    );
    // Case-insensitive lookup, bare names select directory index zero.
    let pack = archive.entry(b"pack.txt").unwrap();
    assert_eq!(archive.extract(pack).unwrap(), b"ABCABCABC");
    assert_eq!(
        archive
            .extract(archive.entry(b".\\Plain.Txt").unwrap())
            .unwrap(),
        b"XYZ"
    );
    assert!(archive.entry(b"missing.txt").is_none());

    // Logical length 10 without more packed data fails safe validation.
    let mut longer = bytes.clone();
    // Entry fields begin at 0x2B: attributes, offset 0x2F, length 0x33, packed 0x37.
    longer[0x33..0x37].copy_from_slice(&10_u32.to_be_bytes());
    let archive = DatArchive::parse(longer).unwrap();
    assert!(
        archive
            .extract(archive.entry(b"PACK.TXT").unwrap())
            .is_err()
    );
    // A payload offset past EOF is rejected rather than read.
    let mut past = bytes;
    past[0x2f..0x33].copy_from_slice(&0x200_u32.to_be_bytes());
    let archive = DatArchive::parse(past).unwrap();
    assert!(
        archive
            .extract(archive.entry(b"PACK.TXT").unwrap())
            .is_err()
    );
    // Truncated tables are parse errors.
    for end in 0..0x55 {
        assert!(
            DatArchive::parse(archive_bytes()[..end].to_vec()).is_err(),
            "{end}"
        );
    }
}

#[test]
fn spec008_streamed_tables_and_members_match_the_in_memory_archive() {
    let bytes = archive_bytes();
    let archive = DatArchive::parse(bytes.clone()).unwrap();
    // The tables end where the first payload begins; nothing past them is read.
    let mut stream = Cursor::new(bytes.clone());
    let tables = DatTables::read(&mut stream).unwrap();
    assert_eq!(stream.position(), 0x55);
    assert_eq!(
        (&tables.root, &tables.directories),
        (&archive.root, &archive.directories)
    );
    let directory = &tables.directories[0];
    let paths: Vec<_> = directory.files.iter().map(|f| directory.path(f)).collect();
    assert_eq!(paths, archive.paths().collect::<Vec<_>>());
    // Members are read in any order by seeking, one at a time.
    for file in directory.files.iter().rev() {
        assert_eq!(file.read_from(&mut stream), archive.extract(file));
    }
    assert_eq!(
        directory.files[0].read_from(&mut stream).unwrap(),
        b"ABCABCABC"
    );
    assert_eq!(directory.files[1].stored_len().unwrap(), 3);
    // A member extending past the stream is rejected rather than padded.
    let mut short = Cursor::new(bytes[..bytes.len() - 1].to_vec());
    assert!(directory.files[1].read_from(&mut short).is_err());
    let mut past = directory.files[0].clone();
    past.offset = 0x200;
    assert!(past.read_from(&mut stream).is_err());
    past.attributes = 0x30;
    assert!(past.stored_len().is_err());
    // Truncated tables fail the same way streamed.
    for end in 0..0x55 {
        assert!(DatTables::read(&bytes[..end]).is_err(), "{end}");
    }
}

#[test]
fn spec008_fixture_lzss_outer_blocks_and_dictionary_reset() {
    for (block, output) in [
        (&[0x80, 0x03, 0x58, 0x59, 0x5a][..], b"XYZ".to_vec()),
        (&[0x00, 0x03, 0x00, 0x00, 0x00][..], b"   ".to_vec()),
        (
            &[0x00, 0x04, 0x01, 0x41, 0xee, 0xf2][..],
            b"AAAAAA".to_vec(),
        ),
        (
            &[0x00, 0x06, 0x07, 0x41, 0x42, 0x43, 0xee, 0xf3][..],
            b"ABCABCABC".to_vec(),
        ),
    ] {
        assert_eq!(decode_blocks(block, output.len()).unwrap(), output);
    }
    // A raw 16384-byte block, then a compressed block: the raw block does not
    // seed the dictionary, so the match reads the reset spaces.
    let mut payload = vec![0xc0, 0x00];
    payload.extend(std::iter::repeat_n(0x41, 16384));
    payload.extend([0x00, 0x03, 0x00, 0x00, 0x00]);
    assert_eq!(payload.len(), 16391);
    let decoded = decode_blocks(&payload, 16387).unwrap();
    assert!(decoded[..16384].iter().all(|byte| *byte == b'A'));
    assert_eq!(&decoded[16384..], b"   ");
    // A compressed block that writes the ring tail, then a block whose first
    // match reads the tail before writing it: the safe rule zeroes the tail
    // at every compressed block start (the reference would read "AAA").
    let mut payload = vec![0x48, 0x00];
    for _ in 0..2048 {
        payload.push(0xff);
        payload.extend([0x41; 8]);
    }
    payload.extend([0x00, 0x03, 0x00, 0xee, 0xf0]);
    let decoded = decode_blocks(&payload, 16387).unwrap();
    assert!(decoded[..16384].iter().all(|byte| *byte == b'A'));
    assert_eq!(&decoded[16384..], [0, 0, 0]);
    // A short non-final block is not a valid streaming layout.
    let mut short = vec![0x80, 0x03, 0x58, 0x59, 0x5a];
    short.extend([0x80, 0x03, 0x58, 0x59, 0x5a]);
    assert!(decode_blocks(&short, 6).is_err());
    // Truncated match token, over-expansion, and zero-output blocks.
    assert!(decode_blocks(&[0x00, 0x02, 0x00, 0xee], 3).is_err());
    assert!(decode_blocks(&[0x00, 0x03, 0x00, 0x00, 0x00], 2).is_err());
    assert!(decode_blocks(&[0x00, 0x00], 3).is_err());
    assert!(decode_blocks(&[0x80, 0x00], 1).is_err());
    // Attribute dispatch: exact zero aliases legacy 0x10; low bits do not
    // change a selected mode; low-bits-only words are not the alias.
    assert_eq!(Storage::from_attributes(0), Some(Storage::Legacy));
    assert_eq!(Storage::from_attributes(0x10), Some(Storage::Legacy));
    assert_eq!(Storage::from_attributes(0x2f), Some(Storage::Raw));
    assert_eq!(Storage::from_attributes(0x41), Some(Storage::Blocks));
    assert_eq!(Storage::from_attributes(0x01), None);
    // The automap variant stops on an exact output count, truncating a match.
    assert_eq!(decode_until(&[0x01, 0x41, 0xee, 0xf2], 4).unwrap(), b"AAAA");
    assert!(decode_until(&[0x01, 0x41], 2).is_err());
}

#[test]
fn spec008_selected_database_precedence_has_no_cross_archive_fallback() {
    let archive = |name: &[u8], body: &[u8]| {
        // One root directory `.` holding one raw file.
        let mut bytes = Vec::new();
        for word in [1_i32, 1, 0, 0] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend([1, b'.']);
        for word in [1_i32, 1, 16, 0] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.push(name.len() as u8);
        bytes.extend(name);
        let offset = bytes.len() as u32 + 16;
        for word in [0x20_u32, offset, body.len() as u32, body.len() as u32] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(body);
        Arc::new(DatArchive::parse(bytes).unwrap())
    };
    let master_root = tempfile::tempdir().unwrap();
    std::fs::write(master_root.path().join("shared.txt"), b"master loose").unwrap();
    let critter_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(critter_root.path().join("art/critters")).unwrap();
    std::fs::write(
        critter_root.path().join("art/critters/loose.frm"),
        b"critter loose",
    )
    .unwrap();
    let provider = DatProvider::new(
        Database {
            loose: Some(Arc::new(
                LocalDirProvider::mount(master_root.path()).unwrap(),
            )),
            archive: Some(archive(b"SHARED.TXT", b"master archive")),
        },
        Database {
            loose: Some(Arc::new(
                LocalDirProvider::mount(critter_root.path()).unwrap(),
            )),
            archive: Some(archive(b"ONLY.TXT", b"critter archive")),
        },
    );
    // The selected database's loose root precedes its archive.
    assert_eq!(provider.read("SHARED.TXT").unwrap(), b"master loose");
    std::fs::remove_file(master_root.path().join("shared.txt")).unwrap();
    assert_eq!(provider.read("shared.txt").unwrap(), b"master archive");
    // A member present only in the unselected archive is not a fallback.
    assert!(provider.read("only.txt").is_err());
    assert_eq!(
        provider.read_critter("only.txt").unwrap(),
        b"critter archive"
    );
    // Critter art selects the critter database for that request.
    assert_eq!(
        provider.read("art/critters/loose.frm").unwrap(),
        b"critter loose"
    );
    assert!(provider.files().any(|path| path.as_str() == "only.txt"));
}
