//! SPEC-021 §3.10 the replay checkpoint stream: one row per headless step,
//! recording the covered state at the end of that step, so two runs show not
//! only that they diverged, but at which step.
//!
//! The row writer, the reader and the comparison live here, beside the hash,
//! so that every tool shares one implementation. Nothing here reads a file:
//! a tool hands over text and gets lines back.
use crate::{
    WorldState,
    state_hash::{COMBAT_MAGIC, WORLD_MAGIC},
};
use std::fmt;

/// The format's name, field 1 of the version line.
pub const CHECKPOINT_FORMAT: &str = "vault-checkpoints";
/// The format's version, field 2 of the version line.
pub const CHECKPOINT_VERSION: u32 = 1;
/// Line 2 of a stream.
pub const CHECKPOINT_HEADER: &str = "tick\tinput_index\tstate_hash\tcombat_hash";
/// The columns of a row, in order.
const COLUMNS: [&str; 4] = ["tick", "input_index", "state_hash", "combat_hash"];

/// Line 1 of a stream. The magics are the encoder's own, so a change of
/// coverage changes this line by itself.
pub fn version_line() -> String {
    let magic = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    format!(
        "{CHECKPOINT_FORMAT}\t{CHECKPOINT_VERSION}\t{}\t{}",
        magic(&WORLD_MAGIC),
        magic(&COMBAT_MAGIC)
    )
}

/// One row of the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    /// The step number: 0 for the first row, then rising by exactly 1.
    pub tick: u64,
    /// The zero-based index of the next record not yet applied.
    pub input_index: u64,
    pub state_hash: u64,
    /// `WorldState::combat_hash()` while a combat is active.
    pub combat_hash: Option<u64>,
}

impl Checkpoint {
    /// The row of `world` now. Reads the world only (rule 4).
    pub fn of(world: &WorldState, tick: u64, input_index: u64) -> Self {
        // The test that makes the combat record present (§2.6).
        let active = world.combat_state.flags & crate::critical::state::ACTIVE != 0;
        Self {
            tick,
            input_index,
            state_hash: world.state_hash(),
            combat_hash: active.then(|| world.combat_hash()),
        }
    }
}

impl fmt::Display for Checkpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\t{}\t{:016X}\t",
            self.tick, self.input_index, self.state_hash
        )?;
        match self.combat_hash {
            Some(hash) => write!(f, "{hash:016X}"),
            None => f.write_str("-"),
        }
    }
}

/// A decimal number with no leading zero and no sign.
fn decimal(field: &str) -> Option<u64> {
    let digits = !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit());
    let canonical = field == "0" || !field.starts_with('0');
    (digits && canonical).then(|| field.parse().ok()).flatten()
}

/// 16 uppercase hexadecimal digits.
fn hash(field: &str) -> Option<u64> {
    let digits = field
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'));
    (field.len() == 16 && digits)
        .then(|| u64::from_str_radix(field, 16).ok())
        .flatten()
}

/// The row `line` holds, if it is well-formed after `last` (`None` before
/// the first row).
fn parse_row(line: &str, last: Option<&Checkpoint>) -> Option<Checkpoint> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [tick, input_index, state_hash, combat_hash] = fields[..] else {
        return None;
    };
    let row = Checkpoint {
        tick: decimal(tick)?,
        input_index: decimal(input_index)?,
        state_hash: hash(state_hash)?,
        combat_hash: match combat_hash {
            "-" => None,
            field => Some(hash(field)?),
        },
    };
    let follows = match last {
        Some(last) => {
            last.tick.checked_add(1) == Some(row.tick) && row.input_index >= last.input_index
        }
        None => row.tick == 0 && row.input_index == 0,
    };
    follows.then_some(row)
}

/// The stream's whole text for `rows`.
pub fn write_checkpoints(rows: &[Checkpoint]) -> String {
    let mut text = stream_head();
    for row in rows {
        text.push_str(&format!("{row}\n"));
    }
    text
}

/// Lines 1 and 2, each with its LF.
pub fn stream_head() -> String {
    format!("{}\n{CHECKPOINT_HEADER}\n", version_line())
}

/// The lines of a stream's text, without their LFs: an error when the text
/// does not end with one.
fn stream_lines(text: &str) -> Result<Vec<&str>, String> {
    let body = text
        .strip_suffix('\n')
        .ok_or("a checkpoint stream ends with LF")?;
    Ok(body.split('\n').collect())
}

