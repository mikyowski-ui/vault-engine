//! SPEC-014 §3.42 the character screen outside creation mode: grouped
//! numbers and the level block, the Perks, Karma and Kills folders, the
//! condition lines, the in-game hot regions, the info-card mapping (both
//! modes), skill spending with its refusals, and the perk chooser's list
//! (which Tag! and Mutate! reuse).
use super::{
    Rect,
    editor::{self, InfoLine},
};
use crate::{CharacterSheet, Perk, StatContext, progression};

/// `edtredt.frm` (`intrface.lst` 177), 640 by 480, copied opaquely.
pub const BACKGROUND: &str = "edtredt.frm";
/// The level block strip restored before each draw, and its lines: y and
/// `editor.msg` caption ("Level:", "Exp:", "Next Level:") at x = 32.
pub const LEVEL_STRIP: Rect = Rect::new(32, 280, 124, 32);
pub const LEVEL_LINES: [(i32, i32); 3] = [(280, 113), (291, 114), (302, 115)];
/// The level block's hot region.
pub const LEVEL_HIT: Rect = Rect::new(28, 280, 124, 32);
/// The folder tabs (one of three images, and the `fldrmask.frm` button).
pub const FOLDER_TABS: Rect = Rect::new(11, 327, 318, 33);
/// The list area restored before every folder draw.
pub const FOLDER_AREA: Rect = Rect::new(34, 360, 280, 120);
/// The folder lines' hot region.
pub const FOLDER_HIT: Rect = Rect::new(28, 363, 283, 105);
/// The skill rows' hot region (the slider cursor's rows).
pub const SKILL_ROWS: Rect = Rect::new(370, 27, 223, 195);
/// At most this many perk rows are drawn in the Perks folder.
pub const PERK_ROWS: usize = 7;
/// A skill at or above this value refuses a plus step.
pub const SKILL_LIMIT: i32 = 200;
/// The kill types (`proto.msg` 450-464 names, 465-479 descriptions).
pub const KILL_TYPES: usize = 15;
/// Campaign variable 155, the general reputation of Karma row 0.
pub const REPUTATION_VAR: usize = 155;
/// Karma entries 1-9: campaign variable and `art/skilldex/` picture; the
/// title is `editor.msg` 1000 + e and the description 1100 + e.
pub const KARMA: [(usize, i32); 9] = [
    (156, 49),
    (157, 51),
    (158, 50),
    (189, 52),
    (190, 53),
    (191, 53),
    (192, 53),
    (193, 53),
    (194, 52),
];
/// The skill cards' fixed bases, skill order 0-17.
pub const SKILL_BASES: [i32; 18] = [
    35, 10, 10, 65, 55, 40, 30, 15, 25, 20, 20, 20, 25, 20, 25, 20, 20, 5,
];
/// Derived lines 51-60: picture, `stat.msg` title (description + 100).
const DERIVED_CARDS: [(i32, i32); 10] = [
    (18, 109),
    (19, 108),
    (20, 112),
    (21, 111),
    (22, 124),
    (23, 132),
    (83, 131),
    (24, 113),
    (25, 114),
    (26, 115),
];

/// §3.42 grouped numbers: the decimal value with a comma after the
/// millions digit and the thousands digit whenever they are printed; a
/// negative value gives an empty string.
pub fn grouped(value: i64) -> String {
    if value < 0 {
        return String::new();
    }
    let digits = value.to_string();
    let mut text = String::with_capacity(digits.len() + 2);
    for (index, digit) in digits.chars().enumerate() {
        text.push(digit);
        let after = digits.len() - index - 1;
        if after == 3 || after == 6 {
            text.push(',');
        }
    }
    text
}

/// The level block's values: the level in decimal, the grouped experience,
/// and the grouped next threshold (SPEC-011 §3.20), `------` at level 21.
pub fn level_values(level: i32, experience: i32) -> [String; 3] {
    let next = progression::xp_threshold(level);
    [
        level.to_string(),
        grouped(i64::from(experience)),
        if next < 0 {
            "------".into()
        } else {
            grouped(i64::from(next))
        },
    ]
}

