//! SPEC-019's interface under the Fallout 2 profile, independent of
//! windowing: art names and their candidates (§2.2), engine strings
//! (§2.3), the main menu (§3.2), the bar geometry from `iface.frm`'s height
//! and the five indicator boxes (§3.5), the world-map side panel, town list
//! and city circles (§3.12), the town-map entrance markers (§3.13), the
//! combat control and custom panels (§3.15), the fuel gauge (§3.19.4),
//! the Pip-Boy's Video Archives (§3.10), the kill-type names (§3.9) and
//! the endgame slides (§3.20). Coordinates are window-relative unless
//! named "screen".
use super::{Armed, ColorKey, Layout, Rect, flow::MenuChoice};
use vault_formats::fallout2::{
    lines::{EndgameSlide, SlideArt},
    party::ControlField,
};

/// §2.2 a piece of Fallout 2 interface art: its candidate names, tried in
/// order through `intrface.lst`, and its stand-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtRole {
    pub candidates: &'static [&'static str],
    pub stand_in: StandIn,
}
/// §2.2 rule 4 the stand-ins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandIn {
    /// The red button, else a 15-by-16 key 18979 rectangle with a key
    /// 14723 border.
    Button,
    /// A key-0 rectangle of this size.
    Background([i32; 2]),
    /// The enhanced layer's engine-drawn icon, else a blank.
    Icon,
    /// Nothing is drawn.
    None,
}
pub const IFACE: ArtRole = ArtRole {
    candidates: &["iface.frm"],
    stand_in: StandIn::Background([640, 100]),
};
pub const PUSH_ICONS: [ArtRole; 2] = [
    ArtRole {
        candidates: &["pushn.frm"],
        stand_in: StandIn::Icon,
    },
    ArtRole {
        candidates: &["pushh.frm"],
        stand_in: StandIn::Icon,
    },
];
pub const TAKE_ALL: [ArtRole; 2] = [
    ArtRole {
        candidates: &["invmaup.frm", "takeallu.frm"],
        stand_in: StandIn::Icon,
    },
    ArtRole {
        candidates: &["invmadn.frm", "takealld.frm"],
        stand_in: StandIn::Icon,
    },
];
pub const PARTY_TALK_PANEL: ArtRole = ArtRole {
    candidates: &["di_talkp.frm", "di_talk.frm"],
    stand_in: StandIn::Background([640, 190]),
};
pub const CONTROL_PANEL: ArtRole = ArtRole {
    candidates: &["control.frm"],
    stand_in: StandIn::Background([640, 190]),
};
pub const CUSTOM_PANEL: ArtRole = ArtRole {
    candidates: &["custom.frm"],
    stand_in: StandIn::Background([300, 190]),
};
pub const DIAL: ArtRole = ArtRole {
    candidates: &["wmdial.frm"],
    stand_in: StandIn::None,
};
pub const LIST_ARROWS: [ArtRole; 2] = [
    ArtRole {
        candidates: &["wmlstup.frm", "uparwoff.frm"],
        stand_in: StandIn::Button,
    },
    ArtRole {
        candidates: &["wmlstdn.frm", "dnarwoff.frm"],
        stand_in: StandIn::Button,
    },
];
/// Every §2.2 named role, for the asset audit (D-2).
pub const NAMED_ART: [ArtRole; 12] = [
    IFACE,
    PUSH_ICONS[0],
    PUSH_ICONS[1],
    TAKE_ALL[0],
    TAKE_ALL[1],
    PARTY_TALK_PANEL,
    CONTROL_PANEL,
    CUSTOM_PANEL,
    DIAL,
    LIST_ARROWS[0],
    LIST_ARROWS[1],
    ArtRole {
        candidates: &["mainmenu.frm"],
        stand_in: StandIn::Background([640, 480]),
    },
];
impl ArtRole {
    /// §2.2 rules 1-2: the first candidate present in `intrface.lst`
    /// (compared case-insensitively), with its zero-based row.
    pub fn resolve<'a>(
        &self,
        rows: impl IntoIterator<Item = &'a [u8]> + Clone,
    ) -> Option<(&'static str, usize)> {
        self.candidates.iter().find_map(|name| {
            rows.clone()
                .into_iter()
                .position(|row| row.eq_ignore_ascii_case(name.as_bytes()))
                .map(|row| (*name, row))
        })
    }
}
/// §2.2 rule 4 the button stand-in's size and colors.
pub const STAND_IN_BUTTON: ([i32; 2], ColorKey, ColorKey) =
    ([15, 16], ColorKey::LABEL, ColorKey::LABEL_PRESSED);

