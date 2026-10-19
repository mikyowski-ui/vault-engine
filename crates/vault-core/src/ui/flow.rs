//! SPEC-014 §3.1-3.7 and §3.12-3.13: startup, splash choice, palette fades,
//! the main menu and its loop, attract mode, the death scene, premade
//! selection, the new-game hand-off and the generic dialog box.
use super::{ColorKey, Rect};

/// §3.1 step 7: the splash names tried from the configured `splash` value.
/// Up to 10 names; after each miss s grows by 1 and wraps to 0 on reaching
/// 10. Returns the shown path and the value to store (`s + 1`, unwrapped).
pub fn choose_splash(start: i64, exists: impl Fn(&str) -> bool) -> Option<(String, i64)> {
    let mut s = start;
    for _ in 0..10 {
        let path = format!("art/splash/splash{s}.rix");
        if exists(&path) {
            return Some((path, s + 1));
        }
        s += 1;
        if s >= 10 {
            s = 0;
        }
    }
    None
}

/// §3.1 step 8 and every full reset: 07:21:00 on 5 December 2161.
pub const START_TICKS: u64 = 264_600;
/// §3.1 step 9: both start movies play on every start.
pub const START_MOVIES: [(&str, u32); 2] =
    [("art/cuts/iplogo.mve", 0x01), ("art/cuts/intro.mve", 0)];

/// §3.2 fade calibration: one timed 60-step fade sets the step count so a
/// fade lasts about 700 ms; a zero measurement counts as 46 ms.
pub fn fade_steps(elapsed_ms: u64) -> u32 {
    let elapsed = if elapsed_ms == 0 { 46 } else { elapsed_ms };
    (60.0 / (elapsed as f64 / 700.0)) as u32
}
/// Step `k` of `n` toward `new` from `old` six-bit levels.
pub fn fade_level(old: u8, new: u8, k: u32, n: u32) -> u8 {
    if n == 0 || k >= n {
        return new;
    }
    let (old, new) = (i64::from(old), i64::from(new));
    (old - (old - new) * i64::from(k) / i64::from(n)) as u8
}
/// The palette after step `k` of `n`; the target is installed exactly at
/// the end.
pub fn fade_palette(old: &[[u8; 3]; 256], new: &[[u8; 3]; 256], k: u32, n: u32) -> [[u8; 3]; 256] {
    std::array::from_fn(|i| std::array::from_fn(|c| fade_level(old[i][c], new[i][c], k, n)))
}
/// The two non-palette fade targets.
pub const BLACK: [[u8; 3]; 256] = [[0; 3]; 256];
pub const WHITE: [[u8; 3]; 256] = [[63; 3]; 256];

/// §3.3 main-menu buttons in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuButton {
    Intro,
    NewGame,
    LoadGame,
    Credits,
    Exit,
}
pub const MENU_BUTTONS: [MenuButton; 5] = [
    MenuButton::Intro,
    MenuButton::NewGame,
    MenuButton::LoadGame,
    MenuButton::Credits,
    MenuButton::Exit,
];
pub const MENU_BACKGROUND: &str = "mainmenu.frm";
pub const MENU_BUTTON_ART: (&str, &str) = ("menuup.frm", "menudown.frm");
pub const MENU_MUSIC: &str = "sound/music/07desert.acm";
pub const MENU_VERSION: &str = "FALLOUT 1.1";
pub const MENU_TEXT_COLOR: ColorKey = ColorKey::GREY_GREEN;
pub const MENU_LABEL_COLOR: ColorKey = ColorKey::AMBER;
/// Copyright (`misc.msg` 14) top-left, and the version's right edge.
pub const MENU_COPYRIGHT: [i32; 2] = [15, 460];
pub const MENU_VERSION_RIGHT: [i32; 2] = [615, 460];
/// The idle timeout, from loop entry, unaffected by input.
pub const MENU_TIMEOUT_MS: u64 = 120_000;
impl MenuButton {
    pub fn index(self) -> usize {
        MENU_BUTTONS
            .iter()
            .position(|b| *b == self)
            .expect("listed")
    }
    pub fn rect(self) -> Rect {
        Rect::new(425, 45 + 41 * self.index() as i32, 26, 26)
    }
    /// `misc.msg` label index.
    pub fn label(self) -> i32 {
        9 + self.index() as i32
    }
    /// Label top-left for a label `width` pixels wide (font 104).
    pub fn label_origin(self, width: i32) -> [i32; 2] {
        [520 - width / 2, 46 + 41 * self.index() as i32]
    }
    pub fn hotkey(self) -> char {
        ['i', 'n', 'l', 'c', 'e'][self.index()]
    }
}

