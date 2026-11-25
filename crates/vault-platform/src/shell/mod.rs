mod boot_flow;
mod cli;
mod events;
mod present;
mod session_ops;
mod warm_switch;

pub(crate) use cli::{Args, run};
pub(crate) use present::{EnhancedShown, bezel_layer, enhanced_layers, side_tiles};

use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use vault_core::SequenceIds;
use vault_formats::{Palette, palette::PaletteCycler};
use vault_render::{EnhancedLayout, IndexedImage, Layer, Renderer, ScalingMode};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

use crate::{
    AppError, AppResult, audio, autoplay_runner, boot, config, enhanced, icon,
    input::{self, InputCommand},
    launcher, map_preview, replay, session, toast, touch,
};

/// SPEC-020 §2.6 a session's toasts: none when the setting says so, and the
/// seventh hint only with both editions found.
fn session_toasts(config: Option<&config::Config>) -> Option<toast::HotkeyToasts> {
    let setting = boot::hint_setting(config);
    (setting != config::HotkeyHints::Off).then(|| {
        let both =
            config.is_some_and(|config| boot::both_found(config, &config::standard_data_dirs()));
        toast::HotkeyToasts::new(both)
    })
}

/// The settings a selector reads now: the file's, or none.
fn current_settings() -> config::Config {
    config::find(&config::search_dirs())
        .and_then(|path| config::Config::load(&path).ok())
        .unwrap_or_default()
}

/// The interactive window's first inner size: 1280 x 960 (the frame at
/// scale 2), or on a smaller monitor the largest whole multiple of
/// 640 x 480 that fits it, at least 640 x 480.
fn initial_window_size(monitor: Option<[u32; 2]>) -> [u32; 2] {
    let k = monitor
        .filter(|size| !size.contains(&0))
        .map_or(2, |[width, height]| {
            (width / 640).min(height / 480).clamp(1, 2)
        });
    [640 * k, 480 * k]
}

/// A key the shell keeps for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellHotkey {
    /// F11 or Alt+Enter: borderless fullscreen on or off.
    Fullscreen,
    /// Alt+S: aspect-fit or integer scaling.
    Scaling,
    /// Alt+V, with the enhanced preset only: the fixed or unlocked
    /// viewport.
    Viewport,
    /// Alt+F, with the enhanced preset only: the next game speed.
    GameSpeed,
    /// Alt+L: the warm switch (SPEC-020 §2.5), under both presets.
    Switch,
}

/// The `enhanced` preset's shell hotkey `key` is with Alt held or not:
/// Alt+F cycles the game speed (`qol`: an enhanced-preset session). F is
/// no game key with Alt, and under Vanilla it stays the game's.
fn qol_hotkey(key: KeyCode, alt: bool, qol: bool) -> Option<ShellHotkey> {
    (key == KeyCode::KeyF && alt && qol).then_some(ShellHotkey::GameSpeed)
}

/// The shell hotkey `key` is with Alt held or not, in an `enhanced`
/// session or not. F10 is never one: it is the game's quit-confirmation
/// key (SPEC-014), and V is no game key, so Alt+V takes nothing from it.
fn shell_hotkey(key: KeyCode, alt: bool, enhanced: bool) -> Option<ShellHotkey> {
    match key {
        KeyCode::F11 => Some(ShellHotkey::Fullscreen),
        KeyCode::Enter | KeyCode::NumpadEnter if alt => Some(ShellHotkey::Fullscreen),
        KeyCode::KeyS if alt => Some(ShellHotkey::Scaling),
        KeyCode::KeyL if alt => Some(ShellHotkey::Switch),
        KeyCode::KeyV if alt && enhanced => Some(ShellHotkey::Viewport),
        _ => None,
    }
}

/// SPEC-001 display palette: cycling replaces display entries, never the PAL.
pub(crate) struct DisplayPalette {
    entries: [[u8; 4]; 256],
    cycler: PaletteCycler,
    generation: u32,
    /// SPEC-014 §3.18 brightness: the gamma the entries were built with.
    gamma: f64,
}
impl DisplayPalette {
    fn new(palette: &Palette, gamma: f64) -> Self {
        Self {
            entries: palette.display_rgba8(gamma),
            cycler: PaletteCycler::default(),
            generation: 0,
            gamma,
        }
    }
}

/// A Watch Mode frame of the virtual clock.
const WATCH_FRAME_MS: u64 = 16;
/// How long Max speed plays in a shown frame, leaving time to draw it.
const WATCH_MAX_BUDGET: Duration = Duration::from_millis(12);

/// The JSONL input log `--record-inputs` asks for, buffered: P-02 one
/// flush per event-loop pass (and at exit) instead of one per input event.
/// Without the flag nothing is written. The log is a diagnostic: when it
/// cannot be written (a closed pipe, a full disk), logging stops with a
/// warning and the game goes on.
#[derive(Default)]
pub(crate) struct InputLog(pub(crate) Option<Box<dyn std::io::Write>>);
impl InputLog {
    /// The log at `path`, `-` being standard output; none without a path.
    fn open(path: Option<&Path>) -> AppResult<Self> {
        let Some(path) = path else {
            return Ok(Self(None));
        };
        if path == Path::new("-") {
            return Ok(Self(Some(Box::new(std::io::BufWriter::new(
                std::io::stdout(),
            )))));
        }
        let file =
            std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self(Some(Box::new(std::io::BufWriter::new(file)))))
    }
    fn stop(&mut self, error: &dyn std::fmt::Display) {
        tracing::warn!(%error, "input log stopped: it cannot be written");
        self.0 = None;
    }
}

