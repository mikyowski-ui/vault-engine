mod boot_flow;
mod cli;
mod warm_switch;

pub(crate) use cli::{Args, run};

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

/// An enhanced frame, its layout, its side panels tiled to that layout,
/// and the bezel around a full-screen window, if it shows one.
pub(crate) struct EnhancedShown {
    pub(crate) layout: EnhancedLayout,
    pub(crate) frame: enhanced::EnhancedFrame,
    pub(crate) sides: Vec<(IndexedImage, [f32; 4])>,
    pub(crate) bezel: Option<(IndexedImage, [f32; 4])>,
}

/// The widescreen bezel's layer around a 640x480 frame drawn into
/// `frame_rect` on a `surface`-sized window, in the display palette's
/// nearest colors (`vault_render::bezel`).
pub(crate) fn bezel_layer(
    surface: [u32; 2],
    frame_rect: [f32; 4],
    palette: &[[u8; 4]; 256],
) -> Option<(IndexedImage, [f32; 4])> {
    let inks = vault_render::bezel::BezelInks::nearest(palette);
    vault_render::bezel::bezel_layer(surface, frame_rect, inks)
}

/// The side panels' images and surface rectangles: `panel` tiled to each
/// side rectangle's size in interface pixels (rounded up), the left one
/// aligned to its right edge and the right one to its left edge, so both
/// continue from the interface bar. Empty sides have none.
pub(crate) fn side_tiles(
    layout: &EnhancedLayout,
    panel: &IndexedImage,
) -> Vec<(IndexedImage, [f32; 4])> {
    let scale = layout.ui_scale;
    let [panel_w, panel_h] = [panel.width() as usize, panel.height() as usize];
    let mut out = Vec::new();
    for (side, [x, y, w, h]) in layout.side_rects.into_iter().enumerate() {
        let width = (w / scale).ceil() as u32;
        let height = (h / scale).ceil() as u32;
        let Ok(mut tile) = IndexedImage::new(width, height) else {
            continue;
        };
        let cols = width as usize;
        // The left panel's last column is the tile's last column.
        let offset = if side == 0 {
            panel_w - cols % panel_w
        } else {
            0
        };
        let pixels = panel.pixels();
        for (row, line) in tile.pixels_mut().chunks_exact_mut(cols).enumerate() {
            let source = &pixels[(row % panel_h) * panel_w..][..panel_w];
            for (col, pixel) in line.iter_mut().enumerate() {
                *pixel = source[(col + offset) % panel_w];
            }
        }
        let (drawn_w, drawn_h) = (width as f32 * scale, height as f32 * scale);
        let left = if side == 0 { x + w - drawn_w } else { x };
        out.push((tile, [left, y, drawn_w, drawn_h]));
    }
    out
}

/// The enhanced layers back to front: the world (opaque; from the world
/// area's corner, at its own size), or the bezel in its place around a
/// full-screen window, the side panels, the interface frame (see-through
/// where it says), then the pointer art at the pointer, its hotspot and
/// size at interface scale (index 0 see-through).
pub(crate) fn enhanced_layers<'a>(
    shown: &'a EnhancedShown,
    pointer: Option<[f64; 2]>,
) -> Vec<Layer<'a>> {
    let EnhancedShown {
        layout,
        frame,
        sides,
        bezel,
    } = shown;
    let mut layers = vec![match bezel {
        Some((image, rect)) => Layer {
            image,
            rect: *rect,
            transparent: None,
            sharp: true,
        },
        None => Layer {
            image: &frame.world,
            rect: layout.canvas_rect([frame.world.width(), frame.world.height()]),
            transparent: None,
            sharp: true,
        },
    }];
    layers.extend(sides.iter().map(|(image, rect)| Layer {
        image,
        rect: *rect,
        transparent: None,
        sharp: true,
    }));
    layers.push(Layer {
        image: &frame.ui,
        rect: layout.ui_rect,
        transparent: frame.ui_transparent,
        sharp: true,
    });
    if let (Some((cursor, [hot_x, hot_y])), Some([x, y])) = (&frame.cursor, pointer) {
        let scale = layout.ui_scale;
        layers.push(Layer {
            image: cursor,
            rect: [
                x as f32 - *hot_x as f32 * scale,
                y as f32 - *hot_y as f32 * scale,
                cursor.width() as f32 * scale,
                cursor.height() as f32 * scale,
            ],
            transparent: Some(0),
            sharp: true,
        });
    }
    layers
}

/// A hint's plate as a layer, at its place in the interface frame.
fn toast_layer<'a>(plate: &'a IndexedImage, shown: &EnhancedShown) -> Layer<'a> {
    let scale = shown.layout.ui_scale;
    let [left, top, _, _] = shown.layout.ui_rect;
    let [x, y] = toast::plate_origin(shown.frame.ui.width(), plate);
    Layer {
        image: plate,
        rect: [
            left + x as f32 * scale,
            top + y as f32 * scale,
            plate.width() as f32 * scale,
            plate.height() as f32 * scale,
        ],
        transparent: Some(0),
        sharp: true,
    }
}