/// Read a stream written in this format, version line and header included.
pub fn read_checkpoints(text: &str) -> Result<Vec<Checkpoint>, String> {
    let lines = stream_lines(text)?;
    if lines.len() < 2 {
        return Err("a checkpoint stream has a version line and a header".into());
    }
    if lines[0] != version_line() || lines[1] != CHECKPOINT_HEADER {
        return Err("the checkpoint format differs".into());
    }
    let mut rows: Vec<Checkpoint> = Vec::with_capacity(lines.len() - 2);
    for (index, line) in lines[2..].iter().enumerate() {
        let row = parse_row(line, rows.last())
            .ok_or_else(|| format!("checkpoint row malformed: line {}", index + 3))?;
        rows.push(row);
    }
    Ok(rows)
}

/// A stream a run is compared with: its text's lines 1 and 2 as bytes, and
/// its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedStream {
    version: String,
    header: String,
    rows: Vec<Checkpoint>,
}

impl ExpectedStream {
    /// `text` as an expected stream. Its lines 1 and 2 may hold another
    /// format, which the comparison reports; a text that is not a stream
    /// of rows at all cannot be compared with anything.
    pub fn parse(text: &str) -> Result<Self, String> {
        let lines = stream_lines(text)?;
        let [version, header, rows @ ..] = &lines[..] else {
            return Err("a checkpoint stream has a version line and a header".into());
        };
        let mut parsed: Vec<Checkpoint> = Vec::with_capacity(rows.len());
        for (index, line) in rows.iter().enumerate() {
            let row = parse_row(line, parsed.last())
                .ok_or_else(|| format!("checkpoint row malformed: line {}", index + 3))?;
            parsed.push(row);
        }
        Ok(Self {
            version: (*version).to_owned(),
            header: (*header).to_owned(),
            rows: parsed,
        })
    }
}

/// How a comparison stopped: `Display` is the one-line report of §3.10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointFailure {
    /// A difference in line 1 or 2. No row is compared.
    Format {
        line: u64,
        expected: String,
        actual: String,
    },
    Mismatch {
        tick: u64,
        column: &'static str,
        expected: String,
        actual: String,
    },
    /// `after` is the last row's tick, `None` when the stream has no row.
    EndedEarly {
        expected: u64,
        after: Option<u64>,
    },
    TooLong {
        tick: u64,
    },
    Malformed {
        line: u64,
    },
}

impl fmt::Display for CheckpointFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spaced = |line: &str| line.replace('\t', " ");
        match self {
            Self::Format {
                line,
                expected,
                actual,
            } => write!(
                f,
                "checkpoint format mismatch: line {line} expected {}, actual {}",
                spaced(expected),
                spaced(actual)
            ),
            Self::Mismatch {
                tick,
                column,
                expected,
                actual,
            } => write!(
                f,
                "checkpoint mismatch at tick {tick}: {column} expected {expected}, actual {actual}"
            ),
            Self::EndedEarly { expected, after } => {
                let after = after.map_or_else(|| "-".to_owned(), |tick| tick.to_string());
                write!(
                    f,
                    "checkpoint stream ended early: expected tick {expected}, actual ended after \
                     tick {after}"
                )
            }
            Self::TooLong { tick } => {
                write!(f, "checkpoint stream too long: unexpected tick {tick}")
            }
            Self::Malformed { line } => write!(f, "checkpoint row malformed: line {line}"),
        }
    }
}

/// The columns of a well-formed row as text, in order.
fn columns(row: &Checkpoint) -> [String; 4] {
    let line = row.to_string();
    let mut fields = line.split('\t').map(str::to_owned);
    std::array::from_fn(|_| fields.next().unwrap_or_default())
}

/// The first column in which two rows differ.
fn first_difference(expected: &Checkpoint, actual: &Checkpoint) -> Option<CheckpointFailure> {
    let (want, got) = (columns(expected), columns(actual));
    (0..COLUMNS.len())
        .find(|column| want[*column] != got[*column])
        .map(|column| CheckpointFailure::Mismatch {
            tick: expected.tick,
            column: COLUMNS[column],
            expected: want[column].clone(),
            actual: got[column].clone(),
        })
}

/// §3.10 the comparison, fed one line at a time, so that a run can stop at
/// the step of the first failure. It stops there: every later line is
/// expected to differ, so none is looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointComparison {
    expected: ExpectedStream,
    /// Lines given so far.
    lines: u64,
    last: Option<Checkpoint>,
}

