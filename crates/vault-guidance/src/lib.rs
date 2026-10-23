//! Quest/progression tracking: SPEC-008 §3.8 Pip-Boy holodisks and quest
//! status. Disk acquisition and quest progress derive only from campaign
//! globals and localized messages; this version parses no disk/quest
//! definition text files, and the Pip-Boy save component has zero bytes.
use vault_formats::MsgCatalog;

include!(concat!(env!("OUT_DIR"), "/pipboy_tables.rs"));

pub const DISK_TITLE_BASE: i32 = 400;
/// Content ids `1000 x ordinal + 1000` through `+ 499`.
pub const DISK_RECORD_LIMIT: i32 = 500;
pub const RECORDS_PER_PAGE: usize = 35;
pub const END_DISK: &[u8] = b"**END-DISK**";
pub const END_PARAGRAPH: &[u8] = b"**END-PAR**";
/// The water-chip quest always shows: active at most 1, completed above.
pub const WATER_CHIP_GLOBAL: usize = 101;
pub const LOCATION_TITLE_BASE: i32 = 700;

fn global(gvars: &[i32], index: usize) -> i32 {
    gvars.get(index).copied().unwrap_or(0)
}

/// Ordinals enumerated in the title list. Under the SPEC-008 §3.8 safe rule
/// only positive acquisition values count, as in selection (the reference
/// lists any nonzero value).
pub fn listed_disks(gvars: &[i32]) -> Vec<usize> {
    (0..DISK_GLOBALS.len())
        .filter(|ordinal| global(gvars, DISK_GLOBALS[*ordinal]) > 0)
        .collect()
}
/// List selection maps the n-th row to the n-th disk with a positive value,
/// so the list and the selected disk always agree.
pub fn selected_disk(gvars: &[i32], row: usize) -> Option<usize> {
    (0..DISK_GLOBALS.len())
        .filter(|ordinal| global(gvars, DISK_GLOBALS[*ordinal]) > 0)
        .nth(row)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskLine {
    Text(Vec<u8>),
    /// `**END-PAR**`: a blank paragraph line (still one record).
    Paragraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holodisk {
    pub ordinal: usize,
    pub title: Vec<u8>,
    /// Records before `**END-DISK**` (or the whole window), at most 35 per
    /// page.
    pub records: Vec<DiskLine>,
    /// `floor((records - 1) / 35)`, at least 0: under the SPEC-008 §3.8 safe
    /// rule an exact multiple of 35 exposes no extra empty final page.
    pub last_page: usize,
    /// Lookups that fell back to the `Error` placeholder, and a missing
    /// terminator; none of them stops the disk from showing.
    pub diagnostics: Vec<DiskDiagnostic>,
}
impl Holodisk {
    /// Records of `page`; a page past the end is empty (the multiplication
    /// saturates, so no page index can overflow).
    pub fn page(&self, page: usize) -> &[DiskLine] {
        let start = page
            .saturating_mul(RECORDS_PER_PAGE)
            .min(self.records.len());
        let end = (start + RECORDS_PER_PAGE).min(self.records.len());
        &self.records[start..end]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskError {
    Ordinal(usize),
}
impl std::fmt::Display for DiskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ordinal(ordinal) => write!(f, "holodisk ordinal {ordinal} outside 0-17"),
        }
    }
}
impl std::error::Error for DiskError {}

/// A logged, non-fatal holodisk lookup problem (SPEC-008 §3.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskDiagnostic {
    /// The title id is absent; the title shows the `Error` placeholder.
    MissingTitle(i32),
    /// A gap: a content id absent inside the window. It shows as one
    /// `Error` record (SPEC-002 §3.5) and does not end the disk.
    MissingRecord(i32),
    /// No `**END-DISK**` within the 500-record window; the disk shows the
    /// whole window.
    Unterminated(usize),
}
impl std::fmt::Display for DiskDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTitle(id) => {
                write!(f, "holodisk title message {id} is missing; showing Error")
            }
            Self::MissingRecord(id) => {
                // Content ids run from `1000 x ordinal + 1000`.
                let first = id.div_euclid(1000) * 1000;
                write!(
                    f,
                    "holodisk {} content message {id} is missing from the {first}-{} \
                     window; showing Error",
                    id.div_euclid(1000) - 1,
                    first + DISK_RECORD_LIMIT - 1
                )
            }
            Self::Unterminated(ordinal) => {
                write!(
                    f,
                    "holodisk {ordinal} has no **END-DISK** within 500 records"
                )
            }
        }
    }
}

