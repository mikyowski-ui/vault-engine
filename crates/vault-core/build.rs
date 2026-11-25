//! Compile the critical tables of SPEC-007 and the fixed world catalogs of
//! SPEC-008 into exhaustive, checked data, plus the same data as canonical
//! `campaign_<table>.txt` text (the format `tables.rs` parses and a mod may
//! override through the VFS). The input is their committed form under
//! `oracle/`. No build reads a spec: `tests/table_sync.rs` holds these tables
//! to the specs' text.
#[path = "../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use std::{env, fs, path::PathBuf};
use tsv_table::{Row, Table, build_input};

const SUCCESS_TIERS: usize = 6;
const SUCCESS_LOCATIONS: usize = 9;
const SUCCESS_VICTIMS: usize = 16;
const HOTSPOT_ROWS: usize = 15;
const HOTSPOT_SLOTS: usize = 7;
const GRID_DIGITS: usize = 28;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    critical_tables()?;
    world_tables()
}

/// A file of OUT_DIR.
fn output(name: &str, contents: String) -> Result<(), String> {
    let out = env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    fs::write(PathBuf::from(out).join(name), contents).map_err(|error| format!("{name}: {error}"))
}

/// Write one canonical table text into OUT_DIR: a comment header naming
/// its fields, then one `|`-separated record per line.
fn canonical(name: &str, fields: &str, lines: &[String]) -> Result<(), String> {
    let mut text = format!("# vault-engine campaign table: {name}\n# {fields}\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    output(&format!("campaign_{name}.txt"), text)
}

/// The victim, location and tier of `row`, the entry numbered `index`: these
/// columns must count up through every entry of the matrices.
fn critical_entry(row: &Row, index: usize) -> Result<[usize; 3], String> {
    let place = [
        row.parse("victim")?,
        row.parse("location")?,
        row.parse("tier")?,
    ];
    let wanted = [
        index / (SUCCESS_LOCATIONS * SUCCESS_TIERS),
        index / SUCCESS_TIERS % SUCCESS_LOCATIONS,
        index % SUCCESS_TIERS,
    ];
    if place != wanted {
        return Err(row.error(format_args!(
            "victim, location and tier must be {wanted:?} here, found {place:?}"
        )));
    }
    Ok(place)
}

/// SPEC-007 sections 2.4 and 2.5.
fn critical_tables() -> Result<(), String> {
    let table = build_input(
        "oracle/SPEC-007/critical-success.tsv",
        Some("SPEC-007 section 2.5"),
        &[
            "victim",
            "location",
            "tier",
            "multiplier",
            "flags",
            "save_stat",
            "save_adjustment",
            "failed_flags",
            "message",
            "failed_message",
        ],
    )?;
    let mut source =
        String::from("pub static CRITICAL_SUCCESS: [[[CriticalEffect; 6]; 9]; 16] = [\n");
    let mut critical_text = Vec::new();
    for (index, row) in table.rows().enumerate() {
        let [victim, location, tier] = critical_entry(&row, index)?;
        let multiplier: i64 = row.parse("multiplier")?;
        let flags = row.hex("flags")?;
        let stat: i64 = row.parse("save_stat")?;
        let adjustment: i64 = row.parse("save_adjustment")?;
        let failed = row.hex("failed_flags")?;
        let message: i64 = row.parse("message")?;
        let alternate: i64 = row.parse("failed_message")?;
        if location == 0 && tier == 0 {
            source.push_str("[\n");
        }
        if tier == 0 {
            source.push('[');
        }
        critical_text.push(format!(
            "{victim}|{location}|{tier}|{multiplier}|{flags:x}|{stat}|{adjustment}|\
             {failed:x}|{message}|{alternate}"
        ));
        source.push_str(&format!(
            "CriticalEffect{{multiplier:{multiplier},flags:{flags:#x},save_stat:{stat},\
             save_adjustment:{adjustment},failed_flags:{failed:#x},message:{message},\
             failed_message:{alternate}}},"
        ));
        if tier + 1 == SUCCESS_TIERS {
            source.push_str("],\n");
            if location + 1 == SUCCESS_LOCATIONS {
                source.push_str("],\n");
            }
        }
    }
    source.push_str("];\n");
    table.expect_rows(SUCCESS_VICTIMS * SUCCESS_LOCATIONS * SUCCESS_TIERS)?;
    canonical(
        "critical",
        "victim|location|tier|multiplier|flags (hex)|save stat|save adjustment|failed flags (hex)|message|failed message",
        &critical_text,
    )?;

    const MASKS: [&str; 5] = ["column_0", "column_1", "column_2", "column_3", "column_4"];
    let table = build_input(
        "oracle/SPEC-007/critical-failure.tsv",
        Some("SPEC-007 section 2.4"),
        &["table", MASKS[0], MASKS[1], MASKS[2], MASKS[3], MASKS[4]],
    )?;
    let mut failures = Vec::new();
    let mut failure_text = Vec::new();
    for row in table.rows() {
        let number: usize = row.parse("table")?;
        if number != failures.len() {
            return Err(row.column_error("table", "failure tables run 0 to 6 in order"));
        }
        let masks = MASKS
            .iter()
            .map(|column| row.hex(column))
            .collect::<Result<Vec<_>, _>>()?;
        let text: Vec<_> = masks.iter().map(|mask| format!("{mask:x}")).collect();
        failure_text.push(format!("{number}|{}", text.join("|")));
        let hex: Vec<_> = masks.iter().map(|mask| format!("{mask:#x}")).collect();
        failures.push(hex.join(","));
    }
    table.expect_rows(7)?;
    canonical(
        "critical_failure",
        "table|column 0|column 1|column 2|column 3|column 4 (hex masks)",
        &failure_text,
    )?;
    source.push_str(&format!(
        "pub const CRITICAL_FAILURE: [[u32; 5]; 7] = [{}];\n",
        failures
            .iter()
            .map(|row| format!("[{row}]"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    output("critical_tables.rs", source)
}

/// One row's digits of a grid, as array elements.
fn grid_cells(row: &Row, column: &str) -> Result<(String, String), String> {
    let digits = row.text(column)?;
    if digits.len() != GRID_DIGITS {
        return Err(row.column_error(
            column,
            format_args!("{} digits, but a row has {GRID_DIGITS}", digits.len()),
        ));
    }
    let cells = digits
        .chars()
        .map(|digit| match digit.to_digit(16) {
            Some(value) if !digit.is_ascii_lowercase() => Ok(value.to_string()),
            _ => Err(row.column_error(column, format_args!("{digit:?} is no grid digit"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((format!("[{}]", cells.join(",")), digits.to_owned()))
}

/// SPEC-008 Section 3.6 grids and the Section 3.7 town and hotspot catalogs.
fn world_tables() -> Result<(), String> {
    const GRIDS: [(&str, &str, &str); 3] = [
        ("terrain", "terrain", "TERRAIN"),
        ("frequency", "frequency", "FREQUENCY"),
        ("region", "region", "REGION"),
    ];
    let table = build_input(
        "oracle/SPEC-008/world-grids.tsv",
        Some("SPEC-008 section 3.6"),
        &["row", GRIDS[0].1, GRIDS[1].1, GRIDS[2].1],
    )?;
    let mut grids: [Vec<String>; 3] = Default::default();
    let mut grid_text: [Vec<String>; 3] = Default::default();
    for (index, row) in table.rows().enumerate() {
        let number: usize = row.parse("row")?;
        if number != index {
            return Err(row.column_error("row", "grid rows are in order"));
        }
        for ((grid, lines), (_, column, _)) in grids.iter_mut().zip(&mut grid_text).zip(GRIDS) {
            let (cells, digits) = grid_cells(&row, column)?;
            lines.push(format!("{number}|{digits}"));
            grid.push(cells);
        }
    }
    table.expect_rows(30)?;
    let mut source = String::new();
    for ((name, _, _), lines) in GRIDS.iter().zip(&grid_text) {
        canonical(
            name,
            "row|28 hexadecimal cell digits, column 0 first",
            lines,
        )?;
    }
    for ((_, _, name), grid) in GRIDS.iter().zip(&grids) {
        source.push_str(&format!(
            "pub static {name}: [[u8; 28]; 30] = [{}];\n",
            grid.join(",")
        ));
    }

    let table = build_input(
        "oracle/SPEC-008/towns.tsv",
        Some("SPEC-008 section 3.7"),
        &[
            "id",
            "name",
            "column",
            "row",
            "known_global",
            "entrance_globals",
        ],
    )?;
    source.push_str(&towns(&table)?);
    source.push_str(&hotspots()?);
    source.push_str(&map_table(table.rows().count())?);
    output("world_tables.rs", source)
}

/// The `TOWNS` static, and the canonical `towns` text.
fn towns(table: &Table) -> Result<String, String> {
    let mut rows = Vec::new();
    let mut town_text = Vec::new();
    for row in table.rows() {
        let id: usize = row.parse("id")?;
        if id != rows.len() {
            return Err(row.column_error("id", "towns are in id order"));
        }
        let name = row.text("name")?;
        let column: usize = row.parse("column")?;
        let cell_row: usize = row.parse("row")?;
        let known: usize = row.parse("known_global")?;
        let entrances = row
            .list("entrance_globals")?
            .into_iter()
            .map(|index| {
                index.parse::<usize>().map_err(|error| {
                    row.column_error("entrance_globals", format_args!("{index:?}: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let listed: Vec<_> = entrances.iter().map(usize::to_string).collect();
        town_text.push(format!(
            "{id}|{name}|{column}|{cell_row}|{known}|{}",
            listed.join(",")
        ));
        rows.push(format!(
            "Town{{name:{name:?},cell:[{column},{cell_row}],known_global:{known},\
             entrance_globals:&{entrances:?}}}"
        ));
    }
    table.expect_rows(12)?;
    canonical(
        "towns",
        "id|name|cell column|cell row|known-town global|entrance globals (comma-separated)",
        &town_text,
    )?;
    Ok(format!(
        "pub static TOWNS: [Town; 12] = [{}];\n",
        rows.join(",")
    ))
}

/// The `HOTSPOTS` static, and the canonical `hotspots` text.
fn hotspots() -> Result<String, String> {
    let table = build_input(
        "oracle/SPEC-008/hotspots.tsv",
        Some("SPEC-008 section 3.7"),
        &["knowledge_row", "slot", "x", "y", "entry", "map"],
    )?;
    let mut hotspots = vec![vec![String::from("Hotspot::EMPTY"); HOTSPOT_SLOTS]; HOTSPOT_ROWS];
    let mut hotspot_text = Vec::new();
    let mut previous = None;
    for row in table.rows() {
        let place: (usize, usize) = (row.parse("knowledge_row")?, row.parse("slot")?);
        let (knowledge, slot) = place;
        if knowledge >= HOTSPOT_ROWS || slot >= HOTSPOT_SLOTS {
            return Err(row.error(format_args!(
                "knowledge rows run to {} and slots to {}",
                HOTSPOT_ROWS - 1,
                HOTSPOT_SLOTS - 1
            )));
        }
        if previous.is_some_and(|before| before >= place) {
            return Err(row.error("hotspots are in row and slot order"));
        }
        previous = Some(place);
        let x: i32 = row.parse("x")?;
        let y: i32 = row.parse("y")?;
        let entry: i32 = row.parse("entry")?;
        let map = row.text("map")?;
        hotspot_text.push(format!("{knowledge}|{slot}|{x}|{y}|{entry}|{map}"));
        hotspots[knowledge][slot] = format!("Hotspot{{x:{x},y:{y},entry:{entry},map:{map:?}}}");
    }
    if let Some(empty) = hotspots
        .iter()
        .position(|row| row.iter().all(|slot| slot == "Hotspot::EMPTY"))
    {
        return Err(format!(
            "{}: knowledge row {empty} has no hotspot",
            table.file()
        ));
    }
    canonical(
        "hotspots",
        "knowledge row|slot|x|y|entry selector|map file (listed slots only)",
        &hotspot_text,
    )?;
    let rows: Vec<_> = hotspots
        .iter()
        .map(|row| format!("[{}]", row.join(",")))
        .collect();
    Ok(format!(
        "pub static HOTSPOTS: [[Hotspot; 7]; 15] = [{}];\n",
        rows.join(",")
    ))
}

/// SPEC-008 Section 3.10: the 66 map numbers with their `map.msg` text,
/// fixed town and city music track. `towns` counts the towns of Section 3.7.
fn map_table(towns: usize) -> Result<String, String> {
    let table = build_input(
        "oracle/SPEC-008/maps.tsv",
        Some("SPEC-008 section 3.10"),
        &["number", "name", "town", "music"],
    )?;
    let mut rows = Vec::new();
    let mut map_text = Vec::new();
    for row in table.rows() {
        let number: usize = row.parse("number")?;
        if number != rows.len() {
            return Err(row.column_error("number", "map numbers are in order"));
        }
        let name = row.text("name")?;
        if !(name.ends_with(".MAP") && name == name.to_ascii_uppercase() && name.len() <= 12) {
            return Err(row.column_error(
                "name",
                format_args!("map {number} needs an uppercase 8.3 `.MAP` name, not {name:?}"),
            ));
        }
        let town: usize = row.parse("town")?;
        if town >= towns {
            return Err(row.column_error("town", format_args!("no town {town}")));
        }
        let music = row.text("music")?;
        if !music.is_ascii() || music.contains(['/', '\\', '.']) {
            return Err(row.column_error(
                "music",
                format_args!("map {number} needs a bare track name, not {music:?}"),
            ));
        }
        map_text.push(format!("{number}|{name}|{town}|{music}"));
        rows.push(format!(
            "MapEntry{{name:{name:?},town:{town},music:{music:?}}}"
        ));
    }
    table.expect_rows(66)?;
    canonical(
        "maps",
        "number|map.msg name|town|city music track",
        &map_text,
    )?;
    Ok(format!(
        "pub static MAP_TABLE: [MapEntry; 66] = [{}];\n",
        rows.join(",")
    ))
}
