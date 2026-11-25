//! Holds `oracle/SPEC-008/holodisks.tsv` and `quests.tsv` to SPEC-008 section
//! 3.8. The build reads only the tables, so this is what fails when the spec
//! changes and they do not.
#[path = "../../../tools/build-support/spec_sync.rs"]
mod spec_sync;
#[path = "../../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use spec_sync::{SpecRow, cells, compare, read, section};
use tsv_table::Table;

const SPEC: &str = "specs/approved/SPEC-008-dat-saves-and-worldmap.md";
const DISKS: &str = "oracle/SPEC-008/holodisks.tsv";
const QUESTS: &str = "oracle/SPEC-008/quests.tsv";
const SOURCE: &str = "SPEC-008 section 3.8";

/// The words after `prefix`, on the line that starts with it.
fn numbers<'a>(text: &'a str, prefix: &str) -> Result<Vec<&'a str>, String> {
    let line = text
        .lines()
        .find(|line| line.starts_with(prefix))
        .ok_or_else(|| format!("{SOURCE}: no {prefix} line"))?;
    Ok(line[prefix.len()..].split_whitespace().collect())
}

fn row(cells: Vec<String>) -> SpecRow {
    SpecRow {
        section: SOURCE.to_owned(),
        cells,
    }
}

/// The holodisk rows: the `Ordinal:` and `Global:` lines, zipped.
fn disk_rows(text: &str) -> Result<Vec<SpecRow>, String> {
    let (ordinals, globals) = (numbers(text, "Ordinal:")?, numbers(text, "Global:")?);
    if ordinals.len() != globals.len() {
        return Err(format!(
            "{SOURCE}: {} ordinals but {} globals",
            ordinals.len(),
            globals.len()
        ));
    }
    Ok(ordinals
        .iter()
        .zip(&globals)
        .map(|(ordinal, global)| row(vec![(*ordinal).to_owned(), (*global).to_owned()]))
        .collect())
}

/// The quest rows, with `None` written `-` as the table writes it.
fn quest_rows(text: &str) -> Vec<SpecRow> {
    text.lines()
        .filter_map(|line| {
            let cells = cells(line);
            if cells.len() != 4 {
                return None;
            }
            let town = cells[1].parse::<usize>().ok()?;
            let globals = if cells[2] == "None" { "-" } else { cells[2] };
            Some(row(vec![town.to_string(), globals.to_owned()]))
        })
        .collect()
}

/// Whether the two tables say what `spec` says.
fn check(spec: &str, disks: &str, quests: &str) -> Result<(), String> {
    let text = section("SPEC-008", spec, "3.8")?;
    let disks = Table::parse(DISKS, disks, Some(SOURCE), &["ordinal", "global"])?;
    let quests = Table::parse(QUESTS, quests, Some(SOURCE), &["town", "globals"])?;
    compare(&disks, 1, SOURCE, &disk_rows(&text)?)?;
    compare(&quests, 1, SOURCE, &quest_rows(&text))
}

fn tables() -> (String, String) {
    (read(DISKS), read(QUESTS))
}

fn replaced(text: &str, from: &str, to: &str) -> String {
    let edited = text.replacen(from, to, 1);
    assert_ne!(text, edited, "{from} is in the text");
    edited
}

#[test]
fn the_tables_say_what_spec_008_section_3_8_says() {
    let (disks, quests) = tables();
    check(&read(SPEC), &disks, &quests).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_spec_edit_that_misses_a_table_fails_and_names_the_section() {
    let (disks, quests) = tables();
    let spec = read(SPEC);
    let holodisk = replaced(&spec, "Global:  162 ", "Global:  169 ");
    let quest = replaced(&spec, "| 5 | 29,31 |", "| 5 | 29,32 |");
    for (edited, table, column) in [
        (holodisk, DISKS, "column `global`"),
        (quest, QUESTS, "column `globals`"),
    ] {
        let message = check(&edited, &disks, &quests).expect_err("the copies disagree");
        assert!(message.contains(SOURCE), "{message}");
        assert!(message.contains(column), "{message}");
        assert!(message.contains(table), "{message}");
    }
}

#[test]
fn a_table_edit_that_misses_the_spec_fails_and_names_the_section() {
    let (disks, quests) = tables();
    let edited = replaced(&quests, "\n5\t29,31\n", "\n5\t29\n");
    let message = check(&read(SPEC), &disks, &edited).expect_err("the copies disagree");
    assert!(message.contains(SOURCE), "{message}");
    assert!(message.contains("the spec says \"29,31\""), "{message}");
}

#[test]
fn the_section_is_found_by_its_exact_number() {
    let text = "### 3.80 Not this\nno\n### 3.8 This\nyes\n#### 3.8.1 Inside\nyes\n## 4 Next\nno";
    assert_eq!(
        section("SPEC-X", text, "3.8"),
        Ok("yes\n#### 3.8.1 Inside\nyes".to_owned())
    );
    assert!(section("SPEC-X", text, "3.9").is_err());
}
