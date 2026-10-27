//! SPEC-014 §3.43 character sheet save, load and print: the file list, the
//! load and save dialogs' list paging and name field, the written file
//! names, the `.GCD` sheet of a player, and the printed sheet.
use super::character::grouped;
use crate::CharacterSheet;
use vault_formats::GcdFile;

/// Both file dialogs open here, as modal windows.
pub const DIALOG_ORIGIN: [i32; 2] = [168, 80];
/// Every message box of the three operations.
pub const MESSAGE_ORIGIN: [i32; 2] = [169, 126];
/// Visible list rows.
pub const FILE_ROWS: usize = 12;
/// The reference reserves 13 bytes per name.
pub const NAME_LIMIT: usize = 12;
/// The save dialog's field takes typed keys while it is shorter than this.
pub const FIELD_TYPING: usize = 8;
/// Free-running idle frames between forgotten clicks.
pub const IDLE_FRAMES: u64 = 32;

/// The two file patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetFile {
    /// `*.GCD`, a character sheet.
    Sheet,
    /// `*.TXT`, a printed sheet.
    Print,
}
impl SheetFile {
    pub fn extension(self) -> &'static [u8; 3] {
        match self {
            Self::Sheet => b"GCD",
            Self::Print => b"TXT",
        }
    }
}

fn has_extension(name: &[u8], extension: &[u8; 3]) -> bool {
    name.len() > 4 && name[name.len() - 3..].eq_ignore_ascii_case(extension)
}

/// The file list: root entries of the master archive whose stored name is
/// longer than 4 characters and ends in the extension (the dot is not
/// checked), and loose files matching `*.EXT` in the master patch root,
/// merged, sorted case-insensitively, each name once, as stored. A loose
/// name longer than 12 characters overflows the reference's list;
/// `[May-improve]` safe rule: leave it out.
pub fn file_list<'a>(
    archive_root: impl IntoIterator<Item = &'a [u8]>,
    loose: impl IntoIterator<Item = &'a [u8]>,
    kind: SheetFile,
) -> Vec<Vec<u8>> {
    let extension = kind.extension();
    let archive = archive_root
        .into_iter()
        .filter(|name| has_extension(name, extension));
    let loose = loose.into_iter().filter(|name| {
        name.len() <= NAME_LIMIT
            && name.len() >= 4
            && name[name.len() - 4] == b'.'
            && name[name.len() - 3..].eq_ignore_ascii_case(extension)
    });
    let mut names: Vec<Vec<u8>> = Vec::new();
    for name in archive.chain(loose) {
        if !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            names.push(name.to_vec());
        }
    }
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names
}

/// Which file dialog a list belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Load,
    Save,
}

