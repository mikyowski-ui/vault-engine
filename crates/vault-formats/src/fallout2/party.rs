//! SPEC-018 §4.5 `data/party.txt`: party-member records and the seven
//! option vocabularies (SPEC-019 §2.5 owns the behavior).
use super::text::{self, Diagnostics, SectionedText, TableError};

pub const FILE: &str = "data/party.txt";

/// The seven control fields, in SPEC-019 §2.5's order (the order of the
/// saved member record, SPEC-018 §7.5 words 2-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ControlField {
    Disposition,
    AreaAttackMode,
    RunAwayMode,
    BestWeapon,
    Distance,
    AttackWho,
    ChemUse,
}

impl ControlField {
    pub const ALL: [Self; 7] = [
        Self::Disposition,
        Self::AreaAttackMode,
        Self::RunAwayMode,
        Self::BestWeapon,
        Self::Distance,
        Self::AttackWho,
        Self::ChemUse,
    ];
    /// The `party.txt` key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Disposition => "disposition",
            Self::AreaAttackMode => "area_attack_mode",
            Self::RunAwayMode => "run_away_mode",
            Self::BestWeapon => "best_weapon",
            Self::Distance => "distance",
            Self::AttackWho => "attack_who",
            Self::ChemUse => "chem_use",
        }
    }
    /// §4.5 the vocabulary; a word's index is its saved value.
    pub fn words(self) -> &'static [&'static str] {
        match self {
            Self::AreaAttackMode => &[
                "always",
                "sometimes",
                "be_sure",
                "be_careful",
                "be_absolutely_sure",
            ],
            Self::AttackWho => &[
                "whomever_attacking_me",
                "strongest",
                "weakest",
                "whomever",
                "closest",
            ],
            Self::BestWeapon => &[
                "no_pref",
                "melee",
                "melee_over_ranged",
                "ranged_over_melee",
                "ranged",
                "unarmed",
                "unarmed_over_thrown",
                "random",
            ],
            Self::ChemUse => &[
                "clean",
                "stims_when_hurt_little",
                "stims_when_hurt_lots",
                "sometimes",
                "anytime",
                "always",
            ],
            Self::Distance => &["stay_close", "charge", "snipe", "on_your_own", "stay"],
            Self::Disposition => &["custom", "coward", "defensive", "aggressive", "berserk"],
            Self::RunAwayMode => &[
                "none",
                "coward",
                "finger_hurts",
                "bleeding",
                "not_feeling_good",
                "tourniquet",
                "never",
            ],
        }
    }
    /// SPEC-019 §2.5's neutral record value, as a vocabulary index.
    pub fn neutral(self) -> i32 {
        match self {
            Self::Disposition => 0,    // custom
            Self::AreaAttackMode => 0, // always
            Self::RunAwayMode => 0,    // none
            Self::BestWeapon => 0,     // no_pref
            Self::Distance => 3,       // on_your_own
            Self::AttackWho => 3,      // whomever
            Self::ChemUse => 4,        // anytime
        }
    }
    /// Whether `value` is an index of this vocabulary.
    pub fn valid(self, value: i32) -> bool {
        usize::try_from(value).is_ok_and(|value| value < self.words().len())
    }
}

/// One `[Party Member N]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyMember {
    pub number: u32,
    pub pid: u32,
    /// Per field in [`ControlField::ALL`] order: the allowed indices.
    pub allowed: [Vec<i32>; 7],
    pub level_minimum: i32,
    pub level_up_every: i32,
    pub level_pids: Vec<u32>,
}

impl PartyMember {
    pub fn allowed(&self, field: ControlField) -> &[i32] {
        &self.allowed[field as usize]
    }
    /// Whether an object of `pid` is a candidate of this record.
    pub fn candidate(&self, pid: u32) -> bool {
        self.pid == pid || self.level_pids.contains(&pid)
    }
}

/// The parsed party table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartyTable {
    /// Index = record number, `0..P_T`.
    members: Vec<Option<PartyMember>>,
}

