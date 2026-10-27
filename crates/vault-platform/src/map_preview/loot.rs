//! SPEC-013 §3.16-3.17 the container/corpse loot window (and steal mode)
//! over the world, with the move-items counter. The right button toggles
//! the hand and arrow cursors. In the hand cursor a drag lifts an entry on
//! the left press (the steal attempt counts here, `ipickup1`) and drops it
//! on the release (`iputdown`) when the pointer is over the other pane. In
//! the arrow cursor a hovered entry prints its short look line, a quick
//! click prints it too, and a held press opens the action menu: Look,
//! Unload or Use, Drop (the player's own items), Cancel. Use opens a
//! container in its pane; the portrait hot zones go back up a level.
use super::{
    MapPreview, Screen,
    counter::{CounterInput, counter_click, counter_key},
    modal_kit::{self, ArrowPress, ItemMenu, at},
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectRef,
    loot::{LootMove, LootSession},
    skills::{self, Counter, ItemEntry},
    ui::{ERROR_SOUND, Rect, fallout2 as ui2, inventory as layout},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct LootScreen {
    pub session: LootSession,
    /// Scroll offsets: player pane, target pane.
    pub offsets: [usize; 2],
    /// The lifted entry: its pane (true for the target) and item.
    pub held: Option<(bool, ObjectRef, i32)>,
    /// The open move-items counter: the lifted entry's move, or a Drop.
    pub counter: Option<(Counter, LootCount)>,
    /// The arrow cursor (else the hand).
    pub arrow: bool,
    /// An arrow press and its pane, waiting for its release or the hold
    /// pulse.
    press: Option<(bool, ArrowPress)>,
    /// The arrow cursor's item menu and its pane.
    pub menu: Option<(bool, ItemMenu)>,
    /// The entry under the arrow cursor, whose look line was printed.
    hovered: Option<ObjectRef>,
}

/// What the loot window's counter counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LootCount {
    /// The lifted entry into a pane (true for the target).
    Move(bool),
    /// The menu's Drop of an item at the player's position.
    Drop(ObjectRef),
}

const DONE: Rect = Rect::new(288, 328, 15, 16);
/// The `enhanced` window's engine-drawn Take All button, centered over the
/// middle panel above its emblem (the window's art has no such button).
pub(crate) const TAKE_ALL: Rect = Rect::new(214, 236, 108, 20);
/// The companion gear window's engine-drawn Distance toggle, above Take
/// All.
pub(crate) const DISTANCE: Rect = Rect::new(214, 210, 108, 20);
/// The companion gear window's engine-drawn WIELD button, above Distance,
/// and the label naming the held weapon above it.
pub(crate) const WIELD: Rect = Rect::new(214, 184, 108, 20);
pub(crate) const WIELD_LABEL: Rect = Rect::new(214, 168, 108, 12);
/// Body cycling (sending PgUp and PgDn) with its up and down art.
const CYCLE: [(Rect, bool, &str, &str); 2] = [
    (Rect::new(436, 162, 20, 18), false, "slu.frm", "sld.frm"),
    (Rect::new(456, 162, 20, 18), true, "sru.frm", "srd.frm"),
];
/// The portrait boxes and their hot zones, which go up one container
/// level: the player's (under the safe rule over the drawn portrait at
/// (176, 37), with item slots winning) and the target's.
const PORTRAITS: [(bool, Rect); 2] = [
    (false, Rect::new(176, 37, 60, 100)),
    (true, Rect::new(297, 37, 60, 100)),
];
const SLOT: [i32; 2] = [64, 48];
const SCROLL: [(bool, bool, Rect); 4] = [
    (false, true, Rect::new(128, 39, 22, 23)),
    (false, false, Rect::new(128, 62, 22, 23)),
    (true, true, Rect::new(379, 39, 22, 23)),
    (true, false, Rect::new(379, 62, 22, 23)),
];

/// The Gear window's click targets other than WIELD (window
/// coordinates): the item slots, scroll and body-cycling buttons, the
/// portraits, Done, and the engine-drawn Take All and Distance.
#[cfg(test)]
pub(crate) fn gear_targets() -> Vec<Rect> {
    let slots = (0..skills::LOOT_ROWS).flat_map(|row| [slot(false, row), slot(true, row)]);
    slots
        .chain(SCROLL.map(|(_, _, rect)| rect))
        .chain(CYCLE.map(|(rect, ..)| rect))
        .chain(PORTRAITS.map(|(_, rect)| rect))
        .chain([DONE, TAKE_ALL, DISTANCE])
        .collect()
}

