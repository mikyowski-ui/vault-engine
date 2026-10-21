mod audio;
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
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::Parser;
use input::InputCommand;
use tracing_subscriber::EnvFilter;
use vault_core::SequenceIds;
use vault_formats::{Palette, palette::PaletteCycler};
use vault_render::Renderer;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

type AppError = Box<dyn Error + Send + Sync>;
type AppResult<T> = Result<T, AppError>;

#[derive(Parser)]
#[command(
    about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives"
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
    #[command(flatten)]
    preview: preview::PreviewArgs,
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

/// The JSONL input log on stdout, buffered: P-02 one flush per event-loop
/// pass (and at exit) instead of one per input event.
struct InputLog(std::io::BufWriter<std::io::Stdout>);
impl Default for InputLog {
    fn default() -> Self {
        Self(std::io::BufWriter::new(std::io::stdout()))
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
    started: Option<Instant>,
    display: Option<DisplayPalette>,
    /// The output stream, kept alive for the whole session.
    _audio: Option<audio::Audio>,
    /// SPEC-014 §3.29 the engine's key repeat (the OS repeat is ignored).
    repeat: input::KeyRepeat<preview::KeyLayer>,
    input_log: InputLog,
    /// The window title last set (P-02: set only when it changes).
    title: String,
    /// The drawn cursor moved since the last presented frame; the next
    /// redraw presents it once, however many moves came in between (P-01).
    pointer_moved: bool,
}

impl Shell {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<AppError>) {
        let error = error.into();
        tracing::error!(%error, "desktop shell failed");
        self.failure = Some(error);
        event_loop.exit();
    }

    fn log(&mut self, event_loop: &ActiveEventLoop, command: InputCommand) {
        if let Err(error) = input::write_command(&mut self.input_log.0, &mut self.ids, &command) {
            self.fail(event_loop, error);
        }
    }
    fn flush_log(&mut self, event_loop: &ActiveEventLoop) {
        use std::io::Write;
        if let Err(error) = self.input_log.0.flush() {
            self.fail(event_loop, error);
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
        let size = window.inner_size();
        let Some([x, y]) = surface_point([size.width, size.height], [640, 480], pixel) else {
            return;
        };
        if let Err(error) = window.set_cursor_position(winit::dpi::PhysicalPosition::new(x, y)) {
            tracing::debug!(%error, "pointer warp unsupported");
        }
    }
    /// The frame pixel under the pointer, or `None` outside the frame.
    fn pixel(&self) -> Option<[i32; 2]> {
        self.pointer
            .and_then(|point| self.renderer.as_ref()?.image_position(point))
    }

    /// The pointer's frame pixel reaches the preview, clamped to the frame
    /// so its edge pixels stay reachable (SPEC-014 §3.29 edge scrolling).
    fn log_hover(&mut self, event_loop: &ActiveEventLoop, point: [f64; 2]) {
        let hover = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.image_position_clamped(point))
            .and_then(|pixel| self.preview.as_mut()?.hover(pixel));
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
        let attributes = Window::default_attributes()
            .with_title(self.preview.as_ref().map_or_else(
                || "Vault Engine — Indexed Color Test".to_owned(),
                preview::AssetPreview::title,
            ))
            .with_inner_size(LogicalSize::new(960, 640));
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
            Ok(renderer) => self.renderer = Some(renderer),
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
        // SPEC-014 §3.29: a held key repeats on the frame clock.
        if let Some(key) = self.repeat.poll(now, preview.key_layer())
            && let Some(window) = self.window.clone()
        {
            self.press_key(event_loop, &window, key);
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
                } else if !event.repeat {
                    if let Some(preview) = &self.preview {
                        self.repeat.press(key, preview.key_layer());
                    }
                    self.press_key(event_loop, &window, key);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Some([position.x, position.y]);
                self.log_hover(event_loop, [position.x, position.y]);
                self.log(
                    event_loop,
                    InputCommand::PointerMoved {
                        x: position.x,
                        y: position.y,
                    },
                );
                self.refresh_pointer(&window);
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
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
                let pixel = self.pixel();
                let pressed = state == ElementState::Pressed;
                // SPEC-012 §3.12 (G-31): a movie counts the buttons the
                // frame paths below never see, a middle or extra button
                // and a press in the letterbox around the frame.
                let framed = matches!(button, MouseButton::Left | MouseButton::Right);
                if (!framed || pressed && pixel.is_none())
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
                // Keys held when the focus goes stop repeating.
                if !focused {
                    self.repeat.clear();
                }
                self.log(event_loop, InputCommand::Focus { focused });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
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
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner_size();
                // The surface changes here too, so the log records it.
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
                window.request_redraw();
            }
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

/// The surface point at the center of frame pixel `pixel`, found through
/// the renderer's own pointer mapping (monotonic on each axis), so a warp
/// lands where a pointer maps back to that pixel.
fn surface_point(surface: [u32; 2], image: [u32; 2], pixel: [i32; 2]) -> Option<[f64; 2]> {
    let mut point = [0.0; 2];
    for axis in 0..2 {
        let mapped = |value: f64| {
            let mut probe = [0.0; 2];
            probe[axis] = value;
            vault_render::image_position_clamped(surface, image, probe).map(|at| at[axis])
        };
        // The first surface coordinate that maps to `target` or beyond.
        let first = |target: i32| -> Option<f64> {
            let (mut low, mut high) = (0.0, f64::from(surface[axis]));
            for _ in 0..48 {
                let middle = (low + high) / 2.0;
                if mapped(middle)? >= target {
                    high = middle;
                } else {
                    low = middle;
                }
            }
            Some(high)
        };
        point[axis] = (first(pixel[axis])? + first(pixel[axis] + 1)?) / 2.0;
    }
    Some(point)
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
        ..Default::default()
    };
    shell.display = preview
        .as_ref()
        .map(|preview| DisplayPalette::new(preview.palette(), preview.brightness()));
    shell.preview = preview;
    let run = event_loop.run_app(&mut shell);
    {
        use std::io::Write;
        shell.input_log.0.flush()?;
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
