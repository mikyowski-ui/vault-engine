//! SPEC-023 T1-T7 on the vectors in `tests/fixtures/spec023/`. With
//! `host-fs` on, every archive result is also compared with a file holding
//! the same bytes; `cargo test -p vault-formats --no-default-features` runs
//! the rest with no file-backed code compiled (T7).
use super::*;
use crate::GameProfile::{Fallout1, Fallout2};
use std::collections::BTreeSet;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../../tests/fixtures/spec023/", $name))
    };
}

fn hex(text: &str) -> Vec<u8> {
    text.lines()
        .filter_map(|line| line.split_once(':'))
        .flat_map(|(_, bytes)| bytes.split_whitespace())
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

fn rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect()
}

fn source(bytes: impl Into<Vec<u8>>) -> ByteSource {
    ByteSource::new(bytes.into()).unwrap()
}

fn f1_master() -> Vec<u8> {
    hex(fixture!("f1-master.dat.hex"))
}
fn f1_critter() -> Vec<u8> {
    hex(fixture!("f1-critter.dat.hex"))
}
fn f2_master() -> Vec<u8> {
    hex(fixture!("f2-master.dat.hex"))
}
fn f2_patch() -> Vec<u8> {
    hex(fixture!("f2-patch.dat.hex"))
}
fn f2_critter() -> Vec<u8> {
    hex(fixture!("f2-critter.dat.hex"))
}

fn loose(text: &str) -> MemoryDir {
    MemoryDir::new(rows(text).iter().map(|row| (row[0], source(row[1])))).unwrap()
}

/// One of the mounts of SPEC-023 §5.3.
fn mount(name: &str) -> MemoryMount {
    match name {
        "f1" => MemoryMount {
            profile: Fallout1,
            master: Some(source(f1_master())),
            critter: Some(source(f1_critter())),
            loose: Some(loose(fixture!("f1-loose.tsv"))),
            ..Default::default()
        },
        "f2" => MemoryMount {
            patch: Some(source(f2_patch())),
            ..mount("f2-nopatch")
        },
        "f2-nopatch" => MemoryMount {
            profile: Fallout2,
            master: Some(source(f2_master())),
            critter: Some(source(f2_critter())),
            loose: Some(loose(fixture!("f2-loose.tsv"))),
            ..Default::default()
        },
        other => panic!("no mount {other}"),
    }
}

/// A byte's stored form in `paths()`, as text.
fn stored(path: Vec<u8>) -> String {
    String::from_utf8(path).unwrap()
}

/// Every member of a version 1 archive: its stored path and the result of
/// reading it, as text so that a refusal compares too.
fn v1_members(archive: &DatArchive) -> Vec<(String, Result<Vec<u8>, String>)> {
    archive
        .paths()
        .map(|path| {
            let entry = archive.entry(&path).unwrap();
            let read = archive.extract(entry).map_err(|e| e.to_string());
            (stored(path), read)
        })
        .collect()
}

fn v2_members(archive: &Dat2Archive) -> Vec<(String, Result<Vec<u8>, String>)> {
    archive
        .entries()
        .iter()
        .map(|entry| {
            let read = archive.extract(entry).map_err(|e| e.to_string());
            (entry.path.as_str().to_owned(), read)
        })
        .collect()
}

/// The same bytes opened from a file: the same refusal, or the same list in
/// the same order and the same result for every member (§3.1 rule 2).
#[cfg(feature = "host-fs")]
fn file_parity(bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.dat");
    std::fs::write(&path, bytes).unwrap();
    match (
        DatArchive::from_bytes(source(bytes)),
        DatArchive::open(&path),
    ) {
        (Ok(memory), Ok(file)) => {
            assert!(!memory.is_streamed() && file.is_streamed());
            assert_eq!(v1_members(&memory), v1_members(&file));
        }
        (Err(memory), Err(file)) => assert_eq!(memory.to_string(), file.to_string()),
        (memory, file) => panic!("{:?} and {:?}", memory.is_ok(), file.is_ok()),
    }
    match (
        Dat2Archive::from_bytes(source(bytes)),
        Dat2Archive::open(&path),
    ) {
        (Ok(memory), Ok(file)) => assert_eq!(v2_members(&memory), v2_members(&file)),
        (Err(memory), Err(file)) => assert_eq!(memory.to_string(), file.to_string()),
        (memory, file) => panic!("{:?} and {:?}", memory.is_ok(), file.is_ok()),
    }
}
#[cfg(not(feature = "host-fs"))]
fn file_parity(_: &[u8]) {}

