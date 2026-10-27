//! The launcher's 640 x 480 layout and drawing, in its own palette: a dark
//! terminal look. [`layout`] lists the widgets in focus order with their
//! rectangles, so clicks, focus and drawing agree.
use super::browse::Target;
use super::font::{self, CHECK, CROSS};
use super::validate::{Check, State};
use super::widgets::{self, Look, Rect, Row};
use super::{Field, Launcher, Tab, Widget};
use crate::config::{Preset, game_name, roof_peek_name, scaling_name};
use crate::enhanced::{RoofPeek, ViewportMode};
use vault_core::GameProfile;
use vault_formats::{LayerEntry, ModManifest};
use vault_render::{IndexedImage, ScalingMode};

/// The launcher palette's entries.
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
    pub const FIELD: u8 = 12;
}

/// The display palette (RGBA): green phosphor on near black, amber focus.
pub fn palette() -> [[u8; 4]; 256] {
    const COLORS: [[u8; 3]; 13] = [
        [0x0A, 0x0F, 0x0C],
        [0x10, 0x18, 0x13],
        [0x2A, 0x48, 0x33],
        [0xB8, 0xE0, 0xC0],
        [0x6A, 0x8A, 0x72],
        [0x5C, 0xF0, 0x8A],
        [0xFF, 0xC0, 0x40],
        [0xFF, 0x5F, 0x52],
        [0x52, 0xE0, 0x72],
        [0x1A, 0x2C, 0x21],
        [0x26, 0x40, 0x30],
        [0x22, 0x4A, 0x30],
        [0x06, 0x0B, 0x08],
    ];
    let mut palette = [[0, 0, 0, 255]; 256];
    for (entry, [r, g, b]) in palette.iter_mut().zip(COLORS) {
        *entry = [r, g, b, 255];
    }
    palette
}

/// The mod layer list and the browser's entry list.
pub const LAYERS: Rect = Rect::new(28, 100, 584, 172);
pub const BROWSE_LIST: Rect = Rect::new(28, 100, 584, 268);
const PANEL: Rect = Rect::new(8, 60, 624, 344);

const TAB_LABELS: [&str; 3] = ["Game data", "Display & audio", "Mods"];
const FIELD_LABELS: [&str; 3] = ["MASTER.DAT", "CRITTER.DAT", "Data folder"];
const FIELD_HINTS: [&str; 3] = [
    "the master database: palette, scripts, maps",
    "the critter database: critter art",
    "loose files read before the archives (optional with them)",
];
const FIELD_PLACEHOLDERS: [&str; 3] = ["path to MASTER.DAT", "path to CRITTER.DAT", "(none)"];

/// The Game row, below the fields' checks.
const GAME_TOP: i32 = 302;

fn field_top(field: Field) -> i32 {
    72 + field.index() as i32 * 84
}
/// The Display & audio tab: two columns of evenly spaced rows under their
/// titles (the general rows on the left, the enhanced preset's on the
/// right), then one help line for the focused or hovered row, and the note.
const COLUMN_TITLE_TOP: i32 = 72;
const COLUMNS: [(i32, i32, i32); 2] = [
    // Label x, toggle x and toggle width.
    (28, 136, 150),
    (330, 440, 172),
];
const HELP_TOP: i32 = 330;
const NOTE_TOP: i32 = 380;
fn display_top(row: i32) -> i32 {
    96 + row * 36
}
fn display_rect(column: usize, row: i32) -> Rect {
    let (_, x, w) = COLUMNS[column];
    Rect::new(x, display_top(row), w, 18)
}
const DISPLAY: [Widget; 6] = [
    Widget::Scaling,
    Widget::Fullscreen,
    Widget::TouchUi,
    Widget::Preset,
    Widget::Audio,
    Widget::Debug,
];
/// The enhanced preset's rows, in the right-hand column, shown with it only.
const ENHANCED: [Widget; 6] = [
    Widget::Viewport,
    Widget::Zoom,
    Widget::SmoothCamera,
    Widget::RoofPeek,
    Widget::UiScale,
    Widget::GameSpeed,
];
fn enhanced_shown(launcher: &Launcher) -> bool {
    launcher.config.preset == Some(Preset::Enhanced)
}