/// §3.43 the file dialogs' shared list rules: the selected file is
/// `top + row`; the list scrolls past 12 files with maximum top M. The
/// reference's M is `n - 13`, so the last file of a long list is never
/// drawn; `[May-improve]` safe rule (`safe`): M is `n - 12`, and the row
/// stays within 0-11 and within the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileList {
    pub kind: DialogKind,
    pub n: usize,
    pub top: usize,
    pub row: usize,
    safe: bool,
    /// The row of the previous click, until forgotten.
    click: Option<usize>,
    /// The free-running idle-frame counter.
    idle_frames: u64,
}
impl FileList {
    pub fn new(kind: DialogKind, n: usize, safe: bool) -> Self {
        Self {
            kind,
            n,
            top: 0,
            row: 0,
            safe,
            click: None,
            idle_frames: 0,
        }
    }
    fn scrolls(&self) -> bool {
        self.n > FILE_ROWS
    }
    /// The maximum top while scrolling.
    pub fn max_top(&self) -> usize {
        if self.safe {
            self.n.saturating_sub(FILE_ROWS)
        } else {
            self.n.saturating_sub(FILE_ROWS + 1)
        }
    }
    /// The selected entry (which may be past the drawn rows).
    pub fn selected(&self) -> Option<usize> {
        let entry = self.top + self.row;
        (entry < self.n).then_some(entry)
    }
    /// The entries drawn: rows 0-11 from the top.
    pub fn visible(&self) -> std::ops::Range<usize> {
        self.top..(self.top + FILE_ROWS).min(self.n)
    }
    fn clamp(&mut self) {
        if self.safe {
            self.row = self.row.min(FILE_ROWS - 1);
            if self.top + self.row >= self.n {
                self.row = self.n.saturating_sub(self.top + 1);
            }
        }
    }
    /// Up, by the key or one arrow step.
    pub fn up(&mut self) {
        self.click = None;
        if self.top > 0 {
            self.top -= 1;
        } else {
            self.row = self.row.saturating_sub(1);
        }
    }
    /// Down by one arrow step.
    pub fn down_arrow(&mut self) {
        self.click = None;
        if self.scrolls() {
            if self.top < self.max_top() {
                self.top += 1;
            } else {
                self.row = (self.row + 1).min(FILE_ROWS - 1);
            }
        } else {
            self.row = (self.row + 1).min(self.n.saturating_sub(1));
        }
        self.clamp();
    }
    /// Down by the key: as the arrow, except that the load dialog lets the
    /// row reach 12 (not drawn) before it clamps to 11, and the save dialog
    /// moves the row on the very press that reaches M.
    pub fn down_key(&mut self) {
        if !self.scrolls() {
            return self.down_arrow();
        }
        self.click = None;
        let max = self.max_top();
        if self.top < max {
            self.top += 1;
            if self.kind == DialogKind::Save && self.top == max {
                self.row = (self.row + 1).min(FILE_ROWS - 1);
            }
        } else if self.kind == DialogKind::Load {
            self.row = if self.row >= FILE_ROWS {
                FILE_ROWS - 1
            } else {
                self.row + 1
            };
        } else {
            self.row = (self.row + 1).min(FILE_ROWS - 1);
        }
        self.clamp();
    }
    pub fn home(&mut self) {
        self.click = None;
        self.top = 0;
        self.row = 0;
    }
    pub fn end(&mut self) {
        self.click = None;
        if self.scrolls() {
            self.top = self.max_top();
            self.row = FILE_ROWS - 1;
        } else {
            self.top = 0;
            self.row = self.n.saturating_sub(1);
        }
        self.clamp();
    }
    /// A click released in the list area at screen y: row
    /// `trunc((y - 80 - 49) / 10)`, at least 0; at most 11 when scrolling,
    /// else at most the last file. Returns whether it chose the file (a
    /// second click on the same row). With no files clicks do nothing.
    pub fn click(&mut self, screen_y: i32) -> bool {
        if self.n == 0 {
            return false;
        }
        let row = ((screen_y - 80 - 49) / 10).max(0) as usize;
        self.row = if self.scrolls() {
            row.min(FILE_ROWS - 1)
        } else {
            row.min(self.n - 1)
        };
        self.clamp();
        if self.click == Some(self.row) {
            self.click = None;
            return true;
        }
        self.click = Some(self.row);
        false
    }
    /// One idle 41-millisecond frame: every 32nd frame forgets the click.
    pub fn idle_frame(&mut self) {
        self.idle_frames += 1;
        if self.idle_frames.is_multiple_of(IDLE_FRAMES) {
            self.click = None;
        }
    }
}

