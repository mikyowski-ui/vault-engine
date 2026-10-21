//! SPEC-008 §4.1-4.2 archives and LZSS, and §3.2 database precedence.
use crate::{
    DatArchive, DatProvider, Database, LocalDirProvider, Vfs, VfsError,
    dat::{DatTables, Storage, decode_blocks, decode_until},
};
use std::{io::Cursor, sync::Arc};

#[test]
fn spec008_final_block_truncation_and_bounded_reads() {
    // §2.3: the final block is limited by the remaining logical length, so
    // output past it is discarded (a literal, a match cut mid-way, or raw
    // bytes), while a final block that stops short still fails.
    assert_eq!(
        decode_blocks(&[0x00, 0x03, 0x00, 0x00, 0x00], 2).unwrap(),
        b"  "
    );
    assert_eq!(
        decode_blocks(&[0x00, 0x04, 0x01, 0x41, 0xee, 0xf2], 4).unwrap(),
        b"AAAA"
    );
    assert_eq!(
        decode_blocks(&[0x80, 0x03, 0x58, 0x59, 0x5a], 2).unwrap(),
        b"XY"
    );
    assert!(decode_blocks(&[0x80, 0x03, 0x58, 0x59, 0x5a], 4).is_err());
    // Bounded reads: an archive member over the cap is refused from its
    // table entry, before any payload is read.
    let bytes = raw_archive(&[(b"BIG.TXT", b"0123456789")]);
    let archive = Arc::new(DatArchive::parse(bytes).unwrap());
    let database = Database {
        loose: None,
        archive: Some(archive.clone()),
    };
    assert_eq!(database.read_bounded("big.txt", 10).unwrap(), b"0123456789");
    assert!(matches!(
        database.read_bounded("big.txt", 9),
        Err(VfsError::FileTooLarge { max_bytes: 9, .. })
    ));
    let provider = DatProvider::new(database, Database::default());
    assert!(matches!(
        provider.read_bounded("big.txt", 9),
        Err(VfsError::FileTooLarge { .. })
    ));
    assert_eq!(provider.read("big.txt").unwrap(), b"0123456789");
    // §3.2: only an absent loose file falls through to the archive; an
    // oversize loose copy is reported, not masked by the archive member.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.txt"), b"loose copy, longer").unwrap();
    let database = Database {
        loose: Some(Arc::new(LocalDirProvider::mount(dir.path()).unwrap())),
        archive: Some(archive),
    };
    assert_eq!(database.read("big.txt").unwrap(), b"loose copy, longer");
    assert!(matches!(
        database.read_bounded("big.txt", 10),
        Err(VfsError::FileTooLarge { max_bytes: 10, .. })
    ));
    assert!(matches!(
        database.read("missing.txt"),
        Err(VfsError::NotFound(_))
    ));
}

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
    // Truncated match token, non-final over-expansion, and zero-output blocks.
    assert!(decode_blocks(&[0x00, 0x02, 0x00, 0xee], 3).is_err());
    let mut over = vec![0xc0, 0x01];
    over.extend(std::iter::repeat_n(0x41, 16385));
    over.extend([0x80, 0x01, 0x42]);
    assert!(decode_blocks(&over, 16386).is_err());
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

/// A root-directory archive of raw (`0x20`) members, payloads after the table.
fn raw_archive(members: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [1_i32, 1, 0, 0] {
        bytes.extend(word.to_be_bytes());
    }
    bytes.extend([1, b'.']);
    for word in [members.len() as i32, members.len() as i32, 16, 0] {
        bytes.extend(word.to_be_bytes());
    }
    let table_end = bytes.len()
        + members
            .iter()
            .map(|(name, _)| 1 + name.len() + 16)
            .sum::<usize>();
    let mut offset = table_end as u32;
    for (name, body) in members {
        bytes.push(name.len() as u8);
        bytes.extend(*name);
        for word in [0x20_u32, offset, body.len() as u32, body.len() as u32] {
            bytes.extend(word.to_be_bytes());
        }
        offset += body.len() as u32;
    }
    for (_, body) in members {
        bytes.extend(*body);
    }
    bytes
}