fn ok(bytes: &[u8]) -> Result<Vec<u8>, String> {
    Ok(bytes.to_vec())
}

#[test]
fn t1_version_1_archives_open_from_byte_sources() {
    let (master, critter) = (f1_master(), f1_critter());
    assert_eq!((master.len(), critter.len()), (217, 132));
    assert_eq!(
        v1_members(&DatArchive::from_bytes(source(master.clone())).unwrap()),
        [
            ("README.TXT".to_owned(), ok(b"MASTER1")),
            ("ART\\CRITTERS\\ONLYM.FRM".to_owned(), ok(b"MFRM")),
            ("DATA\\PACK.TXT".to_owned(), ok(b"ABCABCABC")),
            ("DATA\\SHARED.TXT".to_owned(), ok(b"M1-SHARED")),
        ]
    );
    assert_eq!(
        v1_members(&DatArchive::from_bytes(source(critter.clone())).unwrap()),
        [
            ("ART\\CRITTERS\\TEST.FRM".to_owned(), ok(b"C1FRM")),
            ("DATA\\SHARED.TXT".to_owned(), ok(b"C1-SHARED")),
        ]
    );
    file_parity(&master);
    file_parity(&critter);
}

#[test]
fn t2_version_2_archives_open_from_byte_sources() {
    let (master, patch, critter) = (f2_master(), f2_patch(), f2_critter());
    assert_eq!((master.len(), patch.len(), critter.len()), (142, 94, 55));
    assert_eq!(
        v2_members(&Dat2Archive::from_bytes(source(master.clone())).unwrap()),
        [
            ("art/critters/onlym.frm".to_owned(), ok(b"M2FRM")),
            ("data/shared.txt".to_owned(), ok(b"M2-SHARED")),
            ("data/zip.txt".to_owned(), ok(b"HELLO")),
        ]
    );
    assert_eq!(
        v2_members(&Dat2Archive::from_bytes(source(patch.clone())).unwrap()),
        [
            ("data/patchonly.txt".to_owned(), ok(b"P2ONLY")),
            ("data/shared.txt".to_owned(), ok(b"P2-SHARED")),
        ]
    );
    assert_eq!(
        v2_members(&Dat2Archive::from_bytes(source(critter.clone())).unwrap()),
        [("art/critters/test.frm".to_owned(), ok(b"C2FRM"))]
    );
    for bytes in [&master, &patch, &critter] {
        file_parity(bytes);
    }
}

#[test]
fn t3_every_lookup_gives_its_bytes_and_part() {
    for row in rows(fixture!("lookups.tsv")) {
        let [name, request, result, layer] = row[..] else {
            panic!("{row:?}");
        };
        let provider = mount(name).mount().unwrap();
        let context = format!("{name} {request}");
        match provider.read_served(request, MAX_ASSET_BYTES) {
            Ok((bytes, part)) => {
                assert_eq!(bytes, result.as_bytes(), "{context}");
                assert_eq!(part.label(), layer, "{context}");
                assert_eq!(provider.read(request).unwrap(), bytes, "{context}");
                // Base data has no mod layer's name, as for a file mount.
                let (_, named) = provider.read_with_layer(request, MAX_ASSET_BYTES).unwrap();
                assert_eq!(named, None, "{context}");
            }
            Err(VfsError::NotFound(_)) => {
                assert_eq!((result, layer), ("NOT_FOUND", "-"), "{context}");
                assert!(provider.read(request).is_err(), "{context}");
            }
            Err(error) => panic!("{context}: {error}"),
        }
    }
}

