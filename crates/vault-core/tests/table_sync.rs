//! Holds the `oracle/SPEC-007/` and `oracle/SPEC-008/` tables to the sections
//! of the specs they come from. The build reads only the tables, so this is
//! what fails when a spec changes and its table does not.
#[path = "../../../tools/build-support/spec_sync.rs"]
mod spec_sync;
#[path = "../../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use spec_sync::{SpecRow, cells, compare, read, section};
use tsv_table::Table;

const SPEC_007: &str = "specs/approved/SPEC-007-critter-fid-pathfinding-combat.md";
const SPEC_008: &str = "specs/approved/SPEC-008-dat-saves-and-worldmap.md";
const SUCCESS: &str = "oracle/SPEC-007/critical-success.tsv";
const FAILURE: &str = "oracle/SPEC-007/critical-failure.tsv";
const GRIDS: &str = "oracle/SPEC-008/world-grids.tsv";
const TOWNS: &str = "oracle/SPEC-008/towns.tsv";
const HOTSPOTS: &str = "oracle/SPEC-008/hotspots.tsv";
const MAPS: &str = "oracle/SPEC-008/maps.tsv";
/// The victim column's number for the section's row `P`.
const PLAYER_MATRIX: usize = 15;

fn rows(section: &str, spec_rows: Vec<Vec<String>>) -> Vec<SpecRow> {
    spec_rows
        .into_iter()
        .map(|cells| SpecRow {
            section: section.to_owned(),
            cells,
        })
        .collect()
}

/// Hexadecimal text as the tables write it.
fn hex(place: &str, text: &str) -> Result<String, String> {
    u32::from_str_radix(text, 16)
        .map(|value| format!("{value:X}"))
        .map_err(|error| format!("{place}: mask {text:?}: {error}"))
}

/// A decimal value as the tables write it.
fn decimal(place: &str, text: &str) -> Result<String, String> {
    text.parse::<i64>()
        .map(|value| value.to_string())
        .map_err(|error| format!("{place}: value {text:?}: {error}"))
}

/// One slash-separated matrix entry: only the second and fifth values are hex.
fn entry(place: &str, cell: &str) -> Result<Vec<String>, String> {
    let fields: Vec<_> = cell.split('/').collect();
    if fields.len() != 7 {
        return Err(format!("{place}: entry {cell:?} needs seven values"));
    }
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            if matches!(index, 1 | 4) {
                hex(place, field)
            } else {
                decimal(place, field)
            }
        })
        .collect()
}

/// Section 2.5: victim tables 0-14 and the player matrix P, locations 0-8,
/// tiers 0-5. `=` repeats the entire entry to its left.
fn success_rows(text: &str) -> Result<Vec<SpecRow>, String> {
    const SECTION: &str = "SPEC-007 section 2.5";
    let body = section("SPEC-007", text, "2.5")?;
    let mut found = Vec::new();
    for line in body.lines() {
        let row = cells(line);
        if row.len() != 10 {
            continue;
        }
        let victim = match row[1] {
            "P" => PLAYER_MATRIX,
            value => match value.parse::<usize>() {
                Ok(victim) if victim < PLAYER_MATRIX => victim,
                _ => continue,
            },
        };
        let location = decimal(SECTION, row[2])?;
        let mut previous: Option<Vec<String>> = None;
        for (tier, cell) in row[3..9].iter().enumerate() {
            let place = format!("{SECTION} (victim {victim}, location {location}, tier {tier})");
            let fields = if *cell == "=" {
                previous
                    .clone()
                    .ok_or_else(|| format!("{place}: `=` needs an entry to its left"))?
            } else {
                entry(&place, cell)?
            };
            let mut cells = vec![victim.to_string(), location.clone(), tier.to_string()];
            cells.extend(fields.iter().cloned());
            found.push(cells);
            previous = Some(fields);
        }
    }
    Ok(rows(SECTION, found))
}

