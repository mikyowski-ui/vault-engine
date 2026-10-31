//! SPEC-011 §3.17-3.19 barter in the SPEC-013 §3.18 window, opened when a
//! dialogue requests it (`gdialog_barter`): `barter.frm` replaces the
//! dialogue's lower subwindow at screen (0, 289); the item subwindow at
//! (80, 290) shows three rows of the player's inventory and offer table and
//! the merchant's offer table and inventory (its armor and held weapon
//! hidden for the session), with their scroll buttons and the two totals.
//!
//! The right button toggles the hand and arrow cursors (the loot window's
//! rules). In the hand cursor an entry is dragged: the left press lifts it
//! (`ipickup1`), the release (`iputdown`) over its route's drop rectangle
//! moves it, forced, through the move-items counter for a stack. In the
//! arrow cursor a hovered entry or a quick click shows its short look line
//! and a held press opens the action menu (table entries count as not the
//! player's); Use opens a container in its column, and the portraits go
//! back up a level. Offer (`m`) attempts the trade at the merchant's price;
//! Talk (`t`) and Esc return every table and the hidden equipment, then
//! resume dialogue. Under the `enhanced` preset a Fallout 2 party Trade
//! window adds the engine-drawn WIELD button, and a HUD party pill can
//! open it without a conversation (leaving then returns to the map).
use super::{
    MapPreview, Screen,
    counter::{CounterInput, counter_click, counter_key},
    modal_kit::{self, ArrowPress, ItemMenu},
};
use crate::AppResult;
/// The `enhanced` party Trade window's engine-drawn WIELD button and the
/// plate naming the member's held weapon (barter-window coordinates), side
/// by side along the middle panel's top edge, above the two tables' slots.
/// Fallout 2's `barter.frm` is the same frame as Fallout 1's, so they
/// clear its native buttons, slots and portraits in both games.
pub(crate) use vault_core::ui::inventory::{TRADE_WIELD, TRADE_WIELD_LABEL};
use vault_core::{
    ObjectAction, ObjectRef,
    barter::{BarterSession, Trade},
    skills::{self, Counter, ItemEntry},
    ui::{
        ColorKey, ERROR_SOUND, Rect,
        inventory::{
            self as layout, BARTER_BUTTON_SOUNDS, BARTER_ICONS, BARTER_OFFER, BARTER_PORTRAITS,
            BARTER_ROWS as ROWS, BARTER_SCROLL, BARTER_SUBWINDOW as SUBWINDOW, BARTER_TALK,
            BARTER_WINDOW as WINDOW, BarterControl, BarterView, scroll_list,
        },
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The open window. SPEC-020 §2.1: its hit boxes are the core's
/// [`layout::barter_layout`] over a [`BarterView`] filled from this state,
/// and its drops follow [`layout::barter_drop`]; the trade rules are
/// `vault_core::barter`'s. The core keeps no barter window state, so the
/// offsets and the cursor are the caller's with the world objects.
pub(crate) struct BarterScreen {
    pub session: BarterSession,
    /// Scroll offsets by column: player inventory, player table, merchant
    /// table, merchant inventory.
    offsets: [usize; 4],
    /// The lifted entry: its column, item and quantity.
    held: Option<(usize, ObjectRef, i32)>,
    /// The counter for the lifted stack's drop, or for a menu Drop of the
    /// item it names.
    counter: Option<(Counter, Option<ObjectRef>)>,
    /// The last trade or move message, shown in the conversation's reply
    /// area (the barter window covers the message monitor).
    pub reply: Option<Vec<u8>>,
    /// The arrow cursor (else the hand).
    pub arrow: bool,
    /// An arrow press and its column, waiting for its release or the hold
    /// pulse.
    press: Option<(usize, ArrowPress)>,
    /// The arrow cursor's item menu and its column.
    pub menu: Option<(usize, ItemMenu)>,
    /// The entry under the arrow cursor, whose look line was shown.
    hovered: Option<ObjectRef>,
    /// Containers opened in each column, outermost first.
    pub views: [Vec<ObjectRef>; 4],
    /// Opened from an `enhanced` HUD pill without a conversation: leaving
    /// returns to the map.
    pub direct: bool,
}
impl BarterScreen {
    pub(super) fn new(session: BarterSession, direct: bool) -> Self {
        Self {
            session,
            offsets: [0; 4],
            held: None,
            counter: None,
            reply: None,
            arrow: false,
            press: None,
            menu: None,
            hovered: None,
            views: Default::default(),
            direct,
        }
    }
    /// The item a drag holds (the counter shows the arrow instead).
    pub fn dragged(&self) -> Option<ObjectRef> {
        self.held
            .filter(|_| self.counter.is_none())
            .map(|(_, item, _)| item)
    }
    /// The move-items counter is open.
    pub fn counter_open(&self) -> bool {
        self.counter.is_some()
    }
}

/// The barter window's click targets (screen coordinates): the four
/// columns' slots, the scroll buttons, the portraits, Offer and Talk.
#[cfg(test)]
pub(crate) fn trade_targets() -> Vec<Rect> {
    let slots =
        (0..4).flat_map(|column| (0..ROWS).map(move |row| layout::barter_slot(column, row)));
    slots
        .chain(BARTER_SCROLL.map(|(_, _, rect, ..)| rect.offset(SUBWINDOW)))
        .chain(BARTER_PORTRAITS.map(|(_, rect)| rect.offset(WINDOW)))
        .chain([BARTER_OFFER, BARTER_TALK].map(|rect| rect.offset(WINDOW)))
        .collect()
}

impl MapPreview {
    /// A pending dialogue barter request opens the barter window with the
    /// dialogue's target as merchant.
    pub(crate) fn check_barter(&mut self) {
        let dialogue = &self.host.world.dialogue;
        if !matches!(self.screen, Screen::Map) || !dialogue.open || !dialogue.pending_barter {
            return;
        }
        match self.host.begin_barter(dialogue.target) {
            Ok(session) => {
                // SPEC-010 §3.12: switching to Barter stops speech without
                // an ending.
                self.host.stop_speech();
                self.screen = Screen::Barter(Box::new(BarterScreen::new(session, false)))
            }
            Err(error) => {
                self.status = error.to_string().into_bytes();
                self.host.world.dialogue.pending_barter = false;
            }
        }
    }
    /// The `enhanced` HUD pill's party Trade window under Fallout 2, opened
    /// on the map without a conversation (the host refuses it otherwise,
    /// printing the talk exception's message when the member is out of
    /// reach). Returns whether it opened.
    pub(crate) fn open_party_trade(&mut self, member: ObjectRef) -> bool {
        if !matches!(self.screen, Screen::Map) || self.host.world.dialogue.open {
            return false;
        }
        let opened = match self.host.open_party_trade(member) {
            Ok(Some(session)) => {
                self.screen = Screen::Barter(Box::new(BarterScreen::new(session, true)));
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                false
            }
        };
        self.drain_effects();
        opened
    }
    /// Whether the barter window shows the `enhanced` WIELD button: a
    /// Fallout 2 party Trade window under the enhanced preset.
    fn trade_wield_shown(&self, screen: &BarterScreen) -> bool {
        self.host.world.fallout2.is_some() && self.host.can_wield_for(screen.session.merchant)
    }
    /// The four holders shown, left to right: each column's innermost open
    /// container, else the player, the two tables and the merchant.
    fn barter_owners(&self) -> Option<[ObjectRef; 4]> {
        let Screen::Barter(screen) = &self.screen else {
            return None;
        };
        let session = &screen.session;
        let roots = [
            self.host.world.player,
            session.offer,
            session.table,
            session.merchant,
        ];
        Some(std::array::from_fn(|column| {
            screen.views[column]
                .last()
                .copied()
                .unwrap_or(roots[column])
        }))
    }
    /// What the window shows, for the core layout.
    fn barter_view(&self, screen: &BarterScreen) -> BarterView {
        let counts = self.barter_owners().map_or([0; 4], |owners| {
            owners.map(|owner| self.barter_entries(owner).len())
        });
        BarterView {
            wield: self.trade_wield_shown(screen),
            counts,
            offsets: screen.offsets,
        }
    }
    /// The control under a screen point, with its rectangle.
    fn barter_control_at(&self, point: [i32; 2]) -> Option<(BarterControl, Rect)> {
        let Screen::Barter(screen) = &self.screen else {
            return None;
        };
        let layout = layout::barter_layout(&self.barter_view(screen));
        let control = layout.hit(point)?;
        Some((control, layout.rect(control)?))
    }
    /// The filled entry at a screen point, and its column.
    fn barter_entry_at(&self, point: [i32; 2]) -> Option<(usize, ObjectRef, i32)> {
        let owners = self.barter_owners()?;
        let Screen::Barter(screen) = &self.screen else {
            return None;
        };
        let Some((BarterControl::Slot { column, row }, _)) = self.barter_control_at(point) else {
            return None;
        };
        let (item, quantity) = self
            .barter_entries(owners[column])
            .get(screen.offsets[column] + row)
            .copied()?;
        Some((column, item, quantity))
    }
    /// Whether a filled entry is under a screen point.
    pub(crate) fn barter_entry_at_point(&self, point: [i32; 2]) -> bool {
        self.barter_entry_at(point).is_some()
    }
    /// The text an action on `item` prints, shown in the reply area (the
    /// barter window covers the message monitor).
    fn barter_text(&mut self, item: ObjectRef, action: ObjectAction) {
        let text = self.action_text(item, action);
        if !text.is_empty() {
            self.barter_reply(text);
        }
        self.drain_effects();
    }
    /// The arrow cursor over a filled entry shows its short look line when
    /// the hovered entry changes.
    pub(crate) fn barter_hover(&mut self, point: [i32; 2]) {
        let entry = self.barter_entry_at(point).map(|(_, item, _)| item);
        let Screen::Barter(screen) = &mut self.screen else {
            return;
        };
        let dragging = screen.held.is_some();
        if let Some(item) = modal_kit::hover(screen.arrow, dragging, &mut screen.hovered, entry) {
            self.barter_text(item, ObjectAction::LookAt);
        }
    }
    /// H13: every column's open containers must still form an ownership
    /// chain from the column's holder (a move, trade or script can take a
    /// bag away). The chain is cut at the first container that is no
    /// longer inside the one before it, and each column's scroll offset is
    /// clamped to its entries. Returns whether anything changed.
    fn check_barter_views(&mut self) -> bool {
        let Screen::Barter(screen) = &self.screen else {
            return false;
        };
        let session = &screen.session;
        let roots = [
            self.host.world.player,
            session.offer,
            session.table,
            session.merchant,
        ];
        let world = &self.host.world;
        let valid: [usize; 4] = std::array::from_fn(|column| {
            let mut holder = roots[column];
            screen.views[column]
                .iter()
                .take_while(|container| {
                    let inside = world
                        .get(**container)
                        .is_some_and(|object| object.owner == Some(holder));
                    holder = **container;
                    inside
                })
                .count()
        });
        let mut changed = false;
        for (column, keep) in valid.into_iter().enumerate() {
            if let Screen::Barter(screen) = &mut self.screen
                && screen.views[column].len() > keep
            {
                screen.views[column].truncate(keep);
                screen.offsets[column] = 0;
                changed = true;
            }
        }
        let Some(owners) = self.barter_owners() else {
            return changed;
        };
        let counts = owners.map(|owner| self.barter_entries(owner).len());
        if let Screen::Barter(screen) = &mut self.screen {
            changed |= modal_kit::clamp_offsets(&mut screen.offsets, counts, ROWS);
        }
        changed
    }
    /// The hold pulse of an arrow press opens the item menu at the pointer;
    /// every frame also checks the columns' open containers.
    pub(crate) fn barter_tick(&mut self) -> bool {
        if let Screen::Barter(screen) = &self.screen {
            let session = screen.session.clone();
            self.host.sfall_barter_preview(&session);
        }
        let checked = self.check_barter_views();
        checked | self.barter_pulse()
    }
    fn barter_pulse(&mut self) -> bool {
        let (clock, at) = (self.clock, self.pointer.at);
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        let Some((column, press)) = screen.press.filter(|(_, press)| press.due(clock)) else {
            return false;
        };
        screen.press = None;
        // SPEC-013 §3.18: only the player's inventory column holds the
        // player's own items (table entries are not).
        let entries = self.window_item_menu(press.item, column == 0);
        let origin = at.unwrap_or(SUBWINDOW);
        if let Screen::Barter(screen) = &mut self.screen {
            screen.menu = Some((
                column,
                ItemMenu::new(press.item, entries, origin, layout::BARTER_EDGES),
            ));
        }
        self.menu_open_cue();
        true
    }
    /// An action menu entry, as in the loot window: Look shows the full
    /// examine text, Unload gives the ammunition, forced, to the item's
    /// owner, Use opens a container in its column (up to nine deep), and
    /// Drop drops the player's item on the ground, through the counter for
    /// a stack.
    fn barter_entry(&mut self, column: usize, item: ObjectRef, entry: ItemEntry) {
        match entry {
            ItemEntry::Look => self.barter_text(item, ObjectAction::Description),
            ItemEntry::Unload => self.unload_to_owner(item),
            ItemEntry::Use => {
                if let Screen::Barter(screen) = &mut self.screen
                    && screen.views[column].len() < skills::MAX_NESTING
                {
                    screen.views[column].push(item);
                    screen.offsets[column] = 0;
                }
            }
            ItemEntry::Drop => {
                let Some(owners) = self.barter_owners() else {
                    return;
                };
                let quantity = self
                    .barter_entries(owners[column])
                    .iter()
                    .find(|(entry, _)| *entry == item)
                    .map_or(1, |(_, quantity)| *quantity);
                if quantity > 1 {
                    if let Screen::Barter(screen) = &mut self.screen {
                        screen.counter = Some((Counter::new(quantity), Some(item)));
                    }
                } else {
                    self.drop_units(item, 1);
                }
            }
            ItemEntry::Cancel => {}
        }
    }
    fn barter_entries(&self, owner: ObjectRef) -> Vec<(ObjectRef, i32)> {
        self.host
            .world
            .get(owner)
            .map(|object| object.inventory.clone())
            .unwrap_or_default()
    }
    /// Leave barter: everything returns, the request clears, and a dialogue
    /// whose session already ended closes.
    fn end_barter_screen(&mut self) {
        let Screen::Barter(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if let Err(error) = self.host.end_barter(screen.session) {
            self.status = error.to_string().into_bytes();
        }
        if screen.direct {
            self.drain_effects();
            return;
        }
        let dialogue = &mut self.host.world.dialogue;
        dialogue.pending_barter = false;
        if dialogue.options.is_empty()
            && !dialogue.waiting
            && let Err(error) = self.host.close_dialogue()
        {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    fn attempt_barter(&mut self) {
        let Screen::Barter(screen) = &self.screen else {
            return;
        };
        let session = screen.session.clone();
        let trade = self.host.attempt_trade(&session);
        self.drain_effects();
        match trade {
            // H13 / SPEC-011 §3.19 step 4: the tables' items have changed
            // sides, so containers opened on a table close and the table
            // columns scroll back to the top (the inventories keep their
            // views, clamped by the next frame's check).
            Ok(Trade::Accepted) => {
                if let Screen::Barter(screen) = &mut self.screen {
                    for column in [1, 2] {
                        screen.views[column].clear();
                        screen.offsets[column] = 0;
                    }
                }
                self.check_barter_views();
                let text = self.status.clone();
                self.barter_reply(text);
            }
            // `inventry.msg` 27 or 28 reaches the reply area.
            Ok(Trade::Refused) => {
                let text = self.status.clone();
                self.barter_reply(text);
            }
            Ok(Trade::Nothing) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// M-I5 / SPEC-013 §3.18: the player table's plain value, the whole
    /// table even while a container opened on it is shown.
    fn barter_offer_value(&self) -> i64 {
        if let Some((_, offer)) = self.host.sfall_barter_shown() {
            return offer;
        }
        match &self.screen {
            Screen::Barter(screen) => self.host.world.inventory_value(screen.session.offer),
            _ => 0,
        }
    }
    fn barter_reply(&mut self, text: Vec<u8>) {
        if let Screen::Barter(screen) = &mut self.screen {
            screen.reply = Some(text);
        }
    }
    fn scroll_barter(&mut self, column: usize, up: bool) {
        let Some(owners) = self.barter_owners() else {
            return;
        };
        let count = self.barter_entries(owners[column]).len();
        let Screen::Barter(screen) = &mut self.screen else {
            return;
        };
        scroll_list(&mut screen.offsets[column], up, count, ROWS);
    }
    /// Keys: `m` offers, `t` and Esc leave; the scroll keys move one row.
    pub(crate) fn barter_key(&mut self, key: KeyCode) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, _)) = &mut screen.counter {
            let input = counter_key(counter, key);
            let input = self.counter_key_cue(input);
            return self.barter_counter(input);
        }
        // G-25 / SPEC-014 §3.34: all other input is ignored during a drag,
        // and the open item menu takes the keys too.
        if screen.held.is_some() || screen.menu.is_some() {
            return true;
        }
        let control = self.hud.modifiers[1];
        match key {
            KeyCode::Escape | KeyCode::KeyT => self.end_barter_screen(),
            KeyCode::KeyM => self.attempt_barter(),
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                self.scroll_barter(if control { 3 } else { 0 }, key == KeyCode::ArrowUp)
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                self.scroll_barter(if control { 2 } else { 1 }, key == KeyCode::PageUp)
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn barter_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if screen.counter.is_some() {
            self.press_counter_button(point);
            let Screen::Barter(screen) = &mut self.screen else {
                return false;
            };
            let Some((counter, _)) = &mut screen.counter else {
                return true;
            };
            let input = counter_click(counter, point);
            self.barter_counter(input);
            return true;
        }
        // G-25 / SPEC-014 §3.34: a drag or an open menu takes every other
        // input (the drag ends only with its release).
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
        let Some((control, rect)) = self.barter_control_at(point) else {
            return true;
        };
        let (press, release) = BARTER_BUTTON_SOUNDS;
        match control {
            BarterControl::Offer => {
                self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
                self.attempt_barter();
            }
            BarterControl::Talk => {
                self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
                self.end_barter_screen();
            }
            BarterControl::TradeWield => {
                self.press_button(rect);
                let Screen::Barter(screen) = &mut self.screen else {
                    return true;
                };
                if let Err(error) = self.host.barter_wield(&mut screen.session) {
                    self.status = error.to_string().into_bytes();
                }
                self.drain_effects();
            }
            BarterControl::Scroll { column, up } => {
                self.press_button(rect);
                self.scroll_barter(column, up);
            }
            // A press on a filled slot: the hand lifts the entry (row +
            // offset must stay below the column's length), the arrow waits
            // for the release or the hold pulse.
            BarterControl::Slot { .. } => {
                let clock = self.clock;
                let Some((column, item, quantity)) = self.barter_entry_at(point) else {
                    return true;
                };
                let Screen::Barter(screen) = &mut self.screen else {
                    return false;
                };
                if screen.arrow {
                    screen.press = Some((column, ArrowPress::new(item, clock)));
                } else {
                    screen.held = Some((column, item, quantity));
                    self.play_effect(b"ipickup1", None);
                }
            }
            // The portraits go up one container level: the inventory
            // column, else the side's table.
            BarterControl::Portrait { merchant } => {
                let Screen::Barter(screen) = &mut self.screen else {
                    return true;
                };
                let columns = if merchant { [3, 2] } else { [0, 1] };
                if let Some(column) = columns
                    .into_iter()
                    .find(|column| !screen.views[*column].is_empty())
                {
                    screen.views[column].pop();
                    screen.offsets[column] = 0;
                }
            }
        }
        true
    }
    /// The release ends a drag: inside its route's drop rectangle the entry
    /// moves (through the counter for a stack); elsewhere nothing.
    pub(crate) fn barter_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if let Some((column, menu)) = screen.menu.take() {
            // The pointer returns to where it was when the menu opened.
            self.pointer.at = Some(menu.origin);
            self.barter_entry(column, menu.item, menu.chosen());
            return true;
        }
        if let Some((_, press)) = screen.press.take() {
            // A quick click shows the short look text.
            self.barter_text(press.item, ObjectAction::LookAt);
            return true;
        }
        if screen.counter.is_some() {
            return false;
        }
        let Some((column, _, quantity)) = screen.held else {
            return false;
        };
        let dropped = point.is_some_and(|point| layout::barter_drop(column, point).is_some());
        if !dropped {
            screen.held = None;
        } else if quantity > 1 {
            screen.counter = Some((Counter::new(quantity), None));
        }
        self.play_effect(b"iputdown", None);
        if dropped && quantity == 1 {
            self.barter_drop(1);
        }
        true
    }
    fn barter_counter(&mut self, input: CounterInput) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        match input {
            CounterInput::Accept(quantity) => match screen.counter {
                Some((_, Some(item))) => {
                    screen.counter = None;
                    self.drop_units(item, quantity);
                }
                _ => self.barter_drop(quantity),
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
    /// Move `quantity` of the lifted entry along its route, forced; a
    /// failure shows `inventry.msg` 26 (player side) or 25 (merchant side).
    fn barter_drop(&mut self, quantity: i32) {
        let Some(owners) = self.barter_owners() else {
            return;
        };
        let Screen::Barter(screen) = &mut self.screen else {
            return;
        };
        screen.counter = None;
        let Some((column, item, _)) = screen.held.take() else {
            return;
        };
        let (to, _, failure) = layout::barter_route(column);
        if let Err(error) = self
            .host
            .barter_move(owners[column], owners[to], item, quantity)
        {
            tracing::debug!(%error, "barter move refused");
            let text = self.message("inventry.msg", failure);
            self.notify(text.clone());
            self.barter_reply(text);
        }
        self.drain_effects();
    }
    /// A side's portrait, drawn as in the loot window: the player's figure
    /// facing southwest, frame 0, or the merchant's art at its facing, last
    /// frame; an open container in the side's inventory column shows its own
    /// world art. Centered in the 60-by-100 box and clipped to it.
    fn draw_barter_portrait(
        &self,
        image: &mut IndexedImage,
        screen: &BarterScreen,
        merchant: bool,
        rect: Rect,
    ) {
        let world = &self.host.world;
        let column = if merchant { 3 } else { 0 };
        let (fid, direction, last) = match screen.views[column].last() {
            Some(container) => (world.get(*container).map(|o| o.raw.art_id), 0, false),
            None if merchant => {
                let object = world.get(screen.session.merchant);
                (
                    object.map(|o| o.raw.art_id),
                    object.map_or(0, |o| usize::from(o.raw.orientation)),
                    true,
                )
            }
            None => (
                self.host.figure_art(world.player, self.active_item(), 3),
                3,
                false,
            ),
        };
        self.draw_portrait(image, fid, (direction, last), rect);
    }
    pub(crate) fn barter_image(&mut self) -> AppResult<IndexedImage> {
        let Some(owners) = self.barter_owners() else {
            return Err("barter is not open".into());
        };
        let price = match (&self.screen, self.host.sfall_barter_shown()) {
            (_, Some((price, _))) => price,
            (Screen::Barter(screen), None) => {
                let session = screen.session.clone();
                self.host.barter_price(&session)
            }
            _ => 0,
        };
        // The conversation's upper part stays, with the last barter message
        // as its reply; the barter window covers its lower subwindow.
        let reply = match &self.screen {
            Screen::Barter(screen) => screen.reply.clone(),
            _ => None,
        };
        let saved = reply
            .map(|reply| std::mem::replace(&mut self.host.world.dialogue.resolved_reply, reply));
        let image = self.map_image();
        if let Some(saved) = saved {
            self.host.world.dialogue.resolved_reply = saved;
        }
        let mut image = image?;
        let Screen::Barter(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = self.painter();
        painter.blit_opaque(&mut image, "barter.frm", 0, WINDOW);
        for (column, owner) in owners.iter().enumerate() {
            let entries = self.barter_entries(*owner);
            let ([ix, iy], [w, h]) = BARTER_ICONS[column];
            for (row, entry) in entries
                .iter()
                .skip(screen.offsets[column])
                .take(ROWS)
                .enumerate()
            {
                // G-19: ammunition counts rounds (SPEC-013 §3.16).
                let lifted = screen.held.is_some_and(|(_, held, _)| held == entry.0);
                let rect = Rect::new(ix, iy + 48 * row as i32, w, h).offset(SUBWINDOW);
                self.draw_entry(&painter, &mut image, rect, *entry, lifted);
            }
        }
        for (_, _, rect, up, down) in BARTER_SCROLL {
            self.draw_button(&painter, &mut image, rect.offset(SUBWINDOW), (up, down));
        }
        for (merchant, rect) in BARTER_PORTRAITS {
            self.draw_barter_portrait(&mut image, screen, merchant, rect.offset(WINDOW));
        }
        for (text, x) in [
            (format!("${}", self.barter_offer_value()), 169),
            (format!("${price}"), 254),
        ] {
            let origin = modal_kit::offset(SUBWINDOW, [x, 168]);
            painter.text(
                &mut image,
                101,
                ColorKey::WHITE,
                text.as_bytes(),
                origin,
                80,
            );
        }
        for rect in [BARTER_OFFER, BARTER_TALK] {
            let art = ("di_rdbt2.frm", "di_rdbt1.frm");
            self.draw_button(&painter, &mut image, rect.offset(WINDOW), art);
        }
        if self.trade_wield_shown(screen) {
            let weapon = self.host.barter_held_weapon(&screen.session);
            let rects = [TRADE_WIELD, TRADE_WIELD_LABEL].map(|rect| rect.offset(WINDOW));
            self.draw_wield(&painter, &mut image, rects, weapon);
        }
        if let Some((_, menu)) = &screen.menu {
            self.draw_item_menu(&painter, &mut image, menu);
        }
        if let Some((counter, dropping)) = &screen.counter {
            let item = dropping.or(screen.held.map(|(_, item, _)| item));
            self.draw_item_counter(&painter, &mut image, counter, item);
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests;