/// §2.3 the engine strings.
pub mod text {
    pub const MENU: [&str; 6] = [
        "INTRO",
        "NEW GAME",
        "LOAD GAME",
        "OPTIONS",
        "CREDITS",
        "EXIT",
    ];
    pub const INDICATORS: [&str; 5] = ["SNEAK", "LEVEL", "ADDICT", "POISONED", "RADIATED"];
    pub const PUSH_BLOCKED: &str = crate::host_fallout2_party::PUSH_BLOCKED;
    pub const LEVEL: &str = crate::host_fallout2_party::LEVEL_GAINED;
    pub const FUEL_OUT: &str = crate::host_fallout2::FUEL_OUT;
    pub const CONTROL_TITLE: &str = "COMBAT CONTROL";
    pub const WEAPON: &str = "Weapon";
    pub const ARMOR: &str = "Armor";
    pub const BEST: &str = "USE BEST WEAPON";
    pub const CUSTOM: &str = "CUSTOM";
    pub const TALK: &str = "TALK";
    pub const DONE: &str = "DONE";
    pub const DISPOSITIONS: [&str; 5] = ["Custom", "Coward", "Defensive", "Aggressive", "Berserk"];
    pub const ROWS: [&str; 6] = [
        "Area Attacks",
        "Run Away",
        "Weapon Preference",
        "Distance",
        "Attack Who",
        "Chem Use",
    ];
    /// E-hp, E-ac and E-lvl.
    pub fn hp(current: i32, max: i32) -> String {
        format!("HP {current}/{max}")
    }
    pub fn ac(ac: i32) -> String {
        format!("AC {ac}")
    }
    pub fn level(level: i32) -> String {
        format!("Level {level}")
    }
}
/// §2.3 E-val text of a control field's value.
pub fn value_text(field: ControlField, value: i32) -> String {
    crate::fallout2_party::value_text(field, value)
}

/// §3.2 the main menu's six buttons.
pub const MENU_CHOICES: [MenuChoice; 6] = [
    MenuChoice::Intro,
    MenuChoice::NewGame,
    MenuChoice::LoadGame,
    MenuChoice::Options,
    MenuChoice::Credits,
    MenuChoice::Exit,
];
pub const MENU_HOTKEYS: [char; 6] = ['i', 'n', 'l', 'o', 'c', 'e'];
pub const MENU_VERSION: &str = "FALLOUT II 1.02d";
/// The copyright line is `misc.msg` 20.
pub const MENU_COPYRIGHT_ID: i32 = 20;

/// §3.3 (Revision 3) the premade selection screen's monitor window: the
/// portrait's top-left is its top-left, and the portrait is clipped to it.
/// The retail portraits are 592 by 260 and fill it, bezel edge included.
pub const SELECTION_SCREEN: Rect = Rect::new(27, 23, 592, 260);
/// Button i's rectangle.
pub fn menu_button(i: usize) -> Rect {
    Rect::new(30, 19 + 41 * i as i32, 26, 26)
}
/// Label i's top-left for a label `width` pixels wide (font 104).
pub fn menu_label(i: usize, width: i32) -> [i32; 2] {
    [126 - width / 2, 20 + 41 * i as i32]
}
/// The button under `point`, if any (the caller tests the up image's
/// pixel for the "only nonzero pixels" rule).
pub fn menu_button_at(point: [i32; 2]) -> Option<usize> {
    (0..6).find(|i| menu_button(*i).contains(point))
}
/// A hotkey's button, either case.
pub fn menu_hotkey(key: char) -> Option<usize> {
    MENU_HOTKEYS
        .iter()
        .position(|hotkey| *hotkey == key.to_ascii_lowercase())
}

