//! SPEC-015 §2-§6 over synthetic version 2 archives.
use super::*;
use vault_fixtures::dat2::{Member, archive, zlib_vector};

/// A stored text, a compressed text, two compressed members in a folder
/// (one written with forward slashes) and the multi-block stored vector.
fn sample() -> Vec<u8> {
    archive(&[
        Member::stored("COLOR.PAL", b"palette"),
        Member::zlib("TEXT\\ENGLISH\\GAME\\NOTE.MSG", &zlib_vector("text")),
        Member::zlib("data/words.txt", &zlib_vector("words")),
        Member::zlib("DATA\\REPEAT.BIN", &zlib_vector("repeat")),
        Member::zlib("art\\noise.frm", &zlib_vector("stored")),
    ])
}

/// Where the tree starts in an archive's bytes.
fn tree_start(bytes: &[u8]) -> usize {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let end = bytes.len() - 8;
    end - word(end)
}

#[test]
fn members_read_back_in_memory_and_streamed() {
    let bytes = sample();
    let memory = Dat2Archive::parse(bytes.clone()).unwrap();
    let streamed = Dat2Archive::from_reader(std::io::Cursor::new(bytes)).unwrap();
    for archive in [&memory, &streamed] {
        let paths: Vec<&str> = archive.files().map(VfsPath::as_str).collect();
        assert_eq!(
            paths,
            [
                "art/noise.frm",
                "color.pal",
                "data/repeat.bin",
                "data/words.txt",
                "text/english/game/note.msg"
            ]
        );
        assert_eq!(archive.read("color.pal").unwrap(), b"palette");
        assert_eq!(
            archive.read("text/english/game/note.msg").unwrap(),
            zlib_vector("text").plain
        );
        assert_eq!(
            archive.read("DATA/WORDS.TXT").unwrap(),
            zlib_vector("words").plain
        );
        assert_eq!(
            archive.read("data/repeat.bin").unwrap(),
            zlib_vector("repeat").plain
        );
        assert_eq!(
            archive.read("art/noise.frm").unwrap(),
            zlib_vector("stored").plain
        );
        // Lookups take either slash and any case.
        let entry = archive.entry("Data\\Words.TXT").unwrap();
        assert_eq!(entry.stored_name, b"data/words.txt");
        assert!(entry.compressed);
        assert!(matches!(
            archive.read("missing.txt"),
            Err(VfsError::NotFound(_))
        ));
        assert!(matches!(
            archive.read_bounded("data/words.txt", 10),
            Err(VfsError::FileTooLarge { .. })
        ));
    }
    assert_eq!(memory.entries().len(), 5);
    // A file on disk opens through the streamed path.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("MASTER.DAT");
    std::fs::write(&path, sample()).unwrap();
    let opened = Dat2Archive::open(&path).unwrap();
    assert_eq!(opened.read("color.pal").unwrap(), b"palette");
    // An empty archive is valid.
    assert_eq!(Dat2Archive::parse(archive(&[])).unwrap().files().count(), 0);
}

#[test]
fn the_footer_and_tree_are_checked_whole() {
    let refuse = |bytes: Vec<u8>, needle: &str| {
        let message = Dat2Archive::parse(bytes.clone()).unwrap_err().to_string();
        assert!(message.contains(needle), "{needle}: {message}");
        // The streamed reader refuses it alike.
        let message = Dat2Archive::from_reader(std::io::Cursor::new(bytes))
            .unwrap_err()
            .to_string();
        assert!(message.contains(needle), "{needle} (streamed): {message}");
    };
    let good = sample();
    let end = good.len() - 8;
    let start = tree_start(&good);
    let set = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    refuse(vec![0; 7], "8-byte footer");
    // A file size that is not the length (an appended byte).
    let mut longer = good.clone();
    longer.push(0);
    refuse(longer, "is not the length");
    // A tree larger than the archive.
    let mut huge = good.clone();
    set(&mut huge, end, good.len() as u32);
    refuse(huge, "does not fit");
    // A count the tree cannot hold.
    let mut count = good.clone();
    set(&mut count, start, 10_000);
    refuse(count, "cannot fit the tree");
    // Bytes left after the last entry: one entry fewer than stored.
    let mut fewer = good.clone();
    set(&mut fewer, start, 4);
    refuse(fewer, "after the last entry");
    // The first entry's fields: its path is 9 bytes ("COLOR.PAL").
    let flags = start + 4 + 4 + 9;
    let mut flagged = good.clone();
    flagged[flags] = 2;
    refuse(flagged, "flags 2");
    let mut sizes = good.clone();
    set(&mut sizes, flags + 1, 8);
    refuse(sizes, "two different sizes");
    let mut past = good.clone();
    set(&mut past, flags + 9, start as u32);
    refuse(past, "past the member data");
    let mut control = good.clone();
    control[start + 8] = 0x07;
    refuse(control, "printable ASCII");
    let mut empty_path = archive(&[Member::stored("A", b"x")]);
    let at = tree_start(&empty_path) + 4;
    set(&mut empty_path, at, 0);
    refuse(empty_path, "a path of 0 bytes");
    // Paths with no canonical form, or two that fold to one.
    refuse(archive(&[Member::stored("..\\escape.txt", b"x")]), "escape");
    refuse(
        archive(&[
            Member::stored("Data\\A.TXT", b"x"),
            Member::stored("data/a.txt", b"y"),
        ]),
        "the same path",
    );
}

