#![cfg(feature = "host-fs")]

use std::{fs, path::Path};
use vault_formats::{LocalDirProvider, Vfs, VfsError, VfsPath};

fn fixture() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/loose"
    ))
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn root_assets_and_nested_assets_are_case_insensitive() {
    let provider = LocalDirProvider::mount(fixture()).unwrap();
    let vfs: &dyn Vfs = &provider;
    for (query, original) in [
        ("color.pal", "COLOR.PAL"),
        ("FONT0.AAF", "font0.AAF"),
        ("Font5.fon", "font5.FON"),
        (r"ART\ITEMS\test.FRM", "art/items/Test.FRM"),
        ("TEXT/ENGLISH/GAME/test.msg", "text/english/game/Test.MSG"),
    ] {
        assert_eq!(
            vfs.read(query).unwrap(),
            fs::read(fixture().join(original)).unwrap()
        );
    }
    let paths: Vec<_> = vfs.files().map(VfsPath::as_str).collect();
    assert_eq!(paths.len(), 11);
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(paths.contains(&"premade/hero.gcd"));
    assert!(paths.contains(&"sound/music/theme.acm"));
    assert!(matches!(
        vfs.read("missing.frm"),
        Err(VfsError::NotFound(_))
    ));
}

#[test]
fn bounded_reads_allow_exact_lengths_and_reject_oversized_assets() {
    let provider = LocalDirProvider::mount(fixture()).unwrap();
    let bytes = provider.read("color.pal").unwrap();
    assert_eq!(
        provider.read_bounded("color.pal", bytes.len()).unwrap(),
        bytes
    );
    assert!(matches!(
        provider.read_bounded("color.pal", bytes.len() - 1),
        Err(VfsError::FileTooLarge { .. })
    ));
}

#[test]
fn normalization_is_idempotent_and_rejects_ambiguous_or_escaping_paths() {
    let provider = LocalDirProvider::mount(fixture()).unwrap();
    let normalized = VfsPath::new(r"ArT\Items\Test.FRM").unwrap();
    assert_eq!(normalized.as_str(), "art/items/test.frm");
    assert_eq!(normalized.extension(), Some("frm"));
    assert_eq!(VfsPath::new(normalized.as_str()).unwrap(), normalized);
    for path in [
        "",
        ".",
        "..",
        "../color.pal",
        r"..\color.pal",
        "art/../../color.pal",
        "art/../color.pal",
        "art/./test.frm",
        "/color.pal",
        r"\color.pal",
        r"C:\color.pal",
        "C:color.pal",
        r"\\server\share\color.pal",
        r"\\?\C:\color.pal",
        "art//items/test.frm",
        "art/",
        "color.pal\0",
        "color.pal\n",
        "color.pal:stream",
        "color.pal.",
        "color.pal ",
        "art/CON.frm",
        "nul",
        "com1.txt",
        "LPT9",
        "art/*.frm",
        "cólor.pal",
    ] {
        assert!(
            matches!(provider.read(path), Err(VfsError::InvalidPath { .. })),
            "{path:?}"
        );
    }
}

