//! The files an exported enhanced setup carries (the F9 overlay's "export
//! the current setup as a mod" and `cargo xtask export-mod`): the campaign
//! table overrides ([`crate::tables`]), the scripts that need the enhanced
//! opcode range, such as the camera demo's, and everything the mod layers
//! register ([`ModRegistry`]) with the registrations themselves, the
//! `[[art]]`, `[[protos]]` and `[[tile_remap]]` rows of SPEC-024a §2.4
//! included. Engine extension, not part of any specification.
use crate::tables::Table;
use std::collections::BTreeMap;
use vault_formats::{
    Vfs, VfsError,
    fid::ART_DIRECTORIES,
    mod_manifest::{
        ArtRegistration, MapRegistration, ModRegistry, PROTO_CATEGORIES, PatchRegistration,
        ProtoRegistration, ScriptRegistration, TileRemapRegistration,
    },
};
use vault_vm::IntProgram;

/// An exported setup: its files, by VFS path, and the registrations its
/// manifest carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Setup {
    pub files: BTreeMap<String, Vec<u8>>,
    pub scripts: BTreeMap<u32, ScriptRegistration>,
    pub maps: BTreeMap<u32, MapRegistration>,
    pub patches: Vec<PatchRegistration>,
    /// `[[art]]` rows by (category, index).
    pub art: BTreeMap<(u8, u32), ArtRegistration>,
    /// `[[protos]]` rows by (category, index).
    pub protos: BTreeMap<(u8, u32), ProtoRegistration>,
    /// `[[tile_remap]]` rows by lowercased map file name.
    pub tile_remaps: BTreeMap<String, TileRemapRegistration>,
}

impl Setup {
    /// `other` over this one: its files, scripts, maps, art and prototype
    /// rows and tile remaps win a path, row, number or map; its patches
    /// follow (a later patch wins a record).
    pub fn merge(&mut self, other: Setup) {
        self.files.extend(other.files);
        self.scripts.extend(other.scripts);
        self.maps.extend(other.maps);
        self.art.extend(other.art);
        self.protos.extend(other.protos);
        self.tile_remaps.extend(other.tile_remaps);
        for patch in other.patches {
            if !self.patches.contains(&patch) {
                self.patches.push(patch);
            }
        }
    }
}

