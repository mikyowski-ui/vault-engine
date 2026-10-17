//! Compile the approved SPEC-007 critical tables and the SPEC-008 fixed
//! world catalogs into exhaustive, checked data.
use std::{env, fs, path::PathBuf};

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
    for (victim, locations) in matrices.iter().enumerate() {
        source.push_str("[\n");
        for (location, tiers) in locations.iter().enumerate() {
            let tiers = tiers
                .as_ref()
                .unwrap_or_else(|| panic!("missing critical row {victim}/{location}"));
            source.push('[');
            for [
                multiplier,
                flags,
                stat,
                adjustment,
                failed,
                message,
                alternate,
            ] in tiers
            {
                count += 1;
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

    // Section 2.4: critical-failure masks, tables 0-6 by columns 0-4.
    let mut failures = Vec::new();
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
        failures.push(
            row[2..7]
                .iter()
                .map(|mask| format!("{:#x}", u32::from_str_radix(mask, 16).expect("hex mask")))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    assert_eq!(failures.len(), 7, "failure tables 0-6");
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
    for line in section(&text, "### 3.6").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(row) = fields.first().and_then(|row| row.parse::<usize>().ok()) else {
            continue;
        };
        assert_eq!(fields.len(), 4, "grid row {row} needs three column groups");
        assert_eq!(row, grids[0].len(), "grid rows are in order");
        for (grid, digits) in grids.iter_mut().zip(&fields[1..]) {
            assert_eq!(digits.len(), 28, "grid row {row} has 28 columns");
            let cells: Vec<_> = digits
                .chars()
                .map(|digit| digit.to_digit(16).expect("grid digit").to_string())
                .collect();
            grid.push(format!("[{}]", cells.join(",")));
        }
    }
    let mut source = String::new();
    for (name, grid) in ["TERRAIN", "FREQUENCY", "REGION"].iter().zip(&grids) {
        assert_eq!(grid.len(), 30, "grid {name} has 30 rows");
        source.push_str(&format!(
            "pub static {name}: [[u8; 28]; 30] = [{}];\n",
            grid.join(",")
        ));
    }

    let towns = section(&text, "### 3.7");
    let mut rows = Vec::new();
    for line in towns.lines() {
        let row = cells(line);
        if row.len() != 7 {
            continue;
        }
        let Ok(id) = row[1].parse::<usize>() else {
            continue;
        };
        assert_eq!(id, rows.len(), "towns are in id order");
        let (column, cell_row) = row[3].split_once(',').expect("cell column,row");
        let entrances: Vec<usize> = row[5]
            .split(',')
            .map(|index| index.parse().expect("entrance campaign index"))
            .collect();
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
    source.push_str(&format!(
        "pub static TOWNS: [Town; 12] = [{}];\n",
        rows.join(",")
    ));

    let catalog = &towns[towns.find("```text").expect("hotspot catalog") + 7..];
    let catalog = &catalog[..catalog.find("```").expect("catalog end")];
    let mut hotspots = Vec::new();
    for line in catalog.lines().filter(|line| !line.trim().is_empty()) {
        let (row, slots) = line.split_once(": ").expect("hotspot row");
        assert_eq!(row.parse::<usize>().ok(), Some(hotspots.len()));
        let mut entries = vec![String::from("Hotspot::EMPTY"); 7];
        for slot in slots.split("; ") {
            let (index, fields) = slot.split_once(':').expect("slot index");
            let index: usize = index.parse().expect("slot number");
            let fields: Vec<_> = fields.split('/').collect();
            assert_eq!(fields.len(), 4, "x/y/entry/filename");
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
    source.push_str(&format!(
        "pub static HOTSPOTS: [[Hotspot; 7]; 15] = [{}];\n",
        hotspots.join(",")
    ));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("world_tables.rs"),
        source,
    )
    .unwrap();
}