impl CheckpointComparison {
    pub fn new(expected: ExpectedStream) -> Self {
        Self {
            expected,
            lines: 0,
            last: None,
        }
    }

    /// The next line of the actual stream, without its LF.
    pub fn line(&mut self, line: &str) -> Result<(), CheckpointFailure> {
        self.lines += 1;
        let number = self.lines;
        if number <= 2 {
            let want = if number == 1 {
                &self.expected.version
            } else {
                &self.expected.header
            };
            if want != line {
                return Err(CheckpointFailure::Format {
                    line: number,
                    expected: want.clone(),
                    actual: line.to_owned(),
                });
            }
            return Ok(());
        }
        let row = parse_row(line, self.last.as_ref())
            .ok_or(CheckpointFailure::Malformed { line: number })?;
        let Some(expected) = self.expected.rows.get((number - 3) as usize) else {
            return Err(CheckpointFailure::TooLong { tick: row.tick });
        };
        if let Some(failure) = first_difference(expected, &row) {
            return Err(failure);
        }
        self.last = Some(row);
        Ok(())
    }

    /// The actual stream ended: the rows it matched, or the first failure.
    pub fn finish(&self) -> Result<u64, CheckpointFailure> {
        if self.lines < 2 {
            let line = self.lines + 1;
            let expected = if line == 1 {
                &self.expected.version
            } else {
                &self.expected.header
            };
            return Err(CheckpointFailure::Format {
                line,
                expected: expected.clone(),
                actual: String::new(),
            });
        }
        let matched = self.lines - 2;
        match self.expected.rows.get(matched as usize) {
            Some(row) => Err(CheckpointFailure::EndedEarly {
                expected: row.tick,
                after: self.last.map(|last| last.tick),
            }),
            None => Ok(matched),
        }
    }
}

/// Compare a whole stream's text with an expected stream, as a run compares
/// its rows while it goes.
pub fn compare_checkpoints(
    expected: &ExpectedStream,
    actual: &str,
) -> Result<u64, CheckpointFailure> {
    let mut comparison = CheckpointComparison::new(expected.clone());
    let body = actual.strip_suffix('\n');
    for line in body.unwrap_or(actual).split('\n') {
        comparison.line(line)?;
    }
    if body.is_none() {
        return Err(CheckpointFailure::Malformed {
            line: comparison.lines,
        });
    }
    comparison.finish()
}

/// The result text of a comparison, as §3.10 words it.
pub fn comparison_report(result: &Result<u64, CheckpointFailure>) -> String {
    match result {
        Ok(rows) => format!("checkpoints match: {rows} rows"),
        Err(failure) => failure.to_string(),
    }
}

/// One run's stream: it numbers the rows and turns them into lines, and
/// compares each with the expected stream if one is given. The tool writes
/// the lines wherever it was asked to.
#[derive(Debug, Clone)]
pub struct CheckpointStream {
    next_tick: u64,
    comparison: Option<CheckpointComparison>,
}

impl CheckpointStream {
    pub fn new(expected: Option<ExpectedStream>) -> Self {
        Self {
            next_tick: 0,
            comparison: expected.map(CheckpointComparison::new),
        }
    }

    /// Lines 1 and 2, compared; written once, before any row.
    pub fn head(&mut self) -> Result<String, CheckpointFailure> {
        if let Some(comparison) = &mut self.comparison {
            comparison.line(&version_line())?;
            comparison.line(CHECKPOINT_HEADER)?;
        }
        Ok(stream_head())
    }

    /// The next row, from `world` now, as a line with its LF. The line is
    /// returned with the failure too, so a written stream holds the row the
    /// run stopped at.
    pub fn row(
        &mut self,
        world: &WorldState,
        input_index: u64,
    ) -> Result<String, (String, CheckpointFailure)> {
        let row = Checkpoint::of(world, self.next_tick, input_index);
        self.next_tick += 1;
        let line = row.to_string();
        if let Some(comparison) = &mut self.comparison
            && let Err(failure) = comparison.line(&line)
        {
            return Err((format!("{line}\n"), failure));
        }
        Ok(format!("{line}\n"))
    }

    /// The run ended: with an expected stream, its verdict.
    pub fn finish(&self) -> Result<Option<u64>, CheckpointFailure> {
        self.comparison
            .as_ref()
            .map(CheckpointComparison::finish)
            .transpose()
    }
}

#[cfg(test)]
#[path = "checkpoints_tests.rs"]
mod tests;