/// §3.43 the save dialog's name field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameField {
    pub text: Vec<u8>,
}
impl NameField {
    /// The default name, cut to its first 12 characters.
    pub fn open(default: &[u8]) -> Self {
        Self {
            text: default[..default.len().min(NAME_LIMIT)].to_vec(),
        }
    }
    /// A typed key: appended while the field holds fewer than 8 characters,
    /// when it is a code 32-122 that is an ASCII letter or digit or one of
    /// ``# @ ! $ ` ' ~ ^ & ( ) - _ = [ ] { }``. Space is not accepted.
    pub fn key(&mut self, code: u8) -> bool {
        let accepted = (32..=122).contains(&code)
            && (code.is_ascii_alphanumeric() || b"#@!$`'~^&()-_=[]{}".contains(&code));
        if accepted && self.text.len() < FIELD_TYPING {
            self.text.push(code);
            return true;
        }
        false
    }
    /// Backspace or Delete removes the last character.
    pub fn backspace(&mut self) {
        self.text.pop();
    }
    /// A single click on a file puts its name in the field, cut at its
    /// first `.` and to 12 characters; typing then waits until erasing
    /// brings the field below 8 characters.
    pub fn take(&mut self, file: &[u8]) {
        self.text = base_name(file);
    }
    /// Done or Return: the field, or `None` (cancel) when it is empty.
    pub fn accept(&self) -> Option<Vec<u8>> {
        (!self.text.is_empty()).then(|| self.text.clone())
    }
}

/// A listed file's name cut at its first `.` and to 12 characters.
pub fn base_name(file: &[u8]) -> Vec<u8> {
    let end = file.iter().position(|b| *b == b'.').unwrap_or(file.len());
    file[..end.min(NAME_LIMIT)].to_vec()
}

/// The returned name with `.GCD` or `.TXT` appended.
pub fn with_extension(name: &[u8], kind: SheetFile) -> Vec<u8> {
    let mut file = name.to_vec();
    file.push(b'.');
    file.extend_from_slice(kind.extension());
    file
}

/// The file name a write uses in the master patch root. The reference file
/// system ignores case; `[May-improve]` safe rule for a case-sensitive one:
/// an existing loose name matching without regard to case is reused, and
/// a new name is written in upper case.
pub fn written_name<'a>(file: &[u8], existing: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    existing
        .into_iter()
        .find(|name| name.eq_ignore_ascii_case(file))
        .map_or_else(|| file.to_ascii_uppercase(), <[u8]>::to_vec)
}

/// The SPEC-006 §2.2 sheet of a player: its flags, base and bonus
/// statistics, invested skill points, name (NUL-terminated, at most 31
/// bytes), tags, traits and character points; the critter-only words are
/// zero. Under Fallout 2 it is SPEC-018 §6.3's 432-byte sheet (Revision 4),
/// whose damage-type word is 0 (normal).
pub fn sheet_gcd(sheet: &CharacterSheet, game: crate::GameProfile) -> GcdFile {
    let mut name = [0; 32];
    let length = sheet.name.len().min(31);
    name[..length].copy_from_slice(&sheet.name[..length]);
    GcdFile {
        flags: sheet.flags,
        base: sheet.base,
        bonus: sheet.bonus,
        skills: sheet.invested,
        body: 0,
        experience_reward: 0,
        kill_category: 0,
        damage_type: (game == crate::GameProfile::Fallout2).then_some(0),
        name,
        tags: sheet.tags,
        traits: sheet.traits,
        remaining_points: sheet.remaining_creation,
    }
}

/// What the printed sheet shows: the player as currently shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrintedSheet {
    pub name: Vec<u8>,
    pub age: i32,
    /// 0 male, 1 female.
    pub gender: i32,
    pub level: i32,
    pub experience: i32,
    /// Effective SPECIAL, ST through LK.
    pub special: [i32; 7],
    pub hit_points: i32,
    pub max_hit_points: i32,
    pub armor_class: i32,
    pub healing_rate: i32,
    pub action_points: i32,
    pub critical_chance: i32,
    pub melee_damage: i32,
    pub carry_weight: i32,
    pub damage_resistance: i32,
    pub radiation_resistance: i32,
    pub poison_resistance: i32,
    pub sequence: i32,
    /// The two trait slots' names (`None` for an empty slot).
    pub traits: [Option<Vec<u8>>; 2],
    /// Held perks in perk-index order: name and rank.
    pub perks: Vec<(Vec<u8>, u32)>,
    /// The 18 skills: name and value.
    pub skills: Vec<(Vec<u8>, i32)>,
    /// Kill types with a nonzero count, in kill-type order: name, count.
    pub kills: Vec<(Vec<u8>, i32)>,
    /// Inventory entries in stored order: quantity and item name.
    pub inventory: Vec<(i32, Vec<u8>)>,
    pub weight: i64,
    pub day: i32,
    pub month: i32,
    pub year: i32,
    /// `100 * (trunc(t / 36000) mod 24) + trunc(t / 600) mod 60`.
    pub hhmm: i32,
}

