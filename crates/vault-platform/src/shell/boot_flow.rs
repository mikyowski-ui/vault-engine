use std::time::{Duration, Instant};

use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::PhysicalKey,
    window::{Fullscreen, Window},
};

use super::{
    DisplayPalette, Shell,
    cli::{
        Launch, boot_args, launch_game_speed, launcher_for, launcher_on, next_preset, play_args,
        start,
    },
    current_settings, session_toasts,
};
use crate::{audio, boot, config, input::InputCommand, launcher, session, touch};

/// §2.4 what a choice makes of the settings it commits, all of it in one
/// commit: `game`, `last_played`, a toggle, and a hints value held in memory.
pub(super) fn apply_choice(
    settings: &mut config::Config,
    game: vault_core::GameProfile,
    toggle: Option<config::Preset>,
    hints_off: bool,
) {
    settings.last_played = Some(config::game_name(game).to_owned());
    settings.preset = toggle.or(settings.preset);
    if hints_off {
        settings.hotkey_hints = Some(config::HotkeyHints::Off);
    }
}

impl Shell {
    pub(super) fn refresh_launcher(&mut self, window: &Window) {
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
    pub(super) fn launcher_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        event: WindowEvent,
    ) -> Option<WindowEvent> {
        // What the session under the selector must still see.
        let original = self.stopped.is_some().then(|| event.clone());
        if self.stopped.is_some() {
            match &event {
                WindowEvent::Focused(_) | WindowEvent::ModifiersChanged(_) => {
                    self.hold_back(original.clone());
                }
                WindowEvent::Resized(size) => {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.resize(size.width, size.height);
                    }
                    self.hold_back(original);
                    window.request_redraw();
                    return None;
                }
                _ => {}
            }
        }
        let input = match event {
            WindowEvent::CloseRequested => {
                // A session's log ends as at any quit.
                if self.session.is_some() {
                    self.boot = None;
                    self.launcher = None;
                    self.record(InputCommand::Quit);
                }
                event_loop.exit();
                return None;
            }
            // While a finger is down mouse events derived from it drop.
            WindowEvent::CursorMoved { .. } | WindowEvent::MouseInput { .. }
                if self.touch.active() =>
            {
                return None;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return None;
                };
                let pressed = event.state == ElementState::Pressed;
                if self.shell_key(event_loop, window, key, pressed, event.repeat) {
                    return None;
                }
                if !pressed {
                    // Pressed before the selector opened: the session
                    // has to see it go.
                    self.hold_back(original);
                    return None;
                }
                if self.boot.is_some() && event.repeat {
                    return None;
                }
                // Its release stays with the shell, even once closed.
                if self.stopped.is_some() && !event.repeat {
                    self.shell_keys.push(key);
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
                button,
                ..
            } if self.boot.is_some() || self.stopped.is_some() => {
                self.boot_buttons.push(button);
                match self.pixel() {
                    Some(pixel) if button == MouseButton::Left => {
                        launcher::LauncherInput::Press(pixel)
                    }
                    // The banner stops for any button, anywhere.
                    _ if matches!(self.boot, Some(boot::Screen::Banner(_))) => {
                        launcher::LauncherInput::Press([-1, -1])
                    }
                    _ => return None,
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button,
                ..
            } if self.boot.is_some() || self.stopped.is_some() => {
                if !self.take_boot_button(button) {
                    self.hold_back(original);
                }
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
                self.touch = touch::TouchGestures::new(self.touch.slop);
                return None;
            }
            // With the touch UI a tap hovers and presses as the mouse does
            // (the launcher has no camera or right button).
            WindowEvent::Touch(touch) if self.touch_ui => {
                let point = [touch.location.x, touch.location.y];
                let now = self.now();
                let actions = self.touch.feed(
                    touch.phase,
                    touch.id,
                    point,
                    now,
                    touch::TouchContext::default(),
                );
                for action in actions {
                    if self.launcher.is_none() && self.boot.is_none() {
                        break;
                    }
                    let input = match action {
                        touch::TouchAction::Pointer(point) => {
                            self.pointer = Some(point);
                            self.virtual_pointer.anchor(point);
                            match self.pixel() {
                                Some(pixel) => launcher::LauncherInput::Pointer(pixel),
                                None => continue,
                            }
                        }
                        touch::TouchAction::Button {
                            button: touch::TouchButton::Left,
                            pressed: true,
                        } => match self.pixel() {
                            Some(pixel) => launcher::LauncherInput::Press(pixel),
                            None => continue,
                        },
                        _ => continue,
                    };
                    self.launcher_input(event_loop, window, input);
                }
                return None;
            }
            other => return Some(other),
        };
        self.launcher_input(event_loop, window, input);
        None
    }