#[test]
fn mount_rejects_file_and_directory_case_collisions() {
    for (original, collision, is_dir) in [
        ("COLOR.PAL", "color.pal", false),
        ("art", "ART", true),
        ("COLOR.PAL", "Color.pal", true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        copy_tree(fixture(), temp.path());
        let collision = temp.path().join(collision);
        if collision.exists() {
            // The host filesystem itself folds case.
            continue;
        }
        if is_dir {
            fs::create_dir(&collision).unwrap();
        } else {
            fs::copy(temp.path().join(original), &collision).unwrap();
        }
        assert!(matches!(
            LocalDirProvider::mount(temp.path()),
            Err(VfsError::CaseCollision { .. })
        ));
    }
}

#[test]
fn namespace_is_a_snapshot_reads_are_live_and_missing_or_non_directory_roots_fail() {
    let temp = tempfile::tempdir().unwrap();
    copy_tree(fixture(), temp.path());
    let vfs = LocalDirProvider::mount(temp.path()).unwrap();
    fs::copy(temp.path().join("COLOR.PAL"), temp.path().join("new.pal")).unwrap();
    // The listing is the mount-time snapshot, but a read that misses it
    // consults the live directory (SPEC-008 §3.2 safe rule).
    assert!(!vfs.files().any(|path| path.as_str() == "new.pal"));
    assert_eq!(
        vfs.read("NEW.PAL").unwrap(),
        fs::read(temp.path().join("COLOR.PAL")).unwrap()
    );
    assert!(matches!(vfs.read("absent.pal"), Err(VfsError::NotFound(_))));
    // Live lookups still refuse case-folded ambiguity, where the host
    // filesystem can hold two names that differ only in case (macOS and
    // Windows fold case by default, so `NEW.pal` would be `new.pal`).
    if !temp.path().join("NEW.pal").exists() {
        fs::copy(temp.path().join("COLOR.PAL"), temp.path().join("NEW.pal")).unwrap();
        assert!(matches!(
            vfs.read("new.pal"),
            Err(VfsError::CaseCollision { .. })
        ));
        fs::remove_file(temp.path().join("NEW.pal")).unwrap();
    }
    assert_eq!(
        LocalDirProvider::mount(temp.path())
            .unwrap()
            .files()
            .count(),
        12
    );
    assert!(LocalDirProvider::mount(temp.path().join("missing")).is_err());
    assert!(LocalDirProvider::mount(temp.path().join("COLOR.PAL")).is_err());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };

    #[test]
    fn links_to_files_directories_and_inside_the_mount_are_rejected() {
        for target in [
            fixture().join("COLOR.PAL"),
            fixture().join("art"),
            "COLOR.PAL".into(),
        ] {
            let temp = tempfile::tempdir().unwrap();
            copy_tree(fixture(), temp.path());
            symlink(target, temp.path().join("link")).unwrap();
            assert!(matches!(
                LocalDirProvider::mount(temp.path()),
                Err(VfsError::Symlink(_))
            ));
        }
    }

    #[test]
    fn replacing_a_file_or_parent_with_an_escape_link_cannot_escape() {
        for replace_directory in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            copy_tree(fixture(), temp.path());
            let vfs = LocalDirProvider::mount(temp.path()).unwrap();
            let (path, query) = if replace_directory {
                ("art", "art/items/test.frm")
            } else {
                ("COLOR.PAL", "color.pal")
            };
            let target = temp.path().join(path);
            if replace_directory {
                fs::remove_dir_all(&target).unwrap();
            } else {
                fs::remove_file(&target).unwrap();
            }
            symlink(fixture().join(path), target).unwrap();
            assert!(matches!(vfs.read(query), Err(VfsError::Symlink(_))));
        }
    }

    #[test]
    fn replacing_the_ambient_mount_path_does_not_retarget_the_capability() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("root");
        copy_tree(fixture(), &original);
        let vfs = LocalDirProvider::mount(&original).unwrap();
        fs::rename(&original, temp.path().join("moved")).unwrap();
        fs::create_dir(&original).unwrap();
        assert_eq!(
            vfs.read("color.pal").unwrap(),
            fs::read(fixture().join("COLOR.PAL")).unwrap()
        );
    }

    #[test]
    fn host_filenames_cannot_smuggle_virtual_separators_or_non_utf8() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(r"art\test.frm"), b"synthetic").unwrap();
        assert!(matches!(
            LocalDirProvider::mount(temp.path()),
            Err(VfsError::InvalidPath { .. })
        ));
        fs::remove_file(temp.path().join(r"art\test.frm")).unwrap();
        // A filesystem that refuses non-UTF-8 names (APFS on macOS) cannot
        // hold one to smuggle in.
        if fs::write(
            temp.path().join(OsString::from_vec(vec![0xff])),
            b"synthetic",
        )
        .is_ok()
        {
            assert!(matches!(
                LocalDirProvider::mount(temp.path()),
                Err(VfsError::NonUtf8Name(_))
            ));
        }
    }
}

mod layered {
    use std::{fs, path::Path, sync::Arc};
    use vault_formats::{
        LayerEntry, LayeredVfs, LoadOrderPolicy, LocalDirProvider, ModLayer, Vfs, VfsError,
        VfsPath, parse_load_order,
    };

