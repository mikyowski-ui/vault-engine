mod audio;
mod config;
mod editor_screen;
mod enhanced;
mod game;
mod input;
mod interface;
mod launcher;
mod map_preview;
mod movie;
mod preview;
mod replay;

#[cfg(test)]
pub(crate) use vault_fixtures::formats as fixture_assets;

use std::{
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::Parser;
use input::InputCommand;
use tracing_subscriber::EnvFilter;
use vault_core::SequenceIds;
use vault_formats::{Palette, palette::PaletteCycler};
use vault_render::{EnhancedLayout, IndexedImage, Layer, Renderer, ScalingMode};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

type AppError = Box<dyn Error + Send + Sync>;
type AppResult<T> = Result<T, AppError>;

#[derive(Parser, Clone)]
#[command(
    about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives",
    long_about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives.\n\nWithout arguments the game starts from the data named in vault-engine.toml (beside the executable, else in the working directory). Without that file, MASTER.DAT and CRITTER.DAT are looked for in the working directory, beside the executable, in ../local-corpus and in the usual Steam and GOG install directories, and what is found is saved to vault-engine.toml. Command-line flags always win over the file.\n\nF11 or Alt+Enter toggles borderless fullscreen; Alt+S toggles aspect-fit and integer scaling; with the enhanced preset, Alt+V toggles the fixed and unlocked viewports."
)]
struct Args {
    /// Present one frame and exit successfully (requires a desktop and GPU backend).
    #[arg(long)]
    smoke_test: bool,
    /// Do not open an audio output device; sounds resolve but stay silent.
    #[arg(long)]
    no_audio: bool,
    /// Replay a captured JSONL input log headlessly (no window, GPU, or
    /// audio) against the selected preview, ticking it at the logged frame
    /// times, then print the final state SHA-256 digest as JSON.
    #[arg(
        long,
        value_name = "PATH",
        requires = "preview_asset",
        conflicts_with_all = ["smoke_test", "no_audio"]
    )]
    replay_inputs: Option<PathBuf>,
    /// With --replay-inputs: also write the final frame as a binary PPM.
    #[arg(long, value_name = "PATH", requires = "replay_inputs")]
    replay_frame: Option<PathBuf>,
    /// Record every input command and frame clock as a JSONL input log to
    /// FILE (`-`: standard output), for --replay-inputs. Without it no
    /// input log is written.
    #[arg(long, value_name = "FILE", conflicts_with = "replay_inputs")]
    record_inputs: Option<PathBuf>,
    /// How the frame is scaled into the window: `aspect_fit` (the largest
    /// fit, sharp-bilinear at fractional scales) or `integer` (whole scales
    /// only). Default: the settings file's, else integer. Alt+S toggles it.
    #[arg(long, value_name = "MODE", value_parser = scaling_arg)]
    scaling: Option<ScalingMode>,
    /// Start in borderless fullscreen (F11 or Alt+Enter toggles it).
    #[arg(long)]
    fullscreen: bool,
    /// The engine preset: `vanilla` or `enhanced`. Default: the settings
    /// file's, else vanilla.
    #[arg(long, value_name = "PRESET", value_parser = preset_arg)]
    preset: Option<config::Preset>,
    /// With the enhanced preset, the viewport: `fixed_640x480` or
    /// `unlocked_widescreen` (the world across the window behind the
    /// interface). Default: the settings file's `viewport_mode`, else
    /// unlocked. Alt+V toggles it.
    #[arg(long, value_name = "MODE", value_parser = viewport_arg)]
    viewport: Option<enhanced::ViewportMode>,
    /// With the enhanced preset, the zoom a map starts at, 0.5 to 3.0.
    /// Default: the settings file's `default_zoom`, else 1.0.
    #[arg(long, value_name = "ZOOM", value_parser = zoom_arg)]
    zoom: Option<f32>,
    /// Open the launcher (game data, display, audio and mod settings, then
    /// Play) even when the settings are complete. A start naming nothing
    /// opens it anyway while the settings name no valid game data.
    #[arg(long, conflicts_with_all = ["replay_inputs", "smoke_test", "preview_asset"])]
    launcher: bool,
    #[command(flatten)]
    preview: preview::PreviewArgs,
}

fn scaling_arg(text: &str) -> Result<ScalingMode, String> {
    config::parse_scaling(text).ok_or_else(|| "expected aspect_fit or integer".into())
}
fn preset_arg(text: &str) -> Result<config::Preset, String> {
    config::Preset::parse(text).ok_or_else(|| "expected vanilla or enhanced".into())
}
fn viewport_arg(text: &str) -> Result<enhanced::ViewportMode, String> {
    config::parse_viewport(text)
        .ok_or_else(|| "expected fixed_640x480 or unlocked_widescreen".into())
}
fn zoom_arg(text: &str) -> Result<f32, String> {
    config::parse_zoom(text).ok_or_else(|| "expected a number from 0.5 to 3.0".into())
}

impl Args {
    /// An asset to preview, or the game, is named.
    fn names_asset(&self) -> bool {
        let preview = &self.preview;
        preview.game
            || preview.preview_frm.is_some()
            || preview.preview_map.is_some()
            || preview.play_movie.is_some()
    }
    /// The command line names the asset source.
    fn names_source(&self) -> bool {
        self.preview.corpus_dir.is_some() || self.preview.master_dat.is_some()
    }
    /// The launch would use game data the command line does not name: the
    /// settings file's or, without one, discovered data. A replay never
    /// does, and neither does a bare smoke test (the color test).
    fn wants_data(&self) -> bool {
        self.replay_inputs.is_none()
            && !self.names_source()
            && (self.names_asset() || !self.smoke_test)
    }
}

/// What the command line and the settings file make of a launch.
#[derive(Debug, Clone, PartialEq)]
struct Launch {
    scaling: ScalingMode,
    fullscreen: bool,
    preset: config::Preset,
    /// With the enhanced preset only, its presentation settings.
    enhanced: Option<enhanced::EnhancedSettings>,
    /// The settings' mod list, for the engine to load.
    mod_list: Option<config::ModList>,
    /// No asset was named and the settings' game data starts the game.
    zero_arg_game: bool,
    /// The settings' `audio = false` keeps the device closed (as
    /// `--no-audio` does).
    audio: bool,
}

