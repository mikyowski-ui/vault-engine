//! SPEC-011 inventory screen: the player's entries with quantities, worn
//! and held markers, weights, and caps; the selected entry can be used
//! (drugs consume through §3.6), wielded or worn (§3.10), reloaded from
//! the inventory, or unloaded (§3.15). Opening it in combat costs the
//! §3.11 action points. SPEC-013: using a kit applies it to the player
//! (§3.13), and using an explosive opens the Set Timer dialog (§3.10, §3.15).
//!
//! An entry can also be dragged: a left press lifts it (`ipickup1`) and the
//! release (`iputdown`) drops it. Dropped on an action button it runs that
//! action; ammunition dropped on a weapon's row is the §3.15 drag reload,
//! asking for the count through the move-items counter for a stack. Opened
//! from the action menu's Inventory entry (SPEC-014 §3.28), Use applies
//! the item to that target. The list layout itself is provisional: no
//! approved spec gives the inventory window's art or geometry.
use super::{
    MapPreview, Screen, button,
    counter::{CounterInput, counter_click, counter_key, draw_counter},
    panel_text,
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectAction, ObjectRef,
    skills::{self, Counter, SetTimer},
    ui::{BUTTON_PRESS, ColorKey, ERROR_SOUND, Rect},
};
use vault_formats::pro::{BasicFlags, ItemData, PrototypeData};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const ROWS: [i32; 2] = [20, 60];
const LINE: i32 = 16;
const VISIBLE: usize = 22;
const ACTIONS: [&[u8]; 5] = [b"Use", b"Wield", b"Reload", b"Unload", b"Close"];

pub(crate) struct InventoryScreen {
    pub selected: Option<usize>,
    /// The Set Timer dialog for an explosive being armed.
    pub timer: Option<(ObjectRef, SetTimer)>,
    /// The action menu's target for Use.
    pub target: Option<ObjectRef>,
    /// The lifted entry: its row and item.
    pub held: Option<(usize, ObjectRef)>,
    /// The drag-reload count: the counter, the weapon and the ammunition.
    pub counter: Option<(Counter, ObjectRef, ObjectRef)>,
}
impl InventoryScreen {
    /// The item a drag holds (the counter shows the arrow instead).
    pub fn dragged(&self) -> Option<ObjectRef> {
        self.held
            .filter(|_| self.counter.is_none())
            .map(|(_, item)| item)
    }
}

/// §3.15 Set Timer layout (window at screen (140, 80)).
const TIMER_ORIGIN: [i32; 2] = [140, 80];
const TIMER_PLUS: Rect = Rect::new(194, 64, 16, 12);
const TIMER_MINUS: Rect = Rect::new(194, 76, 17, 12);
const TIMER_DONE: Rect = Rect::new(98, 128, 15, 16);
const TIMER_CANCEL: Rect = Rect::new(148, 128, 15, 16);

fn row(index: usize) -> [i32; 4] {
    [ROWS[0], ROWS[1] + index as i32 * LINE, 400, LINE - 2]
}
fn action_button(index: usize) -> [i32; 4] {
    [440, 60 + 30 * index as i32, 160, 20]
}
pub(crate) fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

