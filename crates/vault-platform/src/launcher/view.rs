//! Vault Engine Studio: the launcher's 640 x 480 layout and drawing, in its
//! own palette. The frame is fixed (the window scales it, and never below
//! 640 x 480), so every pane is laid out for those pixels: profile cards on
//! the left, pill tabs over the centre pane's settings, the live preview and
//! a description on the right, and the actions and status along the bottom.
//!
//! [`layout`] lists the widgets in focus order with their rectangles, so
//! clicks, focus and drawing agree.
use super::browse::{Browser, Target};
use super::font::{self, ADVANCE, CHECK, CROSS};
use super::validate::{Check, State};
use super::widgets::{self, BADGE_HEIGHT, Look, Rect, Row};
use super::{
    Field, Launcher, MARGIN_STEPS, ProfileCard, Tab, UI_SCALES, Widget, ZOOM_STEPS, title,
};
use crate::config::{HotkeyHints, Preset};
use crate::enhanced::{
    BuiltinPreset, CameraFollow, GameSpeed, PresetMatch, RoofPeek, ViewportMode,
};
use vault_core::GameProfile;
use vault_formats::{LayerEntry, ModManifest};
use vault_render::{IndexedImage, ScalingMode};

/// The launcher palette's entries. `FIELD` stays the last of them.
pub mod color {
    pub const BACKGROUND: u8 = 0;
    pub const PANEL: u8 = 1;
    pub const BORDER: u8 = 2;
    pub const TEXT: u8 = 3;
    pub const DIM: u8 = 4;
    pub const ACCENT: u8 = 5;
    pub const FOCUS: u8 = 6;
    pub const ERROR: u8 = 7;
    pub const OK: u8 = 8;
    pub const BUTTON: u8 = 9;
    pub const HOVER: u8 = 10;
    pub const SELECTED: u8 = 11;
    /// A warning: the one thing besides errors that is not green.
    pub const WARN: u8 = 12;
    /// The accent under the pointer, and on a dark accent fill.
    pub const BRIGHT: u8 = 13;
    pub const FIELD: u8 = 14;
}

/// The display palette (RGBA): soft greys with a green cast, one green
/// accent, and a pale ring for the keyboard focus.
pub fn palette() -> [[u8; 4]; 256] {
    const COLORS: [[u8; 3]; 15] = [
        [0x0B, 0x0F, 0x0D],
        [0x12, 0x18, 0x15],
        [0x2A, 0x38, 0x31],
        [0xD6, 0xE4, 0xDB],
        [0x82, 0x98, 0x8C],
        [0x4F, 0xD8, 0x86],
        [0xF0, 0xF6, 0xF2],
        [0xFF, 0x6B, 0x5E],
        [0x4F, 0xD8, 0x86],
        [0x1B, 0x24, 0x20],
        [0x2B, 0x3B, 0x34],
        [0x17, 0x3B, 0x2A],
        [0xF2, 0xB8, 0x4B],
        [0x8E, 0xF0, 0xB6],
        [0x07, 0x0A, 0x09],
    ];
    let mut palette = [[0, 0, 0, 255]; 256];
    for (entry, [r, g, b]) in palette.iter_mut().zip(COLORS) {
        *entry = [r, g, b, 255];
    }
    palette
}

const FRAME_WIDTH: i32 = 640;
const FRAME_HEIGHT: i32 = 480;
const FRAME: Rect = Rect::new(0, 0, FRAME_WIDTH, FRAME_HEIGHT);
const HEADER_HEIGHT: i32 = 24;
const FOOTER_TOP: i32 = 438;
const SIDEBAR: Rect = Rect::new(0, HEADER_HEIGHT, 164, FOOTER_TOP - HEADER_HEIGHT);
/// The pill row, over both the centre pane and the right one.
const PILL_TOP: i32 = 29;
const PILL_HEIGHT: i32 = 17;
const PANES_TOP: i32 = 50;
/// The centre pane's content.
const PANE: Rect = Rect::new(172, 54, 284, FOOTER_TOP - 54 - 4);
const RIGHT: Rect = Rect::new(464, PANES_TOP, 176, FOOTER_TOP - PANES_TOP);
/// The live preview, 4:3.
const PREVIEW: Rect = Rect::new(472, 56, 160, 120);
/// The card under the preview.
const DESCRIPTION: Rect = Rect::new(472, 182, 160, 250);

/// The top of the description card: the help beside the settings.
pub(super) const HELP_TOP: i32 = DESCRIPTION.y;
const DESCRIPTION_BODY: Rect = Rect::new(
    DESCRIPTION.x,
    DESCRIPTION.y + 14,
    DESCRIPTION.w,
    DESCRIPTION.h - 14,
);

/// The mod layer list and the browser's entry list.
pub const LAYERS: Rect = Rect::new(PANE.x, 205, 254, 148);
pub const BROWSE_LIST: Rect = Rect::new(184, 64, 424, 328);
/// The browser's dialog.
const DIALOG: Rect = Rect::new(20, 30, 600, 402);
const BROWSE_USE: Rect = Rect::new(456, 402, 104, 20);
const BROWSE_CANCEL: Rect = Rect::new(564, 402, 44, 20);

// --- The sidebar -----------------------------------------------------------

const CARD_TOP: i32 = 40;
const CARD_X: i32 = 6;
const CARD_WIDTH: i32 = 148;
const CARD_HEIGHT: i32 = 70;
const CARD_PITCH: i32 = 73;
/// The cards the sidebar shows at once.
const CARDS_SHOWN: usize = 5;
const CARD_INNER: i32 = CARD_WIDTH - 8;
/// The part of a card that selects it: its title and badges.
const CARD_HEAD: i32 = 38;
const NEW_PROFILE: Rect = Rect::new(CARD_X, 418, CARD_WIDTH, 16);

/// A card's panel, head, and the buttons in it.
fn card_rects(slot: usize) -> [Rect; 5] {
    let top = CARD_TOP + slot as i32 * CARD_PITCH;
    let x = CARD_X + 4;
    let play = 52;
    [
        Rect::new(CARD_X, top, CARD_WIDTH, CARD_HEIGHT),
        Rect::new(CARD_X, top, CARD_WIDTH, CARD_HEAD),
        Rect::new(x, top + 39, play, 14),
        Rect::new(x + play + 4, top + 39, CARD_INNER - play - 4, 14),
        Rect::new(x, top + 54, CARD_INNER, 14),
    ]
}

/// The first of `count` items to show `shown` of them around `anchor`, with
/// a neighbour of the anchor in view on both sides, so that walking the
/// focus through the items brings each of them into view in turn.
fn sliding_first(count: usize, shown: usize, anchor: usize) -> usize {
    if count <= shown {
        return 0;
    }
    anchor
        .saturating_sub(shown.saturating_sub(2))
        .min(count - shown)
}

fn card_count(launcher: &Launcher) -> usize {
    launcher.config.profile_names().len()
}

/// The first card shown: around the focused card, else the active one.
fn first_card(launcher: &Launcher, count: usize) -> usize {
    let anchor = match launcher.focus {
        Widget::ProfileCard(at)
        | Widget::ProfilePlay(at)
        | Widget::ProfileResume(at)
        | Widget::ProfileAutoplay(at) => at,
        _ => {
            let active = launcher.config.active_profile();
            launcher
                .config
                .profile_names()
                .iter()
                .position(|(name, _)| name == active)
                .unwrap_or(0)
        }
    };
    sliding_first(count, CARDS_SHOWN, anchor)
}

fn sidebar_layout(launcher: &Launcher, out: &mut Vec<(Widget, Rect)>) {
    let count = card_count(launcher);
    let first = first_card(launcher, count);
    for (slot, at) in (first..count).take(CARDS_SHOWN).enumerate() {
        let [_, head, play, autoplay, resume] = card_rects(slot);
        out.extend([
            (Widget::ProfileCard(at), head),
            (Widget::ProfilePlay(at), play),
            (Widget::ProfileAutoplay(at), autoplay),
            (Widget::ProfileResume(at), resume),
        ]);
    }
    out.push((Widget::NewProfile, NEW_PROFILE));
}

// --- The pills -------------------------------------------------------------

const PILLS: [(Tab, &str); 4] = [
    (Tab::Data, "Paths & Mods"),
    (Tab::Display, "Enhanced & Visuals"),
    (Tab::Audio, "Audio & Input"),
    (Tab::Debug, "Debug & Replays"),
];
const PILL_PADDING: i32 = 12;
const PILL_GAP: i32 = 4;

fn pill_rects() -> impl Iterator<Item = (Tab, &'static str, Rect)> {
    let mut x = PANE.x;
    PILLS.into_iter().map(move |(tab, label)| {
        let w = font::width(label) + 2 * PILL_PADDING;
        let rect = Rect::new(x, PILL_TOP, w, PILL_HEIGHT);
        x += w + PILL_GAP;
        (tab, label, rect)
    })
}

// --- The centre pane -------------------------------------------------------

const SECTION: i32 = 12;
const ROW_HEIGHT: i32 = 18;
const ROW_PITCH: i32 = 20;
const BUTTON_HEIGHT: i32 = 16;
const FIELD_PITCH: i32 = 30;
/// A settings row's control, at the row's right.
const CONTROL_WIDTH: i32 = 132;
const FIELD_LABELS: [&str; 3] = ["MASTER.DAT", "CRITTER.DAT", "Data folder"];
const FIELD_PLACEHOLDERS: [&str; 3] = ["path to MASTER.DAT", "path to CRITTER.DAT", "(none)"];