/// Merge `config` into the command line, which always wins: with no asset
/// named (and no smoke test) settings that name game data start the game;
/// an asset or the game named without a source takes the settings' data;
/// and the display, preset and debug settings fill what the flags leave
/// unset. A replay takes nothing from the settings: it runs as logged.
fn resolve_launch(args: &mut Args, config: Option<&config::Config>) -> Launch {
    let none = config::Config::default();
    let config = match config {
        Some(config) if args.replay_inputs.is_none() => config,
        _ => &none,
    };
    let named = args.names_asset();
    let zero_arg_game = args.wants_data() && !named && config.has_game_data();
    if zero_arg_game {
        args.preview.game = true;
    }
    if (named || zero_arg_game) && args.wants_data() {
        if let (Some(master), Some(critter)) = (&config.master_dat, &config.critter_dat) {
            args.preview.master_dat = Some(master.clone());
            args.preview.critter_dat = Some(critter.clone());
        }
        args.preview.corpus_dir = config.data_root.clone();
    }
    args.preview.debug |= config.debug.unwrap_or(false);
    let preset = args.preset.or(config.preset).unwrap_or_default();
    let enhanced = (preset == config::Preset::Enhanced).then(|| {
        let settings = config.enhanced_settings();
        enhanced::EnhancedSettings {
            viewport: args.viewport.unwrap_or(settings.viewport),
            default_zoom: args.zoom.unwrap_or(settings.default_zoom),
            ..settings
        }
    });
    Launch {
        scaling: args
            .scaling
            .or(config.scaling)
            .unwrap_or(ScalingMode::AspectFit),
        fullscreen: args.fullscreen || config.fullscreen.unwrap_or(false),
        preset,
        enhanced,
        mod_list: config.mod_list.clone(),
        zero_arg_game,
        audio: !args.no_audio && config.audio.unwrap_or(true),
    }
}

/// Merge the settings into the command line, load what it names, and open
/// the audio output a world or a movie plays through.
fn start(
    args: &mut Args,
    config: Option<&config::Config>,
) -> AppResult<(Launch, Option<preview::AssetPreview>, Option<audio::Audio>)> {
    let launch = resolve_launch(args, config);
    if launch.zero_arg_game {
        tracing::info!("starting the game from the settings' data");
    }
    args.preview.preset = match launch.preset {
        config::Preset::Vanilla => vault_core::EnginePreset::Vanilla,
        config::Preset::Enhanced => vault_core::EnginePreset::Enhanced,
    };
    args.preview.mods = launch.mod_list.clone();
    args.preview.enhanced = launch.enhanced;
    let mut preview = args.preview.load()?;
    // Audio serves an interactive world or a movie; sprites need none.
    let output = || {
        if launch.audio {
            audio::Audio::open()
        } else {
            audio::Audio::silent()
        }
    };
    let audio = match &mut preview {
        Some(preview::AssetPreview::Map(map)) => {
            let audio = output();
            map.audio = audio.handle();
            Some(audio)
        }
        Some(preview::AssetPreview::Movie(movie)) => {
            let audio = output();
            movie.player.audio = audio.handle();
            Some(audio)
        }
        Some(preview::AssetPreview::Game(game)) => {
            let audio = output();
            game.set_audio(audio.handle());
            Some(audio)
        }
        _ => None,
    };
    Ok((launch, preview, audio))
}

/// The command line the launcher's Play starts from: the one it was shown
/// for, its display flags dropped (the launcher showed them as its own
/// choices and saved what it launches with), debug on for Launch with
/// Debug. With the launcher's settings, [`start`] then builds the
/// zero-argument launch.
fn play_args(shown: &Args, debug: bool) -> Args {
    let mut args = shown.clone();
    args.scaling = None;
    args.fullscreen = false;
    args.preset = None;
    args.viewport = None;
    args.zoom = None;
    args.no_audio = false;
    args.preview.debug |= debug;
    args
}

/// The launcher a start shows, if it shows one: with `--launcher`, or for
/// a start naming nothing whose settings are missing, malformed
/// (`problem`) or name game data that does not validate. The command
/// line's display flags pre-set its choices.
fn launcher_for(
    args: &Args,
    config: Option<&config::Config>,
    problem: Option<String>,
) -> Option<launcher::Launcher> {
    let mut config = config.cloned().unwrap_or_default();
    if let Some(scaling) = args.scaling {
        config.scaling = Some(scaling);
    }
    if let Some(preset) = args.preset {
        config.preset = Some(preset);
    }
    if args.fullscreen {
        config.fullscreen = Some(true);
    }
    if let Some(viewport) = args.viewport {
        config.viewport_mode = Some(viewport);
    }
    if let Some(zoom) = args.zoom {
        config.default_zoom = Some(zoom);
    }
    if args.no_audio {
        config.audio = Some(false);
    }
    let forced = args.launcher || problem.is_some();
    let launcher = launcher::Launcher::new(launcher::Setup {
        config,
        settings_path: config::find(&config::search_dirs()),
        save_dirs: config::search_dirs(),
        start_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        problem,
    });
    match launcher.ready() {
        Ok(()) if !forced => None,
        Ok(()) => Some(launcher),
        Err(reason) => {
            tracing::info!(%reason, "no valid game data set up: showing the launcher");
            Some(launcher)
        }
    }
}

