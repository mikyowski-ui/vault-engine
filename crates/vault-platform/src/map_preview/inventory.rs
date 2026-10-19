//! SPEC-014 §3.34 the player inventory window at screen (80, 0), with the
//! SPEC-011 rules behind its moves: opening it in combat costs the §3.11
//! action points; using a drug consumes it (§3.6), a kit heals the player
//! (SPEC-013 §3.13), an explosive opens the Set Timer dialog (SPEC-013
//! §3.10, §3.15), and a Motion Sensor opens the automap (SPEC-014 §3.33).
//!
//! The window opens in the hand cursor. A left press on a filled slot (a
//! list row, a hand, the armor) lifts the item (`ipickup1`); the release
//! drops it (`iputdown`) on the first target in the §3.34 order: the list
//! (into a container, onto a weapon it reloads, else back into the list),
//! the left hand, the right hand, the armor slot, or nowhere. There is no
//! two-click drag. A right press switches to the arrow cursor, where a
//! quick click shows the item's examine text and a held press opens its
//! menu (Look, Unload or Use, Drop, Cancel). Opened from the world action
//! menu's Inventory entry, a press on a list row uses that item on the
//! target and closes the window.
//!
//! No approved spec gives the window's background art, the stats panel's
//! place, the scroll or Done buttons, or the Unload and Drop icon art, so
//! those are provisional: a dark panel, the examine text at (345, 40), the
//! arrow keys for scrolling, a red Done button at (500, 340) and labeled
//! boxes for the two icons.
use super::{
    MapPreview, Screen,
    counter::{CounterInput, counter_click, counter_key, draw_counter},
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectAction, ObjectRef, WorldEffect,
    assets::WorldAssets,
    items::EQUIPPED,
    skills::{self, Counter, ItemEntry, SetTimer},
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND, Rect,
        automap::MOTION_SENSOR,
        cursor::{self, MenuTracker},
        inventory::{self as layout, DropTarget},
    },
};
use vault_formats::{
    Fid,
    pro::{BasicFlags, ItemData, PrototypeData},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Where a lifted item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    List,
    LeftHand,
    RightHand,
    Armor,
}
impl Slot {
    fn flag(self) -> Option<u32> {
        match self {
            Self::List => None,
            Self::LeftHand => Some(BasicFlags::LEFT_HAND.0),
            Self::RightHand => Some(BasicFlags::RIGHT_HAND.0),
            Self::Armor => Some(BasicFlags::WORN.0),
        }
    }
}

/// What an open counter is counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Count {
    /// The §3.15 drag reload of `weapon` from `ammo`.
    Reload { weapon: ObjectRef, ammo: ObjectRef },
    /// `item` into `container`.
    Into {
        item: ObjectRef,
        container: ObjectRef,
    },
    /// `item` dropped on the player's hex.
    Drop { item: ObjectRef },
}

/// An arrow-cursor press on an item, waiting for its release or the hold
/// pulse.
#[derive(Debug, Clone, Copy)]
struct ArrowPress {
    item: ObjectRef,
    next_pulse: u64,
}

/// The arrow cursor's item menu.
#[derive(Debug, Clone)]
pub(crate) struct ItemMenu {
    item: ObjectRef,
    pub entries: Vec<ItemEntry>,
    pub tracker: MenuTracker,
    origin: [i32; 2],
}

pub(crate) struct InventoryScreen {
    /// The world action menu's target: the use-on window.
    pub target: Option<ObjectRef>,
    /// The list's scroll offset.
    offset: usize,
    /// The arrow cursor (else the hand).
    pub arrow: bool,
    /// The lifted item and its source slot.
    pub held: Option<(Slot, ObjectRef)>,
    press: Option<ArrowPress>,
    pub menu: Option<ItemMenu>,
    pub counter: Option<(Counter, Count)>,
    /// The Set Timer dialog for an explosive being armed.
    pub timer: Option<(ObjectRef, SetTimer)>,
    /// The stats panel's examine text.
    examine: Vec<u8>,
}
impl InventoryScreen {
    /// The item a drag holds (the counter shows the arrow instead).
    pub fn dragged(&self) -> Option<ObjectRef> {
        self.held
            .filter(|_| self.counter.is_none())
            .map(|(_, item)| item)
    }
    /// The cursor art: none while the menu is open, the arrow over the
    /// counter, else the mode's hand or arrow.
    pub fn cursor(&self) -> Option<&'static str> {
        if self.menu.is_some() {
            None
        } else if self.counter.is_some() || self.timer.is_some() || self.arrow {
            Some(layout::ARROW_CURSOR)
        } else {
            Some(layout::HAND_CURSOR)
        }
    }
}