impl Shell {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<AppError>) {
        let error = error.into();
        tracing::error!(%error, "desktop shell failed");
        self.failure = Some(error);
        event_loop.exit();
    }

    /// Launcher input is never game input: the log starts with the game
    /// (its surface and scaling first, as a direct start writes them).
    fn log(&mut self, _event_loop: &ActiveEventLoop, command: InputCommand) {
        self.record(command);
    }
    /// [`Self::log`], which writes no window state: a session's records
    /// are the same with no window. SPEC-020 §2.4 and §2.5: a boot screen
    /// writes nothing, before a game or over one.
    pub(crate) fn record(&mut self, command: InputCommand) {
        if self.launcher.is_some() || self.boot.is_some() {
            return;
        }
        let Some(log) = &mut self.input_log.0 else {
            return;
        };
        if let Err(error) = input::write_command(log, &mut self.ids, &command) {
            self.input_log.stop(&*error);
        }
    }
    /// What a session logs after its surface, in the order a replay reads
    /// them. Both surface sites come here, so the two cannot differ.
    pub(crate) fn log_session_records(&mut self) {
        self.log_scaling();
        self.log_enhanced();
        self.log_touch_ui();
        self.start_game_speed();
        self.log_mod_stack();
    }
    /// The display scaling reaches the log, so a replay maps the pointer
    /// through the same viewport.
    fn log_scaling(&mut self) {
        let aspect_fit = self.scaling == ScalingMode::AspectFit;
        self.record(InputCommand::Scaling { aspect_fit });
    }
    /// An enhanced session's settings reach the log after the surface and
    /// scaling, so a replay presents the same layers; a Vanilla session
    /// logs nothing here.
    fn log_enhanced(&mut self) {
        if let Some(settings) = self.enhanced {
            self.record(InputCommand::Enhanced {
                settings: settings.into(),
            });
        }
    }
    /// A touch UI session logs that it is one after the surface, scaling
    /// and enhanced records, so a replay shows the pill bar; a session
    /// without it logs nothing here.
    fn log_touch_ui(&mut self) {
        if self.touch_ui {
            self.record(InputCommand::TouchUi { enabled: true });
        }
    }
    /// SPEC-024a §2.3: a scripted session logs its mod stack last of the
    /// session records, and before the first tick. Any other logs nothing.
    fn log_mod_stack(&mut self) {
        let session = self
            .session
            .as_ref()
            .and_then(|session| session.mod_session());
        if let Some(session) = session {
            self.record(input::mod_stack_record(&session));
        }
    }
    /// The session's game speed, when it is not 1.0: logged after the
    /// other session records, then applied, as a replay applies it; 1.0
    /// (and Vanilla) logs nothing here.
    fn start_game_speed(&mut self) {
        if let Some(speed) = self
            .game_speed
            .filter(|speed| *speed != enhanced::GameSpeed::Normal)
        {
            self.apply_game_speed(speed);
        }
    }
    /// Log the game speed, then give it to the preview.
    fn apply_game_speed(&mut self, speed: enhanced::GameSpeed) {
        self.game_speed = Some(speed);
        self.record(InputCommand::GameSpeed {
            speed: speed.factor(),
        });
        if let Some(preview) = &mut self.session {
            preview.set_game_speed(speed);
        }
    }
    /// Watch Mode's control changes follow the inputs that made them.
    fn log_autoplay_change(&mut self) {
        let change = self
            .session
            .as_mut()
            .and_then(|preview| replay::interactive_world(preview))
            .and_then(|world| world.take_autoplay_change(&mut self.autoplay_logged));
        if let Some(on) = change {
            self.record(InputCommand::Autoplay { on });
        }
    }
    /// `--autoplay-watch`: the bot joins the first session with a world.
    fn start_watch(&mut self) {
        if let Some(world) = self
            .session
            .as_mut()
            .and_then(|preview| replay::interactive_world(preview))
            && let Some((seed, chaos)) = self.autoplay_start.take()
        {
            match vault_core::autoplay::AutoplayAgent::new(seed, chaos) {
                Ok(agent) => {
                    world.watch = Some(Box::new(map_preview::bot::Watch::new(agent)));
                }
                Err(error) => tracing::error!(%error, "the bot did not start"),
            }
        }
    }
    /// Whether the bot is playing now.
    fn bot_has_control(&mut self) -> bool {
        self.session
            .as_mut()
            .and_then(|preview| replay::interactive_world(preview))
            .and_then(|world| world.watch.as_ref())
            .is_some_and(|watch| !watch.paused)
    }
    /// One shown frame of Watch Mode: its speed's virtual 16 ms frames, each a
    /// logged tick. The clock runs ahead of the wall by what they added, so a
    /// tick never goes back when the player takes over.
    fn watch_frames(&mut self, now: u64) -> AppResult<session::Advance> {
        let begun = Instant::now();
        let Some(preview) = self.session.as_mut() else {
            return Ok(session::Advance::Finished);
        };
        let Some(mut watch) =
            replay::interactive_world(preview).and_then(|world| world.watch.take())
        else {
            return Ok(session::Advance::Running {
                changed: false,
                due: Vec::new(),
            });
        };
        let (log, ids) = (&mut self.input_log, &mut self.ids);
        let mut changed = false;
        let mut finished = false;
        let mut frames = 0;
        let mut failure = None;
        loop {
            let at = now.saturating_add(frames * WATCH_FRAME_MS);
            let step = autoplay_runner::autoplay_step(
                preview,
                &mut watch.agent,
                |command| {
                    if let Some(writer) = &mut log.0
                        && let Err(error) = input::write_command(writer, ids, command)
                    {
                        log.stop(&*error);
                    }
                    Ok(())
                },
                at,
            );
            frames += 1;
            match step {
                Ok(step) => {
                    changed |= step.changed;
                    finished = step.finished;
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
            let done = match watch.speed.frames() {
                Some(count) => frames >= u64::from(count),
                None => begun.elapsed() >= WATCH_MAX_BUDGET,
            };
            if done || finished || watch.agent.status().finished {
                break;
            }
        }
        self.watch_ahead = self
            .watch_ahead
            .saturating_add((frames - 1) * WATCH_FRAME_MS);
        if watch.agent.status().finished {
            watch.paused = true;
        }
        if let Some(world) = replay::interactive_world(preview) {
            world.watch = Some(watch);
        }
        match failure {
            Some(error) => Err(error),
            None if finished => Ok(session::Advance::Finished),
            None => Ok(session::Advance::Running {
                changed,
                due: Vec::new(),
            }),
        }
    }
    /// A session changes its own settings from its in-game window. The shell
    /// takes them up as if it had changed them: logged, and given to the
    /// `Game` that builds the next world.
    fn sync_session_settings(&mut self) {
        let Some(preview) = self.session.as_mut() else {
            return;
        };
        let settings = preview.enhanced_settings();
        let speed = preview.session_game_speed();
        if settings.is_some() && settings != self.enhanced {
            self.enhanced = settings;
            preview.set_enhanced(settings);
            self.log_enhanced();
        }
        if let Some(speed) = speed.filter(|speed| self.game_speed != Some(*speed)) {
            self.apply_game_speed(speed);
        }
    }
    /// The F9 overlay's committed commands: each is logged, then applied,
    /// so a replay of the log applies it at the same point, exactly once.
    fn drain_debug(&mut self, event_loop: &ActiveEventLoop) {
        self.log_autoplay_change();
        self.sync_session_settings();
        let Some(preview) = self.session.as_mut() else {
            return;
        };
        let (log, ids) = (&mut self.input_log, &mut self.ids);
        let result = replay::drain_debug_commands(preview, |command| {
            if let Some(writer) = &mut log.0
                && let Err(error) = input::write_command(writer, ids, command)
            {
                log.stop(&*error);
            }
            Ok(())
        });
        match result {
            Ok(true) => {
                if let Some(window) = self.window.clone() {
                    self.refresh_preview(event_loop, &window);
                }
            }
            Ok(false) => {}
            Err(error) => self.fail(event_loop, error),
        }
    }
    fn flush_log(&mut self, _event_loop: &ActiveEventLoop) {
        self.flush_records();
    }
    fn flush_records(&mut self) {
        use std::io::Write;
        if let Some(Err(error)) = self.input_log.0.as_mut().map(|log| log.flush()) {
            self.input_log.stop(&error);
        }
    }
}

impl Shell {
    /// A key press (or an engine repeat) reaches the preview. A standalone
    /// sprite or map preview, or the bare color test, exits on an Escape it
    /// does not consume; the game never exits on a key (G-01).
    fn press_key(&mut self, event_loop: &ActiveEventLoop, window: &Window, key: KeyCode) {
        let pressed = match &mut self.session {
            Some(session) => {
                let exits = session.escape_exits();
                session.press_key(key, exits)
            }
            None if key == KeyCode::Escape => session::KeyPress::Exit,
            None => session::KeyPress::Ignored,
        };
        match pressed {
            session::KeyPress::Changed => self.refresh_preview(event_loop, window),
            session::KeyPress::Exit => {
                self.log(event_loop, InputCommand::Quit);
                event_loop.exit();
            }
            session::KeyPress::Ignored => {}
        }
    }
    fn refresh_preview(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        if self.enhanced_layout(window).is_some() {
            return self.refresh_enhanced(event_loop, window);
        }
        self.shown = None;
        let Some(preview) = &mut self.session else {
            return;
        };
        match preview.image() {
            Ok(image) => {
                let palette = self.palette();
                self.show_frame(image, &palette);
                // The composed frame already shows the cursor where it is.
                self.pointer_moved = false;
                let Some(preview) = &self.session else {
                    return;
                };
                let title = preview.title();
                if title != self.title {
                    window.set_title(&title);
                    self.title = title;
                }
                window.set_cursor_visible(preview.system_cursor());
                window.request_redraw();
            }
            Err(error) => self.fail(event_loop, error),
        }
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
        }
    }
    /// A composed frame on the screen, with the shown hint over a copy, so
    /// that the frame itself never holds a toast (SPEC-020 §2.6).
    fn show_frame(&mut self, image: IndexedImage, palette: &[[u8; 4]; 256]) {
        let plate = self.toast_plate(palette);
        if let Some(renderer) = &mut self.renderer {
            match &plate {
                Some(plate) => renderer.set_image(&toast::over(&image, plate), palette),
                None => renderer.set_image(&image, palette),
            }
        }
        self.frame = Some(image);
    }
    /// The hint showing, as a plate in `palette`'s nearest colors.
    fn toast_plate(&self, palette: &[[u8; 4]; 256]) -> Option<IndexedImage> {
        let toasts = self.toasts.as_ref()?;
        let (_, step) = toasts.shown()?;
        Some(toast::plate(
            toasts.text()?,
            step,
            toast::Inks::nearest(palette),
        ))
    }
    /// One frame of the hints: the timer runs on the shell clock, and only
    /// reads the session. A changed hint redraws what is already composed.
    fn tick_toasts(&mut self, window: &Window, now: u64) {
        let ready = self
            .session
            .as_ref()
            .is_some_and(|session| session.hints_ready());
        let Some(toasts) = &mut self.toasts else {
            return;
        };
        let changed = toasts.tick(now, ready);
        if toasts.take_finished() {
            self.finish_hints();
        }
        if changed {
            self.redraw_toast(window);
        }
    }
    /// The hint changed: the same frame again, with it or without it.
    fn redraw_toast(&mut self, window: &Window) {
        if self.shown.is_none()
            && let Some(frame) = self.frame.take()
        {
            let palette = self.palette();
            self.show_frame(frame, &palette);
        }
        window.request_redraw();
    }
    /// A key press ends the hint that names it (it changes no key's action).
    fn toast_key(&mut self, window: &Window, key: KeyCode, alt: bool) {
        if self
            .toasts
            .as_mut()
            .is_some_and(|toasts| toasts.key(key, alt))
        {
            self.redraw_toast(window);
        }
    }
    /// The list is finished: `once` becomes `off`, in one commit, or in
    /// memory with no settings file. A failed commit is a diagnostic, and
    /// the session goes on (SPEC-020 §2.6).
    fn finish_hints(&mut self) {
        self.toasts = None;
        if !std::mem::take(&mut self.hints_once) {
            return;
        }
        match boot::finish_hints(&config::search_dirs()) {
            Ok(saved) => self.hints_off = !saved,
            Err(error) => tracing::warn!(%error, "hotkey_hints not saved"),
        }
    }
    /// The enhanced presentation's refresh: the next redraw builds the
    /// frame for the window's layout then; the title and the pointer
    /// follow as in [`Shell::refresh_preview`].
    fn refresh_enhanced(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(preview) = &mut self.session else {
            return;
        };
        self.enhanced_dirty = true;
        self.pointer_moved = false;
        let title = preview.title();
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
        window.request_redraw();
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
        }
    }
    /// The enhanced layout for the window, while the preview's world shows
    /// the unlocked viewport (it follows the window size and the zoom, so
    /// resizes, DPI and fullscreen changes and zooms all lay it out anew).
    fn enhanced_layout(&mut self, window: &Window) -> Option<EnhancedLayout> {
        let size = window.inner_size();
        replay::enhanced_layout(self.session.as_mut()?, [size.width, size.height])
    }
    /// Present the enhanced layers for this redraw, building the frame
    /// again when the world or the layout changed; returns whether the
    /// enhanced frame is shown. When the world answers with no enhanced
    /// frame the Vanilla frame is shown instead.
    fn present_enhanced(&mut self, event_loop: &ActiveEventLoop, window: &Window) -> bool {
        let Some(layout) = self.enhanced_layout(window) else {
            return false;
        };
        let stale = self.enhanced_dirty
            || self
                .shown
                .as_ref()
                .is_none_or(|shown| shown.layout != layout);
        if stale {
            let Some(preview) = &mut self.session else {
                return false;
            };
            match preview.enhanced_frame(&layout) {
                Some(Ok(frame)) => {
                    let sides = frame
                        .side_panel
                        .as_ref()
                        .map(|panel| side_tiles(&layout, panel))
                        .unwrap_or_default();
                    let bezel = if frame.bezel {
                        bezel_layer(layout.surface, layout.ui_rect, &self.palette())
                    } else {
                        None
                    };
                    self.shown = Some(EnhancedShown {
                        layout,
                        frame,
                        sides,
                        bezel,
                    });
                    self.enhanced_dirty = false;
                }
                Some(Err(error)) => {
                    self.fail(event_loop, error);
                    return false;
                }
                None => {
                    self.shown = None;
                    self.enhanced_dirty = false;
                    match preview.image() {
                        Ok(image) => {
                            let palette = self.palette();
                            self.show_frame(image, &palette);
                        }
                        Err(error) => self.fail(event_loop, error),
                    }
                    return false;
                }
            }
        }
        let palette = self.palette();
        let plate = self.toast_plate(&palette);
        let (Some(shown), Some(preview)) = (&self.shown, &self.session) else {
            return false;
        };
        if let Some(renderer) = &mut self.renderer {
            let mut layers = enhanced_layers(shown, self.pointer);
            if let Some(plate) = &plate {
                // Over the interface, under the pointer's art.
                let cursor = shown.frame.cursor.is_some() && self.pointer.is_some();
                let at = layers.len() - usize::from(cursor);
                layers.insert(at, toast_layer(plate, shown));
            }
            renderer.set_layers(&layers, &palette);
        }
        // The frame draws its own pointer art: the OS pointer hides then.
        window.set_cursor_visible(shown.frame.cursor.is_none() && preview.system_cursor());
        true
    }
    /// The widescreen bezel under the single 640x480 frame of a
    /// full-screen game screen (the main menu, character selection and
    /// editor; `AssetPreview::bezel_screen`) while the session shows the
    /// unlocked widescreen view (the enhanced preset's, or the touch UI's
    /// navigation-only view). The frame's viewport, and with it every
    /// pointer mapping, stays as it is: the bezel only fills the
    /// letterbox. Vanilla, the fixed 640x480 view, the launcher and movies
    /// keep the plain clear color.
    fn present_backdrop(&mut self) {
        let widescreen = self
            .enhanced
            .is_some_and(|settings| settings.viewport == enhanced::ViewportMode::Unlocked);
        let wanted = widescreen
            && self.launcher.is_none()
            && self.boot.is_none()
            && self
                .session
                .as_mut()
                .is_some_and(|session| session.bezel_screen());
        let palette = self.palette();
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let viewport = renderer
            .viewport()
            .filter(|viewport| wanted && viewport.image == vault_render::bezel::FRAME);
        let key = viewport
            .zip(renderer.surface_size())
            .map(|(viewport, surface)| {
                (
                    viewport,
                    surface,
                    vault_render::bezel::BezelInks::nearest(&palette),
                )
            });
        if key == self.backdrop_key {
            return;
        }
        self.backdrop_key = key;
        let layer = key.and_then(|(viewport, surface, _)| {
            let rect = [
                viewport.left as f32,
                viewport.top as f32,
                viewport.width as f32,
                viewport.height as f32,
            ];
            bezel_layer(surface, rect, &palette)
        });
        renderer.set_backdrop(layer.as_ref().map(|(image, rect)| Layer {
            image,
            rect: *rect,
            transparent: None,
            sharp: true,
        }));
    }
    /// The display palette the frame is drawn through.
    fn palette(&self) -> [[u8; 4]; 256] {
        let Some(preview) = &self.session else {
            return [[0, 0, 0, 255]; 256];
        };
        preview.display_override().unwrap_or_else(|| {
            self.display
                .as_ref()
                .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
        })
    }
    /// A mouse button in enhanced routing: a movie takes any button, else
    /// the left, right and middle ones reach the world in both layers'
    /// coordinates (extra buttons do nothing). Returns `false`, leaving
    /// the button to the Vanilla routing, outside the enhanced viewport.
    fn enhanced_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        button: MouseButton,
        pressed: bool,
    ) -> bool {
        let Some(layout) = self.enhanced_layout(window) else {
            return false;
        };
        let at = replay::pointer_at(&layout, self.pointer);
        let Some(preview) = &mut self.session else {
            return true;
        };
        let button = match button {
            MouseButton::Left => enhanced::PointerButton::Left,
            MouseButton::Right => enhanced::PointerButton::Right,
            MouseButton::Middle => enhanced::PointerButton::Middle,
            _ => {
                if preview.movie_button(pressed) {
                    self.refresh_preview(event_loop, window);
                }
                return true;
            }
        };
        if preview.movie_button(pressed) || preview.enhanced_button(at, button, pressed) {
            self.refresh_preview(event_loop, window);
        }
        if !pressed {
            self.warp_pointer(window);
        }
        true
    }
    /// Alt+V: the other viewport, applied, logged and drawn.
    fn toggle_viewport(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(session) = self.enhanced else {
            return;
        };
        let current = self
            .session
            .as_mut()
            .and_then(|session| session.enhanced_settings())
            .unwrap_or(session);
        let settings = replay::toggled_viewport(current);
        tracing::info!(
            viewport = config::viewport_name(settings.viewport),
            "viewport changed"
        );
        self.enhanced = Some(settings);
        self.log(
            event_loop,
            InputCommand::Enhanced {
                settings: settings.into(),
            },
        );
        if let Some(preview) = &mut self.session {
            preview.set_enhanced(Some(settings));
        }
        self.refresh_preview(event_loop, window);
    }
    /// The drawn cursor moved: the next redraw presents it.
    fn refresh_pointer(&mut self, window: &Window) {
        if self
            .session
            .as_ref()
            .is_some_and(|preview| !preview.system_cursor())
        {
            self.pointer_moved = true;
            window.request_redraw();
        }
    }
    /// Present the drawn cursor's move without composing the scene again.
    fn present_pointer(&mut self) {
        let Some(preview) = &mut self.session else {
            return;
        };
        let Some(image) = preview.pointer_frame() else {
            return;
        };
        let palette = self.palette();
        self.show_frame(image, &palette);
    }
    /// G-31 / SPEC-014 §3.28: the action menu's release moves the system
    /// pointer to the frame pixel the preview asks for (where supported).
    fn warp_pointer(&mut self, window: &Window) {
        let Some(pixel) = self
            .session
            .as_mut()
            .and_then(|session| session.take_warp())
        else {
            return;
        };
        // The renderer's own viewport: the warp lands on the surface pixel
        // that shows the frame pixel, never in the letterbox. In the
        // enhanced presentation the pixel is an interface pixel.
        let target = match self.enhanced_layout(window) {
            Some(layout) => {
                let [x, y, _, _] = layout.ui_rect.map(f64::from);
                let scale = f64::from(layout.ui_scale);
                let center = |origin: f64, pixel: i32, size: u32| {
                    (origin + (f64::from(pixel.clamp(0, size as i32 - 1)) + 0.5) * scale).floor()
                };
                Some([
                    center(x, pixel[0], EnhancedLayout::UI_FRAME[0]),
                    center(y, pixel[1], EnhancedLayout::UI_FRAME[1]),
                ])
            }
            None => self
                .renderer
                .as_ref()
                .and_then(|renderer| renderer.surface_point(pixel)),
        };
        let Some([x, y]) = target else {
            return;
        };
        match window.set_cursor_position(winit::dpi::PhysicalPosition::new(x, y)) {
            Ok(()) => self.virtual_pointer.anchor([x, y]),
            Err(error) => tracing::debug!(%error, "pointer warp unsupported"),
        }
    }
    /// The OS pointer moved to `os`: the game's pointer, scaled by the
    /// applied mouse sensitivity while the software cursor is drawn. The
    /// hidden OS pointer is warped after it, so it cannot reach the window
    /// edge first; where warps are unsupported OS positions stay unscaled.
    fn scaled_pointer(&mut self, window: &Window, os: [f64; 2]) -> [f64; 2] {
        let (scale, software) = self.session.as_mut().map_or((1.0, false), |preview| {
            (preview.mouse_sensitivity(), !preview.system_cursor())
        });
        let scale = if self.warp_unsupported { 1.0 } else { scale };
        let size = window.inner_size();
        let surface = [f64::from(size.width), f64::from(size.height)];
        let point = self.virtual_pointer.moved(os, scale, software, surface);
        let Some(target) = self.virtual_pointer.drifted() else {
            return point;
        };
        match window.set_cursor_position(winit::dpi::PhysicalPosition::new(target[0], target[1])) {
            Ok(()) => {
                self.virtual_pointer.anchor(target);
                point
            }
            Err(error) => {
                tracing::debug!(%error, "pointer warp unsupported: mouse sensitivity off");
                self.warp_unsupported = true;
                self.virtual_pointer.anchor(os);
                os
            }
        }
    }
    /// F11 and Alt+Enter toggle borderless fullscreen, Alt+S the scaling
    /// mode, and in an enhanced session Alt+V the viewport. The shell
    /// keeps these keys: neither their presses (with the
    /// repeats) nor their releases reach the game or the input log.
    /// Returns whether the shell took the key.
    fn shell_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> bool {
        if !pressed {
            let Some(at) = self.shell_keys.iter().position(|held| *held == key) else {
                return false;
            };
            self.shell_keys.swap_remove(at);
            return true;
        }
        if repeat {
            return self.shell_keys.contains(&key);
        }
        let hotkey = shell_hotkey(key, self.alt, self.enhanced.is_some())
            .or_else(|| qol_hotkey(key, self.alt, self.game_speed.is_some()));
        // No session, or a smoke test, has no warm switch (SPEC-020 §2.5).
        let switches = self.session.is_some() && !self.smoke_test;
        let Some(hotkey) = hotkey.filter(|hotkey| *hotkey != ShellHotkey::Switch || switches)
        else {
            return false;
        };
        if !self.shell_keys.contains(&key) {
            self.shell_keys.push(key);
        }
        // The session under the selector takes no key of its own.
        if self.stopped.is_some()
            && matches!(hotkey, ShellHotkey::Viewport | ShellHotkey::GameSpeed)
        {
            return true;
        }
        match hotkey {
            ShellHotkey::Switch => {
                self.switch_key(event_loop, window);
                return true;
            }
            ShellHotkey::Fullscreen => {
                let fullscreen = window.fullscreen().is_none();
                window.set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
                tracing::info!(fullscreen, "fullscreen toggled");
            }
            ShellHotkey::Scaling => {
                self.scaling = self.scaling.toggled();
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_scaling(self.scaling);
                }
                tracing::info!(
                    scaling = config::scaling_name(self.scaling),
                    "scaling mode changed"
                );
                self.log_scaling();
            }
            ShellHotkey::Viewport => self.toggle_viewport(event_loop, window),
            ShellHotkey::GameSpeed => {
                let speed = self.game_speed.unwrap_or_default().next();
                tracing::info!(speed = speed.label(), "game speed changed");
                self.apply_game_speed(speed);
                self.refresh_preview(event_loop, window);
            }
        }
        self.viewport_changed(event_loop, window);
        true
    }
    /// M-P1: the frame's viewport changed (a resize, or a scaling mode or
    /// fullscreen change): the stored pointer maps to another frame pixel,
    /// so hover runs again and the drawn cursor follows, and the next click
    /// acts where the cursor shows.
    fn viewport_changed(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        if self.boot.is_some() {
            return self.refresh_boot(window);
        }
        // The launcher from Setup, over a session that is still stopped.
        if self.stopped.is_some() {
            return self.refresh_launcher(window);
        }
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
            self.refresh_pointer(window);
        }
        window.request_redraw();
    }
    /// The frame pixel under the pointer, or `None` outside the frame.
    fn pixel(&self) -> Option<[i32; 2]> {
        self.pointer
            .and_then(|point| self.renderer.as_ref()?.image_position(point))
    }
    /// H3: the pointer's frame pixel, a pointer in the letterbox clamped to
    /// the nearest frame pixel, as hover sees it.
    fn clamped_pixel(&self) -> Option<[i32; 2]> {
        self.pointer
            .and_then(|point| self.renderer.as_ref()?.frame_point(point))
            .map(|point| point.pixel)
    }

    /// The pointer's frame pixel reaches the preview, clamped to the frame
    /// so its edge pixels stay reachable (SPEC-014 §3.29 edge scrolling),
    /// with whether it is outside the frame (H3: never an edge).
    fn log_hover(&mut self, event_loop: &ActiveEventLoop, point: [f64; 2]) {
        // In the enhanced presentation the pointer reaches the world in
        // both layers' coordinates (no hover record: the replay recomputes
        // it), and the next redraw moves the cursor layer.
        if let Some(window) = self.window.clone()
            && let Some(layout) = self.enhanced_layout(&window)
        {
            let at = replay::pointer_at(&layout, Some(point));
            if self
                .session
                .as_mut()
                .is_some_and(|preview| preview.enhanced_pointer(at))
            {
                self.enhanced_dirty = true;
            }
            window.request_redraw();
            return;
        }
        let hover = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.frame_point(point))
            .and_then(|point| self.session.as_mut()?.pointer_moved(point));
        if let Some(hover) = hover {
            self.log(
                event_loop,
                InputCommand::MapHover {
                    elevation: hover.elevation,
                    hex: hover.hex,
                    floor_square: hover.floor,
                    roof_square: hover.roof,
                },
            );
        }
    }
}

