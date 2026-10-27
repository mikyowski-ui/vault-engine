//! SPEC-014 §3.8-3.11: the character editor in creation mode: character
//! points, SPECIAL steps, tag and trait picks, validation, the name, age and
//! sex dialogs, the info hot regions, big numbers and the options dialog's
//! erase defaults.
use super::{ColorKey, Layout, Rect};
use crate::stats::{CharacterSheet, StatContext};

pub const BACKGROUND: &str = "edtrcrte.frm";
/// SPECIAL row tops, Strength first.
pub const SPECIAL_Y: [i32; 7] = [37, 70, 103, 136, 169, 202, 235];
/// Condition lines (`editor.msg` 300 then 312-318) and the derived list
/// (message, stat, shown as a percentage).
pub const CONDITIONS: [i32; 8] = [300, 312, 313, 314, 315, 316, 317, 318];
pub const DERIVED: [(i32, usize, bool); 10] = [
    (302, 9, false),
    (301, 8, false),
    (311, 12, false),
    (304, 11, false),
    (305, 24, true),
    (306, 32, true),
    (307, 31, true),
    (308, 13, false),
    (309, 14, false),
    (310, 15, true),
];
pub const OPTIONS_BUTTONS: [[i32; 2]; 3] = [[343, 454], [455, 454], [552, 454]];
pub const DIALOG_ORIGIN: [i32; 2] = [192, 126];
pub const DIALOG_COLOR: ColorKey = ColorKey::ERROR;

/// What the info card describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoLine {
    Special(usize),
    CharacterPoints,
    OptionalTraits,
    Trait(usize),
    Condition(usize),
    Derived(usize),
    SkillsCard,
    Skill(usize),
    TagSkillsCard,
    /// SPEC-014 §3.42 in game: the level block's lines (Level, Exp, Next
    /// Level), info lines 7-9.
    Level(usize),
    /// §3.42 in game: folder line j, info line 10 + j.
    Folder(usize),
}
impl InfoLine {
    /// The reference's info line number (§3.42 "Info card mapping"): 0-6
    /// SPECIAL, 7 character points or Level, 8-9 the level block, 10 on the
    /// folder lines, 43 Hit Points, 44-50 conditions, 51-60 derived, 61-78
    /// skills, 79 tag skills or skill points, 80 Skills, 81 optional traits,
    /// 82-97 traits.
    pub fn number(self) -> usize {
        match self {
            Self::Special(i) => i,
            Self::CharacterPoints => 7,
            Self::Level(k) => 7 + k,
            Self::Folder(j) => 10 + j,
            Self::Condition(k) => 43 + k,
            Self::Derived(k) => 51 + k,
            Self::Skill(s) => 61 + s,
            Self::TagSkillsCard => 79,
            Self::SkillsCard => 80,
            Self::OptionalTraits => 81,
            Self::Trait(t) => 82 + t,
        }
    }
    /// The line with a reference number; `game` picks the in-game meaning
    /// of 7-42 (the level block and folder lines).
    pub fn from_number(number: usize, game: bool) -> Option<Self> {
        Some(match number {
            0..=6 => Self::Special(number),
            7 if !game => Self::CharacterPoints,
            7..=9 if game => Self::Level(number - 7),
            10..=42 if game => Self::Folder(number - 10),
            43..=50 => Self::Condition(number - 43),
            51..=60 => Self::Derived(number - 51),
            61..=78 => Self::Skill(number - 61),
            79 => Self::TagSkillsCard,
            80 => Self::SkillsCard,
            81 => Self::OptionalTraits,
            82..=97 => Self::Trait(number - 82),
            _ => return None,
        })
    }
}

/// §3.8 info hot regions for a mouse press, under the safe rule of §3.42
/// (the condition and derived regions clamp to their own blocks).
pub fn info_hit(point: [i32; 2]) -> Option<InfoLine> {
    info_hit_with(point, true)
}

