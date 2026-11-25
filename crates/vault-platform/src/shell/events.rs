use super::*;

/// The mouse's handlers, which the touch gestures reach through the same
/// log records.
impl Shell {
    /// The game's pointer moved to surface point `point` (the OS pointer,
    /// scaled, or a finger): it is logged, hovers, and the drawn cursor
    /// follows.
    pub(super) fn pointer_to(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        point: [f64; 2],
    ) {
        let [x, y] = point;
        self.pointer = Some(point);
        self.log_hover(event_loop, point);
        self.log(event_loop, InputCommand::PointerMoved { x, y });
        self.refresh_pointer(window);
    }
    /// The pointer left the window.
    pub(super) fn pointer_leave(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
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
    pub(super) fn now(&self) -> u64 {
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
