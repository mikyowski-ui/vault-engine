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
    // Live lookups still refuse case-folded ambiguity.
    fs::copy(temp.path().join("COLOR.PAL"), temp.path().join("NEW.pal")).unwrap();
    assert!(matches!(
        vfs.read("new.pal"),
        Err(VfsError::CaseCollision { .. })
    ));
    fs::remove_file(temp.path().join("NEW.pal")).unwrap();
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
        fs::write(
            temp.path().join(OsString::from_vec(vec![0xff])),
            b"synthetic",
        )
        .unwrap();
        assert!(matches!(
            LocalDirProvider::mount(temp.path()),
            Err(VfsError::NonUtf8Name(_))
        ));
    }
}