fn field_top(field: Field) -> i32 {
    PANE.y + SECTION + field.index() as i32 * FIELD_PITCH + 10
}
const GAME_TOP: i32 = PANE.y + SECTION + 3 * FIELD_PITCH + 1;
const LOAD_ORDER_TOP: i32 = LAYERS.y - SECTION;
const LAYER_ACTIONS_TOP: i32 = LAYERS.y + LAYERS.h + 3;
const NEW_LAYER_TOP: i32 = LAYER_ACTIONS_TOP + BUTTON_HEIGHT + 3;
const LAYER_NOTES_TOP: i32 = NEW_LAYER_TOP + BUTTON_HEIGHT + 4;

/// The up and down buttons, in the gutter beside the selected layer's row.
fn layer_arrows(launcher: &Launcher) -> [Rect; 2] {
    let list = &launcher.mods.list;
    let line = list.selected.saturating_sub(list.scroll) as i32;
    let y = LAYERS.y + 2 + line.min(widgets::List::rows(LAYERS) as i32 - 1) * widgets::ROW;
    let x = LAYERS.right() + 2;
    [
        Rect::new(x, y, 13, widgets::ROW),
        Rect::new(x + 15, y, 13, widgets::ROW),
    ]
}

fn paths_layout(launcher: &Launcher, out: &mut Vec<(Widget, Rect)>) {
    let w = 48;
    for field in Field::ALL {
        let y = field_top(field);
        let width = PANE.w - w - 4;
        out.push((
            Widget::Field(field),
            Rect::new(PANE.x, y, width, BUTTON_HEIGHT),
        ));
        out.push((
            Widget::Browse(field),
            Rect::new(PANE.right() - w, y, w, BUTTON_HEIGHT),
        ));
    }
    out.push((
        Widget::Game,
        Rect::new(PANE.x + 32, GAME_TOP, 120, BUTTON_HEIGHT),
    ));
    out.push((
        Widget::AutoDetect,
        Rect::new(PANE.right() - 84, GAME_TOP, 84, BUTTON_HEIGHT),
    ));
    let [up, down] = layer_arrows(launcher);
    out.extend([
        (Widget::Layers, LAYERS),
        (Widget::LayerUp, up),
        (Widget::LayerDown, down),
    ]);
    for (x, w, widget) in [
        (PANE.x, 64, Widget::LayerToggle),
        (PANE.x + 68, 64, Widget::LayerRemove),
    ] {
        out.push((widget, Rect::new(x, LAYER_ACTIONS_TOP, w, BUTTON_HEIGHT)));
    }
    let y = NEW_LAYER_TOP;
    out.push((Widget::NewLayer, Rect::new(PANE.x, y, 188, BUTTON_HEIGHT)));
    out.push((
        Widget::AddLayer,
        Rect::new(PANE.x + 192, y, 40, BUTTON_HEIGHT),
    ));
    out.push((
        Widget::BrowseLayer,
        Rect::new(PANE.right() - w, y, w, BUTTON_HEIGHT),
    ));
}

/// The custom presets shown at once: two rows of two.
const CUSTOM_SHOWN: usize = 6;
const PRESET_WIDTH: i32 = 140;
const PRESET_PITCH: i32 = 18;

fn enhanced_shown(launcher: &Launcher) -> bool {
    launcher.active_enhanced().preset == Preset::Enhanced
}

/// The grid cell of a preset button.
fn preset_cell(at: usize, top: i32) -> Rect {
    let column = at as i32 % 2;
    let row = at as i32 / 2;
    Rect::new(
        PANE.x + column * (PRESET_WIDTH + 4),
        top + row * PRESET_PITCH,
        PRESET_WIDTH,
        BUTTON_HEIGHT,
    )
}

const DISPLAY: [Widget; 3] = [Widget::Preset, Widget::Scaling, Widget::Fullscreen];
/// The enhanced preset's rows, shown with it only.
const ENHANCED: [Widget; 9] = [
    Widget::Viewport,
    Widget::Zoom,
    Widget::SmoothCamera,
    Widget::CameraFollow,
    Widget::RecenterMargin,
    Widget::RoofPeek,
    Widget::UiScale,
    Widget::GameSpeed,
    Widget::TacticalOverlay,
];

fn first_custom(launcher: &Launcher) -> usize {
    let count = launcher.config.custom_presets.len();
    let anchor = match launcher.focus {
        Widget::CustomPreset(at) => at,
        _ => 0,
    };
    sliding_first(count, CUSTOM_SHOWN, anchor)
}

/// A column of settings rows from `top`; returns where it ends.
fn rows_layout(widgets: &[Widget], top: i32, out: &mut Vec<(Widget, Rect)>) -> i32 {
    let mut y = top;
    for widget in widgets {
        out.push((*widget, Rect::new(PANE.x, y, PANE.w, ROW_HEIGHT)));
        y += ROW_PITCH;
    }
    y
}

fn display_layout(launcher: &Launcher, out: &mut Vec<(Widget, Rect)>) {
    let top = PANE.y + SECTION;
    let grid = BuiltinPreset::ALL
        .into_iter()
        .map(Widget::BuiltinPreset)
        .chain([Widget::SaveCustomPreset]);
    for (at, widget) in grid.enumerate() {
        out.push((widget, preset_cell(at, top)));
    }
    let customs = launcher.config.custom_presets.len();
    let first = first_custom(launcher);
    let shown = customs.min(CUSTOM_SHOWN);
    for slot in 0..shown {
        out.push((
            Widget::CustomPreset(first + slot),
            preset_cell(slot + 4, top),
        ));
    }
    let y = top + (shown.div_ceil(2) + 2) as i32 * PRESET_PITCH + 3 + SECTION;
    let y = rows_layout(&DISPLAY, y, out);
    if enhanced_shown(launcher) {
        rows_layout(&ENHANCED, y + 3 + SECTION, out);
    }
}

fn audio_layout(out: &mut Vec<(Widget, Rect)>) {
    let y = rows_layout(&[Widget::Audio], PANE.y + SECTION, out);
    rows_layout(
        &[Widget::TouchUi, Widget::HotkeyHints],
        y + 3 + SECTION,
        out,
    );
}

/// The browser's shortcuts: one column, its focused one in view.
const SHORTCUT_TOP: i32 = 76;
const SHORTCUT_PITCH: i32 = 15;
const SHORTCUTS_SHOWN: usize = 21;

fn first_shortcut(launcher: &Launcher, count: usize) -> usize {
    let anchor = match launcher.focus {
        Widget::KnownPath(at) => at,
        _ => 0,
    };
    sliding_first(count, SHORTCUTS_SHOWN, anchor)
}

fn browser_layout(launcher: &Launcher, browser: &Browser) -> Vec<(Widget, Rect)> {
    let mut out = vec![
        (Widget::BrowseList, BROWSE_LIST),
        (Widget::BrowseUse, BROWSE_USE),
        (Widget::BrowseCancel, BROWSE_CANCEL),
    ];
    let count = browser.shortcuts().len();
    let first = first_shortcut(launcher, count);
    for (slot, at) in (first..count).take(SHORTCUTS_SHOWN).enumerate() {
        let y = SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH;
        out.push((Widget::KnownPath(at), Rect::new(32, y, 144, 14)));
    }
    out
}

/// The widgets of the panes beneath any dialog, in focus order.
fn panes(launcher: &Launcher) -> Vec<(Widget, Rect)> {
    let mut out = Vec::with_capacity(72);
    sidebar_layout(launcher, &mut out);
    out.extend(pill_rects().map(|(tab, _, rect)| (Widget::Tab(tab), rect)));
    match launcher.tab {
        Tab::Data | Tab::Mods => paths_layout(launcher, &mut out),
        Tab::Display => display_layout(launcher, &mut out),
        Tab::Audio => audio_layout(&mut out),
        Tab::Debug => {
            rows_layout(&[Widget::Debug], PANE.y + SECTION, &mut out);
        }
    }
    out.extend(BAR.map(|(widget, _, rect)| (widget, rect)));
    out
}

/// The widgets in focus order, with their rectangles. An open browser is
/// modal: only its own widgets are listed.
pub fn layout(launcher: &Launcher) -> Vec<(Widget, Rect)> {
    match &launcher.browser {
        Some(browser) => browser_layout(launcher, browser),
        None => panes(launcher),
    }
}

/// The bottom bar, always shown (inactive while the browser is open).
const BAR_TOP: i32 = 441;
const BAR: [(Widget, &str, Rect); 4] = [
    (Widget::Save, "Save", Rect::new(8, BAR_TOP, 60, 17)),
    (Widget::Play, "Play", Rect::new(390, BAR_TOP, 64, 17)),
    (
        Widget::PlayDebug,
        "Launch with Debug",
        Rect::new(458, BAR_TOP, 114, 17),
    ),
    (Widget::Quit, "Quit", Rect::new(576, BAR_TOP, 56, 17)),
];
const STATUS_TOP: i32 = 461;
const HELP_LINE_TOP: i32 = 471;

// --- Settings --------------------------------------------------------------

/// How a settings row shows its value.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Control {
    Switch(bool),
    Choice(&'static str),
    Slider {
        stop: usize,
        stops: usize,
        value: String,
    },
}

/// The stop nearest `value`.
fn nearest(stops: impl Iterator<Item = f32>, value: f32) -> usize {
    stops
        .enumerate()
        .min_by(|(_, a), (_, b)| (a - value).abs().total_cmp(&(b - value).abs()))
        .map_or(0, |(at, _)| at)
}

