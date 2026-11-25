//! Holds `oracle/SPEC-005/host-opcodes.tsv` to SPEC-005. The build reads only
//! the table, so this is what fails when the spec changes and the table does
//! not: it reads the Markdown, derives each row, and compares them all.
#[path = "../../../tools/build-support/spec_sync.rs"]
mod spec_sync;
#[path = "../../../tools/build-support/tsv_table.rs"]
mod tsv_table;

use spec_sync::{SpecRow, cells, compare, read};
use tsv_table::{Table, parse_word};

const SPEC: &str = "specs/approved/SPEC-005-host-opcodes.md";
const TABLE: &str = "oracle/SPEC-005/host-opcodes.tsv";
const SOURCE: &str = "SPEC-005 sections 2.3 to 2.10";
const COLUMNS: [&str; 9] = [
    "word",
    "section",
    "name",
    "operands",
    "spec_operands",
    "push",
    "spec_return",
    "registered",
    "variable",
];
/// The one word SPEC-005 gives a variable operand count.
const VARIABLE_WORD: u16 = 0x807f;

/// The `Rule` of one operand of a Pop cell, by the symbols of section 2.1:
/// "Unmarked operands are I; an explicit suffix such as `text S` overrides
/// that default." A trailing token that looks like a symbol but is not one is
/// an error, not an unmarked operand.
fn operand_rule(operand: &str) -> Result<&'static str, String> {
    if operand.contains("S/0/-1") {
        return Ok("ScrollFourth");
    }
    if operand.contains("S or I zero") {
        return Ok("Previous");
    }
    if operand.contains("I or S") || operand.contains("S or I") {
        return Ok("IntegerOrString");
    }
    // Any other choice of types, such as `value F or S`: its last symbol alone
    // would read as a plain `String`.
    if operand.contains(" or ") {
        return Err(format!("unknown operand type in {operand:?}"));
    }
    let last = operand
        .split_whitespace()
        .last()
        .ok_or("an empty operand")?;
    match last {
        "I" => Ok("Integer"),
        "S" => Ok("String"),
        "F" => Ok("Float"),
        "U" => Ok("Unchecked"),
        "O" => Ok("Optional"),
        "N" => Ok("Filename"),
        "C" => Ok("FillColor"),
        "T" => Ok("TextColor"),
        _ if is_symbol(last) => Err(format!("unknown operand type in {operand:?}")),
        _ => Ok("Integer"),
    }
}

/// Whether `token` is written as symbols, such as `X` or `S/F`, not a word.
fn is_symbol(token: &str) -> bool {
    token
        .split('/')
        .all(|part| part.len() == 1 && part.chars().all(|symbol| symbol.is_ascii_uppercase()))
}

/// The push kind of a Return cell.
fn push_kind(cell: &str) -> Result<&'static str, String> {
    match cell.chars().next() {
        _ if cell == "none" => Ok("None"),
        Some('D') => Ok("Text"),
        Some('I') => Ok("Value"),
        _ => Err(format!("unknown return type in {cell:?}")),
    }
}

/// The rows of the host words `0x804C..=0x8155`, wherever the spec has them.
fn spec_rows(text: &str) -> Result<Vec<SpecRow>, String> {
    let mut rows = Vec::new();
    let mut number = "";
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            number = heading.split_whitespace().next().unwrap_or("");
        }
        if !line.starts_with("| `0x") {
            continue;
        }
        let row = cells(line);
        // Rows of other shapes, or of other words, are not host rows.
        let Some(word) = row
            .get(1)
            .and_then(|cell| parse_word(cell.trim_matches('`')))
        else {
            continue;
        };
        if !(0x804c..=0x8155).contains(&word) {
            continue;
        }
        let section = format!("SPEC-005 section {number}");
        if row.len() < 7 {
            return Err(format!("{section}, word {word:#06x}: a short row"));
        }
        let name = row[2].trim_matches('`');
        let operands = if row[3] == "none" {
            "-".to_owned()
        } else {
            row[3]
                .split(',')
                .map(operand_rule)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{section}, word {word:#06x}: {error}"))?
                .join(",")
        };
        let push =
            push_kind(row[4]).map_err(|error| format!("{section}, word {word:#06x}: {error}"))?;
        let flag = |value: bool| if value { "1" } else { "0" }.to_owned();
        rows.push(SpecRow {
            section,
            cells: vec![
                format!("0x{word:04X}"),
                number.to_owned(),
                name.to_owned(),
                operands,
                row[3].to_owned(),
                push.to_owned(),
                row[4].to_owned(),
                flag(name != "Unregistered"),
                flag(word == VARIABLE_WORD),
            ],
        });
    }
    Ok(rows)
}