/// The mouse's handlers, which the touch gestures reach through the same
/// log records.
impl Shell {
    /// The game's pointer moved to surface point `point` (the OS pointer,
    /// scaled, or a finger): it is logged, hovers, and the drawn cursor
    /// follows.
    fn pointer_to(&mut self, event_loop: &ActiveEventLoop, window: &Window, point: [f64; 2]) {
        let [x, y] = point;
        self.pointer = Some(point);
        self.log_hover(event_loop, point);
        self.log(event_loop, InputCommand::PointerMoved { x, y });
        self.refresh_pointer(window);
    }
    /// The pointer left the window.
    fn pointer_leave(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        self.pointer = None;
        self.virtual_pointer.left();
        if let Some(preview) = &mut self.session {
            preview.pointer_left();
        }
        self.log(event_loop, InputCommand::PointerLeft);
        self.refresh_pointer(window);
    }
    /// A pointer button went down or up at the pointer.
    fn pointer_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        button: MouseButton,
        pressed: bool,
    ) {
        self.log(
            event_loop,
            InputCommand::PointerButton {
                button: format!("{button:?}"),
                pressed,
            },
        );
        if let Some(session) = &mut self.session {
            session.physical_button(pressed, &format!("{button:?}"));
        }
        if self.enhanced_button(event_loop, window, button, pressed) {
            return;
        }
        // H3: presses and releases map with the same clamping as
        // hover, so the letterbox acts at the nearest frame pixel.
        let pixel = self.clamped_pixel();
        // SPEC-012 §3.12 (G-31): a movie counts the buttons the
        // frame paths below never see, a middle or extra button
        // and a press in the letterbox around the frame.
        let framed = matches!(button, MouseButton::Left | MouseButton::Right);
        if (!framed || pressed && self.pixel().is_none())
            && self
                .session
                .as_mut()
                .is_some_and(|preview| preview.movie_button(pressed))
        {
            self.refresh_preview(event_loop, window);
            return;
        }
        if !pressed
            && framed
            && self
                .session
                .as_mut()
                .is_some_and(|preview| preview.release(pixel, button == MouseButton::Right))
        {
            self.refresh_preview(event_loop, window);
            self.warp_pointer(window);
        }
        if pressed
            && framed
            && let Some(pixel) = pixel
            && self
                .session
                .as_mut()
                .is_some_and(|preview| preview.click(pixel, button == MouseButton::Right))
        {
            self.refresh_preview(event_loop, window);
        }
    }
    /// A wheel delta at the pointer.
    fn wheel(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        [x, y]: [f64; 2],
        unit: input::WheelUnit,
    ) {
        self.log(event_loop, InputCommand::Wheel { x, y, unit });
        let at = self
            .enhanced_layout(window)
            .map(|layout| replay::pointer_at(&layout, self.pointer));
        if self.session.as_mut().is_some_and(|preview| match at {
            Some(at) => preview.enhanced_wheel(at, replay::wheel_lines(y, unit)),
            None => preview.scroll(y),
        }) {
            self.refresh_preview(event_loop, window);
        }
    }
    /// The shell clock, in milliseconds since the window opened (or the
    /// launcher's game started).
    fn now(&self) -> u64 {
        self.started.map_or(0, |started| {
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
        })
    }
    /// Whether a finger landing at surface point `point` is on the touch
    /// UI's pill bar.
    fn touch_button_at(&mut self, window: &Window, point: [f64; 2]) -> bool {
        let pixel = match self.enhanced_layout(window) {
            Some(layout) => layout.ui_pixel(point),
            None => self
                .renderer
                .as_ref()
                .and_then(|renderer| renderer.image_position(point)),
        };
        let Some(pixel) = pixel else {
            return false;
        };
        self.session
            .as_mut()
            .is_some_and(|preview| preview.touch_button_at(pixel))
    }
    /// A touch, with the touch UI on: its gesture's actions.
    fn touch_event(&mut self, event_loop: &ActiveEventLoop, window: &Window, touch: Touch) {
        let point = [touch.location.x, touch.location.y];
        let context = touch::TouchContext {
            camera: self.enhanced_layout(window).is_some(),
            button: touch.phase == TouchPhase::Started && self.touch_button_at(window, point),
            menu: self
                .session
                .as_mut()
                .is_some_and(|preview| preview.held_menu_open()),
        };
        let now = self.now();
        let actions = self.touch.feed(touch.phase, touch.id, point, now, context);
        self.touch_actions(event_loop, window, actions);
    }
    /// Gesture actions, each as the input record it is, through the
    /// mouse's handler for that record (which logs it). A pinch's wheel
    /// lines zoom over the world only: elsewhere wheel lines would scroll
    /// a conversation's options, so they are not sent.
    fn touch_actions(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        actions: Vec<touch::TouchAction>,
    ) {
        for action in actions {
            match action.command() {
                InputCommand::PointerMoved { x, y } => {
                    self.virtual_pointer.anchor([x, y]);
                    self.pointer_to(event_loop, window, [x, y]);
                }
                InputCommand::PointerLeft => self.pointer_leave(event_loop, window),
                InputCommand::PointerButton { button, pressed } => {
                    let button = match button.as_str() {
                        "Left" => MouseButton::Left,
                        "Right" => MouseButton::Right,
                        _ => MouseButton::Middle,
                    };
                    self.pointer_button(event_loop, window, button, pressed);
                }
                InputCommand::Wheel { x, y, unit } => {
                    let zoomable = self.enhanced_layout(window).is_some_and(|layout| {
                        let at = replay::pointer_at(&layout, self.pointer);
                        self.session
                            .as_mut()
                            .is_some_and(|preview| preview.enhanced_zoomable(at))
                    });
                    if zoomable {
                        self.wheel(event_loop, window, [x, y], unit);
                    }
                }
                _ => {}
            }
        }
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        self.started.get_or_insert_with(Instant::now);
        let monitor = event_loop
            .primary_monitor()
            .map(|monitor| [monitor.size().width, monitor.size().height]);
        let [width, height] = initial_window_size(monitor);
        let attributes = Window::default_attributes()
            .with_window_icon(winit::window::Icon::from_rgba(icon::rgba(64), 64, 64).ok())
            .with_title(match (&self.session, &self.launcher, &self.boot) {
                (Some(preview), ..) => preview.title(),
                (None, Some(launcher), _) => launcher.title(),
                (None, None, Some(screen)) => screen.title(),
                (None, None, None) => "Vault Engine — Indexed Color Test".to_owned(),
            })
            .with_inner_size(PhysicalSize::new(width, height))
            // H2: the frame is never down-sampled in a window that keeps
            // its minimum size.
            .with_min_inner_size(PhysicalSize::new(640, 480))
            .with_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
        // The Wayland app id and the X11 class alike: the desktop entry's
        // `StartupWMClass`, and on Wayland the only way to its icon.
        #[cfg(target_os = "linux")]
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes,
            "vault-engine",
            "vault-engine",
        );
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        let size = window.inner_size();
        // D-11: the surface in physical pixels (a HiDPI window is larger
        // than its logical size), so a replay picks with the same geometry.
        self.log(
            event_loop,
            InputCommand::Surface {
                width: size.width,
                height: size.height,
            },
        );
        let mut first_frame = None;
        let renderer = (|| -> AppResult<Renderer> {
            if let Some(preview) = &mut self.session {
                let image = preview.image()?;
                let palette = preview.display_override().unwrap_or_else(|| {
                    self.display
                        .as_ref()
                        .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
                });
                let renderer = pollster::block_on(Renderer::new_with_image(
                    window.clone(),
                    size.width,
                    size.height,
                    &image,
                    &palette,
                ))?;
                first_frame = Some(image);
                Ok(renderer)
            } else if let Some(screen) = &self.boot {
                Ok(pollster::block_on(Renderer::new_with_image(
                    window.clone(),
                    size.width,
                    size.height,
                    &screen.image(0),
                    &launcher::palette(),
                ))?)
            } else if let Some(launcher) = &self.launcher {
                Ok(pollster::block_on(Renderer::new_with_image(
                    window.clone(),
                    size.width,
                    size.height,
                    &launcher.image(),
                    &launcher::palette(),
                ))?)
            } else {
                Ok(pollster::block_on(Renderer::new(
                    window.clone(),
                    size.width,
                    size.height,
                ))?)
            }
        })();
        match renderer {
            Ok(mut renderer) => {
                renderer.set_scaling(self.scaling);
                self.renderer = Some(renderer);
                self.frame = first_frame;
                self.log_session_records();
            }
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        }
        if let Some(preview) = &self.session {
            window.set_cursor_visible(preview.system_cursor());
        }
        // The gestures' slop is in logical pixels.
        self.touch.slop = touch::SLOP * window.scale_factor();
        window.request_redraw();
        self.window = Some(window);
        tracing::info!("desktop ready; Escape or close the window to exit");
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Release the surface before its window, including on suspend/resume.
        self.renderer = None;
        self.window = None;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // SPEC-020 §2.4 and §2.5: a boot screen has no game frame, and the
        // session under the selector takes no step.
        if self.boot.is_some() || self.stopped.is_some() {
            return self.boot_tick(event_loop);
        }
        // The F9 overlay's commands follow the inputs that committed them.
        self.drain_debug(event_loop);
        // The commands logged since the last pass.
        self.flush_log(event_loop);
        if self.launcher.is_some() {
            return self.launcher_tick(event_loop);
        }
        let Some(started) = self.started.filter(|_| self.session.is_some()) else {
            return;
        };
        let now = (started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
            .saturating_add(self.watch_ahead);
        // A touch held long enough comes due as a long press.
        if self.touch_ui
            && let Some(window) = self.window.clone()
        {
            let actions = self.touch.poll(now);
            self.touch_actions(event_loop, &window, actions);
        }
        // D-11: every frame's clock is logged, so a replay ticks the preview
        // at the same times between the same commands.
        self.start_watch();
        let (changed, due) = if self.bot_has_control() {
            match self.watch_frames(now) {
                Ok(session::Advance::Running { changed, due }) => (changed, due),
                Ok(session::Advance::Finished) => {
                    tracing::info!("the bot's game ended");
                    event_loop.exit();
                    return;
                }
                Err(error) => return self.fail(event_loop, error),
            }
        } else {
            self.log(event_loop, InputCommand::Tick { now_ms: now });
            let Some(session) = self.session.as_mut() else {
                return;
            };
            // The session's frame step (SPEC-020 §2.3), as a replay runs it;
            // SPEC-014 §3.29: held keys repeat on the frame clock.
            match session.advance(now, &mut self.repeat) {
                Ok(session::Advance::Running { changed, due }) => (changed, due),
                Ok(session::Advance::Finished) => {
                    tracing::info!("movie finished");
                    event_loop.exit();
                    return;
                }
                Err(error) => return self.fail(event_loop, error),
            }
        };
        if let Some(window) = self.window.clone() {
            for key in due {
                self.press_key(event_loop, &window, key);
            }
        }
        let Some(preview) = self.session.as_mut() else {
            return;
        };
        // Each loop pass is one enabled cycling poll; the world can disable it
        // (for example during dialogue or a movie) and later restore it. A
        // movie's device palette is presented with its frames instead.
        let overridden = preview.display_override().is_some();
        let cycled = !overridden
            && self.display.as_mut().is_some_and(|display| {
                // A script-loaded palette replaces the display entries; cycling
                // phases and timers carry on from their current state.
                let gamma = preview.brightness();
                let replaced =
                    display.generation != preview.palette_generation() || display.gamma != gamma;
                if replaced {
                    display.generation = preview.palette_generation();
                    display.gamma = gamma;
                    display.entries = preview.palette().display_rgba8(gamma);
                }
                display.cycler.enabled = preview.palette_cycling();
                display.cycler.poll(now, &mut display.entries) || replaced
            });
        if let Some(window) = self.window.clone() {
            self.tick_toasts(&window, now);
        }
        if changed {
            if let Some(window) = self.window.clone() {
                self.refresh_preview(event_loop, &window);
            }
        } else if cycled
            && let (Some(renderer), Some(display), Some(window)) =
                (&mut self.renderer, &self.display, &self.window)
        {
            renderer.set_palette(&display.entries);
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self
            .window
            .clone()
            .filter(|window| window.id() == window_id)
        else {
            return;
        };
        // A button pressed on the selector, released once it has closed.
        if let WindowEvent::MouseInput {
            state: ElementState::Released,
            button,
            ..
        } = &event
            && self.boot.is_none()
            && self.take_boot_button(*button)
        {
            return;
        }
        let event = if self.launcher.is_some() || self.boot.is_some() {
            match self.launcher_event(event_loop, &window, event) {
                Some(event) => event,
                None => return,
            }
        } else {
            event
        };
        match event {
            WindowEvent::CloseRequested => {
                self.log(event_loop, InputCommand::Quit);
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(key) = event.physical_key
                    && self.shell_key(event_loop, &window, key, pressed, event.repeat)
                {
                    return;
                }
                // A replay's Space would end a turn: the takeover is logged
                // as `Autoplay`, never as the key.
                let swallowed = match event.physical_key {
                    PhysicalKey::Code(key) => self
                        .session
                        .as_mut()
                        .and_then(|preview| replay::interactive_world(preview))
                        .is_some_and(|world| world.autoplay_swallows(key, pressed)),
                    PhysicalKey::Unidentified(_) => false,
                };
                if !swallowed {
                    self.log(
                        event_loop,
                        InputCommand::Key {
                            physical_key: match event.physical_key {
                                PhysicalKey::Code(code) => format!("{code:?}"),
                                PhysicalKey::Unidentified(code) => {
                                    format!("Unidentified({code:?})")
                                }
                            },
                            pressed,
                            repeat: event.repeat,
                        },
                    );
                }
                // SPEC-014 §3.29: the engine repeats a held key itself, so
                // the operating system's repeats are only logged.
                let PhysicalKey::Code(key) = event.physical_key else {
                    return;
                };
                if !pressed {
                    let key = self
                        .session
                        .as_mut()
                        .map_or(key, |session| session.physical_key(false, key));
                    self.repeat.release(key);
                    if let Some(preview) = &mut self.session {
                        preview.key_up(key);
                    }
                } else if !event.repeat {
                    self.toast_key(&window, key, self.alt);
                    let key = self
                        .session
                        .as_mut()
                        .map_or(key, |session| session.physical_key(true, key));
                    if let Some(preview) = &self.session {
                        self.repeat.press(key, preview.keyboard());
                    }
                    self.press_key(event_loop, &window, key);
                }
            }
            // While a finger is down the touch gestures own the pointer:
            // mouse events a platform derives from the touch are dropped.
            WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
                if self.touch.active() => {}
            WindowEvent::CursorMoved { position, .. } => {
                // The log records the game's pointer, so a replay needs no
                // sensitivity of its own.
                let point = self.scaled_pointer(&window, [position.x, position.y]);
                self.pointer_to(event_loop, &window, point);
            }
            WindowEvent::CursorLeft { .. } => self.pointer_leave(event_loop, &window),
            WindowEvent::MouseInput { state, button, .. } => {
                self.pointer_button(event_loop, &window, button, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y, unit) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (f64::from(x), f64::from(y), input::WheelUnit::Lines)
                    }
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x, position.y, input::WheelUnit::Pixels)
                    }
                };
                self.wheel(event_loop, &window, [x, y], unit);
            }
            WindowEvent::Touch(touch) if self.touch_ui => {
                self.touch_event(event_loop, &window, touch);
            }
            WindowEvent::Focused(focused) => {
                // Keys held when the focus goes stop repeating, and (M-P6)
                // held mouse presses are dropped: their releases go to
                // another window.
                if !focused {
                    self.repeat.clear();
                    // Their releases go elsewhere too, and so do the
                    // fingers' lifts.
                    self.shell_keys.clear();
                    self.touch = touch::TouchGestures::new(self.touch.slop);
                    if let Some(preview) = &mut self.session {
                        preview.focus_lost();
                    }
                }
                self.log(event_loop, InputCommand::Focus { focused });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
                self.alt = modifiers.alt_key();
                if let Some(preview) = &mut self.session {
                    preview.set_modifiers([
                        modifiers.shift_key(),
                        modifiers.control_key(),
                        modifiers.alt_key(),
                    ]);
                }
                self.log(
                    event_loop,
                    InputCommand::Modifiers {
                        shift: modifiers.shift_key(),
                        control: modifiers.control_key(),
                        alt: modifiers.alt_key(),
                        super_key: modifiers.super_key(),
                    },
                );
            }
            WindowEvent::Resized(size) => {
                self.log(
                    event_loop,
                    InputCommand::Resize {
                        width: size.width,
                        height: size.height,
                    },
                );
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.viewport_changed(event_loop, &window);
            }
            // The window is resized to the new scale's suggested size after
            // this event, which reports the new physical size in `Resized`
            // (handled above); `inner_size` here is still the old one.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.touch.slop = touch::SLOP * scale_factor;
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                // A boot screen is on the window already. The enhanced
                // layers draw the cursor themselves.
                let boot = self.boot.is_some() || self.stopped.is_some();
                let enhanced = !boot && self.present_enhanced(event_loop, &window);
                if std::mem::take(&mut self.pointer_moved) && !enhanced && !boot {
                    self.present_pointer();
                }
                if !enhanced {
                    self.present_backdrop();
                }
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                // Notify Wayland only when a completed frame will be presented.
                match renderer.render(|| window.pre_present_notify()) {
                    Ok(true) if self.smoke_test => {
                        self.frame_presented = true;
                        tracing::info!("smoke test: indexed sprite frame presented");
                        event_loop.exit();
                    }
                    Ok(false) => {
                        let size = window.inner_size();
                        if size.width != 0 && size.height != 0 {
                            window.request_redraw();
                        }
                    }
                    Ok(true) => self.frame_presented = true,
                    Err(error) => self.fail(event_loop, error),
                }
            }
            _ => {}
        }
    }
}