/// A setting's row: its label and its value as the launcher holds it.
pub(super) fn setting(launcher: &Launcher, widget: Widget) -> Option<(&'static str, Control)> {
    let config = &launcher.config;
    let active = launcher.active_enhanced();
    let settings = active.settings;
    Some(match widget {
        Widget::Preset => (
            "Preset",
            Control::Choice(match active.preset {
                Preset::Vanilla => "Vanilla",
                Preset::Enhanced => "Enhanced",
            }),
        ),
        Widget::Scaling => (
            "Scaling",
            Control::Choice(match config.scaling.unwrap_or(ScalingMode::AspectFit) {
                ScalingMode::AspectFit => "Aspect fit",
                ScalingMode::Integer => "Integer",
            }),
        ),
        Widget::Fullscreen => (
            "Fullscreen",
            Control::Switch(config.fullscreen.unwrap_or(false)),
        ),
        Widget::Viewport => (
            "Widescreen",
            Control::Switch(settings.viewport == ViewportMode::Unlocked),
        ),
        Widget::Zoom => (
            "Default zoom",
            Control::Slider {
                stop: nearest(ZOOM_STEPS.into_iter(), settings.default_zoom),
                stops: ZOOM_STEPS.len(),
                value: format!("{}x", settings.default_zoom),
            },
        ),
        Widget::SmoothCamera => ("Smooth camera", Control::Switch(settings.smooth_camera)),
        Widget::CameraFollow => (
            "Camera follow",
            Control::Choice(settings.camera_follow.label()),
        ),
        Widget::RecenterMargin => {
            let margin = settings.camera_recenter_margin;
            let margins = MARGIN_STEPS.iter().map(|margin| f32::from(*margin));
            (
                "Recentre margin",
                Control::Slider {
                    stop: nearest(margins, f32::from(margin)),
                    stops: MARGIN_STEPS.len(),
                    value: if margin == 0 {
                        "Off".to_owned()
                    } else {
                        format!("{margin}px")
                    },
                },
            )
        }
        Widget::RoofPeek => (
            "Roof peek",
            Control::Choice(match settings.roof_peek {
                RoofPeek::Vanilla => "Original",
                RoofPeek::CursorXray => "Cursor X-ray",
                RoofPeek::Hidden => "Hidden",
            }),
        ),
        Widget::UiScale => (
            "Interface scale",
            Control::Slider {
                stop: settings.ui_scale.map_or(0, |scale| {
                    1 + nearest(UI_SCALES[1..].iter().flatten().copied(), scale)
                }),
                stops: UI_SCALES.len(),
                value: settings
                    .ui_scale
                    .map_or_else(|| "Auto".to_owned(), |scale| format!("{scale}x")),
            },
        ),
        Widget::GameSpeed => {
            let speed = active.game_speed;
            (
                "Game speed",
                Control::Slider {
                    stop: GameSpeed::ALL
                        .iter()
                        .position(|it| *it == speed)
                        .unwrap_or(0),
                    stops: GameSpeed::ALL.len(),
                    value: speed.label().to_owned(),
                },
            )
        }
        Widget::TacticalOverlay => ("Tactical overlay", Control::Switch(active.tactical_overlay)),
        Widget::Audio => ("Audio", Control::Switch(config.audio.unwrap_or(true))),
        Widget::TouchUi => (
            "Touch UI",
            Control::Switch(config.touch_ui.unwrap_or(false)),
        ),
        Widget::HotkeyHints => (
            "Hotkey hints",
            Control::Choice(match config.hotkey_hints.unwrap_or_default() {
                HotkeyHints::Once => "Once",
                HotkeyHints::Always => "Always",
                HotkeyHints::Off => "Off",
            }),
        ),
        Widget::Debug => (
            "Debug overlay",
            Control::Switch(config.debug.unwrap_or(false)),
        ),
        _ => return None,
    })
}

/// The part of a settings row that holds its control.
fn control_area(row: Rect) -> Rect {
    Rect::new(row.right() - CONTROL_WIDTH - 4, row.y, CONTROL_WIDTH, row.h)
}

/// The slider of a settings row at `row`: its track, left of its value.
pub(super) fn slider_track(row: Rect) -> Rect {
    let area = control_area(row);
    Rect::new(area.x, area.y, area.w - 40, area.h)
}

fn draw_setting(
    image: &mut IndexedImage,
    launcher: &Launcher,
    widget: Widget,
    rect: Rect,
    look: Look,
) {
    let Some((label, control)) = setting(launcher, widget) else {
        return;
    };
    widgets::row(image, rect, look);
    let text_y = rect.y + (rect.h - 7) / 2;
    font::draw(image, [rect.x + 6, text_y], label, color::TEXT);
    let area = control_area(rect);
    match control {
        Control::Switch(on) => widgets::switch(image, area, on),
        Control::Choice(value) => {
            let control = Rect::new(area.x, area.y + 1, area.w, area.h - 2);
            widgets::choice(image, control, value, Look::default());
        }
        Control::Slider { stop, stops, value } => {
            widgets::slider(image, slider_track(rect), stop, stops, Look::default());
            let x = area.right() - font::width(&value) + 1;
            font::draw(image, [x, text_y], &value, color::ACCENT);
        }
    }
}

// --- Descriptions ----------------------------------------------------------

/// What a widget does, in a line or two for the description card.
fn describe(launcher: &Launcher, widget: Widget) -> Option<(&'static str, &'static str)> {
    let config = &launcher.config;
    let active = launcher.active_enhanced();
    let settings = active.settings;
    let text = match widget {
        Widget::Field(Field::Master) => {
            "Holds the maps, scripts and palette. Nothing starts without it."
        }
        Widget::Field(Field::Critter) => "Holds the art for every creature and character.",
        Widget::Field(Field::DataRoot) => {
            "Loose files here are read before the archives, so they win. Optional with both archives set."
        }
        Widget::Browse(Field::DataRoot) => "Picks the loose-files folder from a list of folders.",
        Widget::Browse(_) => {
            "Picks this file from a list of folders. Picking the game folder fills every path."
        }
        Widget::Game => {
            "Reads this game's archive format and rules. Each game keeps its own paths and mods."
        }
        Widget::AutoDetect => {
            "Looks in the folder named above and fills in the paths, Et Tu and its mods included."
        }
        Widget::Preset => match active.preset {
            Preset::Vanilla => "Plays by the original rules, unchanged.",
            Preset::Enhanced => {
                "Adds engine extensions and table overrides, and opens the rows below."
            }
        },
        Widget::Scaling => match config.scaling.unwrap_or(ScalingMode::AspectFit) {
            ScalingMode::AspectFit => "Fills the window at any size and keeps the pixels sharp.",
            ScalingMode::Integer => "Scales by whole numbers only and borders what is left.",
        },
        Widget::Fullscreen => {
            "Covers the screen without a border. F11 or Alt+Enter switches in play."
        }
        Widget::Viewport => match settings.viewport {
            ViewportMode::Fixed => "Keeps the classic 640x480 frame. Alt+V switches in play.",
            ViewportMode::Unlocked => {
                "Spreads the world across the window. Alt+V switches in play."
            }
        },
        Widget::Zoom => "Sets how close maps start. The - and = keys zoom in play.",
        Widget::SmoothCamera => "Glides the camera to where it is going. Off, it snaps there.",
        Widget::CameraFollow => match settings.camera_follow {
            CameraFollow::Off => {
                "Leaves the view still while you walk. Home centers it; Alt+C switches in play."
            }
            CameraFollow::Step => "Recentres the view at every step, as the original does.",
            CameraFollow::Smooth => {
                "Tracks the player all the time. Alt+C switches back to Off in play."
            }
        },
        Widget::RecenterMargin => {
            "With follow Off, recentres when the player comes this close to an edge. 0 never does."
        }
        Widget::RoofPeek => match settings.roof_peek {
            RoofPeek::Vanilla => "Draws roofs as the original does. Hold Alt to peek under them.",
            RoofPeek::CursorXray => "Cuts roofs away around the pointer.",
            RoofPeek::Hidden => "Leaves roofs out altogether. Alt+R cycles the modes.",
        },
        Widget::UiScale => "Sizes the interface. Auto fits it to the window's height.",
        Widget::GameSpeed => "Speeds up world animations and combat. Alt+F cycles in play.",
        Widget::TacticalOverlay => {
            "Shows HP bars, AP badges and the other combat aids from the start of a session."
        }
        Widget::BuiltinPreset(BuiltinPreset::VanillaPurist) => {
            "Sets this profile to the original rules in the classic 640x480 frame."
        }
        Widget::BuiltinPreset(BuiltinPreset::WidescreenClassic) => {
            "Sets this profile to a wide view at 1x zoom, with roofs as in the original."
        }
        Widget::BuiltinPreset(BuiltinPreset::ModernTactical) => {
            "Sets this profile to a wide view with cursor X-ray and the tactical overlay on."
        }
        Widget::SaveCustomPreset => "Keeps the settings below as a preset of your own.",
        Widget::CustomPreset(_) => "Applies this saved preset to the active profile.",
        Widget::Audio => "Opens the sound device. Off starts silent, as --no-audio does.",
        Widget::TouchUi => "Adds tap, drag, pinch and hold gestures and an on-screen button bar.",
        Widget::HotkeyHints => match config.hotkey_hints.unwrap_or_default() {
            HotkeyHints::Once => "Teaches the keys once, then stops.",
            HotkeyHints::Always => "Teaches the keys every session.",
            HotkeyHints::Off => "Shows no key hints.",
        },
        Widget::Debug => {
            "Shows the inspector for scripts, variables and map objects. F9 toggles it."
        }
        Widget::Layers => {
            "Mods load from the top row down, and the last one wins. A box turns a mod on or off."
        }
        Widget::LayerUp => "Moves the selected mod earlier, so later mods win over it.",
        Widget::LayerDown => "Moves the selected mod later, so it wins over earlier mods.",
        Widget::LayerToggle => "Skips the selected mod, or brings it back, without removing it.",
        Widget::LayerRemove => "Takes the selected mod off the list. Its files stay on disk.",
        Widget::NewLayer => "Type a mod folder or .DAT path here, then press Enter.",
        Widget::AddLayer => "Adds the typed path as the last mod, so it wins.",
        Widget::BrowseLayer => "Picks a mod folder or .DAT from a list of folders.",
        Widget::ProfileCard(_) => "Makes this the profile the panes edit.",
        Widget::ProfilePlay(_) => "Saves, then starts this profile.",
        Widget::ProfileResume(_) => "Saves, then starts this profile on its most recent save.",
        Widget::ProfileAutoplay(_) => "Saves, then starts this profile with the bot in Watch Mode.",
        Widget::NewProfile => "Adds an empty profile with its own paths, mods and settings.",
        Widget::Save => "Writes the settings to vault-engine.toml and stays here.",
        Widget::Play => "Saves, then starts the active profile.",
        Widget::PlayDebug => "Saves, then starts the active profile with the debug overlay.",
        Widget::Quit if launcher.from_selector => "Goes back to the game selector without saving.",
        Widget::Quit => "Closes the launcher without saving.",
        _ => return None,
    };
    Some((widget_title(launcher, widget), text))
}