/// What the main-menu loop returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    Intro,
    NewGame,
    LoadGame,
    Credits,
    /// Credits with a Shift key physically held.
    Quotes,
    Exit,
    /// 120 seconds idle, or `d`: attract mode.
    Timeout,
}
/// One input observed by a main-menu poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    None,
    Key(char),
    Escape,
    CtrlR,
    /// A mouse press on a button (not an auto-repeat).
    Press(MenuButton),
    Release(MenuButton),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuPoll {
    pub choice: Option<MenuChoice>,
    pub sounds: Vec<&'static str>,
    /// Brightness steps to apply (and save) immediately.
    pub brightness: i32,
    /// Ctrl+R: self-run recording (a developer feature this engine omits).
    pub record: bool,
}
/// §3.4 one poll of the menu loop, `elapsed_ms` after loop entry. Safe rule:
/// an explicit selection wins over a simultaneous timeout.
pub fn menu_poll(input: MenuInput, shift_held: bool, elapsed_ms: u64) -> MenuPoll {
    let mut poll = MenuPoll::default();
    let select = |button: MenuButton, poll: &mut MenuPoll| {
        poll.sounds.push("nmselec1");
        poll.choice = Some(match button {
            MenuButton::Intro => MenuChoice::Intro,
            MenuButton::NewGame => MenuChoice::NewGame,
            MenuButton::LoadGame => MenuChoice::LoadGame,
            MenuButton::Credits if shift_held => MenuChoice::Quotes,
            MenuButton::Credits => MenuChoice::Credits,
            MenuButton::Exit => MenuChoice::Exit,
        });
    };
    match input {
        MenuInput::Key(key) => {
            let lower = key.to_ascii_lowercase();
            if let Some(button) = MENU_BUTTONS.iter().find(|b| b.hotkey() == lower) {
                select(*button, &mut poll);
            } else {
                match key {
                    '+' | '=' => poll.brightness = 1,
                    '-' | '_' => poll.brightness = -1,
                    'd' | 'D' => poll.choice = Some(MenuChoice::Timeout),
                    _ => {}
                }
            }
        }
        MenuInput::Release(button) => select(button, &mut poll),
        MenuInput::Press(_) => poll.sounds.push("nmselec0"),
        MenuInput::CtrlR => poll.record = true,
        MenuInput::Escape => {
            poll.sounds.push("nmselec1");
            poll.choice = Some(MenuChoice::Exit);
        }
        MenuInput::None => {}
    }
    if poll.choice.is_none() && elapsed_ms >= MENU_TIMEOUT_MS {
        poll.choice = Some(MenuChoice::Timeout);
    }
    poll
}

/// §3.6 attract mode: a toggle starting on "demo" flips on every trigger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attract {
    intro_turn: bool,
    demo: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttractAction {
    Demo(usize),
    Intro,
}
/// Every attract trigger plays the intro with flag 0x08 (pause music).
pub const ATTRACT_INTRO_FLAGS: u32 = 0x08;
/// Demo playback seed (SPEC-006 §3.4).
pub const DEMO_SEED: u32 = 0xBEEF_FEED;
impl Attract {
    /// The next trigger, with `demos` `selfrun/*.sdf` files present. A demo
    /// turn without demos plays the intro.
    pub fn trigger(&mut self, demos: usize) -> AttractAction {
        let demo_turn = !self.intro_turn;
        self.intro_turn = !self.intro_turn;
        if demo_turn && demos > 0 {
            let index = self.demo % demos;
            self.demo = (index + 1) % demos;
            AttractAction::Demo(index)
        } else {
            AttractAction::Intro
        }
    }
}