/// Section 2.4: critical-failure masks, tables 0-6 by columns 0-4.
fn failure_rows(text: &str) -> Result<Vec<SpecRow>, String> {
    const SECTION: &str = "SPEC-007 section 2.4";
    let body = section("SPEC-007", text, "2.4")?;
    let start = body
        .find("| Failure table number")
        .ok_or_else(|| format!("{SECTION}: no failure table"))?;
    let mut found = Vec::new();
    for line in body[start..].lines().skip(2) {
        let row = cells(line);
        if row.len() != 8 {
            break;
        }
        let mut cells = vec![decimal(SECTION, row[1])?];
        for mask in &row[2..7] {
            cells.push(hex(SECTION, mask)?);
        }
        found.push(cells);
    }
    Ok(rows(SECTION, found))
}

/// Section 3.6: a row number, then the digits of three grids.
fn grid_rows(text: &str) -> Result<Vec<SpecRow>, String> {
    const SECTION: &str = "SPEC-008 section 3.6";
    let body = section("SPEC-008", text, "3.6")?;
    let mut found = Vec::new();
    for line in body.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(row) = fields.first().and_then(|row| row.parse::<usize>().ok()) else {
            continue;
        };
        if fields.len() != 4 {
            return Err(format!(
                "{SECTION}: grid row {row} needs three column groups"
            ));
        }
        let mut cells = vec![row.to_string()];
        cells.extend(fields[1..].iter().map(|digits| digits.to_ascii_uppercase()));
        found.push(cells);
    }
    Ok(rows(SECTION, found))
}

/// The cells of section 3.7's town table, in row order.
fn town_cells(body: &str) -> Result<Vec<Vec<String>>, String> {
    let mut found = Vec::new();
    for line in body.lines() {
        let row = cells(line);
        if row.len() != 7 || row[1].parse::<usize>().is_err() {
            continue;
        }
        let (column, cell_row) = row[3]
            .split_once(',')
            .ok_or_else(|| format!("SPEC-008 section 3.7: town {}: cell column,row", row[1]))?;
        found.push(
            [row[1], row[2], column, cell_row, row[4], row[5]]
                .map(str::to_owned)
                .to_vec(),
        );
    }
    Ok(found)
}

/// Section 3.7's hotspot catalog: `row: slot:X/Y/entry-selector/filename; ...`.
fn hotspot_cells(body: &str) -> Result<Vec<Vec<String>>, String> {
    const SECTION: &str = "SPEC-008 section 3.7";
    let start = body
        .find("```text")
        .ok_or_else(|| format!("{SECTION}: no hotspot catalog"))?;
    let catalog = &body[start + "```text".len()..];
    let end = catalog
        .find("```")
        .ok_or_else(|| format!("{SECTION}: no end to the catalog"))?;
    let mut found = Vec::new();
    for line in catalog[..end]
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let (row, slots) = line
            .split_once(": ")
            .ok_or_else(|| format!("{SECTION}: hotspot row {line:?}"))?;
        for slot in slots.split("; ") {
            let (index, fields) = slot
                .split_once(':')
                .ok_or_else(|| format!("{SECTION}: hotspot slot {slot:?}"))?;
            let fields: Vec<_> = fields.split('/').collect();
            if fields.len() != 4 {
                return Err(format!("{SECTION}: {slot:?} needs x/y/entry/filename"));
            }
            let mut cells = vec![decimal(SECTION, row)?, index.to_owned()];
            cells.extend(fields.iter().map(|field| (*field).to_owned()));
            found.push(cells);
        }
    }
    Ok(found)
}

/// Section 3.10's rows. Each names its town, which must be `town_names`' row.
fn map_rows(text: &str, town_names: &[String]) -> Result<Vec<SpecRow>, String> {
    const SECTION: &str = "SPEC-008 section 3.10";
    let body = section("SPEC-008", text, "3.10")?;
    let mut found = Vec::new();
    for line in body.lines() {
        let row = cells(line);
        if row.len() != 6 || row[1].parse::<usize>().is_err() {
            continue;
        }
        let number = row[1];
        let (town, label) = row[3]
            .split_once(' ')
            .ok_or_else(|| format!("{SECTION}: map {number} needs a town id and name"))?;
        let known = town
            .parse::<usize>()
            .ok()
            .and_then(|town| town_names.get(town));
        if !known.is_some_and(|known| known.starts_with(label)) {
            return Err(format!(
                "{SECTION}: map {number} names town {town} {label:?}, \
                 which SPEC-008 section 3.7 does not"
            ));
        }
        found.push(vec![
            number.to_owned(),
            row[2].trim_matches('`').to_owned(),
            town.to_owned(),
            row[4].trim_matches('`').to_owned(),
        ]);
    }
    Ok(rows(SECTION, found))
}

