//! SPEC-014 §3.8-3.11 the character editor in creation mode, as the game
//! flow (`game.rs`) runs it: the main window's buttons and keys, the name,
//! age and sex dialogs, the editor options dialog, the refusal box, and the
//! held-button repeat of the SPECIAL and age steps. The rules (points,
//! steps, picks, validation) are `vault_core::ui::editor`'s.
//!
//! The pieces the in-game character screen shares live here too: the
//! SPECIAL column, condition and skill lists, the info card (SPEC-014
//! §3.42 mapping), the §3.13 message boxes, and the §3.43 sheet files: the
//! load and save file dialogs, the file operations in the master patch
//! root, and the printed sheet's inputs.
use crate::interface::Painter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vault_core::{
    CharacterSheet, Perk,
    assets::{VfsWorldAssets, WorldAssets},
    stats::StatContext,
    ui::{
        BUTTON_PRESS, BUTTON_RELEASE, ColorKey, ERROR_SOUND, HoldRepeat, Rect,
        character::{self, Card, Title},
        editor::{self, Editor, InfoLine, NameField},
        flow::{self, DialogSize},
        sheet::{self, DialogKind, FileList, PrintedSheet, SheetFile},
    },
};
use vault_formats::Vfs;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The dialog open over the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditorDialog {
    /// §3.10 the name field.
    Name(NameField),
    /// §3.10 the age dialog, with the age on entry (Esc restores it).
    Age { entry: i32 },
    /// §3.10 the sex dialog (a radio group acting on press).
    Sex,
    /// §3.11 the options dialog; `erase` is its open Yes/No question.
    Options { erase: bool },
}

/// A held step button: a SPECIAL plus or minus, or an age arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Special { stat: usize, up: bool },
    Age { up: bool },
}

pub(crate) struct EditorStage {
    pub editor: Editor,
    pub dialog: Option<EditorDialog>,
    /// A validation or pick refusal on screen.
    pub refusal: Option<editor::Refusal>,
    /// The step button held, its screen rectangle and repeat.
    repeat: Option<(Step, Rect, HoldRepeat)>,
    /// The button pressed and waiting for its release.
    pressed: Option<Press>,
    /// Interface sounds for the game flow to play.
    pub sounds: Vec<&'static str>,
    /// SPEC-014 §3.43 the sheet files (`None`: SAVE, LOAD and PRINT TO
    /// FILE do nothing).
    files: Option<SheetFiles>,
    /// The file dialog open over the options dialog, and its operation.
    pub file: Option<(SheetOp, FileDialog)>,
    /// A §3.43 message or overwrite question over the options dialog.
    pub notice: Option<(Notice, AfterNotice)>,
    /// The last time a press or frame reported, in milliseconds.
    now: u64,
}

/// The three file operations of the options dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetOp {
    Save,
    Load,
    Print,
}

/// What a closed notice leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AfterNotice {
    /// Back to the options dialog.
    Options,
    /// The options dialog closes too.
    CloseOptions,
    /// Yes writes the file of the operation; No returns to the options
    /// dialog.
    Overwrite(SheetOp, Vec<u8>),
}

/// A button that acts on its release over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Press {
    Done,
    Cancel,
    Options,
    Name,
    Age,
    Sex,
    DialogDone,
    OptionsButton(usize),
    EraseYes,
    EraseNo,
    RefusalDone,
}

pub(crate) enum EditorExit {
    Stay(bool),
    Done(Box<CharacterSheet>),
    Cancel,
}

/// §3.8 element positions.
pub(crate) const EDITOR_DONE: Rect = Rect::new(455, 454, 15, 16);
pub(crate) const EDITOR_CANCEL: Rect = Rect::new(552, 454, 15, 16);
pub(crate) const EDITOR_OPTIONS: Rect = Rect::new(343, 454, 15, 16);
/// The name, age and sex buttons: rectangle, off and on art and mask.
const TOP_BUTTONS: [(Rect, &str, &str, &str); 3] = [
    (
        Rect::new(9, 0, 145, 35),
        "nameoff.frm",
        "nameon.frm",
        "namemsk.frm",
    ),
    (
        Rect::new(154, 0, 81, 32),
        "ageoff.frm",
        "ageon.frm",
        "agemask.frm",
    ),
    (
        Rect::new(235, 0, 80, 32),
        "sexoff.frm",
        "sexon.frm",
        "sexmask.frm",
    ),
];
/// The name, age and sex buttons' press sound (no release sound).
const TOP_SOUND: &str = "ib3p1xx1";
/// §3.10 dialog windows (`charwin.frm` at screen y = 0).
const NAME_X: i32 = 17;
const AGE_X: i32 = 154;
const SEX_X: i32 = 235;
/// Age arrows (20 by 18), window-relative.
const AGE_LEFT: Rect = Rect::new(19, 13, 20, 18);
const AGE_RIGHT: Rect = Rect::new(105, 13, 20, 18);
/// Male (45 by 43) and female (45 by 42) buttons, window-relative.
const MALE: Rect = Rect::new(22, 2, 45, 43);
const FEMALE: Rect = Rect::new(71, 3, 45, 42);
/// §3.11 the options window and its five buttons (`editor.msg` 600-604).
const OPTIONS_ORIGIN: [i32; 2] = [238, 90];
fn options_button(i: usize) -> Rect {
    Rect::new(
        OPTIONS_ORIGIN[0] + 13,
        OPTIONS_ORIGIN[1] + 17 + 37 * i as i32,
        137,
        34,
    )
}
/// The Erase question's Yes/No box.
const ERASE_ORIGIN: [i32; 2] = [169, 126];
/// §3.9 the SPECIAL plus repeats from the 20th frame, §3.10 the age
/// arrows from the 15th.
const SPECIAL_IDLE_FRAMES: u32 = 20;
const AGE_IDLE_FRAMES: u32 = 15;

fn red_button(origin: [i32; 2]) -> Rect {
    Rect::new(origin[0], origin[1], 15, 16)
}
fn special_buttons(y: i32) -> [Rect; 2] {
    [Rect::new(149, y, 16, 12), Rect::new(149, y + 11, 17, 12)]
}
fn shifted(rect: Rect, origin: [i32; 2]) -> Rect {
    Rect::new(origin[0] + rect.x, origin[1] + rect.y, rect.w, rect.h)
}
/// A dialog window's Done button: (26, 44), or (28, 48) in the sex dialog.
fn dialog_done(x: i32, sex: bool) -> Rect {
    if sex {
        red_button([x + 28, 48])
    } else {
        red_button([x + 26, 44])
    }
}
/// A Yes/No or Done box's red buttons at `origin` in a box of `size`, with
/// Yes first.
fn box_buttons(origin: [i32; 2], yes_no: bool, size: DialogSize) -> Vec<Rect> {
    flow::dialog_buttons_for(yes_no, size)
        .into_iter()
        .map(|button| red_button([origin[0] + button.button[0], origin[1] + button.button[1]]))
        .collect()
}
fn letter(key: KeyCode) -> Option<char> {
    let name = format!("{key:?}");
    name.strip_prefix("Key")
        .and_then(|rest| rest.chars().next())
        .map(|c| c.to_ascii_lowercase())
}