/// The three folders of the lower-left area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Folder {
    #[default]
    Perks,
    Karma,
    Kills,
}
impl Folder {
    pub const ALL: [Self; 3] = [Self::Perks, Self::Karma, Self::Kills];
    /// The tab image drawn at (11, 327).
    pub fn tab_art(self) -> &'static str {
        match self {
            Self::Perks => "perksfdr.frm",
            Self::Karma => "karmafdr.frm",
            Self::Kills => "killsfdr.frm",
        }
    }
    /// The tab caption (`editor.msg` 109-111) and the x it centers on.
    pub fn caption(self) -> (i32, i32) {
        match self {
            Self::Perks => (109, 61),
            Self::Karma => (110, 159),
            Self::Kills => (111, 257),
        }
    }
    /// A tab release at screen x: 208 or more Kills, 111-207 Karma, else
    /// Perks.
    pub fn at(x: i32) -> Self {
        match x {
            208.. => Self::Kills,
            111.. => Self::Karma,
            _ => Self::Perks,
        }
    }
    /// The folder line of its general card: 30 Perks, 32 Karma, 31 Kills.
    pub fn general_line(self) -> InfoLine {
        InfoLine::Folder(match self {
            Self::Perks => 30,
            Self::Karma => 32,
            Self::Kills => 31,
        })
    }
}

/// §3.42 hot regions in game (the folder tabs are a button, handled
/// apart). The level block gives `7 + trunc(max(0, y - 280) / 10)`, which
/// is 10 for y = 310-311; with the condition and derived regions,
/// `[May-improve]` safe rule (`clamp`): stop at 9, 50 and 60. The folder
/// region gives folder line `trunc(max(0, y - 364) / 11)`, plus 10 in the
/// Kills folder right of x = 174. The creation-only trait regions do not
/// exist.
pub fn game_info_hit(point: [i32; 2], folder: Folder, clamp: bool) -> Option<InfoLine> {
    let [x, y] = point;
    if Rect::new(19, 38, 125, 227).contains(point) {
        return editor::info_hit(point);
    }
    if LEVEL_HIT.contains(point) {
        let line = 7 + ((y - 280).max(0) / 10) as usize;
        return InfoLine::from_number(if clamp { line.min(9) } else { line }, true);
    }
    if FOLDER_HIT.contains(point) {
        let mut line = ((y - 364).max(0) / 11) as usize;
        if folder == Folder::Kills && x > 174 {
            line += 10;
        }
        return Some(InfoLine::Folder(line));
    }
    if Rect::new(52, 324, 169, 20).contains(point) || Rect::new(47, 353, 245, 100).contains(point) {
        return None;
    }
    editor::info_hit_with(point, clamp)
}

/// The skill cursor row a press in the skill rows picks.
pub fn skill_row(y: i32) -> usize {
    ((f64::from(y - 27) * 0.092_307_694) as usize).min(17)
}

/// §3.42 condition lines: Hit Points always lit, then Poisoned, Radiated,
/// and the persistent effect flags (SPEC-007 §2.4) blinded `0x40`,
/// crippled right arm `0x20`, left arm `0x10`, right leg `0x08`, left leg
/// `0x04`.
pub fn condition_lines(poison: i32, radiation: i32, effects: u32) -> [bool; 8] {
    [
        true,
        poison != 0,
        radiation != 0,
        effects & 0x40 != 0,
        effects & 0x20 != 0,
        effects & 0x10 != 0,
        effects & 0x08 != 0,
        effects & 0x04 != 0,
    ]
}

/// Held perks with their ranks, in perk-index order 0-63 (the effect perks
/// 53-63 included).
pub fn held_perks(sheet: &CharacterSheet) -> Vec<(Perk, u32)> {
    Perk::ALL
        .into_iter()
        .map(|perk| (perk, sheet.rank(perk)))
        .filter(|(_, rank)| *rank > 0)
        .collect()
}

/// Karma rows 1 and on: the entries (1-9) whose campaign variable is not 0.
pub fn karma_rows(global: impl Fn(usize) -> i32) -> Vec<usize> {
    (1..=KARMA.len())
        .filter(|entry| global(KARMA[entry - 1].0) != 0)
        .collect()
}