/// `table_text` holds what `spec_rows` state.
fn agree(
    file: &str,
    table_text: &str,
    source: &str,
    columns: &[&str],
    keys: usize,
    spec_rows: &[SpecRow],
) -> Result<(), String> {
    let table = Table::parse(file, table_text, Some(source), columns)?;
    compare(&table, keys, source, spec_rows)
}

/// Every difference between SPEC-007 and its tables.
fn check_007(spec: &str) -> Vec<String> {
    let success = success_rows(spec).and_then(|rows| {
        let columns = [
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
        ];
        agree(
            SUCCESS,
            &read(SUCCESS),
            "SPEC-007 section 2.5",
            &columns,
            3,
            &rows,
        )
    });
    let failure = failure_rows(spec).and_then(|rows| {
        let columns = [
            "table", "column_0", "column_1", "column_2", "column_3", "column_4",
        ];
        agree(
            FAILURE,
            &read(FAILURE),
            "SPEC-007 section 2.4",
            &columns,
            1,
            &rows,
        )
    });
    [success, failure]
        .into_iter()
        .filter_map(Result::err)
        .collect()
}

/// Every difference between SPEC-008 and the world tables.
fn check_008(spec: &str) -> Vec<String> {
    let grids = grid_rows(spec).and_then(|rows| {
        let columns = ["row", "terrain", "frequency", "region"];
        agree(
            GRIDS,
            &read(GRIDS),
            "SPEC-008 section 3.6",
            &columns,
            1,
            &rows,
        )
    });
    // Each comparison runs whatever another finds, so one failure lists them all.
    let town_section = "SPEC-008 section 3.7";
    let body = section("SPEC-008", spec, "3.7");
    let towns = body.clone().and_then(|body| town_cells(&body));
    let town_table = towns.clone().and_then(|towns| {
        let columns = [
            "id",
            "name",
            "column",
            "row",
            "known_global",
            "entrance_globals",
        ];
        let towns = rows(town_section, towns);
        agree(TOWNS, &read(TOWNS), town_section, &columns, 1, &towns)
    });
    let hotspots = body.and_then(|body| {
        let columns = ["knowledge_row", "slot", "x", "y", "entry", "map"];
        let hotspots = rows(town_section, hotspot_cells(&body)?);
        agree(
            HOTSPOTS,
            &read(HOTSPOTS),
            town_section,
            &columns,
            2,
            &hotspots,
        )
    });
    let maps = towns.and_then(|towns| {
        let names: Vec<_> = towns.iter().map(|town| town[1].clone()).collect();
        let columns = ["number", "name", "town", "music"];
        let maps = map_rows(spec, &names)?;
        agree(
            MAPS,
            &read(MAPS),
            "SPEC-008 section 3.10",
            &columns,
            1,
            &maps,
        )
    });
    [grids, town_table, hotspots, maps]
        .into_iter()
        .filter_map(Result::err)
        .collect()
}

fn replaced(text: &str, from: &str, to: &str) -> String {
    let edited = text.replacen(from, to, 1);
    assert_ne!(text, edited, "{from} is in the text");
    edited
}

/// Each `(from, to)` edit of `spec` must fail `check`, naming `wanted`.
fn each_edit_fails(spec: &str, check: fn(&str) -> Vec<String>, edits: &[(&str, &str, &str, &str)]) {
    for (from, to, place, column) in edits {
        let edited = replaced(spec, from, to);
        let message = check(&edited).join("\n");
        assert!(message.contains(place), "{from} -> {to}: {message}");
        assert!(message.contains(column), "{from} -> {to}: {message}");
    }
}

