//! SPEC-014 §3.34 the player inventory and use-on windows at screen (80,
//! 0), drawn over the paused world, with the SPEC-011 rules behind their
//! moves: opening the inventory in combat costs the §3.11 action points;
//! using a skill book reads it (SPEC-013 §3.25), a drug is consumed (§3.6),
//! a kit heals the player (SPEC-013 §3.13), an explosive opens the Set Timer
//! dialog (SPEC-013 §3.10, §3.15), and a Motion Sensor opens the automap
//! (SPEC-014 §3.33).
//!
//! The inventory window shows `invbox.frm`: six list rows, the scroll
//! buttons, the rotating figure, the armor and hand slots, the stats panel
//! and the Done button. The use-on window (from the world action menu's
//! Inventory entry) shows `use.frm` with the list, scroll buttons, figure
//! and a Cancel button; a press on a list row uses the item drawn there on
//! the target (safe rule) and closes it.
//!
//! The window opens in the hand cursor. A left press on a filled slot lifts
//! the item (`ipickup1`); the release drops it (`iputdown`) on the first
//! target in the §3.34 order: the list (into a container, onto a weapon it
//! reloads, else back into the list), the left hand, the right hand, the
//! armor slot, the figure, or nowhere. There is no two-click drag. A right
//! press switches to the arrow cursor, where a quick click shows the item's
//! examine text in the stats panel and a held press opens its menu.
//!
//! **Nested containers.** Use on a container in the list opens it, up to
//! nine levels deep (as in the SPEC-013 §3.16 windows): the list then shows
//! its contents, drops into the list go into it, dropping an item on the
//! figure moves it up one container level, and a press on the figure (its
//! invisible button) goes up one level.
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
    map::{ObjectDetail, ObjectState},
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

/// An arrow-cursor press on an item (in the inventory, use-on, loot or
/// barter window), waiting for its release or the 250 ms hold pulse.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArrowPress {
    pub item: ObjectRef,
    next_pulse: u64,
}
impl ArrowPress {
    pub(crate) fn new(item: ObjectRef, clock: u64) -> Self {
        Self {
            item,
            next_pulse: clock + cursor::MENU_HOLD_MS,
        }
    }
    /// The hold pulse is due: strictly more than 250 ms after the press.
    pub(crate) fn due(&self, clock: u64) -> bool {
        clock > self.next_pulse
    }
}

/// The arrow cursor's item menu.
#[derive(Debug, Clone)]
pub(crate) struct ItemMenu {
    pub item: ObjectRef,
    pub entries: Vec<ItemEntry>,
    pub tracker: MenuTracker,
    /// The pointer when it opened, where the release returns it.
    pub origin: [i32; 2],
    /// The menu's top after any upward shift, and whether it is flipped.
    pub top: i32,
    pub flipped: bool,
}
impl ItemMenu {
    /// §3.34 a menu of `entries` for `item`, opened with the pointer at
    /// `origin` in a window with the given edges.
    pub(crate) fn new(
        item: ObjectRef,
        entries: Vec<ItemEntry>,
        origin: [i32; 2],
        edges: [i32; 2],
    ) -> Self {
        let (top, flipped) = layout::menu_place(origin, entries.len(), edges);
        Self {
            item,
            tracker: MenuTracker::new(origin[1], entries.len()),
            entries,
            origin,
            top,
            flipped,
        }
    }
    /// The highlighted entry, run on the release.
    pub(crate) fn chosen(&self) -> ItemEntry {
        self.entries
            .get(self.tracker.highlight)
            .copied()
            .unwrap_or(ItemEntry::Cancel)
    }
}

/// §3.34 the stats panel's examine text: the item's name, its examine text
/// and its weight.
#[derive(Debug, Clone)]
struct Examine {
    name: Vec<u8>,
    text: Vec<u8>,
    weight: i64,
}

