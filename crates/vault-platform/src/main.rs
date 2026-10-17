mod input;
mod map_preview;
mod preview;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
#[cfg(test)]
pub(crate) use map_preview::fixture as fixture_world;

use std::{
    error::Error,
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
#[command(about = "Vault Engine indexed-color shell and loose-asset sprite preview")]
struct Args {
    /// Present one frame and exit successfully (requires a desktop and GPU backend).
    #[arg(long)]
    smoke_test: bool,
    #[command(flatten)]
    preview: preview::PreviewArgs,
}

/// SPEC-001 display palette: cycling replaces display entries, never the PAL.
struct DisplayPalette {
    entries: [[u8; 4]; 256],
    cycler: PaletteCycler,
}
impl DisplayPalette {
    fn new(palette: &Palette) -> Self {
        Self {
            entries: palette.rgba8(),
            cycler: PaletteCycler::default(),
        }
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
}

impl Shell {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<AppError>) {
        let error = error.into();
        tracing::error!(%error, "desktop shell failed");
        self.failure = Some(error);
        event_loop.exit();
    }

    fn log(&mut self, event_loop: &ActiveEventLoop, command: InputCommand) {
        if let Err(error) = input::write_command(std::io::stdout().lock(), &mut self.ids, &command)
        {
            self.fail(event_loop, error);
        }
    }
}

impl Shell {
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
                    let palette = self
                        .display
                        .as_ref()
                        .map_or_else(|| preview.palette().rgba8(), |display| display.entries);
                    renderer.set_image(&image, &palette);
                }
                window.set_title(&preview.title());
                window.request_redraw();
            }
            Err(error) => self.fail(event_loop, error),
        }
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
        }
    }

    fn log_hover(&mut self, event_loop: &ActiveEventLoop, point: [f64; 2]) {
        let hover = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.image_position(point))
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
        let renderer = (|| -> AppResult<Renderer> {
            if let Some(preview) = &mut self.preview {
                let image = preview.image()?;
                let palette = self
                    .display
                    .as_ref()
                    .map_or_else(|| preview.palette().rgba8(), |display| display.entries);
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
        let (Some(started), Some(preview)) = (self.started, self.preview.as_mut()) else {
            return;
        };
        let now = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let changed = match preview {
            preview::AssetPreview::Map(_) => match preview.tick(now) {
                Ok(changed) => changed,
                Err(error) => return self.fail(event_loop, error),
            },
            preview::AssetPreview::Sprite(_) => false,
        };
        // Each loop pass is one enabled cycling poll; the world can disable it
        // (for example during dialogue) and later restore it.
        let cycled = self.display.as_mut().is_some_and(|display| {
            display.cycler.enabled = preview.palette_cycling();
            display.cycler.poll(now, &mut display.entries)
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
                let handled = pressed
                    && if let PhysicalKey::Code(key) = event.physical_key {
                        self.preview_key(event_loop, &window, key)
                    } else {
                        false
                    };
                if pressed && !handled && event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    self.log(event_loop, InputCommand::Quit);
                    event_loop.exit();
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
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.log(
                    event_loop,
                    InputCommand::PointerButton {
                        button: format!("{button:?}"),
                        pressed: state == ElementState::Pressed,
                    },
                );
                if state == ElementState::Pressed
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                    && let Some(pixel) = self
                        .pointer
                        .and_then(|point| self.renderer.as_ref()?.image_position(point))
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
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(x), f64::from(y), "lines"),
                    MouseScrollDelta::PixelDelta(position) => (position.x, position.y, "pixels"),
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
            WindowEvent::Focused(focused) => self.log(event_loop, InputCommand::Focus { focused }),
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
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
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
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

fn run(args: Args) -> AppResult<()> {
    let preview = args.preview.load()?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell {
        smoke_test: args.smoke_test,
        ..Default::default()
    };
    shell.display = preview
        .as_ref()
        .map(|preview| DisplayPalette::new(preview.palette()));
    shell.preview = preview;
    event_loop.run_app(&mut shell)?;
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