/// §3.15 Set Timer layout (window at screen (140, 80)).
const TIMER_ORIGIN: [i32; 2] = [140, 80];
const TIMER_PLUS: Rect = Rect::new(194, 64, 16, 12);
const TIMER_MINUS: Rect = Rect::new(194, 76, 17, 12);
const TIMER_DONE: Rect = Rect::new(98, 128, 15, 16);
const TIMER_CANCEL: Rect = Rect::new(148, 128, 15, 16);
/// A screen rectangle `[x, y, w, h]` contains `point`.
pub(crate) fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

/// Provisional elements (see the module documentation).
const PANEL: [i32; 4] = [80, 0, 500, 377];
const EXAMINE: [i32; 2] = [345, 40];
const DONE: Rect = Rect::new(500, 340, 15, 16);

impl MapPreview {
    /// Open the inventory; in combat its AP cost may refuse it.
    pub(crate) fn open_inventory(&mut self) {
        self.open_inventory_on(None);
    }
    pub(crate) fn open_inventory_on(&mut self, target: Option<ObjectRef>) {
        if self.host.open_inventory() {
            self.screen = Screen::Inventory(InventoryScreen {
                target,
                offset: 0,
                arrow: false,
                held: None,
                press: None,
                menu: None,
                counter: None,
                timer: None,
                examine: Vec::new(),
            });
        }
        self.drain_effects();
    }
    /// The list: the player's entries that no slot holds.
    fn list_entries(&self) -> Vec<(ObjectRef, i32)> {
        let world = &self.host.world;
        world
            .get(world.player)
            .map(|player| {
                player
                    .inventory
                    .iter()
                    .copied()
                    .filter(|(item, _)| {
                        world
                            .get(*item)
                            .is_some_and(|object| object.raw.flags.0 & EQUIPPED == 0)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    /// The item a slot holds.
    fn slot_item(&self, slot: Slot) -> Option<ObjectRef> {
        let flag = slot.flag()?;
        let world = &self.host.world;
        world
            .get(world.player)?
            .inventory
            .iter()
            .find_map(|(item, _)| (world.get(*item)?.raw.flags.0 & flag != 0).then_some(*item))
    }
    fn item_kind(&self, item: ObjectRef) -> Option<&ItemData> {
        match &self.host.world.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(data) => Some(&data.data),
            _ => None,
        }
    }
    fn entry_quantity(&self, item: ObjectRef) -> i32 {
        let world = &self.host.world;
        world
            .get(world.player)
            .and_then(|player| player.inventory.iter().find(|(entry, _)| *entry == item))
            .map_or(1, |(_, quantity)| *quantity)
    }
    /// The filled slot under a point: a list row (at the scroll offset), a
    /// hand or the armor.
    fn slot_at(&self, point: [i32; 2]) -> Option<(Slot, ObjectRef)> {
        let Screen::Inventory(screen) = &self.screen else {
            return None;
        };
        if let Some(row) = (0..layout::ROWS).find(|row| layout::row_rect(*row).contains(point)) {
            let entries = self.list_entries();
            return entries
                .get(screen.offset + row)
                .map(|(item, _)| (Slot::List, *item));
        }
        [
            (Slot::LeftHand, layout::LEFT_HAND),
            (Slot::RightHand, layout::RIGHT_HAND),
            (Slot::Armor, layout::ARMOR),
        ]
        .into_iter()
        .find(|(_, rect)| rect.contains(point))
        .and_then(|(slot, _)| Some((slot, self.slot_item(slot)?)))
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
        let count = self.list_entries().len();
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, _)) = &mut screen.counter {
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
        // All other input is ignored during a drag or with the menu open.
        if screen.held.is_some() || screen.menu.is_some() {
            return true;
        }
        match key {
            KeyCode::Escape | KeyCode::KeyI => self.return_to_map(),
            KeyCode::ArrowDown if screen.offset + layout::ROWS < count => screen.offset += 1,
            KeyCode::ArrowUp => screen.offset = screen.offset.saturating_sub(1),
            KeyCode::ArrowDown => {}
            _ => return false,
        }
        true
    }
    pub(crate) fn inventory_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, _)) = &mut screen.counter {
            let input = counter_click(counter, point);
            if input != CounterInput::Ignored {
                self.press();
            }
            self.inventory_counter(input);
            return true;
        }
        if let Some((_, timer)) = &mut screen.timer {
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
        if screen.held.is_some() || screen.menu.is_some() {
            return true;
        }
        // A right press switches the cursor (only on the press, safe rule);
        // returning to the hand redraws the stats panel.
        if right {
            screen.arrow = !screen.arrow;
            screen.press = None;
            if !screen.arrow {
                screen.examine.clear();
            }
            return true;
        }
        if DONE.contains(point) {
            self.press();
            self.return_to_map();
            return true;
        }
        let (arrow, target) = (screen.arrow, screen.target);
        let Some((slot, item)) = self.slot_at(point) else {
            return true;
        };
        if let Some(target) = target {
            if !arrow && slot == Slot::List {
                self.use_on_target(item, target);
            }
            return true;
        }
        let clock = self.clock;
        let has_art = self.item_icon(item).is_some();
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if arrow {
            screen.press = Some(ArrowPress {
                item,
                next_pulse: clock + cursor::MENU_HOLD_MS,
            });
        } else {
            screen.held = Some((slot, item));
            if has_art {
                self.play_effect(b"ipickup1", None);
            }
        }
        true
    }
    /// The use-on window: the pressed row's item (the one drawn there,
    /// safe rule) is used on the target at once, for 2 AP in combat, and
    /// the window closes.
    fn use_on_target(&mut self, item: ObjectRef, target: ObjectRef) {
        let player = self.host.world.player;
        if self.host.world.combat {
            let Some(state) = self.host.world.critter_state_mut(player) else {
                return;
            };
            if state.ap < 2 {
                return;
            }
            state.ap -= 2;
        }
        self.return_to_map();
        self.selected = Some(target);
        self.interact(ObjectAction::UseObject(item));
    }
    /// The release: the menu's entry, an arrow click's examine text, or the
    /// drop of a lifted item.
    pub(crate) fn inventory_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if let Some(menu) = screen.menu.take() {
            let entry = menu
                .entries
                .get(menu.tracker.highlight)
                .copied()
                .unwrap_or(ItemEntry::Cancel);
            self.pointer.at = Some([menu.origin[0], menu.tracker.anchor]);
            self.item_entry(menu.item, entry);
            return true;
        }
        if let Some(press) = screen.press.take() {
            self.examine(press.item);
            return true;
        }
        if screen.counter.is_some() {
            return false;
        }
        let Some((slot, item)) = screen.held.take() else {
            return false;
        };
        if self.item_icon(item).is_some() {
            self.play_effect(b"iputdown", None);
        }
        let target = point.map_or(DropTarget::Nowhere, layout::drop_target);
        self.inventory_drop(slot, item, target);
        self.drain_effects();
        true
    }
    /// The arrow press's hold pulse opens the item menu at the pointer.
    pub(crate) fn inventory_tick(&mut self) -> bool {
        let clock = self.clock;
        let at = self.pointer.at;
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        let Some(press) = screen.press else {
            return false;
        };
        if clock <= press.next_pulse {
            return false;
        }
        screen.press = None;
        let item = press.item;
        let entries = self.item_menu(item);
        let origin = at.unwrap_or([layout::WINDOW[0], 0]);
        if let Screen::Inventory(screen) = &mut self.screen {
            screen.menu = Some(ItemMenu {
                item,
                tracker: MenuTracker::new(origin[1], entries.len()),
                entries,
                origin,
            });
        }
        self.play_effect(cursor::MENU_OPEN_SOUND.as_bytes(), None);
        true
    }
    /// The pointer moved with the item menu open.
    pub(crate) fn inventory_pointer(&mut self, y: i32) {
        if let Screen::Inventory(InventoryScreen {
            menu: Some(menu), ..
        }) = &mut self.screen
        {
            menu.tracker.moved(y);
        }
    }
    fn item_menu(&self, item: ObjectRef) -> Vec<ItemEntry> {
        let world = &self.host.world;
        let unloadable = matches!(self.item_kind(item), Some(ItemData::Weapon(gun)) if gun.capacity > 0)
            && world.loaded(item).0 > 0;
        let container = matches!(self.item_kind(item), Some(ItemData::Container { .. }));
        let held = world
            .get(item)
            .is_some_and(|object| object.raw.flags.0 & EQUIPPED != 0);
        let pid = world.get(item).map_or(0, |object| object.raw.pid);
        let usable = world.get(item).is_some_and(|object| {
            object
                .prototype
                .as_ref()
                .is_some_and(|p| p.permits_use() || p.permits_use_on())
        }) || matches!(self.item_kind(item), Some(ItemData::Drug(_)))
            || skills::is_explosive(pid)
            || self.host.kit_skill(pid).is_some()
            || pid == MOTION_SENSOR;
        layout::player_menu(unloadable, container && held, usable)
    }
    fn item_entry(&mut self, item: ObjectRef, entry: ItemEntry) {
        let player = self.host.world.player;
        match entry {
            ItemEntry::Look => self.examine(item),
            ItemEntry::Use => self.use_item(item),
            ItemEntry::Unload => {
                match self.host.world.unload_weapon(item) {
                    Ok(Some(ammo)) => {
                        if let Err(error) = self.host.world.add_item(player, ammo, 1, true) {
                            self.status = error.to_string().into_bytes();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
                self.drain_effects();
            }
            ItemEntry::Drop => {
                let quantity = self.entry_quantity(item);
                if quantity > 1 {
                    if let Screen::Inventory(screen) = &mut self.screen {
                        screen.counter = Some((Counter::new(quantity), Count::Drop { item }));
                    }
                } else {
                    self.drop_units(item, 1);
                }
            }
            ItemEntry::Cancel => {}
        }
    }
    /// The full examine text in the stats panel (not the monitor).
    fn examine(&mut self, item: ObjectRef) {
        let player = self.host.world.player;
        let start = self.host.world.effects.len();
        if let Err(error) = self.host.action(player, item, ObjectAction::Description) {
            self.status = error.to_string().into_bytes();
        }
        let emitted: Vec<_> = self.host.world.effects.drain(start..).collect();
        let mut text = Vec::new();
        for effect in emitted {
            match effect {
                WorldEffect::Message(line) => {
                    if !text.is_empty() {
                        text.push(b' ');
                    }
                    text.extend(line);
                }
                other => self.host.world.effects.push_back(other),
            }
        }
        if let Screen::Inventory(screen) = &mut self.screen {
            screen.examine = text;
        }
        self.drain_effects();
    }
    /// Use an item from the window: an explosive's timer, a kit on the
    /// player, a drug, a Motion Sensor's automap, else its use procedure.
    fn use_item(&mut self, item: ObjectRef) {
        let player = self.host.world.player;
        let pid = self.host.world.get(item).map_or(0, |o| o.raw.pid);
        if skills::is_explosive(pid) {
            if self.host.explosive_armable(item)
                && let Screen::Inventory(screen) = &mut self.screen
            {
                screen.timer = Some((item, SetTimer::default()));
            }
            self.drain_effects();
            return;
        }
        if pid == MOTION_SENSOR {
            self.use_motion_sensor(item);
            return;
        }
        let result = if self.host.kit_skill(pid).is_some() {
            self.host.use_kit(player, item, player).map(|_| ())
        } else if matches!(self.item_kind(item), Some(ItemData::Drug(_))) {
            self.host.use_drug(player, item).map(|_| ())
        } else {
            self.host.action(player, item, ObjectAction::Use)
        };
        if let Err(error) = result {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    fn drop_units(&mut self, item: ObjectRef, quantity: i32) {
        let player = self.host.world.player;
        if let Err(error) = self.host.world.drop_item(player, item, quantity) {
            self.status = error.to_string().into_bytes();
        }
        self.host.refresh_armor_art(player);
        self.drain_effects();
    }
    /// §3.34 the drop of `item`, lifted from `from`, on `target`.
    fn inventory_drop(&mut self, from: Slot, item: ObjectRef, target: DropTarget) {
        let player = self.host.world.player;
        let quantity = self.entry_quantity(item);
        let is = |host: &Self, object: ObjectRef, check: fn(&ItemData) -> bool| {
            host.item_kind(object).is_some_and(check)
        };
        let container = |data: &ItemData| matches!(data, ItemData::Container { .. });
        let weapon = |data: &ItemData| matches!(data, ItemData::Weapon(_));
        let ammo = |data: &ItemData| matches!(data, ItemData::Ammunition { .. });
        let armor = |data: &ItemData| matches!(data, ItemData::Armor(_));
        let result = match target {
            DropTarget::List(row) => {
                let offset = match &self.screen {
                    Screen::Inventory(screen) => screen.offset,
                    _ => 0,
                };
                let occupant = self
                    .list_entries()
                    .get(offset + row)
                    .map(|(entry, _)| *entry)
                    .filter(|entry| *entry != item);
                match occupant {
                    Some(bag) if is(self, bag, container) => {
                        self.count_or(
                            quantity,
                            Count::Into {
                                item,
                                container: bag,
                            },
                        );
                        Ok(())
                    }
                    Some(gun)
                        if is(self, gun, weapon)
                            && is(self, item, ammo)
                            && self.host.world.reload_permitted(gun, item) =>
                    {
                        self.count_or(
                            quantity,
                            Count::Reload {
                                weapon: gun,
                                ammo: item,
                            },
                        );
                        Ok(())
                    }
                    // From the list it stays put; from a slot it returns.
                    _ if from == Slot::List => Ok(()),
                    _ => self.host.world.unequip(player, item),
                }
            }
            DropTarget::LeftHand | DropTarget::RightHand => {
                let (slot, other) = if target == DropTarget::LeftHand {
                    (Slot::LeftHand, Slot::RightHand)
                } else {
                    (Slot::RightHand, Slot::LeftHand)
                };
                let flag = slot.flag().expect("a hand");
                match self.slot_item(slot) {
                    Some(occupant) if occupant == item => Ok(()),
                    Some(bag) if is(self, bag, container) => {
                        self.count_or(
                            quantity,
                            Count::Into {
                                item,
                                container: bag,
                            },
                        );
                        Ok(())
                    }
                    None => self.host.world.equip_slot(player, item, flag).map(|_| ()),
                    Some(gun) if is(self, gun, weapon) && is(self, item, ammo) => {
                        if self.host.world.reload_permitted(gun, item) {
                            self.count_or(
                                quantity,
                                Count::Reload {
                                    weapon: gun,
                                    ammo: item,
                                },
                            );
                        }
                        Ok(())
                    }
                    Some(occupant)
                        if from == other || (from == Slot::Armor && is(self, occupant, armor)) =>
                    {
                        // The two swap.
                        let back = from.flag().expect("an equipped slot");
                        self.host
                            .world
                            .equip_slot(player, occupant, back)
                            .and_then(|_| self.host.world.equip_slot(player, item, flag))
                            .map(|_| ())
                    }
                    Some(occupant) => self
                        .host
                        .world
                        .unequip(player, occupant)
                        .and_then(|()| self.host.world.equip_slot(player, item, flag))
                        .map(|_| ()),
                }
            }
            DropTarget::Armor if is(self, item, armor) => {
                let old = self.slot_item(Slot::Armor).filter(|old| *old != item);
                let worn = self
                    .host
                    .world
                    .equip_slot(player, item, BasicFlags::WORN.0)
                    .map(|_| ());
                // The old armor goes to the source hand, else the list.
                match (worn, old, from.flag()) {
                    (Ok(()), Some(old), Some(hand)) if from != Slot::Armor => {
                        self.host.world.equip_slot(player, old, hand).map(|_| ())
                    }
                    (result, ..) => result,
                }
            }
            // Only with a nested container open (not offered here).
            DropTarget::Armor | DropTarget::Figure | DropTarget::Nowhere => Ok(()),
        };
        if let Err(error) = result {
            self.status = error.to_string().into_bytes();
        }
        self.host.refresh_armor_art(player);
        self.host.world.emit(WorldEffect::RollArmorClass);
    }
    /// A stack opens the counter; a single item moves at once.
    fn count_or(&mut self, quantity: i32, count: Count) {
        if quantity > 1 {
            if let Screen::Inventory(screen) = &mut self.screen {
                screen.counter = Some((Counter::new(quantity), count));
            }
        } else {
            self.counted(count, 1);
        }
    }
    fn counted(&mut self, count: Count, quantity: i32) {
        let player = self.host.world.player;
        match count {
            Count::Reload { weapon, ammo } => {
                match self.host.drag_reload(player, weapon, ammo, quantity) {
                    Ok(true) => {}
                    Ok(false) => self.status = b"The weapon did not load".to_vec(),
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
            }
            Count::Into { item, container } => {
                if let Err(error) = self.host.world.move_into(player, item, container, quantity) {
                    self.status = error.to_string().into_bytes();
                }
                self.host.refresh_armor_art(player);
            }
            Count::Drop { item } => self.drop_units(item, quantity),
        }
        self.drain_effects();
    }
    /// The counter's outcome.
    fn inventory_counter(&mut self, input: CounterInput) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        match input {
            CounterInput::Accept(quantity) => {
                if let Some((_, count)) = screen.counter.take() {
                    self.counted(count, quantity);
                }
            }
            CounterInput::Refused => self.play_effect(ERROR_SOUND.as_bytes(), None),
            CounterInput::Cancel => screen.counter = None,
            CounterInput::Changed => {}
            CounterInput::Ignored => return false,
        }
        true
    }
    pub(crate) fn inventory_image(&self) -> AppResult<IndexedImage> {
        let Screen::Inventory(screen) = &self.screen else {
            return Err("inventory is not open".into());
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect(PANEL, painter.color(ColorKey(1057)));
        let dark = painter.color(ColorKey::BLACK);
        let frame_color = painter.color(ColorKey::DARK_GREEN);
        let outline = |image: &mut IndexedImage, rect: Rect| {
            image.fill_rect([rect.x, rect.y, rect.w, rect.h], frame_color);
            image.fill_rect([rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2], dark);
        };
        let icon = |image: &mut IndexedImage, item: ObjectRef, rect: Rect| {
            if let Some(art) = self.item_icon(item)
                && let Some(frame) = art.frame(0, 0)
            {
                image.draw_frame(
                    frame,
                    [rect.x as u32, rect.y as u32],
                    [rect.w as u32, rect.h as u32],
                );
            }
        };
        let lifted = screen.held;
        // The list, a lifted stack drawn one smaller.
        let entries = self.list_entries();
        for row in 0..layout::ROWS {
            let rect = layout::row_rect(row);
            outline(&mut image, rect);
            let Some((item, quantity)) = entries.get(screen.offset + row).copied() else {
                continue;
            };
            let shown = if lifted == Some((Slot::List, item)) {
                quantity - 1
            } else {
                quantity
            };
            if shown > 0 {
                icon(&mut image, item, rect);
            }
            if let Some(label) = skills::quantity_label(shown, None) {
                painter.text(
                    &mut image,
                    101,
                    ColorKey::WHITE,
                    &label,
                    [rect.x + 2, rect.y + 2],
                    60,
                );
            }
        }
        // The hands and the armor; a lifted slot shows empty.
        for (slot, rect) in [
            (Slot::LeftHand, layout::LEFT_HAND),
            (Slot::RightHand, layout::RIGHT_HAND),
            (Slot::Armor, layout::ARMOR),
        ] {
            outline(&mut image, rect);
            if let Some(item) = self.slot_item(slot)
                && lifted != Some((slot, item))
            {
                icon(&mut image, item, rect);
            }
        }
        // The player figure.
        let player = self.host.world.player;
        if let Some(object) = self.host.world.get(player)
            && let Ok(art) = self.host.assets.object_art(Fid(object.raw.art_id).0)
            && let Some(frame) = art.frame(usize::from(object.raw.orientation), 0)
        {
            let rect = layout::FIGURE;
            image.draw_frame(
                frame,
                [rect.x as u32, rect.y as u32],
                [rect.w as u32, rect.h as u32],
            );
        }
        // The stats panel: the examine text, caps and weight.
        let world = &self.host.world;
        let carry = world
            .player_sheet()
            .map_or(0, |sheet| sheet.stat(12, world.context(player)));
        let summary = format!(
            "Caps: {}   Weight: {} / {} lb",
            world.caps(player),
            world.inventory_weight(player),
            carry
        );
        painter.text(
            &mut image,
            101,
            ColorKey::GREEN,
            summary.as_bytes(),
            [EXAMINE[0], 250],
            220,
        );
        let mut y = EXAMINE[1];
        let font = painter.font(101);
        let width = |line: &[u8]| {
            font.as_ref()
                .map_or(line.len() as i32 * 6, |f| f.text_width(line, false) as i32)
        };
        for line in vault_core::ui::bar::wrap_message(&screen.examine, width) {
            painter.text(
                &mut image,
                101,
                ColorKey::GREEN,
                &line,
                [EXAMINE[0], y],
                220,
            );
            y += 10;
        }
        painter.blit(&mut image, "lilredup.frm", 0, [DONE.x, DONE.y]);
        let done = painter.message("dbox.msg", 100);
        painter.text(
            &mut image,
            103,
            ColorKey::LABEL,
            &done,
            [DONE.x + 20, DONE.y + 1],
            80,
        );
        painter.text(
            &mut image,
            101,
            ColorKey::GREEN,
            &self.status,
            [22, 466],
            600,
        );
        if let Some(menu) = &screen.menu {
            self.draw_item_menu(&painter, &mut image, menu);
        }
        if let Some((_, timer)) = &screen.timer {
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
        if let Some((counter, count)) = &screen.counter {
            let item = match count {
                Count::Reload { ammo, .. } => *ammo,
                Count::Into { item, .. } | Count::Drop { item } => *item,
            };
            let icon = self.item_icon(item);
            draw_counter(
                &painter,
                &mut image,
                counter,
                icon.as_ref().and_then(|icon| icon.frame(0, 0)),
            );
        }
        Ok(image)
    }
    /// The item menu at the pointer (the pointer hidden): the arrow, then
    /// the 40-by-40 icons, highlighted entry in its `h` art. The Unload and
    /// Drop icons have no approved art name, so they are labeled boxes.
    fn draw_item_menu(&self, painter: &Painter, image: &mut IndexedImage, menu: &ItemMenu) {
        let [x, y] = menu.origin;
        let top = y - cursor::menu_shift(y, menu.entries.len());
        painter.blit(image, layout::ARROW_CURSOR, 0, [x, top]);
        for (index, entry) in menu.entries.iter().enumerate() {
            let highlighted = index == menu.tracker.highlight;
            let origin = [x + 29, top + 40 * index as i32];
            let art = match entry {
                ItemEntry::Look => Some(("lookn.frm", "lookh.frm")),
                ItemEntry::Use => Some(("usegetn.frm", "usegeth.frm")),
                ItemEntry::Cancel => Some(("canceln.frm", "cancelh.frm")),
                ItemEntry::Unload | ItemEntry::Drop => None,
            };
            match art {
                Some((normal, lit)) => {
                    painter.blit(image, if highlighted { lit } else { normal }, 0, origin);
                }
                None => {
                    let key = if highlighted {
                        ColorKey::SELECTED
                    } else {
                        ColorKey::GREEN
                    };
                    image.fill_rect(
                        [origin[0], origin[1], 40, 40],
                        painter.color(ColorKey::BLACK),
                    );
                    let label: &[u8] = if *entry == ItemEntry::Drop {
                        b"Drop"
                    } else {
                        b"Unload"
                    };
                    painter.centered(image, 101, key, label, [origin[0], origin[1] + 15], 40);
                }
            }
        }
    }
}