/// The fixed text an absent message id yields (SPEC-002 §3.5).
pub const MISSING_TEXT: &[u8] = b"Error";

/// Read a disk's title (`400 + ordinal`) and content records from
/// `1000 x ordinal + 1000`, stopping at exact `**END-DISK**` text. Content is
/// enumerated by id: a gap reads as the `Error` placeholder and counts as one
/// record. An unterminated disk stops at the window bound (the SPEC-008 §3.8
/// safe rule), so its last page never reads past `+ 499`.
pub fn holodisk(ordinal: usize, messages: &MsgCatalog) -> Result<Holodisk, DiskError> {
    if ordinal >= DISK_GLOBALS.len() {
        return Err(DiskError::Ordinal(ordinal));
    }
    let mut diagnostics = Vec::new();
    let title_id = DISK_TITLE_BASE + ordinal as i32;
    let title = match messages.get(title_id) {
        Some(message) => message.text.clone(),
        None => {
            diagnostics.push(DiskDiagnostic::MissingTitle(title_id));
            MISSING_TEXT.to_vec()
        }
    };
    let first = 1000 * ordinal as i32 + 1000;
    let mut records = Vec::new();
    let mut terminated = false;
    for id in first..first + DISK_RECORD_LIMIT {
        let Some(message) = messages.get(id) else {
            diagnostics.push(DiskDiagnostic::MissingRecord(id));
            records.push(DiskLine::Text(MISSING_TEXT.to_vec()));
            continue;
        };
        let text = &message.text;
        if text == END_DISK {
            terminated = true;
            break;
        }
        records.push(if text == END_PARAGRAPH {
            DiskLine::Paragraph
        } else {
            DiskLine::Text(text.clone())
        });
    }
    if !terminated {
        diagnostics.push(DiskDiagnostic::Unterminated(ordinal));
    }
    Ok(Holodisk {
        ordinal,
        title,
        last_page: records.len().saturating_sub(1) / RECORDS_PER_PAGE,
        records,
        diagnostics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestState {
    Hidden,
    /// Ordinary green text.
    Active,
    /// Struck through and dimmed.
    Completed,
}
/// Fixed thresholds: at most 0 hidden, 1 active, above 1 completed; the
/// water-chip quest is active at most 1 (so always visible).
pub fn quest_state(index: usize, value: i32) -> QuestState {
    match value {
        _ if index == WATER_CHIP_GLOBAL && value <= 1 => QuestState::Active,
        ..=0 => QuestState::Hidden,
        1 => QuestState::Active,
        _ => QuestState::Completed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestEntry {
    /// Compact visible numbering from 1, skipping hidden entries.
    pub number: usize,
    /// Zero-based row position (the text id uses this, not the number).
    pub position: usize,
    pub global: usize,
    pub message: i32,
    pub state: QuestState,
}

/// Location-list pass: a town is listed when any of its positions before
/// the zero sentinel is visible. Under the SPEC-008 §3.8 safe rule this pass
/// stops at the sentinel like the others, so padded slots never inspect
/// campaign global zero.
pub fn quest_locations(gvars: &[i32]) -> Vec<(usize, i32)> {
    QUESTS
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            row.iter()
                .take_while(|index| **index != 0)
                .any(|index| quest_state(*index, global(gvars, *index)) != QuestState::Hidden)
        })
        .map(|(town, _)| (town, LOCATION_TITLE_BASE + 10 * town as i32))
        .collect()
}

/// Detail pass for one town: stops at the zero sentinel; text id is
/// `701 + 10 x town + position`.
pub fn town_quests(town: usize, gvars: &[i32]) -> Vec<QuestEntry> {
    let Some(row) = QUESTS.get(town) else {
        return Vec::new();
    };
    row.iter()
        .take_while(|index| **index != 0)
        .enumerate()
        .filter_map(|(position, index)| {
            let state = quest_state(*index, global(gvars, *index));
            (state != QuestState::Hidden).then_some((position, *index, state))
        })
        .enumerate()
        .map(|(visible, (position, index, state))| QuestEntry {
            number: visible + 1,
            position,
            global: index,
            message: LOCATION_TITLE_BASE + 1 + 10 * town as i32 + position as i32,
            state,
        })
        .collect()
}

#[cfg(test)]
mod tests;