/// Kill types with a nonzero count, sorted by name in byte order (types in
/// order on equal names).
pub fn kill_rows(kills: &[i32; KILL_TYPES], name: impl Fn(usize) -> Vec<u8>) -> Vec<(usize, i32)> {
    let mut rows: Vec<_> = (0..KILL_TYPES)
        .filter(|kind| kills[*kind] != 0)
        .map(|kind| (name(kind), kind, kills[kind]))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    rows.into_iter()
        .map(|(_, kind, count)| (kind, count))
        .collect()
}

/// A kill row's position: the first 10 fill the left column (name x = 34,
/// count x = 136) from y = 362 stepping 11, the rest the right column
/// (191, 293).
pub fn kill_row_position(index: usize) -> ([i32; 2], i32) {
    let (column, row) = if index < 10 {
        (0, index)
    } else {
        (1, index - 10)
    };
    let y = 362 + 11 * row as i32;
    if column == 0 {
        ([34, y], 136)
    } else {
        ([191, y], 293)
    }
}

/// The y of the two Perks-folder trait slots. The second slot is drawn at
/// y = 461 when the first slot is in use, else at 450, but lights and
/// hit-tests as folder line 9; `[May-improve]` safe rule (`safe`): always
/// at 461.
pub fn trait_slot_y(slot: usize, traits: [i32; 2], safe: bool) -> i32 {
    if slot == 0 || (traits[0] < 0 && !safe) {
        450
    } else {
        461
    }
}

/// What the Perks, Karma and Kills folders hold, for the card mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderView {
    pub folder: Folder,
    /// Held perks in perk-index order.
    pub perks: Vec<(Perk, u32)>,
    pub traits: [i32; 2],
    /// Karma rows 1 and on (entries 1-9).
    pub karma: Vec<usize>,
    /// Kill rows in name order: kill type and count.
    pub kills: Vec<(usize, i32)>,
}

/// An info card's title text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Title {
    Message(&'static str, i32),
    /// `proto.msg` 450 + t, a space, `editor.msg` 126 "Kills".
    Kill(usize),
}

/// One info card: the `art/skilldex/` picture (list index), the title, the
/// skill whose "Base: NN% formula" follows it, and the description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    pub picture: i32,
    pub title: Title,
    pub base: Option<usize>,
    pub description: (&'static str, i32),
}
const fn card(picture: i32, title: (&'static str, i32), description: (&'static str, i32)) -> Card {
    Card {
        picture,
        title: Title::Message(title.0, title.1),
        base: None,
        description,
    }
}

/// §3.42 info card mapping for `line`, in game (`view` given) or in
/// creation mode. Safe rules (`[May-improve]`): an effect perk 53-63 keeps
/// its title and description although its picture index lies past the
/// list; an empty trait slot shows the Optional traits card.
pub fn info_card(line: InfoLine, view: Option<&FolderView>) -> Card {
    const EDITOR: &str = "editor.msg";
    const STAT: &str = "stat.msg";
    let game = view.is_some();
    match line {
        InfoLine::Special(k) => card(k as i32, (STAT, 100 + k as i32), (STAT, 200 + k as i32)),
        InfoLine::CharacterPoints => card(7, (EDITOR, 120), (EDITOR, 121)),
        InfoLine::Level(0) => card(7, (STAT, 401), (STAT, 501)),
        InfoLine::Level(1) => card(8, (STAT, 402), (STAT, 502)),
        InfoLine::Level(_) => card(9, (EDITOR, 122), (EDITOR, 123)),
        InfoLine::Folder(j) => folder_card(j, view.cloned().unwrap_or_default()),
        InfoLine::Condition(0) => card(10, (EDITOR, 300), (STAT, 207)),
        InfoLine::Condition(k) => {
            let k = k.min(7) as i32;
            card(10 + k, (EDITOR, 311 + k), (EDITOR, 399 + k))
        }
        InfoLine::Derived(k) => {
            let (picture, title) = DERIVED_CARDS[k.min(9)];
            card(picture, (STAT, title), (STAT, title + 100))
        }
        InfoLine::Skill(s) => {
            let s = s.min(17);
            Card {
                base: Some(s),
                ..card(
                    28 + s as i32,
                    ("skill.msg", 100 + s as i32),
                    ("skill.msg", 200 + s as i32),
                )
            }
        }
        InfoLine::TagSkillsCard if game => card(27, (EDITOR, 130), (EDITOR, 131)),
        InfoLine::TagSkillsCard => card(27, (EDITOR, 144), (EDITOR, 145)),
        InfoLine::SkillsCard => card(27, (EDITOR, 150), (EDITOR, 151)),
        InfoLine::OptionalTraits => card(27, (EDITOR, 146), (EDITOR, 147)),
        InfoLine::Trait(t) => {
            let t = t.min(15) as i32;
            card(55 + t, ("trait.msg", 100 + t), ("trait.msg", 200 + t))
        }
    }
}