/// One input a Fallout 2 main-menu poll observes; buttons by index 0-5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    None,
    Key(char),
    Escape,
    CtrlR,
    Press(usize),
    Release(usize),
}
/// §3.2 one poll of the menu loop, as SPEC-014 §3.4 with the Options
/// button: a release or hotkey selects (`nmselec1`), a press sounds
/// `nmselec0`, Shift+Credits is Quotes, Esc exits, `d` and the 120-second
/// timeout run attract mode, and the brightness keys step.
pub fn menu_poll(input: MenuInput, shift_held: bool, elapsed_ms: u64) -> super::flow::MenuPoll {
    let mut poll = super::flow::MenuPoll::default();
    let select = |button: usize, poll: &mut super::flow::MenuPoll| {
        let Some(choice) = MENU_CHOICES.get(button) else {
            return;
        };
        poll.sounds.push("nmselec1");
        poll.choice = Some(match choice {
            MenuChoice::Credits if shift_held => MenuChoice::Quotes,
            choice => *choice,
        });
    };
    match input {
        MenuInput::Key(key) => match menu_hotkey(key) {
            Some(button) => select(button, &mut poll),
            None => match key {
                '+' | '=' => poll.brightness = 1,
                '-' | '_' => poll.brightness = -1,
                'd' | 'D' => poll.choice = Some(MenuChoice::Timeout),
                _ => {}
            },
        },
        MenuInput::Release(button) => select(button, &mut poll),
        MenuInput::Press(_) => poll.sounds.push("nmselec0"),
        MenuInput::CtrlR => poll.record = true,
        MenuInput::Escape => {
            poll.sounds.push("nmselec1");
            poll.choice = Some(MenuChoice::Exit);
        }
        MenuInput::None => {}
    }
    if poll.choice.is_none() && elapsed_ms >= super::flow::MENU_TIMEOUT_MS {
        poll.choice = Some(MenuChoice::Timeout);
    }
    poll
}

/// §3.5.1 the interface bar's geometry for an `iface.frm` of height h
/// (values below 60 or above 100 are taken as 100, with a diagnostic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarGeometry {
    pub height: i32,
    /// The height was out of range.
    pub clamped: bool,
}
impl BarGeometry {
    pub fn new(height: i32) -> Self {
        let clamped = !(60..=100).contains(&height);
        Self {
            height: if clamped { 100 } else { height },
            clamped,
        }
    }
    /// The bar window's screen origin, 640 by h.
    pub fn origin(self) -> [i32; 2] {
        [0, 479 - self.height]
    }
    /// An element's window-relative position from its SPEC-014 one: y
    /// minus `100 - h`, so its screen position is kept.
    pub fn element(self, spec014: [i32; 2]) -> [i32; 2] {
        [spec014[0], spec014[1] - (100 - self.height)]
    }
    pub fn rect(self, spec014: Rect) -> Rect {
        let [x, y] = self.element([spec014.x, spec014.y]);
        Rect::new(x, y, spec014.w, spec014.h)
    }
    /// The map window, 640 by `481 - h`.
    pub fn map_size(self) -> [i32; 2] {
        [640, 481 - self.height]
    }
    /// The last screen row a map click accepts.
    pub fn map_bottom(self) -> i32 {
        479 - self.height
    }
    /// The indicator window's screen origin.
    pub fn indicator_origin(self) -> [i32; 2] {
        [0, 479 - self.height - 21]
    }
}

/// §3.5.2 the five indicator boxes, in drawing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Addict,
    Sneak,
    Level,
    Poisoned,
    Radiated,
}
impl Indicator {
    /// `intrface.msg` label ID, engine string and color.
    pub fn label(self) -> (i32, &'static str, ColorKey) {
        match self {
            Self::Sneak => (100, text::INDICATORS[0], ColorKey::GREEN),
            Self::Level => (101, text::INDICATORS[1], ColorKey::GREEN),
            Self::Addict => (102, text::INDICATORS[2], ColorKey::RED),
            Self::Poisoned => (103, text::INDICATORS[3], ColorKey::RED),
            Self::Radiated => (104, text::INDICATORS[4], ColorKey::RED),
        }
    }
}
/// The lit boxes in order: ADDICT, SNEAK, LEVEL, POISONED, RADIATED.
pub fn indicators(
    addict: bool,
    sneak: bool,
    level: bool,
    poison: i32,
    radiation: i32,
) -> Vec<Indicator> {
    [
        (addict, Indicator::Addict),
        (sneak, Indicator::Sneak),
        (level, Indicator::Level),
        (poison > 0, Indicator::Poisoned),
        (radiation > 0, Indicator::Radiated),
    ]
    .into_iter()
    .filter_map(|(on, indicator)| on.then_some(indicator))
    .collect()
}