#[test]
fn the_tables_say_what_spec_007_says() {
    let differences = check_007(&read(SPEC_007));
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn the_tables_say_what_spec_008_says() {
    let differences = check_008(&read(SPEC_008));
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn a_spec_007_edit_that_misses_a_table_fails_and_names_the_section() {
    each_edit_fails(
        &read(SPEC_007),
        check_007,
        &[
            (
                "| 0 | 0 | 4/0/-1/0/0/5001/5000 |",
                "| 0 | 0 | 5/0/-1/0/0/5001/5000 |",
                "SPEC-007 section 2.5 (victim 0, location 0, tier 0",
                "column `multiplier`",
            ),
            (
                // The entry an `=` repeats: tier 3, and tier 4 after it.
                "4/810/-1/0/0/5012/5000 | =",
                "4/811/-1/0/0/5012/5000 | =",
                "tier 4",
                "column `flags`",
            ),
            (
                "| 0 | 0 | 8000 | 8000 | 80002 | 200000 |",
                "| 0 | 0 | 8000 | 8000 | 80003 | 200000 |",
                "SPEC-007 section 2.4",
                "column `column_3`",
            ),
        ],
    );
}

#[test]
fn a_spec_008_edit_that_misses_a_table_fails_and_names_the_section() {
    each_edit_fails(
        &read(SPEC_008),
        check_008,
        &[
            (
                "00  1110000001111111111100000000",
                "00  1110000001111111111100000001",
                "SPEC-008 section 3.6",
                "column `terrain`",
            ),
            (
                "| 3 | Junktown | 17,10 |",
                "| 3 | Junktown | 17,11 |",
                "SPEC-008 section 3.7",
                "column `row`",
            ),
            (
                "0:400/317/3/JUNKENT.MAP",
                "0:400/317/3/JUNKGATE.MAP",
                "SPEC-008 section 3.7",
                "column `map`",
            ),
            (
                "| 0 | `DESERT1.MAP` | 1 Vault 15 | `07desert` |",
                "| 0 | `DESERT1.MAP` | 1 Vault 15 | `08desert` |",
                "SPEC-008 section 3.10",
                "column `music`",
            ),
        ],
    );
}

#[test]
fn one_failure_lists_the_differences_of_every_section() {
    let spec = read(SPEC_008);
    let edited = replaced(
        &spec,
        "| 3 | Junktown | 17,10 |",
        "| 3 | Junktown | 17,11 |",
    );
    let edited = replaced(
        &edited,
        "0:400/317/3/JUNKENT.MAP",
        "0:400/317/3/JUNKGATE.MAP",
    );
    let edited = replaced(
        &edited,
        "| 0 | `DESERT1.MAP` | 1 Vault 15 | `07desert` |",
        "| 0 | `DESERT1.MAP` | 1 Vault 15 | `08desert` |",
    );
    let message = check_008(&edited).join("\n");
    for wanted in [TOWNS, HOTSPOTS, MAPS, "SPEC-008 section 3.10"] {
        assert!(message.contains(wanted), "{wanted}: {message}");
    }
}

#[test]
fn a_map_of_a_town_section_3_7_does_not_have_is_an_error() {
    let edited = replaced(
        &read(SPEC_008),
        "| 0 | `DESERT1.MAP` | 1 Vault 15 |",
        "| 0 | `DESERT1.MAP` | 1 Shady Sands |",
    );
    let message = check_008(&edited).join("\n");
    assert!(message.contains("SPEC-008 section 3.10"), "{message}");
    assert!(message.contains("section 3.7 does not"), "{message}");
}

#[test]
fn a_row_the_spec_drops_is_a_row_the_table_may_not_keep() {
    let spec = read(SPEC_008);
    let edited: String = spec
        .lines()
        .filter(|line| !line.starts_with("| 11 | Cathedral "))
        .map(|line| format!("{line}\n"))
        .collect();
    let message = check_008(&edited).join("\n");
    assert!(message.contains("SPEC-008 section 3.7"), "{message}");
    assert!(message.contains("the spec has no such row"), "{message}");
    assert!(message.contains(TOWNS), "{message}");
}

#[test]
fn a_table_edit_that_misses_the_spec_fails_and_names_the_section() {
    let table = replaced(&read(FAILURE), "\n3\t8000\t", "\n3\t8001\t");
    let spec_rows = failure_rows(&read(SPEC_007)).expect("the failure table");
    let columns = [
        "table", "column_0", "column_1", "column_2", "column_3", "column_4",
    ];
    let message = agree(
        FAILURE,
        &table,
        "SPEC-007 section 2.4",
        &columns,
        1,
        &spec_rows,
    )
    .expect_err("the copies disagree");
    assert!(message.contains("SPEC-007 section 2.4"), "{message}");
    assert!(message.contains("the spec says \"8000\""), "{message}");
}