    /// An archive member: its name and bytes.
    type Member<'a> = (&'a str, &'a [u8]);
    /// A raw (uncompressed) synthetic archive: directories with members.
    fn archive(directories: &[(&str, &[Member])]) -> Vec<u8> {
        let mut tables = Vec::new();
        let count = directories.len() as u32;
        for word in [count, count, 0, 0] {
            tables.extend(word.to_be_bytes());
        }
        for (name, _) in directories {
            tables.push(name.len() as u8);
            tables.extend(name.as_bytes());
        }
        let table_len: usize = tables.len()
            + directories
                .iter()
                .map(|(_, files)| {
                    16 + files
                        .iter()
                        .map(|(name, _)| 1 + name.len() + 16)
                        .sum::<usize>()
                })
                .sum::<usize>();
        let mut offset = table_len as u32;
        let mut payload: Vec<u8> = Vec::new();
        for (_, files) in directories {
            let count = files.len() as u32;
            for word in [count, count, 16, 0] {
                tables.extend(word.to_be_bytes());
            }
            for (name, body) in *files {
                tables.push(name.len() as u8);
                tables.extend(name.as_bytes());
                for word in [0x20, offset, body.len() as u32, body.len() as u32] {
                    tables.extend(word.to_be_bytes());
                }
                offset += body.len() as u32;
                payload.extend(*body);
            }
        }
        assert_eq!(tables.len(), table_len);
        tables.extend(payload);
        tables
    }
    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn base(root: &Path) -> Arc<dyn Vfs> {
        write(root, "base/data/a.txt", b"base a");
        write(root, "base/data/b.txt", b"base b");
        write(root, "base/data/c.txt", b"base c");
        write(root, "base/art/critters/hero.frm", b"base hero");
        Arc::new(LocalDirProvider::mount(root.join("base")).unwrap())
    }