/// The widgets in focus order, with their rectangles. An open browser is
/// modal: only its own widgets are listed.
pub fn layout(launcher: &Launcher) -> Vec<(Widget, Rect)> {
    if launcher.browser.is_some() {
        return vec![
            (Widget::BrowseList, BROWSE_LIST),
            (Widget::BrowseUse, Rect::new(28, 376, 140, 20)),
            (Widget::BrowseCancel, Rect::new(176, 376, 80, 20)),
        ];
    }
    let mut out = Vec::new();
    let mut x = 16;
    for (tab, label) in Tab::ALL.into_iter().zip(TAB_LABELS) {
        let w = font::width(label) + 24;
        out.push((Widget::Tab(tab), Rect::new(x, 36, w, 20)));
        x += w + 4;
    }
    match launcher.tab {
        Tab::Data => {
            for field in Field::ALL {
                let y = field_top(field) + 14;
                out.push((Widget::Field(field), Rect::new(28, y, 500, 18)));
                out.push((Widget::Browse(field), Rect::new(536, y, 76, 18)));
            }
            out.push((Widget::Game, Rect::new(160, GAME_TOP, 150, 18)));
        }
        Tab::Display => {
            for (row, widget) in (0..).zip(DISPLAY) {
                out.push((widget, display_rect(0, row)));
            }
            if enhanced_shown(launcher) {
                for (row, widget) in (0..).zip(ENHANCED) {
                    out.push((widget, display_rect(1, row)));
                }
            }
        }
        Tab::Mods => {
            out.push((Widget::Layers, LAYERS));
            for (x, w, widget) in [
                (28, 80, Widget::LayerUp),
                (116, 80, Widget::LayerDown),
                (204, 116, Widget::LayerToggle),
                (328, 80, Widget::LayerRemove),
            ] {
                out.push((widget, Rect::new(x, 280, w, 18)));
            }
            out.push((Widget::NewLayer, Rect::new(100, 314, 340, 18)));
            out.push((Widget::AddLayer, Rect::new(448, 314, 76, 18)));
            out.push((Widget::BrowseLayer, Rect::new(532, 314, 80, 18)));
        }
    }
    out.extend(BAR.map(|(widget, _, rect)| (widget, rect)));
    out
}

/// The bottom bar, always shown (inactive while the browser is open).
const BAR: [(Widget, &str, Rect); 4] = [
    (Widget::Save, "Save", Rect::new(16, 412, 80, 22)),
    (Widget::Play, "Play", Rect::new(312, 412, 100, 22)),
    (
        Widget::PlayDebug,
        "Launch with Debug",
        Rect::new(418, 412, 136, 22),
    ),
    (Widget::Quit, "Quit", Rect::new(560, 412, 64, 22)),
];

fn mark(check: &Check) -> (char, u8) {
    match check.state {
        State::Ok => (CHECK, color::OK),
        State::Bad => (CROSS, color::ERROR),
        State::Unset => ('-', color::DIM),
    }
}

/// A check: its glyph, then its text, cut to `columns`.
fn draw_check(image: &mut IndexedImage, origin: [i32; 2], check: &Check, columns: usize) {
    let (glyph, tone) = mark(check);
    let x = font::draw(image, origin, &glyph.to_string(), tone);
    let text = if check.state == State::Bad {
        color::ERROR
    } else {
        color::DIM
    };
    font::draw(
        image,
        [x + font::ADVANCE, origin[1]],
        &font::clip_end(&check.text, columns),
        text,
    );
}

fn on_off(value: bool) -> &'static str {
    if value { "[x] on" } else { "[ ] off" }
}