#[test]
fn a_mount_lists_the_union_of_its_parts_in_canonical_order() {
    let listed = |name: &str| {
        let provider = mount(name).mount().unwrap();
        provider
            .files()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        listed("f2"),
        [
            "art/critters/onlym.frm",
            "art/critters/test.frm",
            "data/patchonly.txt",
            "data/shared.txt",
            "data/zip.txt",
            "text/note.txt",
        ]
    );
    assert_eq!(
        listed("f1"),
        [
            "art/critters/loose.frm",
            "art/critters/onlym.frm",
            "art/critters/test.frm",
            "data/pack.txt",
            "data/shared.txt",
            "maps/note.txt",
            "readme.txt",
        ]
    );
}

/// `f2-master byte 0x8A: 8E -> 8F` or `f2-master cut to its first 7 bytes`,
/// applied to the bytes it names, checking the byte it says is there.
fn mutate(mutation: &str, bytes: &mut Vec<u8>) {
    let (_, change) = mutation.split_once(' ').unwrap();
    if let Some(rest) = change.strip_prefix("byte 0x") {
        let (offset, values) = rest.split_once(": ").unwrap();
        let (old, new) = values.split_once(" -> ").unwrap();
        let offset = usize::from_str_radix(offset, 16).unwrap();
        assert_eq!(bytes[offset], u8::from_str_radix(old, 16).unwrap());
        bytes[offset] = u8::from_str_radix(new, 16).unwrap();
    } else {
        let count = change
            .strip_prefix("cut to its first ")
            .and_then(|rest| rest.strip_suffix(" bytes"))
            .unwrap();
        bytes.truncate(count.parse().unwrap());
    }
}

/// `caf\xC3\xA9.txt` as the text it stands for.
fn unescape(text: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once("\\x") {
        bytes.extend_from_slice(head.as_bytes());
        bytes.push(u8::from_str_radix(&tail[..2], 16).unwrap());
        rest = &tail[2..];
    }
    bytes.extend_from_slice(rest.as_bytes());
    String::from_utf8(bytes).unwrap()
}

#[test]
fn t4_every_refusal_is_refused_at_its_stage() {
    let cases = rows(fixture!("refusals.tsv"));
    assert_eq!(cases.len(), 13);
    for row in cases {
        let [case, profile, input, mutation, expected, rule] = row[..] else {
            panic!("{row:?}");
        };
        let context = format!("case {case}: {rule}");
        if let Some(entries) = input.strip_prefix("loose entr") {
            let entries = entries.trim_start_matches("ies ").trim_start_matches("y ");
            let trees: Vec<(String, ByteSource)> = entries
                .split(" and ")
                .map(|path| (unescape(path), source(*b"x")))
                .collect();
            assert_eq!(expected, "mount_refused", "{context}");
            assert!(MemoryDir::new(trees.clone()).is_err(), "{context}");
            // The input's order never changes the refusal.
            let reversed = trees.into_iter().rev();
            assert!(MemoryDir::new(reversed).is_err(), "{context}");
            continue;
        }
        let f2 = profile == "fallout2";
        let mut bytes = match (f2, input.contains("master = ")) {
            (true, false) => f2_master(),
            (true, true) => f1_master(),
            (false, false) => f1_master(),
            (false, true) => f2_master(),
        };
        if mutation != "-" {
            mutate(mutation, &mut bytes);
        }
        file_parity(&bytes);
        let refused = MemoryMount {
            master: Some(source(bytes)),
            ..mount(if f2 { "f2" } else { "f1" })
        }
        .mount();
        if expected == "open_refused" {
            let Err(VfsError::Archive(message)) = refused else {
                panic!("{context}: not refused");
            };
            assert!(message.starts_with("master.dat: "), "{context}: {message}");
            if input.contains("master = ") {
                let game = if f2 {
                    "Fallout 1 data"
                } else {
                    "Fallout 2 data"
                };
                assert!(message.contains(game), "{context}: {message}");
            }
            continue;
        }
        let path = expected.rsplit(' ').next().unwrap();
        let provider = refused.unwrap_or_else(|error| panic!("{context}: {error}"));
        assert!(
            matches!(provider.read(path), Err(VfsError::Archive(_))),
            "{context}"
        );
    }
}