/// The description card's heading for a widget.
fn widget_title(launcher: &Launcher, widget: Widget) -> &'static str {
    if let Some((label, _)) = setting(launcher, widget) {
        return label;
    }
    match widget {
        Widget::Field(field) => FIELD_LABELS[field.index()],
        Widget::Browse(_) | Widget::BrowseLayer => "Browse",
        Widget::Game => "Game",
        Widget::AutoDetect => "Auto-Detect",
        Widget::BuiltinPreset(preset) => preset.label(),
        Widget::SaveCustomPreset => "Save Custom Preset",
        Widget::CustomPreset(_) => "Custom preset",
        Widget::Layers => "Load order",
        Widget::LayerUp => "Move up",
        Widget::LayerDown => "Move down",
        Widget::LayerToggle => "Enable or disable",
        Widget::LayerRemove => "Remove",
        Widget::NewLayer => "New mod",
        Widget::AddLayer => "Add",
        Widget::ProfileCard(_) => "Profile",
        Widget::ProfilePlay(_) | Widget::Play => "Play",
        Widget::ProfileResume(_) => "Resume Last Save",
        Widget::ProfileAutoplay(_) => "Autoplay Test",
        Widget::NewProfile => "New Profile",
        Widget::Save => "Save",
        Widget::PlayDebug => "Launch with Debug",
        Widget::Quit if launcher.from_selector => "Back",
        Widget::Quit => "Quit",
        _ => "",
    }
}

/// The card's text while nothing is pointed at: what the pane is for.
fn tab_note(launcher: &Launcher) -> (&'static str, &'static str) {
    match launcher.tab {
        Tab::Data => (
            "Drop files here",
            "Drop MASTER.DAT, CRITTER.DAT or the game folder on this window to fill the paths.",
        ),
        Tab::Mods => (
            "Drop mods here",
            "Drop a mod folder or .DAT on this window to add it to the load order.",
        ),
        Tab::Display => match launcher.active_enhanced().preset {
            Preset::Vanilla => (
                "Vanilla",
                "The original rules and frame. Choose Enhanced or a preset to open the rest.",
            ),
            Preset::Enhanced => (
                "Enhanced",
                "Point at a setting to see what it does. The preview follows it.",
            ),
        },
        Tab::Audio | Tab::Debug => ("Settings", "Point at a setting to see what it does."),
    }
}

// --- Drawing ---------------------------------------------------------------

fn mark(check: &Check) -> (char, u8) {
    match check.state {
        State::Ok => (CHECK, color::OK),
        State::Bad => (CROSS, color::ERROR),
        State::Unset => ('-', color::DIM),
    }
}

/// A check: its glyph, then its text, cut to `pixels` across.
fn draw_check(image: &mut IndexedImage, origin: [i32; 2], check: &Check, pixels: i32) {
    let (glyph, tone) = mark(check);
    let x = font::draw(image, origin, &glyph.to_string(), tone);
    let text = if check.state == State::Bad {
        color::ERROR
    } else {
        color::DIM
    };
    font::draw(
        image,
        [x + ADVANCE, origin[1]],
        &font::clip_end(&check.text, font::columns(pixels - 2 * ADVANCE)),
        text,
    );
}

/// A section's heading and rule.
fn section(image: &mut IndexedImage, top: i32, title: &str) {
    let end = font::draw(image, [PANE.x, top], title, color::DIM);
    image.fill_rect([end + 2, top + 3, PANE.right() - end - 2, 1], color::BORDER);
}

/// Lines of `text` wrapped to `rect`, as many as fit from its top; returns
/// the y after the last.
fn paragraph(image: &mut IndexedImage, rect: Rect, text: &str, ink: u8) -> i32 {
    let mut y = rect.y;
    for line in font::wrap(text, font::columns(rect.w)) {
        if y + 8 > rect.bottom() {
            break;
        }
        font::draw(image, [rect.x, y], &line, ink);
        y += LINE;
    }
    y
}

const LINE: i32 = 10;

/// Whether the right pane holds the preview, with `shown` the widget it
/// describes.
fn shows_preview(launcher: &Launcher, shown: Option<Widget>) -> bool {
    launcher.tab == Tab::Display || shown.is_some_and(|widget| setting(launcher, widget).is_some())
}

/// Whether the preview is on screen, so its animation asks for frames.
pub fn preview_shown(launcher: &Launcher) -> bool {
    launcher.browser.is_none() && shows_preview(launcher, launcher.hover)
}

pub fn draw(launcher: &Launcher) -> IndexedImage {
    draw_with_cards(launcher, &launcher.profile_cards())
}

fn draw_with_cards(launcher: &Launcher, cards: &[ProfileCard]) -> IndexedImage {
    let mut image = IndexedImage::new(FRAME_WIDTH as u32, FRAME_HEIGHT as u32)
        .expect("the launcher frame size is valid");
    let image_ref = &mut image;
    FRAME.fill(image_ref, color::BACKGROUND);
    let panes_layout = panes(launcher);
    let modal = launcher.browser.is_some();
    let look = |widget: Widget| Look {
        focused: launcher.focus == widget,
        hovered: launcher.hover == Some(widget),
        disabled: false,
    };
    let rect = |widget: Widget| {
        panes_layout
            .iter()
            .find(|(candidate, _)| *candidate == widget)
            .map(|(_, rect)| *rect)
    };
    let active = cards
        .iter()
        .find(|card| card.active)
        .map_or_else(|| title(launcher.game()), |card| card.title.as_str());
    draw_header(image_ref, active);
    draw_sidebar(image_ref, launcher, cards, &look);
    for (tab, label, rect) in pill_rects() {
        let current = launcher.tab == tab || (tab == Tab::Data && launcher.tab == Tab::Mods);
        widgets::pill(image_ref, rect, label, current, look(Widget::Tab(tab)));
    }
    image_ref.fill_rect(
        [SIDEBAR.right(), PANES_TOP - 2, FRAME_WIDTH, 1],
        color::BORDER,
    );
    match launcher.tab {
        Tab::Data | Tab::Mods => draw_paths(image_ref, launcher, &look, &rect),
        Tab::Display => draw_display(image_ref, launcher, &look, &rect),
        Tab::Audio => draw_audio(image_ref, launcher, &look, &rect),
        Tab::Debug => draw_debug(image_ref, launcher, &look, &rect),
    }
    draw_right(image_ref, launcher, active, &panes_layout);
    if let Some(browser) = &launcher.browser {
        dim_behind(image_ref);
        draw_browser(image_ref, launcher, browser);
    }
    draw_footer(image_ref, launcher, &look, modal);
    image
}

fn draw_header(image: &mut IndexedImage, active: &str) {
    Rect::new(0, 0, FRAME_WIDTH, HEADER_HEIGHT).fill(image, color::PANEL);
    image.fill_rect([0, HEADER_HEIGHT - 1, FRAME_WIDTH, 1], color::BORDER);
    let end = font::draw_scaled(image, [10, 4], "VAULT ENGINE", color::ACCENT, 2);
    font::draw(image, [end + 2, 11], "Studio", color::DIM);
    let active = font::clip_end(active, 40);
    let x = FRAME_WIDTH - 10 - font::width(&active) + 1;
    font::draw(image, [x, 9], &active, color::TEXT);
}

/// A card's badges, in the order they flow.
fn badges(card: &ProfileCard) -> Vec<(String, u8, u8)> {
    let mut out = Vec::with_capacity(4);
    out.push(if card.ready {
        ("Ready".to_owned(), color::SELECTED, color::BRIGHT)
    } else {
        ("No Data".to_owned(), color::FIELD, color::WARN)
    });
    if card.sfall {
        out.push(("sfall Active".to_owned(), color::FIELD, color::TEXT));
    }
    if card.mods > 0 {
        let text = if card.mods > 99 {
            "Mods: 99+".to_owned()
        } else {
            format!("Mods: {}", card.mods)
        };
        out.push((text, color::FIELD, color::TEXT));
    }
    if card.last_played {
        out.push(("Last Played".to_owned(), color::FIELD, color::DIM));
    }
    out
}

