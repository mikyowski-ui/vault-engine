//! SPEC-011 §3.17-3.19 barter in the SPEC-013 §3.18 window, opened when a
//! dialogue requests it (`gdialog_barter`): `barter.frm` replaces the
//! dialogue's lower subwindow at screen (0, 289); the item subwindow at
//! (80, 290) shows three rows of the player's inventory and offer table and
//! the merchant's offer table and inventory (its armor and held weapon
//! hidden for the session), with their scroll buttons and the two totals.
//!
//! An entry is dragged: the left press lifts it (`ipickup1`), the release
//! (`iputdown`) over its route's drop rectangle moves it, forced, through
//! the move-items counter for a stack. Offer (`m`) attempts the trade at
//! the merchant's price; Talk (`t`) and Esc return every table and the
//! hidden equipment, then resume dialogue.
use super::{
    MapPreview, Screen,
    counter::{CounterInput, counter_click, counter_key, draw_counter},
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectRef,
    barter::{BarterSession, Trade},
    skills::{self, Counter},
    ui::{BUTTON_PRESS, ColorKey, ERROR_SOUND, Rect},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct BarterScreen {
    pub session: BarterSession,
    /// Scroll offsets by column: player inventory, player table, merchant
    /// table, merchant inventory.
    offsets: [usize; 4],
    /// The lifted entry: its column, item and quantity.
    held: Option<(usize, ObjectRef, i32)>,
    /// The counter for the lifted stack's drop.
    counter: Option<Counter>,
    /// The last trade or move message, shown in the conversation's reply
    /// area (the barter window covers the message monitor).
    pub reply: Option<Vec<u8>>,
}
impl BarterScreen {
    /// The item a drag holds (the counter shows the arrow instead).
    pub fn dragged(&self) -> Option<ObjectRef> {
        self.held
            .filter(|_| self.counter.is_none())
            .map(|(_, item, _)| item)
    }
}

const WINDOW: [i32; 2] = [0, 289];
const SUBWINDOW: [i32; 2] = [80, 290];
const ROWS: usize = 3;
/// Per column (subwindow coordinates): the slot buttons and icon boxes.
const SLOTS: [[i32; 2]; 4] = [[29, 35], [165, 20], [250, 20], [395, 35]];
const ICONS: [([i32; 2], [i32; 2]); 4] = [
    ([26, 39], [59, 40]),
    ([169, 24], [56, 40]),
    ([254, 24], [56, 40]),
    ([397, 39], [56, 40]),
];
const SLOT: [i32; 2] = [64, 48];
/// Scroll buttons (subwindow coordinates): column, up, rectangle, art.
const SCROLL: [(usize, bool, Rect, &str); 8] = [
    (0, true, Rect::new(109, 56, 23, 24), "di_up1.frm"),
    (0, false, Rect::new(109, 82, 24, 25), "di_down1.frm"),
    (3, true, Rect::new(342, 56, 23, 24), "di_up1.frm"),
    (3, false, Rect::new(342, 82, 24, 25), "di_down1.frm"),
    (1, true, Rect::new(128, 113, 22, 23), "invupout.frm"),
    (1, false, Rect::new(128, 136, 22, 23), "invdnout.frm"),
    (2, true, Rect::new(333, 113, 22, 23), "invupout.frm"),
    (2, false, Rect::new(333, 136, 22, 23), "invdnout.frm"),
];
/// Offer and Talk (barter-window coordinates).
const OFFER: Rect = Rect::new(41, 163, 14, 14);
const TALK: Rect = Rect::new(584, 162, 14, 14);
/// Drop rectangles span screen y 310-454.
const DROP_Y: std::ops::RangeInclusive<i32> = 310..=454;

/// The route from a column: the destination column, its drop ranges in
/// screen x, and the failure message. The player-table return also takes
/// the visible player column (screen x 109-172) under the safe rule.
fn route(column: usize) -> (usize, &'static [(i32, i32)], i32) {
    match column {
        0 => (1, &[(245, 309)], 26),
        1 => (0, &[(80, 144), (109, 172)], 26),
        2 => (3, &[(475, 539)], 25),
        _ => (2, &[(330, 394)], 25),
    }
}

fn slot_rect(column: usize, row: usize) -> Rect {
    let [x, y] = SLOTS[column];
    Rect::new(
        SUBWINDOW[0] + x,
        SUBWINDOW[1] + y + 48 * row as i32,
        SLOT[0],
        SLOT[1],
    )
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
                self.screen = Screen::Barter(Box::new(BarterScreen {
                    session,
                    offsets: [0; 4],
                    held: None,
                    counter: None,
                    reply: None,
                }))
            }
            Err(error) => {
                self.status = error.to_string().into_bytes();
                self.host.world.dialogue.pending_barter = false;
            }
        }
    }
    /// The four holders, left to right.
    fn barter_owners(&self) -> Option<[ObjectRef; 4]> {
        let Screen::Barter(screen) = &self.screen else {
            return None;
        };
        let session = &screen.session;
        Some([
            self.host.world.player,
            session.offer,
            session.table,
            session.merchant,
        ])
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
            // `inventry.msg` 27 or 28 reaches the reply area.
            Ok(Trade::Accepted | Trade::Refused) => {
                let text = self.status.clone();
                self.barter_reply(text);
            }
            Ok(Trade::Nothing) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
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
        let offset = &mut screen.offsets[column];
        if up {
            *offset = offset.saturating_sub(1);
        } else if *offset + ROWS < count {
            *offset += 1;
        }
    }
    /// Keys: `m` offers, `t` and Esc leave; the scroll keys move one row.
    pub(crate) fn barter_key(&mut self, key: KeyCode) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if let Some(counter) = &mut screen.counter {
            let input = counter_key(counter, key);
            if matches!(input, CounterInput::Accept(_)) {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            return self.barter_counter(input);
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
        let Some(owners) = self.barter_owners() else {
            return false;
        };
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if let Some(counter) = &mut screen.counter {
            let input = counter_click(counter, point);
            if input != CounterInput::Ignored {
                self.press();
            }
            self.barter_counter(input);
            return true;
        }
        screen.held = None;
        let window = [point[0] - WINDOW[0], point[1] - WINDOW[1]];
        if OFFER.contains(window) {
            self.press_cue(b"ib2p1xx1", b"ib2lu1x1");
            self.attempt_barter();
            return true;
        }
        if TALK.contains(window) {
            self.press_cue(b"ib2p1xx1", b"ib2lu1x1");
            self.end_barter_screen();
            return true;
        }
        let local = [point[0] - SUBWINDOW[0], point[1] - SUBWINDOW[1]];
        if let Some((column, up, ..)) = SCROLL.iter().find(|(.., rect, _)| rect.contains(local)) {
            self.press();
            self.scroll_barter(*column, *up);
            return true;
        }
        if right {
            return true;
        }
        // A press on a filled slot lifts the entry (row + offset must stay
        // below the column's length).
        for (column, owner) in owners.iter().enumerate() {
            let Some(row) = (0..ROWS).find(|row| slot_rect(column, *row).contains(point)) else {
                continue;
            };
            let entries = self.barter_entries(*owner);
            let Screen::Barter(screen) = &mut self.screen else {
                return false;
            };
            if let Some((item, quantity)) = entries.get(screen.offsets[column] + row).copied() {
                screen.held = Some((column, item, quantity));
                self.play_effect(b"ipickup1", None);
            }
            return true;
        }
        true
    }
    /// The release ends a drag: inside its route's drop rectangle the entry
    /// moves (through the counter for a stack); elsewhere nothing.
    pub(crate) fn barter_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Barter(screen) = &mut self.screen else {
            return false;
        };
        if screen.counter.is_some() {
            return false;
        }
        let Some((column, _, quantity)) = screen.held else {
            return false;
        };
        let (_, ranges, _) = route(column);
        let dropped = point.is_some_and(|[x, y]| {
            DROP_Y.contains(&y)
                && ranges
                    .iter()
                    .any(|(left, right)| (*left..=*right).contains(&x))
        });
        if !dropped {
            screen.held = None;
        } else if quantity > 1 {
            screen.counter = Some(Counter::new(quantity));
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
            CounterInput::Accept(quantity) => self.barter_drop(quantity),
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
        let (to, _, failure) = route(column);
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
    pub(crate) fn barter_image(&mut self) -> AppResult<IndexedImage> {
        let Some(owners) = self.barter_owners() else {
            return Err("barter is not open".into());
        };
        let price = match &self.screen {
            Screen::Barter(screen) => {
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
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        painter.blit_opaque(&mut image, "barter.frm", 0, WINDOW);
        for (column, owner) in owners.iter().enumerate() {
            let entries = self.barter_entries(*owner);
            let ([ix, iy], size) = ICONS[column];
            for (row, (item, quantity)) in entries
                .iter()
                .skip(screen.offsets[column])
                .take(ROWS)
                .enumerate()
            {
                let lifted = screen.held.is_some_and(|(_, held, _)| held == *item);
                let shown = if lifted { quantity - 1 } else { *quantity };
                let origin = [SUBWINDOW[0] + ix, SUBWINDOW[1] + iy + 48 * row as i32];
                if shown > 0
                    && let Some(icon) = self.item_icon(*item)
                    && let Some(frame) = icon.frame(0, 0)
                {
                    image.draw_frame(
                        frame,
                        [origin[0] as u32, origin[1] as u32],
                        [size[0] as u32, size[1] as u32],
                    );
                }
                if let Some(label) = skills::quantity_label(shown, None) {
                    painter.text(&mut image, 101, ColorKey::WHITE, &label, origin, 80);
                }
            }
        }
        for (_, _, rect, art) in SCROLL {
            painter.blit(
                &mut image,
                art,
                0,
                [SUBWINDOW[0] + rect.x, SUBWINDOW[1] + rect.y],
            );
        }
        let world = &self.host.world;
        for (text, x) in [
            (format!("${}", world.inventory_value(owners[1])), 169),
            (format!("${price}"), 254),
        ] {
            painter.text(
                &mut image,
                101,
                ColorKey::WHITE,
                text.as_bytes(),
                [SUBWINDOW[0] + x, SUBWINDOW[1] + 168],
                80,
            );
        }
        for rect in [OFFER, TALK] {
            painter.blit(
                &mut image,
                "di_rdbt2.frm",
                0,
                [WINDOW[0] + rect.x, WINDOW[1] + rect.y],
            );
        }
        if let Some(counter) = &screen.counter {
            let icon = screen.held.and_then(|(_, item, _)| self.item_icon(item));
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