#[test]
fn t5_the_older_archive_fixtures_read_the_same_from_bytes() {
    use vault_fixtures::dat2::{Member, archive, zlib_vectors};
    let dump = include_str!("../../../../tests/fixtures/archive/synthetic.dat.hex");
    let synthetic = hex(dump);
    assert_eq!(synthetic.len(), 96);
    let archived = DatArchive::parse(synthetic.clone()).unwrap();
    let from_bytes = DatArchive::from_bytes(source(synthetic.clone())).unwrap();
    assert_eq!(v1_members(&archived), v1_members(&from_bytes));
    file_parity(&synthetic);
    for bytes in [
        vault_fixtures::dat::master_archive(),
        vault_fixtures::dat::critter_archive(),
    ] {
        file_parity(&bytes);
    }
    let vectors = zlib_vectors();
    assert!(!vectors.is_empty());
    let names: Vec<String> = vectors
        .iter()
        .map(|vector| format!("data/{}.bin", vector.name))
        .collect();
    let members: Vec<Member> = vectors
        .iter()
        .zip(&names)
        .map(|(vector, name)| Member::zlib(name, vector))
        .collect();
    let bytes = archive(&members);
    let opened = Dat2Archive::from_bytes(source(bytes.clone())).unwrap();
    for (vector, name) in vectors.iter().zip(&names) {
        assert_eq!(opened.read(name).unwrap(), vector.plain, "{name}");
    }
    file_parity(&bytes);
}

/// The bytes of a dropped entry: the fixture archive for an archive name,
/// else its own path.
fn dropped_bytes(profile: GameProfile, path: &str) -> ByteSource {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap()
        .to_ascii_lowercase();
    source(match (profile, name.as_str()) {
        (Fallout1, "master.dat") => f1_master(),
        (Fallout1, "critter.dat") => f1_critter(),
        (Fallout2, "master.dat") => f2_master(),
        (Fallout2, "critter.dat") => f2_critter(),
        (Fallout2, "patch000.dat") => f2_patch(),
        _ => path.as_bytes().to_vec(),
    })
}

fn tree_paths(mount: &MemoryMount) -> Vec<String> {
    mount.loose.as_ref().map_or_else(Vec::new, |tree| {
        tree.files().map(|path| path.as_str().to_owned()).collect()
    })
}

fn list(text: &str) -> Vec<String> {
    if text == "(none)" {
        return Vec::new();
    }
    text.split(", ").map(str::to_owned).collect()
}

