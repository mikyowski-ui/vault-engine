//! The canonical campaign-table text `build.rs` emits, its parser, and the
//! preset-gated VFS overrides.
use crate::{
    CharacterSheet, EnginePreset, WorldHost, WorldState,
    assets::VfsWorldAssets,
    critical, fixture_state as fixture,
    tables::{CampaignTables, Table},
    worldmap,
};
use std::{fs, sync::Arc};
use vault_formats::{LocalDirProvider, Vfs};

fn records(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

#[test]
fn canonical_text_parses_back_to_exactly_the_compiled_tables() {
    let compiled = CampaignTables::compiled();
    for table in Table::ALL {
        let text = table.canonical();
        assert!(text.starts_with("# vault-engine campaign table: "));
        // Parsing the emitted text over the compiled set changes nothing.
        let parsed = compiled.with_override(table, text).unwrap();
        assert_eq!(&parsed, compiled, "{table:?}");
        // The emitter and the runtime writer agree record for record.
        assert_eq!(
            records(text),
            records(&compiled.to_text(table)),
            "{table:?}"
        );
    }
    // Spot checks against the compiled constants themselves.
    assert_eq!(*compiled.critical_success, critical::CRITICAL_SUCCESS);
    assert_eq!(compiled.critical_failure, critical::CRITICAL_FAILURE);
    assert_eq!(compiled.terrain, worldmap::TERRAIN);
    assert_eq!(compiled.frequency, worldmap::FREQUENCY);
    assert_eq!(compiled.region, worldmap::REGION);
    assert_eq!(records(Table::Critical.canonical()).len(), 864);
    assert_eq!(records(Table::Maps.canonical()).len(), worldmap::MAP_COUNT);
    for (number, entry) in worldmap::MAP_TABLE.iter().enumerate() {
        let row = compiled.map_row(number as i32).unwrap();
        assert_eq!(
            (row.name.as_str(), row.town, row.music.as_str()),
            (entry.name, entry.town, entry.music)
        );
    }
    for (town, row) in worldmap::TOWNS.iter().zip(&compiled.towns) {
        assert_eq!(row.name, town.name);
        assert_eq!(row.cell, town.cell);
        assert_eq!(row.known_global, town.known_global);
        assert_eq!(row.entrance_globals, town.entrance_globals);
    }
    for (slots, row) in worldmap::HOTSPOTS.iter().zip(&compiled.hotspots) {
        for (hotspot, parsed) in slots.iter().zip(row) {
            assert_eq!(
                (hotspot.x, hotspot.y, hotspot.entry, hotspot.map),
                (parsed.x, parsed.y, parsed.entry, parsed.map.as_str())
            );
        }
    }
    // The table-backed lookups agree with the free functions.
    for row in 0..worldmap::ROWS {
        for column in 0..worldmap::COLUMNS {
            let cell = [column, row];
            assert_eq!(compiled.terrain(cell), worldmap::terrain(cell));
            assert_eq!(compiled.frequency(cell), worldmap::frequency(cell));
            assert_eq!(compiled.region(cell), worldmap::region(cell));
            assert_eq!(compiled.town_at(cell), worldmap::town_at(cell));
        }
    }
    for number in -2..70 {
        assert_eq!(compiled.map_town(number), worldmap::map_town(number));
        assert_eq!(compiled.city_music(number), worldmap::city_music(number));
    }
}

#[test]
fn overrides_are_validated_whole_before_replacing_a_table() {
    let compiled = CampaignTables::compiled();
    let maps = Table::Maps.canonical();
    // One changed value replaces exactly that row.
    let changed = maps.replace("|DESERT1.MAP|", "|DESERT9.MAP|");
    let tables = compiled.with_override(Table::Maps, &changed).unwrap();
    assert_eq!(tables.map_name(0), Some("DESERT9.MAP"));
    assert_eq!(tables.maps[1..], compiled.maps[1..]);
    // A missing row, a duplicate, a traversal name, a bad town, or a short
    // record is refused.
    let lines: Vec<&str> = maps.lines().collect();
    let without_last = lines[..lines.len() - 1].join("\n");
    let duplicate = format!("{maps}\n0|DESERT1.MAP|0|07desert");
    for bad in [
        without_last.as_str(),
        duplicate.as_str(),
        &maps.replace("|DESERT1.MAP|", "|../X.MAP|"),
        &maps.replace("|DESERT1.MAP|", "|desert1.map|"),
        &maps.replace("0|DESERT1.MAP|", "0|DESERT1.MAP|99|"),
        "0|DESERT1.MAP|0",
    ] {
        assert!(
            compiled.with_override(Table::Maps, bad).is_err(),
            "{bad:.60}"
        );
    }
    let grid = Table::Terrain.canonical();
    assert!(
        compiled
            .with_override(Table::Terrain, &grid.replace("0|1110", "0|111G"))
            .is_err()
    );
    assert!(
        compiled
            .with_override(Table::Terrain, &grid.replace("0|1110", "0|1119"))
            .is_err(),
        "terrain classes are 0-3"
    );
    let critical = Table::Critical.canonical();
    assert!(
        compiled
            .with_override(
                Table::Critical,
                &format!("{critical}\n16|0|0|1|0|-1|0|0|0|0")
            )
            .is_err()
    );
}

fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    fs::create_dir_all(dir.path().join("data/vault-engine")).unwrap();
    let maps = Table::Maps
        .canonical()
        .replace("|DESERT1.MAP|1|", "|DESERT1.MAP|5|");
    fs::write(dir.path().join("data/vault-engine/maps.txt"), maps).unwrap();
    dir
}
fn host(vfs: Arc<dyn Vfs>) -> WorldHost<VfsWorldAssets> {
    let assets = VfsWorldAssets::new(vfs, "english").unwrap();
    WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    )
}