fn draw_sidebar(
    image: &mut IndexedImage,
    launcher: &Launcher,
    cards: &[ProfileCard],
    look: &LookFn,
) {
    SIDEBAR.fill(image, color::PANEL);
    image.fill_rect(
        [SIDEBAR.right() - 1, SIDEBAR.y, 1, SIDEBAR.h],
        color::BORDER,
    );
    font::draw(image, [CARD_X + 2, 29], "PROFILES", color::DIM);
    let count = card_count(launcher).max(cards.len());
    let first = first_card(launcher, count);
    for (slot, at) in (first..cards.len()).take(CARDS_SHOWN).enumerate() {
        draw_card(image, &cards[at], at, slot, look);
    }
    let shown = CARDS_SHOWN as i32;
    widgets::scrollbar(
        image,
        Rect::new(
            CARD_X + CARD_WIDTH + 3,
            CARD_TOP,
            3,
            shown * CARD_PITCH - (CARD_PITCH - CARD_HEIGHT),
        ),
        first,
        CARDS_SHOWN,
        count,
    );
    widgets::button(image, NEW_PROFILE, "New Profile", look(Widget::NewProfile));
}

fn draw_card(image: &mut IndexedImage, card: &ProfileCard, at: usize, slot: usize, look: &LookFn) {
    let [panel, head, play, autoplay, resume] = card_rects(slot);
    let head_look = look(Widget::ProfileCard(at));
    panel.rounded(
        image,
        if head_look.hovered {
            color::HOVER
        } else {
            color::BUTTON
        },
        2,
    );
    if card.active {
        panel.rounded_outline(image, color::ACCENT, 2);
    }
    if head_look.focused {
        head.rounded_outline(image, color::FOCUS, 2);
    }
    let x = panel.x + 4;
    let ink = if card.active {
        color::BRIGHT
    } else {
        color::TEXT
    };
    let name = font::clip_end(&card.title, font::columns(CARD_INNER));
    font::draw(image, [x, panel.y + 4], &name, ink);
    let (mut pen, mut top) = (x, panel.y + 14);
    for (text, fill, ink) in badges(card) {
        if pen + widgets::badge_width(&text) > x + CARD_INNER {
            pen = x;
            top += BADGE_HEIGHT + 1;
        }
        pen = widgets::badge(image, [pen, top], &text, fill, ink) + 3;
    }
    let playable = |widget: Widget| Look {
        disabled: !card.ready,
        ..look(widget)
    };
    let draw_play = if card.active {
        widgets::primary
    } else {
        widgets::button
    };
    draw_play(image, play, "Play", playable(Widget::ProfilePlay(at)));
    widgets::button(
        image,
        autoplay,
        "Autoplay Test",
        playable(Widget::ProfileAutoplay(at)),
    );
    widgets::button(
        image,
        resume,
        "Resume Last Save",
        playable(Widget::ProfileResume(at)),
    );
}

fn draw_footer(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, modal: bool) {
    Rect::new(0, FOOTER_TOP, FRAME_WIDTH, FRAME_HEIGHT - FOOTER_TOP).fill(image, color::PANEL);
    image.fill_rect([0, FOOTER_TOP, FRAME_WIDTH, 1], color::BORDER);
    let ready = launcher.ready().is_ok();
    for (widget, label, rect) in BAR {
        let playing = matches!(widget, Widget::Play | Widget::PlayDebug);
        let look = Look {
            disabled: modal || (playing && !ready),
            ..look(widget)
        };
        let label = match widget {
            Widget::Quit if launcher.from_selector => "Back",
            _ => label,
        };
        if widget == Widget::Play {
            widgets::primary(image, rect, label, look);
        } else {
            widgets::button(image, rect, label, look);
        }
    }
    let tone = match launcher.status_state {
        State::Ok => color::OK,
        State::Bad => color::ERROR,
        State::Unset => color::TEXT,
    };
    font::draw(
        image,
        [8, STATUS_TOP],
        &font::clip_end(&launcher.status, font::columns(FRAME_WIDTH - 16)),
        tone,
    );
    // SPEC-017 §8.7 rule 6: under Play, the script gate's notice while the
    // gate is closed (Revision 3 opens it).
    let help = if launcher.config.game == Some(GameProfile::Fallout2)
        && !vault_core::fallout2::SCRIPT_GATE_OPEN
    {
        vault_core::fallout2::GATE_NOTICE
    } else {
        "Tab/Shift+Tab move   Enter/Space activate   Arrows choose   Esc backs out"
    };
    font::draw(image, [8, HELP_LINE_TOP], help, color::DIM);
}

/// What the right pane speaks of: the widget under the pointer, else the
/// focused one (not the bottom bar's, where the focus rests at the start).
fn subject(launcher: &Launcher, panes: &[(Widget, Rect)]) -> Option<Widget> {
    let laid_out = |widget: &Widget| panes.iter().any(|(it, _)| it == widget);
    let about = |widget: Widget| describe(launcher, widget).is_some();
    let focus = Some(launcher.focus).filter(|widget| {
        laid_out(widget)
            && !matches!(
                widget,
                Widget::Save | Widget::Play | Widget::PlayDebug | Widget::Quit
            )
    });
    launcher
        .hover
        .filter(|widget| about(*widget))
        .or(focus.filter(|widget| about(*widget)))
}

/// The right pane: the preview and a card on what is pointed at.
fn draw_right(
    image: &mut IndexedImage,
    launcher: &Launcher,
    active: &str,
    panes: &[(Widget, Rect)],
) {
    RIGHT.fill(image, color::PANEL);
    image.fill_rect([RIGHT.x, RIGHT.y - 2, 1, RIGHT.h + 2], color::BORDER);
    let shown = subject(launcher, panes);
    if shows_preview(launcher, shown) {
        launcher
            .preview_canvas()
            .draw(image, [PREVIEW.x, PREVIEW.y, PREVIEW.w, PREVIEW.h]);
        PREVIEW.outline(image, color::BORDER);
    } else {
        draw_summary(image, launcher, active);
    }
    let (heading, text) = shown
        .and_then(|widget| describe(launcher, widget))
        .unwrap_or_else(|| tab_note(launcher));
    font::draw(
        image,
        [DESCRIPTION.x, HELP_TOP],
        &font::clip_end(heading, font::columns(DESCRIPTION.w)),
        color::ACCENT,
    );
    paragraph(image, DESCRIPTION_BODY, text, color::TEXT);
}

/// In the preview's place on the panes it does not answer to: the state of
/// the active profile.
fn draw_summary(image: &mut IndexedImage, launcher: &Launcher, active: &str) {
    let mut y = PREVIEW.y;
    let name = font::clip_end(active, font::columns(PREVIEW.w));
    font::draw(image, [PREVIEW.x, y], &name, color::ACCENT);
    y += 14;
    let area = Rect::new(PREVIEW.x, y, PREVIEW.w, 60);
    y = match launcher.ready() {
        Ok(()) => paragraph(image, area, "Ready to play.", color::OK),
        Err(reason) => paragraph(image, area, &reason, color::WARN),
    } + 4;
    let preset = match launcher.active_enhanced().preset {
        Preset::Vanilla => "Preset: Vanilla",
        Preset::Enhanced => "Preset: Enhanced",
    };
    font::draw(image, [PREVIEW.x, y], preset, color::DIM);
    let enabled = launcher
        .mods
        .entries
        .iter()
        .filter(|entry| entry.enabled)
        .count();
    font::draw(
        image,
        [PREVIEW.x, y + LINE],
        &format!("Mods enabled: {enabled}"),
        color::DIM,
    );
}

type LookFn<'a> = dyn Fn(Widget) -> Look + 'a;
type RectFn<'a> = dyn Fn(Widget) -> Option<Rect> + 'a;

fn draw_rows(
    image: &mut IndexedImage,
    launcher: &Launcher,
    rows: &[Widget],
    look: &LookFn,
    rect: &RectFn,
) {
    for widget in rows {
        if let Some(r) = rect(*widget) {
            draw_setting(image, launcher, *widget, r, look(*widget));
        }
    }
}

fn draw_paths(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    section(image, PANE.y, "GAME DATA");
    for field in Field::ALL {
        let i = field.index();
        let y = field_top(field) - 10;
        let x = font::draw(image, [PANE.x, y], FIELD_LABELS[i], color::TEXT) + ADVANCE;
        draw_check(image, [x, y], &launcher.checks[i], PANE.right() - x);
        if let Some(r) = rect(Widget::Field(field)) {
            widgets::text_field(
                image,
                r,
                &launcher.fields[i],
                FIELD_PLACEHOLDERS[i],
                look(Widget::Field(field)),
            );
        }
        if let Some(r) = rect(Widget::Browse(field)) {
            widgets::button(image, r, "Browse", look(Widget::Browse(field)));
        }
    }
    font::draw(image, [PANE.x, GAME_TOP + 5], "Game", color::TEXT);
    if let Some(r) = rect(Widget::Game) {
        widgets::choice(image, r, title(launcher.game()), look(Widget::Game));
    }
    if let Some(r) = rect(Widget::AutoDetect) {
        widgets::button(image, r, "Auto-Detect", look(Widget::AutoDetect));
    }
    let settings = launcher.settings_path.as_ref().map_or_else(
        || "none yet: Save writes it".to_owned(),
        |path| path.display().to_string(),
    );
    let top = GAME_TOP + BUTTON_HEIGHT + 3;
    let x = font::draw(image, [PANE.x, top], "Settings:", color::DIM) + ADVANCE;
    font::draw(
        image,
        [x, top],
        &font::clip_start(&settings, font::columns(PANE.right() - x)),
        color::DIM,
    );
    draw_load_order(image, launcher, look, rect);
}

/// A mod layer's list text: its path, or with a `vault-mod.toml` the mod's
/// name, an `[enhanced]` badge when it carries launch settings (they apply
/// on Play), and the path.
pub(super) fn layer_name(entry: &LayerEntry, manifest: Option<&ModManifest>) -> String {
    let path = entry
        .path
        .clone()
        .or_else(|| entry.master_dat.clone())
        .or_else(|| entry.critter_dat.clone())
        .unwrap_or_default();
    match manifest {
        Some(manifest) => format!(
            "{}{} ({path})",
            manifest.name,
            if manifest.enhanced.is_some() {
                " [enhanced]"
            } else {
                ""
            }
        ),
        None => path,
    }
}