    /// One launcher input, and what its action asks for.
    fn launcher_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        input: launcher::LauncherInput,
    ) {
        if self.boot.is_some() {
            return self.boot_input(event_loop, window, &input);
        }
        let Some(launcher) = self.launcher.as_mut() else {
            return;
        };
        let action = launcher.handle(input);
        match action {
            None => self.refresh_launcher(window),
            Some(launcher::LauncherAction::Quit) => event_loop.exit(),
            Some(launcher::LauncherAction::Back) => self.setup_back(window),
            Some(launcher::LauncherAction::Play) => self.launcher_play(event_loop, window, false),
            Some(launcher::LauncherAction::PlayDebug) => {
                self.launcher_play(event_loop, window, true);
            }
            Some(launcher::LauncherAction::ResumeLast) => {
                self.resume_next = true;
                self.launcher_play(event_loop, window, false);
            }
            Some(launcher::LauncherAction::AutoplayTest) => {
                self.autoplay_start = Some((1, false));
                self.launcher_play(event_loop, window, false);
            }
        }
    }

    /// Play. Over a session it is a switch (case S25): the launcher has
    /// validated its data, so the session ends and the game boots.
    fn launcher_play(&mut self, event_loop: &ActiveEventLoop, window: &Window, debug: bool) {
        if self.stopped.is_some() {
            self.end_session();
            self.pending = self.switch_base.clone();
        }
        self.start_game(event_loop, window, debug);
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
        // Its own Save wrote the value held in memory, or it changed it.
        self.hints_off &= config.hotkey_hints == Some(config::HotkeyHints::Off)
            && config::find(&config::search_dirs()).is_none();
        let mut args = play_args(pending, debug);
        args.preview.resume_latest = std::mem::take(&mut self.resume_next);
        let (launch, session, audio) =
            match start(&mut args, Some(&config), session::SessionOrigin::Launcher) {
                Ok((launch, Some(session), audio)) => (launch, session, audio),
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
        tracing::info!(
            debug = args.preview.debug,
            game = config::game_name(session.edition()),
            "launcher: starting the game"
        );
        self.enter_session(event_loop, window, (launch, session, audio), &config);
    }

    /// A started session takes over the window, whichever way it started:
    /// the launcher's Play, a boot screen's choice, or a warm switch.
    fn enter_session(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        (launch, session, audio): (Launch, session::SessionRuntime, Option<audio::Audio>),
        config: &config::Config,
    ) {
        self.launcher = None;
        self.boot = None;
        self.boot_config = None;
        self.boot_found = None;
        self.pending = None;
        self.toasts = session_toasts(Some(config));
        self.hints_once = boot::hint_setting(Some(config)) == config::HotkeyHints::Once;
        self._audio = audio;
        self.display = Some(DisplayPalette::new(session.palette(), session.brightness()));
        self.session = Some(session);
        // The game's clock starts at its first frame, as in a direct start.
        self.started = Some(Instant::now());
        self.scaling = launch.scaling;
        self.enhanced = launch.enhanced;
        self.touch_ui = launch.touch_ui;
        self.game_speed = launch_game_speed(&launch, Some(config));
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
        self.log_session_records();
        self.refresh_preview(event_loop, window);
    }

    /// A boot screen on the window.
    pub(super) fn refresh_boot(&mut self, window: &Window) {
        let now = self.now();
        let Some(screen) = &self.boot else {
            return;
        };
        let title = screen.title();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_image(&screen.image(now), &launcher::palette());
        }
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
        window.request_redraw();
    }

    /// One input on a boot screen, and what it asks for.
    fn boot_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        input: &launcher::LauncherInput,
    ) {
        let selector = self.pre_boot_selector();
        let Some(screen) = self.boot.as_mut() else {
            return;
        };
        let action = match screen {
            // A key or a button stops the countdown (SPEC-020 §2.4).
            boot::Screen::Banner(_) => {
                if let Some(selector) = selector.filter(|_| boot::Banner::stops(input)) {
                    *screen = boot::Screen::Selector(selector);
                }
                None
            }
            boot::Screen::Selector(selector) => selector.handle(input),
        };
        match action {
            None => self.refresh_boot(window),
            Some(boot::BootAction::Leave) if self.stopped.is_some() => {
                self.close_switch(event_loop, window);
            }
            Some(boot::BootAction::Leave) => event_loop.exit(),
            Some(boot::BootAction::Choose(game)) => self.choose_edition(event_loop, window, game),
            Some(boot::BootAction::Setup(game)) => self.open_setup(window, game),
        }
    }

    /// The selector before any session, on the boot settings.
    fn pre_boot_selector(&self) -> Option<boot::Selector> {
        let (config, found) = self.boot_config.as_ref().zip(self.boot_found.as_ref())?;
        let next = self
            .pending
            .as_ref()
            .map_or_else(Default::default, |args| next_preset(args, config));
        Some(boot::Selector::new(config, found.clone(), None).with_preset(next, None))
    }

    /// Setup: the launcher, on the Game data tab with `game` in its Game
    /// row. A session under it stays stopped.
    fn open_setup(&mut self, window: &Window, game: vault_core::GameProfile) {
        let mut settings = current_settings();
        if self.hints_off {
            settings.hotkey_hints = Some(config::HotkeyHints::Off);
        }
        let args = self.pending.as_ref().or(self.switch_base.as_ref());
        let Some(args) = args else {
            return;
        };
        let launcher = launcher_on(args, Some(&settings), None).opened_by_selector(game);
        window.set_title(&launcher.title());
        self.title = launcher.title();
        self.launcher = Some(Box::new(launcher));
        self.boot = None;
        self.refresh_launcher(window);
    }

    /// Back from Setup: the selector, which runs discovery again. Nothing
    /// it held survives, because Save may have changed what it read.
    fn setup_back(&mut self, window: &Window) {
        let Some(launcher) = self.launcher.take() else {
            return;
        };
        if config::find(&config::search_dirs()).is_some() {
            self.hints_off = false;
        }
        let selector = if self.stopped.is_some() {
            self.switch_selector()
        } else {
            let settings = current_settings();
            self.boot_found = Some(boot::discover(&settings, &config::standard_data_dirs()));
            self.boot_config = Some(settings);
            self.pre_boot_selector()
        };
        self.boot =
            selector.map(|selector| boot::Screen::Selector(selector.highlighting(launcher.game())));
        self.refresh_boot(window);
    }

    /// Boot `game` on the boot settings, through the same path as Play. A
    /// failed boot opens the launcher, showing why. `choice` records a
    /// choice, with its toggled preset; the banner's own boot has none, and
    /// writes nothing.
    pub(super) fn boot_edition(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        game: vault_core::GameProfile,
        paths: config::GamePaths,
        choice: Option<Option<config::Preset>>,
    ) {
        let (Some(mut settings), Some(pending)) = (self.boot_config.take(), self.pending.take())
        else {
            return;
        };
        settings.choose_game(game, paths);
        let toggle = choice.flatten();
        if choice.is_some() {
            apply_choice(&mut settings, game, toggle, self.hints_off);
            match boot::commit_choice(&settings, &config::search_dirs()) {
                Ok(_) => self.hints_off = false,
                Err(error) => tracing::warn!(%error, "the choice of game was not saved"),
            }
        }
        // The toggle is a choice for this boot: a later one takes the usual
        // order, so a carried `--preset` does not undo it.
        if let (Some(_), Some(base)) = (toggle, &mut self.switch_base) {
            base.preset = None;
        }
        let mut args = boot_args(&pending, toggle);
        let origin = session::SessionOrigin::Launcher;
        let reason = match start(&mut args, Some(&settings), origin) {
            Ok((launch, Some(session), audio)) => {
                tracing::info!(game = config::game_name(game), "boot: starting the game");
                return self.enter_session(event_loop, window, (launch, session, audio), &settings);
            }
            Ok((_, None, _)) => "the settings name no game data".to_owned(),
            Err(error) => {
                tracing::error!(%error, "the chosen game did not start");
                error.to_string()
            }
        };
        self.boot = None;
        let problem = format!("The game did not start: {reason}");
        self.launcher = launcher_for(&pending, Some(&settings), Some(problem)).map(Box::new);
        self.pending = Some(pending);
        self.refresh_launcher(window);
    }

    /// Whether `button`'s press happened on a boot screen.
    pub(super) fn take_boot_button(&mut self, button: MouseButton) -> bool {
        match self.boot_buttons.iter().position(|held| *held == button) {
            Some(at) => {
                self.boot_buttons.swap_remove(at);
                true
            }
            None => false,
        }
    }

    /// The launcher's preview runs on its own clock, at its own step.
    pub(super) fn launcher_tick(&mut self, event_loop: &ActiveEventLoop) {
        let now = self.now();
        let changed = self
            .launcher
            .as_mut()
            .is_some_and(|launcher| launcher.tick(now));
        if changed && let Some(window) = self.window.clone() {
            self.refresh_launcher(&window);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(40),
        ));
    }

    /// One frame of a boot screen: the banner counts down on the wall
    /// clock, and boots its edition when the time is up.
    pub(super) fn boot_tick(&mut self, event_loop: &ActiveEventLoop) {
        let now = self.now();
        let Some(boot::Screen::Banner(banner)) = &mut self.boot else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        let before = banner.seconds(now);
        let due = banner.tick(now);
        let (edition, changed) = (banner.edition, banner.seconds(now) != before);
        let Some(window) = self.window.clone() else {
            return;
        };
        if due {
            let paths = self
                .boot_found
                .as_ref()
                .and_then(|found| found.get(edition).clone().ok());
            if let Some(paths) = paths {
                return self.boot_edition(event_loop, &window, edition, paths, None);
            }
        }
        if changed {
            self.refresh_boot(&window);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(50),
        ));
    }
}
