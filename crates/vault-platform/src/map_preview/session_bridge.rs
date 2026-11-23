#[cfg(test)]
use super::FADE_MS;
use super::{MapPreview, Screen, options};
use crate::AppResult;
use vault_render::{IndexedImage, map::MapHover};

impl MapPreview {
    /// SPEC-014 §3.29 the keyboard for the engine key repeat: no input is
    /// polled during a game fade (§3.2) and no key is read while combat
    /// disables the keyboard (§3.27); each player combat turn start clears
    /// the keyboard.
    pub(crate) fn keyboard(&self) -> crate::input::Keyboard {
        crate::input::Keyboard {
            enabled: !self.fade_blocks() && !self.keyboard_locked(),
            clears: self.host.combat_lock.turns,
        }
    }
    /// SPEC-014 §3.2: a game fade blocks completely, outside a movie (whose
    /// own palette hides a fade queued behind it).
    pub(crate) fn fade_blocks(&self) -> bool {
        self.fades.blocking() && !self.movie_active()
    }
    /// Tick past every queued or running fade (tests): a game fade holds
    /// the world and input until it ends (SPEC-014 §3.2).
    #[cfg(test)]
    pub(crate) fn finish_fades(&mut self) {
        let mut now = self.clock;
        for _ in 0..64 {
            if !self.fades.running() {
                return;
            }
            now += FADE_MS + 16;
            self.tick(now).unwrap();
        }
        panic!("the fades did not end");
    }
    /// Whether the save/load screen is open (the main menu's load stage).
    pub fn load_screen_open(&self) -> bool {
        matches!(self.screen, Screen::SaveLoad(_))
    }
    /// The save/load selection and quick-slot flag, which last for the
    /// program run (SPEC-014 §3.14, §3.16).
    pub fn slot_session(
        &self,
    ) -> (
        vault_core::ui::saveload::SlotList,
        vault_core::ui::saveload::QuickSlot,
    ) {
        (self.slot_selection().clone(), self.quick)
    }
    pub fn set_slot_session(
        &mut self,
        slot_list: vault_core::ui::saveload::SlotList,
        quick: vault_core::ui::saveload::QuickSlot,
    ) {
        self.slot_list = slot_list;
        self.quick = quick;
    }
    /// Held modifiers: shift, control, alt.
    pub fn set_modifiers(&mut self, modifiers: [bool; 3]) {
        // Holding Alt peeks under the roofs, and holding Shift highlights
        // the ground items, in the `enhanced` world.
        if self.enhanced_extras()
            && (modifiers[2] != self.hud.modifiers[2] || modifiers[0] != self.hud.modifiers[0])
        {
            self.pointer.redraw = true;
            self.thaw_view();
        }
        self.hud.modifiers = modifiers;
    }
    /// M-P6: the window lost the focus, so the releases of the buttons held
    /// now will not arrive: every held press is dropped, and no action
    /// menu opens later with the button up.
    pub fn focus_lost(&mut self) {
        self.drop_presses();
        self.enhanced_release_keys();
    }
    /// Whether the shell may cycle the palette now: the library's flag,
    /// except while a window that suspends cycling is open. SPEC-012 §3.12
    /// movies; SPEC-014 §3.17 the in-game pause window (not the world
    /// map's), §3.14 in-game save/load, §3.2 the help screen, §3.8 the
    /// character editor, §3.39-§3.40 the world and town maps, §3.41 the
    /// Pip-Boy; SPEC-013 §3.15 the Skilldex and §3.21 elevators.
    pub(crate) fn palette_cycling(&self) -> bool {
        self.host.library.palette_cycling
            && !matches!(
                self.screen,
                Screen::Movie(_)
                    | Screen::PipBoy(_)
                    | Screen::World(_)
                    | Screen::Town(_)
                    | Screen::World2(_)
                    | Screen::Town2(_)
                    | Screen::Options(options::OptionsScreen::Window(
                        vault_core::ui::options::OptionsWindow {
                            page: vault_core::ui::options::OptionsPage::Pause
                        }
                    ))
                    | Screen::SaveLoad(_)
                    | Screen::Help(_)
                    | Screen::Character(_)
                    | Screen::Skilldex(_)
                    | Screen::Elevator(_)
            )
    }
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        self.drain_effects();
        // SPEC-014 §3.2: a game fade to black blocks before anything after
        // its request shows: the frame on screen stays until the screen is
        // black, so `gfade_out; move_to; gfade_in` hides the move.
        if self.fade_blocks()
            && self.fades.freezes()
            && let Some(frame) = &self.frame
        {
            let mut image = frame.clone();
            self.draw_cursor(&mut image);
            return Ok(image);
        }
        self.check_barter();
        self.check_loot();
        // The map view sets it when it draws the in-world cursor.
        self.cursor_in_frame = false;
        let mut image = self.screen_image()?;
        self.held_shown = self.held_button().is_some();
        self.frame = Some(image.clone());
        self.draw_cursor(&mut image);
        Ok(image)
    }
    /// The last composed frame with the cursor at the pointer's current
    /// place, without composing the scene again. SPEC-014 §3.27: the
    /// in-world cursor is a map object drawn under the upright objects and
    /// roofs, so while it shows, or the last frame showed it, the scene is
    /// composed again.
    pub fn pointer_frame(&mut self) -> Option<IndexedImage> {
        if self.cursor_in_frame || self.world_cursor_at().is_some() {
            return self.image().ok();
        }
        let mut image = self.frame.clone()?;
        self.draw_cursor(&mut image);
        Some(image)
    }
    /// The window title: the plain game name, or with the vault-debug
    /// inspector on (`--debug`, F9) the internal hints (screen, map file,
    /// elevation, cursor mode and the preview keys).
    pub fn title(&self) -> String {
        if !self.debug_enabled() {
            return "Fallout — Vault Engine".into();
        }
        match &self.screen {
            Screen::World(_) | Screen::World2(_) => {
                return "Vault — world map — click to travel; click the party to enter".into();
            }
            Screen::Town(_) | Screen::Town2(_) => {
                return "Vault — town map — choose an area".into();
            }
            Screen::PipBoy(_) => return "Vault — Pip-Boy — Escape returns".into(),
            Screen::Inventory(_) => {
                return "Vault — inventory — click an item, then an action; Escape returns".into();
            }
            Screen::Character(_) => {
                return "Vault — character — Done keeps, Cancel or Escape restores".into();
            }
            Screen::Barter(_) => {
                return "Vault — barter — drag items between columns; M offers; T or Escape returns"
                    .into();
            }
            Screen::Movie(screen) => {
                return format!(
                    "Vault — movie {} — any key or a click skips",
                    screen.player.request.path
                );
            }
            Screen::Skilldex(_) => {
                return "Vault — Skilldex — choose a skill; Escape cancels".into();
            }
            Screen::Elevator(_) => {
                return "Vault — elevator — choose a level; Escape cancels".into();
            }
            Screen::Loot(_) => {
                return "Vault — loot — drag an item to the other pane; Shift+A takes all; Escape closes"
                    .into();
            }
            Screen::Options(_) => return "Vault — options".into(),
            Screen::SaveLoad(_) => {
                return "Vault — save/load — Enter commits; Escape cancels".into();
            }
            Screen::Automap(_) => {
                return "Vault — automap — Tab, Escape or A closes; H/L detail; S scanner".into();
            }
            Screen::Help(_) => return "Vault — help — any key or a click returns".into(),
            Screen::Map => {}
        }
        let roofs = if !self.roof_key {
            String::new()
        } else if self.view.roofs {
            " — roofs on".into()
        } else {
            " — roofs off".into()
        };
        let keys = if self.preview_keys {
            "; L/D/U/G/T actions; W world map"
        } else {
            ""
        };
        format!(
            "Vault — {} — elevation {}{roofs} — {} cursor; right-click cycles{keys}",
            self.path,
            self.view.elevation,
            self.mode_name()
        )
    }
    /// One frame. Its Simulation phase ends here, whether the world ticked
    /// or a modal screen held it (SPEC-024b §5.3).
    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        // SPEC-024b §4.11: after this step's input records, each tapped key
        // is pressed and released. A replay queues them again.
        for scan in self.host.sfall_take_keys() {
            if let Some(key) = crate::input::key_of_scan(scan) {
                self.step(key);
                self.key_up(key);
            }
        }
        self.apply_sfall_requests();
        self.host.set_sfall_screens(self.sfall_screens());
        let changed = self.frame(now);
        let opened = self.open_requested_windows();
        self.show_message_box();
        self.sfall_previews();
        self.host.sfall_end_of_step();
        self.commit_mod_state();
        self.quit_requested |= self.host.sfall_session_closed();
        changed.map(|changed| changed || opened)
    }
    /// SPEC-024a §7.4 and §7.5, at the end of phase 2: the log's new lines
    /// reach the debug log, and each mod with a changed setting its file.
    fn commit_mod_state(&mut self) {
        let Some(session) = self.host.mod_session.clone() else {
            return;
        };
        for line in session.take_unsent() {
            tracing::debug!(target: "vault_debug", "{line}");
        }
        let ops = crate::mods::settings_batch(session.take_changed());
        if ops.is_empty() {
            return;
        }
        let committed = self.storage.as_ref().map(|storage| {
            storage
                .commit(crate::storage::Namespace::Config, &ops)
                .and_then(|()| storage.flush())
        });
        if let Some(Err(error)) = committed {
            self.host
                .world
                .diagnostic(format!("mod settings commit: {error}"));
        }
    }
    /// SPEC-024b §4.12: what scripts asked of the interface.
    fn apply_sfall_requests(&mut self) {
        use vault_core::ui::cursor::Mode;
        if let Some(text) = self.host.world.dialogue.script_message.take() {
            if matches!(self.screen, Screen::Barter(_)) {
                self.barter_reply(text);
            } else if let Err(error) = self.host.show_script_reply(text) {
                self.status = error.to_string().into_bytes();
            }
        }
        match self.host.world.session.cursor_mode.take() {
            Some(0) => self.hud.mode = Mode::Movement,
            Some(1) => self.hud.mode = Mode::Arrow,
            Some(2) => self.hud.mode = Mode::Crosshair,
            _ => {}
        }
    }
    /// SPEC-024b §4.4: the `get_game_mode` bits of the open screen.
    pub(super) fn sfall_screens(&self) -> u32 {
        use vault_core::host_sfall::game_mode as mode;
        match &self.screen {
            Screen::World(_) | Screen::Town(_) | Screen::World2(_) | Screen::Town2(_) => {
                mode::WORLD_MAP
            }
            Screen::PipBoy(_) => mode::PIP_BOY,
            Screen::Inventory(_) => mode::INVENTORY,
            Screen::Character(_) => mode::CHARACTER,
            Screen::Barter(_) => mode::BARTER,
            Screen::Skilldex(_) => mode::SKILLDEX,
            Screen::Loot(_) => mode::LOOT,
            Screen::Options(_) => mode::OPTIONS,
            Screen::SaveLoad(screen) => match screen.mode {
                vault_core::ui::saveload::Mode::Save
                | vault_core::ui::saveload::Mode::PickQuickSave => mode::SAVE,
                _ => mode::LOAD,
            },
            Screen::Automap(_) => mode::AUTOMAP,
            Screen::Help(_) => mode::HELP,
            Screen::Map | Screen::Movie(_) | Screen::Elevator(_) => 0,
        }
    }
    /// The pointer moved over the frame; returns the map hover once per hex
    /// change, for the shell's log.
    pub fn hover(&mut self, point: [i32; 2]) -> Option<MapHover> {
        self.hover_in(point, false)
    }
    /// [`Self::hover`] at a frame pixel, or with `world` at a pixel of the
    /// `enhanced` world canvas.
    pub(crate) fn hover_in(&mut self, point: [i32; 2], world: bool) -> Option<MapHover> {
        // SPEC-014 §3.2: the pointer is not polled during a game fade; its
        // last position applies when the fade ends.
        if self.fade_blocks() {
            self.fade_pointer = Some(Some(point));
            return None;
        }
        self.pointer_moved_in(point, world);
        if self.host.world.dialogue.open || !matches!(self.screen, Screen::Map) {
            return None;
        }
        let hover = self.pointer_view().hover(point);
        if self.last_hover == Some(hover) {
            return None;
        }
        self.last_hover = Some(hover);
        Some(hover)
    }
}