/// §3.8 info hot regions. SPEC-014 §3.42: the condition and derived
/// formulas are not clamped, so y = 145-150 gives line 51 (the Armor Class
/// card) and y = 305-309 line 61 (Small Guns); `[May-improve]` safe rule
/// (`clamp`): stop at lines 50 and 60.
pub fn info_hit_with(point: [i32; 2], clamp: bool) -> Option<InfoLine> {
    let [x, y] = point;
    if Rect::new(19, 38, 125, 227).contains(point) {
        return SPECIAL_Y.iter().enumerate().find_map(|(i, top)| {
            let slack = 5.0 - 0.25 * i as f64;
            let y = f64::from(y);
            (y >= f64::from(*top) - slack && y <= f64::from(*top) + 22.0 + slack)
                .then_some(InfoLine::Special(i))
        });
    }
    if Rect::new(28, 280, 124, 32).contains(point) {
        return Some(InfoLine::CharacterPoints);
    }
    if Rect::new(52, 324, 169, 20).contains(point) {
        return Some(InfoLine::OptionalTraits);
    }
    if Rect::new(47, 353, 245, 100).contains(point) {
        return (0..8)
            .find(|k| {
                let row = trait_label_y(*k);
                y >= row - 4 && y <= row + 10
            })
            .map(|k| InfoLine::Trait(if x >= 169 { k + 8 } else { k }));
    }
    let block = |row: i32, first: usize, last: usize| {
        let line = first + row.max(0) as usize;
        InfoLine::from_number(if clamp { line.min(last) } else { line }, false)
    };
    if Rect::new(191, 41, 122, 110).contains(point) {
        return block((y - 41) / 13, 43, 50);
    }
    if Rect::new(191, 175, 122, 135).contains(point) {
        return block((y - 175) / 13, 51, 60);
    }
    if Rect::new(376, 5, 223, 20).contains(point) {
        return Some(InfoLine::SkillsCard);
    }
    if Rect::new(370, 27, 223, 195).contains(point) {
        let skill = (f64::from(y - 27) * 0.092_307_694) as usize;
        return Some(InfoLine::Skill(skill.min(17)));
    }
    Rect::new(396, 228, 171, 25)
        .contains(point)
        .then_some(InfoLine::TagSkillsCard)
}
/// Trait label row y: `trunc(353 + 13.56 k)` within a column.
pub fn trait_label_y(k: usize) -> i32 {
    (353.0 + 13.56 * k as f64) as i32
}
/// Skill row y: names at x = 380, values at x = 573.
pub fn skill_y(k: usize) -> i32 {
    27 + 11 * k as i32
}

/// §3.8 big numbers: two `bignum.frm` cells (0-9; 11 is the rolling
/// frame). A value or previous value outside 0-99 shows "99".
pub fn big_number_cells(value: i32, previous: i32) -> [u8; 2] {
    if !(0..=99).contains(&value) || !(0..=99).contains(&previous) {
        return [9, 9];
    }
    [(value / 10) as u8, (value % 10) as u8]
}
/// Cell width and the x offset of the red set.
pub const BIG_CELL: [i32; 2] = [14, 24];
pub const BIG_RED_OFFSET: i32 = 168;
pub const ROLL_CELL: u8 = 11;
pub const ROLL_MS: u64 = 123;

/// §3.8 name caption truncation in a font of `width` per prefix: cut at the
/// first prefix whose width before the current character exceeds 100, then
/// after the last space scanned, if any.
pub fn caption_name(name: &[u8], width: impl Fn(&[u8]) -> i32) -> Vec<u8> {
    if width(name) <= 100 {
        return name.to_vec();
    }
    let mut end = name.len();
    let mut last_space = None;
    for index in 0..name.len() {
        if width(&name[..index]) > 100 {
            end = index;
            break;
        }
        if name[index] == b' ' {
            last_space = Some(index);
        }
    }
    match last_space {
        Some(space) if space < end => name[..space + 1].to_vec(),
        _ => name[..end].to_vec(),
    }
}

/// A validation or pick refusal: the `editor.msg` title and body shown in
/// the error dialog with `iisxxxx1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub title: i32,
    pub body: i32,
}

/// The creation-mode editor over a working copy of the player sheet.
#[derive(Debug, Clone)]
pub struct Editor {
    pub sheet: CharacterSheet,
    snapshot: CharacterSheet,
    points_snapshot: i32,
    pub character_points: i32,
    pub tag_picks: i32,
    pub selected: InfoLine,
    context: StatContext,
}

