//! SPEC-018 §8 fixtures: the synthetic data tables of §8.2-§8.7, §8.12 and
//! §8.14, and a synthetic Fallout 2 install built over the synthetic
//! Fallout 1 corpus (`state::write_corpus`), with a start map
//! `maps/artemple.map` and the tables in `data/`.

/// §8.2 the grammar sample.
pub const GRAMMAR: &str = include_str!("grammar.txt");
/// §8.3 `data/maps.txt`.
pub const MAPS: &str = include_str!("maps.txt");
/// §8.4 `data/city.txt`.
pub const CITY: &str = include_str!("city.txt");
/// §8.5 `data/worldmap.txt`.
pub const WORLDMAP: &str = include_str!("worldmap.txt");
/// §8.6 `data/party.txt`.
pub const PARTY: &str = include_str!("party.txt");
/// §8.7 `data/quests.txt` and `data/holodisk.txt`.
pub const QUESTS: &str = include_str!("quests.txt");
pub const HOLODISKS: &str = include_str!("holodisk.txt");
/// §8.14 `data/genrep.txt` and `data/endgame.txt`.
pub const GENREP: &str = include_str!("genrep.txt");
pub const ENDGAME: &str = include_str!("endgame.txt");
/// §8.12 `data/vault-engine/elevators.txt`.
pub const ELEVATORS: &str = include_str!("elevators.txt");

/// The four required tables and the two Pip-Boy tables, by VFS path.
pub fn tables() -> Vec<(&'static str, &'static str)> {
    vec![
        ("data/maps.txt", MAPS),
        ("data/city.txt", CITY),
        ("data/worldmap.txt", WORLDMAP),
        ("data/party.txt", PARTY),
        ("data/quests.txt", QUESTS),
        ("data/holodisk.txt", HOLODISKS),
    ]
}

/// A reader over `files` for `Fallout2Tables::load`.
pub fn reader(
    files: Vec<(&'static str, &'static str)>,
) -> impl FnMut(&str) -> Result<Option<Vec<u8>>, String> {
    move |path: &str| {
        Ok(files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.as_bytes().to_vec()))
    }
}

fn word(bytes: &mut [u8], at: usize, value: i32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A copy of SPEC-003 map stream `map` (the synthetic interactive map)
/// whose header names `name`, map script `script` (a one-based
/// `scripts.lst` reference, 0 for none), and the entrance hex `hex`,
/// elevation 0 and orientation `orientation`.
pub fn named_map(mut map: Vec<u8>, name: &str, script: i32, hex: i32, orientation: i32) -> Vec<u8> {
    // Header: version, name[16], entry hex, elevation, orientation, local
    // count, script reference (SPEC-003 §2.7).
    let mut field = [0u8; 16];
    field[..name.len()].copy_from_slice(name.as_bytes());
    map[4..20].copy_from_slice(&field);
    word(&mut map, 20, hex);
    word(&mut map, 24, 0);
    word(&mut map, 28, orientation);
    word(&mut map, 36, script);
    map
}

/// Write SPEC-018's tables (`tables()`) into a synthetic corpus at `root`
/// with three maps made from its `maps/synthetic.map` by [`named_map`]:
/// §8.13's `maps/artemple.map` (map script reference 1, entrance hex
/// 20100, orientation 2), and `maps/testvil.map` and `maps/desert1.map`
/// at the synthetic map's entrance.
pub fn write_tables(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).unwrap();
    for (path, text) in tables() {
        std::fs::write(root.join(path), text).unwrap();
    }
    let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
    for (file, name, script, orientation) in [
        ("maps/artemple.map", "artemple.map", 1, 2),
        ("maps/testvil.map", "testvil.map", 0, 0),
        ("maps/desert1.map", "desert1.map", 0, 0),
    ] {
        std::fs::write(
            root.join(file),
            named_map(map.clone(), name, script, 20100, orientation),
        )
        .unwrap();
    }
}