/// One drop against its `expected` column, clause by clause.
fn check_drop(case: &str, profile: GameProfile, names: &[&str], expected: &str) -> Option<String> {
    let entries = names
        .iter()
        .map(|name| (*name, dropped_bytes(profile, name)));
    let classified = classify_drop(profile, entries);
    let mut summary = None;
    for clause in expected.split("; ") {
        let context = format!("case {case}: {clause}");
        if let Some(text) = clause.split_once("refused: ").map(|(_, text)| text) {
            let error = classified.as_ref().expect_err(&context).to_string();
            assert_eq!(error, text, "{context}");
            summary = Some(error);
            continue;
        }
        if clause.starts_with("strip ") && classified.is_err() {
            continue;
        }
        let (mount, report) = classified.as_ref().expect(&context);
        if let Some(folder) = clause.strip_prefix("strip ") {
            assert_eq!(
                report.stripped.as_deref(),
                folder.strip_suffix('/'),
                "{context}"
            );
        } else if let Some((role, archive)) = clause.split_once(" = ")
            && ["master", "critter", "patch"].contains(&role)
        {
            assert!(
                report.archives.iter().any(|name| name == archive),
                "{context}"
            );
        } else if let Some(paths) = clause.rsplit_once("loose = ").map(|(_, paths)| paths) {
            assert_eq!(tree_paths(mount), list(paths), "{context}");
            assert_eq!(report.loose_files, list(paths).len(), "{context}");
        } else if let Some(paths) = clause.strip_prefix("ignored = ") {
            let (paths, reason) = match paths.split_once(" (") {
                Some((paths, reason)) => (paths, reason.strip_suffix(')')),
                None => (paths, None),
            };
            let found: Vec<&str> = report.ignored.iter().map(|e| e.path.as_str()).collect();
            assert_eq!(found, list(paths), "{context}");
            if let Some(reason) = reason {
                assert_eq!(report.ignored[0].reason, reason, "{context}");
            }
        } else {
            panic!("{context}: a clause this test does not read");
        }
    }
    let (mount, report) = match classified {
        Ok(found) => found,
        Err(_) => return summary,
    };
    Some(format!("{report:?} {:?}", tree_paths(&mount)))
}

#[test]
fn t6_every_drop_gives_its_mount_in_any_order() {
    let cases = rows(fixture!("drops.tsv"));
    assert_eq!(cases.len(), 9);
    for row in cases {
        let [case, profile, dropped, expected] = row[..] else {
            panic!("{row:?}");
        };
        let profile = if profile == "fallout2" {
            Fallout2
        } else {
            Fallout1
        };
        let mut names: Vec<&str> = dropped.split(", ").collect();
        let forward = check_drop(case, profile, &names, expected);
        names.reverse();
        assert_eq!(
            check_drop(case, profile, &names, expected),
            forward,
            "case {case}"
        );
    }
}

#[test]
fn a_dropped_game_folder_mounts_and_reads() {
    let names = [
        "Fallout/MASTER.DAT",
        "Fallout/CRITTER.DAT",
        "Fallout/DATA/MAPS/X.MAP",
        "Fallout/DATA/SAVEGAME/SLOT01/SAVE.DAT",
        "Fallout/FALLOUT.CFG",
    ];
    let entries = names.map(|name| (name, dropped_bytes(Fallout1, name)));
    let (mount, report) = classify_drop(Fallout1, entries).unwrap();
    assert_eq!(report.archives, ["critter.dat", "master.dat"]);
    let provider = mount.mount().unwrap();
    assert_eq!(
        provider.read("maps/x.map").unwrap(),
        b"Fallout/DATA/MAPS/X.MAP"
    );
    assert_eq!(provider.read("readme.txt").unwrap(), b"MASTER1");
    assert!(provider.read("data/savegame/slot01/save.dat").is_err());
}

#[test]
fn a_loose_corpus_serves_everything_and_fails_validation_later() {
    let names = ["a/master.dat", "b/critter.dat"];
    let entries = names.map(|name| (name, dropped_bytes(Fallout1, name)));
    let (mount, _) = classify_drop(Fallout1, entries).unwrap();
    let provider = mount.mount().unwrap();
    assert_eq!(provider.read("a/master.dat").unwrap(), f1_master());
    let (_, part) = provider
        .read_served("b/critter.dat", MAX_ASSET_BYTES)
        .unwrap();
    assert_eq!(part, ServedBy::Loose);
    assert_eq!(provider.missing_members(), FALLOUT1_MEMBERS);
    assert!(
        matches!(provider.validate(), Err(VfsError::Archive(message)) if message.contains("color.pal"))
    );
}