impl EditorStage {
    pub fn new(editor: Editor) -> Self {
        Self {
            editor,
            dialog: None,
            refusal: None,
            repeat: None,
            pressed: None,
            sounds: Vec::new(),
            files: None,
            file: None,
            notice: None,
            now: 0,
        }
    }
    /// The sheet files in the master patch root `root`, with `vfs` for the
    /// archive.
    pub fn with_files(mut self, files: Option<SheetFiles>) -> Self {
        self.files = files;
        self
    }
    /// The open file dialog's and notice's sounds join the stage's.
    fn take_sounds(&mut self) {
        if let Some((_, dialog)) = &mut self.file {
            self.sounds.append(&mut dialog.sounds);
        }
        if let Some((notice, _)) = &mut self.notice {
            self.sounds.append(&mut notice.sounds);
        }
    }
    /// §3.43: SAVE, LOAD or PRINT TO FILE opens its file dialog over the
    /// options dialog.
    fn open_file(&mut self, op: SheetOp) {
        let Some(files) = &self.files else {
            return;
        };
        let (kind, dialog, title, default) = match op {
            SheetOp::Save => (SheetFile::Sheet, DialogKind::Save, 600, Some(617)),
            SheetOp::Load => (SheetFile::Sheet, DialogKind::Load, 601, None),
            SheetOp::Print => (SheetFile::Print, DialogKind::Save, 602, Some(616)),
        };
        let names = files.list(kind);
        let default = default.map(|id| files.text(id));
        let title = files.text(title);
        self.file = Some((
            op,
            FileDialog::new(dialog, title, names, default.as_deref(), self.now),
        ));
    }
    /// A file dialog's outcome: a chosen name runs the operation.
    fn file_event(&mut self, event: FileEvent) {
        self.take_sounds();
        let op = match (&self.file, event) {
            (_, FileEvent::None) => return,
            (_, FileEvent::Cancel) => {
                self.file = None;
                return;
            }
            (Some((op, _)), FileEvent::Chosen(name)) => {
                let op = *op;
                self.file = None;
                (op, name)
            }
            (None, FileEvent::Chosen(_)) => return,
        };
        self.run_file(op.0, op.1);
    }
    /// §3.43 the operations. The load dialog returns the listed name; the
    /// save dialog the field, which gets its extension. SAVE asks before
    /// overwriting an existing file, and PRINT TO FILE asks only then too
    /// (the §3.11 `[May-improve]` safe rule; the reference asks when the
    /// file does not exist).
    fn run_file(&mut self, op: SheetOp, name: Vec<u8>) {
        let Some(files) = &self.files else {
            return;
        };
        if op == SheetOp::Load {
            return self.load_sheet(&name);
        }
        let kind = if op == SheetOp::Save {
            SheetFile::Sheet
        } else {
            SheetFile::Print
        };
        let file = sheet::with_extension(&name, kind);
        if files.exists(&file) {
            let mut title = file.to_ascii_uppercase();
            title.push(b' ');
            title.extend(files.text(609));
            let notice = Notice::new(
                sheet::MESSAGE_ORIGIN,
                vec![title, files.text(610)],
                ColorKey::ERROR,
                None,
                true,
            );
            self.notice = Some((notice, AfterNotice::Overwrite(op, file)));
            return;
        }
        self.write_sheet(op, &file);
    }
    /// SAVE writes the 428-byte sheet; PRINT TO FILE the printed sheet. A
    /// failure plays `iisxxxx1` before its box. SAVE's failure shows
    /// "Error saving NAME!" (the §3.11 `[May-improve]` safe rule for the
    /// reference's "NAMEError saving !") and keeps the options dialog.
    fn write_sheet(&mut self, op: SheetOp, file: &[u8]) {
        let Some(files) = &self.files else {
            return;
        };
        let bytes = if op == SheetOp::Save {
            sheet::sheet_gcd(&self.editor.sheet).write()
        } else {
            let context = self.editor.context();
            let shown = printed_sheet(
                &self.editor.sheet,
                context,
                |file, id| files.text_in(file, id),
                PrintWorld {
                    ticks: flow::START_TICKS,
                    ..PrintWorld::default()
                },
            );
            sheet::print_sheet(&shown, |id| files.text(id), true)
        };
        let upper = file.to_ascii_uppercase();
        let (notice, after) = match files.write(file, &bytes) {
            Ok(()) => {
                let mut title = upper;
                title.extend(files.text(607));
                (
                    Notice::new(
                        sheet::MESSAGE_ORIGIN,
                        vec![title],
                        ColorKey::GREEN,
                        (op == SheetOp::Save).then_some(DialogSize::Large),
                        false,
                    ),
                    AfterNotice::CloseOptions,
                )
            }
            Err(error) => {
                tracing::warn!(%error, "character sheet not written");
                self.sounds.push(ERROR_SOUND);
                let mut title = files.text(611);
                title.extend(upper);
                title.push(b'!');
                let after = if op == SheetOp::Save {
                    AfterNotice::Options
                } else {
                    AfterNotice::CloseOptions
                };
                (
                    Notice::new(
                        sheet::MESSAGE_ORIGIN,
                        vec![title],
                        ColorKey::ERROR,
                        Some(DialogSize::Large),
                        false,
                    ),
                    after,
                )
            }
        };
        self.notice = Some((notice, after));
    }
    /// §3.11 LOAD: the read sheet replaces the player and the options
    /// dialog closes, with no message; a failure keeps the player and shows
    /// `iisxxxx1` with "Error loading NAME!" (612, the name as listed).
    fn load_sheet(&mut self, name: &[u8]) {
        let Some(files) = &self.files else {
            return;
        };
        let loaded = files
            .read(name)
            .and_then(|bytes| vault_formats::GcdFile::parse(&bytes).map_err(|e| e.to_string()));
        match loaded {
            Ok(gcd) => {
                let context = self.editor.context();
                self.editor.load(CharacterSheet::from_gcd(&gcd, context));
                self.dialog = None;
            }
            Err(error) => {
                tracing::warn!(%error, "character sheet not loaded");
                self.sounds.push(ERROR_SOUND);
                let mut title = files.text(612);
                title.extend_from_slice(name);
                title.push(b'!');
                let notice = Notice::new(
                    sheet::MESSAGE_ORIGIN,
                    vec![title],
                    ColorKey::ERROR,
                    None,
                    false,
                );
                self.notice = Some((notice, AfterNotice::Options));
            }
        }
    }
    /// A notice's answer.
    fn answer_notice(&mut self, yes: bool) {
        self.take_sounds();
        let Some((_, after)) = self.notice.take() else {
            return;
        };
        match after {
            AfterNotice::Options => {}
            AfterNotice::CloseOptions => self.dialog = None,
            AfterNotice::Overwrite(op, file) if yes => self.write_sheet(op, &file),
            AfterNotice::Overwrite(..) => {}
        }
    }
    /// A refusal: `iisxxxx1` and the `editor.msg` box (G-30).
    fn refuse(&mut self, refusal: editor::Refusal) {
        self.sounds.push(ERROR_SOUND);
        self.refusal = Some(refusal);
    }
    fn done(&mut self) -> EditorExit {
        match self.editor.done() {
            Ok(sheet) => EditorExit::Done(Box::new(sheet)),
            Err(refusal) => {
                self.refuse(refusal);
                EditorExit::Stay(true)
            }
        }
    }
    fn open_age(&mut self) {
        self.dialog = Some(EditorDialog::Age {
            entry: self.editor.sheet.base[33],
        });
    }
    fn step(&mut self, step: Step) -> bool {
        match step {
            Step::Special { stat, up } => {
                if up {
                    self.editor.increment(stat)
                } else {
                    self.editor.decrement(stat)
                }
            }
            Step::Age { up } => {
                let age = self.editor.sheet.base[33];
                let next = editor::age_step(age, up);
                self.editor.sheet.base[33] = next;
                next != age
            }
        }
    }
    /// A key. §3.9: Done is Return, `d` or `D` (with `ib1p1xx1`), Cancel is
    /// Esc; `n`, `a`, `s` and `o` open the name, age, sex and options
    /// dialogs (G-31: `a` and `s` no longer change the sheet directly).
    pub fn key(&mut self, key: KeyCode, shift: bool) -> EditorExit {
        if let Some((notice, _)) = &mut self.notice {
            if let Some(yes) = notice.key(key) {
                self.answer_notice(yes);
            }
            self.take_sounds();
            return EditorExit::Stay(true);
        }
        if let Some((_, dialog)) = &mut self.file {
            let event = dialog.key(key, shift);
            self.file_event(event);
            return EditorExit::Stay(true);
        }
        if self.refusal.is_some() {
            if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape) {
                if key != KeyCode::Escape {
                    self.sounds.push(BUTTON_PRESS);
                }
                self.refusal = None;
            }
            return EditorExit::Stay(true);
        }
        match &mut self.dialog {
            Some(EditorDialog::Name(field)) => {
                match key {
                    KeyCode::Enter | KeyCode::NumpadEnter => {
                        self.sounds.push(BUTTON_PRESS);
                        if let Some(name) = field.accept() {
                            self.editor.sheet.name = name;
                        }
                        self.dialog = None;
                    }
                    KeyCode::Escape => self.dialog = None,
                    KeyCode::Backspace | KeyCode::Delete => field.backspace(),
                    KeyCode::Space => field.key(b' '),
                    _ => {
                        let name = format!("{key:?}");
                        let Some(c) = name
                            .strip_prefix("Key")
                            .or_else(|| name.strip_prefix("Digit"))
                            .and_then(|rest| rest.bytes().next())
                        else {
                            return EditorExit::Stay(false);
                        };
                        field.key(if shift { c } else { c.to_ascii_lowercase() });
                    }
                }
                return EditorExit::Stay(true);
            }
            // §3.10 age: `+`, Right and uppercase `N` step up; `-`, Left and
            // uppercase `J` down; Return keeps, Esc restores the entry age.
            Some(EditorDialog::Age { entry }) => {
                let entry = *entry;
                match key {
                    KeyCode::Enter | KeyCode::NumpadEnter => self.dialog = None,
                    KeyCode::Escape => {
                        self.editor.sheet.base[33] = entry;
                        self.dialog = None;
                    }
                    KeyCode::ArrowRight | KeyCode::NumpadAdd => {
                        self.step(Step::Age { up: true });
                    }
                    KeyCode::Equal if shift => {
                        self.step(Step::Age { up: true });
                    }
                    KeyCode::KeyN if shift => {
                        self.step(Step::Age { up: true });
                    }
                    KeyCode::ArrowLeft | KeyCode::Minus | KeyCode::NumpadSubtract => {
                        self.step(Step::Age { up: false });
                    }
                    KeyCode::KeyJ if shift => {
                        self.step(Step::Age { up: false });
                    }
                    _ => return EditorExit::Stay(false),
                }
                return EditorExit::Stay(true);
            }
            // §3.10 sex: Return, Done and Esc all just close.
            Some(EditorDialog::Sex) => {
                if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape) {
                    self.dialog = None;
                    return EditorExit::Stay(true);
                }
                return EditorExit::Stay(false);
            }
            Some(EditorDialog::Options { erase: true }) => {
                match key {
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyY => {
                        if key != KeyCode::KeyY {
                            self.sounds.push(BUTTON_PRESS);
                        }
                        self.answer_erase(true);
                    }
                    KeyCode::Escape | KeyCode::KeyN => self.answer_erase(false),
                    _ => {}
                }
                return EditorExit::Stay(true);
            }
            // §3.11 options: `e` erases (after its question), `d`, Return
            // (with `ib1p1xx1`) and Esc close; `s`, `l` and `p` open the
            // §3.43 file dialogs.
            Some(EditorDialog::Options { erase: false }) => {
                match key {
                    KeyCode::Escape => self.dialog = None,
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD => {
                        self.sounds.push(BUTTON_PRESS);
                        self.dialog = None;
                    }
                    KeyCode::KeyE => self.dialog = Some(EditorDialog::Options { erase: true }),
                    KeyCode::KeyS => self.open_file(SheetOp::Save),
                    KeyCode::KeyL => self.open_file(SheetOp::Load),
                    KeyCode::KeyP => self.open_file(SheetOp::Print),
                    _ => {}
                }
                return EditorExit::Stay(true);
            }
            None => {}
        }
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD => {
                self.sounds.push(BUTTON_PRESS);
                self.done()
            }
            KeyCode::Escape => EditorExit::Cancel,
            _ => {
                match letter(key) {
                    Some('n') => {
                        self.dialog =
                            Some(EditorDialog::Name(NameField::open(&self.editor.sheet.name)))
                    }
                    Some('a') => self.open_age(),
                    Some('s') => self.dialog = Some(EditorDialog::Sex),
                    Some('o') => self.dialog = Some(EditorDialog::Options { erase: false }),
                    _ => return EditorExit::Stay(false),
                }
                EditorExit::Stay(true)
            }
        }
    }
    /// The Erase question: Yes resets the player to blank defaults; the
    /// options dialog stays open either way.
    fn answer_erase(&mut self, yes: bool) {
        if yes {
            self.editor.erase();
        }
        self.dialog = Some(EditorDialog::Options { erase: false });
    }
    /// A left press at `point` (`now` in milliseconds). Step buttons act on
    /// the press and repeat while held; other buttons press here and act on
    /// their release. Returns the pressed button's rectangle, for its down
    /// art.
    pub fn press(&mut self, painter: &Painter, point: [i32; 2], now: u64) -> Option<Rect> {
        self.now = now;
        self.pressed = None;
        self.repeat = None;
        if let Some((notice, _)) = &mut self.notice {
            let held = notice.press(point);
            self.take_sounds();
            return held;
        }
        if let Some((_, dialog)) = &mut self.file {
            let held = dialog.press(point, now);
            self.take_sounds();
            return held;
        }
        if let Some(refusal) = self.refusal {
            let lines = [
                painter.message("editor.msg", refusal.title),
                painter.message("editor.msg", refusal.body),
            ];
            let size = box_size(painter, &lines, false, None);
            let done = box_buttons(editor::DIALOG_ORIGIN, false, size)[0];
            return self.press_red(done, point, Press::RefusalDone);
        }
        match &self.dialog {
            Some(EditorDialog::Name(_)) => {
                return self.press_red(dialog_done(NAME_X, false), point, Press::DialogDone);
            }
            Some(EditorDialog::Age { .. }) => {
                for (rect, up) in [(AGE_LEFT, false), (AGE_RIGHT, true)] {
                    let rect = shifted(rect, [AGE_X, 0]);
                    if rect.contains(point) {
                        // §3.10 the arrows step on the press with `ib2p1xx1`.
                        self.sounds.push("ib2p1xx1");
                        let step = Step::Age { up };
                        if self.step(step) {
                            self.repeat = Some((step, rect, HoldRepeat::new(now, AGE_IDLE_FRAMES)));
                        }
                        return Some(rect);
                    }
                }
                return self.press_red(dialog_done(AGE_X, false), point, Press::DialogDone);
            }
            Some(EditorDialog::Sex) => {
                for (rect, female) in [(MALE, 0), (FEMALE, 1)] {
                    if shifted(rect, [SEX_X, 0]).contains(point) {
                        // The press applies the gender at once. Safe rule
                        // (§3.10 `[May-improve]`): the gender buttons play
                        // `ib1p1xx1` and Done its red-button pair.
                        self.sounds.push(BUTTON_PRESS);
                        self.editor.sheet.base[34] = female;
                        return None;
                    }
                }
                return self.press_red(dialog_done(SEX_X, true), point, Press::DialogDone);
            }
            Some(EditorDialog::Options { erase: true }) => {
                let buttons = box_buttons(ERASE_ORIGIN, true, DialogSize::Large);
                if let Some(rect) = self.press_red(buttons[0], point, Press::EraseYes) {
                    return Some(rect);
                }
                return self.press_red(buttons[1], point, Press::EraseNo);
            }
            Some(EditorDialog::Options { erase: false }) => {
                let index = (0..5).find(|i| options_button(*i).contains(point))?;
                self.sounds.push("ib3p1xx1");
                self.pressed = Some(Press::OptionsButton(index));
                return Some(options_button(index));
            }
            None => {}
        }
        for (rect, press) in [
            (EDITOR_DONE, Press::Done),
            (EDITOR_CANCEL, Press::Cancel),
            (EDITOR_OPTIONS, Press::Options),
        ] {
            if let Some(rect) = self.press_red(rect, point, press) {
                return Some(rect);
            }
        }
        for ((rect, _, _, mask), press) in
            TOP_BUTTONS
                .iter()
                .zip([Press::Name, Press::Age, Press::Sex])
        {
            if masked(painter, mask, *rect, point) {
                self.sounds.push(TOP_SOUND);
                self.pressed = Some(press);
                return Some(*rect);
            }
        }
        for (stat, y) in editor::SPECIAL_Y.iter().enumerate() {
            for (rect, up) in special_buttons(*y).into_iter().zip([true, false]) {
                if rect.contains(point) {
                    // §3.9 a press steps once, then repeats from the 20th
                    // idle frame until release, leaving the button or a
                    // failed step.
                    self.sounds.push(BUTTON_PRESS);
                    let step = Step::Special { stat, up };
                    if self.step(step) {
                        self.repeat = Some((step, rect, HoldRepeat::new(now, SPECIAL_IDLE_FRAMES)));
                    }
                    return Some(rect);
                }
            }
        }
        let editor = &mut self.editor;
        for k in 0..18 {
            if Rect::new(347, 26 + 11 * k, 17, 11).contains(point) {
                self.sounds.push(BUTTON_PRESS);
                if let Err(refusal) = editor.toggle_tag(k) {
                    self.refuse(refusal);
                }
                return None;
            }
        }
        for k in 0..16 {
            let (x, row) = if k < 8 { (23, k) } else { (298, k - 8) };
            if Rect::new(x, 352 + 13 * row, 17, 11).contains(point) {
                self.sounds.push(BUTTON_PRESS);
                if let Err(refusal) = editor.toggle_trait(k) {
                    self.refuse(refusal);
                }
                return None;
            }
        }
        if let Some(line) = editor::info_hit(point) {
            editor.selected = line;
        }
        None
    }
    fn press_red(&mut self, rect: Rect, point: [i32; 2], press: Press) -> Option<Rect> {
        if !rect.contains(point) {
            return None;
        }
        self.sounds.push(BUTTON_PRESS);
        self.pressed = Some(press);
        Some(rect)
    }
    /// A left release; `over` tells whether it is over the pressed button.
    /// The file dialog's list acts on the release position `point`.
    pub fn release(&mut self, over: bool, point: Option<[i32; 2]>) -> EditorExit {
        if let Some((notice, _)) = &mut self.notice {
            if let Some(yes) = notice.release(over) {
                self.answer_notice(yes);
            }
            self.take_sounds();
            return EditorExit::Stay(true);
        }
        if let Some((_, dialog)) = &mut self.file {
            let event = dialog.release(point);
            self.file_event(event);
            return EditorExit::Stay(true);
        }
        self.repeat = None;
        let Some(press) = self.pressed.take() else {
            return EditorExit::Stay(false);
        };
        // Red buttons play their release sound; the name, age and sex
        // buttons and the options dialog's buttons have only a press sound
        // (§3.8, §3.11).
        if !matches!(
            press,
            Press::Name | Press::Age | Press::Sex | Press::OptionsButton(_)
        ) {
            self.sounds.push(BUTTON_RELEASE);
        }
        if !over {
            return EditorExit::Stay(true);
        }
        match press {
            Press::Done => return self.done(),
            Press::Cancel => return EditorExit::Cancel,
            Press::Options => self.dialog = Some(EditorDialog::Options { erase: false }),
            Press::Name => {
                self.dialog = Some(EditorDialog::Name(NameField::open(&self.editor.sheet.name)))
            }
            Press::Age => self.open_age(),
            Press::Sex => self.dialog = Some(EditorDialog::Sex),
            Press::DialogDone => {
                if let Some(EditorDialog::Name(field)) = &self.dialog
                    && let Some(name) = field.accept()
                {
                    self.editor.sheet.name = name;
                }
                self.dialog = None;
            }
            Press::OptionsButton(0) => self.open_file(SheetOp::Save),
            Press::OptionsButton(1) => self.open_file(SheetOp::Load),
            Press::OptionsButton(2) => self.open_file(SheetOp::Print),
            Press::OptionsButton(3) => self.dialog = Some(EditorDialog::Options { erase: true }),
            Press::OptionsButton(_) => self.dialog = None,
            Press::EraseYes => self.answer_erase(true),
            Press::EraseNo => self.answer_erase(false),
            Press::RefusalDone => self.refusal = None,
        }
        EditorExit::Stay(true)
    }
    /// One frame: a held step button repeats while the pointer stays on it;
    /// leaving it or a failed step ends the repeat. Returns whether the
    /// sheet changed.
    pub fn tick(&mut self, now: u64, pointer: Option<[i32; 2]>) -> bool {
        self.now = now;
        if let Some((_, dialog)) = &mut self.file {
            let changed = dialog.tick(now, pointer);
            self.take_sounds();
            return changed;
        }
        let Some((step, rect, repeat)) = &mut self.repeat else {
            return false;
        };
        if !pointer.is_some_and(|point| rect.contains(point)) {
            self.repeat = None;
            return false;
        }
        if !repeat.poll(now) {
            return false;
        }
        let step = *step;
        if !self.step(step) {
            self.repeat = None;
        }
        true
    }
}