/// The game time HHMM of tick `t`.
pub fn hhmm(ticks: u64) -> i32 {
    (100 * ((ticks / 36000) % 24) + (ticks / 600) % 60) as i32
}

/// At least `digits` digits with leading zeros; a negative value takes a
/// leading `-`, as in `-05`.
fn padded(value: i64, digits: usize) -> String {
    if value < 0 {
        format!("-{:0digits$}", value.unsigned_abs())
    } else {
        format!("{value:0digits$}")
    }
}

/// `name`, a space, and dots up to 16 characters in all.
fn dotted(name: &[u8]) -> Vec<u8> {
    let mut text = name.to_vec();
    text.push(b' ');
    while text.len() < 16 {
        text.push(b'.');
    }
    text
}

/// §3.43 the printed sheet, every line ending in CR LF. `message` gives
/// `editor.msg` text (a missing message reads `Error`, SPEC-002). With
/// `safe`, the four `[May-improve]` safe rules apply together: the
/// Sequence column prints the sequence, the skill rows are written (each
/// paired with the next nonzero kill type in kill-type order), the
/// headings 650-653 are assigned by meaning, and Next Level prints
/// `------` at level 21.
pub fn print_sheet(sheet: &PrintedSheet, message: impl Fn(i32) -> Vec<u8>, safe: bool) -> Vec<u8> {
    let mut lines: Vec<Vec<u8>> = vec![Vec::new(), Vec::new()];
    let centered = |text: Vec<u8>| {
        let spaces = ((80 - text.len() as i64) / 2 - 2).max(0) as usize;
        let mut line = vec![b' '; spaces];
        line.extend(text);
        line
    };
    let mut date = format!("{} ", padded(i64::from(sheet.day), 2)).into_bytes();
    date.extend(message(500 + sheet.month - 1));
    date.extend(format!(" {}  {} ", sheet.year, padded(i64::from(sheet.hhmm), 4)).bytes());
    date.extend(message(622));
    lines.push(centered(message(620)));
    lines.push(centered(message(621)));
    lines.push(centered(date));
    lines.push(Vec::new());
    // The name line: "  Name:", the name, padded to 27; age; gender.
    let mut line = message(642);
    line.push(b' ');
    line.extend_from_slice(&sheet.name);
    while line.len() < 27 {
        line.push(b' ');
    }
    line.extend(message(643));
    line.extend(format!(" {}", sheet.age).bytes());
    line.extend(message(644));
    line.push(b' ');
    line.extend(message(645 + sheet.gender.clamp(0, 1)));
    lines.push(line);
    // The level line.
    let experience = grouped(i64::from(sheet.experience));
    let next = crate::progression::xp_threshold(sheet.level);
    let next = if next < 0 && safe {
        "------".to_string()
    } else {
        grouped(i64::from(next))
    };
    let mut line = message(647);
    line.extend(format!(" {} ", padded(i64::from(sheet.level), 2)).bytes());
    line.extend(message(648));
    line.extend(format!(" {experience} ").bytes());
    line.extend(std::iter::repeat_n(
        b' ',
        12usize.saturating_sub(experience.len()),
    ));
    line.extend(message(649));
    line.extend(format!(" {next}").bytes());
    lines.push(line);
    lines.push(Vec::new());
    // Seven statistic lines: each label, a space and the value, the items
    // separated by single spaces.
    let s = &sheet.special;
    let d2 = |value: i32| padded(i64::from(value), 2);
    let d3 = |value: i32| padded(i64::from(value), 3);
    let sequence = if safe { d2(sheet.sequence) } else { d2(s[0]) };
    let rows: [Vec<(i32, String)>; 7] = [
        vec![
            (624, d2(s[0])),
            (
                625,
                format!("{}/{}", d3(sheet.hit_points), d3(sheet.max_hit_points)),
            ),
            (626, sequence),
        ],
        vec![
            (627, d2(s[1])),
            (628, d3(sheet.armor_class)),
            (629, d2(sheet.healing_rate)),
        ],
        vec![
            (630, d2(s[2])),
            (631, d2(sheet.action_points)),
            (632, format!("{}%", d3(sheet.critical_chance))),
        ],
        vec![
            (633, d2(s[3])),
            (634, d2(sheet.melee_damage)),
            (635, format!("{} lbs.", d3(sheet.carry_weight))),
        ],
        vec![
            (636, d2(s[4])),
            (637, format!("{}%", d3(sheet.damage_resistance))),
        ],
        vec![
            (638, d2(s[5])),
            (639, format!("{}%", d3(sheet.radiation_resistance))),
        ],
        vec![
            (640, d2(s[6])),
            (641, format!("{}%", d3(sheet.poison_resistance))),
        ],
    ];
    for row in rows {
        let mut line = Vec::new();
        for (index, (id, value)) in row.into_iter().enumerate() {
            if index > 0 {
                line.push(b' ');
            }
            line.extend(message(id));
            line.push(b' ');
            line.extend(value.bytes());
        }
        lines.push(line);
    }
    lines.extend([Vec::new(), Vec::new()]);
    let indented = |name: &[u8]| [b"  ".as_slice(), name].concat();
    let traits: Vec<Vec<u8>> = sheet
        .traits
        .iter()
        .flatten()
        .map(|name| indented(name))
        .collect();
    let perks: Vec<Vec<u8>> = sheet
        .perks
        .iter()
        .map(|(name, rank)| {
            let mut line = indented(name);
            if *rank > 1 {
                line.extend(format!(" ({rank})").bytes());
            }
            line
        })
        .collect();
    let mut inventory = Vec::new();
    for row in sheet.inventory.chunks(3) {
        let mut line = Vec::new();
        for (quantity, name) in row {
            let mut cell = format!("  {}x ", grouped(i64::from(*quantity))).into_bytes();
            cell.extend_from_slice(name);
            while cell.len() < 25 {
                cell.push(b' ');
            }
            line.extend(cell);
        }
        inventory.push(line);
    }
    let total = |label: i32| {
        let mut line = message(label);
        line.extend(format!(" {} lbs.", sheet.weight).bytes());
        line
    };
    if safe {
        if !traits.is_empty() || !perks.is_empty() {
            lines.push(message(650));
            lines.extend(traits);
            lines.extend(perks);
        }
        lines.extend([Vec::new(), Vec::new(), message(651)]);
        let mut kills = sheet.kills.iter();
        for (name, value) in &sheet.skills {
            let mut line = indented(&dotted(name));
            line.extend(format!(" {}%", d3(*value)).bytes());
            if let Some((kill, count)) = kills.next() {
                line.extend(b"        ");
                line.extend(dotted(kill));
                line.extend(format!(" {}", d3(*count)).bytes());
            }
            lines.push(line);
        }
        lines.extend([Vec::new(), Vec::new(), message(652)]);
        lines.extend(inventory);
        lines.extend([Vec::new(), total(653)]);
    } else {
        if sheet.traits[0].is_some() {
            lines.push(message(650));
            lines.extend(traits);
        }
        if !perks.is_empty() {
            lines.push(message(651));
            lines.extend(perks);
        }
        lines.extend([Vec::new(), message(652), Vec::new()]);
        lines.push(message(653));
        lines.extend([Vec::new(), Vec::new(), message(654)]);
        lines.extend(inventory);
        lines.extend([Vec::new(), total(655)]);
    }
    lines.extend([Vec::new(), Vec::new()]);
    let mut text = Vec::new();
    for line in lines {
        text.extend(line);
        text.extend(b"\r\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_lists_merge_both_sources() {
        let archive: [&[u8]; 4] = [b"PLAYER.GCD", b"XGCD", b"NOTES.TXT", b"ABGCD"];
        let loose: [&[u8]; 4] = [b"player.gcd", b"Alpha.GCD", b"VERYLONGNAME.GCD", b"b.gcd"];
        let list = file_list(archive, loose, SheetFile::Sheet);
        assert_eq!(list, [&b"ABGCD"[..], b"Alpha.GCD", b"b.gcd", b"PLAYER.GCD"]);
        assert_eq!(
            file_list(archive, [], SheetFile::Print),
            [b"NOTES.TXT".to_vec()]
        );
    }

    fn walk(kind: DialogKind, safe: bool) -> Vec<(usize, usize, Option<usize>)> {
        let mut list = FileList::new(kind, 14, safe);
        let mut steps = Vec::new();
        list.down_key();
        steps.push((list.top, list.row, list.selected()));
        for _ in 0..11 {
            list.down_key();
        }
        steps.push((list.top, list.row, list.selected()));
        list.down_key();
        steps.push((list.top, list.row, list.selected()));
        steps
    }

    #[test]
    fn fixture_o_file_dialog_paging() {
        let load = FileList::new(DialogKind::Load, 14, false);
        assert_eq!(load.max_top(), 1);
        assert_eq!(
            walk(DialogKind::Load, false),
            [(1, 0, Some(1)), (1, 11, Some(12)), (1, 12, Some(13))]
        );
        assert_eq!(
            walk(DialogKind::Save, false),
            [(1, 1, Some(2)), (1, 11, Some(12)), (1, 11, Some(12))]
        );
        let mut list = FileList::new(DialogKind::Load, 14, false);
        list.end();
        assert_eq!(list.visible(), 1..13);
        // The safe rule: M = 2 and F13 is the last row of top 2.
        let mut safe = FileList::new(DialogKind::Load, 14, true);
        assert_eq!(safe.max_top(), 2);
        safe.end();
        assert_eq!((safe.top, safe.row, safe.selected()), (2, 11, Some(13)));
        assert_eq!(safe.visible(), 2..14);
        for _ in 0..3 {
            safe.down_key();
        }
        assert_eq!(safe.row, 11);
    }

    #[test]
    fn file_list_clicks_and_short_lists() {
        let mut list = FileList::new(DialogKind::Load, 3, true);
        // Row from y = 80 + 49 + 10r; past the last file it is the last.
        assert!(!list.click(80 + 49 + 55));
        assert_eq!(list.row, 2);
        assert!(list.click(80 + 49 + 25));
        list.end();
        assert_eq!((list.top, list.row), (0, 2));
        list.up();
        list.down_arrow();
        list.down_arrow();
        assert_eq!(list.row, 2);
        // A forgotten click needs two more.
        assert!(!list.click(130));
        for _ in 0..IDLE_FRAMES {
            list.idle_frame();
        }
        assert!(!list.click(130));
        let mut empty = FileList::new(DialogKind::Save, 0, true);
        assert!(!empty.click(130) && !empty.click(130));
        assert_eq!(empty.selected(), None);
    }

    #[test]
    fn the_save_field_and_written_names() {
        let mut field = NameField::open(b"PLAYERWITHALONGNAME");
        assert_eq!(field.text, b"PLAYERWITHAL");
        assert!(!field.key(b'x'));
        field = NameField::open(b"PLAYER");
        assert!(field.key(b'1'));
        assert!(!field.key(b' '), "space is not accepted");
        assert!(!field.key(b'{'), "123 is above 122");
        assert!(field.key(b'#'));
        assert!(!field.key(b'z'), "eight characters");
        field.backspace();
        assert_eq!(field.text, b"PLAYER1");
        field.take(b"CHARACTERS.GCD");
        assert_eq!(field.text, b"CHARACTERS");
        assert!(!field.key(b'a'));
        field.text.clear();
        assert_eq!(field.accept(), None);
        assert_eq!(with_extension(b"alex", SheetFile::Print), b"alex.TXT");
        let existing: [&[u8]; 1] = [b"Alex.txt"];
        assert_eq!(written_name(b"alex.TXT", existing), b"Alex.txt");
        assert_eq!(written_name(b"bob.TXT", existing), b"BOB.TXT");
        assert_eq!(base_name(b"PLAYER.GCD"), b"PLAYER");
    }

    #[test]
    fn a_player_sheet_round_trips_through_its_gcd() {
        let mut sheet = CharacterSheet {
            name: b"Alex".to_vec(),
            remaining_creation: 2,
            ..CharacterSheet::default()
        };
        sheet.tags = [0, 3, 8, -1];
        sheet.traits = [8, -1];
        // Fallout 1 writes SPEC-006's 428 bytes, Fallout 2 SPEC-018 §6.3's
        // 432 (Revision 4); each reads back the same player.
        for (game, length) in [
            (crate::GameProfile::Fallout1, 428),
            (crate::GameProfile::Fallout2, 432),
        ] {
            let bytes = sheet_gcd(&sheet, game).write();
            assert_eq!(bytes.len(), length);
            let read = GcdFile::parse(&bytes).unwrap();
            assert_eq!(read.name_bytes(), b"Alex");
            assert_eq!((read.tags, read.traits), (sheet.tags, sheet.traits));
            assert_eq!(read.remaining_points, 2);
            assert_eq!(read.base, sheet.base);
            assert_eq!(read.damage_type, (length == 432).then_some(0));
        }
    }

    /// The English corpus `editor.msg` texts Fixture P prints.
    fn corpus(id: i32) -> Vec<u8> {
        let text: &str = match id {
            511 => "December",
            620 => "FALLOUT",
            621 => "VAULT-13 PERSONNEL RECORD",
            622 => "hours",
            624 => "       Strength:",
            625 => "        Hit Points:",
            626 => "        Sequence:",
            627 => "     Perception:",
            628 => "       Armor Class:",
            629 => "        Healing Rate:",
            630 => "      Endurance:",
            631 => "     Action Points:",
            632 => "      Critical Chance:",
            633 => "       Charisma:",
            634 => "      Melee Damage:",
            635 => "         Carry Weight:",
            636 => "   Intelligence:",
            637 => "       Damage Res.:",
            638 => "        Agility:",
            639 => "    Radiation Res.:",
            640 => "           Luck:",
            641 => "       Poison Res.:",
            642 => "  Name:",
            643 => "Age:",
            644 => "               Gender:",
            645 => "Male",
            646 => "Female",
            647 => " Level:",
            648 => "                Exp:",
            649 => "Next Level:",
            650 => " ::: Traits :::           ::: Perks :::           ::: Karma :::",
            651 => " ::: Skills :::                ::: Kills :::",
            652 => " ::: Inventory :::",
            653 => "                                      Total Weight:",
            _ => "Error",
        };
        text.as_bytes().to_vec()
    }

    fn alex() -> PrintedSheet {
        let text = |s: &str| s.as_bytes().to_vec();
        PrintedSheet {
            name: text("Alex"),
            age: 25,
            gender: 0,
            level: 6,
            experience: 15500,
            special: [6, 7, 5, 4, 8, 7, 3],
            hit_points: 47,
            max_hit_points: 51,
            armor_class: 7,
            healing_rate: 1,
            action_points: 8,
            critical_chance: 3,
            melee_damage: 1,
            carry_weight: 175,
            damage_resistance: 0,
            radiation_resistance: 10,
            poison_resistance: 25,
            sequence: 18,
            traits: [Some(text("Bloody Mess")), Some(text("Fast Shot"))],
            perks: vec![(text("Earlier Sequence"), 2)],
            skills: vec![
                (text("Small Guns"), 55),
                (text("Big Guns"), 10),
                (text("Energy Weapons"), 10),
            ],
            kills: vec![(text("Radscorpions"), 2), (text("Rats"), 5)],
            inventory: vec![
                (3, text("Stimpak")),
                (1, text("10mm Pistol")),
                (24, text("10mm JHP")),
                (1, text("Leather Jacket")),
            ],
            weight: 18,
            day: 12,
            month: 12,
            year: 2161,
            hhmm: hhmm(9 * 36000 + 30 * 600),
        }
    }

    const HEAD: [&str; 18] = [
        "",
        "",
        "                                  FALLOUT",
        "                         VAULT-13 PERSONNEL RECORD",
        "                        12 December 2161  0930 hours",
        "",
        "  Name: Alex               Age: 25               Gender: Male",
        " Level: 06                 Exp: 15,500       Next Level: 21,000",
        "",
        "       Strength: 06         Hit Points: 047/051         Sequence: 06",
        "     Perception: 07        Armor Class: 007         Healing Rate: 01",
        "      Endurance: 05      Action Points: 08       Critical Chance: 003%",
        "       Charisma: 04       Melee Damage: 01          Carry Weight: 175 lbs.",
        "   Intelligence: 08        Damage Res.: 000%",
        "        Agility: 07     Radiation Res.: 010%",
        "           Luck: 03        Poison Res.: 025%",
        "",
        "",
    ];

    fn joined(lines: &[&str]) -> Vec<u8> {
        lines
            .iter()
            .flat_map(|line| [line.as_bytes(), b"\r\n"].concat())
            .collect()
    }

    #[test]
    fn fixture_p_printed_sheet_reference() {
        let tail = [
            " ::: Traits :::           ::: Perks :::           ::: Karma :::",
            "  Bloody Mess",
            "  Fast Shot",
            " ::: Skills :::                ::: Kills :::",
            "  Earlier Sequence (2)",
            "",
            " ::: Inventory :::",
            "",
            "                                      Total Weight:",
            "",
            "",
            "Error",
            "  3x Stimpak               1x 10mm Pistol           24x 10mm JHP           ",
            "  1x Leather Jacket      ",
            "",
            "Error 18 lbs.",
            "",
            "",
        ];
        let expected = joined(&[HEAD.as_slice(), &tail].concat());
        assert_eq!(
            String::from_utf8(print_sheet(&alex(), corpus, false)).unwrap(),
            String::from_utf8(expected).unwrap()
        );
    }

    #[test]
    fn fixture_p_printed_sheet_safe_rules() {
        let mut head = HEAD;
        head[9] = "       Strength: 06         Hit Points: 047/051         Sequence: 18";
        let tail = [
            " ::: Traits :::           ::: Perks :::           ::: Karma :::",
            "  Bloody Mess",
            "  Fast Shot",
            "  Earlier Sequence (2)",
            "",
            "",
            " ::: Skills :::                ::: Kills :::",
            "  Small Guns ..... 055%        Radscorpions ... 002",
            "  Big Guns ....... 010%        Rats ........... 005",
            "  Energy Weapons . 010%",
            "",
            "",
            " ::: Inventory :::",
            "  3x Stimpak               1x 10mm Pistol           24x 10mm JHP           ",
            "  1x Leather Jacket      ",
            "",
            "                                      Total Weight: 18 lbs.",
            "",
            "",
        ];
        let expected = joined(&[head.as_slice(), &tail].concat());
        assert_eq!(
            String::from_utf8(print_sheet(&alex(), corpus, true)).unwrap(),
            String::from_utf8(expected).unwrap()
        );
        // Next Level at level 21: empty in the reference, `------` safe.
        let mut top = alex();
        top.level = 21;
        let reference = print_sheet(&top, corpus, false);
        assert!(reference.windows(14).any(|w| w == b"Next Level: \r\n"));
        let safe = print_sheet(&top, corpus, true);
        assert!(safe.windows(18).any(|w| w == b"Next Level: ------"));
        assert_eq!(padded(-5, 2), "-05");
    }
}