/// Where a load-order row's note starts, in the list.
const LAYER_NOTE_X: i32 = 150;

fn draw_load_order(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    let mods = &launcher.mods;
    let x = font::draw(image, [PANE.x, LOAD_ORDER_TOP], "LOAD ORDER", color::DIM) + ADVANCE;
    draw_check(
        image,
        [x, LOAD_ORDER_TOP],
        &mods.check.stack,
        PANE.right() - x,
    );
    let name_columns = font::columns(LAYER_NOTE_X - 22);
    let rows: Vec<Row> = mods
        .entries
        .iter()
        .zip(&mods.check.layers)
        .enumerate()
        .map(|(index, (entry, check))| {
            let manifest = mods.check.manifests.get(index).and_then(Option::as_ref);
            let name = layer_name(entry, manifest);
            let (glyph, tone) = mark(check);
            Row {
                // A mod's own name reads from its start, a path from its end.
                text: if manifest.is_some() {
                    font::clip_end(&name, name_columns)
                } else {
                    name
                },
                color: if entry.enabled {
                    color::TEXT
                } else {
                    color::DIM
                },
                check: Some(entry.enabled),
                note: Some((glyph, tone, check.text.clone())),
            }
        })
        .collect();
    if let Some(r) = rect(Widget::Layers) {
        widgets::list(
            image,
            r,
            &mods.list,
            &rows,
            LAYER_NOTE_X,
            look(Widget::Layers),
        );
    }
    let last = mods.entries.len().saturating_sub(1);
    let at = mods.list.selected;
    let selected = mods.entries.get(at);
    let locked = mods.read_only.is_some();
    for (widget, up, stuck) in [
        (Widget::LayerUp, true, at == 0),
        (Widget::LayerDown, false, at >= last),
    ] {
        if let Some(r) = rect(widget) {
            let look = Look {
                disabled: stuck || locked,
                ..look(widget)
            };
            widgets::arrow_button(image, r, up, look);
        }
    }
    let toggle = match selected {
        Some(entry) if !entry.enabled => "Enable",
        _ => "Disable",
    };
    for (widget, label, disabled) in [
        (Widget::LayerToggle, toggle, selected.is_none() || locked),
        (Widget::LayerRemove, "Remove", selected.is_none() || locked),
        (Widget::AddLayer, "Add", locked),
        (Widget::BrowseLayer, "Browse", locked),
    ] {
        if let Some(r) = rect(widget) {
            let look = Look {
                disabled,
                ..look(widget)
            };
            widgets::button(image, r, label, look);
        }
    }
    if let Some(r) = rect(Widget::NewLayer) {
        widgets::text_field(
            image,
            r,
            &mods.new_layer,
            "new mod: folder or .DAT path",
            look(Widget::NewLayer),
        );
    }
    let file = mods.file.as_ref().map_or_else(
        || "none yet: saving creates load_order.toml".to_owned(),
        |path| path.display().to_string(),
    );
    let top = LAYER_NOTES_TOP;
    let x = font::draw(image, [PANE.x, top], "File:", color::DIM) + ADVANCE;
    font::draw(
        image,
        [x, top],
        &font::clip_start(&file, font::columns(PANE.right() - x)),
        color::DIM,
    );
    let (note, ink) = match &mods.read_only {
        Some(reason) => (reason.as_str(), color::WARN),
        None => (
            "Lowest first: the last layer wins. Shift+Up/Down moves, Space toggles, Delete removes.",
            color::DIM,
        ),
    };
    let area = Rect::new(PANE.x, top + LINE, PANE.w, PANE.bottom() - top - LINE);
    paragraph(image, area, note, ink);
}

fn draw_display(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    section(image, PANE.y, "PRESETS");
    let matched = launcher.active_preset();
    for preset in BuiltinPreset::ALL {
        let widget = Widget::BuiltinPreset(preset);
        if let Some(r) = rect(widget) {
            let on = matched == Some(PresetMatch::Builtin(preset));
            widgets::chip(image, r, preset.label(), on, look(widget));
        }
    }
    if let Some(r) = rect(Widget::SaveCustomPreset) {
        widgets::button(
            image,
            r,
            "Save Custom Preset",
            look(Widget::SaveCustomPreset),
        );
    }
    let customs = &launcher.config.custom_presets;
    for (at, preset) in customs.iter().enumerate() {
        let widget = Widget::CustomPreset(at);
        if let Some(r) = rect(widget) {
            let on = matched == Some(PresetMatch::Custom(at));
            widgets::chip(image, r, &preset.name, on, look(widget));
        }
    }
    if customs.len() > CUSTOM_SHOWN {
        let first = first_custom(launcher);
        let text = format!(
            "{}-{} of {}",
            first + 1,
            first + CUSTOM_SHOWN,
            customs.len()
        );
        let x = PANE.right() - font::width(&text);
        Rect::new(x - ADVANCE, PANE.y, font::width(&text) + ADVANCE, 8)
            .fill(image, color::BACKGROUND);
        font::draw(image, [x, PANE.y], &text, color::DIM);
    }
    if let Some(r) = rect(Widget::Preset) {
        section(image, r.y - SECTION, "DISPLAY");
    }
    draw_rows(image, launcher, &DISPLAY, look, rect);
    if let Some(r) = rect(Widget::Viewport) {
        section(image, r.y - SECTION, "ENHANCED");
        draw_rows(image, launcher, &ENHANCED, look, rect);
    } else if let Some(r) = rect(Widget::Fullscreen) {
        let top = r.bottom() + 5;
        section(image, top, "ENHANCED");
        let area = Rect::new(PANE.x + 6, top + SECTION, PANE.w - 12, 40);
        paragraph(
            image,
            area,
            "Widescreen, zoom, camera follow, roof peek, game speed and the tactical overlay \
             open with the Enhanced preset.",
            color::DIM,
        );
    }
}

/// The keys in play, as their rows and the command line describe them.
const KEYS: [(&str, &str); 10] = [
    ("F11, Alt+Enter", "Fullscreen"),
    ("Alt+S", "Scaling"),
    ("Alt+V", "Widescreen (Enhanced)"),
    ("Alt+F", "Game speed (Enhanced)"),
    ("Alt+C", "Camera follow (Enhanced)"),
    ("Home", "Center view"),
    ("Alt+R", "Roof peek (Enhanced)"),
    ("- and =", "Zoom (Enhanced)"),
    ("Alt", "Peek under roofs"),
    ("F9", "Debugger"),
];

fn draw_audio(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    section(image, PANE.y, "AUDIO");
    draw_rows(image, launcher, &[Widget::Audio], look, rect);
    let Some(touch) = rect(Widget::TouchUi) else {
        return;
    };
    section(image, touch.y - SECTION, "INPUT");
    draw_rows(
        image,
        launcher,
        &[Widget::TouchUi, Widget::HotkeyHints],
        look,
        rect,
    );
    let mut y = touch.y + 2 * ROW_PITCH + 3;
    section(image, y, "KEYS IN PLAY");
    y += SECTION;
    for (keys, action) in KEYS {
        font::draw(image, [PANE.x + 6, y], keys, color::TEXT);
        font::draw(image, [PANE.x + 106, y], action, color::DIM);
        y += LINE + 2;
    }
}

fn draw_debug(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    section(image, PANE.y, "DEBUG");
    draw_rows(image, launcher, &[Widget::Debug], look, rect);
    let mut y = PANE.y + SECTION + ROW_PITCH + 3;
    let notes = [
        (
            "BOT",
            "Autoplay Test on a profile card starts it with the bot in Watch Mode. Launch with \
             Debug below starts the active profile with the overlay open.",
        ),
        (
            "REPLAYS",
            "Record a run with --record-inputs FILE and play it back with --replay-inputs FILE. \
             Each is a command-line flag of the game.",
        ),
    ];
    for (heading, text) in notes {
        section(image, y, heading);
        let area = Rect::new(PANE.x + 6, y + SECTION, PANE.w - 12, 60);
        y = paragraph(image, area, text, color::DIM) + 3;
    }
}

/// Everything behind a dialog sinks into the background.
fn dim_behind(image: &mut IndexedImage) {
    let rows = (FOOTER_TOP * FRAME_WIDTH) as usize;
    for pixel in &mut image.pixels_mut()[..rows] {
        *pixel = match *pixel {
            color::BACKGROUND | color::PANEL | color::FIELD | color::BUTTON => color::FIELD,
            _ => color::BACKGROUND,
        };
    }
}

