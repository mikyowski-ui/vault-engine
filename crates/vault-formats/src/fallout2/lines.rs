//! SPEC-018 §4.6 the line tables: `quests.txt`, `holodisk.txt`,
//! `genrep.txt`, `karmavar.txt` and `endgame.txt`.
use super::text::{self, Diagnostics, TableError};

pub const QUESTS: &str = "data/quests.txt";
pub const HOLODISKS: &str = "data/holodisk.txt";
pub const GENREP: &str = "data/genrep.txt";
pub const KARMAVAR: &str = "data/karmavar.txt";
pub const ENDGAME: &str = "data/endgame.txt";

/// The valid records of a line table, each from its Integer fields.
fn integers<const N: usize>(
    file: &str,
    bytes: &[u8],
    notes: &mut Diagnostics,
) -> Result<Vec<[i32; N]>, TableError> {
    let mut out = Vec::new();
    for (line, fields) in text::record_lines(file, bytes)? {
        let values: Option<Vec<i32>> = fields.iter().map(|field| text::integer(field)).collect();
        match values.filter(|values| values.len() == N) {
            Some(values) => out.push(values.try_into().expect("N values")),
            None => notes.push(file, line, format!("expected {N} Integers; dropped")),
        }
    }
    Ok(out)
}

/// One quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quest {
    /// `map.msg` message.
    pub location: i32,
    /// `quests.msg` message.
    pub description: i32,
    pub global: i32,
    pub display: i32,
    pub completed: i32,
}

/// SPEC-018 §5.9 a quest's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestStatus {
    Hidden,
    Active,
    Completed,
}

impl Quest {
    pub fn status(&self, value: i32) -> QuestStatus {
        if value < self.display {
            QuestStatus::Hidden
        } else if value >= self.completed {
            QuestStatus::Completed
        } else {
            QuestStatus::Active
        }
    }
}

pub fn quests(bytes: &[u8], notes: &mut Diagnostics) -> Result<Vec<Quest>, TableError> {
    Ok(integers::<5>(QUESTS, bytes, notes)?
        .into_iter()
        .map(
            |[location, description, global, display, completed]| Quest {
                location,
                description,
                global,
                display,
                completed,
            },
        )
        .collect())
}

/// §5.9 the quest groups: by location, in order of first appearance, each
/// with its quests' ordinals in file order.
pub fn quest_groups(quests: &[Quest]) -> Vec<(i32, Vec<usize>)> {
    let mut groups: Vec<(i32, Vec<usize>)> = Vec::new();
    for (ordinal, quest) in quests.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|(location, _)| *location == quest.location)
        {
            Some((_, members)) => members.push(ordinal),
            None => groups.push((quest.location, vec![ordinal])),
        }
    }
    groups
}

/// One holodisk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holodisk {
    pub global: i32,
    /// `pipboy.msg` title.
    pub title: i32,
    /// The first of the 500 content message ids.
    pub text: i32,
}

pub fn holodisks(bytes: &[u8], notes: &mut Diagnostics) -> Result<Vec<Holodisk>, TableError> {
    Ok(integers::<3>(HOLODISKS, bytes, notes)?
        .into_iter()
        .map(|[global, title, text]| Holodisk {
            global,
            title,
            text,
        })
        .collect())
}

/// One reputation title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reputation {
    pub threshold: i32,
    /// `editor.msg` title.
    pub title: i32,
}

pub fn genrep(bytes: &[u8], notes: &mut Diagnostics) -> Result<Vec<Reputation>, TableError> {
    Ok(integers::<2>(GENREP, bytes, notes)?
        .into_iter()
        .map(|[threshold, title]| Reputation { threshold, title })
        .collect())
}

/// §4.6 the title for karma `karma`: the greatest threshold not above it,
/// the first in file order on a tie; none below every threshold.
pub fn reputation_title(table: &[Reputation], karma: i32) -> Option<i32> {
    let mut best: Option<&Reputation> = None;
    for record in table.iter().filter(|record| record.threshold <= karma) {
        if best.is_none_or(|best| record.threshold > best.threshold) {
            best = Some(record);
        }
    }
    best.map(|record| record.title)
}

/// One karma variable row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KarmaVar {
    pub global: i32,
    /// An `intrface.lst` row, or -1.
    pub art: i32,
    pub name: i32,
    pub description: i32,
}

pub fn karmavar(bytes: &[u8], notes: &mut Diagnostics) -> Result<Vec<KarmaVar>, TableError> {
    let mut out = Vec::new();
    for [global, art, name, description] in integers::<4>(KARMAVAR, bytes, notes)? {
        if art < -1 {
            notes.push(
                KARMAVAR,
                0,
                format!("art {art} is not a row or -1; dropped"),
            );
            continue;
        }
        out.push(KarmaVar {
            global,
            art,
            name,
            description,
        });
    }
    Ok(out)
}

/// An endgame slide's art.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlideArt {
    /// An `intrface.lst` row.
    Row(i32),
    /// `art/intrface/<stem>.frm`.
    Stem(String),
}

/// One endgame slide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndgameSlide {
    pub global: i32,
    pub value: i32,
    pub art: SlideArt,
    /// `sound/speech/narrator/<stem>.acm`.
    pub narration: String,
    /// 0 still, 1 or -1 a desert pan (SPEC-019 §3.20).
    pub direction: i32,
}

impl EndgameSlide {
    /// The art file for a stem (`None` for a row, which `intrface.lst`
    /// names).
    pub fn art_path(&self) -> Option<String> {
        match &self.art {
            SlideArt::Stem(stem) => Some(format!("art/intrface/{}.frm", stem.to_ascii_lowercase())),
            SlideArt::Row(_) => None,
        }
    }
    /// §4.6 the palette for a stem's art: `art/intrface/<stem>.pal`.
    pub fn palette_path(&self) -> Option<String> {
        match &self.art {
            SlideArt::Stem(stem) => Some(format!("art/intrface/{}.pal", stem.to_ascii_lowercase())),
            SlideArt::Row(_) => None,
        }
    }
    pub fn narration_path(&self) -> String {
        format!(
            "sound/speech/narrator/{}.acm",
            self.narration.to_ascii_lowercase()
        )
    }
}

/// §4.6 the palette of an art row's file: its name with `.pal` in place of
/// `.frm`.
pub fn row_palette(frm: &str) -> String {
    match frm.len().checked_sub(4) {
        Some(at) if frm[at..].eq_ignore_ascii_case(".frm") => format!("{}.pal", &frm[..at]),
        _ => format!("{frm}.pal"),
    }
}

pub fn endgame(bytes: &[u8], notes: &mut Diagnostics) -> Result<Vec<EndgameSlide>, TableError> {
    let mut out = Vec::new();
    for (line, fields) in text::record_lines(ENDGAME, bytes)? {
        let slide = (|| {
            if !(4..=5).contains(&fields.len()) {
                return None;
            }
            let art = match text::integer(&fields[2]) {
                Some(row) => SlideArt::Row(row),
                None => SlideArt::Stem(text::stem(&fields[2])?),
            };
            Some(EndgameSlide {
                global: text::integer(&fields[0])?,
                value: text::integer(&fields[1])?,
                art,
                narration: text::stem(&fields[3])?,
                direction: match fields.get(4) {
                    Some(direction) => text::integer_in(direction, -1..=1)?,
                    None => 0,
                },
            })
        })();
        match slide {
            Some(slide) => out.push(slide),
            None => notes.push(ENDGAME, line, "not a valid slide; dropped"),
        }
    }
    Ok(out)
}
