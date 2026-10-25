//! The files an exported enhanced setup carries (the F9 overlay's "export
//! the current setup as a mod" and `cargo xtask export-mod`): the campaign
//! table overrides ([`crate::tables`]) and the scripts that need the
//! enhanced opcode range, such as the camera demo's. Engine extension, not
//! part of any specification.
use crate::tables::Table;
use vault_formats::{Vfs, VfsError};
use vault_vm::IntProgram;

/// The setup files `vfs` holds, as (VFS path, bytes) in path order: every
/// `data/vault-engine/<table>.txt` override present, and every
/// `scripts/*.int` program that a SPEC-004 parse refuses and an enhanced
/// parse accepts (it uses the versioned enhanced words, the camera's among
/// them). Base game data holds neither, so these are what the setup adds.
pub fn setup_files(vfs: &dyn Vfs) -> Result<Vec<(String, Vec<u8>)>, VfsError> {
    let tables: Vec<String> = Table::ALL
        .iter()
        .map(|table| table.override_path())
        .collect();
    let mut files = Vec::new();
    for path in vfs.files() {
        let path = path.as_str();
        let script = path.starts_with("scripts/") && path.ends_with(".int");
        if !script && !tables.iter().any(|table| table == path) {
            continue;
        }
        let bytes = vfs.read(path)?;
        if script && !needs_enhanced(&bytes) {
            continue;
        }
        files.push((path.to_owned(), bytes));
    }
    Ok(files)
}

/// A program only an enhanced host runs: SPEC-004 refuses it, the enhanced
/// parse accepts it.
pub fn needs_enhanced(bytes: &[u8]) -> bool {
    IntProgram::parse(bytes).is_err() && IntProgram::parse_enhanced(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_formats::LocalDirProvider;

    #[test]
    fn the_setup_files_are_the_table_overrides_and_enhanced_scripts() {
        let dir = tempfile::tempdir().unwrap();
        vault_fixtures::state::write_corpus(dir.path());
        std::fs::write(
            dir.path().join("scripts/core-fixture.int"),
            vault_fixtures::vm::program(),
        )
        .unwrap();
        // Without the demo or an override there is nothing to carry.
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        assert!(setup_files(&vfs).unwrap().is_empty());
        vault_fixtures::state::write_cam_demo(dir.path());
        std::fs::create_dir_all(dir.path().join("data/vault-engine")).unwrap();
        let towns = Table::Towns.canonical();
        std::fs::write(dir.path().join("data/vault-engine/towns.txt"), towns).unwrap();
        // Not a table: left out.
        std::fs::write(dir.path().join("data/vault-engine/notes.txt"), "x").unwrap();
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        let files = setup_files(&vfs).unwrap();
        let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(
            paths,
            ["data/vault-engine/towns.txt", "scripts/camdemo.int"]
        );
        assert_eq!(files[0].1, towns.as_bytes());
        assert_eq!(files[1].1, vault_fixtures::state::cam_demo_program());
        assert!(!needs_enhanced(&vault_fixtures::vm::program()));
    }
}