fn draw_browser(image: &mut IndexedImage, launcher: &Launcher, browser: &Browser) {
    let look = |widget: Widget| Look {
        focused: launcher.focus == widget,
        hovered: launcher.hover == Some(widget),
        disabled: false,
    };
    DIALOG.rounded(image, color::PANEL, 2);
    DIALOG.rounded_outline(image, color::ACCENT, 2);
    let title = match browser.target {
        Target::Field(Field::Master) => "Choose MASTER.DAT (or the game folder)",
        Target::Field(Field::Critter) => "Choose CRITTER.DAT (or the game folder)",
        Target::Field(Field::DataRoot) => "Choose the data folder",
        Target::Layer => "Choose a mod layer: a folder or a .DAT",
    };
    let text = DIALOG.inset(12);
    font::draw(image, [text.x, DIALOG.y + 8], title, color::ACCENT);
    let dir = browser.dir.display().to_string();
    font::draw(
        image,
        [text.x, DIALOG.y + 20],
        &font::clip_start(&dir, font::columns(text.w)),
        color::TEXT,
    );
    font::draw(image, [text.x, BROWSE_LIST.y + 2], "PLACES", color::DIM);
    let shortcuts = browser.shortcuts();
    let first = first_shortcut(launcher, shortcuts.len());
    for (slot, at) in (first..shortcuts.len()).take(SHORTCUTS_SHOWN).enumerate() {
        let y = SHORTCUT_TOP + slot as i32 * SHORTCUT_PITCH;
        let rect = Rect::new(text.x, y, 144, 14);
        let shortcut = &shortcuts[at];
        let label = font::clip_end(&shortcut.label, font::columns(rect.w - 12));
        widgets::row(image, rect, look(Widget::KnownPath(at)));
        if shortcut.pinned {
            image.fill_rect([rect.x + 2, rect.y + 5, 3, 3], color::ACCENT);
        }
        font::draw(image, [rect.x + 8, rect.y + 3], &label, color::TEXT);
    }
    if shortcuts.is_empty() {
        font::draw(image, [text.x, SHORTCUT_TOP], "None yet", color::DIM);
    }
    let rows: Vec<Row> = browser
        .entries
        .iter()
        .map(|entry| {
            let dat = entry.name.to_ascii_lowercase().ends_with(".dat");
            Row {
                text: if entry.dir {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                },
                color: if entry.dir {
                    color::ACCENT
                } else if dat {
                    color::BRIGHT
                } else {
                    color::TEXT
                },
                check: None,
                note: None,
            }
        })
        .collect();
    widgets::list(
        image,
        BROWSE_LIST,
        &browser.list,
        &rows,
        BROWSE_LIST.w,
        look(Widget::BrowseList),
    );
    for (widget, label, rect) in [
        (Widget::BrowseUse, "Use this folder", BROWSE_USE),
        (Widget::BrowseCancel, "Cancel", BROWSE_CANCEL),
    ] {
        widgets::button(image, rect, label, look(widget));
    }
    let hint = browser
        .error
        .as_deref()
        .unwrap_or("Enter opens, Backspace goes up, Esc cancels");
    let tone = if browser.error.is_some() {
        color::ERROR
    } else {
        color::DIM
    };
    font::draw(
        image,
        [text.x, BROWSE_USE.y + 6],
        &font::clip_end(hint, font::columns(BROWSE_USE.x - 8 - text.x)),
        tone,
    );
}

#[cfg(test)]
mod tests {
    use super::super::Setup;
    use super::super::browse::Shortcut;
    use super::*;
    use crate::config::{Config, EnhancedProfile};
    use std::path::PathBuf;

    fn launcher_with(config: Config) -> Launcher {
        Launcher::new(Setup {
            config,
            settings_path: None,
            save_dirs: Vec::new(),
            start_dir: PathBuf::from("/nonexistent"),
            problem: None,
            data_dirs: Default::default(),
        })
    }

    /// A config with `profiles` custom profiles and `presets` custom presets,
    /// each with the longest name it can have.
    fn crowded(profiles: usize, presets: usize) -> Config {
        let mut config = Config {
            preset: Some(Preset::Enhanced),
            ..Config::default()
        };
        for at in 0..profiles {
            let name = format!("{at:02}-a-very-long-profile-name-indeed-it-goes-on");
            config
                .add_custom_profile(&name, GameProfile::Fallout2)
                .unwrap();
        }
        for at in 0..presets {
            let name = format!("{at:02}-a-very-long-custom-preset-name-indeed");
            config
                .save_custom_preset(&name, EnhancedProfile::default())
                .unwrap();
        }
        config
    }

    const TABS: [Tab; 5] = [Tab::Data, Tab::Mods, Tab::Display, Tab::Audio, Tab::Debug];

    /// Every state a pane's layout depends on.
    fn states() -> Vec<Launcher> {
        let mut out = Vec::new();
        for (profiles, presets, preset) in [
            (0, 0, Preset::Vanilla),
            (0, 0, Preset::Enhanced),
            (2, 2, Preset::Enhanced),
            (3, 5, Preset::Enhanced),
            (40, 40, Preset::Enhanced),
        ] {
            for tab in TABS {
                let mut launcher = launcher_with(Config {
                    preset: Some(preset),
                    ..crowded(profiles, presets)
                });
                launcher.tab = tab;
                out.push(launcher);
            }
        }
        out
    }

    fn with_browser(shortcuts: usize) -> Launcher {
        let mut launcher = launcher_with(Config::default());
        let mut browser = Browser::open(PathBuf::from("/"), Target::Layer);
        browser.set_shortcuts(
            (0..shortcuts)
                .map(|at| Shortcut {
                    label: format!("{at:02} a very long shortcut label for a folder"),
                    path: PathBuf::from("/"),
                    pinned: at % 2 == 0,
                })
                .collect(),
        );
        launcher.browser = Some(browser);
        launcher
    }

    fn card(at: usize, ready: bool) -> ProfileCard {
        ProfileCard {
            name: format!("profile-{at}"),
            title: format!("{at:02} A Profile With A Title Far Too Long For Any Card"),
            edition: GameProfile::Fallout2,
            active: at == 0,
            ready,
            sfall: true,
            mods: 12_345,
            last_played: true,
        }
    }

    #[test]
    fn every_widget_is_inside_the_frame_and_none_overlap() {
        let mut launchers = states();
        launchers.push(with_browser(0));
        launchers.push(with_browser(40));
        for launcher in &launchers {
            let layout = layout(launcher);
            for (at, (widget, rect)) in layout.iter().enumerate() {
                assert!(rect.within(FRAME), "{widget:?} {rect:?}");
                assert!(rect.w > 0 && rect.h > 0, "{widget:?} {rect:?}");
                for (other, other_rect) in &layout[at + 1..] {
                    assert!(
                        !rect.overlaps(*other_rect),
                        "{widget:?} {rect:?} overlaps {other:?} {other_rect:?}"
                    );
                    assert_ne!(widget, other);
                }
            }
        }
    }

    #[test]
    fn the_centre_pane_stays_in_its_pane() {
        for launcher in &states() {
            for (widget, rect) in layout(launcher) {
                let pane = !matches!(
                    widget,
                    Widget::Tab(_)
                        | Widget::Save
                        | Widget::Play
                        | Widget::PlayDebug
                        | Widget::Quit
                        | Widget::NewProfile
                        | Widget::ProfileCard(_)
                        | Widget::ProfilePlay(_)
                        | Widget::ProfileResume(_)
                        | Widget::ProfileAutoplay(_)
                );
                if pane {
                    assert!(rect.within(PANE), "{widget:?} {rect:?}");
                }
            }
        }
    }

    #[test]
    fn hit_testing_finds_each_widget() {
        let mut launchers = states();
        launchers.push(with_browser(40));
        for launcher in &launchers {
            for (widget, rect) in layout(launcher) {
                for point in [
                    [rect.x, rect.y],
                    [rect.x + rect.w / 2, rect.y + rect.h / 2],
                    [rect.right() - 1, rect.bottom() - 1],
                ] {
                    assert_eq!(launcher.hit(point), Some(widget), "{point:?}");
                }
            }
        }
    }

    #[test]
    fn the_pill_row_fits_and_merges_paths_and_mods() {
        let rects: Vec<_> = pill_rects().collect();
        assert_eq!(rects.len(), 4);
        for (_, label, rect) in &rects {
            assert!(font::width(label) + 8 <= rect.w, "{label}");
        }
        assert!(rects[3].2.right() <= FRAME_WIDTH - 8);
        let mut data = launcher_with(Config::default());
        let mut mods = launcher_with(Config::default());
        data.tab = Tab::Data;
        mods.tab = Tab::Mods;
        assert_eq!(layout(&data), layout(&mods));
    }

    /// The fixed labels at the widths they are drawn in.
    #[test]
    fn text_fits_its_rect_at_the_longest_labels() {
        let fits = |label: &str, rect: Rect| {
            assert!(font::width(label) + 2 <= rect.w, "{label} in {rect:?}");
        };
        for (_, label, rect) in BAR {
            fits(label, rect);
        }
        fits("Back", BAR[3].2);
        let [_, _, play, autoplay, resume] = card_rects(0);
        fits("Play", play);
        fits("Autoplay Test", autoplay);
        fits("Resume Last Save", resume);
        fits("New Profile", NEW_PROFILE);
        let launcher = launcher_with(Config::default());
        let of = |launcher: &Launcher, widget| {
            layout(launcher)
                .into_iter()
                .find(|(candidate, _)| *candidate == widget)
                .unwrap()
                .1
        };
        for (widget, label) in [
            (Widget::Browse(Field::Master), "Browse"),
            (Widget::AutoDetect, "Auto-Detect"),
            (Widget::LayerToggle, "Disable"),
            (Widget::LayerRemove, "Remove"),
            (Widget::AddLayer, "Add"),
            (Widget::BrowseLayer, "Browse"),
            (Widget::Game, "< Fallout 1 >"),
        ] {
            fits(label, of(&launcher, widget));
        }
        let mut display = launcher_with(crowded(0, 1));
        display.tab = Tab::Display;
        for preset in BuiltinPreset::ALL {
            fits(preset.label(), of(&display, Widget::BuiltinPreset(preset)));
        }
        fits("Save Custom Preset", of(&display, Widget::SaveCustomPreset));
        // A row's label stops before its control; a choice's longest value
        // sits between its arrows; a slider's between its track and the edge.
        let row = of(&display, Widget::Zoom);
        let label_end = control_area(row).x - 4;
        for launcher in &states() {
            for (widget, rect) in layout(launcher) {
                if let Some((label, control)) = setting(launcher, widget) {
                    assert!(rect.x + 6 + font::width(label) <= label_end, "{label}");
                    match control {
                        Control::Slider { value, .. } => assert!(font::width(&value) <= 36),
                        Control::Choice(value) => {
                            assert!(font::width(value) + 22 <= CONTROL_WIDTH);
                        }
                        Control::Switch(_) => {}
                    }
                }
            }
        }
        for (keys, action) in KEYS {
            assert!(font::width(keys) <= 96, "{keys}");
            assert!(
                PANE.x + 106 + font::width(action) <= PANE.right(),
                "{action}"
            );
        }
    }