/// Whether `spec`'s text and `table`'s text agree.
fn check(spec: &str, table: &str) -> Result<(), String> {
    let table = Table::parse(TABLE, table, Some(SOURCE), &COLUMNS)?;
    compare(&table, 1, SOURCE, &spec_rows(spec)?)
}

/// `text` with the line holding `marker` changed by `edit`.
fn edit_line(text: &str, marker: &str, edit: impl Fn(&str) -> String) -> String {
    let line = text
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("no line with {marker}"));
    let edited = edit(line);
    assert_ne!(line, edited, "the edit changes the line");
    text.replacen(line, &edited, 1)
}

fn failure(spec: &str, table: &str) -> String {
    check(spec, table).expect_err("the copies disagree")
}

#[test]
fn the_table_says_what_spec_005_says() {
    check(&read(SPEC), &read(TABLE)).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_spec_edit_that_misses_the_table_fails_and_names_the_section() {
    let (spec, table) = (read(SPEC), read(TABLE));
    let edited = edit_line(&spec, "`0x80B1`", |line| {
        line.replacen("ignored value", "ignored value S", 1)
    });
    let message = failure(&edited, &table);
    assert!(message.contains("SPEC-005 section 2.8"), "{message}");
    assert!(message.contains("column `operands`"), "{message}");
    assert!(message.contains(TABLE), "{message}");

    // A word the spec drops, and the row the table keeps for it.
    let edited: String = spec
        .lines()
        .filter(|line| !line.contains("`0x8105` | `message_str`"))
        .map(|line| format!("{line}\n"))
        .collect();
    let message = failure(&edited, &table);
    assert!(message.contains("SPEC-005 section 2.10"), "{message}");
    assert!(message.contains("the spec has no such row"), "{message}");

    // A row moved to another section is the same row, in the wrong place.
    let edited = edit_line(&spec, "### 2.8 ", |line| {
        line.replace("### 2.8 ", "### 2.11 ")
    });
    let message = failure(&edited, &table);
    assert!(message.contains("SPEC-005 section 2.11"), "{message}");
    assert!(message.contains("column `section`"), "{message}");
}

#[test]
fn a_table_edit_that_misses_the_spec_fails_and_names_the_section() {
    let (spec, table) = (read(SPEC), read(TABLE));
    let edited = edit_line(&table, "0x80B1\t", |line| {
        line.replacen("\tInteger\t", "\tString\t", 1)
    });
    let message = failure(&spec, &edited);
    assert!(message.contains("SPEC-005 section 2.8"), "{message}");
    assert!(message.contains("the spec says \"Integer\""), "{message}");
}

#[test]
fn an_operand_of_no_known_type_is_an_error_not_an_integer() {
    for operand in ["value X", "value D", "value S/F", "value F or S"] {
        let message = operand_rule(operand).expect_err(operand);
        assert!(message.contains(operand), "{message}");
    }
    assert_eq!(operand_rule(" "), Err("an empty operand".to_owned()));

    let spec = read(SPEC);
    let edited = edit_line(&spec, "`0x80B1`", |line| {
        line.replacen("ignored value", "ignored value X", 1)
    });
    let message = failure(&edited, &read(TABLE));
    assert!(message.contains("SPEC-005 section 2.8"), "{message}");
    assert!(message.contains("unknown operand type"), "{message}");
}

#[test]
fn an_unmarked_operand_is_an_integer_by_section_2_1() {
    let spec = read(SPEC);
    assert!(
        spec.contains("Unmarked operands are I"),
        "section 2.1's rule"
    );
    for operand in ["object", "PID", "first value", "then repeated y and x"] {
        assert_eq!(operand_rule(operand), Ok("Integer"), "{operand}");
    }
    assert_eq!(operand_rule("damage type/flags"), Ok("Integer"));
    assert_eq!(operand_rule("filename I"), Ok("Integer"));
}

#[test]
fn a_return_of_no_known_type_is_an_error() {
    assert_eq!(push_kind("none"), Ok("None"));
    assert_eq!(push_kind("I or D"), Ok("Value"));
    assert_eq!(push_kind("D text"), Ok("Text"));
    assert!(push_kind("F value").is_err());
}
