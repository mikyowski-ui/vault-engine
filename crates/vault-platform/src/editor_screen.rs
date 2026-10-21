//! SPEC-014 §3.8-3.11 the character editor in creation mode, as the game
//! flow (`game.rs`) runs it: the main window's buttons and keys, the name,
//! age and sex dialogs, the editor options dialog, the refusal box, and the
//! held-button repeat of the SPECIAL and age steps. The rules (points,
//! steps, picks, validation) are `vault_core::ui::editor`'s.
use crate::interface::Painter;
use vault_core::{
    CharacterSheet,
    stats::StatContext,
    ui::{
        BUTTON_PRESS, BUTTON_RELEASE, ColorKey, ERROR_SOUND, HoldRepeat, Rect,
        editor::{self, Editor, InfoLine, NameField},
        flow,
    },
};
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
/// A Yes/No or Done box's red buttons at `origin`, with Yes first.
fn box_buttons(origin: [i32; 2], yes_no: bool) -> Vec<Rect> {
    flow::dialog_buttons(yes_no)
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
            // (with `ib1p1xx1`) and Esc close. Save, Load and Print to file
            // need the file dialog, whose layout no approved specification
            // gives, so `s`, `l` and `p` do nothing here.
            Some(EditorDialog::Options { erase: false }) => {
                match key {
                    KeyCode::Escape => self.dialog = None,
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD => {
                        self.sounds.push(BUTTON_PRESS);
                        self.dialog = None;
                    }
                    KeyCode::KeyE => self.dialog = Some(EditorDialog::Options { erase: true }),
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
        self.pressed = None;
        self.repeat = None;
        if self.refusal.is_some() {
            let [ox, oy] = editor::DIALOG_ORIGIN;
            let done = box_buttons([ox, oy], false)[0];
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
                let buttons = box_buttons(ERASE_ORIGIN, true);
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
    pub fn release(&mut self, over: bool) -> EditorExit {
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
            // SAVE, LOAD and PRINT TO FILE need the unspecified file dialog.
            Press::OptionsButton(3) => self.dialog = Some(EditorDialog::Options { erase: true }),
            Press::OptionsButton(4) => self.dialog = None,
            Press::OptionsButton(_) => {}
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
fn masked(painter: &Painter, mask: &str, rect: Rect, point: [i32; 2]) -> bool {
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

/// A §3.13 box at `origin` with centered lines in `key` and its buttons.
fn draw_box(
    painter: &Painter,
    image: &mut IndexedImage,
    origin: [i32; 2],
    lines: &[Vec<u8>],
    key: ColorKey,
    yes_no: bool,
    held: Option<Rect>,
) {
    let [ox, oy] = origin;
    let widest = lines
        .iter()
        .map(|line| painter.width(101, line))
        .max()
        .unwrap_or(0);
    // §3.13: a Yes/No box is always large; otherwise large when the widest
    // line exceeds 168 pixels or there are more than five lines, else
    // medium, with the matching vertical layout. The Done box offsets the
    // specification gives are used for both sizes.
    let size = flow::dialog_size(yes_no, None, widest, lines.len());
    let (art, [width, _]) = size.art();
    painter.blit_opaque(image, art, 0, origin);
    let mut y = oy + size.title_y(lines.len().saturating_sub(1));
    for line in lines {
        painter.centered(image, 101, key, line, [ox, y], width);
        y += 10;
    }
    for button in flow::dialog_buttons(yes_no) {
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
/// 199 + value clamped to 1-10, in font 101 at (103, Y + 8).
pub(crate) fn draw_special(painter: &Painter, image: &mut IndexedImage, view: &SheetView) {
    for (stat, y) in editor::SPECIAL_Y.iter().enumerate() {
        let shown = view.special[stat];
        let red = shown > 10;
        for (cell, x) in editor::big_number_cells(shown, shown).iter().zip([58, 72]) {
            painter.big_digit(image, *cell, [x, *y], red);
        }
        let word = painter.message("editor.msg", 199 + shown.clamp(1, 10));
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

/// The info card's title message for a selected line.
pub(crate) fn card_title(selected: InfoLine) -> (&'static str, i32) {
    match selected {
        InfoLine::Special(i) => ("stat.msg", 100 + i as i32),
        InfoLine::Skill(k) => ("skill.msg", 100 + k as i32),
        InfoLine::Trait(k) => ("trait.msg", 100 + k as i32),
        InfoLine::Condition(k) => ("editor.msg", editor::CONDITIONS[k.min(7)]),
        InfoLine::Derived(k) => ("editor.msg", editor::DERIVED[k.min(9)].0),
        InfoLine::CharacterPoints => ("editor.msg", 116),
        InfoLine::OptionalTraits => ("editor.msg", 139),
        InfoLine::SkillsCard => ("editor.msg", 117),
        InfoLine::TagSkillsCard => ("editor.msg", 138),
    }
}

/// §3.8 the info card: the title in font 102, raw index 228, at (348, 272)
/// and the two black rules at y = 300 and 301 from x = 348 to 613. The
/// card's picture and description are not drawn: the specification does
/// not say which `art/skilldex/` picture or message belongs to each line.
pub(crate) fn draw_card(painter: &Painter, image: &mut IndexedImage, selected: InfoLine) {
    let (file, id) = card_title(selected);
    let title = painter.message(file, id);
    painter.text(image, 102, ColorKey::BLACK, &title, [348, 272], 266);
    image.fill_rect([348, 300, 266, 2], painter.color(ColorKey::BLACK));
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
    draw_card(painter, image, editor.selected);
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
                    held,
                );
            }
        }
        None => {}
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
            held,
        );
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