/// §3.12.1 the view over a world of `world` pixels (SPEC-018 §5.3): the
/// SPEC-014 §3.39 rules with the origin clamped to 0 through world width
/// - 450 and 0 through world height - 442.
pub mod view {
    use crate::worldmap::{VIEWPORT_SIZE, screen::SCROLL_STEP};
    pub fn limit(world: [i32; 2]) -> [i32; 2] {
        [
            (world[0] - VIEWPORT_SIZE[0]).max(0),
            (world[1] - VIEWPORT_SIZE[1]).max(0),
        ]
    }
    pub fn clamp(view: [i32; 2], world: [i32; 2]) -> [i32; 2] {
        let limit = limit(world);
        [view[0].clamp(0, limit[0]), view[1].clamp(0, limit[1])]
    }
    /// Centering on world position (X, Y): `(X - 247, Y - 242)`, clamped.
    pub fn centered(position: [i32; 2], world: [i32; 2]) -> [i32; 2] {
        clamp([position[0] - 247, position[1] - 242], world)
    }
    /// One 16-pixel step on each requested axis, each clamped on its own;
    /// also whether every requested axis was already at its limit.
    pub fn scroll(view: [i32; 2], direction: [i32; 2], world: [i32; 2]) -> ([i32; 2], bool) {
        let limits = limit(world);
        let mut next = view;
        let mut blocked = direction != [0, 0];
        for axis in 0..2 {
            let step = direction[axis];
            if step == 0 {
                continue;
            }
            let limit = if step < 0 { 0 } else { limits[axis] };
            if view[axis] != limit {
                blocked = false;
            }
            next[axis] = (view[axis] + SCROLL_STEP * step).clamp(0, limits[axis]);
        }
        (next, blocked)
    }
    /// Home centers on the party with vy 0; End takes vy to its limit.
    pub fn home(position: [i32; 2], world: [i32; 2]) -> [i32; 2] {
        [centered(position, world)[0], 0]
    }
    pub fn end(view: [i32; 2], world: [i32; 2]) -> [i32; 2] {
        [view[0], limit(world)[1]]
    }
}

/// §3.12.2 a city's circle outline of radius r around `center`: the pixels
/// whose rounded distance from the center is r (one pixel thick, eight-way
/// symmetric), from the midpoint circle rule.
pub fn circle(center: [i32; 2], radius: i32) -> Vec<[i32; 2]> {
    let mut points = Vec::new();
    if radius <= 0 {
        points.push(center);
        return points;
    }
    let (mut x, mut y, mut error) = (radius, 0, 1 - radius);
    while x >= y {
        for [dx, dy] in [
            [x, y],
            [y, x],
            [-y, x],
            [-x, y],
            [-x, -y],
            [-y, -x],
            [y, -x],
            [x, -y],
        ] {
            let point = [center[0] + dx, center[1] + dy];
            if !points.contains(&point) {
                points.push(point);
            }
        }
        y += 1;
        if error < 0 {
            error += 2 * y + 1;
        } else {
            x -= 1;
            error += 2 * (y - x) + 1;
        }
    }
    points
}
/// §3.12.2 the city name's top-left: centered on X, top at `Y + r + 2`.
pub fn city_label(center: [i32; 2], radius: i32, width: i32) -> [i32; 2] {
    [center[0] - width / 2, center[1] + radius + 2]
}

/// §3.12.3 the side panel.
pub mod panel {
    use super::Rect;
    pub const DAY: [i32; 2] = [487, 12];
    pub const MONTH: [i32; 2] = [513, 12];
    pub const YEAR: [i32; 2] = [548, 12];
    pub const TIME: [i32; 2] = [593, 12];
    pub const DIAL: [i32; 2] = [532, 48];
    pub const ROWS: usize = 7;
    pub const LIST_UP: Rect = Rect::new(590, 112, 15, 16);
    pub const LIST_DOWN: Rect = Rect::new(590, 330, 15, 16);
    pub const TOWN_WORLD: Rect = Rect::new(520, 439, 15, 15);
    pub const GAUGE: Rect = Rect::new(493, 138, 6, 189);
    /// Row k's red button (the top 15 rows shown).
    pub fn row_button(k: usize) -> Rect {
        Rect::new(508, 138 + 27 * k as i32, 15, 15)
    }
    /// Row k's label origin.
    pub fn row_label(k: usize) -> [i32; 2] {
        [531, 138 + 27 * k as i32]
    }
    /// The dial frame for the clock's hour 0-23.
    pub fn dial_frame(hour: i32, frames: i32) -> i32 {
        (hour.clamp(0, 23) * frames.max(1) / 24).clamp(0, frames.max(1) - 1)
    }
}
/// §3.12.3 the town list: every known city in table order, seen through
/// seven rows from `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TownList {
    pub offset: usize,
}
impl TownList {
    /// The offset clamped into range for `count` entries.
    pub fn clamp(&mut self, count: usize) {
        self.offset = self.offset.min(count.saturating_sub(panel::ROWS));
    }
    pub fn can_scroll_up(self) -> bool {
        self.offset > 0
    }
    pub fn can_scroll_down(self, count: usize) -> bool {
        self.offset + panel::ROWS < count
    }
    pub fn up(&mut self) -> bool {
        let moved = self.can_scroll_up();
        if moved {
            self.offset -= 1;
        }
        moved
    }
    pub fn down(&mut self, count: usize) -> bool {
        let moved = self.can_scroll_down(count);
        if moved {
            self.offset += 1;
        }
        moved
    }
    /// The entry row k shows, if any.
    pub fn entry(self, k: usize, count: usize) -> Option<usize> {
        (k < panel::ROWS && self.offset + k < count).then_some(self.offset + k)
    }
}