fn critter_pid(text: &str) -> Option<u32> {
    text::integer(text)
        .filter(|pid| pid >> 24 == 1)
        .map(|pid| pid as u32)
}

/// A record's own PID: a critter's, an item's or a scenery object's.
fn member_pid(text: &str) -> Option<u32> {
    text::integer(text)
        .filter(|pid| (0..=2).contains(&(pid >> 24)))
        .map(|pid| pid as u32)
}

impl PartyTable {
    pub fn parse(bytes: &[u8], notes: &mut Diagnostics) -> Result<Self, TableError> {
        let file = FILE;
        let table = SectionedText::parse(file, bytes, notes)?;
        let mut members: Vec<Option<PartyMember>> = Vec::new();
        let mut seen: std::collections::BTreeSet<u32> = Default::default();
        for (number, section) in table.numbered("party member") {
            if number > 99 {
                notes.push(
                    file,
                    section.line,
                    format!("party member {number} outside 0-99"),
                );
                continue;
            }
            // Revision 2 (R13): an item or scenery record (the car trunk)
            // is a follower.
            let Some(pid) = section
                .get("party_member_pid")
                .and_then(|value| member_pid(&value.text))
            else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}] has no valid party_member_pid; dropped", section.name),
                );
                continue;
            };
            let mut allowed: [Vec<i32>; 7] = Default::default();
            for field in ControlField::ALL {
                let Some(value) = section.get(field.key()) else {
                    continue;
                };
                for word in text::list(&value.text) {
                    let folded = text::fold(&word);
                    if field == ControlField::Disposition && folded == "none" {
                        continue;
                    }
                    match field.words().iter().position(|w| *w == folded) {
                        Some(index) => allowed[field as usize].push(index as i32),
                        None => notes.push(
                            file,
                            value.line,
                            format!("{}: unknown option {word:?} dropped", field.key()),
                        ),
                    }
                }
            }
            let level = |key: &str, notes: &mut Diagnostics| match section.get(key) {
                None => 0,
                Some(value) => text::integer_in(&value.text, 0..=99).unwrap_or_else(|| {
                    notes.push(file, value.line, format!("{key}: invalid; 0"));
                    0
                }),
            };
            let mut level_pids = Vec::new();
            // Revision 2 (R13): `-1` alone is the empty list.
            if let Some(value) = section.get("level_pids")
                && text::integer(&value.text) != Some(-1)
            {
                for item in text::list(&value.text) {
                    match critter_pid(&item) {
                        Some(pid) => level_pids.push(pid),
                        None => {
                            notes.push(file, value.line, format!("level_pids: {item:?} dropped"))
                        }
                    }
                }
            }
            // A PID in two records drops the higher-numbered one.
            let pids: Vec<u32> = std::iter::once(pid)
                .chain(level_pids.iter().copied())
                .collect();
            if pids.iter().any(|pid| seen.contains(pid)) {
                notes.push(
                    file,
                    section.line,
                    format!(
                        "[{}] repeats a PID of a lower record; dropped",
                        section.name
                    ),
                );
                continue;
            }
            seen.extend(pids);
            let member = PartyMember {
                number,
                pid,
                allowed,
                level_minimum: level("level_minimum", notes),
                level_up_every: level("level_up_every", notes),
                level_pids,
            };
            let index = number as usize;
            if members.len() <= index {
                members.resize(index + 1, None);
            }
            members[index] = Some(member);
        }
        Ok(Self { members })
    }
    /// `P_T`: the highest valid record number plus 1.
    pub fn count(&self) -> usize {
        self.members.len()
    }
    pub fn get(&self, number: i32) -> Option<&PartyMember> {
        usize::try_from(number)
            .ok()
            .and_then(|number| self.members.get(number))
            .and_then(Option::as_ref)
    }
    pub fn members(&self) -> impl Iterator<Item = &PartyMember> {
        self.members.iter().flatten()
    }
    /// The record an object of `pid` is a candidate of.
    pub fn record_of(&self, pid: u32) -> Option<&PartyMember> {
        self.members().find(|member| member.candidate(pid))
    }
}