/// A button whose clickable pixels are the nonzero pixels of its mask art;
/// without the art, its rectangle.
pub(crate) fn masked(painter: &Painter, mask: &str, rect: Rect, point: [i32; 2]) -> bool {
    if !rect.contains(point) {
        return false;
    }
    let Some(art) = painter.art(mask) else {
        return true;
    };
    let Some(frame) = art.frame(0, 0) else {
        return true;
    };
    let (x, y) = (point[0] - rect.x, point[1] - rect.y);
    let (w, h) = (i32::from(frame.width), i32::from(frame.height));
    (0..w).contains(&x) && (0..h).contains(&y) && frame.pixels()[(y * w + x) as usize] != 0
}

/// SPEC-014 §3.8 skill and trait rows: untagged 992 (selected 32747),
/// tagged or picked 21140 (selected 32767).
pub(crate) fn row_key(marked: bool, selected: bool) -> ColorKey {
    match (marked, selected) {
        (false, false) => ColorKey::GREEN,
        (false, true) => ColorKey::SELECTED,
        (true, false) => ColorKey::GREY,
        (true, true) => ColorKey::WHITE,
    }
}

/// §3.13 a box's size: a Yes/No box is always large; otherwise the
/// requested size, else large when the widest line exceeds 168 pixels or
/// there are more than five lines, else medium.
pub(crate) fn box_size(
    painter: &Painter,
    lines: &[Vec<u8>],
    yes_no: bool,
    requested: Option<DialogSize>,
) -> DialogSize {
    let widest = lines
        .iter()
        .map(|line| painter.width(101, line))
        .max()
        .unwrap_or(0);
    flow::dialog_size(yes_no, requested, widest, lines.len())
}