/// §3.13 an entrance marker for an anchor at (x, y): the 164-by-25 button
/// at (x - 57, y - 6) and the triangle at (x + 12, y + 6).
pub fn entrance_button(anchor: [i32; 2]) -> Rect {
    Rect::new(anchor[0] - 57, anchor[1] - 6, 164, 25)
}
pub fn entrance_triangle(anchor: [i32; 2]) -> [i32; 2] {
    [anchor[0] + 12, anchor[1] + 6]
}
/// §3.13 the town picture's place and the frame's hole.
pub const TOWN_PICTURE: [i32; 2] = [20, 20];

/// §3.19.4 the gauge's filled rectangle (from the bottom) for `rows`.
pub fn gauge_fill(rows: i32) -> Rect {
    let rows = rows.clamp(0, panel::GAUGE.h);
    Rect::new(
        panel::GAUGE.x,
        panel::GAUGE.y + panel::GAUGE.h - rows,
        panel::GAUGE.w,
        rows,
    )
}

/// §3.15.2 the combat control panel, at screen (0, 290), 640 by 190.
pub mod control {
    use super::Rect;
    pub const ORIGIN: [i32; 2] = [0, 290];
    pub const SIZE: [i32; 2] = [640, 190];
    pub const TITLE: [i32; 2] = [20, 8];
    pub const NAME: [i32; 2] = [20, 28];
    pub const NAME_CLIP: i32 = 120;
    pub const STATS: [[i32; 2]; 3] = [[20, 42], [20, 54], [20, 66]];
    pub const WEAPON_BOX: Rect = Rect::new(150, 26, 90, 61);
    pub const WEAPON_CAPTION: [i32; 2] = [150, 14];
    pub const ARMOR_BOX: Rect = Rect::new(250, 26, 90, 61);
    pub const ARMOR_CAPTION: [i32; 2] = [250, 14];
    pub const BEST_BUTTON: [i32; 2] = [150, 100];
    pub const BEST_CAPTION: [i32; 2] = [170, 100];
    pub const CUSTOM_BUTTON: [i32; 2] = [380, 134];
    pub const CUSTOM_CAPTION: [i32; 2] = [400, 134];
    pub const TALK: Rect = Rect::new(584, 162, 14, 14);
    pub const TALK_ART: (&str, &str) = ("di_rdbt2.frm", "di_rdbt1.frm");
    pub const TALK_SOUNDS: (&str, &str) = ("ib2p1xx1", "ib2lu1x1");
    /// Disposition row k's red button and label.
    pub fn disposition_button(k: usize) -> [i32; 2] {
        [380, 20 + 22 * k as i32]
    }
    pub fn disposition_label(k: usize) -> [i32; 2] {
        [400, 20 + 22 * k as i32]
    }
    /// A red button's clickable rectangle at `at`.
    pub fn button_rect(at: [i32; 2]) -> Rect {
        let [w, h] = crate::ui::RED_BUTTON.2;
        Rect::new(at[0], at[1], w, h)
    }
}
/// §3.15.3 the custom panel, a modal window the size of its background
/// (Revision 2). Its rows and Done are window-relative.
pub mod custom {
    use super::Rect;
    /// The stand-in's window: screen (170, 145), 300 by 190.
    pub const ORIGIN: [i32; 2] = [170, 145];
    pub const SIZE: [i32; 2] = [300, 190];
    /// The retail `custom.frm`'s window: 640 by 190 at screen (0, 290), in
    /// the control panel's place.
    pub const FULL_ORIGIN: [i32; 2] = [0, 290];
    /// The window for a background of `size` (`None` for the stand-in). A
    /// background as wide as the 640-pixel screen takes the control
    /// panel's place, its bottom on the screen's; any other is centred on
    /// the 640 by 480 screen, as the stand-in is.
    pub fn window(size: Option<[i32; 2]>) -> Rect {
        let [w, h] = size.unwrap_or(SIZE);
        if w >= 640 {
            Rect::new(0, 480 - h, w, h)
        } else {
            Rect::new((640 - w) / 2, (480 - h) / 2, w, h)
        }
    }
    pub const DONE_BUTTON: [i32; 2] = [110, 160];
    pub const DONE_CAPTION: [i32; 2] = [130, 160];
    /// Row j's label, red button and value text.
    pub fn label(j: usize) -> [i32; 2] {
        [16, 20 + 22 * j as i32]
    }
    pub fn button(j: usize) -> [i32; 2] {
        [150, 20 + 22 * j as i32]
    }
    pub fn value(j: usize) -> [i32; 2] {
        [170, 20 + 22 * j as i32]
    }
}
/// §3.15.3 the custom panel's six fields, in §2.5 order.
pub const CUSTOM_FIELDS: [ControlField; 6] = [
    ControlField::AreaAttackMode,
    ControlField::RunAwayMode,
    ControlField::BestWeapon,
    ControlField::Distance,
    ControlField::AttackWho,
    ControlField::ChemUse,
];
/// §3.8 the Combat Control button at the old Tell Me About place.
pub const COMBAT_CONTROL_BUTTON: Rect = Rect::new(593, 116, 14, 14);

