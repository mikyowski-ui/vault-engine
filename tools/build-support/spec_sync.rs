//! What the `table_sync` tests share: finding a section of an approved spec,
//! and comparing the rows read from it with a committed table. Only tests
//! include this, with `tsv_table.rs` beside it: no build reads a spec.
// Each including target uses part of this file.
#![allow(dead_code)]

use super::tsv_table::{Row, Table};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

/// How many differences one failure lists.
const MOST_DIFFERENCES: usize = 20;

/// The workspace root. `CARGO_MANIFEST_DIR` is read when the test runs.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("run under cargo")).join("../..")
}

/// The text of `file`, from the workspace root.
pub fn read(file: &str) -> String {
    fs::read_to_string(workspace_root().join(file))
        .unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// The cells of one Markdown table line.
pub fn cells(line: &str) -> Vec<&str> {
    line.split('|').map(str::trim).collect()
}

/// The lines of the section numbered exactly `number` (`### 3.8 Title`, never
/// `### 3.80`), up to the next heading of level 3 or higher. `spec` names the
/// spec in messages.
pub fn section(spec: &str, text: &str, number: &str) -> Result<String, String> {
    let prefix = format!("### {number}");
    let headings: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            line.strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .map(|(index, _)| index)
        .collect();
    let [heading] = headings[..] else {
        return Err(format!(
            "{spec} section {number}: {} headings, expected exactly one",
            headings.len()
        ));
    };
    Ok(text
        .lines()
        .skip(heading + 1)
        .take_while(|line| !line.starts_with("### ") && !line.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// One row as a spec states it. `section` is the full place to name on a
/// failure, such as `SPEC-007 section 2.5`, and `cells` line up with the
/// table's columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecRow {
    pub section: String,
    pub cells: Vec<String>,
}

/// The spec section `row` comes from: its own `section` column, in a table with
/// one, or else the `source` of the whole table.
fn row_source(row: &Row, source: &str) -> String {
    match (row.text("section"), source.split_once(' ')) {
        (Ok(number), Some((spec, _))) => format!("{spec} section {number}"),
        _ => source.to_owned(),
    }
}

/// Compare a table with the rows of its spec, row by row. The first `keys`
/// columns identify a row, and `source` names the sections of the whole table.
/// Each difference is one line of the error, and names the spec section and the
/// table's `file:line`.
pub fn compare(
    table: &Table,
    keys: usize,
    source: &str,
    spec_rows: &[SpecRow],
) -> Result<(), String> {
    let mut stated: BTreeMap<&[String], &SpecRow> = BTreeMap::new();
    let mut differences = Vec::new();
    let columns = table.columns();
    for row in spec_rows {
        assert_eq!(
            row.cells.len(),
            columns.len(),
            "a spec row fills each column"
        );
        if stated.insert(&row.cells[..keys], row).is_some() {
            differences.push(format!(
                "{}: {} is stated twice",
                row.section,
                row.cells[..keys].join(" ")
            ));
        }
    }
    let key_text = |cells: &[String]| {
        columns[..keys]
            .iter()
            .zip(cells)
            .map(|(column, cell)| format!("{column} {cell}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    for row in table.rows() {
        let Some(spec_row) = stated.remove(&row.cells()[..keys]) else {
            differences.push(format!(
                "{} ({}): the spec has no such row, but {}",
                row_source(&row, source),
                key_text(row.cells()),
                row.error("the table has it"),
            ));
            continue;
        };
        for (index, column) in columns.iter().enumerate().skip(keys) {
            let (spec_cell, table_cell) = (&spec_row.cells[index], &row.cells()[index]);
            if spec_cell != table_cell {
                differences.push(format!(
                    "{} ({}): column `{column}`: the spec says {spec_cell:?}, but {}",
                    spec_row.section,
                    key_text(&spec_row.cells),
                    row.error(format_args!("the table says {table_cell:?}")),
                ));
            }
        }
    }
    for (_, spec_row) in stated {
        differences.push(format!(
            "{} ({}): the spec has this row, but {} does not",
            spec_row.section,
            key_text(&spec_row.cells),
            table.file(),
        ));
    }
    if differences.is_empty() {
        return Ok(());
    }
    let total = differences.len();
    differences.truncate(MOST_DIFFERENCES);
    Err(format!(
        "Differences between a spec and its table ({total}); change them together:\n{}",
        differences.join("\n")
    ))
}