#[test]
fn required_members_are_looked_up_in_the_master_database_as_mounted() {
    // The loose tree counts; the critter archive does not.
    let members = FALLOUT1_MEMBERS.map(|path| (path, source(*b"x")));
    let provider = MemoryMount {
        loose: Some(MemoryDir::new(members).unwrap()),
        ..mount("f1")
    }
    .mount()
    .unwrap();
    assert!(provider.validate().is_ok());
    let provider = mount("f1").mount().unwrap();
    assert_eq!(provider.missing_members(), FALLOUT1_MEMBERS);
    let provider = mount("f2").mount().unwrap();
    assert_eq!(provider.missing_members(), fallout2::REQUIRED_MEMBERS);
    let members = fallout2::REQUIRED_MEMBERS.map(|path| (path, source(*b"x")));
    let provider = MemoryMount {
        loose: Some(MemoryDir::new(members).unwrap()),
        ..mount("f2")
    }
    .mount()
    .unwrap();
    assert!(provider.validate().is_ok());
}

#[test]
fn a_mount_refuses_a_patch_under_fallout1_and_half_a_pair() {
    let refused = |mount: MemoryMount| match mount.mount() {
        Err(VfsError::Archive(message)) => message,
        other => panic!("{:?}", other.is_ok()),
    };
    let patched = MemoryMount {
        patch: Some(source(f2_patch())),
        ..mount("f1")
    };
    assert!(refused(patched).contains("patch000.dat"));
    let no_critter = MemoryMount {
        critter: None,
        ..mount("f2")
    };
    assert_eq!(refused(no_critter), "critter.dat is missing");
    let no_master = MemoryMount {
        master: None,
        ..mount("f2")
    };
    assert_eq!(refused(no_master), "master.dat is missing");
    let only_a_patch = MemoryMount {
        profile: Fallout2,
        patch: Some(source(f2_patch())),
        ..MemoryMount::default()
    };
    assert_eq!(refused(only_a_patch), "master.dat is missing");
}

#[test]
fn mod_layers_lie_above_the_mount_highest_first() {
    let tree = |path: &str, bytes: &'static str| {
        MemoryDir::new([(path.to_owned(), source(bytes))]).unwrap()
    };
    let mods = MemoryDir::new([
        ("data/shared.txt", source("LOW")),
        (crate::mod_manifest::MANIFEST, source("name = \"Low\"")),
    ])
    .unwrap();
    let provider = MemoryMount {
        layers: vec![
            ("mods/low".to_owned(), mods),
            ("mods/high".to_owned(), tree("data/shared.txt", "HIGH")),
        ],
        ..mount("f2")
    }
    .mount()
    .unwrap();
    assert_eq!(provider.mod_layers(), 2);
    let (bytes, part) = provider.read_served("data/shared.txt", 64).unwrap();
    assert_eq!(
        (bytes.as_slice(), part.label()),
        (&b"HIGH"[..], "mods/high")
    );
    let (_, named) = provider.read_with_layer("data/shared.txt", 64).unwrap();
    assert_eq!(named.as_deref(), Some("mods/high"));
    assert_eq!(
        provider.layer_manifests().unwrap(),
        [("mods/low".to_owned(), b"name = \"Low\"".to_vec())]
    );
    let listed: BTreeSet<&str> = provider.files().map(VfsPath::as_str).collect();
    assert!(listed.contains("vault-mod.toml") && listed.contains("data/zip.txt"));
}

#[test]
fn a_byte_source_shares_its_buffer_and_never_reads_past_it() {
    let whole = source(*b"0123456789");
    assert_eq!(format!("{whole:?}"), "ByteSource(10 bytes)");
    let part = whole.slice(2, 5).unwrap();
    assert_eq!(part.as_slice(), b"23456");
    assert_eq!(part.slice(1, 2).unwrap().as_slice(), b"34");
    assert!(whole.slice(0, 0).unwrap().is_empty());
    assert!(whole.slice(10, 0).unwrap().is_empty());
    assert!(whole.slice(10, 1).is_none());
    assert!(whole.slice(11, 0).is_none());
    // A 32-bit offset and size near the top never wrap.
    assert!(
        whole
            .slice(u64::from(u32::MAX), u64::from(u32::MAX))
            .is_none()
    );
    assert!(whole.slice(u64::MAX, 2).is_none());
    assert!(std::sync::Arc::strong_count(&whole.owner) >= 2);
    static EMBEDDED: [u8; 3] = *b"abc";
    assert_eq!(ByteSource::new(&EMBEDDED[..]).unwrap().as_slice(), b"abc");
    assert!(check_length(MAX_SOURCE_BYTES).is_ok());
    assert!(check_length(MAX_SOURCE_BYTES + 1).is_err());
}