/// §3.7 the inventory's total-weight line is drawn in key 31744 over the
/// limit, key 992 otherwise.
pub fn weight_color(total: i32, limit: i32) -> ColorKey {
    if total > limit {
        ColorKey::RED
    } else {
        ColorKey::GREEN
    }
}
/// §3.7 the trade session's Offer message (`inventry.msg` 27).
pub const TRADE_MESSAGE: i32 = 27;

/// §3.10 the Video Archives: each movie index 2 through 15 whose played
/// byte is set, in index order.
pub fn archive_movies(played: &[u8; 17]) -> Vec<usize> {
    (2..=15).filter(|index| played[*index] != 0).collect()
}
/// §3.10 the movie Archives row `selection` (from 1) plays, if any.
pub fn archive_selection(played: &[u8; 17], selection: i32) -> Option<usize> {
    let row = usize::try_from(selection).ok()?.checked_sub(1)?;
    archive_movies(played).get(row).copied()
}
/// §3.10 an archive title, `pipboy.msg` 500 + index.
pub fn archive_title(index: usize) -> i32 {
    500 + index as i32
}

/// §3.9 the kill types 0-18.
pub const KILL_TYPES: usize = 19;
/// §3.9 kill type t's `proto.msg` name and description.
pub fn kill_messages(t: usize) -> (i32, i32) {
    (1450 + t as i32, 1469 + t as i32)
}
/// §3.9 the English fallbacks for a missing name.
pub const KILL_FALLBACKS: [&str; KILL_TYPES] = [
    "Man",
    "Woman",
    "Child",
    "Super Mutant",
    "Ghoul",
    "Brahmin",
    "Radscorpion",
    "Rat",
    "Floater",
    "Centaur",
    "Robot",
    "Dog",
    "Mantis",
    "Deathclaw",
    "Plant",
    "Gecko",
    "Alien",
    "Giant Ant",
    "Big Bad Boss",
];
/// §3.9 up to 10 types fill the left column.
pub const KILL_LEFT_ROWS: usize = 10;