/// The setup `vfs` holds:
/// - every `data/vault-engine/<table>.txt` override present;
/// - every `scripts/*.int` program that a SPEC-004 parse refuses and an
///   enhanced parse accepts (it uses the versioned enhanced words, the
///   camera's among them);
/// - every script, map (with its `.gam`, if any) and table patch the mod
///   layers register, with the registrations;
/// - every `[[art]]` row's art file that is present (a row with a name and
///   no file is valid, SPEC-007 §2.1.1), every `[[protos]]` row's prototype,
///   and every `[[tile_remap]]` row's map and sidecar, with the rows.
///
/// Base game data holds none of these, so they are what the setup adds. A
/// malformed manifest or a registered file that is missing (an art file
/// aside) is an error.
pub fn setup(vfs: &dyn Vfs) -> Result<Setup, VfsError> {
    let tables: Vec<String> = Table::ALL
        .iter()
        .map(|table| table.override_path())
        .collect();
    let mut setup = Setup::default();
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
        setup.files.insert(path.to_owned(), bytes);
    }
    let registry = ModRegistry::from_vfs(vfs).map_err(VfsError::LoadOrder)?;
    for script in registry.scripts.values() {
        let path = format!("scripts/{}", script.file);
        setup.files.insert(path.clone(), vfs.read(&path)?);
    }
    for map in registry.maps.values() {
        let path = format!("maps/{}", map.file);
        setup.files.insert(path.clone(), vfs.read(&path)?);
        let gam = path.replacen(".map", ".gam", 1);
        match vfs.read(&gam) {
            Ok(bytes) => {
                setup.files.insert(gam, bytes);
            }
            Err(VfsError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    for patch in &registry.patches {
        setup
            .files
            .insert(patch.file.clone(), vfs.read(&patch.file)?);
    }
    for art in registry.art.values() {
        let Some(directory) = ART_DIRECTORIES.get(usize::from(art.category)) else {
            continue;
        };
        let path = format!("art/{directory}/{}", art.file);
        match vfs.read(&path) {
            Ok(bytes) => {
                setup.files.insert(path, bytes);
            }
            Err(VfsError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    for proto in registry.protos.values() {
        let Some(directory) = PROTO_CATEGORIES.get(usize::from(proto.category)) else {
            continue;
        };
        let path = format!("proto/{directory}/{}", proto.file);
        setup.files.insert(path.clone(), vfs.read(&path)?);
    }
    for remap in registry.tile_remaps.values() {
        let map = format!("maps/{}", remap.map);
        setup.files.insert(map.clone(), vfs.read(&map)?);
        setup
            .files
            .insert(remap.file.clone(), vfs.read(&remap.file)?);
    }
    setup.scripts = registry.scripts;
    setup.maps = registry.maps;
    setup.patches = registry.patches;
    setup.art = registry.art;
    setup.protos = registry.protos;
    setup.tile_remaps = registry.tile_remaps;
    Ok(setup)
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
    fn the_setup_is_the_table_overrides_and_enhanced_scripts() {
        let dir = tempfile::tempdir().unwrap();
        vault_fixtures::state::write_corpus(dir.path());
        std::fs::write(
            dir.path().join("scripts/core-fixture.int"),
            vault_fixtures::vm::program(),
        )
        .unwrap();
        // Without the demo or an override there is nothing to carry.
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        assert_eq!(setup(&vfs).unwrap(), Setup::default());
        vault_fixtures::state::write_cam_demo(dir.path());
        std::fs::create_dir_all(dir.path().join("data/vault-engine")).unwrap();
        let towns = Table::Towns.canonical();
        std::fs::write(dir.path().join("data/vault-engine/towns.txt"), towns).unwrap();
        // Not a table: left out.
        std::fs::write(dir.path().join("data/vault-engine/notes.txt"), "x").unwrap();
        let vfs = LocalDirProvider::mount(dir.path()).unwrap();
        let found = setup(&vfs).unwrap();
        let paths: Vec<&str> = found.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            ["data/vault-engine/towns.txt", "scripts/camdemo.int"]
        );
        assert_eq!(found.files["data/vault-engine/towns.txt"], towns.as_bytes());
        assert_eq!(
            found.files["scripts/camdemo.int"],
            vault_fixtures::state::cam_demo_program()
        );
        assert!(found.scripts.is_empty() && found.maps.is_empty());
        assert!(!needs_enhanced(&vault_fixtures::vm::program()));
    }

    #[test]
    fn the_setup_carries_what_the_mod_layers_register() {
        let dir = tempfile::tempdir().unwrap();
        vault_fixtures::state::write_corpus(dir.path());
        let layer = vault_fixtures::state::write_cam_demo_mod(dir.path());
        let base: std::sync::Arc<dyn Vfs> =
            std::sync::Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let vfs = vault_formats::LayeredVfs::from_load_order(base, layer.load_order).unwrap();
        let found = setup(&vfs).unwrap();
        let paths: Vec<&str> = found.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "maps/cam_demo.gam",
                "maps/cam_demo.map",
                "scripts/camdemo.int"
            ]
        );
        assert_eq!(found.scripts[&layer.script].file, "camdemo.int");
        assert_eq!(found.maps[&layer.map].file, "cam_demo.map");
    }

    #[test]
    fn the_setup_carries_art_prototype_and_tile_remap_rows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tc");
        for folder in ["art/scenery", "proto/scenery", "maps"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(
            root.join("vault-mod.toml"),
            vault_fixtures::art::manifest(
                "TC",
                &[
                    ("scenery", "tc_rock.frm", 5000),
                    // A row with a name and no file: carried without one.
                    ("scenery", "nofile.frm", 5001),
                ],
                &[("scenery", "tc_rock.pro", 5000)],
                &[("tc.map", "maps/tc.tlx")],
            ),
        )
        .unwrap();
        for (path, bytes) in [
            ("art/scenery/tc_rock.frm", b"frm".to_vec()),
            ("proto/scenery/tc_rock.pro", b"pro".to_vec()),
            ("maps/tc.map", b"map".to_vec()),
            (
                "maps/tc.tlx",
                vault_fixtures::art::tile_remap(&[(4000, 5000)]),
            ),
        ] {
            std::fs::write(root.join(path), bytes).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("base")).unwrap();
        let base: std::sync::Arc<dyn Vfs> =
            std::sync::Arc::new(LocalDirProvider::mount(dir.path().join("base")).unwrap());
        let loose = LocalDirProvider::mount(&root).unwrap();
        let layer = vault_formats::ModLayer::new("tc", Some(loose), None, None);
        let vfs = vault_formats::LayeredVfs::new(base, vec![layer]);
        let found = setup(&vfs).unwrap();
        let paths: Vec<&str> = found.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "art/scenery/tc_rock.frm",
                "maps/tc.map",
                "maps/tc.tlx",
                "proto/scenery/tc_rock.pro"
            ]
        );
        assert_eq!(
            found.art.keys().copied().collect::<Vec<_>>(),
            [(2, 5000), (2, 5001)]
        );
        assert_eq!(found.protos[&(2, 5000)].file, "tc_rock.pro");
        assert_eq!(found.tile_remaps["tc.map"].file, "maps/tc.tlx");
        // A merge keeps both sides' rows.
        let mut merged = Setup::default();
        merged.merge(found.clone());
        assert_eq!(merged, found);
    }
}
