//! Compile the approved SPEC-007 critical tables and the SPEC-008 fixed
//! world catalogs into exhaustive, checked data, plus the same data as
//! canonical `campaign_<table>.txt` text (the format `tables.rs` parses and
//! a mod may override through the VFS).
use std::{env, fs, path::PathBuf};

/// Write one canonical table text into OUT_DIR: a comment header naming
/// its fields, then one `|`-separated record per line.
fn canonical(name: &str, fields: &str, lines: &[String]) {
    let mut text = format!("# vault-engine campaign table: {name}\n# {fields}\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join(format!("campaign_{name}.txt")),
        text,
    )
    .unwrap();
}

/// One slash-separated matrix entry: only the second and fifth values are hex.
fn entry(cell: &str) -> [i64; 7] {
    let fields: Vec<_> = cell.split('/').collect();
    assert_eq!(
        fields.len(),
        7,
        "critical entry {cell:?} needs seven values"
    );
    std::array::from_fn(|index| {
        let field = fields[index];
        if matches!(index, 1 | 4) {
            i64::from_str_radix(field, 16).expect("hexadecimal effect mask")
        } else {
            field.parse().expect("decimal critical value")
        }
    })
}

fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text.find(heading).expect("SPEC-007 section");
    let rest = &text[start + heading.len()..];
    &rest[..rest.find("\n### ").unwrap_or(rest.len())]
}

fn cells(line: &str) -> Vec<&str> {
    line.split('|').map(str::trim).collect()
}