fn folder_card(line: usize, view: FolderView) -> Card {
    const EDITOR: &str = "editor.msg";
    match view.folder {
        Folder::Perks => {
            let shown = view.perks.len().min(PERK_ROWS);
            if line < shown {
                let p = view.perks[line].0.index() as i32;
                return card(72 + p, ("perk.msg", 101 + p), ("perk.msg", 201 + p));
            }
            if (8..=9).contains(&line)
                && let Some(t) = Some(view.traits[line - 8]).filter(|t| *t >= 0)
            {
                return card(55 + t, ("trait.msg", 100 + t), ("trait.msg", 200 + t));
            }
            if (7..=10).contains(&line) {
                card(54, (EDITOR, 146), (EDITOR, 147))
            } else {
                card(71, (EDITOR, 124), (EDITOR, 127))
            }
        }
        Folder::Karma => {
            if line == 0 {
                card(48, (EDITOR, 1000), (EDITOR, 1100))
            } else if let Some(entry) = view.karma.get(line - 1) {
                let e = *entry as i32;
                card(KARMA[*entry - 1].1, (EDITOR, 1000 + e), (EDITOR, 1100 + e))
            } else {
                card(47, (EDITOR, 125), (EDITOR, 128))
            }
        }
        Folder::Kills => match view.kills.get(line) {
            Some((kind, _)) => Card {
                picture: 36,
                title: Title::Kill(*kind),
                base: None,
                description: ("proto.msg", 465 + *kind as i32),
            },
            None => card(46, (EDITOR, 126), (EDITOR, 129)),
        },
    }
}

/// §3.42 the skill plus and minus refusals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillRefusal {
    /// A: "No free skill points available." (`editor.msg` 136), no body.
    NoPoints,
    /// B: the skill name and ":", "At maximum level." / "Unable to
    /// increment it." (132, 133).
    AtMaximum,
    /// C: the skill name and ":", "At minimum level." / "Unable to
    /// decrement it." (134, 135).
    AtMinimum,
}
impl SkillRefusal {
    /// The `editor.msg` title (none: the skill name and ":") and body IDs.
    pub fn messages(self) -> (Option<i32>, &'static [i32]) {
        match self {
            Self::NoPoints => (Some(136), &[]),
            Self::AtMaximum => (None, &[132, 133]),
            Self::AtMinimum => (None, &[134, 135]),
        }
    }
}

/// The decrement floor taken when the screen opens: every skill's value
/// and its invested points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillFloor {
    pub value: [i32; 18],
    pub invested: [i32; 18],
}
impl SkillFloor {
    pub fn of(sheet: &CharacterSheet, context: StatContext) -> Self {
        Self {
            value: std::array::from_fn(|skill| sheet.skill(skill, context)),
            invested: sheet.invested,
        }
    }
}

/// One plus step on `skill`: refusal A without unspent points, B at a value
/// of 200 or more; otherwise one point moves into the skill.
pub fn skill_plus(
    sheet: &mut CharacterSheet,
    skill: usize,
    context: StatContext,
) -> Result<(), SkillRefusal> {
    if sheet.unspent_skills <= 0 {
        return Err(SkillRefusal::NoPoints);
    }
    if sheet.skill(skill, context) >= SKILL_LIMIT {
        return Err(SkillRefusal::AtMaximum);
    }
    if sheet.buy_skill(skill) {
        Ok(())
    } else {
        Err(SkillRefusal::NoPoints)
    }
}