/// A stream that counts the bytes read through it.
struct Counting {
    inner: Cursor<Vec<u8>>,
    read: Arc<std::sync::atomic::AtomicUsize>,
}
impl std::io::Read for Counting {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.read
            .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
        Ok(count)
    }
}
impl std::io::Seek for Counting {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn spec008_opened_archive_streams_members_matching_the_in_memory_archive() {
    let bytes = archive_bytes();
    let in_memory = DatArchive::parse(bytes.clone()).unwrap();
    assert!(!in_memory.is_streamed());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("SYNTH.DAT");
    std::fs::write(&path, &bytes).unwrap();
    let opened = DatArchive::open(&path).unwrap();
    assert!(opened.is_streamed());
    assert_eq!(
        (&opened.root, &opened.directories),
        (&in_memory.root, &in_memory.directories)
    );
    assert_eq!(
        opened.paths().collect::<Vec<_>>(),
        in_memory.paths().collect::<Vec<_>>()
    );
    for name in [&b"pack.txt"[..], b".\\Plain.Txt", b"PLAIN.TXT"] {
        let entry = opened.entry(name).unwrap();
        assert_eq!(opened.extract(entry), in_memory.extract(entry));
    }
    // Clones share the stream and read independently of order.
    let clone = opened.clone();
    assert_eq!(
        clone.extract(clone.entry(b"PLAIN.TXT").unwrap()).unwrap(),
        b"XYZ"
    );
    assert_eq!(
        opened.extract(opened.entry(b"PACK.TXT").unwrap()).unwrap(),
        b"ABCABCABC"
    );

    // The same malformed members fail streamed as in memory.
    let mut longer = bytes.clone();
    longer[0x33..0x37].copy_from_slice(&10_u32.to_be_bytes());
    let mut past = bytes.clone();
    past[0x2f..0x33].copy_from_slice(&0x200_u32.to_be_bytes());
    for broken in [longer, past] {
        let archive = DatArchive::from_reader(Cursor::new(broken)).unwrap();
        assert!(
            archive
                .extract(archive.entry(b"PACK.TXT").unwrap())
                .is_err()
        );
    }
    for end in 0..0x55 {
        assert!(
            DatArchive::from_reader(Cursor::new(bytes[..end].to_vec())).is_err(),
            "{end}"
        );
    }
    assert!(DatArchive::open(directory.path().join("MISSING.DAT")).is_err());
}

#[test]
fn spec008_opening_reads_only_tables_and_extraction_only_the_member() {
    let large = vec![0x5a; 1 << 20];
    let bytes = raw_archive(&[(b"LARGE.BIN", &large), (b"SMALL.TXT", b"small")]);
    let read = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let archive = DatArchive::from_reader(Counting {
        inner: Cursor::new(bytes),
        read: read.clone(),
    })
    .unwrap();
    let count = || read.load(std::sync::atomic::Ordering::Relaxed);
    // Only the (buffered) table reads happened: far less than the payloads.
    let after_tables = count();
    assert!(after_tables < 16 * 1024, "{after_tables}");
    let small = archive.entry(b"small.txt").unwrap();
    assert_eq!(archive.extract(small).unwrap(), b"small");
    assert_eq!(count() - after_tables, 5);
    let before = count();
    assert_eq!(
        archive
            .extract(archive.entry(b"LARGE.BIN").unwrap())
            .unwrap(),
        large
    );
    assert_eq!(count() - before, large.len());
}

#[test]
fn spec008_streamed_archive_serves_the_database_vfs() {
    let bytes = raw_archive(&[(b"SHARED.TXT", b"streamed")]);
    let archive = Arc::new(DatArchive::from_reader(Cursor::new(bytes)).unwrap());
    let provider = DatProvider::new(
        Database {
            loose: None,
            archive: Some(archive.clone()),
        },
        Database {
            loose: None,
            archive: Some(archive),
        },
    );
    assert_eq!(provider.read("shared.txt").unwrap(), b"streamed");
    assert_eq!(provider.read_critter("SHARED.TXT").unwrap(), b"streamed");
    assert!(provider.read("missing.txt").is_err());
    assert!(provider.files().any(|path| path.as_str() == "shared.txt"));
}

/// SPEC-008 §2.2-§2.3 over the real archives: every member of `MASTER.DAT`
/// and `CRITTER.DAT` extracts to exactly its logical length.
#[test]
#[ignore = "needs the local corpus"]
fn corpus_every_archive_member_extracts() {
    let Some(corpus) = crate::tests::corpus_dir() else {
        return;
    };
    for name in ["MASTER.DAT", "CRITTER.DAT"] {
        let archive = DatArchive::open(corpus.join(name)).unwrap();
        let mut members = 0;
        let mut failures = Vec::new();
        for directory in &archive.directories {
            for file in &directory.files {
                members += 1;
                let path = String::from_utf8_lossy(&directory.path(file)).into_owned();
                match archive.extract(file) {
                    Ok(bytes) if bytes.len() == file.length as usize => {}
                    Ok(bytes) => failures.push(format!(
                        "{path}: {} bytes, entry says {}",
                        bytes.len(),
                        file.length
                    )),
                    Err(error) => failures.push(format!("{path}: {error}")),
                }
            }
        }
        eprintln!("{name}: {members} members, {} failures", failures.len());
        for line in failures.iter().take(20) {
            eprintln!("  {line}");
        }
        assert!(
            failures.is_empty(),
            "{name}: {} members failed",
            failures.len()
        );
    }
}