    #[test]
    fn three_layers_override_in_order_and_list_the_union() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base = base(root);
        // Layer 1: a loose folder overriding a and b, adding one.
        write(root, "mods/one/data/A.TXT", b"one a");
        write(root, "mods/one/data/b.txt", b"one b");
        write(root, "mods/one/data/only1.txt", b"one only");
        // Layer 2: a DAT overriding b, with critter art in its own archive.
        let master = archive(&[("DATA", &[("B.TXT", b"two b"), ("ONLY2.TXT", b"two only")])]);
        let critter = archive(&[("ART\\CRITTERS", &[("HERO.FRM", b"two hero")])]);
        write(root, "mods/two.dat", &master);
        write(root, "mods/two-critter.dat", &critter);
        // A master-archive copy of critter art is never used for that path.
        let decoy = archive(&[("ART\\CRITTERS", &[("HERO.FRM", b"wrong archive")])]);
        write(root, "mods/three/master.dat", &decoy);
        // Layer 3: folder plus master archive; overrides a only.
        write(root, "mods/three/data/a.txt", b"three a");
        write(
            root,
            "load_order.toml",
            br#"
# lowest first
[[layer]]
path = "mods/one"

[[layer]]
path = "mods/two.dat"   # a .dat path is the layer's master archive
critter_dat = "mods/two-critter.dat"

[[layer]]
path = 'mods/three'
master_dat = "mods/three/master.dat"
"#,
        );
        let vfs = LayeredVfs::from_load_order(base, root.join("load_order.toml")).unwrap();
        assert_eq!(vfs.mod_layers(), 3);
        let names: Vec<_> = vfs.layers().iter().map(ModLayer::name).collect();
        assert_eq!(names, ["mods/one", "mods/two.dat", "mods/three"]);
        for (path, expected) in [
            ("data/a.txt", &b"three a"[..]),
            ("DATA\\B.TXT", b"two b"),
            ("data/c.txt", b"base c"),
            ("data/only1.txt", b"one only"),
            ("data/only2.txt", b"two only"),
            // Critter art: layer three's loose folder lacks it and its
            // master archive is not consulted, so layer two's critter
            // archive serves it.
            ("art/critters/hero.frm", b"two hero"),
        ] {
            assert_eq!(vfs.read(path).unwrap(), expected, "{path}");
        }
        assert!(matches!(
            vfs.read("data/missing.txt"),
            Err(VfsError::NotFound(_))
        ));
        assert!(matches!(
            vfs.read("../escape.txt"),
            Err(VfsError::InvalidPath { .. })
        ));
        let files: Vec<_> = vfs.files().map(VfsPath::as_str).collect();
        let mut sorted = files.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(files, sorted, "canonical order without duplicates");
        for path in [
            "art/critters/hero.frm",
            "data/a.txt",
            "data/b.txt",
            "data/c.txt",
            "data/only1.txt",
            "data/only2.txt",
        ] {
            assert!(files.contains(&path), "{path}");
        }
        // Bounded reads honor the winning layer's size.
        assert!(matches!(
            vfs.read_bounded("data/a.txt", 3),
            Err(VfsError::FileTooLarge { .. })
        ));
    }

    #[test]
    fn the_array_form_and_its_parser() {
        assert_eq!(
            parse_load_order("layers = [\"a\", 'b.dat',\n  \"c\" ] # done\n").unwrap(),
            ["a", "b.dat", "c"].map(|path| LayerEntry {
                path: Some(path.into()),
                ..LayerEntry::default()
            })
        );
        assert_eq!(parse_load_order("# nothing\n").unwrap(), []);
        assert_eq!(
            parse_load_order("layers = [\"x#y\"]").unwrap()[0]
                .path
                .as_deref(),
            Some("x#y")
        );
        for bad in [
            "layers = [\"a\"",
            "layers = \"a\"",
            "layers = [\"a\" \"b\"]",
            "[[layer]]\nname = \"a\"",
            "[[layer]]\npath = \"a\"\npath = \"b\"",
            "[[layer]]\n",
            "[other]\n",
            "path = \"a\"",
            "layers = [\"a\"]\n[[layer]]\npath = \"b\"",
            "[[layer]]\npath = \"a\\q\"",
        ] {
            assert!(
                matches!(parse_load_order(bad), Err(VfsError::LoadOrder(_))),
                "{bad:?}"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let base = base(dir.path());
        write(dir.path(), "mods/a/data/a.txt", b"array a");
        write(dir.path(), "load_order.toml", b"layers = [\"mods/a\"]\n");
        let vfs = LayeredVfs::from_load_order(base, dir.path().join("load_order.toml")).unwrap();
        assert_eq!(vfs.read("data/a.txt").unwrap(), b"array a");
        assert_eq!(vfs.read("data/b.txt").unwrap(), b"base b");
    }

    #[test]
    fn traversal_links_and_missing_layers_are_clean_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        let base = base(&root);
        write(dir.path(), "outside/data/a.txt", b"outside a");
        let load = |text: &str, policy| {
            write(&root, "load_order.toml", text.as_bytes());
            LayeredVfs::from_load_order_with(base.clone(), root.join("load_order.toml"), policy)
        };
        let strict = LoadOrderPolicy::default();
        let absolute = dir.path().join("outside").display().to_string();
        for entry in [
            "../outside",
            "mods/../../outside",
            "..\\\\outside",
            absolute.as_str(),
            "/etc",
            "C:/outside",
            "",
        ] {
            let text = format!("layers = [\"{}\"]", entry.replace('\\', "\\\\"));
            assert!(
                matches!(load(&text, strict), Err(VfsError::LoadOrder(_))),
                "{entry:?}"
            );
        }
        // The caller may allow leaving the directory explicitly.
        let allow = LoadOrderPolicy {
            allow_outside: true,
        };
        let vfs = load("layers = [\"../outside\"]", allow).unwrap();
        assert_eq!(vfs.read("data/a.txt").unwrap(), b"outside a");
        let text = format!("layers = [\"{}\"]", absolute.replace('\\', "\\\\"));
        assert_eq!(
            load(&text, allow).unwrap().read("data/a.txt").unwrap(),
            b"outside a"
        );
        // A missing layer folder or archive is an error, not an empty layer.
        assert!(matches!(
            load("layers = [\"mods/none\"]", strict),
            Err(VfsError::Io { .. })
        ));
        assert!(matches!(
            load("[[layer]]\npath = \"mods/none.dat\"", strict),
            Err(VfsError::Io { .. })
        ));
        // A corrupt archive is an archive error.
        write(&root, "mods/bad.dat", b"not an archive");
        assert!(matches!(
            load("layers = [\"mods/bad.dat\"]", strict),
            Err(VfsError::Archive(_))
        ));
        // Requests through a layer keep the canonical path rules.
        write(&root, "mods/ok/data/a.txt", b"ok a");
        let vfs = load("layers = [\"mods/ok\"]", strict).unwrap();
        for path in ["../data/a.txt", "/data/a.txt", "data//a.txt", "C:data"] {
            assert!(
                matches!(vfs.read(path), Err(VfsError::InvalidPath { .. })),
                "{path}"
            );
        }
        #[cfg(unix)]
        {
            // A symlinked layer, or a link inside one, is refused.
            std::os::unix::fs::symlink(dir.path().join("outside"), root.join("mods/link")).unwrap();
            assert!(matches!(
                load("layers = [\"mods/link\"]", strict),
                Err(VfsError::Symlink(_))
            ));
            std::os::unix::fs::symlink(
                dir.path().join("outside/data/a.txt"),
                root.join("mods/ok/data/linked.txt"),
            )
            .unwrap();
            assert!(matches!(
                load("layers = [\"mods/ok\"]", strict),
                Err(VfsError::Symlink(_))
            ));
        }
    }
}