/// One minus step: refusal C when the value is not above its value at
/// open, or the skill has no invested points; otherwise one point returns
/// to the pool (no upper limit). The floor is the value at open, so a value
/// raised by Tag! or by losing Gifted lets earlier visits' points be
/// refunded; `[May-improve]` safe rule (`safe`): never below the invested
/// points at open.
pub fn skill_minus(
    sheet: &mut CharacterSheet,
    skill: usize,
    context: StatContext,
    floor: &SkillFloor,
    safe: bool,
) -> Result<(), SkillRefusal> {
    let skill = skill.min(17);
    let invested = sheet.invested[skill];
    if sheet.skill(skill, context) <= floor.value[skill]
        || invested <= 0
        || (safe && invested <= floor.invested[skill])
    {
        return Err(SkillRefusal::AtMinimum);
    }
    sheet.invested[skill] = invested - 1;
    sheet.unspent_skills = sheet.unspent_skills.saturating_add(1);
    Ok(())
}

/// The perk chooser's visible rows.
pub const PICK_ROWS: usize = 11;
/// §3.42 the chooser's idle timer: no input for more than this many
/// milliseconds since it last restarted forgets the previous click.
pub const PICK_IDLE_MS: u64 = 700;

/// §3.42 the perk chooser's list (also the Tag! and Mutate! steps): `n`
/// entries, the selected entry `top + row`, `top` within 0 to
/// `max(0, n - 11)` and `row` within 0-10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickList {
    pub n: usize,
    pub top: usize,
    pub row: usize,
    /// The row of the previous click, until forgotten.
    click: Option<usize>,
    /// When the idle timer last restarted.
    idle_mark: u64,
}
impl PickList {
    pub fn new(n: usize, now: u64) -> Self {
        Self {
            n,
            top: 0,
            row: 0,
            click: None,
            idle_mark: now,
        }
    }
    /// The selected entry; `None` for an empty list.
    pub fn selected(&self) -> Option<usize> {
        let entry = self.top + self.row;
        (entry < self.n).then_some(entry)
    }
    /// Up: the top moves while above 0, else the row, not below 0.
    pub fn up(&mut self) {
        self.click = None;
        if self.top > 0 {
            self.top -= 1;
        } else {
            self.row = self.row.saturating_sub(1);
        }
    }
    /// Down: past 11 entries the top moves up to `n - 11`, then the row up
    /// to 10; otherwise the row up to `n - 1`.
    pub fn down(&mut self) {
        self.click = None;
        if self.n > PICK_ROWS {
            if self.top < self.n - PICK_ROWS {
                self.top += 1;
            } else {
                self.row = (self.row + 1).min(PICK_ROWS - 1);
            }
        } else {
            self.row = (self.row + 1).min(self.n.saturating_sub(1));
        }
    }
    pub fn home(&mut self) {
        self.click = None;
        self.top = 0;
        self.row = 0;
    }
    pub fn end(&mut self) {
        self.click = None;
        if self.n > PICK_ROWS {
            self.top = self.n - PICK_ROWS;
            self.row = PICK_ROWS - 1;
        } else {
            self.row = self.n.saturating_sub(1);
        }
    }
    /// A click released in the list area at screen y: row
    /// `trunc((y - 134) / 12)`, clamped to the list. Returns whether it
    /// chose the entry (a second click on the same row). An empty list
    /// takes no click (safe rule).
    pub fn click(&mut self, screen_y: i32) -> bool {
        if self.n == 0 {
            return false;
        }
        let row = ((screen_y - 134) / 12).max(0) as usize;
        self.row = row.min(self.n - 1).min(PICK_ROWS - 1);
        if self.click == Some(self.row) {
            self.click = None;
            return true;
        }
        self.click = Some(self.row);
        false
    }
    /// A frame without input at `now`: more than 700 ms since the timer
    /// last restarted restarts it and forgets the click.
    pub fn idle(&mut self, now: u64) {
        if now.saturating_sub(self.idle_mark) > PICK_IDLE_MS {
            self.idle_mark = now;
            self.click = None;
        }
    }
}

