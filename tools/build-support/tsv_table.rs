//! The reader of the committed TSV tables under `oracle/`, shared by the build
//! scripts that compile them and the `table_sync` tests that hold them to the
//! approved specs. It is included by `#[path]`, not a crate: a build script
//! here has no dependencies.
//!
//! Every failure is a `Result` whose message starts with `file:line`, so a
//! wrong table stops the build at the row to fix.
// Each including target uses part of this file.
#![allow(dead_code)]

use std::{env, fmt::Display, fs, path::PathBuf, str::FromStr};

/// A parsed table: its header was checked, and every row has every column.
#[derive(Debug)]
pub struct Table {
    file: String,
    columns: Vec<String>,
    rows: Vec<(usize, Vec<String>)>,
}

impl Table {
    /// Parse `text`, named `file` in messages. With `source`, the file must
    /// start with a `# <source>: ...` comment, and may carry more `#` comments
    /// before its column row. Without it, as in the older SPEC-024 tables, the
    /// column row comes first.
    pub fn parse(
        file: &str,
        text: &str,
        source: Option<&str>,
        columns: &[&str],
    ) -> Result<Self, String> {
        if let Some(index) = text.lines().position(|line| line.contains('\r')) {
            return Err(format!("{file}:{}: a carriage return", index + 1));
        }
        let mut lines = text.lines().enumerate().peekable();
        if let Some(source) = source {
            let first = lines.peek().map_or("", |(_, line)| line);
            if !first.starts_with(&format!("# {source}:")) {
                return Err(format!(
                    "{file}:1: the header must start with `# {source}:`, found {first:?}"
                ));
            }
            while lines.next_if(|(_, line)| line.starts_with('#')).is_some() {}
        }
        let (index, header) = lines
            .next()
            .ok_or_else(|| format!("{file}: no column row"))?;
        let wanted = columns.join("\t");
        if header != wanted {
            return Err(format!(
                "{file}:{}: the columns must be {wanted:?}, found {header:?}",
                index + 1
            ));
        }
        let mut rows = Vec::new();
        for (index, line) in lines {
            let line_number = index + 1;
            let cells: Vec<String> = line.split('\t').map(str::to_owned).collect();
            if cells.len() != columns.len() {
                return Err(format!(
                    "{file}:{line_number}: {} cells, but the table has {} columns",
                    cells.len(),
                    columns.len()
                ));
            }
            for (cell, column) in cells.iter().zip(columns) {
                if cell.is_empty() || cell.trim() != cell {
                    return Err(format!(
                        "{file}:{line_number}: column `{column}`: {cell:?} is empty or \
                         has spaces at its ends (`-` means none)"
                    ));
                }
            }
            rows.push((line_number, cells));
        }
        Ok(Self {
            file: file.to_owned(),
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            rows,
        })
    }

    /// The table's file, as messages name it.
    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.rows.iter().map(|(line, cells)| Row {
            table: self,
            line: *line,
            cells,
        })
    }

    /// Exactly `count` rows, or a message saying why not.
    pub fn expect_rows(&self, count: usize) -> Result<(), String> {
        if self.rows.len() == count {
            return Ok(());
        }
        let last = self.rows.last().map_or(1, |(line, _)| *line);
        Err(format!(
            "{}:{last}: {} rows, but the table must have {count}",
            self.file,
            self.rows.len()
        ))
    }
}

/// One data row, for typed reads that name its `file:line` when they fail.
#[derive(Clone, Copy)]
pub struct Row<'a> {
    table: &'a Table,
    line: usize,
    cells: &'a [String],
}

impl<'a> Row<'a> {
    pub fn line(&self) -> usize {
        self.line
    }

    pub fn cells(&self) -> &'a [String] {
        self.cells
    }

    /// `message`, placed at this row.
    pub fn error(&self, message: impl Display) -> String {
        format!("{}:{}: {message}", self.table.file, self.line)
    }

    /// `message`, placed at this row's `column`.
    pub fn column_error(&self, column: &str, message: impl Display) -> String {
        self.error(format_args!("column `{column}`: {message}"))
    }

    /// The text of `column`.
    pub fn text(&self, column: &str) -> Result<&'a str, String> {
        let index = self
            .table
            .columns
            .iter()
            .position(|name| name == column)
            .ok_or_else(|| self.error(format_args!("no column `{column}`")))?;
        Ok(&self.cells[index])
    }

    /// `column` as a `T`. A leading `+` is refused: no table writes one.
    pub fn parse<T>(&self, column: &str) -> Result<T, String>
    where
        T: FromStr,
        T::Err: Display,
    {
        let text = self.text(column)?;
        if text.starts_with('+') {
            return Err(self.column_error(column, format_args!("{text:?} has a sign")));
        }
        text.parse()
            .map_err(|error| self.column_error(column, format_args!("{text:?}: {error}")))
    }

    /// `column` as bare hexadecimal digits, in upper case.
    pub fn hex(&self, column: &str) -> Result<u32, String> {
        let text = self.text(column)?;
        if !text
            .chars()
            .all(|digit| matches!(digit, '0'..='9' | 'A'..='F'))
        {
            return Err(self.column_error(
                column,
                format_args!("{text:?} is not upper-case hexadecimal"),
            ));
        }
        u32::from_str_radix(text, 16)
            .map_err(|error| self.column_error(column, format_args!("{text:?}: {error}")))
    }

    /// `column` as a `0x` word of four digits.
    pub fn word(&self, column: &str) -> Result<u16, String> {
        let text = self.text(column)?;
        parse_word(text)
            .ok_or_else(|| self.column_error(column, format_args!("{text:?} is not a 0xNNNN word")))
    }

    /// `column`, which must be one of `allowed`.
    pub fn one_of(&self, column: &str, allowed: &[&str]) -> Result<&'a str, String> {
        let text = self.text(column)?;
        if allowed.contains(&text) {
            return Ok(text);
        }
        Err(self.column_error(
            column,
            format_args!("unknown value {text:?}, expected one of {allowed:?}"),
        ))
    }

    /// `column` as a flag written `1` or `0`.
    pub fn flag(&self, column: &str) -> Result<bool, String> {
        Ok(self.one_of(column, &["0", "1"])? == "1")
    }

    /// The items of a comma-separated `column`, with `-` for none.
    pub fn list(&self, column: &str) -> Result<Vec<&'a str>, String> {
        let text = self.text(column)?;
        if text == "-" {
            return Ok(Vec::new());
        }
        Ok(text.split(',').collect())
    }
}

/// `0xNNNN`, in either case.
pub fn parse_word(text: &str) -> Option<u16> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() != 4 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(digits, 16).ok()
}

/// A build script's read of `file`, from the workspace root: asks for a rerun
/// when the file changes, then parses it as [`Table::parse`] does.
pub fn build_input(file: &str, source: Option<&str>, columns: &[&str]) -> Result<Table, String> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| format!("{file}: CARGO_MANIFEST_DIR is not set"))?;
    let path = PathBuf::from(manifest).join("../..").join(file);
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).map_err(|error| format!("{file}: {error}"))?;
    Table::parse(file, &text, source, columns)
}