/// A §3.13 box at `origin` with centered lines in `key` and its buttons,
/// in the `requested` size or by the size rule.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_box(
    painter: &Painter,
    image: &mut IndexedImage,
    origin: [i32; 2],
    lines: &[Vec<u8>],
    key: ColorKey,
    yes_no: bool,
    requested: Option<DialogSize>,
    held: Option<Rect>,
) {
    let [ox, oy] = origin;
    let size = box_size(painter, lines, yes_no, requested);
    let (art, [width, _]) = size.art();
    painter.blit_opaque(image, art, 0, origin);
    let mut y = oy + size.title_y(lines.len().saturating_sub(1));
    for line in lines {
        painter.centered(image, 101, key, line, [ox, y], width);
        y += 10;
    }
    // The Done box centered for the size: (97, 98) large, (53, 81) medium.
    for button in flow::dialog_buttons_for(yes_no, size) {
        let [bx, by] = button.box_origin;
        if button.transparent_box {
            painter.blit(image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
        } else {
            painter.blit_opaque(image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
        }
        let caption = painter.message("dbox.msg", button.caption);
        let [cx, cy] = button.caption_origin;
        painter.text(
            image,
            103,
            ColorKey::LABEL,
            &caption,
            [ox + cx, oy + cy],
            60,
        );
        let rect = red_button([ox + button.button[0], oy + button.button[1]]);
        let art = if held == Some(rect) {
            "lilreddn.frm"
        } else {
            "lilredup.frm"
        };
        painter.blit(image, art, 0, [rect.x, rect.y]);
    }
}

/// A §3.10 dialog window with its done box, DONE caption and red button.
fn draw_dialog_window(
    painter: &Painter,
    image: &mut IndexedImage,
    x: i32,
    sex: bool,
    held: Option<Rect>,
) {
    painter.blit_opaque(image, "charwin.frm", 0, [x, 0]);
    let (done_box, caption) = if sex {
        ([x + 15, 44], [x + 52, 48])
    } else {
        ([x + 13, 40], [x + 50, 44])
    };
    painter.blit_opaque(image, "donebox.frm", 0, done_box);
    let text = painter.message("editor.msg", 100);
    painter.text(image, 103, ColorKey::LABEL, &text, caption, 60);
    let rect = dialog_done(x, sex);
    let art = if held == Some(rect) {
        "lilreddn.frm"
    } else {
        "lilredup.frm"
    };
    painter.blit(image, art, 0, [rect.x, rect.y]);
}

/// SPEC-014 §3.8: what the editor window's shared parts show. The
/// character screen in game draws the same SPECIAL column, condition and
/// derived lists, skill list and info-card title as creation mode.
pub(crate) struct SheetView<'a> {
    pub sheet: &'a CharacterSheet,
    pub context: StatContext,
    /// The shown SPECIAL values (creation: base + trait adjustment + bonus).
    pub special: [i32; 7],
    /// The hit points before "/max" on the first condition line.
    pub hit_points: i32,
    /// The active condition lines (`editor::CONDITIONS` order).
    pub conditions: [bool; 8],
    pub selected: InfoLine,
    /// §3.42 in game: the SPECIAL numbers are always white and the words
    /// come from `stat.msg` 300 + E.
    pub game: bool,
}

/// §3.8 the name, age and sex buttons with their captions (font 103, key
/// 18979 up, 14723 down, on row 6).
pub(crate) fn draw_top_buttons(
    painter: &Painter,
    image: &mut IndexedImage,
    sheet: &CharacterSheet,
    context: StatContext,
    held: Option<Rect>,
) {
    let captions = {
        let name = editor::caption_name(&sheet.name, |text| painter.width(103, text));
        let mut age = painter.message("editor.msg", 104);
        age.extend(format!(" {}", sheet.stat(33, context)).bytes());
        let sex = painter.message("editor.msg", 107 + sheet.stat(34, context));
        [(name, 75), (age, 41), (sex, 40)]
    };
    for ((rect, off, on, _), (caption, center)) in TOP_BUTTONS.iter().zip(captions) {
        let down = held == Some(*rect);
        painter.blit(image, if down { on } else { off }, 0, [rect.x, rect.y]);
        let key = if down {
            ColorKey::LABEL_PRESSED
        } else {
            ColorKey::LABEL
        };
        painter.text(
            image,
            103,
            key,
            &caption,
            [rect.x + center - painter.width(103, &caption) / 2, 6],
            rect.w,
        );
    }
}

/// §3.8 the SPECIAL column without its plus/minus buttons: two big-number
/// cells at (58, Y) (red set above 10) and the level word, `editor.msg`
/// 199 + value clamped to 1-10, in font 101 at (103, Y + 8). §3.42 in
/// game: the effective value in the white set and `stat.msg` 300 + E.
pub(crate) fn draw_special(painter: &Painter, image: &mut IndexedImage, view: &SheetView) {
    for (stat, y) in editor::SPECIAL_Y.iter().enumerate() {
        let shown = view.special[stat];
        let red = shown > 10 && !view.game;
        for (cell, x) in editor::big_number_cells(shown, shown).iter().zip([58, 72]) {
            painter.big_digit(image, *cell, [x, *y], red);
        }
        let word = if view.game {
            painter.message("stat.msg", 300 + shown.clamp(1, 10))
        } else {
            painter.message("editor.msg", 199 + shown.clamp(1, 10))
        };
        let key = if view.selected == InfoLine::Special(stat) {
            ColorKey::SELECTED
        } else {
            ColorKey::GREEN
        };
        painter.text(image, 101, key, &word, [103, y + 8], 50);
    }
}

/// §3.8 the condition lines at (194, 46 + 13k): "Hit Points cur/max", then
/// `editor.msg` 312-318, active key 992 (selected 32747), inactive 1313
/// (selected 15845); the derived list beneath at (194, 179 + 13k) with
/// values at x = 288.
pub(crate) fn draw_conditions(painter: &Painter, image: &mut IndexedImage, view: &SheetView) {
    let selected = |line: InfoLine, active: bool| match (active, view.selected == line) {
        (true, false) => ColorKey::GREEN,
        (true, true) => ColorKey::SELECTED,
        (false, false) => ColorKey::DARK_GREEN,
        (false, true) => ColorKey::DARK_OLIVE,
    };
    for (k, id) in editor::CONDITIONS.iter().enumerate() {
        let mut text = painter.message("editor.msg", *id);
        if k == 0 {
            let max = view.sheet.stat(7, view.context);
            text.extend(format!(" {}/{max}", view.hit_points).bytes());
        }
        let key = selected(InfoLine::Condition(k), view.conditions[k]);
        painter.text(image, 101, key, &text, [194, 46 + 13 * k as i32], 120);
    }
    for (k, (id, stat, percent)) in editor::DERIVED.iter().enumerate() {
        let key = selected(InfoLine::Derived(k), true);
        let y = 179 + 13 * k as i32;
        let label = painter.message("editor.msg", *id);
        painter.text(image, 101, key, &label, [194, y], 94);
        let value = view.sheet.stat(*stat, view.context);
        let value = if *percent {
            format!("{value}%")
        } else {
            value.to_string()
        };
        painter.text(image, 101, key, value.as_bytes(), [288, y], 30);
    }
}

/// §3.8 the skill list: `caption` (font 103) at (380, 5), then
/// `skill.msg` 100 + k at x = 380 and "NN%" at x = 573 in font 101, tagged
/// skills in the picked colors.
pub(crate) fn draw_skills(
    painter: &Painter,
    image: &mut IndexedImage,
    view: &SheetView,
    caption: &[u8],
) {
    painter.text(image, 103, ColorKey::LABEL, caption, [380, 5], 200);
    for k in 0..18 {
        let tagged = view.sheet.tags.contains(&(k as i32));
        let key = row_key(tagged, view.selected == InfoLine::Skill(k));
        let name = painter.message("skill.msg", 100 + k as i32);
        let y = editor::skill_y(k);
        painter.text(image, 101, key, &name, [380, y], 190);
        let value = format!("{}%", view.sheet.skill(k, view.context)).into_bytes();
        painter.text(image, 101, key, &value, [573, y], 50);
    }
}

/// A card's title text: a message, or a kill type's name and "Kills".
pub(crate) fn card_title(painter: &Painter, card: &Card) -> Vec<u8> {
    match card.title {
        Title::Message(file, id) => painter.message(file, id),
        Title::Kill(kind) => {
            let mut text = painter.message("proto.msg", 450 + kind as i32);
            text.push(b' ');
            text.extend(painter.message("editor.msg", 126));
            text
        }
    }
}

/// The `art/skilldex/` art of a card picture (list index), if it exists.
pub(crate) fn skilldex_art(painter: &Painter, index: i32) -> Option<String> {
    let path = painter.assets.skilldex_path(index).ok()?;
    painter.art(&path).is_some().then_some(path)
}

/// §3.8 c: the leftmost picture column holding a pixel whose largest 5-bit
/// channel is below 2 (transparent index 0 is not part of the picture).
/// Without such a column, the picture's width.
pub(crate) fn dark_column(painter: &Painter, path: &str) -> i32 {
    let Some(art) = painter.art(path) else {
        return 0;
    };
    let Some(frame) = art.frame(0, 0) else {
        return 0;
    };
    let (w, h) = (usize::from(frame.width), usize::from(frame.height));
    let pixels = frame.pixels();
    (0..w)
        .find(|x| {
            (0..h).any(|y| {
                let index = pixels[y * w + x];
                index != 0 && painter.palette.rgb6(index).iter().all(|c| c >> 1 < 2)
            })
        })
        .unwrap_or(w) as i32
}

/// Words of `text` wrapped to `limit` pixels in font 101.
pub(crate) fn wrap_words(painter: &Painter, text: &[u8], limit: i32) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for word in text.split(|b| *b == b' ').filter(|w| !w.is_empty()) {
        match lines.last_mut() {
            Some(line) if painter.width(101, &[line.as_slice(), b" ", word].concat()) <= limit => {
                line.push(b' ');
                line.extend_from_slice(word);
            }
            _ => lines.push(word.to_vec()),
        }
    }
    lines
}