/// The settings file beside the executable or in the working directory.
/// Without one, when the launch wants game data the command line does not
/// name, the data discovered on disk, saved for the next launch.
fn settings(args: &Args) -> AppResult<Option<config::Config>> {
    if args.replay_inputs.is_some() {
        return Ok(None);
    }
    let dirs = config::search_dirs();
    if let Some(path) = config::find(&dirs) {
        tracing::info!(file = %path.display(), "settings loaded");
        return config::Config::load(&path).map(Some);
    }
    if !args.wants_data() {
        return Ok(None);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let home = config::home_dir();
    let Some(found) = config::discover(&config::data_dirs(exe_dir.as_deref(), home.as_deref()))
    else {
        return Ok(None);
    };
    tracing::info!(
        master = ?found.master_dat,
        critter = ?found.critter_dat,
        data = ?found.data_root,
        "game data discovered"
    );
    config::persist(&found, &dirs);
    Ok(Some(found))
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
}

/// The shell hotkey `key` is with Alt held or not, in an `enhanced`
/// session or not. F10 is never one: it is the game's quit-confirmation
/// key (SPEC-014), and V is no game key, so Alt+V takes nothing from it.
fn shell_hotkey(key: KeyCode, alt: bool, enhanced: bool) -> Option<ShellHotkey> {
    match key {
        KeyCode::F11 => Some(ShellHotkey::Fullscreen),
        KeyCode::Enter | KeyCode::NumpadEnter if alt => Some(ShellHotkey::Fullscreen),
        KeyCode::KeyS if alt => Some(ShellHotkey::Scaling),
        KeyCode::KeyV if alt && enhanced => Some(ShellHotkey::Viewport),
        _ => None,
    }
}

/// SPEC-001 display palette: cycling replaces display entries, never the PAL.
struct DisplayPalette {
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

/// The JSONL input log `--record-inputs` asks for, buffered: P-02 one
/// flush per event-loop pass (and at exit) instead of one per input event.
/// Without the flag nothing is written. The log is a diagnostic: when it
/// cannot be written (a closed pipe, a full disk), logging stops with a
/// warning and the game goes on.
#[derive(Default)]
struct InputLog(Option<Box<dyn std::io::Write>>);
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

#[derive(Default)]
struct Shell {
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    ids: SequenceIds,
    smoke_test: bool,
    frame_presented: bool,
    failure: Option<AppError>,
    preview: Option<preview::AssetPreview>,
    pointer: Option<[f64; 2]>,
    /// The OS pointer and the game's pointer, which `mouse_sensitivity`
    /// scales while the software cursor is drawn.
    virtual_pointer: input::VirtualPointer,
    /// The platform cannot move the OS pointer, so the scaled pointer could
    /// not keep it inside the window: OS positions are used unscaled.
    warp_unsupported: bool,
    started: Option<Instant>,
    display: Option<DisplayPalette>,
    /// The output stream, kept alive for the whole session.
    _audio: Option<audio::Audio>,
    /// SPEC-014 §3.29 the engine's key repeat (the OS repeat is ignored).
    repeat: input::KeyRepeat,
    input_log: InputLog,
    /// The window title last set (P-02: set only when it changes).
    title: String,
    /// The drawn cursor moved since the last presented frame; the next
    /// redraw presents it once, however many moves came in between (P-01).
    pointer_moved: bool,
    /// How the frame is scaled (Alt+S toggles it).
    scaling: ScalingMode,
    /// Open the window in borderless fullscreen.
    fullscreen: bool,
    /// Alt is held, for the shell hotkeys.
    alt: bool,
    /// Shell hotkeys held down: their repeats and releases stay with the
    /// shell too.
    shell_keys: Vec<KeyCode>,
    /// The pre-game launcher, while it shows (then `preview` is `None`).
    launcher: Option<Box<launcher::Launcher>>,
    /// The command line the launcher's Play starts the game from.
    pending: Option<Box<Args>>,
    /// Shift is held, for the launcher's text entry.
    shift: bool,
    /// The `enhanced` session's presentation settings (`None` under
    /// Vanilla): logged when the game starts and changed by Alt+V.
    enhanced: Option<enhanced::EnhancedSettings>,
    /// The enhanced frame last presented, with the layout it was built
    /// for and its side-panel tiles; a pointer-only move redraws it with
    /// the cursor moved.
    shown: Option<EnhancedShown>,
    /// The world changed since `shown` was built.
    enhanced_dirty: bool,
}

/// An enhanced frame, its layout and its side panels tiled to that layout.
struct EnhancedShown {
    layout: EnhancedLayout,
    frame: enhanced::EnhancedFrame,
    sides: Vec<(IndexedImage, [f32; 4])>,
}

/// The side panels' images and surface rectangles: `panel` tiled to each
/// side rectangle's size in interface pixels (rounded up), the left one
/// aligned to its right edge and the right one to its left edge, so both
/// continue from the interface bar. Empty sides have none.
fn side_tiles(layout: &EnhancedLayout, panel: &IndexedImage) -> Vec<(IndexedImage, [f32; 4])> {
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

/// The enhanced layers back to front: the world (opaque), the side
/// panels, the interface frame (see-through where it says), then the
/// pointer art at the pointer, its hotspot and size at interface scale
/// (index 0 see-through).
fn enhanced_layers<'a>(shown: &'a EnhancedShown, pointer: Option<[f64; 2]>) -> Vec<Layer<'a>> {
    let EnhancedShown {
        layout,
        frame,
        sides,
    } = shown;
    let mut layers = vec![Layer {
        image: &frame.world,
        rect: layout.world_layer_rect(),
        transparent: None,
        sharp: true,
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
        if self.launcher.is_some() {
            return;
        }
        let Some(log) = &mut self.input_log.0 else {
            return;
        };
        if let Err(error) = input::write_command(log, &mut self.ids, &command) {
            self.input_log.stop(&*error);
        }
    }
    /// The display scaling reaches the log, so a replay maps the pointer
    /// through the same viewport.
    fn log_scaling(&mut self, event_loop: &ActiveEventLoop) {
        let aspect_fit = self.scaling == ScalingMode::AspectFit;
        self.log(event_loop, InputCommand::Scaling { aspect_fit });
    }
    /// An enhanced session's settings reach the log after the surface and
    /// scaling, so a replay presents the same layers; a Vanilla session
    /// logs nothing here.
    fn log_enhanced(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(settings) = self.enhanced {
            self.log(
                event_loop,
                InputCommand::Enhanced {
                    settings: settings.into(),
                },
            );
        }
    }
    /// The F9 overlay's committed commands: each is logged, then applied,
    /// so a replay of the log applies it at the same point, exactly once.
    fn drain_debug(&mut self, event_loop: &ActiveEventLoop) {
        let Some(preview) = self.preview.as_mut() else {
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
        let handled = self.preview_key(event_loop, window, key);
        if !handled
            && key == KeyCode::Escape
            && self
                .preview
                .as_ref()
                .is_none_or(preview::AssetPreview::escape_exits)
        {
            self.log(event_loop, InputCommand::Quit);
            event_loop.exit();
        }
    }
    fn preview_key(&mut self, event_loop: &ActiveEventLoop, window: &Window, key: KeyCode) -> bool {
        let Some(preview) = &mut self.preview else {
            return false;
        };
        if !preview.step(key) {
            return false;
        }
        self.refresh_preview(event_loop, window);
        true
    }
    fn refresh_preview(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        if self.enhanced_layout(window).is_some() {
            return self.refresh_enhanced(event_loop, window);
        }
        self.shown = None;
        let Some(preview) = &mut self.preview else {
            return;
        };
        match preview.image() {
            Ok(image) => {
                if let Some(renderer) = &mut self.renderer {
                    let palette = preview.display_override().unwrap_or_else(|| {
                        self.display
                            .as_ref()
                            .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
                    });
                    renderer.set_image(&image, &palette);
                }
                // The composed frame already shows the cursor where it is.
                self.pointer_moved = false;
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
    /// The enhanced presentation's refresh: the next redraw builds the
    /// frame for the window's layout then; the title and the pointer
    /// follow as in [`Shell::refresh_preview`].
    fn refresh_enhanced(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(preview) = &mut self.preview else {
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
        replay::enhanced_layout(self.preview.as_mut()?, [size.width, size.height])
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
            let Some(preview) = &mut self.preview else {
                return false;
            };
            match preview.enhanced_frame(&layout) {
                Some(Ok(frame)) => {
                    let sides = frame
                        .side_panel
                        .as_ref()
                        .map(|panel| side_tiles(&layout, panel))
                        .unwrap_or_default();
                    self.shown = Some(EnhancedShown {
                        layout,
                        frame,
                        sides,
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
                            if let Some(renderer) = &mut self.renderer {
                                renderer.set_image(&image, &palette);
                            }
                        }
                        Err(error) => self.fail(event_loop, error),
                    }
                    return false;
                }
            }
        }
        let (Some(shown), Some(preview)) = (&self.shown, &self.preview) else {
            return false;
        };
        let palette = self.palette();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_layers(&enhanced_layers(shown, self.pointer), &palette);
        }
        // The frame draws its own pointer art: the OS pointer hides then.
        window.set_cursor_visible(shown.frame.cursor.is_none() && preview.system_cursor());
        true
    }
    /// The display palette the frame is drawn through.
    fn palette(&self) -> [[u8; 4]; 256] {
        let Some(preview) = &self.preview else {
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
        let Some(preview) = &mut self.preview else {
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
            .preview
            .as_mut()
            .and_then(preview::AssetPreview::enhanced_settings)
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
        if let Some(preview) = &mut self.preview {
            preview.set_enhanced(Some(settings));
        }
        self.refresh_preview(event_loop, window);
    }
    /// The drawn cursor moved: the next redraw presents it.
    fn refresh_pointer(&mut self, window: &Window) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| !preview.system_cursor())
        {
            self.pointer_moved = true;
            window.request_redraw();
        }
    }
    /// Present the drawn cursor's move without composing the scene again.
    fn present_pointer(&mut self) {
        let Some(preview) = &mut self.preview else {
            return;
        };
        let Some(image) = preview.pointer_frame() else {
            return;
        };
        if let Some(renderer) = &mut self.renderer {
            let palette = preview.display_override().unwrap_or_else(|| {
                self.display
                    .as_ref()
                    .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
            });
            renderer.set_image(&image, &palette);
        }
    }
    /// G-31 / SPEC-014 §3.28: the action menu's release moves the system
    /// pointer to the frame pixel the preview asks for (where supported).
    fn warp_pointer(&mut self, window: &Window) {
        let Some(pixel) = self
            .preview
            .as_mut()
            .and_then(preview::AssetPreview::take_warp)
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
        let (scale, software) = self.preview.as_mut().map_or((1.0, false), |preview| {
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
        let Some(hotkey) = shell_hotkey(key, self.alt, self.enhanced.is_some()) else {
            return false;
        };
        if !self.shell_keys.contains(&key) {
            self.shell_keys.push(key);
        }
        match hotkey {
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
                self.log_scaling(event_loop);
            }
            ShellHotkey::Viewport => self.toggle_viewport(event_loop, window),
        }
        self.viewport_changed(event_loop, window);
        true
    }
    /// M-P1: the frame's viewport changed (a resize, or a scaling mode or
    /// fullscreen change): the stored pointer maps to another frame pixel,
    /// so hover runs again and the drawn cursor follows, and the next click
    /// acts where the cursor shows.
    fn viewport_changed(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
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
                .preview
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
            .and_then(|point| self.preview.as_mut()?.pointer_moved(point));
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

impl Shell {
    fn refresh_launcher(&mut self, window: &Window) {
        let Some(launcher) = &self.launcher else {
            return;
        };
        if let Some(renderer) = &mut self.renderer {
            renderer.set_image(&launcher.image(), &launcher::palette());
        }
        window.request_redraw();
    }

    /// A window event while the launcher shows: its input goes to the
    /// launcher (never to the input log); the window's own events come
    /// back for the shared handling.
    fn launcher_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        event: WindowEvent,
    ) -> Option<WindowEvent> {
        let input = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return None;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return None;
                };
                let pressed = event.state == ElementState::Pressed;
                if self.shell_key(event_loop, window, key, pressed, event.repeat) || !pressed {
                    return None;
                }
                launcher::LauncherInput::Key {
                    key,
                    shift: self.shift,
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.alt = modifiers.state().alt_key();
                self.shift = modifiers.state().shift_key();
                return None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some(self.scaled_pointer(window, [position.x, position.y]));
                launcher::LauncherInput::Pointer(self.pixel()?)
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.virtual_pointer.left();
                return None;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => launcher::LauncherInput::Press(self.pixel()?),
            WindowEvent::MouseInput { .. } => return None,
            WindowEvent::MouseWheel { delta, .. } => launcher::LauncherInput::Wheel(match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                MouseScrollDelta::PixelDelta(position) => position.y / 40.0,
            }),
            WindowEvent::DroppedFile(path) => launcher::LauncherInput::Dropped(path),
            WindowEvent::Focused(false) => {
                self.shell_keys.clear();
                return None;
            }
            other => return Some(other),
        };
        let action = self.launcher.as_mut()?.handle(input);
        match action {
            None => self.refresh_launcher(window),
            Some(launcher::LauncherAction::Quit) => event_loop.exit(),
            Some(launcher::LauncherAction::Play) => self.start_game(event_loop, window, false),
            Some(launcher::LauncherAction::PlayDebug) => self.start_game(event_loop, window, true),
        }
        None
    }

    /// Play: the launcher's settings start the game exactly as a
    /// zero-argument start with those settings would, in this window and
    /// renderer. When it does not load, the launcher stays and says why.
    fn start_game(&mut self, event_loop: &ActiveEventLoop, window: &Window, debug: bool) {
        let (Some(launcher), Some(pending)) = (self.launcher.as_mut(), self.pending.as_ref())
        else {
            return;
        };
        let config = launcher.launch_config();
        let mut args = play_args(pending, debug);
        let (launch, preview, audio) = match start(&mut args, Some(&config)) {
            Ok((launch, Some(preview), audio)) => (launch, preview, audio),
            Ok((_, None, _)) => {
                launcher.launch_failed("the settings name no game data");
                return self.refresh_launcher(window);
            }
            Err(error) => {
                tracing::error!(%error, "the launcher's game did not start");
                launcher.launch_failed(error);
                return self.refresh_launcher(window);
            }
        };
        tracing::info!(debug = args.preview.debug, "launcher: starting the game");
        self.launcher = None;
        self.pending = None;
        self._audio = audio;
        self.display = Some(DisplayPalette::new(preview.palette(), preview.brightness()));
        self.preview = Some(preview);
        // The game's clock starts at its first frame, as in a direct start.
        self.started = Some(Instant::now());
        self.scaling = launch.scaling;
        self.enhanced = launch.enhanced;
        if let Some(renderer) = &mut self.renderer {
            renderer.set_scaling(self.scaling);
        }
        if launch.fullscreen && window.fullscreen().is_none() {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        // `--record-inputs` starts here, with what a direct start logs first.
        let size = window.inner_size();
        self.log(
            event_loop,
            InputCommand::Surface {
                width: size.width,
                height: size.height,
            },
        );
        self.log_scaling(event_loop);
        self.log_enhanced(event_loop);
        self.refresh_preview(event_loop, window);
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
            .with_title(match (&self.preview, &self.launcher) {
                (Some(preview), _) => preview.title(),
                (None, Some(launcher)) => launcher.title(),
                (None, None) => "Vault Engine — Indexed Color Test".to_owned(),
            })
            .with_inner_size(PhysicalSize::new(width, height))
            // H2: the frame is never down-sampled in a window that keeps
            // its minimum size.
            .with_min_inner_size(PhysicalSize::new(640, 480))
            .with_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
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
        let renderer = (|| -> AppResult<Renderer> {
            if let Some(preview) = &mut self.preview {
                let image = preview.image()?;
                let palette = preview.display_override().unwrap_or_else(|| {
                    self.display
                        .as_ref()
                        .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
                });
                Ok(pollster::block_on(Renderer::new_with_image(
                    window.clone(),
                    size.width,
                    size.height,
                    &image,
                    &palette,
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
                self.log_scaling(event_loop);
                self.log_enhanced(event_loop);
            }
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        }
        if let Some(preview) = &self.preview {
            window.set_cursor_visible(preview.system_cursor());
        }
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
        // The F9 overlay's commands follow the inputs that committed them.
        self.drain_debug(event_loop);
        // The commands logged since the last pass.
        self.flush_log(event_loop);
        let Some(started) = self.started.filter(|_| self.preview.is_some()) else {
            return;
        };
        let now = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        // D-11: every frame's clock is logged, so a replay ticks the preview
        // at the same times between the same commands.
        self.log(event_loop, InputCommand::Tick { now_ms: now });
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let changed = match preview.tick(now) {
            Ok(changed) => changed,
            Err(error) => return self.fail(event_loop, error),
        };
        if preview.finished() {
            tracing::info!("movie finished");
            event_loop.exit();
            return;
        }
        // SPEC-014 §3.29: held keys repeat on the frame clock.
        let due = self.repeat.poll(now, preview.keyboard());
        if let Some(window) = self.window.clone() {
            for key in due {
                self.press_key(event_loop, &window, key);
            }
        }
        let Some(preview) = self.preview.as_mut() else {
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
        let event = if self.launcher.is_some() {
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
                self.log(
                    event_loop,
                    InputCommand::Key {
                        physical_key: match event.physical_key {
                            PhysicalKey::Code(code) => format!("{code:?}"),
                            PhysicalKey::Unidentified(code) => format!("Unidentified({code:?})"),
                        },
                        pressed,
                        repeat: event.repeat,
                    },
                );
                // SPEC-014 §3.29: the engine repeats a held key itself, so
                // the operating system's repeats are only logged.
                let PhysicalKey::Code(key) = event.physical_key else {
                    return;
                };
                if !pressed {
                    self.repeat.release(key);
                    if let Some(preview) = &mut self.preview {
                        preview.key_up(key);
                    }
                } else if !event.repeat {
                    if let Some(preview) = &self.preview {
                        self.repeat.press(key, preview.keyboard());
                    }
                    self.press_key(event_loop, &window, key);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // The log records the game's pointer, so a replay needs no
                // sensitivity of its own.
                let [x, y] = self.scaled_pointer(&window, [position.x, position.y]);
                self.pointer = Some([x, y]);
                self.log_hover(event_loop, [x, y]);
                self.log(event_loop, InputCommand::PointerMoved { x, y });
                self.refresh_pointer(&window);
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.virtual_pointer.left();
                if let Some(preview) = &mut self.preview {
                    preview.pointer_left();
                }
                self.log(event_loop, InputCommand::PointerLeft);
                self.refresh_pointer(&window);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.log(
                    event_loop,
                    InputCommand::PointerButton {
                        button: format!("{button:?}"),
                        pressed: state == ElementState::Pressed,
                    },
                );
                if self.enhanced_button(event_loop, &window, button, state == ElementState::Pressed)
                {
                    return;
                }
                // H3: presses and releases map with the same clamping as
                // hover, so the letterbox acts at the nearest frame pixel.
                let pixel = self.clamped_pixel();
                let pressed = state == ElementState::Pressed;
                // SPEC-012 §3.12 (G-31): a movie counts the buttons the
                // frame paths below never see, a middle or extra button
                // and a press in the letterbox around the frame.
                let framed = matches!(button, MouseButton::Left | MouseButton::Right);
                if (!framed || pressed && self.pixel().is_none())
                    && self
                        .preview
                        .as_mut()
                        .is_some_and(|preview| preview.movie_button(pressed))
                {
                    self.refresh_preview(event_loop, &window);
                    return;
                }
                if state == ElementState::Released
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                    && self
                        .preview
                        .as_mut()
                        .is_some_and(|preview| preview.release(pixel, button == MouseButton::Right))
                {
                    self.refresh_preview(event_loop, &window);
                    self.warp_pointer(&window);
                }
                if state == ElementState::Pressed
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                    && let Some(pixel) = pixel
                    && self
                        .preview
                        .as_mut()
                        .is_some_and(|preview| preview.click(pixel, button == MouseButton::Right))
                {
                    self.refresh_preview(event_loop, &window);
                }
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
                self.log(event_loop, InputCommand::Wheel { x, y, unit });
                let at = self
                    .enhanced_layout(&window)
                    .map(|layout| replay::pointer_at(&layout, self.pointer));
                if self.preview.as_mut().is_some_and(|preview| match at {
                    Some(at) => preview.enhanced_wheel(at, replay::wheel_lines(y, unit)),
                    None => preview.scroll(y),
                }) {
                    self.refresh_preview(event_loop, &window);
                }
            }
            WindowEvent::Focused(focused) => {
                // Keys held when the focus goes stop repeating, and (M-P6)
                // held mouse presses are dropped: their releases go to
                // another window.
                if !focused {
                    self.repeat.clear();
                    // Their releases go elsewhere too.
                    self.shell_keys.clear();
                    if let Some(preview) = &mut self.preview {
                        preview.focus_lost();
                    }
                }
                self.log(event_loop, InputCommand::Focus { focused });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
                self.alt = modifiers.alt_key();
                if let Some(preview) = &mut self.preview {
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
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::RedrawRequested => {
                // The enhanced layers draw the cursor themselves.
                let enhanced = self.present_enhanced(event_loop, &window);
                if std::mem::take(&mut self.pointer_moved) && !enhanced {
                    self.present_pointer();
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

fn run(mut args: Args) -> AppResult<()> {
    if let Some(path) = &args.replay_inputs {
        args.preview.headless = true;
        let preview = args
            .preview
            .load()?
            .ok_or("--replay-inputs requires a preview asset")?;
        let log =
            std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let report = replay::replay(
            preview,
            std::io::BufReader::new(log),
            args.replay_frame.as_deref(),
        )?;
        tracing::info!(
            commands = report.commands,
            elapsed_ms = report.elapsed_ms,
            "replay finished"
        );
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    // A start naming nothing may show the launcher instead, which also
    // reports a malformed settings file rather than failing on it.
    let bare = args.launcher || (!args.names_asset() && !args.smoke_test);
    let (config, problem) = match settings(&args) {
        Ok(config) => (config, None),
        Err(error) if bare => (None, Some(error.to_string())),
        Err(error) => return Err(error),
    };
    let launcher = bare
        .then(|| launcher_for(&args, config.as_ref(), problem))
        .flatten();
    let input_log = InputLog::open(args.record_inputs.as_deref())?;
    let mut shell = Shell {
        smoke_test: args.smoke_test,
        input_log,
        ..Default::default()
    };
    if let Some(launcher) = launcher {
        shell.scaling = resolve_launch(&mut args.clone(), config.as_ref()).scaling;
        shell.launcher = Some(Box::new(launcher));
        shell.pending = Some(Box::new(args));
    } else {
        let (launch, preview, audio) = start(&mut args, config.as_ref())?;
        shell.scaling = launch.scaling;
        shell.enhanced = launch.enhanced;
        shell.fullscreen = launch.fullscreen;
        shell._audio = audio;
        shell.display = preview
            .as_ref()
            .map(|preview| DisplayPalette::new(preview.palette(), preview.brightness()));
        shell.preview = preview;
    }
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let run = event_loop.run_app(&mut shell);
    if let Some(Err(error)) = shell.input_log.0.as_mut().map(std::io::Write::flush) {
        shell.input_log.stop(&error);
    }
    run?;
    if let Some(error) = shell.failure {
        return Err(error);
    }
    if shell.smoke_test && !shell.frame_presented {
        return Err("smoke test exited before presenting a frame".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "exiting");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("vault-platform").chain(args.iter().copied())).unwrap()
    }

    fn installed() -> config::Config {
        config::Config {
            master_dat: Some("/games/fallout/MASTER.DAT".into()),
            critter_dat: Some("/games/fallout/CRITTER.DAT".into()),
            data_root: Some("/games/fallout/DATA".into()),
            scaling: Some(ScalingMode::AspectFit),
            fullscreen: Some(true),
            debug: Some(true),
            preset: Some(config::Preset::Enhanced),
            ..Default::default()
        }
    }

    #[test]
    fn no_arguments_start_the_game_from_the_settings() {
        let mut args = parse(&[]);
        assert!(args.wants_data());
        let launch = resolve_launch(&mut args, Some(&installed()));
        assert!(launch.zero_arg_game);
        assert!(args.preview.game && args.preview.debug);
        assert_eq!(
            args.preview.master_dat,
            Some("/games/fallout/MASTER.DAT".into())
        );
        assert_eq!(
            args.preview.critter_dat,
            Some("/games/fallout/CRITTER.DAT".into())
        );
        assert_eq!(args.preview.corpus_dir, Some("/games/fallout/DATA".into()));
        assert_eq!(
            launch,
            Launch {
                scaling: ScalingMode::AspectFit,
                fullscreen: true,
                preset: config::Preset::Enhanced,
                enhanced: Some(enhanced::EnhancedSettings::default()),
                mod_list: None,
                zero_arg_game: true,
                audio: true,
            }
        );
        // Without settings, or with settings naming no data, the color test
        // shows as before.
        let mut args = parse(&[]);
        let launch = resolve_launch(&mut args, None);
        assert!(!launch.zero_arg_game && !args.names_asset());
        assert_eq!(
            (launch.scaling, launch.fullscreen, launch.preset),
            (ScalingMode::AspectFit, false, config::Preset::Vanilla)
        );
        let display_only = config::Config {
            fullscreen: Some(true),
            master_dat: Some("/only/MASTER.DAT".into()),
            ..Default::default()
        };
        let mut args = parse(&[]);
        let launch = resolve_launch(&mut args, Some(&display_only));
        assert!(!launch.zero_arg_game && !args.names_asset() && launch.fullscreen);
        // A loose data directory alone is game data.
        let loose = config::Config {
            data_root: Some("/unpacked".into()),
            ..Default::default()
        };
        let mut args = parse(&[]);
        assert!(resolve_launch(&mut args, Some(&loose)).zero_arg_game);
        assert_eq!(
            (args.preview.corpus_dir, args.preview.master_dat),
            (Some("/unpacked".into()), None)
        );
    }

    #[test]
    fn command_line_flags_win_over_the_settings() {
        let mut args = parse(&[
            "--game",
            "--master-dat",
            "/cli/master.dat",
            "--critter-dat",
            "/cli/critter.dat",
            "--scaling",
            "integer",
            "--preset",
            "vanilla",
        ]);
        assert!(!args.wants_data());
        let launch = resolve_launch(&mut args, Some(&installed()));
        assert!(!launch.zero_arg_game);
        // The command line's source is used whole: no settings data mixes in.
        assert_eq!(args.preview.master_dat, Some("/cli/master.dat".into()));
        assert_eq!(args.preview.critter_dat, Some("/cli/critter.dat".into()));
        assert_eq!(args.preview.corpus_dir, None);
        assert_eq!(launch.scaling, ScalingMode::Integer);
        assert_eq!(launch.preset, config::Preset::Vanilla);
        // Unset flags take the settings.
        assert!(launch.fullscreen && args.preview.debug);
        // `--game` without a source takes the settings' data.
        let mut args = parse(&["--game", "--scaling", "aspect_fit"]);
        let launch = resolve_launch(&mut args, Some(&installed()));
        assert!(!launch.zero_arg_game);
        assert_eq!(
            args.preview.master_dat,
            Some("/games/fallout/MASTER.DAT".into())
        );
        assert_eq!(launch.scaling, ScalingMode::AspectFit);
        // A map preview with its own corpus keeps it.
        let mut args = parse(&["--corpus-dir", "/cli", "--preview-map", "m.map"]);
        resolve_launch(&mut args, Some(&installed()));
        assert!(!args.preview.game);
        assert_eq!(
            (args.preview.corpus_dir, args.preview.master_dat),
            (Some("/cli".into()), None)
        );
        // A bare smoke test stays the color test.
        let mut args = parse(&["--smoke-test"]);
        assert!(!args.wants_data());
        assert!(!resolve_launch(&mut args, Some(&installed())).zero_arg_game);
        assert!(!args.preview.game);
        // A replay takes nothing from the settings.
        let mut args = parse(&["--replay-inputs", "log.jsonl", "--preview-map", "m.map"]);
        let launch = resolve_launch(&mut args, Some(&installed()));
        assert!(!args.preview.debug && args.preview.master_dat.is_none());
        // (Its own pointer mapping follows the log's `scaling` records.)
        assert_eq!(
            (launch.scaling, launch.fullscreen),
            (ScalingMode::AspectFit, false)
        );
        // The value flags check their values.
        for bad in [["--scaling", "stretch"], ["--preset", "modern"]] {
            assert!(Args::try_parse_from(["vault-platform", bad[0], bad[1]]).is_err());
        }
        assert!(
            Args::try_parse_from([
                "vault-platform",
                "--record-inputs",
                "-",
                "--replay-inputs",
                "log.jsonl",
                "--preview-map",
                "m.map"
            ])
            .is_err()
        );
    }

    #[test]
    fn the_launchers_play_builds_the_zero_argument_launch() {
        let shown = parse(&["--launcher", "--scaling", "integer", "--no-audio"]);
        assert!(shown.launcher && shown.wants_data() && !shown.names_asset());
        // What the launcher saved and hands over (its fields made absolute).
        let chosen = config::Config {
            scaling: Some(ScalingMode::AspectFit),
            audio: Some(false),
            debug: None,
            mod_list: Some(config::ModList::File(
                "/games/fallout/load_order.toml".into(),
            )),
            ..installed()
        };
        let mut args = play_args(&shown, false);
        let launch = resolve_launch(&mut args, Some(&chosen));
        assert!(args.preview.game && launch.zero_arg_game && !args.preview.debug);
        assert_eq!(
            (
                args.preview.master_dat,
                args.preview.critter_dat,
                args.preview.corpus_dir
            ),
            (
                Some("/games/fallout/MASTER.DAT".into()),
                Some("/games/fallout/CRITTER.DAT".into()),
                Some("/games/fallout/DATA".into())
            )
        );
        // The launcher's choices, not the flags it was shown with.
        assert_eq!(
            launch,
            Launch {
                scaling: ScalingMode::AspectFit,
                fullscreen: true,
                preset: config::Preset::Enhanced,
                enhanced: Some(enhanced::EnhancedSettings::default()),
                mod_list: chosen.mod_list.clone(),
                zero_arg_game: true,
                audio: false,
            }
        );
        // Launch with Debug, and audio back on.
        let mut args = play_args(&shown, true);
        let on = config::Config {
            audio: Some(true),
            ..chosen
        };
        let launch = resolve_launch(&mut args, Some(&on));
        assert!(args.preview.debug && launch.audio);
        // `--no-audio` still wins over the settings without the launcher.
        let mut args = parse(&["--no-audio"]);
        assert!(!resolve_launch(&mut args, Some(&on)).audio);
        // The launcher is never shown for a replay, a smoke test or an asset.
        for other in [
            &["--smoke-test"][..],
            &["--game"],
            &["--preview-map", "m.map"],
            &["--replay-inputs", "log.jsonl", "--preview-map", "m.map"],
        ] {
            let line = std::iter::once("vault-platform")
                .chain(["--launcher"])
                .chain(other.iter().copied());
            assert!(Args::try_parse_from(line).is_err(), "{other:?}");
        }
    }

    #[test]
    fn the_input_log_is_written_only_when_asked_for() {
        use std::io::Write;
        assert!(InputLog::open(None).unwrap().0.is_none());
        assert!(parse(&[]).record_inputs.is_none());
        assert_eq!(
            parse(&["--record-inputs", "-"]).record_inputs,
            Some(PathBuf::from("-"))
        );
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inputs.jsonl");
        let mut log = InputLog::open(Some(&path)).unwrap();
        let mut ids = SequenceIds::default();
        input::write_command(log.0.as_mut().unwrap(), &mut ids, &InputCommand::Quit).unwrap();
        log.0.as_mut().unwrap().flush().unwrap();
        drop(log);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"command_id\":1,\"kind\":\"quit\"}\n"
        );
        assert!(InputLog::open(Some(&temp.path().join("missing/inputs.jsonl"))).is_err());
    }

    #[test]
    fn the_window_opens_at_scale_two_or_the_largest_fitting_multiple() {
        assert_eq!(initial_window_size(None), [1280, 960]);
        assert_eq!(initial_window_size(Some([0, 0])), [1280, 960]);
        assert_eq!(initial_window_size(Some([3840, 2160])), [1280, 960]);
        assert_eq!(initial_window_size(Some([1280, 960])), [1280, 960]);
        assert_eq!(initial_window_size(Some([1366, 768])), [640, 480]);
        assert_eq!(initial_window_size(Some([1279, 1024])), [640, 480]);
        assert_eq!(initial_window_size(Some([800, 400])), [640, 480]);
    }

    #[test]
    fn the_shell_hotkeys_leave_f10_to_the_game() {
        assert_eq!(
            shell_hotkey(KeyCode::F11, false, false),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::Enter, true, false),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::NumpadEnter, true, false),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::KeyS, true, false),
            Some(ShellHotkey::Scaling)
        );
        // SPEC-014: F10 is the game's quit confirmation; plain Enter and S
        // are the game's keys.
        for alt in [false, true] {
            assert_eq!(shell_hotkey(KeyCode::F10, alt, false), None);
        }
        assert_eq!(shell_hotkey(KeyCode::Enter, false, false), None);
        assert_eq!(shell_hotkey(KeyCode::KeyS, false, false), None);
        // Alt+V is the shell's in an enhanced session only; V and Alt+V
        // otherwise reach the game as before.
        assert_eq!(
            shell_hotkey(KeyCode::KeyV, true, true),
            Some(ShellHotkey::Viewport)
        );
        assert_eq!(shell_hotkey(KeyCode::KeyV, true, false), None);
        assert_eq!(shell_hotkey(KeyCode::KeyV, false, true), None);
        assert_eq!(
            shell_hotkey(KeyCode::KeyS, true, true),
            Some(ShellHotkey::Scaling)
        );
    }

    #[test]
    fn the_enhanced_settings_reach_the_launch_with_the_enhanced_preset_only() {
        let settings = config::Config {
            viewport_mode: Some(enhanced::ViewportMode::Fixed),
            default_zoom: Some(1.5),
            roof_peek: Some(enhanced::RoofPeek::Hidden),
            ..installed()
        };
        let mut args = parse(&[]);
        let launch = resolve_launch(&mut args, Some(&settings));
        let expected = enhanced::EnhancedSettings {
            viewport: enhanced::ViewportMode::Fixed,
            default_zoom: 1.5,
            roof_peek: enhanced::RoofPeek::Hidden,
            ..Default::default()
        };
        assert_eq!(launch.enhanced, Some(expected));
        // The flags win over the file.
        let mut args = parse(&["--viewport", "unlocked_widescreen", "--zoom", "2.5"]);
        let launch = resolve_launch(&mut args, Some(&settings));
        assert_eq!(
            launch.enhanced,
            Some(enhanced::EnhancedSettings {
                viewport: enhanced::ViewportMode::Unlocked,
                default_zoom: 2.5,
                ..expected
            })
        );
        // Vanilla carries none, whatever the file and flags say.
        let mut args = parse(&["--preset", "vanilla", "--zoom", "2"]);
        assert_eq!(resolve_launch(&mut args, Some(&settings)).enhanced, None);
        assert_eq!(resolve_launch(&mut parse(&[]), None).enhanced, None);
        // The values are checked, and Play drops the flags as it does the
        // other display flags.
        for bad in [["--viewport", "wide"], ["--zoom", "4"], ["--zoom", "x"]] {
            assert!(Args::try_parse_from(["vault-platform", bad[0], bad[1]]).is_err());
        }
        let shown = parse(&["--launcher", "--viewport", "fixed_640x480", "--zoom", "3"]);
        let args = play_args(&shown, false);
        assert_eq!((args.viewport, args.zoom), (None, None));
    }

    fn image(width: u32, height: u32, pixels: impl IntoIterator<Item = u8>) -> IndexedImage {
        IndexedImage::from_pixels(width, height, pixels.into_iter().collect()).unwrap()
    }

    #[test]
    fn side_panels_tile_from_the_bar_outward() {
        // 1000 x 480 at interface scale 1: the 640-pixel frame at x 180,
        // so each side is 180 x 100 interface pixels.
        let layout = EnhancedLayout::compute([1000, 480], Some(1.0), 1.0).unwrap();
        let panel = image(4, 2, [1, 2, 3, 4, 5, 6, 7, 8]);
        let tiles = side_tiles(&layout, &panel);
        assert_eq!(tiles.len(), 2);
        let (left, left_rect) = &tiles[0];
        let (right, right_rect) = &tiles[1];
        assert_eq!((left.width(), left.height()), (180, 100));
        assert_eq!(*left_rect, [0.0, 380.0, 180.0, 100.0]);
        assert_eq!(*right_rect, [820.0, 380.0, 180.0, 100.0]);
        // The left tile ends on the panel's last column next to the bar,
        // and the right one starts on its first; rows repeat.
        assert_eq!(&left.pixels()[176..180], &[1, 2, 3, 4]);
        assert_eq!(&right.pixels()[..4], &[1, 2, 3, 4]);
        assert_eq!(&right.pixels()[180..184], &[5, 6, 7, 8]);
        assert_eq!(&right.pixels()[360..364], &[1, 2, 3, 4]);
        // At a fractional scale the tile rounds up and overhangs outward.
        let layout = EnhancedLayout::compute([1001, 720], Some(1.5), 1.0).unwrap();
        let tiles = side_tiles(&layout, &panel);
        let [x, _, w, h] = layout.side_rects[0];
        let (left, rect) = &tiles[0];
        assert_eq!(left.width(), (w / 1.5).ceil() as u32);
        assert_eq!(rect[0] + rect[2], x + w);
        assert_eq!(rect[3], h);
        // A frame spanning the window has no side panels.
        let layout = EnhancedLayout::compute([640, 480], None, 1.0).unwrap();
        assert!(side_tiles(&layout, &panel).is_empty());
    }

    #[test]
    fn the_enhanced_layers_go_world_sides_interface_cursor() {
        let layout = EnhancedLayout::compute([1280, 480], Some(1.0), 2.0).unwrap();
        let frame = enhanced::EnhancedFrame {
            world: IndexedImage::new(layout.world_size[0], layout.world_size[1]).unwrap(),
            ui: IndexedImage::new(640, 480).unwrap(),
            ui_transparent: Some(0),
            side_panel: Some(image(2, 1, [9, 9])),
            cursor: Some((image(3, 2, [0; 6]), [1, 2])),
        };
        let sides = side_tiles(&layout, frame.side_panel.as_ref().unwrap());
        let shown = EnhancedShown {
            layout,
            frame,
            sides,
        };
        let layers = enhanced_layers(&shown, Some([700.0, 300.0]));
        let rects: Vec<[f32; 4]> = layers.iter().map(|layer| layer.rect).collect();
        assert_eq!(
            rects,
            [
                layout.world_layer_rect(),
                [0.0, 380.0, 320.0, 100.0],
                [960.0, 380.0, 320.0, 100.0],
                layout.ui_rect,
                [699.0, 298.0, 3.0, 2.0],
            ]
        );
        let transparent: Vec<Option<u8>> = layers.iter().map(|layer| layer.transparent).collect();
        assert_eq!(transparent, [None, None, None, Some(0), Some(0)]);
        assert!(layers.iter().all(|layer| layer.sharp));
        // Without a pointer there is no cursor layer.
        assert_eq!(enhanced_layers(&shown, None).len(), 4);
        // At interface scale 2 the cursor's hotspot and size double.
        let layout = EnhancedLayout::compute([1280, 960], Some(2.0), 1.0).unwrap();
        let shown = EnhancedShown { layout, ..shown };
        let layers = enhanced_layers(&shown, Some([700.0, 300.0]));
        assert_eq!(layers.last().unwrap().rect, [698.0, 296.0, 6.0, 4.0]);
    }
}
