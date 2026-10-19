//! SPEC-014 §3.32 scrolling-credit text (`credits.txt`, `quotes.txt`) and
//! §3.30 endgame narration subtitles (`text/<language>/cuts/<name>.txt`).
//! Both are read in text mode: CR LF ends a line like LF.

/// The row a credit line occupies, chosen by its first character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditKind {
    /// `@`: title font and color.
    Title,
    /// `#`: name font in grey.
    GreyName,
    /// Anything else, including an empty line.
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditLine {
    pub kind: CreditKind,
    /// The text after the code character, without its line terminator
    /// (safe rule: the reference measures the kept newline, shifting the
    /// centering by up to one pixel).
    pub text: Vec<u8>,
}

fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\n").unwrap_or(line))
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

/// Every row in file order; `;` comment lines produce none, and an empty
/// line still produces an (empty) name row.
pub fn parse_credits(bytes: &[u8]) -> Vec<CreditLine> {
    lines(bytes)
        .filter_map(|line| {
            let (kind, text) = match line.first() {
                Some(b';') => return None,
                Some(b'@') => (CreditKind::Title, &line[1..]),
                Some(b'#') => (CreditKind::GreyName, &line[1..]),
                _ => (CreditKind::Name, line),
            };
            Some(CreditLine {
                kind,
                text: text.to_vec(),
            })
        })
        .collect()
}

/// At most this many narration lines are kept.
pub const MAX_NARRATION_LINES: usize = 50;
/// Each read stores at most 254 bytes plus its terminator.
pub const NARRATION_READ: usize = 255;
/// Seconds per character without a narration sound.
pub const DEFAULT_SECONDS_PER_CHARACTER: f64 = 0.08;

/// Narration subtitle lines `N:text`: the text after the first colon is
/// kept and the number ignored; a read without a colon is dropped. Reads
/// go through a 255-byte buffer, so a line longer than 254 bytes continues
/// as a further read, which is dropped unless it holds a colon.
pub fn parse_narration(bytes: &[u8]) -> Vec<Vec<u8>> {
    let text: Vec<u8> = lines(bytes)
        .flat_map(|line| [line, b"\n"])
        .flatten()
        .copied()
        .collect();
    let mut kept = Vec::new();
    let mut rest = text.as_slice();
    while !rest.is_empty() && kept.len() < MAX_NARRATION_LINES {
        let limit = rest.len().min(NARRATION_READ - 1);
        let end = rest[..limit]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(limit, |newline| newline + 1);
        let read = &rest[..end];
        rest = &rest[end..];
        let read = read.strip_suffix(b"\n").unwrap_or(read);
        if let Some(colon) = read.iter().position(|byte| *byte == b':') {
            kept.push(read[colon + 1..].to_vec());
        }
    }
    kept
}

/// Cumulative end time of each line in milliseconds. The time per
/// character is the narration length in whole seconds over the total
/// character count of all lines, or 0.08 seconds without narration; each
/// line adds `trunc(length * time per character * 1000)`.
pub fn narration_timing(lines: &[Vec<u8>], narration_seconds: Option<u32>) -> Vec<u64> {
    let total: usize = lines.iter().map(Vec::len).sum();
    let per_character = match narration_seconds {
        Some(seconds) if total > 0 => f64::from(seconds) / total as f64,
        Some(_) => 0.0,
        None => DEFAULT_SECONDS_PER_CHARACTER,
    };
    let mut end = 0;
    lines
        .iter()
        .map(|line| {
            end += (line.len() as f64 * per_character * 1000.0) as u64;
            end
        })
        .collect()
}