/// The chooser's description wrap width: `133 + max(0, min(150, c) - 8)`.
pub fn chooser_wrap(column: i32) -> i32 {
    133 + (column.min(150) - 8).max(0)
}
/// The info card's description wrap width (§3.8):
/// `136 + max(0, min(150, c) - 8)`.
pub fn card_wrap(column: i32) -> i32 {
    136 + (column.min(150) - 8).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(folder: Folder) -> FolderView {
        FolderView {
            folder,
            ..FolderView::default()
        }
    }

    #[test]
    fn fixture_k_level_block_and_grouped_numbers() {
        assert_eq!(level_values(6, 15500), ["6", "15,500", "21,000"]);
        assert_eq!(level_values(21, 215_000), ["21", "215,000", "------"]);
        let grouped: Vec<_> = [0, 999, 1000, 100_000, 1_234_567, 12_345_678]
            .into_iter()
            .map(grouped)
            .collect();
        assert_eq!(
            grouped,
            ["0", "999", "1,000", "100,000", "1,234,567", "12,345,678"]
        );
        assert_eq!(super::grouped(-1), "");
        assert_eq!(editor::big_number_cells(101, 101), [9, 9]);
    }

    #[test]
    fn fixture_l_hit_tests() {
        let line = |point, folder, clamp| game_info_hit(point, folder, clamp).map(InfoLine::number);
        assert_eq!(line([60, 290], Folder::Perks, false), Some(8));
        // The unclamped level block reaches the Perks folder's first line.
        assert_eq!(line([60, 311], Folder::Perks, false), Some(10));
        let card = info_card(InfoLine::Folder(0), Some(&view(Folder::Perks)));
        assert_eq!(card.title, Title::Message("editor.msg", 124));
        assert_eq!(card.description, ("editor.msg", 127));
        assert_eq!(line([60, 311], Folder::Perks, true), Some(9));
        let mut kills = view(Folder::Kills);
        kills.kills = (0..12).map(|kind| (kind, 1)).collect();
        assert_eq!(line([100, 380], Folder::Kills, false), Some(11));
        assert_eq!(
            info_card(InfoLine::Folder(1), Some(&kills)).title,
            Title::Kill(1)
        );
        assert_eq!(line([200, 380], Folder::Kills, false), Some(21));
        let twelfth = info_card(InfoLine::Folder(11), Some(&kills));
        assert_eq!((twelfth.picture, twelfth.title), (36, Title::Kill(11)));
        let mut perks = view(Folder::Perks);
        perks.perks = vec![(Perk::Awareness, 1), (Perk::Toughness, 1)];
        perks.traits = [8, 6];
        assert_eq!(line([100, 455], Folder::Perks, false), Some(18));
        let first = info_card(InfoLine::Folder(8), Some(&perks));
        assert_eq!(first.title, Title::Message("trait.msg", 108));
        // The tab release at x = 200 is the Karma folder, general line 42.
        assert_eq!(Folder::at(200), Folder::Karma);
        assert_eq!(Folder::Karma.general_line().number(), 42);
        let karma = info_card(Folder::Karma.general_line(), Some(&view(Folder::Karma)));
        assert_eq!(
            (karma.picture, karma.title),
            (47, Title::Message("editor.msg", 125))
        );
        // The unclamped conditions region gives the Armor Class card.
        assert_eq!(line([250, 147], Folder::Perks, false), Some(51));
        assert_eq!(line([250, 147], Folder::Perks, true), Some(50));
        let armor = info_card(InfoLine::Derived(0), Some(&view(Folder::Perks)));
        assert_eq!(
            (armor.picture, armor.title),
            (18, Title::Message("stat.msg", 109))
        );
        assert_eq!(line([250, 307], Folder::Perks, false), Some(61));
        assert_eq!(line([250, 307], Folder::Perks, true), Some(60));
    }

    #[test]
    fn info_cards_follow_the_mapping_table() {
        let game = view(Folder::Perks);
        let numbered = |n: usize, game_view: Option<&FolderView>| {
            info_card(
                InfoLine::from_number(n, game_view.is_some()).unwrap(),
                game_view,
            )
        };
        assert_eq!(numbered(7, None).title, Title::Message("editor.msg", 120));
        assert_eq!(
            numbered(7, Some(&game)).title,
            Title::Message("stat.msg", 401)
        );
        assert_eq!(numbered(9, Some(&game)).picture, 9);
        assert_eq!(numbered(43, None).description, ("stat.msg", 207));
        assert_eq!(numbered(44, None).description, ("editor.msg", 400));
        assert_eq!(numbered(50, None).picture, 17);
        assert_eq!(numbered(57, None).picture, 83);
        let skill = numbered(61, None);
        assert_eq!((skill.picture, skill.base), (28, Some(0)));
        assert_eq!(numbered(79, None).title, Title::Message("editor.msg", 144));
        assert_eq!(
            numbered(79, Some(&game)).title,
            Title::Message("editor.msg", 130)
        );
        assert_eq!(numbered(97, None).picture, 70);
        // A held effect perk keeps its title (safe rule); line 9 with one
        // trait shows the Optional traits card.
        let mut perks = view(Folder::Perks);
        perks.perks = vec![(Perk::ALL[60], 1)];
        perks.traits = [3, -1];
        let effect = info_card(InfoLine::Folder(0), Some(&perks));
        assert_eq!(
            (effect.picture, effect.title),
            (132, Title::Message("perk.msg", 161))
        );
        assert_eq!(info_card(InfoLine::Folder(9), Some(&perks)).picture, 54);
        assert_eq!(info_card(InfoLine::Folder(20), Some(&perks)).picture, 71);
        let mut karma = view(Folder::Karma);
        karma.karma = vec![2, 5];
        assert_eq!(info_card(InfoLine::Folder(0), Some(&karma)).picture, 48);
        let champion = info_card(InfoLine::Folder(1), Some(&karma));
        assert_eq!(
            (champion.picture, champion.description),
            (51, ("editor.msg", 1102))
        );
        assert_eq!(info_card(InfoLine::Folder(2), Some(&karma)).picture, 53);
        assert_eq!(info_card(InfoLine::Folder(3), Some(&karma)).picture, 47);
        assert_eq!(
            info_card(InfoLine::Folder(0), Some(&view(Folder::Kills))).picture,
            46
        );
    }

    #[test]
    fn folder_rows_and_conditions() {
        let names = [
            "Men",
            "Women",
            "Children",
            "Super Mutants",
            "Ghouls",
            "Brahmin",
            "Radscorpions",
            "Rats",
            "Floaters",
            "Centaurs",
            "Robots",
            "Dogs",
            "Manti",
            "DeathClaws",
            "Master",
        ];
        let mut kills = [0; KILL_TYPES];
        kills[11] = 2;
        kills[13] = 1;
        kills[7] = 5;
        let rows = kill_rows(&kills, |kind| names[kind].as_bytes().to_vec());
        // Byte order: "DeathClaws" before "Dogs".
        assert_eq!(rows, [(13, 1), (11, 2), (7, 5)]);
        assert_eq!(kill_row_position(0), ([34, 362], 136));
        assert_eq!(kill_row_position(11), ([191, 373], 293));
        let globals = |var: usize| i32::from(matches!(var, 157 | 194));
        assert_eq!(karma_rows(globals), [2, 9]);
        assert_eq!(
            condition_lines(0, 3, 0x40 | 0x04),
            [true, false, true, true, false, false, false, true]
        );
        assert_eq!(trait_slot_y(1, [-1, 4], false), 450);
        assert_eq!(trait_slot_y(1, [-1, 4], true), 461);
        assert_eq!(trait_slot_y(1, [2, 4], false), 461);
        assert_eq!(skill_row(27), 0);
        assert_eq!(skill_row(221), 17);
    }

    #[test]
    fn fixture_m_skill_spending() {
        let context = StatContext::default();
        // Small Guns 55%, untagged: 35 + AG 5, 5 invested points, and the
        // Skilled trait's +10.
        let mut sheet = CharacterSheet {
            traits: [crate::Trait::Skilled as i32, -1],
            ..CharacterSheet::default()
        };
        sheet.invested[0] = 5;
        sheet.unspent_skills = 2;
        assert_eq!(sheet.skill(0, context), 55);
        let floor = SkillFloor::of(&sheet, context);
        let open = sheet.clone();
        assert_eq!(skill_plus(&mut sheet, 0, context), Ok(()));
        assert_eq!((sheet.skill(0, context), sheet.unspent_skills), (56, 1));
        assert_eq!(skill_plus(&mut sheet, 0, context), Ok(()));
        assert_eq!((sheet.skill(0, context), sheet.unspent_skills), (57, 0));
        assert_eq!(
            skill_plus(&mut sheet, 0, context),
            Err(SkillRefusal::NoPoints)
        );
        assert_eq!(skill_minus(&mut sheet, 0, context, &floor, true), Ok(()));
        assert_eq!((sheet.skill(0, context), sheet.unspent_skills), (56, 1));
        assert_eq!(skill_minus(&mut sheet, 0, context, &floor, true), Ok(()));
        assert_eq!((sheet.skill(0, context), sheet.unspent_skills), (55, 2));
        assert_eq!(
            skill_minus(&mut sheet, 0, context, &floor, false),
            Err(SkillRefusal::AtMinimum)
        );
        assert_eq!(SkillRefusal::AtMinimum.messages(), (None, &[134, 135][..]));
        assert_eq!(SkillRefusal::NoPoints.messages(), (Some(136), &[][..]));
        // Cancel restores the snapshot taken at open.
        sheet = open.clone();
        assert_eq!((sheet.invested[0], sheet.unspent_skills), (5, 2));
        // At 200 or more, refusal B.
        sheet.invested[0] = 300;
        assert_eq!(
            skill_plus(&mut sheet, 0, context),
            Err(SkillRefusal::AtMaximum)
        );
    }

    #[test]
    fn the_decrement_floor_is_the_value_at_open() {
        let context = StatContext::default();
        let mut sheet = CharacterSheet::default();
        sheet.invested[3] = 4;
        let floor = SkillFloor::of(&sheet, context);
        // Tag! during the visit raises the value above the floor.
        sheet.tags[3] = 3;
        assert_eq!(skill_minus(&mut sheet, 3, context, &floor, false), Ok(()));
        assert_eq!(sheet.invested[3], 3);
        // The safe rule keeps the invested points at open.
        sheet.invested[3] = 4;
        assert_eq!(
            skill_minus(&mut sheet, 3, context, &floor, true),
            Err(SkillRefusal::AtMinimum)
        );
    }

    #[test]
    fn fixture_n_perk_chooser_selection() {
        let mut list = PickList::new(14, 0);
        for _ in 0..3 {
            list.down();
        }
        assert_eq!((list.top, list.row, list.selected()), (3, 0, Some(3)));
        list.down();
        list.down();
        assert_eq!((list.top, list.row, list.selected()), (3, 2, Some(5)));
        list.up();
        assert_eq!((list.top, list.row, list.selected()), (2, 2, Some(4)));
        list.end();
        assert_eq!((list.top, list.row, list.selected()), (3, 10, Some(13)));
        list.home();
        assert_eq!((list.top, list.row, list.selected()), (0, 0, Some(0)));
        assert!(!list.click(170));
        list.idle(500);
        assert!(list.click(175));
        assert_eq!((list.top, list.row, list.selected()), (0, 3, Some(3)));
        // The idle timer forgets a click; navigation does too.
        assert!(!list.click(170));
        list.idle(701);
        assert!(!list.click(170));
        list.down();
        assert!(!list.click(170));
        // A short list moves the row only; an empty list takes no click.
        let mut short = PickList::new(3, 0);
        short.end();
        assert_eq!((short.row, short.selected()), (2, Some(2)));
        short.down();
        assert_eq!(short.row, 2);
        let mut empty = PickList::new(0, 0);
        assert!(!empty.click(140) && !empty.click(140));
        assert_eq!(empty.selected(), None);
    }
}