/// SPEC-020 §2.5 a session stopped under the selector: what it needs when
/// it resumes at the same step, as if the selector had never opened.
pub(crate) struct Stopped {
    since: Instant,
    /// The scaling when it stopped: a change while open is logged on close.
    scaling: ScalingMode,
    /// The pointer it last saw.
    pointer: Option<[f64; 2]>,
    /// Events it has to see when it resumes: releases of what was held,
    /// modifiers, focus and resizes.
    held_back: Vec<WindowEvent>,
}

#[derive(Default)]
pub(crate) struct Shell {
    /// `--autoplay-watch`'s seed and chaos audit, until a world is up.
    pub(crate) autoplay_start: Option<(i32, bool)>,
    /// The last `Autoplay` record logged: whether the bot had control.
    pub(crate) autoplay_logged: bool,
    /// How far Watch Mode's speed has run the clock ahead of the wall.
    pub(crate) watch_ahead: u64,
    /// The launcher's Resume Last Save: the next start loads the newest save.
    pub(crate) resume_next: bool,
    pub(crate) renderer: Option<Renderer>,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) ids: SequenceIds,
    pub(crate) smoke_test: bool,
    pub(crate) frame_presented: bool,
    pub(crate) failure: Option<AppError>,
    /// The running session (SPEC-020 §2.3), once started.
    pub(crate) session: Option<session::SessionRuntime>,
    pub(crate) pointer: Option<[f64; 2]>,
    /// The OS pointer and the game's pointer, which `mouse_sensitivity`
    /// scales while the software cursor is drawn.
    pub(crate) virtual_pointer: input::VirtualPointer,
    /// The platform cannot move the OS pointer, so the scaled pointer could
    /// not keep it inside the window: OS positions are used unscaled.
    pub(crate) warp_unsupported: bool,
    pub(crate) started: Option<Instant>,
    pub(crate) display: Option<DisplayPalette>,
    /// The output stream, kept alive for the whole session.
    pub(crate) _audio: Option<audio::Audio>,
    /// SPEC-014 §3.29 the engine's key repeat (the OS repeat is ignored).
    pub(crate) repeat: input::KeyRepeat,
    pub(crate) input_log: InputLog,
    /// The window title last set (P-02: set only when it changes).
    pub(crate) title: String,
    /// The drawn cursor moved since the last presented frame; the next
    /// redraw presents it once, however many moves came in between (P-01).
    pub(crate) pointer_moved: bool,
    /// How the frame is scaled (Alt+S toggles it).
    pub(crate) scaling: ScalingMode,
    /// Open the window in borderless fullscreen.
    pub(crate) fullscreen: bool,
    /// Alt is held, for the shell hotkeys.
    pub(crate) alt: bool,
    /// Shell hotkeys held down: their repeats and releases stay with the
    /// shell too.
    pub(crate) shell_keys: Vec<KeyCode>,
    /// The pre-game launcher, while it shows (then `preview` is `None`).
    pub(crate) launcher: Option<Box<launcher::Launcher>>,
    /// The command line the launcher's Play starts the game from.
    pub(crate) pending: Option<Box<Args>>,
    /// Shift is held, for the launcher's text entry.
    pub(crate) shift: bool,
    /// The `enhanced` session's presentation settings (`None` under
    /// Vanilla): logged when the game starts and changed by Alt+V.
    pub(crate) enhanced: Option<enhanced::EnhancedSettings>,
    /// The enhanced frame last presented, with the layout it was built
    /// for and its side-panel tiles; a pointer-only move redraws it with
    /// the cursor moved.
    pub(crate) shown: Option<EnhancedShown>,
    /// The world changed since `shown` was built.
    pub(crate) enhanced_dirty: bool,
    /// What the single-image path's bezel backdrop was last made for: the
    /// frame's viewport, the surface and the bezel's colors (`None`: no
    /// bezel).
    pub(crate) backdrop_key: Option<(
        vault_render::Viewport,
        [u32; 2],
        vault_render::bezel::BezelInks,
    )>,
    /// The touch UI is on: touches are read (else ignored) and the game's
    /// worlds show the pill bar.
    pub(crate) touch_ui: bool,
    /// The touch gestures, which become pointer, button and wheel input.
    pub(crate) touch: touch::TouchGestures,
    /// The `enhanced` preset's game speed (`None` under Vanilla): logged
    /// when the game starts at another speed than 1.0, and on every Alt+F.
    pub(crate) game_speed: Option<enhanced::GameSpeed>,
    /// SPEC-020 §2.4 the pre-boot screen, or the selector over a session.
    pub(crate) boot: Option<boot::Screen>,
    /// The settings and installs the pre-boot screens choose from.
    pub(crate) boot_config: Option<config::Config>,
    pub(crate) boot_found: Option<boot::Installs>,
    /// The session under the selector.
    pub(crate) stopped: Option<Stopped>,
    /// Buttons pressed on a boot screen: their releases stay with it.
    pub(crate) boot_buttons: Vec<MouseButton>,
    /// What a warm switch keeps of the command line; `None` where there is
    /// no warm switch.
    pub(crate) switch_base: Option<Box<Args>>,
    /// The last composed frame, without a toast. It lets a toast or a closed
    /// selector show again without asking the session for anything.
    pub(crate) frame: Option<IndexedImage>,
    /// SPEC-020 §2.6 this session's hints.
    pub(crate) toasts: Option<toast::HotkeyToasts>,
    /// The list's end turns `hotkey_hints` off.
    pub(crate) hints_once: bool,
    /// With no settings file that `off` stays here, for this process, until
    /// settings are next committed (SPEC-020 §2.6).
    pub(crate) hints_off: bool,
}