impl Editor {
    /// Open on `sheet`. Remaining tag picks are the trailing empty tag slots
    /// minus 1 (3 for a blank sheet, 0 for a premade; safe rule: at least
    /// 0). Character points are 5 after a reset, else the sheet's own.
    pub fn open(sheet: CharacterSheet, after_reset: bool, context: StatContext) -> Self {
        let trailing = sheet.tags.iter().rev().take_while(|tag| **tag < 0).count() as i32;
        let character_points = if after_reset {
            5
        } else {
            sheet.remaining_creation
        };
        Self {
            snapshot: sheet.clone(),
            points_snapshot: character_points,
            sheet,
            character_points,
            tag_picks: (trailing - 1).max(0),
            selected: InfoLine::Special(0),
            context,
        }
    }
    /// The shown SPECIAL value: base plus trait adjustment plus bonus,
    /// unclamped.
    pub fn shown(&self, stat: usize) -> i32 {
        (self.sheet.raw(stat, self.context) + i64::from(self.sheet.bonus[stat])) as i32
    }
    /// SPECIAL plus: needs points, a shown value below 10 and a new base of
    /// at most 10; spends a point.
    pub fn increment(&mut self, stat: usize) -> bool {
        let base = self.sheet.base[stat];
        if self.character_points <= 0 || self.shown(stat) >= 10 || base + 1 > 10 {
            return false;
        }
        // R-10: a step the sheet refuses (a base outside 1-10) spends
        // nothing.
        if !self.sheet.set_base(stat, base + 1, self.context) {
            return false;
        }
        self.character_points -= 1;
        self.selected = InfoLine::Special(stat);
        true
    }
    /// SPECIAL minus: needs a new base of at least 1; refunds a point (no
    /// upper cap).
    pub fn decrement(&mut self, stat: usize) -> bool {
        let base = self.sheet.base[stat];
        if base - 1 < 1 {
            return false;
        }
        // R-10: the sheet keeps SPECIAL bases within 1-10, so a loaded base
        // of 12 cannot step to 11; a refused step refunds nothing (else
        // every press on such a sheet minted a character point).
        if !self.sheet.set_base(stat, base - 1, self.context) {
            return false;
        }
        self.character_points += 1;
        self.selected = InfoLine::Special(stat);
        true
    }
    /// Tag button `skill`: untag (later tags shift down) or fill the first
    /// empty slot of the first three while picks remain.
    pub fn toggle_tag(&mut self, skill: i32) -> Result<(), Refusal> {
        // Safe rule: the picks left are the empty slots among the first
        // three, so a sheet that carried a fourth tag never locks Done.
        let picks = |tags: &[i32; 4]| tags[..3].iter().filter(|tag| **tag < 0).count() as i32;
        if let Some(slot) = self.sheet.tags.iter().position(|tag| *tag == skill) {
            self.sheet.tags.copy_within(slot + 1.., slot);
            self.sheet.tags[3] = -1;
            self.tag_picks = picks(&self.sheet.tags);
        } else if self.tag_picks > 0 {
            let Some(slot) = self.sheet.tags[..3].iter().position(|tag| *tag < 0) else {
                return Err(Refusal {
                    title: 140,
                    body: 141,
                });
            };
            self.sheet.tags[slot] = skill;
            self.tag_picks = picks(&self.sheet.tags);
        } else {
            return Err(Refusal {
                title: 140,
                body: 141,
            });
        }
        self.selected = InfoLine::Skill(skill as usize);
        Ok(())
    }
    /// Trait button: remove a picked trait, or fill the first empty of two.
    pub fn toggle_trait(&mut self, value: i32) -> Result<(), Refusal> {
        if let Some(slot) = self.sheet.traits.iter().position(|t| *t == value) {
            self.sheet.traits[slot] = -1;
        } else if let Some(slot) = self.sheet.traits.iter().position(|t| *t < 0) {
            self.sheet.traits[slot] = value;
        } else {
            return Err(Refusal {
                title: 148,
                body: 149,
            });
        }
        self.sheet.recompute(self.context);
        self.selected = InfoLine::Trait(value as usize);
        Ok(())
    }
    /// §3.9 Done validation, in order: unspent points, remaining tag picks,
    /// then any shown SPECIAL above 10 (only the upper bound).
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.character_points != 0 {
            return Err(Refusal {
                title: 118,
                body: 119,
            });
        }
        if self.tag_picks > 0 {
            return Err(Refusal {
                title: 142,
                body: 143,
            });
        }
        if (0..7).any(|stat| self.shown(stat) > 10) {
            return Err(Refusal {
                title: 157,
                body: 158,
            });
        }
        Ok(())
    }
    /// Done: validation, then the exit commit (heal to maximum, clear the
    /// pending level-up flag). Returns the finished sheet; a refusal leaves
    /// the editor open.
    pub fn done(&mut self) -> Result<CharacterSheet, Refusal> {
        self.validate()?;
        self.sheet.remaining_creation = self.character_points;
        self.sheet.recompute(self.context);
        self.sheet.heal(10000, self.context);
        self.sheet.flags &= !crate::progression::LEVEL_UP_FLAG;
        Ok(self.sheet.clone())
    }
    /// Cancel restores the snapshot, character points included (safe rule).
    pub fn cancel(self) -> (CharacterSheet, i32) {
        (self.snapshot, self.points_snapshot)
    }
    /// §3.11 Erase: blank defaults (5 points, SPECIAL 5, age 25, male, name
    /// "None", no tags or traits, info line 0).
    pub fn erase(&mut self) {
        self.sheet = blank_sheet(self.context);
        self.character_points = 5;
        self.tag_picks = 3;
        self.selected = InfoLine::Special(0);
    }
    /// §3.11 / SPEC-014 §3.43 Load: the player is reset to the read sheet
    /// with its bonus statistics zeroed, then recomputed and healed. The
    /// character points and tag picks are the loaded sheet's (as on
    /// opening), and the info line resets. Cancel still restores the
    /// snapshot taken on opening.
    pub fn load(&mut self, mut sheet: CharacterSheet) {
        sheet.bonus = [0; 35];
        sheet.perks.clear();
        sheet.recompute(self.context);
        sheet.heal(10000, self.context);
        let trailing = sheet.tags.iter().rev().take_while(|tag| **tag < 0).count() as i32;
        self.tag_picks = (trailing - 1).max(0);
        self.character_points = sheet.remaining_creation;
        self.sheet = sheet;
        self.selected = InfoLine::Special(0);
    }
    /// The context the editor computes with.
    pub fn context(&self) -> StatContext {
        self.context
    }
}