    /// A card's badges flow in two rows at most, at the most a card can show.
    #[test]
    fn a_full_cards_badges_stay_on_the_card() {
        for ready in [true, false] {
            let mut pen = 0;
            let mut rows = 1;
            for (text, ..) in badges(&card(0, ready)) {
                let width = widgets::badge_width(&text);
                assert!(width <= CARD_INNER, "{text}");
                if pen + width > CARD_INNER {
                    pen = 0;
                    rows += 1;
                }
                pen += width + 3;
            }
            assert!(rows <= 2, "{rows}");
        }
    }

    /// Many cards: the sidebar shows what fits, draws nothing past its edge,
    /// and Tab walks every card into view and back.
    #[test]
    fn many_profile_cards_do_not_spill_and_all_can_be_reached() {
        let config = crowded(40, 0);
        let count = config.profile_names().len();
        let mut launcher = launcher_with(config);
        let cards: Vec<_> = (0..count).map(|at| card(at, at % 3 != 0)).collect();
        let few: Vec<_> = cards[..2].to_vec();
        let many_image = draw_with_cards(&launcher, &cards);
        let few_image = draw_with_cards(&launcher, &few);
        let width = FRAME_WIDTH as usize;
        let spilled = |image: &IndexedImage| -> Vec<u8> {
            image
                .pixels()
                .chunks(width)
                .flat_map(|row| row[SIDEBAR.right() as usize..].to_vec())
                .collect()
        };
        assert_eq!(spilled(&many_image), spilled(&few_image));
        for (widget, rect) in layout(&launcher) {
            if matches!(widget, Widget::ProfileCard(_) | Widget::ProfilePlay(_)) {
                assert!(rect.bottom() <= NEW_PROFILE.y, "{widget:?} {rect:?}");
            }
        }
        launcher.focus = Widget::Tab(Tab::Data);
        // Into the sidebar from its top, then every card in turn.
        launcher.focus = Widget::ProfileCard(0);
        let mut seen = vec![false; count];
        for _ in 0..4 * count {
            if let Widget::ProfileCard(at) = launcher.focus {
                seen[at] = true;
            }
            launcher.move_focus(1);
            assert!(
                layout(&launcher)
                    .iter()
                    .any(|(widget, _)| *widget == launcher.focus),
                "the focus stays in view"
            );
        }
        assert!(seen.iter().all(|seen| *seen), "{seen:?}");
        launcher.focus = Widget::ProfileCard(count - 1);
        let mut seen = vec![false; count];
        for _ in 0..4 * count {
            if let Widget::ProfileCard(at) = launcher.focus {
                seen[at] = true;
            }
            launcher.move_focus(-1);
        }
        assert!(seen.iter().all(|seen| *seen), "{seen:?}");
    }

    #[test]
    fn sliding_keeps_the_anchors_neighbours_in_view() {
        assert_eq!(sliding_first(3, 5, 2), 0);
        for count in 6..20 {
            for anchor in 0..count {
                let first = sliding_first(count, 5, anchor);
                assert!(first + 5 <= count);
                assert!((first..first + 5).contains(&anchor));
                if anchor + 1 < count {
                    assert!(anchor + 1 < first + 5, "{count} {anchor}");
                }
            }
        }
    }

    #[test]
    fn hidden_custom_presets_come_into_view_with_the_focus() {
        let mut launcher = launcher_with(crowded(0, 20));
        launcher.tab = Tab::Display;
        launcher.focus = Widget::CustomPreset(0);
        let mut seen = vec![false; 20];
        for _ in 0..200 {
            if let Widget::CustomPreset(at) = launcher.focus {
                seen[at] = true;
            }
            launcher.move_focus(1);
        }
        assert!(seen.iter().all(|seen| *seen), "{seen:?}");
    }

    /// Every widget a setting's value can show, at each value.
    fn every_value() -> Vec<Launcher> {
        let mut out = Vec::new();
        for preset in [Preset::Vanilla, Preset::Enhanced] {
            for scaling in [ScalingMode::AspectFit, ScalingMode::Integer] {
                for viewport in [ViewportMode::Fixed, ViewportMode::Unlocked] {
                    for roof_peek in [RoofPeek::Vanilla, RoofPeek::CursorXray, RoofPeek::Hidden] {
                        for hints in HotkeyHints::ALL {
                            out.push(launcher_with(Config {
                                preset: Some(preset),
                                scaling: Some(scaling),
                                viewport_mode: Some(viewport),
                                roof_peek: Some(roof_peek),
                                hotkey_hints: Some(hints),
                                ..Config::default()
                            }));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn every_setting_has_a_description_that_fits_its_card() {
        let lines = DESCRIPTION_BODY.h / LINE;
        let columns = font::columns(DESCRIPTION_BODY.w);
        for launcher in &mut every_value() {
            launcher.config.custom_presets = crowded(0, 1).custom_presets;
            for tab in TABS {
                launcher.tab = tab;
                for (widget, _) in layout(launcher) {
                    if matches!(widget, Widget::Tab(_)) {
                        continue;
                    }
                    let (title, text) = describe(launcher, widget)
                        .unwrap_or_else(|| panic!("{widget:?} has no description"));
                    assert!(!title.is_empty(), "{widget:?}");
                    assert!(title.len() <= columns, "{title}");
                    assert!(text.len() <= 100, "{text}");
                    assert!(font::wrap(text, columns).len() as i32 <= lines, "{text}");
                }
                let (title, text) = tab_note(launcher);
                assert!(title.len() <= columns, "{title}");
                assert!(font::wrap(text, columns).len() as i32 <= lines);
            }
        }
    }

    /// The right pane shows what the pointer is on, and the preview on the
    /// Enhanced tab or over a setting.
    #[test]
    fn the_right_pane_follows_the_pointer() {
        let right = |launcher: &Launcher| -> Vec<u8> {
            let image = launcher.image();
            image
                .pixels()
                .chunks(FRAME_WIDTH as usize)
                .skip(RIGHT.y as usize)
                .take(RIGHT.h as usize)
                .flat_map(|row| row[RIGHT.x as usize..].to_vec())
                .collect()
        };
        let mut launcher = launcher_with(crowded(0, 0));
        launcher.tab = Tab::Display;
        launcher.focus = Widget::Save;
        launcher.hover = None;
        let quiet = right(&launcher);
        launcher.hover = Some(Widget::Zoom);
        assert_ne!(right(&launcher), quiet);
        launcher.hover = Some(Widget::GameSpeed);
        let speed = right(&launcher);
        launcher.hover = Some(Widget::Zoom);
        assert_ne!(right(&launcher), speed);
    }

    #[test]
    fn a_dialog_blocks_the_panes_beneath_it() {
        let mut launcher = with_browser(3);
        let widgets = layout(&launcher);
        assert!(widgets.iter().all(|(widget, _)| matches!(
            widget,
            Widget::BrowseList | Widget::BrowseUse | Widget::BrowseCancel | Widget::KnownPath(_)
        )));
        assert_eq!(widgets.len(), 6);
        // A press beside the dialog lands on nothing.
        assert_eq!(launcher.hit([4, 200]), None);
        assert_eq!(
            launcher.handle(crate::launcher::LauncherInput::Press([4, 200])),
            None
        );
        let play = BAR[1].2;
        assert_eq!(launcher.hit([play.x + 4, play.y + 4]), None);
        // Drawn in the palette, with the status line still readable.
        launcher.status = "A status".into();
        let image = launcher.image();
        assert!(image.pixels().iter().all(|p| *p <= color::FIELD));
    }

    #[test]
    fn every_screen_stays_in_the_palette() {
        assert_eq!(palette()[usize::from(color::FIELD) + 1], [0, 0, 0, 255]);
        for launcher in &states() {
            let image = draw_with_cards(launcher, &[card(0, true), card(1, false)]);
            assert_eq!(
                (image.width() as i32, image.height() as i32),
                (FRAME_WIDTH, FRAME_HEIGHT)
            );
            assert!(image.pixels().iter().all(|p| *p <= color::FIELD));
        }
    }

    #[test]
    fn a_slider_track_is_in_its_row_clear_of_its_value() {
        let row = Rect::new(PANE.x, 100, PANE.w, ROW_HEIGHT);
        let track = slider_track(row);
        assert!(track.within(row));
        assert!(track.right() + 4 <= control_area(row).right() - 36);
        let zoom = |launcher: &Launcher| match setting(launcher, Widget::Zoom) {
            Some((_, Control::Slider { stop, .. })) => stop,
            _ => panic!("the zoom is a slider"),
        };
        for (at, step) in ZOOM_STEPS.into_iter().enumerate() {
            let launcher = launcher_with(Config {
                default_zoom: Some(step),
                ..Config::default()
            });
            assert_eq!(zoom(&launcher), at);
        }
        // A value from the file between two stops shows at the nearest.
        let launcher = launcher_with(Config {
            default_zoom: Some(1.1),
            ..Config::default()
        });
        assert_eq!(zoom(&launcher), 2);
        assert_eq!(
            widgets::slider_stop(track, track.right() - 1, UI_SCALES.len()),
            UI_SCALES.len() - 1
        );
    }
}