/// §3.5 death scene: art, palette, and the narration for draw D(1, 4).
pub const DEATH_ART: &str = "death.frm";
pub const DEATH_PALETTE: &str = "art/intrface/death.pal";
pub fn death_narration(draw: i32) -> String {
    format!("sound/speech/narrator/nar_{}.acm", draw + 2)
}
/// Without speech the scene waits this long; an uninterrupted end waits
/// `DEATH_TAIL_MS` more.
pub const DEATH_SILENT_MS: u64 = 3000;
pub const DEATH_TAIL_MS: u64 = 500;

/// §3.7 premade entries: sheet, bio, portrait art and ID text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Premade {
    pub sheet: &'static str,
    pub bio: &'static str,
    pub portrait: &'static str,
    pub id: &'static str,
}
pub const PREMADES: [Premade; 3] = [
    Premade {
        sheet: "premade/combat.gcd",
        bio: "premade/combat.bio",
        portrait: "combat.frm",
        id: "VID 208-197-88-125",
    },
    Premade {
        sheet: "premade/stealth.gcd",
        bio: "premade/stealth.bio",
        portrait: "stealth.frm",
        id: "VID 208-206-49-229",
    },
    Premade {
        sheet: "premade/diplomat.gcd",
        bio: "premade/diplomat.bio",
        portrait: "diplomat.frm",
        id: "VID 208-206-49-227",
    },
];
pub const SELECTION_BACKGROUND: &str = "pickchar.frm";
pub const SELECTION_MONITOR: Rect = Rect::new(40, 30, 560, 300);
pub const SELECTION_PREVIOUS: Rect = Rect::new(292, 320, 20, 18);
pub const SELECTION_NEXT: Rect = Rect::new(318, 320, 20, 18);
/// Take, Modify, Create and Back red buttons.
pub const SELECTION_BUTTONS: [[i32; 2]; 4] = [[81, 323], [435, 320], [80, 425], [461, 425]];
pub const SELECTION_TEXT: ColorKey = ColorKey::GREEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    /// Take the current premade: "start".
    Take,
    /// Open the editor on the current premade.
    Modify,
    /// Reset the player to blank defaults, then open the editor.
    Create,
    Back,
    Previous,
    Next,
    QuitConfirm,
    Brightness(i32),
}
/// §3.7 selection keys (the four red buttons act as `t`, `m`, `c`, Esc).
pub fn selection_key(key: SelectionKey) -> Option<SelectionAction> {
    Some(match key {
        SelectionKey::Char('t' | 'T') => SelectionAction::Take,
        SelectionKey::Char('m' | 'M') => SelectionAction::Modify,
        SelectionKey::Char('c' | 'C') => SelectionAction::Create,
        SelectionKey::Char('b' | 'B') | SelectionKey::Escape => SelectionAction::Back,
        SelectionKey::Left => SelectionAction::Previous,
        SelectionKey::Right => SelectionAction::Next,
        SelectionKey::F10 => SelectionAction::QuitConfirm,
        SelectionKey::Char('+' | '=') => SelectionAction::Brightness(1),
        SelectionKey::Char('-' | '_') => SelectionAction::Brightness(-1),
        SelectionKey::Char(_) => return None,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKey {
    Char(char),
    Escape,
    Left,
    Right,
    F10,
}
/// The cyclic premade index after a step.
pub fn cycle(index: usize, step: i32, len: usize) -> usize {
    (index as i64 + i64::from(step)).rem_euclid(len as i64) as usize
}
/// Portrait placement for art `w` by `h`; odd rows are skipped (the
/// scan-line effect, drawn without modifying the shared art).
pub fn portrait_origin(w: i32, h: i32) -> [i32; 2] {
    [150 - w / 2, 240 - h]
}
/// Bio lines: from y = 40, 10 apart, while y is below 260.
pub const BIO_ORIGIN: [i32; 2] = [420, 40];
pub const BIO_WIDTH: i32 = 220;
pub fn bio_line_y(index: usize) -> Option<i32> {
    let y = 40 + 10 * index as i32;
    (y < 260).then_some(y)
}
/// Bio text lines, each cut to 255 bytes, as many as fit above y = 260.
pub fn bio_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
        .split(|b| *b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .map(|line| line[..line.len().min(255)].to_vec())
        .zip((0..).map_while(bio_line_y))
        .map(|(line, _)| line)
        .collect()
}
/// SPECIAL row text "<name> NN" (two-digit effective value), drawn
/// right-aligned to x = 348 at `special_row_y(k)`, followed by two spaces
/// and the level word from x = 348.
pub fn special_row(name: &[u8], value: i32) -> Vec<u8> {
    let mut text = name.to_vec();
    text.extend(format!(" {value:02}").bytes());
    text
}
pub const SPECIAL_RIGHT: i32 = 348;
pub fn special_row_y(k: usize) -> i32 {
    70 + 10 * k as i32
}
/// `stat.msg` level word for a value, clamped to 1-10.
pub fn level_word(value: i32) -> i32 {
    300 + value.clamp(1, 10)
}

/// §3.12 the new-game hand-off.
pub const NEW_GAME_MOVIE: (&str, u32) = ("art/cuts/ovrintro.mve", 0x04);
pub const NEW_GAME_MAP: &str = "maps/v13ent.map";
pub const NEW_GAME_MUSIC: &str = "sound/music/06vault.acm";

/// §3.13 generic dialog boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogSize {
    Large,
    Medium,
}
impl DialogSize {
    pub fn art(self) -> (&'static str, [i32; 2]) {
        match self {
            Self::Large => ("lgdialog.frm", [302, 127]),
            Self::Medium => ("medialog.frm", [215, 113]),
        }
    }
}
/// A Yes/No or two-button box is always large; otherwise the request, else
/// large when the widest line exceeds 168 pixels or there are more than five
/// lines including the title.
pub fn dialog_size(
    two_buttons: bool,
    requested: Option<DialogSize>,
    widest: i32,
    lines: usize,
) -> DialogSize {
    if two_buttons {
        return DialogSize::Large;
    }
    requested.unwrap_or(if widest > 168 || lines > 5 {
        DialogSize::Large
    } else {
        DialogSize::Medium
    })
}
/// Title y for `body_lines` body lines; body lines follow 10 apart.
pub fn dialog_title_y(body_lines: usize) -> i32 {
    27 + 30 - (body_lines as i32 + 1) * 10 / 2
}
/// Body lines wider than this wrap.
pub const DIALOG_WRAP: i32 = 276;
pub const DONE_BOX: (&str, [i32; 2]) = ("donebox.frm", [108, 24]);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogButton {
    pub box_origin: [i32; 2],
    pub caption_origin: [i32; 2],
    pub button: [i32; 2],
    /// `dbox.msg` caption.
    pub caption: i32,
    pub transparent_box: bool,
}
/// Done box, or the Yes and No boxes.
pub fn dialog_buttons(yes_no: bool) -> Vec<DialogButton> {
    if yes_no {
        vec![
            DialogButton {
                box_origin: [37, 98],
                caption_origin: [72, 101],
                button: [50, 102],
                caption: 101,
                transparent_box: false,
            },
            DialogButton {
                box_origin: [169, 98],
                caption_origin: [204, 101],
                button: [182, 102],
                caption: 102,
                transparent_box: true,
            },
        ]
    } else {
        vec![DialogButton {
            box_origin: [97, 98],
            caption_origin: [132, 101],
            button: [110, 102],
            caption: 100,
            transparent_box: false,
        }]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKey {
    Enter,
    Escape,
    Char(char),
}
/// The answer a key gives: `Some(true)` yes/OK, `Some(false)` no/cancel.
pub fn dialog_answer(key: DialogKey, yes_no: bool) -> Option<bool> {
    match key {
        DialogKey::Enter => Some(true),
        DialogKey::Escape => Some(false),
        DialogKey::Char('y' | 'Y') if yes_no => Some(true),
        DialogKey::Char('n' | 'N') if yes_no => Some(false),
        DialogKey::Char(_) => None,
    }
}