/// The blank player of a Create or Erase.
pub fn blank_sheet(context: StatContext) -> CharacterSheet {
    let mut sheet = CharacterSheet {
        name: b"None".to_vec(),
        ..CharacterSheet::default()
    };
    sheet.base[..7].fill(5);
    sheet.base[33] = 25;
    sheet.base[34] = 0;
    sheet.tags = [-1; 4];
    sheet.traits = [-1; 2];
    sheet.remaining_creation = 5;
    sheet.recompute(context);
    sheet.heal(10000, context);
    sheet
}

/// §3.10 name entry: at most 11 characters, codes 32-122; Backspace or
/// Delete removes the last character. Accept keeps a nonempty result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameField {
    pub text: Vec<u8>,
}
pub const NAME_LIMIT: usize = 11;
impl NameField {
    /// Prefilled with the current name unless it is "None".
    pub fn open(current: &[u8]) -> Self {
        Self {
            text: if current == b"None" {
                Vec::new()
            } else {
                current[..current.len().min(NAME_LIMIT)].to_vec()
            },
        }
    }
    pub fn key(&mut self, code: u8) {
        if (32..=122).contains(&code) && self.text.len() < NAME_LIMIT {
            self.text.push(code);
        }
    }
    pub fn backspace(&mut self) {
        self.text.pop();
    }
    /// The accepted name, or `None` to leave it unchanged.
    pub fn accept(&self) -> Option<Vec<u8>> {
        (!self.text.is_empty()).then(|| self.text.clone())
    }
}

/// §3.10 age range and steps (`+`, Right, `N` up; `-`, Left, `J` down).
pub const AGE_RANGE: (i32, i32) = (16, 35);
pub fn age_step(age: i32, up: bool) -> i32 {
    if up {
        (age + 1).min(AGE_RANGE.1)
    } else {
        (age - 1).max(AGE_RANGE.0)
    }
}

/// §3.9 / §3.10 / §3.12 held-button repeat: from frame `start`, steps at 5
/// per second rising by 1 per step to 24. Returns the delay before step
/// `n` (0-based) of the repeat.
pub fn repeat_delay(step: u32) -> u64 {
    1000 / u64::from((5 + step).min(24))
}

// SPEC-020 §2.1: the creation-mode editor's layout and dispatch. Rectangles
// are frame (screen) pixels.