/// §3.20 one endgame slide from `endgame.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slide2 {
    pub art: String,
    pub palette: String,
    pub narration: String,
    /// A desert pan, and whether it runs left to right.
    pub pan: Option<bool>,
}
/// §3.20 every record whose global equals its value, in table order. An
/// art row index resolves through `intrface.lst` (`row_name`); a record
/// whose art cannot be named is skipped with a note.
pub fn endgame_slides(
    table: &[EndgameSlide],
    globals: &[i32],
    row_name: impl Fn(i32) -> Option<String>,
    notes: &mut Vec<String>,
) -> Vec<Slide2> {
    let global = |index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|index| globals.get(index))
            .copied()
            .unwrap_or(0)
    };
    table
        .iter()
        .filter(|slide| global(slide.global) == slide.value)
        .filter_map(|slide| {
            let (art, palette) = match &slide.art {
                SlideArt::Row(row) => {
                    let Some(name) = row_name(*row) else {
                        notes.push(format!("endgame art row {row} is not in intrface.lst"));
                        return None;
                    };
                    let art = format!("art/intrface/{}", name.to_ascii_lowercase());
                    let palette = vault_formats::fallout2::lines::row_palette(&art);
                    (art, palette)
                }
                _ => (slide.art_path()?, slide.palette_path()?),
            };
            Some(Slide2 {
                art,
                palette,
                narration: slide.narration_path(),
                pan: match slide.direction {
                    1 => Some(true),
                    -1 => Some(false),
                    _ => None,
                },
            })
        })
        .collect()
}

/// §3.8: under Fallout 2 there is no Tell Me About button and `a` does
/// nothing in the reply loop.
pub const TELL_ME_ABOUT: bool = false;

// SPEC-020 §2.1: the Fallout 2 panels' layouts and dispatch. Rectangles
// are frame (screen) pixels.

/// A control of the combat control panel (§3.15.2) and of the custom panel
/// over it (§3.15.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlButton {
    /// Disposition row k (0-4).
    Disposition(usize),
    Custom,
    Best,
    Talk,
    /// A custom-panel row (0-5).
    Row(usize),
    Done,
}
impl ControlButton {
    /// Talk's own sounds; the red buttons'.
    pub fn sounds(self) -> (&'static str, &'static str) {
        match self {
            Self::Talk => control::TALK_SOUNDS,
            _ => (super::BUTTON_PRESS, super::BUTTON_RELEASE),
        }
    }
}
/// The control panel's enabled buttons on screen: the allowed disposition
/// rows (`allowed` holds the member's allowed disposition values, the
/// custom one being `fallout2_party::disposition::CUSTOM`), Custom when
/// allowed, Use Best Weapon and Talk.
pub fn control_layout(allowed: &[i32]) -> Layout<ControlButton> {
    let origin = control::ORIGIN;
    let mut layout: Layout<ControlButton> = (0..5)
        .filter(|k| allowed.contains(&(*k as i32)))
        .map(|k| {
            (
                ControlButton::Disposition(k),
                control::button_rect(control::disposition_button(k)).offset(origin),
            )
        })
        .collect();
    if allowed.contains(&crate::fallout2_party::disposition::CUSTOM) {
        layout.push(
            ControlButton::Custom,
            control::button_rect(control::CUSTOM_BUTTON).offset(origin),
        );
    }
    layout.push(
        ControlButton::Best,
        control::button_rect(control::BEST_BUTTON).offset(origin),
    );
    layout.with(ControlButton::Talk, control::TALK.offset(origin))
}
/// The custom panel's buttons on screen, for a background of `background`
/// (`None`: the stand-in): its six rows, then Done.
pub fn custom_layout(background: Option<[i32; 2]>) -> Layout<ControlButton> {
    let window = custom::window(background);
    let origin = [window.x, window.y];
    (0..CUSTOM_FIELDS.len())
        .map(|j| {
            (
                ControlButton::Row(j),
                control::button_rect(custom::button(j)).offset(origin),
            )
        })
        .chain([(
            ControlButton::Done,
            control::button_rect(custom::DONE_BUTTON).offset(origin),
        )])
        .collect()
}
/// What a combat-control input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOutcome {
    None,
    /// Choose disposition k for the member (the host refuses a value it
    /// does not allow).
    Disposition(usize),
    /// Use Best Weapon (`iisxxxx1` when it does nothing).
    BestWeapon,
    /// Cycle a custom field's value.
    Cycle(ControlField),
    /// The custom panel opened or closed.
    Custom(bool),
    /// Talk, `t` or Esc: back to the dialogue.
    Talk,
    QuitConfirm,
    Screenshot,
}
/// A key over the combat control panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKey {
    /// `t`.
    Talk,
    Escape,
    Enter,
    /// `u`.
    Best,
    /// `1`-`5` as 0-4.
    Digit(usize),
    Quit,
    F12,
    Other,
}
/// §3.15 the combat control panel over a conversation with a party
/// member, and its modal custom panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPanelWindow {
    /// The custom panel is open over it.
    pub custom: bool,
    /// The member's allowed disposition values.
    pub allowed: Vec<i32>,
    /// The custom panel's background size (`None`: the stand-in).
    pub background: Option<[i32; 2]>,
    armed: Armed<ControlButton>,
}
impl ControlPanelWindow {
    pub fn new(allowed: Vec<i32>, background: Option<[i32; 2]>) -> Self {
        Self {
            custom: false,
            allowed,
            background,
            armed: Armed::new(),
        }
    }
    /// The buttons that act now: the custom panel's while it is open (it is
    /// modal), else the control panel's enabled ones.
    pub fn layout(&self) -> Layout<ControlButton> {
        if self.custom {
            custom_layout(self.background)
        } else {
            control_layout(&self.allowed)
        }
    }
    pub fn pressed(&self) -> Option<ControlButton> {
        self.armed.pressed()
    }
    /// A press holds the button under the pointer (its `sounds`).
    pub fn press(&mut self, point: [i32; 2]) -> Option<ControlButton> {
        self.armed.press(self.layout().hit(point))
    }
    /// A release over the held button acts.
    pub fn release(&mut self, point: Option<[i32; 2]>) -> ControlOutcome {
        let over = point.and_then(|point| self.layout().hit(point));
        match self.armed.release(over) {
            Some(button) => self.dispatch(button),
            None => ControlOutcome::None,
        }
    }
    /// §3.15.2 keys: `t` and Esc return to dialogue, `1`-`5` choose
    /// disposition rows, `u` is Use Best Weapon, the quit keys ask to
    /// quit, F12 takes a screenshot. §3.15.3: Enter and Esc close the
    /// custom panel, which ignores every other key.
    pub fn key(&mut self, key: ControlKey) -> ControlOutcome {
        if self.custom {
            return match key {
                ControlKey::Enter | ControlKey::Escape => self.dispatch(ControlButton::Done),
                _ => ControlOutcome::None,
            };
        }
        match key {
            ControlKey::Talk | ControlKey::Escape => ControlOutcome::Talk,
            ControlKey::Best => self.dispatch(ControlButton::Best),
            ControlKey::Digit(k) if k < 5 => self.dispatch(ControlButton::Disposition(k)),
            ControlKey::Quit => ControlOutcome::QuitConfirm,
            ControlKey::F12 => ControlOutcome::Screenshot,
            _ => ControlOutcome::None,
        }
    }
    pub fn dispatch(&mut self, button: ControlButton) -> ControlOutcome {
        match button {
            ControlButton::Disposition(k) => ControlOutcome::Disposition(k),
            ControlButton::Custom => {
                self.custom = true;
                ControlOutcome::Custom(true)
            }
            ControlButton::Done => {
                self.custom = false;
                ControlOutcome::Custom(false)
            }
            ControlButton::Talk => ControlOutcome::Talk,
            ControlButton::Row(j) => CUSTOM_FIELDS
                .get(j)
                .map_or(ControlOutcome::None, |field| ControlOutcome::Cycle(*field)),
            ControlButton::Best => ControlOutcome::BestWeapon,
        }
    }
}

