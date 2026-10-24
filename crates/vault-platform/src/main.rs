mod audio;
mod config;
mod editor_screen;
mod game;
mod input;
mod interface;
mod map_preview;
mod movie;
mod preview;
mod replay;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
#[cfg(test)]
pub(crate) use map_preview::fixture as fixture_world;

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
use vault_render::{Renderer, ScalingMode};
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

#[derive(Parser)]
#[command(
    about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives",
    long_about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives.\n\nWithout arguments the game starts from the data named in vault-engine.toml (beside the executable, else in the working directory). Without that file, MASTER.DAT and CRITTER.DAT are looked for in the working directory, beside the executable, in ../local-corpus and in the usual Steam and GOG install directories, and what is found is saved to vault-engine.toml. Command-line flags always win over the file.\n\nF11 or Alt+Enter toggles borderless fullscreen; Alt+S toggles aspect-fit and integer scaling."
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
    #[command(flatten)]
    preview: preview::PreviewArgs,
}

fn scaling_arg(text: &str) -> Result<ScalingMode, String> {
    config::parse_scaling(text).ok_or_else(|| "expected aspect_fit or integer".into())
}
fn preset_arg(text: &str) -> Result<config::Preset, String> {
    config::Preset::parse(text).ok_or_else(|| "expected vanilla or enhanced".into())
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct Launch {
    scaling: ScalingMode,
    fullscreen: bool,
    preset: config::Preset,
    /// The settings' mod list, for the engine to load.
    mod_list: Option<config::ModList>,
    /// No asset was named and the settings' game data starts the game.
    zero_arg_game: bool,
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
    Launch {
        scaling: args
            .scaling
            .or(config.scaling)
            .unwrap_or(ScalingMode::AspectFit),
        fullscreen: args.fullscreen || config.fullscreen.unwrap_or(false),
        preset: args.preset.or(config.preset).unwrap_or_default(),
        mod_list: config.mod_list.clone(),
        zero_arg_game,
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
}

/// The shell hotkey `key` is with Alt held or not. F10 is never one: it
/// is the game's quit-confirmation key (SPEC-014).
fn shell_hotkey(key: KeyCode, alt: bool) -> Option<ShellHotkey> {
    match key {
        KeyCode::F11 => Some(ShellHotkey::Fullscreen),
        KeyCode::Enter | KeyCode::NumpadEnter if alt => Some(ShellHotkey::Fullscreen),
        KeyCode::KeyS if alt => Some(ShellHotkey::Scaling),
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
}

impl Shell {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<AppError>) {
        let error = error.into();
        tracing::error!(%error, "desktop shell failed");
        self.failure = Some(error);
        event_loop.exit();
    }

    fn log(&mut self, _event_loop: &ActiveEventLoop, command: InputCommand) {
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
        // that shows the frame pixel, never in the letterbox.
        let Some([x, y]) = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.surface_point(pixel))
        else {
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
    /// mode. The shell keeps these keys: neither their presses (with the
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
        let Some(hotkey) = shell_hotkey(key, self.alt) else {
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
            .with_title(self.preview.as_ref().map_or_else(
                || "Vault Engine — Indexed Color Test".to_owned(),
                preview::AssetPreview::title,
            ))
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
                if self
                    .preview
                    .as_mut()
                    .is_some_and(|preview| preview.scroll(y))
                {
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
                if std::mem::take(&mut self.pointer_moved) {
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
    let config = settings(&args)?;
    let launch = resolve_launch(&mut args, config.as_ref());
    if launch.zero_arg_game {
        tracing::info!("starting the game from the settings' data");
    } else if !args.names_asset() && !args.smoke_test {
        tracing::warn!(
            "no game data found: name it in {} beside the executable or in the working \
             directory (master_dat = \"/path/to/MASTER.DAT\", critter_dat = \
             \"/path/to/CRITTER.DAT\"), put MASTER.DAT and CRITTER.DAT in the working \
             directory, or pass --game --master-dat PATH --critter-dat PATH; showing the \
             color test",
            config::FILE_NAME
        );
    }
    args.preview.preset = match launch.preset {
        config::Preset::Vanilla => vault_core::EnginePreset::Vanilla,
        config::Preset::Enhanced => vault_core::EnginePreset::Enhanced,
    };
    args.preview.mods = launch.mod_list.clone();
    let input_log = InputLog::open(args.record_inputs.as_deref())?;
    let mut preview = args.preview.load()?;
    // Audio serves an interactive world or a movie; sprites need none.
    let output = || {
        if args.no_audio {
            audio::Audio::silent()
        } else {
            audio::Audio::open()
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
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell {
        smoke_test: args.smoke_test,
        _audio: audio,
        input_log,
        scaling: launch.scaling,
        fullscreen: launch.fullscreen,
        ..Default::default()
    };
    shell.display = preview
        .as_ref()
        .map(|preview| DisplayPalette::new(preview.palette(), preview.brightness()));
    shell.preview = preview;
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
                mod_list: None,
                zero_arg_game: true,
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
            shell_hotkey(KeyCode::F11, false),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::Enter, true),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::NumpadEnter, true),
            Some(ShellHotkey::Fullscreen)
        );
        assert_eq!(
            shell_hotkey(KeyCode::KeyS, true),
            Some(ShellHotkey::Scaling)
        );
        // SPEC-014: F10 is the game's quit confirmation; plain Enter and S
        // are the game's keys.
        for alt in [false, true] {
            assert_eq!(shell_hotkey(KeyCode::F10, alt), None);
        }
        assert_eq!(shell_hotkey(KeyCode::Enter, false), None);
        assert_eq!(shell_hotkey(KeyCode::KeyS, false), None);
    }
}