/// §3.8 the name, age and sex buttons (each tested through its mask art)
/// and the press sound they share (no release sound).
pub const TOP_BUTTONS: [(EditorControl, Rect, &str, &str, &str); 3] = [
    (
        EditorControl::Name,
        Rect::new(9, 0, 145, 35),
        "nameoff.frm",
        "nameon.frm",
        "namemsk.frm",
    ),
    (
        EditorControl::Age,
        Rect::new(154, 0, 81, 32),
        "ageoff.frm",
        "ageon.frm",
        "agemask.frm",
    ),
    (
        EditorControl::Sex,
        Rect::new(235, 0, 80, 32),
        "sexoff.frm",
        "sexon.frm",
        "sexmask.frm",
    ),
];
pub const TOP_SOUND: &str = "ib3p1xx1";
/// §3.9 the SPECIAL plus and minus repeat from the 20th frame, §3.10 the
/// age arrows from the 15th.
pub const SPECIAL_IDLE_FRAMES: u32 = 20;
pub const AGE_IDLE_FRAMES: u32 = 15;
/// §3.10 the dialog windows' x (at screen y = 0), the age arrows and the
/// sex buttons (window-relative).
pub const NAME_X: i32 = 17;
pub const AGE_X: i32 = 154;
pub const SEX_X: i32 = 235;
pub const AGE_LEFT: Rect = Rect::new(19, 13, 20, 18);
pub const AGE_RIGHT: Rect = Rect::new(105, 13, 20, 18);
pub const MALE: Rect = Rect::new(22, 2, 45, 43);
pub const FEMALE: Rect = Rect::new(71, 3, 45, 42);
/// §3.11 the options window and its five buttons (`editor.msg` 600-604):
/// Save, Load, Print, Erase, Done.
pub const OPTIONS_ORIGIN: [i32; 2] = [238, 90];
pub fn options_button(i: usize) -> Rect {
    Rect::new(13, 17 + 37 * i as i32, 137, 34).offset(OPTIONS_ORIGIN)
}
/// The Erase question's Yes/No box.
pub const ERASE_ORIGIN: [i32; 2] = [169, 126];
/// SPECIAL row `stat`'s plus and minus buttons.
pub fn special_buttons(stat: usize) -> [Rect; 2] {
    let y = SPECIAL_Y[stat.min(6)];
    [Rect::new(149, y, 16, 12), Rect::new(149, y + 11, 17, 12)]
}
/// Skill `k`'s tag button.
pub fn tag_button(k: usize) -> Rect {
    Rect::new(347, 26 + 11 * k as i32, 17, 11)
}
/// Trait `k`'s button: 0-7 in the left column, 8-15 in the right.
pub fn trait_button(k: usize) -> Rect {
    let (x, row) = if k < 8 { (23, k) } else { (298, k - 8) };
    Rect::new(x, 352 + 13 * row as i32, 17, 11)
}
/// A dialog window's Done button: (26, 44), or (28, 48) in the sex dialog.
pub fn dialog_done(x: i32, sex: bool) -> Rect {
    if sex {
        super::red_button([x + 28, 48])
    } else {
        super::red_button([x + 26, 44])
    }
}
/// A control of the creation-mode editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorControl {
    Done,
    Cancel,
    Options,
    Name,
    Age,
    Sex,
    SpecialPlus(usize),
    SpecialMinus(usize),
    Tag(usize),
    Trait(usize),
}
/// The editor's controls in press order: the red buttons, the name, age
/// and sex buttons, the SPECIAL plus and minus, the tag buttons and the
/// trait buttons. The info hot regions are [`info_hit`]'s.
pub fn editor_layout() -> Layout<EditorControl> {
    let mut layout = Layout::new()
        .with(EditorControl::Done, super::red_button(OPTIONS_BUTTONS[1]))
        .with(EditorControl::Cancel, super::red_button(OPTIONS_BUTTONS[2]))
        .with(
            EditorControl::Options,
            super::red_button(OPTIONS_BUTTONS[0]),
        );
    layout.extend(
        TOP_BUTTONS
            .iter()
            .map(|(control, rect, ..)| (*control, *rect)),
    );
    for stat in 0..7 {
        let [plus, minus] = special_buttons(stat);
        layout.push(EditorControl::SpecialPlus(stat), plus);
        layout.push(EditorControl::SpecialMinus(stat), minus);
    }
    layout.extend((0..18).map(|k| (EditorControl::Tag(k), tag_button(k))));
    layout.extend((0..16).map(|k| (EditorControl::Trait(k), trait_button(k))));
    layout
}
/// The control under a press: the top buttons only where `mask` accepts
/// the point (their mask art's nonzero pixels).
pub fn editor_hit(
    point: [i32; 2],
    mask: impl Fn(EditorControl, [i32; 2]) -> bool,
) -> Option<EditorControl> {
    editor_layout()
        .controls()
        .iter()
        .find(|(control, rect)| {
            rect.contains(point)
                && (!matches!(
                    control,
                    EditorControl::Name | EditorControl::Age | EditorControl::Sex
                ) || mask(*control, point))
        })
        .map(|(control, _)| *control)
}
/// A dialog control of the creation-mode editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDialogControl {
    Done,
    AgeDown,
    AgeUp,
    Male,
    Female,
    /// An options-window button 0-4.
    Option(usize),
    EraseYes,
    EraseNo,
}
/// Which editor dialog is up, for its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDialogKind {
    Name,
    Age,
    Sex,
    Options,
    Erase,
}
/// A dialog's controls on screen, in press order.
pub fn editor_dialog_layout(kind: EditorDialogKind) -> Layout<EditorDialogControl> {
    match kind {
        EditorDialogKind::Name => {
            Layout::new().with(EditorDialogControl::Done, dialog_done(NAME_X, false))
        }
        EditorDialogKind::Age => Layout::new()
            .with(EditorDialogControl::AgeDown, AGE_LEFT.offset([AGE_X, 0]))
            .with(EditorDialogControl::AgeUp, AGE_RIGHT.offset([AGE_X, 0]))
            .with(EditorDialogControl::Done, dialog_done(AGE_X, false)),
        EditorDialogKind::Sex => Layout::new()
            .with(EditorDialogControl::Male, MALE.offset([SEX_X, 0]))
            .with(EditorDialogControl::Female, FEMALE.offset([SEX_X, 0]))
            .with(EditorDialogControl::Done, dialog_done(SEX_X, true)),
        EditorDialogKind::Options => (0..5)
            .map(|i| (EditorDialogControl::Option(i), options_button(i)))
            .collect(),
        EditorDialogKind::Erase => super::flow::DialogBox::new(ERASE_ORIGIN, true)
            .layout()
            .controls()
            .iter()
            .map(|(yes, rect)| {
                let control = if *yes {
                    EditorDialogControl::EraseYes
                } else {
                    EditorDialogControl::EraseNo
                };
                (control, *rect)
            })
            .collect(),
    }
}
/// What an editor control did.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorOutcome {
    /// A SPECIAL step happened (it repeats while held) or was refused.
    Stepped(bool),
    /// A tag or trait toggled.
    Toggled,
    /// A pick was refused: the error box with `iisxxxx1`.
    Refused(Refusal),
    /// Done passed validation: the finished sheet.
    Finished(Box<CharacterSheet>),
    Cancel,
    /// Open the options window, or the name, age or sex dialog.
    Open(EditorDialogKind),
}
impl Editor {
    /// Act on a control: the SPECIAL steps, tag and trait toggles and Done
    /// change the sheet; the rest open their windows.
    pub fn dispatch(&mut self, control: EditorControl) -> EditorOutcome {
        match control {
            EditorControl::SpecialPlus(stat) => EditorOutcome::Stepped(self.increment(stat)),
            EditorControl::SpecialMinus(stat) => EditorOutcome::Stepped(self.decrement(stat)),
            EditorControl::Tag(k) => match self.toggle_tag(k as i32) {
                Ok(()) => EditorOutcome::Toggled,
                Err(refusal) => EditorOutcome::Refused(refusal),
            },
            EditorControl::Trait(k) => match self.toggle_trait(k as i32) {
                Ok(()) => EditorOutcome::Toggled,
                Err(refusal) => EditorOutcome::Refused(refusal),
            },
            EditorControl::Done => match self.done() {
                Ok(sheet) => EditorOutcome::Finished(Box::new(sheet)),
                Err(refusal) => EditorOutcome::Refused(refusal),
            },
            EditorControl::Cancel => EditorOutcome::Cancel,
            EditorControl::Options => EditorOutcome::Open(EditorDialogKind::Options),
            EditorControl::Name => EditorOutcome::Open(EditorDialogKind::Name),
            EditorControl::Age => EditorOutcome::Open(EditorDialogKind::Age),
            EditorControl::Sex => EditorOutcome::Open(EditorDialogKind::Sex),
        }
    }
    /// The age dialog's arrow: one step within 16-35 (`true` when it
    /// moved, so a held arrow keeps repeating).
    pub fn step_age(&mut self, up: bool) -> bool {
        let age = self.sheet.base[33];
        let next = age_step(age, up);
        self.sheet.base[33] = next;
        next != age
    }
    /// The sex dialog's buttons apply on the press.
    pub fn set_female(&mut self, female: bool) {
        self.sheet.base[34] = i32::from(female);
    }
}