#[test]
fn vanilla_ignores_overrides_and_enhanced_or_a_mod_layer_loads_them() {
    let dir = corpus();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    // Vanilla: the override file is present but never read.
    let mut vanilla = host(vfs.clone());
    assert_eq!(vanilla.preset(), EnginePreset::Vanilla);
    assert_eq!(EnginePreset::default(), EnginePreset::Vanilla);
    assert!(!vault_vm::VmHost::enhanced_opcodes(&vanilla));
    assert!(!vanilla.overrides_allowed());
    assert_eq!(vanilla.load_table_overrides().unwrap(), []);
    let _ = vanilla.load_campaign();
    assert!(vanilla.world.rules.tables.is_none());
    assert_eq!(vanilla.world.rules.tables().map_town(0), 1);

    // Enhanced reads it.
    let mut enhanced = host(vfs.clone());
    enhanced.set_preset(EnginePreset::Enhanced);
    assert_eq!(enhanced.load_table_overrides().unwrap(), [Table::Maps]);
    assert_eq!(enhanced.world.rules.tables().map_town(0), 5);
    assert_eq!(
        enhanced.world.rules.tables().map_town(1),
        worldmap::map_town(1)
    );
    // The enhanced opcode range follows the preset.
    assert!(vault_vm::VmHost::enhanced_opcodes(&enhanced));
    // Back to Vanilla drops the override.
    enhanced.set_preset(EnginePreset::Vanilla);
    assert!(enhanced.world.rules.tables.is_none());

    // A mounted mod layer (even an empty one) allows overrides in Vanilla.
    let layered: Arc<dyn Vfs> = Arc::new(vault_formats::LayeredVfs::new(
        vfs,
        vec![vault_formats::ModLayer::new("empty", None, None, None)],
    ));
    let mut modded = host(layered);
    assert!(modded.overrides_allowed());
    let _ = modded.load_campaign();
    assert_eq!(modded.world.rules.tables().map_town(0), 5);

    // A malformed override is an error and leaves the compiled tables.
    fs::write(dir.path().join("data/vault-engine/maps.txt"), "0|X").unwrap();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut broken = host(vfs);
    broken.set_preset(EnginePreset::Enhanced);
    assert!(broken.load_table_overrides().is_err());
    assert!(broken.world.rules.tables.is_none());
}

#[test]
fn mod_patches_replace_records_over_the_override_in_load_order() {
    let dir = corpus();
    // Two layers patch the maps table: record 0 by both (the higher wins)
    // and record 1 by the lower only.
    for (layer, patch) in [
        (
            "low",
            "0|0|DESERT1.MAP|6|07desert\n1|1|DESERT2.MAP|3|07desert\n",
        ),
        ("high", "# mine\n0|0|DESERT1.MAP|7|07desert\n"),
    ] {
        let folder = dir.path().join("mods").join(layer);
        fs::create_dir_all(folder.join("data").join(layer)).unwrap();
        fs::write(
            folder.join("data").join(layer).join("maps.patch.txt"),
            patch,
        )
        .unwrap();
        fs::write(
            folder.join("vault-mod.toml"),
            format!(
                "name = \"{layer}\"\n[[patches]]\ntable = \"maps\"\nfile = \"data/{layer}/maps.patch.txt\"\n"
            ),
        )
        .unwrap();
    }
    let mount = || -> Arc<dyn Vfs> {
        let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let entry = |path: &str| vault_formats::LayerEntry {
            path: Some(path.into()),
            ..Default::default()
        };
        Arc::new(
            vault_formats::LayeredVfs::from_entries(
                base,
                dir.path(),
                &[entry("mods/low"), entry("mods/high")],
                vault_formats::LoadOrderPolicy::default(),
            )
            .unwrap(),
        )
    };
    let mut modded = host(mount());
    assert_eq!(modded.load_table_overrides().unwrap(), [Table::Maps]);
    let tables = modded.world.rules.tables();
    assert_eq!(tables.map_town(0), 7);
    assert_eq!(tables.map_town(1), 3);
    assert_eq!(tables.map_town(2), worldmap::map_town(2));

    // Without the whole-file override, the patches apply to the compiled
    // table.
    fs::remove_file(dir.path().join("data/vault-engine/maps.txt")).unwrap();
    let mut patched = host(mount());
    assert_eq!(patched.load_table_overrides().unwrap(), [Table::Maps]);
    assert_eq!(patched.world.rules.tables().map_town(0), 7);

    // A record past the table, or an unknown table, is an error and leaves
    // the compiled tables.
    fs::write(
        dir.path().join("mods/high/data/high/maps.patch.txt"),
        "99|x",
    )
    .unwrap();
    let mut broken = host(mount());
    assert!(broken.load_table_overrides().is_err());
    assert!(broken.world.rules.tables.is_none());
    fs::write(
        dir.path().join("mods/high/vault-mod.toml"),
        "name = \"high\"\n[[patches]]\ntable = \"cities\"\nfile = \"data/high/maps.patch.txt\"\n",
    )
    .unwrap();
    let mut unknown = host(mount());
    let error = unknown.load_table_overrides().unwrap_err().to_string();
    assert!(error.contains("no campaign table `cities`"), "{error}");
}