#[test]
fn a_damaged_member_fails_its_read_only() {
    let mut text = zlib_vector("text").stream;
    let last = text.len() - 1;
    text[last] ^= 0xff;
    let bytes = archive(&[
        Member::stored("good.txt", b"fine"),
        Member {
            path: "bad.msg".into(),
            stored: text,
            compressed: true,
            real_size: zlib_vector("text").plain.len() as u32,
        },
        Member {
            path: "huge.bin".into(),
            stored: vec![0; 4],
            compressed: true,
            real_size: (MAX_ASSET_BYTES + 1) as u32,
        },
    ]);
    let archive = Dat2Archive::parse(bytes).unwrap();
    assert_eq!(archive.read("good.txt").unwrap(), b"fine");
    let message = archive.read("bad.msg").unwrap_err().to_string();
    assert!(
        message.contains("bad.msg") && message.contains("Adler-32"),
        "{message}"
    );
    let message = archive.read("huge.bin").unwrap_err().to_string();
    assert!(message.contains("64 MiB"), "{message}");
}

#[test]
fn the_fallout2_databases_layer_loose_patch_master_and_critter() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, members: &[Member]| {
        let path = dir.path().join(name);
        std::fs::write(&path, archive(members)).unwrap();
        Arc::new(Dat2Archive::open(&path).unwrap())
    };
    let master = write(
        "MASTER.DAT",
        &[
            Member::stored("COLOR.PAL", b"master palette"),
            Member::stored("DATA\\ONLY.TXT", b"master only"),
            Member::stored("ART\\CRITTERS\\WRONG.FRM", b"never read here"),
        ],
    );
    let patch = write(
        "Patch000.DAT",
        &[Member::stored("color.pal", b"patched palette")],
    );
    let critter = write(
        "CRITTER.DAT",
        &[Member::stored("ART\\CRITTERS\\HERO.FRM", b"hero")],
    );
    std::fs::create_dir_all(dir.path().join("data/data")).unwrap();
    std::fs::write(dir.path().join("data/data/only.txt"), b"loose").unwrap();
    // patch000.dat is found beside MASTER.DAT whatever its case.
    assert_eq!(
        Dat2Provider::find_patch(&dir.path().join("MASTER.DAT")),
        Some(dir.path().join("Patch000.DAT"))
    );
    assert_eq!(
        Dat2Provider::find_patch(&dir.path().join("data/x.dat")),
        None
    );
    let loose = Arc::new(crate::LocalDirProvider::mount(dir.path().join("data")).unwrap());
    let provider = Dat2Provider::new(
        Some(loose),
        vec![master.clone(), patch],
        Some(critter.clone()),
    );
    // The patch wins over the master, and the loose folder over both.
    assert_eq!(provider.read("color.pal").unwrap(), b"patched palette");
    assert_eq!(provider.read("data/only.txt").unwrap(), b"loose");
    // Critter art reads the critter archive alone, with no fallback.
    assert_eq!(provider.read("art/critters/hero.frm").unwrap(), b"hero");
    assert!(matches!(
        provider.read("art/critters/wrong.frm"),
        Err(VfsError::NotFound(_))
    ));
    // Without a patch the master answers.
    let plain = Dat2Provider::new(None, vec![master], Some(critter));
    assert_eq!(plain.read("color.pal").unwrap(), b"master palette");
    assert!(
        plain
            .files()
            .any(|path| path.as_str() == "art/critters/hero.frm")
    );
}
