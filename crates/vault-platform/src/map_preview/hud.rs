//! SPEC-014 §3.19-3.27 in the interactive world: the interface bar under
//! the 640-by-380 map view (message monitor, HP/AC counters, AP pips, the
//! active item slot, buttons and the combat panel), the indicator boxes,
//! the in-world cursor modes and their clicks, and the game-loop hotkeys.
use super::MapPreview;
use crate::interface::Painter;
use vault_core::{
    ObjectAction, ObjectRef,
    assets::WorldAssets,
    critical::hit_mode,
    progression, skills,
    ui::{
        Rect,
        bar::{self, Action, BarButton, Key},
        cursor::{self, Mode},
    },
};
use vault_formats::{
    Fid,
    pro::{Category, ExtendedFlags, ItemData, PrototypeData},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Interface state that lives with the map view.
pub(crate) struct Hud {
    pub monitor: bar::Monitor,
    pub mode: Mode,
    /// The status text last mirrored into the monitor.
    shown_status: Vec<u8>,
    /// Held modifiers: shift, control, alt.
    pub modifiers: [bool; 3],
    /// The player has had a turn in this combat: until then the pips keep
    /// their empty state (§3.22).
    turn_seen: bool,
}
impl Default for Hud {
    fn default() -> Self {
        Self {
            monitor: bar::Monitor::default(),
            mode: Mode::Movement,
            shown_status: Vec::new(),
            modifiers: [false; 3],
            turn_seen: false,
        }
    }
}

/// The game-loop key for a physical key (with Shift for the shifted digits).
pub(crate) fn game_key(key: KeyCode, [shift, control, _]: [bool; 3]) -> Option<Key> {
    use KeyCode::*;
    Some(match key {
        KeyQ if control => Key::CtrlQ,
        KeyX if control => Key::CtrlX,
        KeyP if control => Key::CtrlP,
        KeyS if control => Key::CtrlS,
        KeyL if control => Key::CtrlL,
        Tab => Key::Tab,
        Space => Key::Space,
        Enter | NumpadEnter => Key::Enter,
        Escape => Key::Escape,
        Home => Key::Home,
        F2 => Key::F2,
        F3 => Key::F3,
        F4 => Key::F4,
        F5 => Key::F5,
        F6 => Key::F6,
        F7 => Key::F7,
        F10 => Key::F10,
        F12 => Key::F12,
        Comma => Key::Char(','),
        Period => Key::Char('.'),
        Slash => Key::Char('/'),
        Minus | NumpadSubtract => Key::Char('-'),
        Equal | NumpadAdd => Key::Char('='),
        _ => {
            let name = format!("{key:?}");
            if let Some(letter) = name.strip_prefix("Key") {
                return Some(Key::Char(letter.chars().next()?.to_ascii_lowercase()));
            }
            let digit = name.strip_prefix("Digit")?.chars().next()?;
            if shift {
                Key::Char(b")!@#$%^&*("[digit.to_digit(10)? as usize] as char)
            } else {
                Key::Char(digit)
            }
        }
    })
}

impl MapPreview {
    /// Mirror a new status line into the monitor.
    fn sync_monitor(&mut self) {
        if self.status != self.hud.shown_status {
            self.hud.shown_status = self.status.clone();
            let status = self.status.clone();
            self.monitor_push(&status);
        }
    }
    /// A message for the monitor (and the status line); the monitor sound
    /// plays out of combat at most once per 500 ms.
    pub(crate) fn notify(&mut self, text: Vec<u8>) {
        self.monitor_push(&text);
        self.hud.shown_status = text.clone();
        self.status = text;
    }
    fn monitor_push(&mut self, text: &[u8]) {
        if text.is_empty() {
            return;
        }
        let font = self.host.assets.font_number(101);
        let width = |line: &[u8]| {
            font.as_ref()
                .map_or(line.len() as i32 * 6, |f| f.text_width(line, false) as i32)
        };
        let now = self.host.world.elapsed_ms;
        if self
            .hud
            .monitor
            .push(text, width, now, self.host.world.combat)
        {
            self.play_effect(bar::MONITOR_SOUND.as_bytes(), None);
        }
    }
    /// The 640-by-480 frame: the map view, the indicator boxes and the bar.
    pub(crate) fn compose_world(&mut self, view: IndexedImage) -> IndexedImage {
        self.sync_monitor();
        if !self.host.world.combat {
            self.hud.turn_seen = false;
        } else if self.host.world.combat_state.player_turn {
            self.hud.turn_seen = true;
        }
        let mut image = IndexedImage::new(640, 480).expect("fixed size");
        image.copy_image(&view, [0, 0]);
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        self.draw_indicators(&painter, &mut image);
        self.draw_bar(&painter, &mut image);
        image
    }
    fn draw_indicators(&self, painter: &Painter, image: &mut IndexedImage) {
        let sheet = self.host.world.player_sheet();
        let flags = sheet.map_or(0, |sheet| sheet.flags);
        let boxes = bar::indicators(
            flags & progression::ADDICTED_FLAG != 0,
            flags & 1 != 0,
            flags & progression::LEVEL_UP_FLAG != 0,
        );
        let [ox, oy] = bar::INDICATOR_ORIGIN;
        for (k, indicator) in boxes.iter().enumerate() {
            let x = ox + bar::indicator_x(k);
            if k == 0 {
                painter.blit_region(
                    image,
                    bar::INDICATOR_BOX.0,
                    Rect::new(3, 0, 127, 21),
                    [x, oy],
                    true,
                );
            } else {
                painter.blit_opaque(image, bar::INDICATOR_BOX.0, 0, [x, oy]);
            }
            let (id, key) = indicator.label();
            let label = painter.message("intrface.msg", id);
            let width = if k == 0 { 127 } else { 130 };
            painter.centered(
                image,
                101,
                key,
                &label,
                [x, oy + bar::INDICATOR_LABEL_Y],
                width,
            );
        }
    }
    fn draw_bar(&self, painter: &Painter, image: &mut IndexedImage) {
        let [bx, by] = bar::BAR_ORIGIN;
        painter.blit_opaque(image, bar::BAR_BACKGROUND, 0, [bx, by]);
        let at = |rect: Rect| [bx + rect.x, by + rect.y];
        let combat = self.host.world.combat;
        for (button, rect, up, _, _) in bar::BAR_BUTTONS {
            if matches!(button, BarButton::EndTurn | BarButton::EndCombat) && !combat {
                continue;
            }
            if button != BarButton::ItemSlot {
                painter.blit(image, up, 0, at(rect));
            }
        }
        // The message monitor: six lines, each one pixel further right.
        for (k, line) in self.hud.monitor.visible().iter().enumerate() {
            let x = bx + bar::monitor_line_x(k);
            let y = by + bar::MONITOR.y + 10 * k as i32;
            painter.text(image, 101, bar::MONITOR_COLOR, line, [x, y], bar::MONITOR.w);
        }
        // HP and AC counters.
        let player = self.host.world.player;
        let hp = self.host.world.hit_points(player).unwrap_or(0);
        let max = self.host.world.player_stat(7);
        let ac = self.host.world.player_stat(9);
        for (value, origin, band) in [
            (hp, bar::HP_COUNTER, bar::hp_band(hp, max)),
            (ac, bar::AC_COUNTER, bar::Band::White),
        ] {
            for (cell, glyph) in bar::COUNTER_CELLS.iter().zip(bar::counter_glyphs(value)) {
                painter.blit_region(
                    image,
                    "numbers.frm",
                    Rect::new(band.offset() + glyph.x(), 0, glyph.width(), 17),
                    [bx + origin[0] + cell, by + origin[1]],
                    true,
                );
            }
        }
        // AP pips: green on the player's turn, red otherwise, none outside
        // combat or before the player's first turn.
        let ap = (combat && self.hud.turn_seen).then(|| {
            self.host
                .world
                .critter_state(player)
                .map_or(0, |state| state.ap)
        });
        let your_turn = self.host.world.combat_state.player_turn;
        for (i, pip) in bar::pips(ap, your_turn).iter().enumerate() {
            let art = match pip {
                Some(bar::Pip::Green) => bar::PIP_ART.0,
                Some(bar::Pip::Red) => bar::PIP_ART.1,
                None => continue,
            };
            painter.blit_opaque(image, art, 0, [bx + bar::pip_x(i), by + bar::PIP_STRIP.y]);
        }
        self.draw_item_slot(painter, image);
        // The combat panel, lit green on the player's turn.
        if combat {
            let (art, origin) = bar::PANEL;
            painter.blit_opaque(image, art, 4, [bx + origin[0], by + origin[1]]);
            let light = if your_turn {
                bar::LIGHTS[0].0
            } else {
                bar::LIGHTS[1].0
            };
            painter.blit(image, light, 0, [bx + origin[0], by + origin[1]]);
        }
    }
    /// The active hand's item, if any.
    pub(crate) fn active_item(&self) -> Option<ObjectRef> {
        let mode = if self.host.world.active_hand == 0 {
            hit_mode::LEFT_PRIMARY
        } else {
            hit_mode::RIGHT_PRIMARY
        };
        self.host.world.wielded(self.host.world.player, mode)
    }
    fn draw_item_slot(&self, painter: &Painter, image: &mut IndexedImage) {
        let [bx, by] = bar::BAR_ORIGIN;
        let slot = bar::SLOT;
        let origin = [bx + slot.x, by + slot.y];
        let item = self.active_item();
        let weapon = item
            .and_then(|item| self.host.world.get(item))
            .and_then(|object| {
                object
                    .prototype
                    .as_ref()
                    .and_then(|prototype| match &prototype.data {
                        // The primary attack mode is the extended flags' low nibble.
                        PrototypeData::Item(data) => match &data.data {
                            ItemData::Weapon(weapon) => {
                                Some(((prototype.extended.0 & 15) as u8, weapon.ap_cost[0]))
                            }
                            _ => None,
                        },
                        _ => None,
                    })
            });
        let use_on = item
            .and_then(|item| self.host.world.get(item))
            .and_then(|object| object.prototype.as_ref())
            .is_some_and(|prototype| prototype.permits_use_on());
        let (action, cost) = match (item, weapon) {
            (None, _) => (
                bar::SlotAction::Attack(vault_core::combat::attack_mode::PUNCH),
                3,
            ),
            (Some(_), Some((mode, cost))) => (bar::SlotAction::Attack(mode), cost),
            (Some(_), None) if use_on => (bar::SlotAction::UseOn, 2),
            (Some(_), None) => (bar::SlotAction::Use, 2),
        };
        match bar::cost_digit(cost) {
            // No cost: the plain background replaces the labels.
            None => {
                painter.blit_opaque(image, bar::SLOT_DISABLED, 0, origin);
            }
            Some(digit) => {
                painter.blit_opaque(image, "sattkbup.frm", 0, origin);
                if let Some(label) = bar::slot_label(action)
                    && let Some([w, _]) = painter.size(label)
                {
                    let [x, y] = bar::slot_label_origin(w, false);
                    painter.blit(image, label, 0, [origin[0] + x, origin[1] + y]);
                }
                let (art, label_at, _) = bar::COST_LABEL;
                painter.blit(
                    image,
                    art,
                    0,
                    [origin[0] + label_at[0], origin[1] + label_at[1]],
                );
                // Digit cells are 10 wide; the last (9) is clipped to the
                // strip's 9 remaining columns (safe rule).
                let (digits, digit_at, _) = bar::COST_DIGIT;
                let width = if digit == 9 { 9 } else { 10 };
                painter.blit_region(
                    image,
                    digits,
                    Rect::new(10 * i32::from(digit), 0, width, 12),
                    [origin[0] + digit_at[0], origin[1] + digit_at[1]],
                    false,
                );
            }
        }
        let Some(item) = item else {
            return;
        };
        let inventory_art = self.host.world.get(item).and_then(|object| {
            object
                .prototype
                .as_ref()
                .and_then(|prototype| match &prototype.data {
                    PrototypeData::Item(data) => Some(data.inventory_art),
                    _ => None,
                })
        });
        // The item art, centered and (safe rule) clipped to the slot.
        if let Some(fid) = inventory_art
            && let Ok(art) = vault_core::assets::WorldAssets::object_art(&self.host.assets, fid)
            && let Some(frame) = art.frame(0, 0)
        {
            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
            let [x, y] = bar::item_art_origin(w, h);
            for row in 0..h {
                for column in 0..w {
                    let (sx, sy) = (x + column, y + row);
                    let color = frame.pixels()[(row * w + column) as usize];
                    if color == 0 || !(0..slot.w).contains(&sx) || !(0..slot.h).contains(&sy) {
                        continue;
                    }
                    let (dx, dy) = (origin[0] + sx, origin[1] + sy);
                    image.pixels_mut()[(dy * 640 + dx) as usize] = color;
                }
            }
        }
    }
    /// A press in the bar (screen y of at least 379). Returns whether it was
    /// handled.
    pub(crate) fn bar_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let [bx, by] = bar::BAR_ORIGIN;
        let local = [point[0] - bx, point[1] - by];
        if bar::MONITOR_OLDER.contains(local) {
            self.hud.monitor.older();
            return true;
        }
        if bar::MONITOR_NEWER.contains(local) {
            self.hud.monitor.newer();
            return true;
        }
        let combat = self.host.world.combat;
        let Some((button, ..)) = bar::BAR_BUTTONS.iter().find(|(button, rect, ..)| {
            rect.contains(local)
                && (combat || !matches!(button, BarButton::EndTurn | BarButton::EndCombat))
        }) else {
            return local[1] >= 0;
        };
        let (press, release) = if *button == BarButton::ItemSlot {
            bar::SLOT_SOUNDS
        } else {
            bar::BUTTON_SOUNDS
        };
        self.press_cue(press.as_bytes(), release.as_bytes());
        let _ = match button {
            BarButton::ItemSlot if right => self.game_action(Action::CycleItemMode),
            BarButton::ItemSlot => {
                self.use_item_slot();
                true
            }
            BarButton::Inventory => self.game_action(Action::Inventory),
            BarButton::Options => self.game_action(Action::Options),
            BarButton::Skilldex => self.game_action(Action::Skilldex),
            BarButton::Map => self.game_action(Action::Automap),
            BarButton::Character => self.game_action(Action::Character),
            BarButton::PipBoy => self.game_action(Action::PipBoy),
            BarButton::SwapHands => self.game_action(Action::SwapHands),
            BarButton::EndTurn => {
                self.host.end_player_turn();
                true
            }
            BarButton::EndCombat => {
                let mut ai = std::mem::replace(
                    &mut self.host.combat_ai,
                    Box::new(vault_core::combat_state::NoAi),
                );
                self.host.request_end_combat(&mut *ai);
                self.host.combat_ai = ai;
                true
            }
        };
        true
    }
    /// §3.23 left click on the item slot: a weapon sets the crosshair (and
    /// starts combat), a non-weapon the use crosshair.
    fn use_item_slot(&mut self) {
        let weapon = self.active_item().is_none_or(|item| {
            self.host
                .world
                .item_subtype(item)
                .is_some_and(|subtype| subtype == vault_formats::pro::ItemSubtype::Weapon)
        });
        self.hud.mode = if weapon {
            Mode::Crosshair
        } else {
            Mode::UseCrosshair
        };
    }
    /// One game-loop action (SPEC-014 §3.19). Returns whether it acted.
    pub(crate) fn game_action(&mut self, action: Action) -> bool {
        match action {
            Action::Inventory => self.open_inventory(),
            Action::Character => self.open_character(),
            Action::PipBoy | Action::AlarmClock => {
                if self.host.world.combat {
                    self.play_effect(vault_core::ui::ERROR_SOUND.as_bytes(), None);
                    let text = self.message("misc.msg", 7);
                    self.notify(text);
                } else {
                    self.open_pipboy();
                }
            }
            Action::Skilldex => self.open_skilldex(),
            Action::Options => self.open_options(),
            Action::Pause => self.open_pause(),
            Action::QuitConfirm => self.open_quit_confirm(),
            Action::Save => self.open_save_screen(vault_core::ui::saveload::Mode::Save),
            Action::Load => self.open_save_screen(vault_core::ui::saveload::Mode::LoadInGame),
            Action::QuickSave => self.quick_save(),
            Action::QuickLoad => self.quick_load(),
            Action::SwapHands => {
                self.host.world.active_hand ^= 1;
                if matches!(self.hud.mode, Mode::Crosshair | Mode::UseCrosshair) {
                    self.hud.mode = Mode::Movement;
                }
            }
            Action::CycleCursor => self.cycle_cursor(),
            Action::CycleItemMode => {}
            Action::Automap => self.open_automap(),
            Action::StartCombat => {
                if !self.host.world.combat {
                    let player = self.host.world.player;
                    self.host.world.pending_combat =
                        Some(vault_core::combat_state::CombatRequest {
                            attacker: player,
                            defender: 0,
                            accuracy: 0,
                            damage: [0, i32::MAX],
                            flags: None,
                        });
                }
            }
            Action::Skill(row) => {
                if let Some(index) = skills::skill_hotkey(row) {
                    self.play_effect(vault_core::ui::BUTTON_PRESS.as_bytes(), None);
                    self.choose_skill(index);
                }
            }
            Action::CenterPlayer => {
                let player = self.host.world.player;
                if let Some(hex) = self.host.world.get(player).and_then(|o| o.raw.hex) {
                    self.host.world.camera_hex = hex;
                }
            }
            Action::Rotate(clockwise) => {
                let player = self.host.world.player;
                if let Some(object) = self.host.world.get_mut(player) {
                    object.raw.orientation =
                        (object.raw.orientation + if clockwise { 1 } else { 5 }) % 6;
                }
            }
            Action::Brightness(step) => self.brightness_step(step > 0),
            Action::DateTime => {
                let text = self.clock_text();
                self.notify(text);
            }
            Action::MasterVolume(_) | Action::Screenshot => return false,
        }
        true
    }
    pub(crate) fn message(&self, file: &str, id: i32) -> Vec<u8> {
        Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        }
        .message(file, id)
    }
    /// A right-click in the map area or `m` (SPEC-014 §3.27).
    pub(crate) fn cycle_cursor(&mut self) {
        let weapon = self.active_item().is_none_or(|item| {
            self.host.world.item_subtype(item) == Some(vault_formats::pro::ItemSubtype::Weapon)
        });
        self.hud.mode = cursor::cycle_mode(self.hud.mode, self.host.world.combat, weapon);
    }
    /// Whether a critter can be talked to: the prototype's talk flag and an
    /// active critter (SPEC-013 §3.16).
    fn talkable(&self, critter: ObjectRef) -> bool {
        let flag = self.host.world.get(critter).is_some_and(|object| {
            object
                .prototype
                .as_ref()
                .is_some_and(|prototype| prototype.extended.contains(ExtendedFlags::TALK))
        });
        flag && self
            .host
            .world
            .critter_state(critter)
            .is_some_and(|state| state.results.0 & (0x01 | 0x80 | 0x8000) == 0)
    }
    /// The SPEC-014 §3.27 hover target kind of an object.
    pub(crate) fn target_kind(&self, object: ObjectRef) -> Option<cursor::Target> {
        let found = self.host.world.get(object)?;
        let usable = found.prototype.as_ref().is_some_and(|p| p.permits_use());
        Some(match found.category() {
            Category::Item => cursor::Target::Item {
                container: matches!(
                    found.prototype.as_ref().map(|p| &p.data),
                    Some(PrototypeData::Item(item)) if matches!(item.data, ItemData::Container { .. })
                ),
            },
            Category::Critter if object == self.host.world.player => cursor::Target::Player,
            Category::Critter => cursor::Target::Critter {
                talkable: self.talkable(object),
            },
            Category::Scenery => cursor::Target::Scenery { usable },
            _ => cursor::Target::Wall { usable },
        })
    }
    /// The player's attack request (SPEC-007 §3.6): an empty active hand
    /// requests punch (4), else the hand's primary mode.
    pub(crate) fn attack_mode(&self) -> u8 {
        if self.active_item().is_none() {
            hit_mode::PUNCH
        } else if self.host.world.active_hand == 0 {
            hit_mode::LEFT_PRIMARY
        } else {
            hit_mode::RIGHT_PRIMARY
        }
    }
    /// A left release in the map area by cursor mode (SPEC-014 §3.27).
    pub(crate) fn map_click(&mut self, point: [i32; 2], object: Option<ObjectRef>) {
        let player = self.host.world.player;
        match self.hud.mode {
            Mode::Movement => {
                if let Some(hex) = self.view.grid.pick_hex(point) {
                    match self.host.walk_to(player, hex) {
                        Ok(true) => {}
                        Ok(false) => self.status = b"You cannot get there".to_vec(),
                        Err(error) => self.status = error.to_string().into_bytes(),
                    }
                }
            }
            Mode::Arrow => {
                let Some(object) = object else {
                    return;
                };
                self.selected = Some(object);
                let Some(target) = self.target_kind(object) else {
                    return;
                };
                let action = match cursor::primary_action(target, self.host.world.combat) {
                    cursor::Primary::Use if matches!(target, cursor::Target::Item { .. }) => {
                        ObjectAction::Pickup
                    }
                    cursor::Primary::Use => ObjectAction::Use,
                    cursor::Primary::Talk => ObjectAction::Talk,
                    cursor::Primary::Look => ObjectAction::Description,
                    cursor::Primary::Rotate => {
                        self.game_action(Action::Rotate(true));
                        return;
                    }
                };
                self.interact(action);
            }
            Mode::Crosshair => {
                let Some(target) = object.filter(|o| {
                    *o != player
                        && self
                            .host
                            .world
                            .get(*o)
                            .is_some_and(|o| Fid(o.raw.art_id).category() == 1)
                }) else {
                    return;
                };
                if self.host.world.combat {
                    // The kick draw follows the player's preflight (no draws).
                    let mode = self.attack_mode();
                    let uncalled = vault_core::critical::location::UNCALLED;
                    let preflight = self
                        .host
                        .world
                        .attack_profile(player, target, mode, uncalled, false)
                        .map(|attack| self.host.world.preflight(player, target, &attack));
                    match preflight {
                        Ok(Err(refusal)) => {
                            self.status = format!("{refusal:?}").into_bytes();
                            return;
                        }
                        Err(error) => {
                            self.status = error.to_string().into_bytes();
                            return;
                        }
                        Ok(Ok(())) => {}
                    }
                    match self
                        .host
                        .perform_attack(player, target, mode, uncalled, false)
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(refusal)) => self.status = format!("{refusal:?}").into_bytes(),
                        Err(error) => self.status = error.to_string().into_bytes(),
                    }
                } else {
                    self.host.world.pending_combat =
                        Some(vault_core::combat_state::CombatRequest {
                            attacker: player,
                            defender: target,
                            accuracy: 0,
                            damage: [0, i32::MAX],
                            flags: None,
                        });
                }
            }
            Mode::UseCrosshair => {
                if let (Some(target), Some(item)) = (object, self.active_item()) {
                    self.selected = Some(target);
                    self.interact(ObjectAction::UseObject(item));
                    self.hud.mode = Mode::Movement;
                }
            }
            Mode::Skill(index) => {
                // SPEC-013 §3.1: no object returns to movement; an accepted
                // request returns to movement, a rejected one stays.
                let Some(target) = object else {
                    self.hud.mode = Mode::Movement;
                    return;
                };
                match self.host.use_skill_on(player, target, i32::from(index)) {
                    Ok(true) => self.hud.mode = Mode::Movement,
                    Ok(false) => {}
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
                self.drain_effects();
            }
        }
    }
    /// A chosen skill: Sneak toggles (after clearing the player's
    /// animation); any other enters its targeting mode.
    pub(crate) fn choose_skill(&mut self, index: i32) {
        if index == skills::skill::SNEAK {
            if let Err(error) = self.host.toggle_sneak() {
                self.status = error.to_string().into_bytes();
            }
            return;
        }
        if let Ok(skill) = u8::try_from(index) {
            self.hud.mode = Mode::Skill(skill);
        }
    }
    /// The cursor-mode label for the window title.
    pub(crate) fn mode_name(&self) -> String {
        match self.hud.mode {
            Mode::Movement => "movement".into(),
            Mode::Arrow => "arrow".into(),
            Mode::Crosshair => "crosshair".into(),
            Mode::UseCrosshair => "use".into(),
            Mode::Skill(skill) => format!("skill {skill}"),
        }
    }
    /// Escape on the map opens the options menu; other bar hotkeys act
    /// through `game_action`. Returns whether the key was handled.
    pub(crate) fn game_hotkey(&mut self, key: KeyCode) -> bool {
        let Some(key) = game_key(key, self.hud.modifiers) else {
            return false;
        };
        let enabled = !self.host.world.ui_disabled;
        match bar::game_key(key, enabled, self.hud.modifiers[2]) {
            Some(action) => {
                if !matches!(action, Action::Skill(_)) {
                    self.play_effect(vault_core::ui::BUTTON_PRESS.as_bytes(), None);
                }
                self.game_action(action)
            }
            // The combat input loop: Space ends the turn, Enter requests
            // the end of combat.
            None if self.host.world.combat && key == Key::Space => {
                self.host.end_player_turn();
                true
            }
            None if self.host.world.combat && key == Key::Enter => {
                let mut ai = std::mem::replace(
                    &mut self.host.combat_ai,
                    Box::new(vault_core::combat_state::NoAi),
                );
                self.host.request_end_combat(&mut *ai);
                self.host.combat_ai = ai;
                true
            }
            None => false,
        }
    }
    /// The menu, selection and editor share brightness (SPEC-014 §3.18).
    /// Brightness hotkeys apply, store and write the configuration at once.
    pub(crate) fn brightness_step(&mut self, up: bool) {
        self.preferences.brightness =
            vault_core::ui::options::brightness_step(self.preferences.brightness, up);
        self.apply_preferences();
    }
}