pub fn draw(launcher: &Launcher) -> IndexedImage {
    let mut image = IndexedImage::new(640, 480).expect("the launcher frame size is valid");
    let image_ref = &mut image;
    image_ref.fill_rect([0, 0, 640, 480], color::BACKGROUND);
    font::draw_scaled(image_ref, [16, 10], "VAULT ENGINE", color::ACCENT, 2);
    font::draw(image_ref, [170, 17], "launcher", color::DIM);
    image_ref.fill_rect([8, 30, 624, 1], color::BORDER);
    image_ref.fill_rect([PANEL.x, PANEL.y, PANEL.w, PANEL.h], color::PANEL);
    PANEL.outline(image_ref, color::BORDER);
    let layout = layout(launcher);
    let look = |widget: Widget| Look {
        focused: launcher.focus == widget,
        hovered: launcher.hover == Some(widget),
        disabled: false,
    };
    let rect = |widget: Widget| {
        layout
            .iter()
            .find(|(candidate, _)| *candidate == widget)
            .map(|(_, rect)| *rect)
    };
    if let Some(browser) = &launcher.browser {
        draw_browser(image_ref, browser, look(Widget::BrowseList));
        for (widget, label) in [
            (Widget::BrowseUse, "Use this folder"),
            (Widget::BrowseCancel, "Cancel"),
        ] {
            if let Some(rect) = rect(widget) {
                widgets::button(image_ref, rect, label, look(widget));
            }
        }
    } else {
        for (tab, label) in Tab::ALL.into_iter().zip(TAB_LABELS) {
            let widget = Widget::Tab(tab);
            let Some(rect) = rect(widget) else { continue };
            let current = launcher.tab == tab;
            image_ref.fill_rect(
                [rect.x, rect.y, rect.w, rect.h],
                if current { color::PANEL } else { color::BUTTON },
            );
            rect.outline(
                image_ref,
                if look(widget).focused {
                    color::FOCUS
                } else {
                    color::BORDER
                },
            );
            let tone = if current { color::ACCENT } else { color::TEXT };
            font::draw(image_ref, [rect.x + 12, rect.y + 6], label, tone);
        }
        match launcher.tab {
            Tab::Data => draw_data(image_ref, launcher, &look, &rect),
            Tab::Display => draw_display(image_ref, launcher, &look, &rect),
            Tab::Mods => draw_mods(image_ref, launcher, &look, &rect),
        }
    }
    let ready = launcher.ready().is_ok();
    for (widget, label, rect) in BAR {
        let modal = launcher.browser.is_some();
        let playing = matches!(widget, Widget::Play | Widget::PlayDebug);
        let look = Look {
            disabled: modal || (playing && !ready),
            ..look(widget)
        };
        widgets::button(image_ref, rect, label, look);
    }
    let tone = match launcher.status_state {
        State::Ok => color::OK,
        State::Bad => color::ERROR,
        State::Unset => color::TEXT,
    };
    font::draw(
        image_ref,
        [16, 444],
        &font::clip_end(&launcher.status, 104),
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
    font::draw(image_ref, [16, 462], help, color::DIM);
    image
}

type LookFn<'a> = dyn Fn(Widget) -> Look + 'a;
type RectFn<'a> = dyn Fn(Widget) -> Option<Rect> + 'a;

fn draw_data(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    for field in Field::ALL {
        let i = field.index();
        let y = field_top(field);
        let x = font::draw(image, [28, y], FIELD_LABELS[i], color::ACCENT);
        font::draw(image, [x + 12, y], FIELD_HINTS[i], color::DIM);
        if let Some(r) = rect(Widget::Field(field)) {
            let placeholder = FIELD_PLACEHOLDERS[i];
            widgets::text_field(
                image,
                r,
                &launcher.fields[i],
                placeholder,
                look(Widget::Field(field)),
            );
        }
        if let Some(r) = rect(Widget::Browse(field)) {
            widgets::button(image, r, "Browse", look(Widget::Browse(field)));
        }
        draw_check(image, [28, y + 40], &launcher.checks[i], 96);
    }
    let game = launcher.config.game.unwrap_or_default();
    font::draw(image, [28, GAME_TOP + 5], "Game", color::TEXT);
    if let Some(r) = rect(Widget::Game) {
        widgets::toggle(
            image,
            r,
            &format!("< {} >", game_name(game)),
            look(Widget::Game),
        );
    }
    font::draw(
        image,
        [322, GAME_TOP + 5],
        match game {
            GameProfile::Fallout1 => "Fallout 1 data",
            GameProfile::Fallout2 => "Fallout 2 data (SPEC-018 install)",
        },
        color::DIM,
    );
    font::draw(
        image,
        [28, 330],
        "Type a path, Browse, or drop MASTER.DAT, CRITTER.DAT or the game folder here.",
        color::DIM,
    );
    let settings = launcher.settings_path.as_ref().map_or_else(
        || "none yet: Save or Play writes vault-engine.toml".to_owned(),
        |path| path.display().to_string(),
    );
    font::draw(image, [28, 346], "Settings:", color::DIM);
    font::draw(
        image,
        [94, 346],
        &font::clip_start(&settings, 86),
        color::TEXT,
    );
}

fn draw_display(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    let config = &launcher.config;
    let scaling = config.scaling.unwrap_or(ScalingMode::AspectFit);
    let preset = config.preset.unwrap_or_default();
    let rows: [(Widget, &str, String, &str); 6] = [
        (
            Widget::Scaling,
            "Scaling",
            format!("< {} >", scaling_name(scaling)),
            match scaling {
                ScalingMode::AspectFit => "fill the window, sharp at any scale",
                ScalingMode::Integer => "whole-pixel scales with a border",
            },
        ),
        (
            Widget::Fullscreen,
            "Fullscreen",
            on_off(config.fullscreen.unwrap_or(false)).into(),
            "borderless; F11 or Alt+Enter toggles",
        ),
        (
            Widget::TouchUi,
            "Touch UI",
            on_off(config.touch_ui.unwrap_or(false)).into(),
            "tap, drag, pinch, hold; on-screen buttons",
        ),
        (
            Widget::Preset,
            "Preset",
            format!("< {} >", preset.name()),
            match preset {
                Preset::Vanilla => "the original rules, unchanged",
                Preset::Enhanced => "engine extensions and table overrides",
            },
        ),
        (
            Widget::Audio,
            "Audio",
            on_off(config.audio.unwrap_or(true)).into(),
            "off: no audio device (as --no-audio)",
        ),
        (
            Widget::Debug,
            "Debug overlay",
            on_off(config.debug.unwrap_or(false)).into(),
            "the vault-debug inspector; F9 toggles",
        ),
    ];
    font::draw(
        image,
        [COLUMNS[0].0, COLUMN_TITLE_TOP],
        "Display & audio",
        color::ACCENT,
    );
    let mut hints = Vec::new();
    draw_rows(image, 0, rows, look, rect, &mut hints);
    font::draw(
        image,
        [COLUMNS[1].0, COLUMN_TITLE_TOP],
        "Enhanced preset",
        color::ACCENT,
    );
    if enhanced_shown(launcher) {
        draw_enhanced(image, launcher, look, rect, &mut hints);
    } else {
        for (line, text) in (0..).zip([
            "Widescreen, zoom, roof peek, game",
            "speed and the quality-of-life extras:",
            "set Preset to enhanced to show them.",
        ]) {
            font::draw(
                image,
                [COLUMNS[1].0, display_top(0) + 5 + line * 14],
                text,
                color::DIM,
            );
        }
    }
    // The focused row's help, else the hovered one's.
    let shown = [Some(launcher.focus), launcher.hover]
        .into_iter()
        .flatten()
        .find_map(|widget| hints.iter().find(|(row, _)| *row == widget));
    image.fill_rect([20, HELP_TOP - 8, 600, 1], color::BORDER);
    if let Some((_, text)) = shown {
        font::draw(
            image,
            [28, HELP_TOP],
            &font::clip_end(text, 96),
            color::TEXT,
        );
    }
    font::draw(
        image,
        [28, NOTE_TOP],
        "Written to vault-engine.toml on Save or Play. Command-line flags still win.",
        color::DIM,
    );
}

/// One column's rows: each label, then its value in a toggle; the rows'
/// help texts are collected for the help line.
fn draw_rows<const N: usize>(
    image: &mut IndexedImage,
    column: usize,
    rows: [(Widget, &str, String, &str); N],
    look: &LookFn,
    rect: &RectFn,
    hints: &mut Vec<(Widget, String)>,
) {
    for (row, (widget, label, value, hint)) in (0..).zip(rows) {
        font::draw(
            image,
            [COLUMNS[column].0, display_top(row) + 5],
            label,
            color::TEXT,
        );
        if let Some(r) = rect(widget) {
            widgets::toggle(image, r, &value, look(widget));
        }
        hints.push((widget, format!("{label}: {hint}")));
    }
}

/// The enhanced preset's rows.
fn draw_enhanced(
    image: &mut IndexedImage,
    launcher: &Launcher,
    look: &LookFn,
    rect: &RectFn,
    hints: &mut Vec<(Widget, String)>,
) {
    let settings = launcher.config.enhanced_settings();
    let zoom = format!("< {}x >", settings.default_zoom);
    let scale = match settings.ui_scale {
        None => "< auto >".to_owned(),
        Some(scale) => format!("< {scale}x >"),
    };
    let speed = launcher.config.game_speed();
    let rows: [(Widget, &str, String, &str); 6] = [
        (
            Widget::Viewport,
            "Widescreen",
            match settings.viewport {
                ViewportMode::Fixed => "< Off (4:3 Classic) >",
                ViewportMode::Unlocked => "< On >",
            }
            .to_owned(),
            match settings.viewport {
                ViewportMode::Fixed => "the classic 640x480 frame; Alt+V toggles in play",
                ViewportMode::Unlocked => "the world fills the window; Alt+V toggles in play",
            },
        ),
        (
            Widget::Zoom,
            "Default zoom",
            zoom,
            "where maps start; - and = zoom in play",
        ),
        (
            Widget::SmoothCamera,
            "Smooth camera",
            on_off(settings.smooth_camera).into(),
            "glide camera moves (off: snap)",
        ),
        (
            Widget::RoofPeek,
            "Roof peek",
            format!("< {} >", roof_peek_name(settings.roof_peek)),
            match settings.roof_peek {
                RoofPeek::Vanilla => "roofs as the original; Alt peeks",
                RoofPeek::CursorXray => "cut roofs away around the cursor",
                RoofPeek::Hidden => "no roofs at all; Alt+R cycles",
            },
        ),
        (
            Widget::UiScale,
            "Interface scale",
            scale,
            "auto fits the interface to the height",
        ),
        (
            Widget::GameSpeed,
            "Game speed",
            format!("< {} >", speed.label()),
            "world animations and combat; Alt+F",
        ),
    ];
    draw_rows(image, 1, rows, look, rect, hints);
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

fn draw_mods(image: &mut IndexedImage, launcher: &Launcher, look: &LookFn, rect: &RectFn) {
    let mods = &launcher.mods;
    let file = mods.file.as_ref().map_or_else(
        || "none yet: adding a layer and saving creates load_order.toml".to_owned(),
        |path| path.display().to_string(),
    );
    font::draw(image, [28, 72], "Load order:", color::DIM);
    font::draw(image, [100, 72], &font::clip_start(&file, 85), color::TEXT);
    if let Some(reason) = &mods.read_only {
        font::draw(image, [28, 86], &font::clip_end(reason, 97), color::FOCUS);
    } else {
        font::draw(
            image,
            [28, 86],
            "Lowest first: the last layer wins.",
            color::DIM,
        );
    }
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
                text: format!("{} {name}", if entry.enabled { "[x]" } else { "[ ]" }),
                color: if entry.enabled {
                    color::TEXT
                } else {
                    color::DIM
                },
                note: Some((glyph, tone, check.text.clone())),
            }
        })
        .collect();
    if let Some(r) = rect(Widget::Layers) {
        widgets::list(image, r, &mods.list, &rows, 330, look(Widget::Layers));
    }
    let toggle = match mods.entries.get(mods.list.selected) {
        Some(entry) if !entry.enabled => "Enable",
        _ => "Disable",
    };
    for (widget, label) in [
        (Widget::LayerUp, "Up"),
        (Widget::LayerDown, "Down"),
        (Widget::LayerToggle, toggle),
        (Widget::LayerRemove, "Remove"),
        (Widget::AddLayer, "Add"),
        (Widget::BrowseLayer, "Browse"),
    ] {
        if let Some(r) = rect(widget) {
            widgets::button(image, r, label, look(widget));
        }
    }
    font::draw(image, [28, 319], "New layer", color::TEXT);
    if let Some(r) = rect(Widget::NewLayer) {
        let field = &mods.new_layer;
        widgets::text_field(
            image,
            r,
            field,
            "folder or .DAT path",
            look(Widget::NewLayer),
        );
    }
    font::draw(image, [28, 346], "Stack:", color::DIM);
    draw_check(image, [76, 346], &mods.check.stack, 88);
    font::draw(
        image,
        [28, 370],
        "Shift+Up/Down moves the selected layer, Space enables or disables it, Delete removes it.",
        color::DIM,
    );
}

fn draw_browser(image: &mut IndexedImage, browser: &super::browse::Browser, look: Look) {
    let title = match browser.target {
        Target::Field(Field::Master) => "Choose MASTER.DAT (or the game folder)",
        Target::Field(Field::Critter) => "Choose CRITTER.DAT (or the game folder)",
        Target::Field(Field::DataRoot) => "Choose the data folder",
        Target::Layer => "Choose a mod layer: a folder or a .DAT",
    };
    font::draw(image, [28, 70], title, color::ACCENT);
    let dir = browser.dir.display().to_string();
    font::draw(image, [28, 84], &font::clip_start(&dir, 97), color::TEXT);
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
                    color::FOCUS
                } else {
                    color::TEXT
                },
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
        look,
    );
    let hint = browser
        .error
        .as_deref()
        .unwrap_or("Enter opens, Backspace goes up, Esc cancels");
    let tone = if browser.error.is_some() {
        color::ERROR
    } else {
        color::DIM
    };
    font::draw(image, [270, 382], &font::clip_end(hint, 58), tone);
}