fn slot(target: bool, row: usize) -> Rect {
    let [x, y] = if target {
        skills::loot_target_slot(row)
    } else {
        skills::loot_player_slot(row)
    };
    Rect::new(x, y, SLOT[0], SLOT[1])
}

impl MapPreview {
    /// A deferred loot or steal request opens the window over the world.
    pub(crate) fn check_loot(&mut self) {
        if !matches!(self.screen, Screen::Map) {
            return;
        }
        let Some(request) = self.host.take_loot_request() else {
            return;
        };
        match self.host.open_loot(request) {
            Ok(Some(session)) => self.show_loot(session),
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// The loot window over the world for `session`.
    fn show_loot(&mut self, session: vault_core::loot::LootSession) {
        self.screen = Screen::Loot(Box::new(LootScreen {
            session,
            offsets: [0, 0],
            held: None,
            counter: None,
            arrow: false,
            press: None,
            menu: None,
            hovered: None,
        }));
    }
    /// The `enhanced` Gear entry: the companion gear window, out of
    /// combat, on a party member (the host refuses anyone else).
    pub(crate) fn open_companion_gear(&mut self, companion: ObjectRef) {
        if !matches!(self.screen, Screen::Map) {
            return;
        }
        match self.host.open_companion_loot(companion) {
            Ok(Some(session)) => self.show_loot(session),
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.drain_effects();
    }
    pub(crate) fn loot_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Loot(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = self.painter();
        let [ox, oy] = skills::LOOT_ORIGIN;
        painter.blit_opaque(&mut image, "loot.frm", 0, [ox, oy]);
        for (index, target) in [false, true].into_iter().enumerate() {
            let entries = self.host.loot_items(&screen.session, target);
            for (row, entry) in entries
                .iter()
                .skip(screen.offsets[index])
                .take(skills::LOOT_ROWS)
                .enumerate()
            {
                let [x, y] = if target {
                    skills::loot_target_icon(row)
                } else {
                    skills::loot_player_icon(row)
                };
                // G-19: ammunition counts rounds, pack size × (quantity - 1)
                // plus the front pack (omitted while lifted).
                let lifted = screen.held.is_some_and(|(_, held, _)| held == entry.0);
                let rect = Rect::new(ox + x, oy + y, 56, 40);
                self.draw_entry(&painter, &mut image, rect, *entry, lifted);
            }
        }
        for target in [false, true] {
            self.draw_loot_portrait(&mut image, screen, target);
        }
        for (_, up, rect) in SCROLL {
            let art = if up {
                ("invupout.frm", "invupin.frm")
            } else {
                ("invdnout.frm", "invdnin.frm")
            };
            self.draw_button(&painter, &mut image, on_screen(rect), art);
        }
        if screen.session.bodies.len() > 1 {
            for (rect, _, up, down) in CYCLE {
                self.draw_button(&painter, &mut image, on_screen(rect), (up, down));
            }
        }
        self.draw_red_button(&painter, &mut image, on_screen(DONE));
        if self.take_all_shown(screen) {
            // SPEC-019 §3.7: Fallout 2's Take All pair at the enhanced
            // button's place, with the art's own size.
            let pressed = self.held == Some(on_screen(self.take_all_rect()));
            let art = self
                .host
                .world
                .fallout2
                .as_ref()
                .and_then(|_| painter.role(&ui2::TAKE_ALL[usize::from(pressed)]));
            match art {
                Some((name, _)) => {
                    painter.blit(&mut image, name, 0, [ox + TAKE_ALL.x, oy + TAKE_ALL.y]);
                }
                None => self.engine_button(&painter, &mut image, TAKE_ALL, b"TAKE ALL"),
            }
        }
        if screen.session.companion {
            let mode = self.host.companion_distance(screen.session.root);
            let label = format!("DISTANCE: {}", mode.name().to_ascii_uppercase());
            self.engine_button(&painter, &mut image, DISTANCE, label.as_bytes());
            let weapon = self.host.companion_weapon(screen.session.root);
            let rects = [on_screen(WIELD), on_screen(WIELD_LABEL)];
            self.draw_wield(&painter, &mut image, rects, weapon);
        }
        if let Some((_, menu)) = &screen.menu {
            self.draw_item_menu(&painter, &mut image, menu);
        }
        if let Some((counter, count)) = &screen.counter {
            let item = match count {
                LootCount::Drop(item) => Some(*item),
                LootCount::Move(_) => screen.held.map(|(_, item, _)| item),
            };
            self.draw_item_counter(&painter, &mut image, counter, item);
        }
        Ok(image)
    }
    /// §3.16 a pane's portrait, centered in its 60-by-100 box and clipped
    /// to it: the player's figure with the active hand's weapon code facing
    /// southwest (3), frame 0; the target's art at its facing, last frame;
    /// an open container's own world art.
    fn draw_loot_portrait(&self, image: &mut IndexedImage, screen: &LootScreen, target: bool) {
        let session = &screen.session;
        let shown = session.shown(target);
        let world = &self.host.world;
        let (fid, direction, last) = if shown != if target { session.root } else { session.looter }
        {
            (world.get(shown).map(|o| o.raw.art_id), 0, false)
        } else if target {
            let object = world.get(shown);
            (
                object.map(|o| o.raw.art_id),
                object.map_or(0, |o| usize::from(o.raw.orientation)),
                true,
            )
        } else {
            (self.host.figure_art(shown, self.active_item(), 3), 3, false)
        };
        let rect = on_screen(PORTRAITS[usize::from(target)].1);
        self.draw_portrait(image, fid, (direction, last), rect);
    }
    pub(super) fn close_loot_screen(&mut self) {
        let Screen::Loot(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if let Err(error) = self.host.close_loot(screen.session) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    /// The entries a pane shows.
    fn loot_entries(&self, target: bool) -> Vec<(ObjectRef, i32)> {
        match &self.screen {
            Screen::Loot(screen) => self.host.loot_items(&screen.session, target),
            _ => Vec::new(),
        }
    }
    /// The filled entry at a screen point, and its pane.
    fn loot_entry_at(&self, point: [i32; 2]) -> Option<(bool, ObjectRef, i32)> {
        let Screen::Loot(screen) = &self.screen else {
            return None;
        };
        let local = modal_kit::local(point, skills::LOOT_ORIGIN);
        [false, true].into_iter().find_map(|target| {
            let row = (0..skills::LOOT_ROWS).find(|row| slot(target, *row).contains(local))?;
            let entries = self.host.loot_items(&screen.session, target);
            let (item, quantity) = entries
                .get(screen.offsets[usize::from(target)] + row)
                .copied()?;
            Some((target, item, quantity))
        })
    }
    /// Whether a filled loot or barter entry is under a screen point (for
    /// the pick cursor).
    pub(crate) fn window_entry_under(&self, point: [i32; 2]) -> bool {
        match self.screen {
            Screen::Loot(_) => self.loot_entry_at(point).is_some(),
            Screen::Barter(_) => self.barter_entry_at_point(point),
            _ => false,
        }
    }
    /// §3.16 the arrow cursor over a filled entry prints its short look
    /// line when the hovered entry changes.
    pub(crate) fn loot_hover(&mut self, point: [i32; 2]) {
        let entry = self.loot_entry_at(point).map(|(_, item, _)| item);
        let Screen::Loot(screen) = &mut self.screen else {
            return;
        };
        let dragging = screen.held.is_some();
        if let Some(item) = modal_kit::hover(screen.arrow, dragging, &mut screen.hovered, entry) {
            self.look_line(item);
        }
    }
    /// Each pane's scroll offset stays within its entries (a move, Take
    /// All or a body change can shorten a pane under a scrolled view).
    fn clamp_loot_offsets(&mut self) -> bool {
        let Screen::Loot(screen) = &self.screen else {
            return false;
        };
        let counts =
            [false, true].map(|target| self.host.loot_items(&screen.session, target).len());
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        modal_kit::clamp_offsets(&mut screen.offsets, counts, skills::LOOT_ROWS)
    }
    pub(crate) fn loot_tick(&mut self) -> bool {
        let clamped = self.clamp_loot_offsets();
        clamped | self.loot_pulse()
    }
    /// The hold pulse of an arrow press opens the item menu at the pointer.
    fn loot_pulse(&mut self) -> bool {
        let (clock, at) = (self.clock, self.pointer.at);
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        let Some((target, press)) = screen.press.filter(|(_, press)| press.due(clock)) else {
            return false;
        };
        screen.press = None;
        let entries = self.window_item_menu(press.item, !target);
        let origin = at.unwrap_or(skills::LOOT_ORIGIN);
        if let Screen::Loot(screen) = &mut self.screen {
            screen.menu = Some((
                target,
                ItemMenu::new(press.item, entries, origin, layout::LOOT_EDGES),
            ));
        }
        self.menu_open_cue();
        true
    }
    /// §3.16 an action menu entry: Look prints the full examine text; Unload
    /// adds the ammunition, forced, to the item's owner; Use opens a
    /// container in its pane, up to nine deep; Drop drops the item at the
    /// player's position, through the counter for a stack.
    fn loot_entry(&mut self, target: bool, item: ObjectRef, entry: ItemEntry) {
        match entry {
            ItemEntry::Look => self.examine_line(item),
            ItemEntry::Unload => self.unload_to_owner(item),
            ItemEntry::Use => {
                if let Screen::Loot(screen) = &mut self.screen
                    && screen.session.open_container(target, item)
                {
                    screen.offsets[usize::from(target)] = 0;
                }
            }
            ItemEntry::Drop => {
                let quantity = self
                    .loot_entries(target)
                    .iter()
                    .find(|(entry, _)| *entry == item)
                    .map_or(1, |(_, quantity)| *quantity);
                if quantity > 1 {
                    if let Screen::Loot(screen) = &mut self.screen {
                        screen.counter = Some((Counter::new(quantity), LootCount::Drop(item)));
                    }
                } else {
                    self.drop_units(item, 1);
                }
            }
            ItemEntry::Cancel => {}
        }
    }
    /// Move the lifted entry's `quantity` into the other pane.
    fn loot_drop(&mut self, quantity: i32) {
        let Screen::Loot(screen) = &mut self.screen else {
            return;
        };
        let Some((from_target, item, _)) = screen.held.take() else {
            return;
        };
        screen.counter = None;
        let outcome = self
            .host
            .loot_move(&mut screen.session, from_target, item, quantity);
        self.drain_effects();
        match outcome {
            Ok(LootMove::Caught) => self.close_loot_screen(),
            Ok(_) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    pub(crate) fn loot_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let clock = self.clock;
        let entry = self.loot_entry_at(point);
        let take_all = self.take_all_rect();
        let take_all_shown = match &self.screen {
            Screen::Loot(screen) => self.take_all_shown(screen),
            _ => false,
        };
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        if screen.counter.is_some() {
            self.press_counter_button(point);
            let Screen::Loot(screen) = &mut self.screen else {
                return false;
            };
            let Some((counter, _)) = &mut screen.counter else {
                return true;
            };
            let input = counter_click(counter, point);
            self.loot_counter(input);
            return true;
        }
        // A drag or an open menu takes every other input.
        if screen.held.is_some() || screen.menu.is_some() {
            return true;
        }
        // The right button toggles the hand and arrow cursors.
        if right {
            screen.arrow = !screen.arrow;
            screen.press = None;
            screen.hovered = None;
            return true;
        }
        let local = modal_kit::local(point, skills::LOOT_ORIGIN);
        if DONE.contains(local) {
            self.press_button(on_screen(DONE));
            self.close_loot_screen();
            return true;
        }
        if DISTANCE.contains(local) && screen.session.companion {
            let companion = screen.session.root;
            self.press_button(on_screen(DISTANCE));
            self.host.cycle_companion_distance(companion);
            return true;
        }
        if WIELD.contains(local) && screen.session.companion {
            self.press_button(on_screen(WIELD));
            let Screen::Loot(screen) = &mut self.screen else {
                return true;
            };
            if let Err(error) = self.host.loot_wield(&mut screen.session) {
                self.status = error.to_string().into_bytes();
            }
            self.drain_effects();
            return true;
        }
        if take_all.contains(local) && take_all_shown {
            self.press_button(on_screen(take_all));
            self.loot_take_all();
            return true;
        }
        if let Some((target, up, rect)) = SCROLL.iter().find(|(_, _, rect)| rect.contains(local)) {
            let index = usize::from(*target);
            let count = self.host.loot_items(&screen.session, *target).len();
            modal_kit::scroll(&mut screen.offsets[index], *up, skills::LOOT_ROWS, count);
            self.press_button(on_screen(*rect));
            return true;
        }
        if screen.session.bodies.len() > 1
            && let Some((rect, forward, ..)) = CYCLE.iter().find(|(rect, ..)| rect.contains(local))
        {
            screen.session.cycle(*forward);
            screen.offsets[1] = 0;
            self.press_button(on_screen(*rect));
            return true;
        }
        // A press on a filled entry: the hand lifts it, the arrow waits for
        // the release or the hold pulse.
        if let Some((target, item, quantity)) = entry {
            if screen.arrow {
                screen.press = Some((target, ArrowPress::new(item, clock)));
                return true;
            }
            screen.session.press();
            screen.held = Some((target, item, quantity));
            self.play_effect(b"ipickup1", None);
            return true;
        }
        // The portrait hot zones go up one container level.
        if let Some((target, _)) = PORTRAITS.iter().find(|(_, rect)| rect.contains(local)) {
            screen.session.up(*target);
            screen.offsets[usize::from(*target)] = 0;
        }
        true
    }
    /// The counter's outcome for the lifted entry.
    fn loot_counter(&mut self, input: CounterInput) -> bool {
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        match input {
            CounterInput::Accept(quantity) => match screen.counter.take() {
                Some((_, LootCount::Drop(item))) => self.drop_units(item, quantity),
                _ => self.loot_drop(quantity),
            },
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
    /// The release ends a drag: over the other pane's drop zone it moves
    /// the entry (through the counter for a stack); elsewhere nothing.
    pub(crate) fn loot_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        if let Some((target, menu)) = screen.menu.take() {
            // The pointer returns to where it was when the menu opened.
            self.pointer.at = Some(menu.origin);
            self.loot_entry(target, menu.item, menu.chosen());
            return true;
        }
        if let Some((_, press)) = screen.press.take() {
            // A quick click prints the short look text.
            self.look_line(press.item);
            return true;
        }
        if screen.counter.is_some() {
            return false;
        }
        let Some((from_target, _, quantity)) = screen.held else {
            return false;
        };
        match point.and_then(skills::loot_drop_target) {
            Some(to_target) if to_target != from_target => {
                if quantity > 1 {
                    screen.counter = Some((Counter::new(quantity), LootCount::Move(to_target)));
                    self.play_effect(b"iputdown", None);
                } else {
                    self.play_effect(b"iputdown", None);
                    self.loot_drop(1);
                }
            }
            _ => {
                screen.held = None;
                self.play_effect(b"iputdown", None);
            }
        }
        true
    }
    pub(crate) fn loot_key(&mut self, key: KeyCode) -> bool {
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, _)) = &mut screen.counter {
            // Only Enter accepts, after `ib1p1xx1`.
            let input = counter_key(counter, key);
            let input = self.counter_key_cue(input);
            return self.loot_counter(input);
        }
        // SPEC-014 §3.34: all other input is ignored during a drag, and the
        // open item menu takes the keys too.
        if screen.held.is_some() || screen.menu.is_some() {
            return true;
        }
        let [shift, control, _] = self.hud.modifiers;
        match key {
            KeyCode::Escape => self.close_loot_screen(),
            // G-31: Ctrl+Q, Ctrl+X and F10 open the quit confirmation; its
            // Yes closes the window (a quit request) and returns to the menu.
            KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
            KeyCode::F10 => self.confirm_quit(),
            // G-20: Take All is Shift+A (code `A`) only, in non-steal
            // sessions; the refusal is silent.
            KeyCode::KeyA if shift => self.loot_take_all(),
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let target = control;
                let count = self.host.loot_items(&screen.session, target).len();
                let offset = &mut screen.offsets[usize::from(target)];
                modal_kit::scroll(offset, key == KeyCode::ArrowUp, skills::LOOT_ROWS, count);
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                screen.session.cycle(key == KeyCode::PageDown);
                screen.offsets[1] = 0;
            }
            _ => return false,
        }
        true
    }
}

impl MapPreview {
    /// An engine-drawn window button at window-relative `rect`, its colors
    /// swapped while pressed.
    fn engine_button(&self, painter: &Painter, image: &mut IndexedImage, rect: Rect, label: &[u8]) {
        self.engine_button_at(painter, image, on_screen(rect), label);
    }
    /// The `enhanced` window shows its Take All button outside steal
    /// sessions (Vanilla, the touch UI's navigation-only view included,
    /// has Shift+A only).
    fn take_all_shown(&self, screen: &LootScreen) -> bool {
        (self.enhanced_extras() || self.host.world.fallout2.is_some()) && !screen.session.steal
    }
    /// The Take All button's window rectangle: the enhanced one, or under
    /// Fallout 2 its art's size at the same place.
    fn take_all_rect(&self) -> Rect {
        let painter = self.painter();
        self.host
            .world
            .fallout2
            .as_ref()
            .and_then(|_| painter.role(&ui2::TAKE_ALL[0]))
            .and_then(|(_, art)| art.frame(0, 0).map(|frame| (frame.width, frame.height)))
            .map_or(TAKE_ALL, |(w, h)| {
                Rect::new(TAKE_ALL.x, TAKE_ALL.y, i32::from(w), i32::from(h))
            })
    }
    /// G-20 Take All (Shift+A, or the enhanced button): refused silently
    /// in steal sessions by the host.
    fn loot_take_all(&mut self) {
        let Screen::Loot(screen) = &self.screen else {
            return;
        };
        let session = screen.session.clone();
        if let Err(error) = self.host.loot_take_all(&session) {
            self.status = error.to_string().into_bytes();
        }
    }
}

/// A window-relative rectangle on the screen.
const fn on_screen(rect: Rect) -> Rect {
    at(skills::LOOT_ORIGIN, rect)
}