#[test]
fn a_tree_refuses_what_a_folder_could_not_hold() {
    let bytes = || source(*b"x");
    let refused = |paths: [&str; 2]| MemoryDir::new(paths.map(|path| (path, bytes()))).unwrap_err();
    assert!(matches!(
        refused(["Data/A.txt", "data/a.TXT"]),
        VfsError::CaseCollision { first, second, .. }
            if first == std::path::Path::new("Data/A.txt")
                && second == std::path::Path::new("data/a.TXT")
    ));
    // Whichever order they come in, and however deep the folder is.
    for paths in [["data", "data/a.txt"], ["data/a/b/c.txt", "data"]] {
        assert!(matches!(refused(paths), VfsError::FileIsFolder { file, .. } if file == "data"));
    }
    // `data-x` beside `data/` is neither.
    assert!(MemoryDir::new(["data-x", "data/a.txt"].map(|path| (path, bytes()))).is_ok());
    let limit = MemoryDir::new([("big.bin", source(vec![0; MAX_ASSET_BYTES]))]).unwrap();
    assert!(matches!(
        limit.read_bounded("big.bin", 1),
        Err(VfsError::FileTooLarge { max_bytes: 1, .. })
    ));
    let over = MemoryDir::new([("big.bin", source(vec![0; MAX_ASSET_BYTES + 1]))]);
    assert!(matches!(over, Err(VfsError::FileTooLarge { .. })));
    // An empty value is an ordinary empty file.
    let empty = MemoryDir::new([("empty.txt", source(Vec::new()))]).unwrap();
    assert_eq!(empty.read("EMPTY.TXT").unwrap(), b"");
}

#[test]
fn a_memory_tree_reads_paths_as_the_loose_provider_does() {
    let tree = MemoryDir::new([("Data\\Sub/File.TXT", source("x"))]).unwrap();
    assert_eq!(tree.read("data/sub/file.txt").unwrap(), b"x");
    assert_eq!(tree.read("DATA\\SUB\\FILE.TXT").unwrap(), b"x");
    assert!(matches!(tree.read("data/sub"), Err(VfsError::NotFound(_))));
    assert!(matches!(
        tree.read("../x"),
        Err(VfsError::InvalidPath { .. })
    ));
}

#[test]
fn a_path_over_1024_bytes_is_refused_by_vfs_path_itself() {
    // SPEC-023 §2.2 rule 1, at the edge and through a real folder structure.
    let edge = format!("{}/{}", "a".repeat(511), "b".repeat(512));
    assert_eq!(edge.len(), 1024);
    assert_eq!(VfsPath::new(&edge).unwrap().as_str(), edge);
    assert!(VfsPath::new(&"a".repeat(crate::vfs::MAX_PATH_BYTES)).is_ok());
    for path in ["a".repeat(1025), format!("{edge}c"), format!("a/{edge}")] {
        assert!(path.len() > crate::vfs::MAX_PATH_BYTES);
        let error = VfsPath::new(&path).unwrap_err();
        assert!(
            matches!(
                error,
                VfsError::InvalidPath {
                    reason: "longer than 1024 bytes",
                    ..
                }
            ),
            "{error}"
        );
    }
    // A memory folder and a read use the same validator.
    let long = "a".repeat(1025);
    assert!(MemoryDir::new(vec![(long.clone(), source(b"x".to_vec()))]).is_err());
    let dir = MemoryDir::new(vec![("a.txt".to_owned(), source(b"x".to_vec()))]).unwrap();
    assert!(matches!(dir.read(&long), Err(VfsError::InvalidPath { .. })));
}