impl MapPreview {
    /// Open the inventory; in combat its AP cost may refuse it.
    pub(crate) fn open_inventory(&mut self) {
        self.open_inventory_on(None);
    }
    pub(crate) fn open_inventory_on(&mut self, target: Option<ObjectRef>) {
        if self.host.open_inventory() {
            self.screen = Screen::Inventory(InventoryScreen {
                selected: None,
                timer: None,
                target,
                held: None,
                counter: None,
            });
        }
        self.drain_effects();
    }
    fn player_entries(&self) -> Vec<(ObjectRef, i32)> {
        self.host
            .world
            .get(self.host.world.player)
            .map(|player| player.inventory.clone())
            .unwrap_or_default()
    }
    fn selected_item(&self) -> Option<ObjectRef> {
        let Screen::Inventory(screen) = &self.screen else {
            return None;
        };
        let entries = self.player_entries();
        screen
            .selected
            .and_then(|index| entries.get(index))
            .map(|(item, _)| *item)
    }
    fn item_kind(&self, item: ObjectRef) -> Option<&ItemData> {
        match &self.host.world.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(data) => Some(&data.data),
            _ => None,
        }
    }
    /// Enter accepts the timer (arming the explosive), Esc cancels.
    fn finish_timer(&mut self, accept: bool) {
        let Screen::Inventory(screen) = &mut self.screen else {
            return;
        };
        let Some((item, timer)) = screen.timer.take() else {
            return;
        };
        if !accept {
            return;
        }
        if !timer.accepts() {
            self.play_effect(ERROR_SOUND.as_bytes(), None);
            return;
        }
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        if let Err(error) = self.host.arm_explosive(item, timer.seconds) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    pub(crate) fn inventory_key(&mut self, key: KeyCode) -> bool {
        let count = self.player_entries().len();
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, ..)) = &mut screen.counter {
            let input = counter_key(counter, key);
            if matches!(input, CounterInput::Accept(_)) {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            return self.inventory_counter(input);
        }
        if screen.timer.is_some() {
            match key {
                KeyCode::Enter | KeyCode::NumpadEnter => self.finish_timer(true),
                KeyCode::Escape => self.finish_timer(false),
                _ => return false,
            }
            return true;
        }
        match key {
            KeyCode::Escape | KeyCode::KeyI => self.return_to_map(),
            KeyCode::ArrowDown if count > 0 => {
                screen.selected = Some(
                    screen
                        .selected
                        .map_or(0, |index| (index + 1).min(count - 1)),
                );
            }
            KeyCode::ArrowUp if count > 0 => {
                screen.selected = Some(screen.selected.map_or(0, |index| index.saturating_sub(1)));
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn inventory_click(&mut self, point: [i32; 2]) -> bool {
        if let Screen::Inventory(InventoryScreen {
            counter: Some((counter, ..)),
            ..
        }) = &mut self.screen
        {
            let input = counter_click(counter, point);
            if input != CounterInput::Ignored {
                self.press();
            }
            self.inventory_counter(input);
            return true;
        }
        if let Screen::Inventory(screen) = &mut self.screen {
            screen.held = None;
        }
        if let Screen::Inventory(InventoryScreen {
            timer: Some((_, timer)),
            ..
        }) = &mut self.screen
        {
            let local = [point[0] - TIMER_ORIGIN[0], point[1] - TIMER_ORIGIN[1]];
            if TIMER_PLUS.contains(local) {
                timer.plus();
            } else if TIMER_MINUS.contains(local) {
                timer.minus();
            } else if TIMER_DONE.contains(local) {
                self.finish_timer(true);
            } else if TIMER_CANCEL.contains(local) {
                self.finish_timer(false);
            }
            return true;
        }
        let entries = self.player_entries();
        let count = entries.len().min(VISIBLE);
        if let Some(index) = (0..count).find(|index| inside(row(*index), point)) {
            // The press selects the entry and lifts it.
            if let Screen::Inventory(screen) = &mut self.screen {
                screen.selected = Some(index);
                screen.held = Some((index, entries[index].0));
            }
            self.play_effect(b"ipickup1", None);
            return true;
        }
        let Some(action) = (0..ACTIONS.len()).find(|index| inside(action_button(*index), point))
        else {
            return false;
        };
        self.press();
        self.inventory_action(action);
        true
    }
    /// The release ends a drag: on an action button that action runs on
    /// the entry; ammunition on a weapon's row reloads it.
    pub(crate) fn inventory_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if screen.counter.is_some() {
            return false;
        }
        let Some((from, ammo)) = screen.held.take() else {
            return false;
        };
        let Some(point) = point else {
            return true;
        };
        let entries = self.player_entries();
        let over_row = (0..entries.len().min(VISIBLE)).find(|index| inside(row(*index), point));
        if over_row == Some(from) {
            // A click: the selection stands.
            return true;
        }
        self.play_effect(b"iputdown", None);
        if let Some(index) = over_row {
            let weapon = entries[index].0;
            let quantity = entries[from].1;
            let reloads = matches!(self.item_kind(ammo), Some(ItemData::Ammunition { .. }))
                && matches!(self.item_kind(weapon), Some(ItemData::Weapon(_)));
            if !reloads {
                return true;
            }
            if quantity > 1 {
                if let Screen::Inventory(screen) = &mut self.screen {
                    screen.held = Some((from, ammo));
                    screen.counter = Some((Counter::new(quantity), weapon, ammo));
                }
            } else {
                self.drag_reload(weapon, ammo, 1);
            }
            return true;
        }
        if let Some(action) = (0..ACTIONS.len()).find(|index| inside(action_button(*index), point))
        {
            if let Screen::Inventory(screen) = &mut self.screen {
                screen.selected = Some(from);
            }
            self.inventory_action(action);
        }
        true
    }
    /// The drag-reload counter's outcome.
    fn inventory_counter(&mut self, input: CounterInput) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        match input {
            CounterInput::Accept(count) => {
                if let Some((_, weapon, ammo)) = screen.counter.take() {
                    screen.held = None;
                    self.drag_reload(weapon, ammo, count);
                }
            }
            CounterInput::Refused => self.play_effect(ERROR_SOUND.as_bytes(), None),
            CounterInput::Cancel => {
                screen.counter = None;
                screen.held = None;
            }
            CounterInput::Changed => {}
            CounterInput::Ignored => return false,
        }
        true
    }
    /// SPEC-011 §3.15 drag reload of `weapon` from up to `count` units.
    fn drag_reload(&mut self, weapon: ObjectRef, ammo: ObjectRef, count: i32) {
        let player = self.host.world.player;
        match self.host.drag_reload(player, weapon, ammo, count) {
            Ok(true) => {}
            Ok(false) => self.status = b"The weapon did not load".to_vec(),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.drain_effects();
        self.clamp_selection();
    }
    fn clamp_selection(&mut self) {
        let count = self.player_entries().len();
        if let Screen::Inventory(screen) = &mut self.screen
            && screen.selected.is_some_and(|index| index >= count)
        {
            screen.selected = count.checked_sub(1);
        }
    }
    /// One action button on the selected entry.
    fn inventory_action(&mut self, action: usize) {
        if action == 4 {
            self.return_to_map();
            return;
        }
        let Some(item) = self.selected_item() else {
            self.status = b"Select an item first".to_vec();
            return;
        };
        let player = self.host.world.player;
        // From the action menu, Use applies the item to its target.
        if let Screen::Inventory(screen) = &self.screen
            && action == 0
            && let Some(target) = screen.target
        {
            self.return_to_map();
            self.selected = Some(target);
            self.interact(ObjectAction::UseObject(item));
            return;
        }
        let pid = self.host.world.get(item).map_or(0, |o| o.raw.pid);
        if action == 0 && skills::is_explosive(pid) {
            if self.host.explosive_armable(item)
                && let Screen::Inventory(screen) = &mut self.screen
            {
                screen.timer = Some((item, SetTimer::default()));
            }
            self.drain_effects();
            return;
        }
        if action == 0 && self.host.kit_skill(pid).is_some() {
            if let Err(error) = self.host.use_kit(player, item, player) {
                self.status = error.to_string().into_bytes();
            }
            self.drain_effects();
            return;
        }
        let drug = matches!(self.item_kind(item), Some(ItemData::Drug(_)));
        let weapon = matches!(self.item_kind(item), Some(ItemData::Weapon(_)));
        let result = match action {
            0 if drug => self.host.use_drug(player, item).map(|_| ()),
            0 => self.host.action(player, item, ObjectAction::Use),
            1 => self.host.wield_object(player, item),
            2 if weapon => self.host.world.search_reload(player, item).map(|reloaded| {
                if !reloaded {
                    self.status = b"No compatible ammunition".to_vec();
                }
            }),
            3 if weapon => self.host.world.unload_weapon(item).map(|_| ()),
            _ => {
                self.status = b"That item is not a weapon".to_vec();
                Ok(())
            }
        };
        if let Err(error) = result {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
        // The selection follows the entry list, which may have shrunk.
        self.clamp_selection();
    }
    pub(crate) fn inventory_image(&self) -> AppResult<IndexedImage> {
        let Screen::Inventory(screen) = &self.screen else {
            return Err("inventory is not open".into());
        };
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let Some(font) = &self.font else {
            return Ok(image);
        };
        let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
        let white = palette.lookup_rgb5([31, 31, 31]).unwrap_or(1);
        let world = &self.host.world;
        let player = world.player;
        panel_text(
            &mut image,
            font,
            palette,
            b"Inventory",
            [ROWS[0], 30],
            white,
        );
        for (index, (item, quantity)) in self.player_entries().iter().take(VISIBLE).enumerate() {
            let Some(object) = world.get(*item) else {
                continue;
            };
            let flags = object.raw.flags;
            let marker = if flags.contains(BasicFlags::WORN) {
                " [worn]"
            } else if flags.contains(BasicFlags::RIGHT_HAND) {
                " [right hand]"
            } else if flags.contains(BasicFlags::LEFT_HAND) {
                " [left hand]"
            } else {
                ""
            };
            let line = [
                object.name.as_slice(),
                format!(" x{quantity}{marker}  ({} lb)", world.item_weight(*item)).as_bytes(),
            ]
            .concat();
            let rect = row(index);
            let selected = screen.selected == Some(index);
            if selected {
                image.fill_rect(rect, palette.lookup_rgb5([0, 8, 0]).unwrap_or(0));
            }
            panel_text(
                &mut image,
                font,
                palette,
                &line,
                [rect[0] + 2, rect[1] + 1],
                if selected { white } else { green },
            );
        }
        for (index, label) in ACTIONS.iter().enumerate() {
            button(&mut image, font, palette, label, action_button(index));
        }
        let carry = world
            .player_sheet()
            .map_or(0, |sheet| sheet.stat(12, world.context(player)));
        let summary = format!(
            "Caps: {}   Weight: {} / {} lb",
            world.caps(player),
            world.inventory_weight(player),
            carry
        );
        panel_text(
            &mut image,
            font,
            palette,
            summary.as_bytes(),
            [440, 230],
            green,
        );
        panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        if let Some((_, timer)) = &screen.timer {
            let painter = Painter {
                assets: &self.host.assets,
                palette,
            };
            let [ox, oy] = TIMER_ORIGIN;
            painter.blit_opaque(&mut image, "movemult.frm", 0, [ox, oy]);
            let title = painter.message("inventry.msg", 23);
            painter.centered(&mut image, 103, ColorKey::AMBER, &title, [ox, oy + 9], 259);
            painter.blit(&mut image, "timer.frm", 0, [ox + 113, oy + 34]);
            for (x, digit) in timer.digits() {
                painter.big_digit(&mut image, digit, [ox + x, oy + 64], false);
            }
            painter.blit(
                &mut image,
                "splsoff.frm",
                0,
                [ox + TIMER_PLUS.x, oy + TIMER_PLUS.y],
            );
            painter.blit(
                &mut image,
                "snegoff.frm",
                0,
                [ox + TIMER_MINUS.x, oy + TIMER_MINUS.y],
            );
            for rect in [TIMER_DONE, TIMER_CANCEL] {
                painter.blit(&mut image, "lilredup.frm", 0, [ox + rect.x, oy + rect.y]);
            }
        }
        if let Some((counter, _, ammo)) = &screen.counter {
            let painter = Painter {
                assets: &self.host.assets,
                palette,
            };
            let icon = self.item_icon(*ammo);
            draw_counter(
                &painter,
                &mut image,
                counter,
                icon.as_ref().and_then(|icon| icon.frame(0, 0)),
            );
        }
        Ok(image)
    }
}