/// §3.8 / §3.42 the info card: the `art/skilldex/` picture at (484, 309);
/// the title in font 102 at (348, 272), black (the specification's raw
/// index 228 is drawn as its black key), and for a skill "Base: NN%
/// formula" in font 101 at (348 + title width + 8, 286); the two black
/// rules at y = 300 and 301 from x = 348 to 613; the description in font
/// 101 from (348, 315) stepping 10, wrapped to `136 + max(0, min(150, c) -
/// 8)`. A card without a picture (the effect perks, past the list) keeps
/// its title and description (`[May-improve]` safe rule) and wraps at 136.
pub(crate) fn draw_card(painter: &Painter, image: &mut IndexedImage, card: &Card) {
    let picture = skilldex_art(painter, card.picture);
    let column = picture
        .as_deref()
        .map_or(0, |path| dark_column(painter, path));
    if let Some(path) = &picture {
        painter.blit(image, path, 0, [484, 309]);
    }
    let title = card_title(painter, card);
    let width = painter.text(image, 102, ColorKey::BLACK, &title, [348, 272], 266);
    if let Some(skill) = card.base {
        let mut base = painter.message("editor.msg", 137);
        base.extend(format!(" {}% ", character::SKILL_BASES[skill]).bytes());
        base.extend(painter.message("skill.msg", 300 + skill as i32));
        let x = 348 + width + 8;
        painter.text(image, 101, ColorKey::BLACK, &base, [x, 286], 614 - x);
    }
    image.fill_rect([348, 300, 266, 2], painter.color(ColorKey::BLACK));
    let (file, id) = card.description;
    let description = painter.message(file, id);
    let limit = character::card_wrap(column);
    for (k, line) in wrap_words(painter, &description, limit).iter().enumerate() {
        painter.text(
            image,
            101,
            ColorKey::BLACK,
            line,
            [348, 315 + 10 * k as i32],
            limit,
        );
    }
}

pub(crate) fn draw_editor(
    painter: &Painter,
    image: &mut IndexedImage,
    stage: &EditorStage,
    held: Option<Rect>,
) {
    let art = |rect: Rect, up, down| if held == Some(rect) { down } else { up };
    let editor = &stage.editor;
    let context = StatContext::default();
    painter.blit_opaque(image, editor::BACKGROUND, 0, [0, 0]);
    draw_top_buttons(painter, image, &editor.sheet, context, held);
    let max_hp = editor.sheet.stat(7, context);
    let view = SheetView {
        sheet: &editor.sheet,
        context,
        special: std::array::from_fn(|stat| editor.shown(stat)),
        // Creation mode shows "Hit Points max/max"; only that condition
        // line is active.
        hit_points: max_hp,
        conditions: [true, false, false, false, false, false, false, false],
        selected: editor.selected,
        game: false,
    };
    draw_special(painter, image, &view);
    for y in editor::SPECIAL_Y {
        let [plus, minus] = special_buttons(y);
        painter.blit(image, art(plus, "splsoff.frm", "splson.frm"), 0, [149, y]);
        painter.blit(
            image,
            art(minus, "snegoff.frm", "snegon.frm"),
            0,
            [149, y + 11],
        );
    }
    draw_conditions(painter, image, &view);
    draw_card(painter, image, &character::info_card(editor.selected, None));
    let points = painter.message("editor.msg", 116);
    painter.text(image, 103, ColorKey::LABEL, &points, [14, 286], 120);
    for (cell, x) in editor::big_number_cells(editor.character_points, editor.character_points)
        .iter()
        .zip([126, 140])
    {
        painter.big_digit(image, *cell, [x, 282], false);
    }
    let caption = painter.message("editor.msg", 139);
    painter.text(image, 103, ColorKey::LABEL, &caption, [52, 326], 160);
    for k in 0..16 {
        let (x, row) = if k < 8 { (47, k) } else { (199, k - 8) };
        let picked = editor.sheet.traits.contains(&k);
        let key = row_key(picked, editor.selected == InfoLine::Trait(k as usize));
        let text = painter.message("trait.msg", 100 + k);
        painter.text(
            image,
            101,
            key,
            &text,
            [x, editor::trait_label_y(row as usize)],
            120,
        );
    }
    let skills = painter.message("editor.msg", 117);
    draw_skills(painter, image, &view, &skills);
    for k in 0..18 {
        let tagged = editor.sheet.tags.contains(&k);
        painter.blit(
            image,
            if tagged {
                "tgsklon.frm"
            } else {
                "tgskloff.frm"
            },
            0,
            [347, 26 + 11 * k],
        );
    }
    let tags = painter.message("editor.msg", 138);
    painter.text(image, 103, ColorKey::LABEL, &tags, [422, 233], 100);
    for (cell, x) in editor::big_number_cells(editor.tag_picks, editor.tag_picks)
        .iter()
        .zip([522, 536])
    {
        painter.big_digit(image, *cell, [x, 228], false);
    }
    for ((button, caption), id) in editor::OPTIONS_BUTTONS
        .iter()
        .zip([[363, 454], [475, 454], [571, 454]])
        .zip([101, 100, 102])
    {
        let up = art(red_button(*button), "lilredup.frm", "lilreddn.frm");
        painter.blit(image, up, 0, *button);
        let text = painter.message("editor.msg", id);
        painter.text(image, 103, ColorKey::LABEL, &text, caption, 80);
    }
    match &stage.dialog {
        Some(EditorDialog::Name(field)) => {
            draw_dialog_window(painter, image, NAME_X, false, held);
            painter.blit_opaque(image, "namebox.frm", 0, [NAME_X + 13, 13]);
            painter.text(
                image,
                101,
                ColorKey::GREEN,
                &field.text,
                [NAME_X + 23, 19],
                100,
            );
        }
        Some(EditorDialog::Age { .. }) => {
            draw_dialog_window(painter, image, AGE_X, false, held);
            painter.blit_opaque(image, "agebox.frm", 0, [AGE_X + 8, 7]);
            let age = editor.sheet.base[33];
            for (cell, x) in editor::big_number_cells(age, age).iter().zip([55, 69]) {
                painter.big_digit(image, *cell, [AGE_X + x, 10], false);
            }
            for (rect, up, down) in [
                (AGE_LEFT, "slu.frm", "sld.frm"),
                (AGE_RIGHT, "sru.frm", "srd.frm"),
            ] {
                let rect = shifted(rect, [AGE_X, 0]);
                painter.blit(image, art(rect, up, down), 0, [rect.x, rect.y]);
            }
        }
        Some(EditorDialog::Sex) => {
            draw_dialog_window(painter, image, SEX_X, true, held);
            let female = editor.sheet.base[34] != 0;
            for (rect, off, on, chosen) in [
                (MALE, "maleoff.frm", "maleon.frm", !female),
                (FEMALE, "femoff.frm", "femon.frm", female),
            ] {
                let rect = shifted(rect, [SEX_X, 0]);
                painter.blit(image, if chosen { on } else { off }, 0, [rect.x, rect.y]);
            }
        }
        Some(EditorDialog::Options { erase }) => {
            painter.blit_opaque(image, "opbase.frm", 0, OPTIONS_ORIGIN);
            for i in 0..5 {
                let rect = options_button(i);
                let down = held == Some(rect);
                let (art, key) = if down {
                    ("opbtnon.frm", ColorKey::LABEL_PRESSED)
                } else {
                    ("opbtnoff.frm", ColorKey::LABEL)
                };
                painter.blit_opaque(image, art, 0, [rect.x, rect.y]);
                let caption = painter.message("editor.msg", 600 + i as i32);
                painter.centered(image, 103, key, &caption, [rect.x, rect.y + 7], rect.w);
            }
            if *erase {
                let lines = [
                    painter.message("editor.msg", 605),
                    painter.message("editor.msg", 606),
                ];
                draw_box(
                    painter,
                    image,
                    ERASE_ORIGIN,
                    &lines,
                    ColorKey::GREEN,
                    true,
                    None,
                    held,
                );
            }
        }
        None => {}
    }
    if let Some((_, dialog)) = &stage.file {
        dialog.draw(painter, image, held, stage.now);
    }
    if let Some((notice, _)) = &stage.notice {
        notice.draw(painter, image, held);
    }
    if let Some(refusal) = stage.refusal {
        let lines = [
            painter.message("editor.msg", refusal.title),
            painter.message("editor.msg", refusal.body),
        ];
        draw_box(
            painter,
            image,
            editor::DIALOG_ORIGIN,
            &lines,
            editor::DIALOG_COLOR,
            false,
            None,
            held,
        );
    }
}