pub(crate) struct InventoryScreen {
    /// The world action menu's target: the use-on window.
    pub target: Option<ObjectRef>,
    /// The open containers, outermost first; empty shows the player's list.
    pub path: Vec<ObjectRef>,
    /// The list's scroll offset.
    pub offset: usize,
    /// The arrow cursor (else the hand).
    pub arrow: bool,
    /// The lifted item and its source slot.
    pub held: Option<(Slot, ObjectRef)>,
    press: Option<ArrowPress>,
    pub menu: Option<ItemMenu>,
    pub counter: Option<(Counter, Count)>,
    /// The Set Timer dialog for an explosive being armed.
    pub timer: Option<(ObjectRef, SetTimer)>,
    /// The stats panel's examine text, until the hand cursor returns.
    examine: Option<Examine>,
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
/// A Set Timer button's screen rectangle.
fn timer_button(rect: Rect) -> Rect {
    Rect::new(
        TIMER_ORIGIN[0] + rect.x,
        TIMER_ORIGIN[1] + rect.y,
        rect.w,
        rect.h,
    )
}
/// A screen rectangle `[x, y, w, h]` contains `point`.
pub(crate) fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

/// How deep containers open (SPEC-013 §3.16).
const MAX_DEPTH: usize = 9;
/// The stats panel's font color, key 992.
const STATS_COLOR: ColorKey = ColorKey(992);

impl MapPreview {
    /// Open the inventory; in combat its AP cost may refuse it.
    pub(crate) fn open_inventory(&mut self) {
        self.open_inventory_on(None);
    }
    pub(crate) fn open_inventory_on(&mut self, target: Option<ObjectRef>) {
        if self.host.open_inventory() {
            self.screen = Screen::Inventory(InventoryScreen {
                target,
                path: Vec::new(),
                offset: 0,
                arrow: false,
                held: None,
                press: None,
                menu: None,
                counter: None,
                timer: None,
                examine: None,
            });
        }
        self.drain_effects();
    }
    /// The innermost open container, else the player.
    fn list_owner(&self) -> ObjectRef {
        match &self.screen {
            Screen::Inventory(screen) => screen.path.last().copied(),
            _ => None,
        }
        .unwrap_or(self.host.world.player)
    }
    /// The list: the open container's entries, or the player's entries that
    /// no slot holds.
    fn list_entries(&self) -> Vec<(ObjectRef, i32)> {
        let world = &self.host.world;
        world
            .get(self.list_owner())
            .map(|owner| {
                owner
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
    /// The number of list entries shown (for tests).
    #[cfg(test)]
    pub(crate) fn list_entries_len(&self) -> usize {
        self.list_entries().len()
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
    /// The quantity of `item`'s entry in its owner's inventory.
    fn entry_quantity(&self, item: ObjectRef) -> i32 {
        let world = &self.host.world;
        world
            .get(item)
            .and_then(|object| world.get(object.owner?))
            .and_then(|owner| owner.inventory.iter().find(|(entry, _)| *entry == item))
            .map_or(1, |(_, quantity)| *quantity)
    }
    /// Unload a weapon: the ammunition goes, forced, to the weapon's owner
    /// (SPEC-011 §3.15, SPEC-013 §3.16).
    pub(crate) fn unload_to_owner(&mut self, item: ObjectRef) {
        let owner = self.owner_of(item);
        match self.host.world.unload_weapon(item) {
            Ok(Some(ammo)) => {
                if let Err(error) = self.host.world.add_item(owner, ammo, 1, true) {
                    self.status = error.to_string().into_bytes();
                }
            }
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.drain_effects();
    }
    /// The item's direct owner (the player when it has none).
    fn owner_of(&self, item: ObjectRef) -> ObjectRef {
        self.host
            .world
            .get(item)
            .and_then(|object| object.owner)
            .unwrap_or(self.host.world.player)
    }
    /// A unit of `item` from an open container comes out to the player, so
    /// that a slot can take it.
    fn take_out(&mut self, item: ObjectRef) -> Result<(), vault_core::WorldError> {
        let player = self.host.world.player;
        let owner = self.owner_of(item);
        if owner == player {
            return Ok(());
        }
        self.host.world.remove_item(owner, item, 1)?;
        self.host.world.add_item(player, item, 1, true)
    }
    /// Open a container from the list, up to nine levels deep.
    fn open_container(&mut self, container: ObjectRef) {
        if let Screen::Inventory(screen) = &mut self.screen
            && screen.path.len() < MAX_DEPTH
        {
            screen.path.push(container);
            screen.offset = 0;
        }
    }
    /// Go up one container level.
    fn close_container(&mut self) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        let closed = screen.path.pop().is_some();
        if closed {
            screen.offset = 0;
        }
        closed
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
    /// The red button at `origin`, sized by its art.
    fn red_button(&self, origin: [i32; 2]) -> Rect {
        let [w, h] = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        }
        .size(layout::RED_BUTTON.0)
        .unwrap_or([15, 16]);
        Rect::new(origin[0], origin[1], w, h)
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
        // §3.34 keys: Esc, Up, Down and Ctrl+Q / Ctrl+X in both windows;
        // the use-on window adds Home, End, PgUp and PgDn (6 rows).
        let rows = layout::ROWS;
        let use_on = screen.target.is_some();
        let control = self.hud.modifiers[1];
        match key {
            KeyCode::Escape => self.return_to_map(),
            KeyCode::ArrowUp => screen.offset = screen.offset.saturating_sub(1),
            KeyCode::ArrowDown if screen.offset + rows < count => screen.offset += 1,
            KeyCode::ArrowDown => {}
            KeyCode::KeyQ | KeyCode::KeyX if control => {
                let title = self.message("misc.msg", 0);
                self.show_dialog(super::dialog::Dialog::quit_confirmation(
                    [169, 117],
                    title,
                    ColorKey::ERROR,
                ));
            }
            KeyCode::Home if use_on => screen.offset = 0,
            KeyCode::End if use_on => screen.offset = count.saturating_sub(rows),
            KeyCode::PageUp if use_on => screen.offset = screen.offset.saturating_sub(rows),
            KeyCode::PageDown if use_on => {
                let next = screen.offset + rows;
                screen.offset = if next + rows >= count {
                    count.saturating_sub(rows)
                } else {
                    next
                };
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn inventory_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if screen.counter.is_some() {
            if let Some(rect) = super::counter::counter_button(point) {
                self.press_button(rect);
            }
            let Screen::Inventory(screen) = &mut self.screen else {
                return false;
            };
            let Some((counter, _)) = &mut screen.counter else {
                return true;
            };
            let input = counter_click(counter, point);
            self.inventory_counter(input);
            return true;
        }
        if let Some((_, timer)) = &mut screen.timer {
            let Some(button) = [TIMER_PLUS, TIMER_MINUS, TIMER_DONE, TIMER_CANCEL]
                .into_iter()
                .find(|&rect| timer_button(rect).contains(point))
            else {
                return true;
            };
            if button == TIMER_PLUS {
                timer.plus();
            } else if button == TIMER_MINUS {
                timer.minus();
            }
            self.held = Some(timer_button(button));
            if button == TIMER_DONE {
                self.finish_timer(true);
            } else if button == TIMER_CANCEL {
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
                screen.examine = None;
            }
            return true;
        }
        // Done (inventory) or Cancel (use-on): a red button sending Esc.
        let (arrow, target) = (screen.arrow, screen.target);
        let red = if target.is_some() {
            layout::CANCEL
        } else {
            layout::DONE
        };
        let red = self.red_button(red);
        if red.contains(point) {
            self.press_button(red);
            self.return_to_map();
            return true;
        }
        // The scroll buttons move one row: up from a positive offset, down
        // while offset + 6 is below the entry count.
        let count = self.list_entries().len();
        for (rect, down) in [(layout::SCROLL_UP.0, false), (layout::SCROLL_DOWN.0, true)] {
            if rect.contains(point) {
                self.press_button(rect);
                if let Screen::Inventory(screen) = &mut self.screen {
                    if down && screen.offset + layout::ROWS < count {
                        screen.offset += 1;
                    } else if !down {
                        screen.offset = screen.offset.saturating_sub(1);
                    }
                }
                return true;
            }
        }
        let Some((slot, item)) = self.slot_at(point) else {
            // The figure goes up one container level.
            if !arrow && layout::FIGURE.contains(point) {
                self.close_container();
            }
            return true;
        };
        // The use-on window has only the list: the hand cursor uses the
        // pressed row's item on the target, the arrow cursor examines it
        // or opens its menu.
        if let Some(target) = target {
            if !arrow && slot == Slot::List {
                self.use_on_target(item, target);
                return true;
            }
            if slot != Slot::List {
                return true;
            }
        }
        let clock = self.clock;
        let has_art = self.item_icon(item).is_some();
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        if arrow {
            screen.press = Some(ArrowPress::new(item, clock));
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
            // The pointer returns to where it was when the menu opened.
            self.pointer.at = Some(menu.origin);
            self.item_entry(menu.item, menu.chosen());
            return true;
        }
        if let Some(press) = screen.press.take() {
            // A quick click: the examine text in the stats panel, or the
            // short look line on the monitor in the use-on window, which
            // has none.
            if screen.target.is_some() {
                self.look_line(press.item);
            } else {
                self.examine(press.item);
            }
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
    /// The figure's rotation, and the arrow press's hold pulse that opens
    /// the item menu at the pointer.
    pub(crate) fn inventory_tick(&mut self) -> bool {
        let clock = self.clock;
        let at = self.pointer.at;
        if !matches!(self.screen, Screen::Inventory(_)) {
            return false;
        }
        // §3.34: the figure turns one facing at most every 166 ms, on the
        // window's polls, even during a drag; the counter persists between
        // openings.
        let (facing, since) = self.figure;
        let turned = clock.saturating_sub(since) >= layout::FIGURE_TURN_MS;
        if turned {
            self.figure = ((facing + 1) % 6, clock);
        }
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
        let edges = if screen.target.is_some() {
            layout::USE_ON_EDGES
        } else {
            layout::INVENTORY_EDGES
        };
        let Some(press) = screen.press else {
            return turned;
        };
        if !press.due(clock) {
            return turned;
        }
        screen.press = None;
        let item = press.item;
        let entries = self.item_menu(item);
        let origin = at.unwrap_or([layout::WINDOW[0], 0]);
        if let Screen::Inventory(screen) = &mut self.screen {
            screen.menu = Some(ItemMenu::new(item, entries, origin, edges));
        }
        self.play_effect(cursor::MENU_OPEN_SOUND.as_bytes(), None);
        true
    }
    /// The pointer moved over a window: an open item menu tracks the
    /// vertical motion, and the loot and barter arrow cursors print the
    /// short look line of a newly hovered entry.
    pub(crate) fn window_pointer(&mut self, point: [i32; 2]) {
        let menu = match &mut self.screen {
            Screen::Inventory(screen) => screen.menu.as_mut(),
            Screen::Loot(screen) => screen.menu.as_mut().map(|(_, menu)| menu),
            Screen::Barter(screen) => screen.menu.as_mut().map(|(_, menu)| menu),
            _ => None,
        };
        if let Some(menu) = menu {
            menu.tracker.moved(point[1]);
            return;
        }
        match self.screen {
            Screen::Loot(_) => self.loot_hover(point),
            Screen::Barter(_) => self.barter_hover(point),
            _ => {}
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
        // A container in the list opens with Use.
        let usable = (container && !held)
            || world.get(item).is_some_and(|object| {
                object
                    .prototype
                    .as_ref()
                    .is_some_and(|p| p.permits_use() || p.permits_use_on())
            })
            || matches!(self.item_kind(item), Some(ItemData::Drug(_)))
            || skills::is_explosive(pid)
            || skills::skill_book(pid).is_some()
            || self.host.kit_skill(pid).is_some()
            || pid == MOTION_SENSOR;
        layout::player_menu(unloadable, container && held, usable)
    }
    fn item_entry(&mut self, item: ObjectRef, entry: ItemEntry) {
        match entry {
            ItemEntry::Look => self.examine(item),
            ItemEntry::Use => self.use_item(item),
            ItemEntry::Unload => self.unload_to_owner(item),
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
    /// The short look line (the look-at text) on the monitor.
    pub(crate) fn look_line(&mut self, item: ObjectRef) {
        let player = self.host.world.player;
        if let Err(error) = self.host.action(player, item, ObjectAction::LookAt) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    /// The full examine text on the monitor (a window without a stats
    /// panel).
    pub(crate) fn examine_line(&mut self, item: ObjectRef) {
        let player = self.host.world.player;
        if let Err(error) = self.host.action(player, item, ObjectAction::Description) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    /// The item's full examine text in the stats panel (not the monitor);
    /// the use-on window has no panel, so there it goes to the monitor.
    fn examine(&mut self, item: ObjectRef) {
        if matches!(&self.screen, Screen::Inventory(screen) if screen.target.is_some()) {
            self.examine_line(item);
            return;
        }
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
        let world = &self.host.world;
        let examine = Examine {
            name: world.get(item).map(|o| o.name.clone()).unwrap_or_default(),
            text,
            weight: world.item_weight(item),
        };
        if let Screen::Inventory(screen) = &mut self.screen {
            screen.examine = Some(examine);
        }
        self.drain_effects();
    }
    /// Use an item from the window: a skill book, an explosive's timer, a
    /// kit on the player, a drug, a Motion Sensor's automap, else its use
    /// procedure.
    fn use_item(&mut self, item: ObjectRef) {
        let player = self.host.world.player;
        let pid = self.host.world.get(item).map_or(0, |o| o.raw.pid);
        // SPEC-013 §3.25: the route tests the skill books first.
        if skills::skill_book(pid).is_some() {
            if let Err(error) = self.host.read_book(item, vault_core::BookRoute::Inventory) {
                self.status = error.to_string().into_bytes();
            }
            self.drain_effects();
            return;
        }
        if matches!(self.item_kind(item), Some(ItemData::Container { .. })) {
            self.open_container(item);
            return;
        }
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
        let open = match &self.screen {
            Screen::Inventory(screen) => screen.path.last().copied(),
            _ => None,
        };
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
                    // From the list it stays put; from a slot it returns,
                    // into the open container when there is one.
                    _ if from == Slot::List => Ok(()),
                    _ => match open {
                        Some(container) => self.unequip_into(item, container),
                        None => self.host.world.unequip(player, item),
                    },
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
                    None => self
                        .take_out(item)
                        .and_then(|()| self.host.world.equip_slot(player, item, flag))
                        .map(|_| ()),
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
                        .and_then(|()| self.take_out(item))
                        .and_then(|()| self.host.world.equip_slot(player, item, flag))
                        .map(|_| ()),
                }
            }
            DropTarget::Armor if is(self, item, armor) => {
                let old = self.slot_item(Slot::Armor).filter(|old| *old != item);
                let worn = self
                    .take_out(item)
                    .and_then(|()| self.host.world.equip_slot(player, item, BasicFlags::WORN.0))
                    .map(|_| ());
                // The old armor goes to the source hand, else the list.
                match (worn, old, from.flag()) {
                    (Ok(()), Some(old), Some(hand)) if from != Slot::Armor => {
                        self.host.world.equip_slot(player, old, hand).map(|_| ())
                    }
                    (result, ..) => result,
                }
            }
            // With a container open, an item from its list moves up a level.
            DropTarget::Figure if from == Slot::List && open.is_some() => {
                self.move_up(item, quantity)
            }
            DropTarget::Armor | DropTarget::Figure | DropTarget::Nowhere => Ok(()),
        };
        if let Err(error) = result {
            self.status = error.to_string().into_bytes();
        }
        self.host.refresh_armor_art(player);
        self.host.world.emit(WorldEffect::RollArmorClass);
    }
    /// A slot's item goes into the open container; a refused move keeps it
    /// in its slot.
    fn unequip_into(
        &mut self,
        item: ObjectRef,
        container: ObjectRef,
    ) -> Result<(), vault_core::WorldError> {
        let player = self.host.world.player;
        if let Some(refusal) = self.host.world.add_refusal(container, item, 1) {
            self.status = refusal.message().as_bytes().to_vec();
            return Ok(());
        }
        self.host.world.unequip(player, item)?;
        self.host
            .world
            .move_into(player, item, container, 1)
            .map(|_| ())
    }
    /// §3.34 target 4: an item of the open container's list moves up one
    /// container level, the whole entry. Into a container the normal limits
    /// apply; into the player it is forced, since the carried weight does
    /// not change.
    fn move_up(&mut self, item: ObjectRef, quantity: i32) -> Result<(), vault_core::WorldError> {
        let (current, parent) = match &self.screen {
            Screen::Inventory(screen) => match screen.path.as_slice() {
                [.., parent, current] => (*current, Some(*parent)),
                [current] => (*current, None),
                [] => return Ok(()),
            },
            _ => return Ok(()),
        };
        match parent {
            Some(parent) => self
                .host
                .world
                .move_into(current, item, parent, quantity)
                .map(|_| ()),
            None => {
                let player = self.host.world.player;
                self.host.world.remove_item(current, item, quantity)?;
                self.host.world.add_item(player, item, quantity, true)
            }
        }
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
                // SPEC-011 §3.15 lifts the weapon out of its owner's
                // inventory and re-adds it; one held in a hand returns to
                // that hand.
                let owner = self.owner_of(weapon);
                let held = self
                    .host
                    .world
                    .get(weapon)
                    .map_or(0, |object| object.raw.flags.0 & EQUIPPED);
                match self.host.drag_reload(owner, weapon, ammo, quantity) {
                    Ok(true) => {}
                    Ok(false) => self.status = b"The weapon did not load".to_vec(),
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
                if held != 0
                    && let Err(error) = self.host.world.equip_slot(owner, weapon, held)
                {
                    self.status = error.to_string().into_bytes();
                }
            }
            Count::Into { item, container } => {
                let owner = self.owner_of(item);
                if let Err(error) = self.host.world.move_into(owner, item, container, quantity) {
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
    pub(crate) fn inventory_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Inventory(screen) = &self.screen else {
            return Err("inventory is not open".into());
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let use_on = screen.target.is_some();
        let art = if use_on {
            layout::USE_ON_ART
        } else {
            layout::INVENTORY_ART
        };
        painter.blit_opaque(&mut image, art, 0, layout::WINDOW);
        let lifted = screen.held;
        // The list: item art scaled into 56 by 40, a lifted stack drawn one
        // smaller, and the loot window's quantity labels.
        let entries = self.list_entries();
        for row in 0..layout::ROWS {
            let Some((item, quantity)) = entries.get(screen.offset + row).copied() else {
                continue;
            };
            let rect = layout::icon_rect(row);
            let dragged = lifted == Some((Slot::List, item));
            let shown = if dragged { quantity - 1 } else { quantity };
            if shown > 0 {
                self.draw_icon(&mut image, item, rect);
            }
            if let Some(label) = skills::quantity_label(shown, self.ammo_label(item, dragged)) {
                painter.text(
                    &mut image,
                    101,
                    ColorKey::WHITE,
                    &label,
                    [rect.x, rect.y],
                    80,
                );
            }
        }
        for (rect, up, down) in [layout::SCROLL_UP, layout::SCROLL_DOWN] {
            painter.blit(
                &mut image,
                self.button_art(rect, up, down),
                0,
                [rect.x, rect.y],
            );
        }
        let red = self.red_button(if use_on { layout::CANCEL } else { layout::DONE });
        let (up, down) = layout::RED_BUTTON;
        painter.blit(
            &mut image,
            self.button_art(red, up, down),
            0,
            [red.x, red.y],
        );
        self.draw_figure(&mut image);
        if !use_on {
            self.draw_slots(&mut image, lifted);
            match &screen.examine {
                Some(examine) => self.draw_examine(&painter, &mut image, examine),
                None => self.draw_stats(&painter, &mut image),
            }
        }
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
            for (rect, up, down) in [
                (TIMER_PLUS, "splsoff.frm", "splson.frm"),
                (TIMER_MINUS, "snegoff.frm", "snegon.frm"),
                (TIMER_DONE, "lilredup.frm", "lilreddn.frm"),
                (TIMER_CANCEL, "lilredup.frm", "lilreddn.frm"),
            ] {
                let rect = timer_button(rect);
                painter.blit(
                    &mut image,
                    self.button_art(rect, up, down),
                    0,
                    [rect.x, rect.y],
                );
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
                self.held_button(),
            );
        }
        Ok(image)
    }
    /// An item's inventory art scaled into `rect`.
    fn draw_icon(&self, image: &mut IndexedImage, item: ObjectRef, rect: Rect) {
        if let Some(art) = self.item_icon(item)
            && let Some(frame) = art.frame(0, 0)
        {
            image.draw_frame(
                frame,
                [rect.x as u32, rect.y as u32],
                [rect.w as u32, rect.h as u32],
            );
        }
    }
    /// The ammunition terms of a quantity label: the pack size and the
    /// front pack's rounds, which a dragged entry leaves out (SPEC-013
    /// §3.16).
    fn ammo_label(&self, item: ObjectRef, dragged: bool) -> Option<(i32, i32)> {
        let Some(ItemData::Ammunition { quantity: pack, .. }) = self.item_kind(item) else {
            return None;
        };
        let front = match &self.host.world.get(item)?.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::Ammunition(rounds),
                ..
            } if !dragged => *rounds,
            _ => *pack,
        };
        Some((*pack, front))
    }
    /// §3.34 the rotating figure, centered in its 60-by-100 box, clipped to
    /// it and drawn transparently over the window art (the safe rule's
    /// restore from the window's own background).
    fn draw_figure(&self, image: &mut IndexedImage) {
        let player = self.host.world.player;
        let facing = self.figure.0;
        let Some(fid) = self.host.figure_art(player, self.active_item(), facing) else {
            return;
        };
        let Ok(art) = self.host.assets.object_art(fid) else {
            return;
        };
        let Some(frame) = art
            .frame(usize::from(facing), 0)
            .or_else(|| art.frame(0, 0))
        else {
            return;
        };
        let rect = layout::FIGURE_BOX;
        let origin = [
            i64::from(rect.x + (rect.w - i32::from(frame.width)) / 2),
            i64::from(rect.y + (rect.h - i32::from(frame.height)) / 2),
        ];
        let clip = [rect.x, rect.y, rect.w, rect.h].map(i64::from);
        image.blit_within(frame, origin, clip);
    }
    /// The hands and the armor, each scaled into its slot; a lifted slot
    /// shows empty. An item held in both hands is drawn once into the
    /// 180-by-61 hand area (safe rule).
    fn draw_slots(&self, image: &mut IndexedImage, lifted: Option<(Slot, ObjectRef)>) {
        let left = self.slot_item(Slot::LeftHand);
        let right = self.slot_item(Slot::RightHand);
        let held = |slot: Slot, item: ObjectRef| lifted != Some((slot, item));
        if let (Some(both), true) = (left, left == right) {
            if held(Slot::LeftHand, both) && held(Slot::RightHand, both) {
                self.draw_icon(image, both, layout::BOTH_HANDS);
            }
        } else {
            for (slot, item, rect) in [
                (Slot::LeftHand, left, layout::LEFT_HAND),
                (Slot::RightHand, right, layout::RIGHT_HAND),
            ] {
                if let Some(item) = item.filter(|item| held(slot, *item)) {
                    self.draw_icon(image, item, rect);
                }
            }
        }
        if let Some(armor) = self
            .slot_item(Slot::Armor)
            .filter(|armor| held(Slot::Armor, *armor))
        {
            self.draw_icon(image, armor, layout::ARMOR);
        }
    }
    /// A one-pixel rule across the stats panel, (377, y) to (520, y).
    fn panel_rule(painter: &Painter, image: &mut IndexedImage, y: i32) {
        image.fill_rect([377, y, 144, 1], painter.color(STATS_COLOR));
    }
    /// §3.34 the stats panel: the owner's name, the primary statistics, HP,
    /// AC and the DT/DR pairs, a block per hand and the total weight.
    fn draw_stats(&self, painter: &Painter, image: &mut IndexedImage) {
        let world = &self.host.world;
        let player = world.player;
        let text = |image: &mut IndexedImage, line: &[u8], at: [i32; 2], clip: i32| {
            painter.text(image, 101, STATS_COLOR, line, at, clip);
        };
        let name = world
            .get(player)
            .map(|o| o.name.clone())
            .unwrap_or_default();
        text(image, &name, [377, 44], 80);
        Self::panel_rule(painter, image, 59);
        // DT and DR stat pairs: normal, laser, fire, plasma, explosion.
        const PROTECTION: [usize; 5] = [17, 18, 19, 20, 23];
        for k in 0..7 {
            let y = 64 + 10 * k as i32;
            text(
                image,
                &painter.message("inventry.msg", k as i32),
                [377, y],
                24,
            );
            let stat = world.player_stat(k).to_string();
            text(image, stat.as_bytes(), [401, y], 16);
            let label = painter.message("inventry.msg", 7 + k as i32);
            text(image, &label, [417, y], 64);
            let value = match k {
                0 => format!(
                    "{}/{}",
                    world.hit_points(player).unwrap_or(0),
                    world.player_stat(7)
                ),
                1 => format!("   {}", world.player_stat(9)),
                _ => {
                    let dt = PROTECTION[k - 2];
                    format!("{}/{}%", world.player_stat(dt), world.player_stat(dt + 7))
                }
            };
            text(image, value.as_bytes(), [481, y], 48);
        }
        Self::panel_rule(painter, image, 138);
        Self::panel_rule(painter, image, 178);
        for (slot, y) in [(Slot::LeftHand, 144), (Slot::RightHand, 184)] {
            for (index, (line, clip)) in self.hand_lines(painter, slot).iter().enumerate() {
                text(image, line, [377, y + 10 * index as i32], *clip);
            }
        }
        let weight = [
            painter.message("inventry.msg", 20),
            format!(" {}", world.inventory_weight(player)).into_bytes(),
        ]
        .concat();
        text(image, &weight, [407, 224], 80);
    }
    /// §3.34 a hand's stats block: an empty hand's unarmed damage, else the
    /// item's name with an armor note, or a weapon's damage, range and
    /// loaded ammunition.
    pub(crate) fn hand_lines(&self, painter: &Painter, slot: Slot) -> Vec<(Vec<u8>, i32)> {
        let world = &self.host.world;
        let melee = world.player_stat(11);
        let message = |id: i32| painter.message("inventry.msg", id);
        let Some(item) = self.slot_item(slot) else {
            let damage = [message(24), format!(" 1-{}", melee + 2).into_bytes()].concat();
            return vec![(message(14), 120), (damage, 120)];
        };
        let name = world.get(item).map(|o| o.name.clone()).unwrap_or_default();
        let mut lines = vec![(name, 140)];
        match self.item_kind(item) {
            Some(ItemData::Armor(_)) => lines.push((message(18), 140)),
            Some(ItemData::Weapon(gun)) => {
                let mode = world
                    .get(item)
                    .and_then(|o| o.prototype.as_ref())
                    .map_or(0, |prototype| (prototype.extended.0 & 15) as u8);
                let subtype = vault_core::combat::Subtype::of(mode);
                let close = subtype.is_some_and(vault_core::combat::Subtype::close);
                let max = gun.damage[1] + if close { melee } else { 0 };
                let mut damage = [
                    message(15),
                    format!(" {}-{max}", gun.damage[0]).into_bytes(),
                ]
                .concat();
                if subtype == Some(vault_core::combat::Subtype::Ranged) || gun.ranges[0] > 1 {
                    damage.extend(b"   ");
                    damage.extend(message(16));
                    damage.extend(format!(" {}", gun.ranges[0]).into_bytes());
                }
                lines.push((damage, 140));
                if gun.capacity > 0 {
                    let (rounds, ammo) = world.loaded(item);
                    let mut loaded = [
                        message(17),
                        format!(" {rounds}/{}", gun.capacity).into_bytes(),
                    ]
                    .concat();
                    let name = u32::try_from(ammo)
                        .ok()
                        .filter(|pid| *pid != 0)
                        .and_then(|pid| self.host.assets.prototype(pid).ok())
                        .and_then(|prototype| self.host.assets.prototype_text(&prototype, false));
                    if let Some(name) = name {
                        loaded.push(b' ');
                        loaded.extend(name);
                    }
                    lines.push((loaded, 140));
                }
            }
            _ => {}
        }
        lines
    }
    /// §3.34 the examine text over the stats panel: the item's name on line
    /// 1, a rule at y = 64, then the wrapped text from line 3 and the
    /// weight line (`proto.msg` 540, or 541 for a weight of 1; none for 0).
    /// Line n is at y = 44 + 10n; lines past 17 are dropped.
    fn draw_examine(&self, painter: &Painter, image: &mut IndexedImage, examine: &Examine) {
        let width = layout::STATS.w;
        let line = |image: &mut IndexedImage, n: i32, text: &[u8]| {
            painter.text(image, 101, STATS_COLOR, text, [377, 44 + 10 * n], width);
        };
        line(image, 1, &examine.name);
        Self::panel_rule(painter, image, 64);
        let measure = |text: &[u8]| painter.width(101, text);
        let mut lines = layout::wrap_examine(&examine.text, width, measure);
        if examine.weight > 0 {
            let format = painter.message("proto.msg", if examine.weight == 1 { 541 } else { 540 });
            let weight = String::from_utf8_lossy(&format)
                .replacen("%d", &examine.weight.to_string(), 1)
                .into_bytes();
            lines.extend(layout::wrap_examine(&weight, width, measure));
        }
        for (n, text) in (3..=17).zip(&lines) {
            line(image, n, text);
        }
    }
    /// §3.34 the item menu at its placement (the pointer hidden): the arrow
    /// at the menu's top-left, or `actarrom.frm` at its top-right when
    /// flipped, then the 40-by-40 icons, the highlighted entry in its `h`
    /// art.
    pub(crate) fn draw_item_menu(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        menu: &ItemMenu,
    ) {
        let x = menu.origin[0];
        let (arrow, arrow_x, icons_x) = if menu.flipped {
            ("actarrom.frm", x - 28, x - 68)
        } else {
            (layout::ARROW_CURSOR, x, x + 29)
        };
        painter.blit(image, arrow, 0, [arrow_x, menu.top]);
        for (index, entry) in menu.entries.iter().enumerate() {
            let (normal, lit) = layout::entry_icons(*entry);
            let art = if index == menu.tracker.highlight {
                lit
            } else {
                normal
            };
            painter.blit(image, art, 0, [icons_x, menu.top + 40 * index as i32]);
        }
    }
}