fn main() {
    let spec = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../specs/approved/SPEC-007-critter-fid-pathfinding-combat.md");
    println!("cargo:rerun-if-changed={}", spec.display());
    let text = fs::read_to_string(spec).expect("approved SPEC-007 must be present");

    // Section 2.5: victim tables 0-14 and the player matrix P (index 15),
    // locations 0-8, tiers 0-5. `=` repeats the entire entry to its left.
    let mut matrices = vec![vec![None; 9]; 16];
    for line in section(&text, "### 2.5").lines() {
        let row = cells(line);
        if row.len() != 10 {
            continue;
        }
        let victim = match row[1] {
            "P" => 15,
            value => match value.parse::<usize>() {
                Ok(victim) if victim <= 14 => victim,
                _ => continue,
            },
        };
        let location: usize = row[2].parse().expect("location number");
        assert!(location <= 8, "location 0-8");
        let mut tiers: Vec<[i64; 7]> = Vec::with_capacity(6);
        for cell in &row[3..9] {
            let value = if *cell == "=" {
                *tiers.last().expect("`=` needs an entry to its left")
            } else {
                entry(cell)
            };
            tiers.push(value);
        }
        assert!(
            matrices[victim][location].replace(tiers).is_none(),
            "duplicate critical row {victim}/{location}"
        );
    }
    let mut source =
        String::from("pub static CRITICAL_SUCCESS: [[[CriticalEffect; 6]; 9]; 16] = [\n");
    let mut count = 0;
    let mut critical_text = Vec::new();
    for (victim, locations) in matrices.iter().enumerate() {
        source.push_str("[\n");
        for (location, tiers) in locations.iter().enumerate() {
            let tiers = tiers
                .as_ref()
                .unwrap_or_else(|| panic!("missing critical row {victim}/{location}"));
            source.push('[');
            for (
                tier,
                [
                    multiplier,
                    flags,
                    stat,
                    adjustment,
                    failed,
                    message,
                    alternate,
                ],
            ) in tiers.iter().enumerate()
            {
                count += 1;
                critical_text.push(format!(
                    "{victim}|{location}|{tier}|{multiplier}|{flags:x}|{stat}|{adjustment}|\
                     {failed:x}|{message}|{alternate}"
                ));
                source.push_str(&format!(
                    "CriticalEffect{{multiplier:{multiplier},flags:{flags:#x},save_stat:{stat},\
                     save_adjustment:{adjustment},failed_flags:{failed:#x},message:{message},\
                     failed_message:{alternate}}},"
                ));
            }
            source.push_str("],\n");
        }
        source.push_str("],\n");
    }
    source.push_str("];\n");
    assert_eq!(count, 864, "all 864 critical entries must be present");
    canonical(
        "critical",
        "victim|location|tier|multiplier|flags (hex)|save stat|save adjustment|failed flags (hex)|message|failed message",
        &critical_text,
    );

    // Section 2.4: critical-failure masks, tables 0-6 by columns 0-4.
    let mut failures = Vec::new();
    let mut failure_text = Vec::new();
    let failure = section(&text, "### 2.4");
    let table = &failure[failure
        .find("| Failure table number")
        .expect("failure table")..];
    for line in table.lines().skip(2) {
        let row = cells(line);
        if row.len() != 8 {
            break;
        }
        assert_eq!(row[1].parse::<usize>().ok(), Some(failures.len()));
        let masks: Vec<_> = row[2..7]
            .iter()
            .map(|mask| format!("{:x}", u32::from_str_radix(mask, 16).expect("hex mask")))
            .collect();
        failure_text.push(format!("{}|{}", failures.len(), masks.join("|")));
        failures.push(
            row[2..7]
                .iter()
                .map(|mask| format!("{:#x}", u32::from_str_radix(mask, 16).expect("hex mask")))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    assert_eq!(failures.len(), 7, "failure tables 0-6");
    canonical(
        "critical_failure",
        "table|column 0|column 1|column 2|column 3|column 4 (hex masks)",
        &failure_text,
    );
    source.push_str(&format!(
        "pub const CRITICAL_FAILURE: [[u32; 5]; 7] = [{}];\n",
        failures
            .iter()
            .map(|row| format!("[{row}]"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("critical_tables.rs"),
        source,
    )
    .unwrap();
    world_tables();
}

/// SPEC-008 Section 3.6 grids and the Section 3.7 town and hotspot catalogs.
fn world_tables() {
    let spec = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../specs/approved/SPEC-008-dat-saves-and-worldmap.md");
    println!("cargo:rerun-if-changed={}", spec.display());
    let text = fs::read_to_string(spec).expect("approved SPEC-008 must be present");

    let mut grids: [Vec<String>; 3] = Default::default();
    let mut grid_text: [Vec<String>; 3] = Default::default();
    for line in section(&text, "### 3.6").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(row) = fields.first().and_then(|row| row.parse::<usize>().ok()) else {
            continue;
        };
        assert_eq!(fields.len(), 4, "grid row {row} needs three column groups");
        assert_eq!(row, grids[0].len(), "grid rows are in order");
        for ((grid, lines), digits) in grids.iter_mut().zip(&mut grid_text).zip(&fields[1..]) {
            assert_eq!(digits.len(), 28, "grid row {row} has 28 columns");
            lines.push(format!("{row}|{}", digits.to_ascii_uppercase()));
            let cells: Vec<_> = digits
                .chars()
                .map(|digit| digit.to_digit(16).expect("grid digit").to_string())
                .collect();
            grid.push(format!("[{}]", cells.join(",")));
        }
    }
    let mut source = String::new();
    for (name, lines) in ["terrain", "frequency", "region"].iter().zip(&grid_text) {
        canonical(
            name,
            "row|28 hexadecimal cell digits, column 0 first",
            lines,
        );
    }
    for (name, grid) in ["TERRAIN", "FREQUENCY", "REGION"].iter().zip(&grids) {
        assert_eq!(grid.len(), 30, "grid {name} has 30 rows");
        source.push_str(&format!(
            "pub static {name}: [[u8; 28]; 30] = [{}];\n",
            grid.join(",")
        ));
    }

    let towns = section(&text, "### 3.7");
    let mut rows = Vec::new();
    let mut town_names = Vec::new();
    let mut town_text = Vec::new();
    for line in towns.lines() {
        let row = cells(line);
        if row.len() != 7 {
            continue;
        }
        let Ok(id) = row[1].parse::<usize>() else {
            continue;
        };
        assert_eq!(id, rows.len(), "towns are in id order");
        town_names.push(row[2].to_owned());
        let (column, cell_row) = row[3].split_once(',').expect("cell column,row");
        let entrances: Vec<usize> = row[5]
            .split(',')
            .map(|index| index.parse().expect("entrance campaign index"))
            .collect();
        town_text.push(format!(
            "{id}|{}|{}|{}|{}|{}",
            row[2],
            column.parse::<usize>().expect("cell column"),
            cell_row.parse::<usize>().expect("cell row"),
            row[4].parse::<usize>().expect("known-town index"),
            entrances
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ));
        rows.push(format!(
            "Town{{name:{:?},cell:[{},{}],known_global:{},entrance_globals:&{:?}}}",
            row[2],
            column.parse::<usize>().expect("cell column"),
            cell_row.parse::<usize>().expect("cell row"),
            row[4].parse::<usize>().expect("known-town index"),
            entrances,
        ));
    }
    assert_eq!(rows.len(), 12, "towns 0-11");
    canonical(
        "towns",
        "id|name|cell column|cell row|known-town global|entrance globals (comma-separated)",
        &town_text,
    );
    source.push_str(&format!(
        "pub static TOWNS: [Town; 12] = [{}];\n",
        rows.join(",")
    ));

    let catalog = &towns[towns.find("```text").expect("hotspot catalog") + 7..];
    let catalog = &catalog[..catalog.find("```").expect("catalog end")];
    let mut hotspots = Vec::new();
    let mut hotspot_text = Vec::new();
    for line in catalog.lines().filter(|line| !line.trim().is_empty()) {
        let (row, slots) = line.split_once(": ").expect("hotspot row");
        assert_eq!(row.parse::<usize>().ok(), Some(hotspots.len()));
        let mut entries = vec![String::from("Hotspot::EMPTY"); 7];
        for slot in slots.split("; ") {
            let (index, fields) = slot.split_once(':').expect("slot index");
            let index: usize = index.parse().expect("slot number");
            let fields: Vec<_> = fields.split('/').collect();
            assert_eq!(fields.len(), 4, "x/y/entry/filename");
            hotspot_text.push(format!(
                "{}|{index}|{}|{}|{}|{}",
                hotspots.len(),
                fields[0].parse::<i32>().expect("anchor x"),
                fields[1].parse::<i32>().expect("anchor y"),
                fields[2].parse::<i32>().expect("entry selector"),
                fields[3],
            ));
            entries[index] = format!(
                "Hotspot{{x:{},y:{},entry:{},map:{:?}}}",
                fields[0].parse::<i32>().expect("anchor x"),
                fields[1].parse::<i32>().expect("anchor y"),
                fields[2].parse::<i32>().expect("entry selector"),
                fields[3],
            );
        }
        hotspots.push(format!("[{}]", entries.join(",")));
    }
    assert_eq!(hotspots.len(), 15, "hotspot rows 0-14");
    canonical(
        "hotspots",
        "knowledge row|slot|x|y|entry selector|map file (listed slots only)",
        &hotspot_text,
    );
    source.push_str(&format!(
        "pub static HOTSPOTS: [[Hotspot; 7]; 15] = [{}];\n",
        hotspots.join(",")
    ));
    source.push_str(&map_table(&text, &town_names));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("world_tables.rs"),
        source,
    )
    .unwrap();
}

/// SPEC-008 Section 3.10: the 66 map numbers with their `map.msg` text,
/// fixed town and city music track. The table is exact data, so any change
/// of its shape fails the build.
fn map_table(text: &str, town_names: &[String]) -> String {
    let mut rows = Vec::new();
    let mut map_text = Vec::new();
    for line in section(text, "### 3.10").lines() {
        let row = cells(line);
        if row.len() != 6 {
            continue;
        }
        let Ok(number) = row[1].parse::<usize>() else {
            continue;
        };
        assert_eq!(number, rows.len(), "map numbers are in order");
        let name = row[2].trim_matches('`');
        assert!(
            name.ends_with(".MAP") && name == name.to_ascii_uppercase() && name.len() <= 12,
            "map {number} needs an uppercase 8.3 `.MAP` name, not {name:?}"
        );
        let (town, label) = row[3].split_once(' ').expect("town id and name");
        let town: usize = town.parse().expect("town id");
        assert!(
            town_names
                .get(town)
                .is_some_and(|known| known.starts_with(label)),
            "map {number} names town {town} {label:?}, which Section 3.7 does not"
        );
        let music = row[4].trim_matches('`');
        assert!(
            !music.is_empty() && music.is_ascii() && !music.contains(['/', '\\', '.']),
            "map {number} needs a bare track name, not {music:?}"
        );
        map_text.push(format!("{number}|{name}|{town}|{music}"));
        rows.push(format!(
            "MapEntry{{name:{name:?},town:{town},music:{music:?}}}"
        ));
    }
    assert_eq!(rows.len(), 66, "maps 0-65");
    canonical(
        "maps",
        "number|map.msg name|town|city music track",
        &map_text,
    );
    format!(
        "pub static MAP_TABLE: [MapEntry; 66] = [{}];\n",
        rows.join(",")
    )
}