/// §3.13 a message box or Yes/No question at `origin`: lines in one color
/// key, the box's red buttons, Enter (with `ib1p1xx1`) for OK or Yes, Esc
/// for No, `y` and `n` in a Yes/No box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notice {
    pub origin: [i32; 2],
    pub lines: Vec<Vec<u8>>,
    pub key: ColorKey,
    pub size: Option<DialogSize>,
    pub yes_no: bool,
    /// The pressed button's answer, until its release.
    pressed: Option<(Rect, bool)>,
    pub sounds: Vec<&'static str>,
    /// The size the last draw resolved (the size rule measures the text),
    /// so a press tests the buttons where they were drawn.
    shown: std::cell::Cell<Option<DialogSize>>,
}
impl Notice {
    pub fn new(
        origin: [i32; 2],
        lines: Vec<Vec<u8>>,
        key: ColorKey,
        size: Option<DialogSize>,
        yes_no: bool,
    ) -> Self {
        Self {
            origin,
            lines,
            key,
            size,
            yes_no,
            pressed: None,
            sounds: Vec::new(),
            shown: std::cell::Cell::new(None),
        }
    }
    /// The red buttons and their answers (Done and Yes are `true`), in the
    /// size the box was drawn (§3.13: a medium box's Done box sits at
    /// (53, 81)). A box not drawn yet counts as its requested size, else
    /// large.
    fn buttons(&self) -> Vec<(Rect, bool)> {
        let [ox, oy] = self.origin;
        let size = self.shown.get().or(self.size).unwrap_or(DialogSize::Large);
        flow::dialog_buttons_for(self.yes_no, size)
            .into_iter()
            .map(|button| {
                let rect = red_button([ox + button.button[0], oy + button.button[1]]);
                (rect, button.caption != 102)
            })
            .collect()
    }
    pub fn key(&mut self, key: KeyCode) -> Option<bool> {
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.sounds.push(BUTTON_PRESS);
                Some(true)
            }
            KeyCode::Escape => Some(false),
            KeyCode::KeyY if self.yes_no => Some(true),
            KeyCode::KeyN if self.yes_no => Some(false),
            _ => None,
        }
    }
    /// A press on a red button holds it (the box is modal).
    pub fn press(&mut self, point: [i32; 2]) -> Option<Rect> {
        let (rect, answer) = self
            .buttons()
            .into_iter()
            .find(|(rect, _)| rect.contains(point))?;
        self.sounds.push(BUTTON_PRESS);
        self.pressed = Some((rect, answer));
        Some(rect)
    }
    /// The release: the held button answers when the release is over it.
    pub fn release(&mut self, over: bool) -> Option<bool> {
        let (_, answer) = self.pressed.take()?;
        self.sounds.push(BUTTON_RELEASE);
        over.then_some(answer)
    }
    pub fn draw(&self, painter: &Painter, image: &mut IndexedImage, held: Option<Rect>) {
        self.shown
            .set(Some(box_size(painter, &self.lines, self.yes_no, self.size)));
        draw_box(
            painter,
            image,
            self.origin,
            &self.lines,
            self.key,
            self.yes_no,
            self.size,
            held,
        );
    }
}

/// What a file dialog input led to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileEvent {
    None,
    /// The load dialog's listed name, or the save dialog's name.
    Chosen(Vec<u8>),
    Cancel,
}

/// §3.43 the file dialogs at (168, 80), window-relative geometry.
const FILE_LIST: Rect = Rect::new(55, 49, 190, 124);
const FILE_UP: Rect = Rect::new(36, 44, 11, 14);
const FILE_DOWN: Rect = Rect::new(36, 58, 11, 14);
/// The arrows repeat with the perk chooser's timings, from the 15th frame.
pub(crate) const ARROW_IDLE_FRAMES: u32 = 15;
/// Raw palette index of the file list and name field background.
const FIELD_INDEX: u8 = 100;

/// A button of a file dialog, held until its release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilePress {
    Done,
    Cancel,
    List,
}

/// §3.43 the load or save file dialog: the title, the file list with its
/// arrows, Done and Cancel, and the save dialog's name field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDialog {
    pub title: Vec<u8>,
    pub names: Vec<Vec<u8>>,
    pub list: FileList,
    pub field: Option<sheet::NameField>,
    repeat: Option<(bool, Rect, HoldRepeat)>,
    pressed: Option<FilePress>,
    /// When it opened, for the idle frames and the field cursor.
    opened: u64,
    frames: u64,
    pub sounds: Vec<&'static str>,
}
impl FileDialog {
    /// `default` is the save dialog's initial name; the load dialog has no
    /// field.
    pub fn new(
        kind: DialogKind,
        title: Vec<u8>,
        names: Vec<Vec<u8>>,
        default: Option<&[u8]>,
        now: u64,
    ) -> Self {
        // The last file of a long list is never drawn with M = n - 13;
        // `[May-improve]` safe rule: M = n - 12 within the list.
        let list = FileList::new(kind, names.len(), true);
        Self {
            title,
            list,
            names,
            field: (kind == DialogKind::Save)
                .then(|| sheet::NameField::open(default.unwrap_or_default())),
            repeat: None,
            pressed: None,
            opened: now,
            frames: 0,
            sounds: Vec::new(),
        }
    }
    fn save(&self) -> bool {
        self.list.kind == DialogKind::Save
    }
    fn origin(&self) -> [i32; 2] {
        sheet::DIALOG_ORIGIN
    }
    fn at(&self, rect: Rect) -> Rect {
        shifted(rect, self.origin())
    }
    fn buttons(&self) -> [(Rect, FilePress); 2] {
        let y = if self.save() { 214 } else { 187 };
        [
            (self.at(red_button([58, y])), FilePress::Done),
            (self.at(red_button([163, y])), FilePress::Cancel),
        ]
    }
    /// Done or Return: the load dialog's selected name (Cancel with no
    /// files), the save dialog's field (Cancel when empty).
    fn accept(&self) -> FileEvent {
        let chosen = match &self.field {
            Some(field) => field.accept(),
            None => self
                .list
                .selected()
                .and_then(|entry| self.names.get(entry).cloned()),
        };
        chosen.map_or(FileEvent::Cancel, FileEvent::Chosen)
    }
    /// Keys: Up, Down, Home and End move; Esc cancels; in the save dialog
    /// Return accepts (with `ib1p1xx1`), Backspace and Delete erase, and
    /// other keys type. Return does nothing in the load dialog.
    pub fn key(&mut self, key: KeyCode, shift: bool) -> FileEvent {
        match key {
            KeyCode::ArrowUp => self.list.up(),
            KeyCode::ArrowDown => self.list.down_key(),
            KeyCode::Home => self.list.home(),
            KeyCode::End => self.list.end(),
            KeyCode::Escape => return FileEvent::Cancel,
            KeyCode::Enter | KeyCode::NumpadEnter if self.save() => {
                self.sounds.push(BUTTON_PRESS);
                return self.accept();
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(field) = &mut self.field {
                    field.backspace();
                }
            }
            _ => {
                if let (Some(field), Some(code)) = (&mut self.field, typed_char(key, shift)) {
                    field.key(code);
                }
            }
        }
        FileEvent::None
    }
    /// A press: an arrow steps at once (with `ib1p1xx1`, the
    /// `[May-improve]` safe rule for the reference's silent arrows) and
    /// repeats while held; Done and Cancel press; the list acts on release.
    pub fn press(&mut self, point: [i32; 2], now: u64) -> Option<Rect> {
        self.repeat = None;
        self.pressed = None;
        for (rect, up) in [(FILE_UP, true), (FILE_DOWN, false)] {
            let rect = self.at(rect);
            if rect.contains(point) {
                self.sounds.push(BUTTON_PRESS);
                self.step(up);
                self.repeat = Some((up, rect, HoldRepeat::new(now, ARROW_IDLE_FRAMES)));
                return Some(rect);
            }
        }
        for (rect, press) in self.buttons() {
            if rect.contains(point) {
                self.sounds.push(BUTTON_PRESS);
                self.pressed = Some(press);
                return Some(rect);
            }
        }
        if self.at(FILE_LIST).contains(point) {
            self.pressed = Some(FilePress::List);
        }
        None
    }
    fn step(&mut self, up: bool) {
        if up {
            self.list.up();
        } else {
            self.list.down_arrow();
        }
    }
    /// A release at `point`: a held red button acts over itself; a list
    /// click selects the row, and a second click on it chooses the file
    /// (with `ib1p1xx1`). In the save dialog a single click copies the
    /// file's base name into the field.
    pub fn release(&mut self, point: Option<[i32; 2]>) -> FileEvent {
        self.repeat = None;
        let Some(press) = self.pressed.take() else {
            return FileEvent::None;
        };
        let over = |rect: Rect| point.is_some_and(|point| rect.contains(point));
        match press {
            FilePress::Done | FilePress::Cancel => {
                self.sounds.push(BUTTON_RELEASE);
                let rect = self.buttons()[usize::from(press == FilePress::Cancel)].0;
                if !over(rect) {
                    return FileEvent::None;
                }
                if press == FilePress::Done {
                    self.accept()
                } else {
                    FileEvent::Cancel
                }
            }
            FilePress::List => {
                let Some(point) = point.filter(|point| self.at(FILE_LIST).contains(*point)) else {
                    return FileEvent::None;
                };
                let chosen = self.list.click(point[1]);
                let Some(name) = self.list.selected().and_then(|i| self.names.get(i)) else {
                    return FileEvent::None;
                };
                let name = name.clone();
                if chosen {
                    self.sounds.push(BUTTON_PRESS);
                    return FileEvent::Chosen(if self.save() {
                        sheet::base_name(&name)
                    } else {
                        name
                    });
                }
                if let Some(field) = &mut self.field {
                    field.take(&name);
                }
                FileEvent::None
            }
        }
    }
    /// One frame: a held arrow repeats while the pointer stays on it; the
    /// idle-frame counter runs; the field cursor blinks every 3 frames.
    /// Returns whether the dialog changed.
    pub fn tick(&mut self, now: u64, pointer: Option<[i32; 2]>) -> bool {
        let mut changed = false;
        if let Some((up, rect, repeat)) = &mut self.repeat {
            let up = *up;
            if !pointer.is_some_and(|point| rect.contains(point)) {
                self.repeat = None;
            } else if repeat.poll(now) {
                self.step(up);
                changed = true;
            }
        }
        let frames = now.saturating_sub(self.opened) / vault_core::ui::FRAME_MS;
        let elapsed = frames.saturating_sub(self.frames);
        for _ in 0..elapsed.min(sheet::IDLE_FRAMES) {
            self.list.idle_frame();
        }
        if self.field.is_some() && frames / 3 != self.frames / 3 {
            changed = true;
        }
        self.frames = frames;
        changed
    }
    pub fn draw(&self, painter: &Painter, image: &mut IndexedImage, held: Option<Rect>, now: u64) {
        let origin = self.origin();
        let art = if self.save() {
            "savebox.frm"
        } else {
            "loadbox.frm"
        };
        painter.blit_opaque(image, art, 0, origin);
        let at = |x: i32, y: i32| [origin[0] + x, origin[1] + y];
        painter.text(image, 103, ColorKey::LABEL, &self.title, at(49, 16), 200);
        let area = self.at(FILE_LIST);
        image.fill_rect([area.x, area.y, area.w, area.h], FIELD_INDEX);
        for (row, entry) in self.list.visible().enumerate() {
            let key = if row == self.list.row {
                ColorKey::SELECTED
            } else {
                ColorKey::GREEN
            };
            let y = 49 + 10 * row as i32;
            painter.text(image, 101, key, &self.names[entry], at(55, y), 190);
        }
        for (rect, up, down) in [
            (FILE_UP, "uparwoff.frm", "uparwon.frm"),
            (FILE_DOWN, "dnarwoff.frm", "dnarwon.frm"),
        ] {
            let rect = self.at(rect);
            let name = if held == Some(rect) { down } else { up };
            painter.blit(image, name, 0, [rect.x, rect.y]);
        }
        let (caption_y, done) = if self.save() { (213, 214) } else { (187, 187) };
        for (x, caption_x, id) in [(58, 79, 100), (163, 182, 103)] {
            let rect = self.at(red_button([x, done]));
            let name = if held == Some(rect) {
                "lilreddn.frm"
            } else {
                "lilredup.frm"
            };
            painter.blit(image, name, 0, [rect.x, rect.y]);
            let caption = painter.message("dbox.msg", id);
            painter.text(
                image,
                103,
                ColorKey::LABEL,
                &caption,
                at(caption_x, caption_y),
                80,
            );
        }
        // The field: key 992 text on raw index 100, drawn with one trailing
        // space; a 4-by-8 cursor block at the end of the space toggles every
        // 3 frames between key 992 and the background.
        if let Some(field) = &self.field {
            let mut text = field.text.clone();
            text.push(b' ');
            let width = painter.width(101, &text);
            let [x, y] = at(57, 190);
            let height = painter.line_height(101);
            image.fill_rect([x, y, width, height], FIELD_INDEX);
            painter.text(image, 101, ColorKey::GREEN, &text, [x, y], 200);
            let frames = now.saturating_sub(self.opened) / vault_core::ui::FRAME_MS;
            if (frames / 3).is_multiple_of(2) {
                image.fill_rect([x + width - 4, y, 4, 8], painter.color(ColorKey::GREEN));
            }
        }
    }
}

