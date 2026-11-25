use super::*;

impl Shell {
    pub(super) fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl Into<AppError>) {
        let error = error.into();
        tracing::error!(%error, "desktop shell failed");
        self.failure = Some(error);
        event_loop.exit();
    }

    /// Launcher input is never game input: the log starts with the game
    /// (its surface and scaling first, as a direct start writes them).
    pub(super) fn log(&mut self, _event_loop: &ActiveEventLoop, command: InputCommand) {
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
    pub(super) fn log_scaling(&mut self) {
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
    pub(super) fn apply_game_speed(&mut self, speed: enhanced::GameSpeed) {
        self.game_speed = Some(speed);
        self.record(InputCommand::GameSpeed {
            speed: speed.factor(),
        });
        if let Some(preview) = &mut self.session {
            preview.set_game_speed(speed);
        }
    }
    /// Watch Mode's control changes follow the inputs that made them.
    pub(super) fn log_autoplay_change(&mut self) {
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
    pub(super) fn start_watch(&mut self) {
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
    pub(super) fn bot_has_control(&mut self) -> bool {
        self.session
            .as_mut()
            .and_then(|preview| replay::interactive_world(preview))
            .and_then(|world| world.watch.as_ref())
            .is_some_and(|watch| !watch.paused)
    }
    /// One shown frame of Watch Mode: its speed's virtual 16 ms frames, each a
    /// logged tick. The clock runs ahead of the wall by what they added, so a
    /// tick never goes back when the player takes over.
    pub(super) fn watch_frames(&mut self, now: u64) -> AppResult<session::Advance> {
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
    pub(super) fn drain_debug(&mut self, event_loop: &ActiveEventLoop) {
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
    pub(super) fn flush_log(&mut self, _event_loop: &ActiveEventLoop) {
        self.flush_records();
    }
    pub(super) fn flush_records(&mut self) {
        use std::io::Write;
        if let Some(Err(error)) = self.input_log.0.as_mut().map(|log| log.flush()) {
            self.input_log.stop(&error);
        }
    }
}
