use super::{BUTTON_PRESS, BUTTON_RELEASE, MapPreview, Screen, hud};
use crate::{AppResult, input::key_text};
use vault_core::ObjectAction;
use vault_formats::HexGrid;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

impl MapPreview {
    /// Every press still held is forgotten (its release was swallowed).
    pub(super) fn drop_presses(&mut self) {
        self.held = None;
        self.hud.held = None;
        self.pressed = None;
        self.pointer.held = None;
        self.pointer.menu = None;
        self.touch.pressed = None;
    }
    /// The last game fade ended: the pointer's move during it applies.
    fn fade_ended(&mut self) {
        // The `enhanced` pointer is routed again from where the shell last
        // reported it.
        if self.fade_pointer.is_some() && self.enhanced_pointer_again() {
            self.fade_pointer = None;
            return;
        }
        match self.fade_pointer.take() {
            Some(Some(point)) => {
                self.hover(point);
            }
            Some(None) => self.pointer_left(),
            None => {}
        }
    }
    /// A red button press: its cue, and the release cue to follow.
    pub(crate) fn press(&mut self) {
        self.press_cue(BUTTON_PRESS, BUTTON_RELEASE);
    }
    /// A button press with its own press and release cues.
    pub(crate) fn press_cue(&mut self, press: &'static [u8], release: &'static [u8]) {
        self.pressed = Some(release);
        self.play_effect(press, None);
    }
    /// A red-button press on the button at screen `rect`: the cues, and its
    /// down art until the release.
    pub(crate) fn press_button(&mut self, rect: vault_core::ui::Rect) {
        self.press();
        self.held = Some(rect);
    }
    /// A press on a button at screen `rect` with its own cues.
    pub(crate) fn press_button_cue(
        &mut self,
        press: &'static [u8],
        release: &'static [u8],
        rect: vault_core::ui::Rect,
    ) {
        self.press_cue(press, release);
        self.held = Some(rect);
    }
    /// The held button, while the pointer is over it.
    pub(crate) fn held_button(&self) -> Option<vault_core::ui::Rect> {
        self.held
            .filter(|rect| self.pointer.at.is_some_and(|at| rect.contains(at)))
    }
    /// The pointer moved onto or off the held button since the last frame.
    pub(super) fn held_moved(&self) -> bool {
        self.held_button().is_some() != self.held_shown
    }
    /// A button's art: `down` while it is held with the pointer over it,
    /// else `up`.
    pub(crate) fn button_art(
        &self,
        rect: vault_core::ui::Rect,
        up: &'static str,
        down: &'static str,
    ) -> &'static str {
        if self.held_button() == Some(rect) {
            down
        } else {
            up
        }
    }
    /// A mouse release at a frame pixel (`None` outside the frame): a drag
    /// drops, a map click acts, a held desktop button plays its release
    /// cue, and a button that reports on release acts when it is over it.
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        self.thaw_view();
        // SPEC-014 §3.2: a release during a game fade is swallowed with its
        // press, so nothing pressed before the fade acts or stays held.
        if self.fade_blocks() {
            if !right {
                self.drop_presses();
            }
            return true;
        }
        // A press on the touch UI's pill bar acts on its release.
        if !right && self.touch_bar_held() {
            return self.touch_bar_release(point);
        }
        // A left release ends the held button's down art.
        let (held, bar) = if right {
            (None, None)
        } else {
            (self.held.take(), self.hud.held.take())
        };
        let over = held.filter(|rect| point.is_some_and(|point| rect.contains(point)));
        if self.movie_release() {
            self.pressed = None;
            return true;
        }
        // SPEC-014 §3.21: a click whose release is read during a counter
        // roll or the panel animation is lost: no map action, bar button or
        // menu entry results.
        if self.bar_blocks() {
            if !right {
                self.drop_presses();
            }
            return true;
        }
        // SPEC-013 §3.15: the Set Timer window is modal over any window.
        let mut handled = !right
            && (self.timer_release(over)
                || match self.screen {
                    Screen::Map => {
                        self.talk_release(point, over)
                            || self.called_release(point)
                            || self.map_release(point)
                            || self.bar_release(bar, over)
                    }
                    Screen::Elevator(_) => self.elevator_release(point, over),
                    Screen::Loot(_) => self.loot_release(point),
                    Screen::Inventory(_) => self.inventory_release(point),
                    Screen::Barter(_) => self.barter_release(point),
                    Screen::Automap(_) => self.automap_release(point),
                    Screen::Options(_) => self.options_release(point),
                    Screen::PipBoy(_) => self.pipboy_release(point),
                    Screen::World(_) => self.world_release(point),
                    Screen::Town(_) => self.town_release(point),
                    Screen::World2(_) => self.world2_release(point),
                    Screen::Town2(_) => self.town2_release(point),
                    Screen::Character(_) => self.character_release(point, over.is_some()),
                    Screen::SaveLoad(_) => self.saveload_release(),
                    _ => false,
                });
        if let Some(cue) = self.pressed.take() {
            self.play_effect(cue, None);
            handled = true;
        }
        handled || held.is_some()
    }
    /// The screen in front composed with the windows over it (the Set
    /// Timer and called-shot windows, a dialog box, the debug overlay),
    /// without the cursor.
    pub(super) fn screen_image(&mut self) -> AppResult<IndexedImage> {
        let mut image = match &self.screen {
            Screen::Map => self.map_image(),
            Screen::Skilldex(_) => self.skilldex_image(),
            Screen::Elevator(_) => self.elevator_image(),
            Screen::Loot(_) => self.loot_image(),
            Screen::Options(_) => self.options_image(),
            Screen::SaveLoad(_) => self.saveload_image(),
            Screen::World(_) => self.world_image(),
            Screen::Town(_) => self.town_image(),
            Screen::World2(_) => self.world2_image(),
            Screen::Town2(_) => self.town2_image(),
            Screen::PipBoy(_) => self.pipboy_image(),
            Screen::Inventory(_) => self.inventory_image(),
            Screen::Character(_) => self.character_image(),
            Screen::Barter(_) => self.barter_image(),
            Screen::Movie(_) => self.movie_image().ok_or_else(|| "no movie".into()),
            Screen::Automap(_) => self.automap_image(),
            Screen::Help(_) => self.help_image(),
        }?;
        self.draw_overlays(&mut image);
        Ok(image)
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        // Before the fade check and every screen: Space must reach the
        // takeover, and must not end a combat turn while the bot plays.
        if self.autoplay_key(key) {
            return true;
        }
        self.thaw_view();
        self.enhanced_key_down(key);
        let handled = self.step_key(key);
        // G-22: a key that opened a window over a held map press drops it.
        self.drop_map_press();
        handled
    }
    fn step_key(&mut self, key: KeyCode) -> bool {
        // SPEC-014 §3.2: no input is polled while a game fade runs.
        if self.fade_blocks() {
            return true;
        }
        // SPEC-014 §3.19: F12 is caught as the key enters the common input
        // queue, before any window, movie or dialog box sees it, so it works
        // everywhere (a hand swap's wait included, §3.23) and no screen
        // receives it. While combat disables the keyboard (§3.27) it is not
        // read: it is held below with the other keys and acts when read.
        if key == KeyCode::F12 && !self.keyboard_locked() && !self.bar_blocks() {
            self.screenshot();
            return true;
        }
        // SPEC-014 §3.29: a modifier or lock key on its own translates to no
        // key code. While the keyboard is disabled its press or repeat still
        // takes a buffer slot (read later, it does nothing); otherwise it is
        // read at once and nothing acts: no window opens or closes, no
        // "any key" test is met (a movie, the help screen, the pause window).
        if crate::input::is_modifier(key) {
            let modal = matches!(self.screen, Screen::Movie(_))
                || self.dialog.is_some()
                || self.hud.timer.is_some()
                || self.hud.called.is_some();
            if !modal && (self.keyboard_locked() || self.bar_blocks()) {
                let modifiers = self.hud.modifiers;
                self.hold_key(hud::HeldKey::Press(key, modifiers));
            }
            return false;
        }
        if self.movie_key() || self.dialog_key(key) || self.timer_key(key) || self.called_key(key) {
            return true;
        }
        // F9 toggles the vault-debug inspector; while it is open, F8 and
        // its editing keys belong to it.
        if key == KeyCode::F9 {
            let on = !self.debug_enabled();
            self.set_debug(on);
            return true;
        }
        if self.debug_key(key) {
            return true;
        }
        // The world stays frozen while the debugger steps a procedure.
        if self.host.debug_vm.is_some() {
            return true;
        }
        // The `enhanced` tactical overlay's toggle, Alt+H (presentation
        // only, so the combat lock does not hold it).
        if key == KeyCode::KeyH && self.hud.modifiers[2] && self.qol() {
            self.toggle_tactical();
            return true;
        }
        // SPEC-014 §3.27: while combat locks the keyboard no key is read,
        // F12 included; presses and releases are held (up to 32) and read
        // in order once it is enabled (`read_held_keys`).
        // §3.21: during a counter roll or the panel animation keys are
        // deferred the same way, and read in order when it ends.
        if self.keyboard_locked() || self.bar_blocks() {
            let modifiers = self.hud.modifiers;
            self.hold_key(hud::HeldKey::Press(key, modifiers));
            return true;
        }
        // §3.23: a hand swap's wait reads and discards every key (F12 was
        // caught above).
        if matches!(self.screen, Screen::Map) && self.swapping_hands() {
            return true;
        }
        // SPEC-014 §3.28-3.29: keyboard input is discarded while the action
        // menu is open or edge scrolling is under way.
        if self.pointer_swallows_keys() {
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return self.barter_key(key);
        }
        // SPEC-014 §3.45: the Review and Ask About windows are modal.
        if self.host.world.dialogue.open && self.talk.is_some() {
            return self.talk_key(key);
        }
        if self.host.world.dialogue.open {
            if self.panel_key(key) {
                return true;
            }
            // The number row: 1-9 pick an option, 0 none.
            let choice = (key_text(key, false).filter(u8::is_ascii_digit))
                .map(|digit| usize::from(digit - b'0').checked_sub(1));
            if let Some(index) = choice {
                if let Err(error) = self.host.choose_dialogue(index) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            match key {
                KeyCode::Escape => {
                    if let Err(error) = self.host.close_dialogue() {
                        self.status = error.to_string().into_bytes();
                    }
                    return true;
                }
                KeyCode::Space | KeyCode::ArrowDown => {
                    let dialog = &mut self.host.world.dialogue;
                    dialog.page =
                        (dialog.page + 1).min(dialog.page_offsets.len().saturating_sub(1));
                    dialog.page_ms = self.host.world.elapsed_ms;
                    return true;
                }
                KeyCode::ArrowUp => {
                    self.host.world.dialogue.page = self.host.world.dialogue.page.saturating_sub(1);
                    return true;
                }
                _ => return false,
            }
        }
        match self.screen {
            Screen::World(_) => return self.world_key(key),
            Screen::Town(_) => return self.town_key(key),
            Screen::World2(_) => return self.world2_key(key),
            Screen::Town2(_) => return self.town2_key(key),
            Screen::PipBoy(_) => return self.pipboy_key(key),
            Screen::Inventory(_) => return self.inventory_key(key),
            Screen::Character(_) => return self.character_key(key),
            Screen::Skilldex(_) => return self.skilldex_key(key),
            Screen::Elevator(_) => return self.elevator_key(key),
            Screen::Loot(_) => return self.loot_key(key),
            Screen::Options(_) => return self.options_key(key),
            Screen::SaveLoad(_) => return self.saveload_key(key),
            Screen::Automap(_) => return self.automap_key(key),
            Screen::Help(_) => return self.close_help(),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        // SPEC-014 §3.2: F1 shows the help screen.
        if key == KeyCode::F1 && matches!(self.screen, Screen::Map) && !self.host.world.ui_disabled
        {
            self.open_help();
            return true;
        }
        // The `enhanced` world's camera and roof-peek keys.
        if self.enhanced_key(key) {
            return true;
        }
        // SPEC-014 §3.19 game-loop hotkeys (Escape opens the options).
        if self.game_hotkey(key) {
            return true;
        }
        // SPEC-014 §3.17: Escape opens the options only while the bar is
        // enabled, and otherwise does nothing; it never leaves the game
        // (G-01: a script that disabled the interface made it quit).
        if key == KeyCode::Escape {
            return true;
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if key == KeyCode::KeyW && self.preview_keys {
            self.open_world_map_with_music();
            return true;
        }
        let key_number = key_text(key, true).filter(|code| b"LDUGT".contains(code));
        if let Some(key) = key_number
            && self.host.key_callback(i32::from(key))
        {
            return true;
        }
        let action = match key {
            KeyCode::KeyL => Some(ObjectAction::LookAt),
            KeyCode::KeyD => Some(ObjectAction::Description),
            KeyCode::KeyU => Some(ObjectAction::Use),
            KeyCode::KeyG => Some(ObjectAction::Pickup),
            KeyCode::KeyT => Some(ObjectAction::Talk),
            _ => None,
        };
        if let Some(action) = action
            && self.preview_keys
        {
            self.interact(action);
            return true;
        }
        // SPEC-014 §3.29: an arrow key scrolls one step through the same
        // 33 ms gate and blocked test as the edges.
        let arrow = match key {
            KeyCode::ArrowUp => Some(1),
            KeyCode::ArrowRight => Some(3),
            KeyCode::ArrowDown => Some(5),
            KeyCode::ArrowLeft => Some(7),
            _ => None,
        };
        if let Some(direction) = arrow {
            self.scroll_step(direction);
            return true;
        }
        match key {
            KeyCode::KeyR if self.roof_key => self.view.roofs = !self.view.roofs,
            KeyCode::PageUp if self.preview_keys => {
                self.view.elevation = (self.view.elevation + 1).min(2)
            }
            KeyCode::PageDown if self.preview_keys => {
                self.view.elevation = self.view.elevation.saturating_sub(1)
            }
            _ => return false,
        }
        self.last_hover = None;
        // An elevation change keeps the map border (SPEC-014 §3.29).
        if let Some(center) = self.view.grid.pick_hex([320, 190]) {
            self.host.world.set_view_center(center);
        }
        if self.host.world.elevation != self.view.elevation {
            self.host.world.elevation = self.view.elevation;
            if self.host.world.map_script.is_some()
                && let Err(error) = self.host.map_update()
            {
                self.status = error.to_string().into_bytes();
            }
        }
        true
    }
    pub(super) fn interact(&mut self, action: ObjectAction) {
        if let Some(target) = self.selected {
            if let Err(error) = self.host.action(self.host.world.player, target, action) {
                self.status = error.to_string().into_bytes();
            }
        } else {
            self.status = b"Select an object first".to_vec();
        }
        self.drain_effects();
    }
    pub fn click(&mut self, point: [i32; 2], right: bool) -> bool {
        self.thaw_view();
        // SPEC-014 §3.2: no input is polled while a game fade runs.
        if self.fade_blocks() {
            return true;
        }
        // Drawn over everything, so they come before anything that would
        // swallow the press: a combat lock must not kill the debugger's tabs.
        // On the unlocked world `point` is a canvas pixel, and `enhanced_button`
        // has asked already with the interface pixel.
        if !right && self.autoplay_click(point) {
            return true;
        }
        if self.movie_press() {
            return true;
        }
        if !right && !self.pointer.world && self.debug_panel_at(point) {
            self.debug_panel_press(point);
            return true;
        }
        // The world stays frozen while the debugger steps a procedure.
        if self.host.debug_vm.is_some() {
            return true;
        }
        // The dialog box and the Set Timer window take the left button
        // only: a right click presses none of their buttons (which would
        // leave a held button with no release), and is swallowed.
        if right && (self.dialog.is_some() || self.hud.timer.is_some()) {
            return true;
        }
        if self.dialog_click(point) || self.timer_click(point) || self.called_click(point, right) {
            return true;
        }
        // The touch UI's pill bar (`touchbar`): its buttons act as their
        // keys do, so they come before the counter roll and the combat
        // lock, which the keys' own path handles.
        if !right && self.touch_bar_press(point) {
            return true;
        }
        // SPEC-014 §3.21, §3.25: while a counter rolls or the combat panel
        // animates no click acts. Buttons are sampled, not queued: a left
        // press in the map still down when the animation ends counts as
        // held from the reading that saw it, and its release then acts as
        // an ordinary release; a release during the animation loses it.
        if self.bar_blocks() {
            if !right
                && matches!(self.screen, Screen::Map)
                && !self.host.world.dialogue.open
                && self.map_area(point)
            {
                self.map_press(point);
            }
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return self.barter_click(point, right);
        }
        if self.host.world.dialogue.open && !right && self.talk_click(point) {
            return true;
        }
        if self.host.world.dialogue.open {
            if self.talk.is_some() {
                return true;
            }
            if let Some(choice) = self.choices.iter().find(|choice| choice.contains(point)) {
                let index = choice.index;
                self.press();
                if let Err(error) = self.host.choose_dialogue(Some(index)) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            return false;
        }
        match self.screen {
            Screen::World(_) => return self.world_click(point, right),
            Screen::Town(_) => return self.town_click(point, right),
            Screen::World2(_) => return self.world2_click(point, right),
            Screen::Town2(_) => return self.town2_click(point, right),
            Screen::PipBoy(_) => return self.pipboy_click(point),
            Screen::Inventory(_) => return self.inventory_click(point, right),
            Screen::Character(_) => return self.character_click(point),
            Screen::Skilldex(_) => return self.skilldex_click(point),
            Screen::Elevator(_) => return self.elevator_click(point),
            Screen::Loot(_) => return self.loot_click(point, right),
            Screen::Options(_) => return self.options_click(point),
            Screen::SaveLoad(_) => return self.saveload_click(point),
            Screen::Automap(_) => return self.automap_click(point),
            Screen::Help(_) => return self.close_help(),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        // SPEC-014 §3.19: the interface bar, then the map area by cursor
        // mode; a right-click in the map cycles the mode.
        // The map area runs through y = 379 (the bar window overlaps it).
        // SPEC-014 §3.26-§3.27, §3.23: while the animated watch shows (the
        // combat lock or a hand swap) the game mouse and the bar take
        // nothing; clicks are discarded, never queued. The Swap hands
        // button alone still presses (§3.27).
        if self.watch_shown() {
            // The debugger picks an object during a lock too.
            if self.map_area(point) && self.debug_click(point, right) {
                return true;
            }
            if !right && !self.map_area(point) {
                self.swap_button_press(point);
            }
            return true;
        }
        // The `enhanced` HUD's party pills, in frame pixels (on the unlocked
        // canvas `enhanced_button` routes them).
        if !right && !self.pointer.world && self.party_pill_click(point) {
            return true;
        }
        if !self.map_area(point) {
            return self.bar_click(point, right);
        }
        // While the vault-debug overlay is open a map click selects the
        // object under the pointer instead of acting.
        if self.debug_click(point, right) {
            return true;
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if right {
            self.cycle_cursor();
            return true;
        }
        self.map_press(point);
        true
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        self.thaw_view();
        if self.fade_blocks()
            || !matches!(self.screen, Screen::Map)
            || !self.host.world.dialogue.open
            || self.talk.is_some()
        {
            return false;
        }
        if delta < 0.0 {
            self.option_scroll = (self.option_scroll + 1)
                .min(self.host.world.dialogue.options.len().saturating_sub(1));
        } else {
            self.option_scroll = self.option_scroll.saturating_sub(1);
        }
        true
    }
    pub(super) fn frame(&mut self, now: u64) -> AppResult<bool> {
        self.clock = now;
        // The world's own clock: the shell clock, run ahead by the
        // `enhanced` game speed (exactly the shell clock at 1.0x).
        let world_now = self.world_time(now);
        self.sync_volumes();
        // SPEC-014 §3.2: a game fade blocks completely: no world update,
        // animation, game clock, hover or background task runs until it
        // ends; only the fade advances, and sound keeps playing.
        // SPEC-014 §3.28 / §3.2: a frame that skips the world's tick on the
        // local map pauses its sequences, which resume where they stood.
        let local_map = !matches!(
            self.screen,
            Screen::World(_) | Screen::Town(_) | Screen::World2(_) | Screen::Town2(_)
        );
        if self.fade_blocks() {
            if local_map {
                self.host.pause_background(world_now);
            }
            let game = self.host.library.palette.clone();
            let changed = self
                .fades
                .tick(now, game.as_deref().unwrap_or(&self.palette));
            self.update_audio();
            if !self.fade_blocks() {
                self.fade_ended();
            }
            return Ok(changed);
        }
        // The debugger holds the world, as a dialog box does.
        if self.host.debug_paused {
            if local_map {
                self.host.pause_background(world_now);
            }
            return Ok(false);
        }
        let crosshair = self.sync_crosshair();
        // SPEC-010 §3.12: the head updater reads the speech position once
        // per tick.
        self.host.speech_clock(self.audio.speech_position());
        // The world and Pip-Boy screens are modal: the local map's scripts
        // and clock wait while travel or rest advances time. An open action
        // menu pauses background map processing.
        let mut world_ticked = false;
        let screen = match &self.screen {
            // M-W3 / SPEC-014 §3.13: a dialog box is modal over every
            // screen: world travel, encounters, the map and the windows
            // below it wait; audio, fades and the cursor below keep running.
            _ if self.dialog.is_some() => Ok(false),
            // A counter roll or an open action menu pauses the world.
            _ if self.bar_blocks() => Ok(false),
            Screen::Map if self.pointer.menu.is_some() || self.timer_open() => Ok(false),
            // SPEC-014 §3.46: floating texts go at the background update,
            // before the scripts may create new ones.
            Screen::Map => {
                world_ticked = true;
                let removed = self.floating_tick(world_now);
                self.host.tick(world_now).map(|changed| changed | removed)
            }
            Screen::World(_) | Screen::Town(_) => self
                .world_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            Screen::World2(_) | Screen::Town2(_) => self
                .world2_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            Screen::PipBoy(_) => self
                .pipboy_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            // Movies and the inventory, character, and barter screens are
            // modal: the world waits for them.
            Screen::Movie(_) => Ok(self.movie_tick(now)),
            Screen::Inventory(_) => Ok(self.inventory_tick()),
            Screen::Barter(_) => Ok(self.barter_tick()),
            Screen::Loot(_) => Ok(self.loot_tick()),
            Screen::Character(_) => Ok(self.character_tick(now)),
            // SPEC-013/014 windows suspend the world.
            Screen::Skilldex(_) => Ok(self.skilldex_tick()),
            Screen::Elevator(_) => Ok(self.elevator_tick(now)),
            Screen::Options(_) => Ok(self.options_tick(now)),
            Screen::Automap(_) | Screen::Help(_) => Ok(false),
            Screen::SaveLoad(_) => Ok(self.saveload_frame(now)),
        };
        if local_map && !world_ticked {
            self.host.pause_background(world_now);
        }
        let mut changed = match screen {
            Ok(changed) => changed | crosshair,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                tracing::warn!(%error,"world update failed");
                true
            }
        };
        changed |= self.talk_tick(now);
        self.update_audio();
        // The `enhanced` camera moves (and keeps the world's camera hex at
        // the hex nearest its center) before the map view follows it.
        changed |= self.enhanced_tick(now);
        if self.view.grid.pick_hex([320, 190]) != Some(self.host.world.camera_hex) {
            self.view.grid = HexGrid::centered(self.host.world.camera_hex, 640, 380)?;
            changed = true;
        }
        if self.view.elevation != self.host.world.elevation {
            self.view.elevation = self.host.world.elevation;
            changed = true;
        }
        self.dialogue_music_tick();
        if !self.host.world.effects.is_empty() {
            self.drain_effects();
            changed = true;
        }
        let game = self.host.library.palette.clone();
        changed |= self
            .fades
            .tick(now, game.as_deref().unwrap_or(&self.palette));
        changed |= self.pointer_tick();
        changed |= std::mem::take(&mut self.pointer.redraw);
        changed |= self.held_moved();
        changed |= self.counters_tick(now);
        changed |= self.panel_tick(now);
        changed |= self.watch_tick(now);
        changed |= self.called_tick();
        changed |= self.read_held_keys();
        changed |= self.replay_attack();
        self.cues_tick(now);
        changed |= self.debug_tick();
        changed |= self.qol_tick(now);
        Ok(changed)
    }
}