/// The byte a key types, US layout, with shift.
pub(crate) fn typed_char(key: KeyCode, shift: bool) -> Option<u8> {
    let name = format!("{key:?}");
    if let Some(letter) = name.strip_prefix("Key") {
        let c = letter.bytes().next()?;
        return Some(if shift { c } else { c.to_ascii_lowercase() });
    }
    if let Some(digit) = name.strip_prefix("Digit") {
        let d = digit.bytes().next()?;
        return Some(if shift {
            b")!@#$%^&*("[usize::from(d - b'0')]
        } else {
            d
        });
    }
    let (plain, shifted) = match key {
        KeyCode::Minus => (b'-', b'_'),
        KeyCode::Equal => (b'=', b'+'),
        KeyCode::BracketLeft => (b'[', b'{'),
        KeyCode::BracketRight => (b']', b'}'),
        KeyCode::Backquote => (b'`', b'~'),
        KeyCode::Quote => (b'\'', b'"'),
        KeyCode::Space => (b' ', b' '),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

/// §3.43 the sheet files: the master patch root (`fallout.cfg`
/// `master_patches`, where every file is written as a loose file) and the
/// archive, with `editor.msg` and the other game messages for the texts.
pub(crate) struct SheetFiles {
    pub root: PathBuf,
    pub assets: VfsWorldAssets,
}
impl SheetFiles {
    pub fn new(vfs: Arc<dyn Vfs>, language: &str, root: PathBuf) -> Option<Self> {
        let assets = VfsWorldAssets::new(vfs, language).ok()?;
        Some(Self { root, assets })
    }
    /// An `editor.msg` text; a missing message reads `Error` (SPEC-002).
    pub fn text(&self, id: i32) -> Vec<u8> {
        self.text_in("editor.msg", id)
    }
    pub fn text_in(&self, file: &str, id: i32) -> Vec<u8> {
        game_text(&self.assets, file, id)
    }
    pub fn list(&self, kind: SheetFile) -> Vec<Vec<u8>> {
        sheet_list(&*self.assets.vfs, &self.root, kind)
    }
    pub fn exists(&self, file: &[u8]) -> bool {
        file_exists(&*self.assets.vfs, &self.root, file)
    }
    pub fn read(&self, file: &[u8]) -> Result<Vec<u8>, String> {
        read_file(&*self.assets.vfs, &self.root, file)
    }
    pub fn write(&self, file: &[u8], bytes: &[u8]) -> Result<(), String> {
        write_file(&self.root, file, bytes)
    }
}

/// A game message, or `Error` when it is missing (SPEC-002).
pub(crate) fn game_text(assets: &VfsWorldAssets, file: &str, id: i32) -> Vec<u8> {
    assets
        .game_messages(file)
        .ok()
        .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
        .unwrap_or_else(|| b"Error".to_vec())
}

/// The loose file names in the master patch root.
fn loose_names(root: &Path) -> Vec<Vec<u8>> {
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().to_str().map(|n| n.as_bytes().to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

/// §3.43 the file list of a pattern: the archive root's entries (as the
/// archive lists them) and the loose files of the master patch root.
pub(crate) fn sheet_list(vfs: &dyn Vfs, root: &Path, kind: SheetFile) -> Vec<Vec<u8>> {
    let archive: Vec<&[u8]> = vfs
        .files()
        .map(|path| path.as_str().as_bytes())
        .filter(|name| !name.contains(&b'/'))
        .collect();
    let loose = loose_names(root);
    sheet::file_list(archive, loose.iter().map(Vec::as_slice), kind)
}

/// A plain file name: no separators, no traversal.
fn plain(file: &[u8]) -> Option<&str> {
    let name = std::str::from_utf8(file).ok()?;
    (!name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..").then_some(name)
}

/// The loose file of that name in the root, matched without regard to
/// case (the reference file system ignores case).
fn loose_file(root: &Path, file: &[u8]) -> Option<PathBuf> {
    let names = loose_names(root);
    let name = names.iter().find(|name| name.eq_ignore_ascii_case(file))?;
    Some(root.join(plain(name)?))
}

/// "Exists": the file opens for reading, as a loose file in the root or as
/// an archive member.
pub(crate) fn file_exists(vfs: &dyn Vfs, root: &Path, file: &[u8]) -> bool {
    loose_file(root, file).is_some() || plain(file).is_some_and(|name| vfs.read(name).is_ok())
}

pub(crate) fn read_file(vfs: &dyn Vfs, root: &Path, file: &[u8]) -> Result<Vec<u8>, String> {
    if let Some(path) = loose_file(root, file) {
        return std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()));
    }
    let name = plain(file).ok_or("invalid file name")?;
    vfs.read(name).map_err(|error| error.to_string())
}

/// Write a loose file in the root: an existing name matching without
/// regard to case is reused, a new one is upper-cased (§3.43 safe rule).
pub(crate) fn write_file(root: &Path, file: &[u8], bytes: &[u8]) -> Result<(), String> {
    let names = loose_names(root);
    let name = sheet::written_name(file, names.iter().map(Vec::as_slice));
    let name = plain(&name).ok_or("invalid file name")?;
    let path = root.join(name);
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// The world parts of a printed sheet: kills (kill-type order, nonzero),
/// inventory (stored order), total weight and game tick (the date follows
/// the fixed calendar, `host_save::game_date`).
#[derive(Debug, Clone, Default)]
pub(crate) struct PrintWorld {
    pub kills: Vec<(Vec<u8>, i32)>,
    pub inventory: Vec<(i32, Vec<u8>)>,
    pub weight: i64,
    pub ticks: u64,
}

/// §3.43 the printed sheet of `sheet` as currently shown, with `text` for
/// messages.
pub(crate) fn printed_sheet(
    sheet: &CharacterSheet,
    context: StatContext,
    text: impl Fn(&str, i32) -> Vec<u8>,
    world: PrintWorld,
) -> PrintedSheet {
    let stat = |index| sheet.stat(index, context);
    let [month, day, year] = vault_core::host_save::game_date(world.ticks);
    PrintedSheet {
        name: sheet.name.clone(),
        age: stat(33),
        gender: stat(34),
        level: sheet.level,
        experience: sheet.experience,
        special: std::array::from_fn(stat),
        hit_points: sheet.current_hp,
        max_hit_points: stat(7),
        armor_class: stat(9),
        healing_rate: stat(14),
        action_points: stat(8),
        critical_chance: stat(15),
        melee_damage: stat(11),
        carry_weight: stat(12),
        damage_resistance: stat(24),
        radiation_resistance: stat(31),
        poison_resistance: stat(32),
        sequence: stat(13),
        traits: sheet
            .traits
            .map(|t| (t >= 0).then(|| text("trait.msg", 100 + t))),
        perks: Perk::ALL
            .into_iter()
            .filter(|perk| sheet.rank(*perk) > 0)
            .map(|perk| {
                (
                    text("perk.msg", 101 + perk.index() as i32),
                    sheet.rank(perk),
                )
            })
            .collect(),
        skills: (0..18)
            .map(|skill| {
                (
                    text("skill.msg", 100 + skill as i32),
                    sheet.skill(skill, context),
                )
            })
            .collect(),
        kills: world.kills,
        inventory: world.inventory,
        weight: world.weight,
        day: i32::from(day),
        month: i32::from(month),
        year: i32::from(year),
        hhmm: sheet::hhmm(world.ticks),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> EditorStage {
        let context = StatContext::default();
        EditorStage::new(Editor::open(editor::blank_sheet(context), true, context))
    }

    #[test]
    fn age_and_sex_keys_open_their_dialogs_and_o_the_options() {
        let mut stage = stage();
        let age = stage.editor.sheet.base[33];
        stage.key(KeyCode::KeyA, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Age { entry: age }));
        // Uppercase N steps up; Esc restores the age on entry.
        stage.key(KeyCode::KeyN, true);
        assert_eq!(stage.editor.sheet.base[33], age + 1);
        stage.key(KeyCode::Escape, false);
        assert_eq!(
            (stage.dialog.clone(), stage.editor.sheet.base[33]),
            (None, age)
        );
        stage.key(KeyCode::KeyS, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Sex));
        assert_eq!(
            stage.editor.sheet.base[34], 0,
            "`s` no longer flips the sex"
        );
        stage.key(KeyCode::Escape, false);
        stage.key(KeyCode::KeyO, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Options { erase: false }));
        stage.key(KeyCode::KeyE, false);
        stage.key(KeyCode::KeyY, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Options { erase: false }));
    }

    /// A stage with sheet files over a synthetic `editor.msg` in a
    /// temporary master patch root.
    fn filed(dir: &Path) -> EditorStage {
        let game = dir.join("text/english/game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(
            game.join("editor.msg"),
            b"{600}{}{SAVE}\n{601}{}{LOAD}\n{602}{}{PRINT TO FILE}\n\
              {607}{}{ was successfully saved.}\n{609}{}{already exists.}\n\
              {610}{}{Do you wish to overwrite?}\n{611}{}{Error saving }\n\
              {612}{}{Error loading }\n{616}{}{PRINT}\n{617}{}{PLAYER}\n",
        )
        .unwrap();
        let vfs: Arc<dyn Vfs> = Arc::new(vault_formats::LocalDirProvider::mount(dir).unwrap());
        stage().with_files(SheetFiles::new(vfs, "english", dir.to_path_buf()))
    }
    fn notice(stage: &EditorStage) -> Vec<Vec<u8>> {
        stage
            .notice
            .as_ref()
            .map(|(n, _)| n.lines.clone())
            .unwrap_or_default()
    }

    #[test]
    fn options_save_load_and_print_sheet_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut stage = filed(dir.path());
        stage.editor.sheet.name = b"Alex".to_vec();
        stage.key(KeyCode::KeyO, false);
        stage.key(KeyCode::KeyS, false);
        let (op, dialog) = stage.file.as_ref().unwrap();
        assert_eq!(*op, SheetOp::Save);
        assert_eq!(dialog.field.as_ref().unwrap().text, b"PLAYER");
        assert_eq!(dialog.title, b"SAVE");
        stage.key(KeyCode::Enter, false);
        let bytes = std::fs::read(dir.path().join("PLAYER.GCD")).unwrap();
        assert_eq!(bytes.len(), 428);
        assert_eq!(
            notice(&stage),
            [b"PLAYER.GCD was successfully saved.".to_vec()]
        );
        stage.key(KeyCode::Enter, false);
        assert_eq!(stage.dialog, None, "success closes the options dialog");
        // Saving again asks; No returns to the options dialog.
        stage.key(KeyCode::KeyO, false);
        stage.key(KeyCode::KeyS, false);
        stage.key(KeyCode::Enter, false);
        assert_eq!(
            notice(&stage),
            [
                b"PLAYER.GCD already exists.".to_vec(),
                b"Do you wish to overwrite?".to_vec()
            ]
        );
        stage.key(KeyCode::KeyN, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Options { erase: false }));
        // LOAD lists the file; Return does nothing, Done loads it.
        stage.editor.sheet.name = b"Bob".to_vec();
        stage.key(KeyCode::KeyL, false);
        let (_, dialog) = stage.file.as_ref().unwrap();
        assert_eq!(dialog.names, [b"PLAYER.GCD".to_vec()]);
        stage.key(KeyCode::Enter, false);
        assert!(stage.file.is_some());
        let context = StatContext::default();
        let assets = VfsWorldAssets::new(
            Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
            "english",
        )
        .unwrap();
        let palette = vault_formats::Palette::parse(&vec![0; 0x8300]).unwrap();
        let painter = Painter {
            assets: &assets,
            palette: &palette,
        };
        let done = [168 + 60, 80 + 190];
        assert!(stage.press(&painter, done, 0).is_some());
        stage.release(true, Some(done));
        assert!(stage.file.is_none());
        assert_eq!(stage.editor.sheet.name, b"Alex");
        assert_eq!(stage.dialog, None);
        assert_eq!(
            stage.editor.sheet.stat(7, context),
            stage.editor.sheet.current_hp
        );
        // PRINT TO FILE writes the printed sheet in text mode.
        stage.key(KeyCode::KeyO, false);
        stage.key(KeyCode::KeyP, false);
        stage.key(KeyCode::Backspace, false);
        stage.key(KeyCode::KeyX, true);
        stage.key(KeyCode::Enter, false);
        let text = std::fs::read(dir.path().join("PRINX.TXT")).unwrap();
        assert!(text.starts_with(b"\r\n\r\n"));
        assert!(text.windows(5).any(|w| w == b" Alex"));
        assert_eq!(
            notice(&stage),
            [b"PRINX.TXT was successfully saved.".to_vec()]
        );
        stage.key(KeyCode::Escape, false);
        assert_eq!(stage.dialog, None);
        // A file that does not parse keeps the player and shows 612.
        std::fs::write(dir.path().join("BAD.GCD"), b"short").unwrap();
        stage.key(KeyCode::KeyO, false);
        stage.key(KeyCode::KeyL, false);
        stage.key(KeyCode::Home, false);
        let list = [168 + 60, 80 + 49 + 2];
        for _ in 0..2 {
            stage.press(&painter, list, 0);
            stage.release(false, Some(list));
        }
        assert_eq!(notice(&stage), [b"Error loading BAD.GCD!".to_vec()]);
        assert_eq!(stage.sounds.last(), Some(&ERROR_SOUND));
        stage.key(KeyCode::Enter, false);
        assert_eq!(stage.dialog, Some(EditorDialog::Options { erase: false }));
    }

    #[test]
    fn typed_characters_follow_the_us_layout() {
        assert_eq!(typed_char(KeyCode::KeyA, false), Some(b'a'));
        assert_eq!(typed_char(KeyCode::KeyA, true), Some(b'A'));
        assert_eq!(typed_char(KeyCode::Digit3, true), Some(b'#'));
        assert_eq!(typed_char(KeyCode::Minus, true), Some(b'_'));
        assert_eq!(typed_char(KeyCode::F1, false), None);
    }

    #[test]
    fn refused_tag_and_trait_picks_sound_and_show_the_box() {
        let mut stage = stage();
        stage.editor.tag_picks = 0;
        stage.editor.sheet.tags = [-1; 4];
        let result = stage.editor.toggle_tag(4);
        let refusal = result.unwrap_err();
        stage.refuse(refusal);
        assert_eq!(stage.refusal.map(|r| (r.title, r.body)), Some((140, 141)));
        assert_eq!(stage.sounds, vec![ERROR_SOUND]);
    }

    #[test]
    fn a_held_special_plus_repeats_from_the_twentieth_frame() {
        let mut stage = stage();
        let [plus, _] = special_buttons(editor::SPECIAL_Y[0]);
        let point = [plus.x + 1, plus.y + 1];
        stage.editor.character_points = 3;
        stage.editor.sheet.base[0] = 5;
        stage.editor.sheet.recompute(StatContext::default());
        // The press steps once at once.
        stage.step(Step::Special { stat: 0, up: true });
        stage.repeat = Some((
            Step::Special { stat: 0, up: true },
            plus,
            HoldRepeat::new(0, SPECIAL_IDLE_FRAMES),
        ));
        assert!(!stage.tick(20 * 41 - 1, Some(point)));
        assert!(stage.tick(20 * 41, Some(point)));
        assert_eq!(stage.editor.character_points, 1);
        // Leaving the button ends the repeat.
        assert!(!stage.tick(5000, Some([0, 0])));
        assert!(stage.repeat.is_none());
    }
}