/// A control of the world map's side panel (§3.12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldPanelControl {
    ListUp,
    ListDown,
    /// The town/world button.
    TownWorld,
    /// Town-list row k (0-6) that shows an entry.
    Row(usize),
}
/// The side panel's buttons on screen (the world map window is at (0, 0)):
/// the list arrows, the rows showing an entry of `count`, and Town/World.
pub fn world_panel_layout(list: TownList, count: usize) -> Layout<WorldPanelControl> {
    let mut layout = Layout::new()
        .with(WorldPanelControl::ListUp, panel::LIST_UP)
        .with(WorldPanelControl::ListDown, panel::LIST_DOWN);
    layout.extend(
        (0..panel::ROWS)
            .filter(|k| list.entry(*k, count).is_some())
            .map(|k| (WorldPanelControl::Row(k), panel::row_button(k))),
    );
    layout.with(WorldPanelControl::TownWorld, panel::TOWN_WORLD)
}
/// §3.13 the town map's entrance buttons by entrance index, for their
/// anchors.
pub fn entrance_layout(anchors: &[[i32; 2]]) -> Layout<usize> {
    anchors
        .iter()
        .enumerate()
        .map(|(i, anchor)| (i, entrance_button(*anchor)))
        .collect()
}

#[cfg(test)]
#[path = "fallout2_tests.rs"]
mod tests;
