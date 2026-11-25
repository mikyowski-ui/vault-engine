use std::time::Instant;

use winit::{
    application::ApplicationHandler, event::WindowEvent, event_loop::ActiveEventLoop,
    keyboard::KeyCode, window::Window,
};

use super::{InputLog, Shell, Stopped, cli::next_preset, current_settings};
use crate::{
    boot, config,
    input::{self, InputCommand},
    interface, touch,
};

impl Shell {
    /// The selector over the stopped session, on what the disk says now.
    pub(super) fn switch_selector(&self) -> Option<boot::Selector> {
        let session = self.session.as_ref()?;
        let settings = current_settings();
        let found = boot::discover(&settings, &config::standard_data_dirs());
        let next = self
            .switch_base
            .as_ref()
            .map_or_else(Default::default, |args| next_preset(args, &settings));
        let running = session.edition();
        Some(
            boot::Selector::new(&settings, found, Some(running))
                .with_preset(next, Some(session.preset())),
        )
    }

    /// The selector's choice boots, with `game`, `last_played` and any
    /// toggled preset set in one commit.
    pub(super) fn choose_edition(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        game: vault_core::GameProfile,
    ) {
        let toggle = match &self.boot {
            Some(boot::Screen::Selector(selector)) => selector.toggled(),
            _ => None,
        };
        if self.stopped.is_none() {
            // The chosen game's own profile, before anything starts.
            let sfall = self.pending.as_ref().is_some_and(|args| args.preview.sfall);
            if let Some(settings) = &self.boot_config
                && let Err(reason) = boot::check_profile(settings, game, sfall)
            {
                if let Some(boot::Screen::Selector(selector)) = &mut self.boot {
                    selector.fail(game, reason);
                }
                return self.refresh_boot(window);
            }
            let paths = self
                .boot_found
                .as_ref()
                .and_then(|found| found.get(game).clone().ok());
            if let Some(paths) = paths {
                self.boot_edition(event_loop, window, game, paths, Some(toggle));
            }
            return;
        }
        // 1. Validate first: a failure costs the running session nothing.
        let dirs = config::search_dirs();
        let settings = config::find(&dirs)
            .map(|path| config::Config::load(&path))
            .transpose()
            .map_err(|error| error.to_string());
        let found = settings.and_then(|settings| {
            let settings = settings.unwrap_or_default();
            // A warm switch carries no `--sfall` (`switch_args`).
            boot::validate_choice(&settings, &config::standard_data_dirs(), game, false)
                .map(|paths| (settings, paths))
        });
        let (settings, paths) = match found {
            Ok(found) => found,
            Err(reason) => {
                if let Some(boot::Screen::Selector(selector)) = &mut self.boot {
                    selector.fail(game, reason);
                }
                return self.refresh_boot(window);
            }
        };
        // 2. Tear down. 3. Boot.
        self.end_session();
        self.boot_config = Some(settings);
        self.pending = self.switch_base.clone();
        self.boot_edition(event_loop, window, game, paths, Some(toggle));
    }

    /// SPEC-020 §2.5 step 2: the session ends as at quit. Its log ends, and
    /// the world, its programs, its hooks, its decoded assets and its `Vfs`
    /// go with it. Only the window and the renderer stay.
    pub(super) fn end_session(&mut self) {
        self.boot = None;
        self.stopped = None;
        let launcher = self.launcher.take();
        self.record(InputCommand::Quit);
        self.launcher = launcher;
        self.flush_records();
        self.input_log = InputLog::default();
        self.session = None;
        self._audio = None;
        self.display = None;
        self.shown = None;
        self.enhanced_dirty = false;
        self.backdrop_key = None;
        self.frame = None;
        self.toasts = None;
        self.repeat = input::KeyRepeat::default();
        self.touch = touch::TouchGestures::new(self.touch.slop);
        self.pointer_moved = false;
        interface::forget_notes();
    }

    /// Alt+L: the selector over the session, or closing it again. A session
    /// from memory discovered nothing, so it only says so (case S12).
    pub(super) fn switch_key(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        // Setup is left by its own Back.
        if self.launcher.is_some() {
            return;
        }
        if self.stopped.is_some() {
            return self.close_switch(event_loop, window);
        }
        self.toast_key(window, KeyCode::KeyL, true);
        let Some(session) = &self.session else {
            return;
        };
        if session.memory_mounted() {
            tracing::warn!("Alt+L: this session's data is in memory, with nothing to switch to");
            return;
        }
        let Some(selector) = self.switch_selector() else {
            return;
        };
        self.stop_session(selector);
        window.set_cursor_visible(true);
        self.refresh_boot(window);
    }

    /// The session stops at this step boundary: nothing runs, is logged or
    /// is timed until it resumes, and its audio holds (SPEC-020 §2.5).
    pub(super) fn stop_session(&mut self, selector: boot::Selector) {
        self.boot = Some(boot::Screen::Selector(selector));
        self.stopped = Some(Stopped {
            since: Instant::now(),
            scaling: self.scaling,
            pointer: self.pointer,
            held_back: Vec::new(),
        });
        if let Some(audio) = &self._audio {
            audio.hold(true);
        }
    }

    /// The session resumes at the same step: the frame clock skips the
    /// time the selector took, what it missed arrives now, and the frame
    /// comes back without asking it for one.
    pub(super) fn resume_session(&mut self) -> Option<Stopped> {
        let stopped = self.stopped.take()?;
        self.boot = None;
        if let Some(started) = &mut self.started {
            *started += stopped.since.elapsed();
        }
        if let Some(audio) = &self._audio {
            audio.hold(false);
        }
        Some(stopped)
    }

    /// The selector closes with no switch (cases S2 to S4).
    pub(super) fn close_switch(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(stopped) = self.resume_session() else {
            return;
        };
        if self.scaling != stopped.scaling {
            self.log_scaling();
        }
        if let Some(frame) = self.frame.take() {
            let palette = self.palette();
            self.show_frame(frame, &palette);
        }
        if let Some(preview) = &self.session {
            let title = preview.title();
            window.set_title(&title);
            self.title = title;
            window.set_cursor_visible(preview.system_cursor());
        }
        let moved = std::mem::replace(&mut self.pointer, stopped.pointer);
        if moved != stopped.pointer {
            match moved {
                Some(point) => self.pointer_to(event_loop, window, point),
                None => self.pointer_leave(event_loop, window),
            }
        }
        for event in stopped.held_back {
            self.window_event(event_loop, window.id(), event);
        }
        window.request_redraw();
    }

    /// An event the session under the selector has to see when it resumes.
    pub(super) fn hold_back(&mut self, event: Option<WindowEvent>) {
        if let (Some(stopped), Some(event)) = (&mut self.stopped, event) {
            stopped.held_back.push(event);
        }
    }
}
