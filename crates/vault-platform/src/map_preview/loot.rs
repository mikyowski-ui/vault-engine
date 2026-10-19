//! SPEC-013 §3.16-3.17 the container/corpse loot window (and steal mode)
//! over the world, with the move-items counter. A drag lifts an entry on
//! the left press (the steal attempt counts here, `ipickup1`) and drops it
//! on the release (`iputdown`) when the pointer is over the other pane.
use super::{
    MapPreview, Screen,
    counter::{CounterInput, counter_click, counter_key, draw_counter},
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectRef,
    assets::WorldAssets,
    loot::{LootMove, LootSession},
    skills::{self, Counter},
    ui::{BUTTON_PRESS, ColorKey, ERROR_SOUND, Rect},
};
use vault_formats::pro::PrototypeData;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct LootScreen {
    pub session: LootSession,
    /// Scroll offsets: player pane, target pane.
    pub offsets: [usize; 2],
    /// The lifted entry: its pane (true for the target) and item.
    pub held: Option<(bool, ObjectRef, i32)>,
    /// The open move-items counter for the lifted entry's drop.
    pub counter: Option<(Counter, bool)>,
}

const DONE: Rect = Rect::new(288, 328, 15, 16);
const SLOT: [i32; 2] = [64, 48];
const SCROLL: [(bool, bool, Rect); 4] = [
    (false, true, Rect::new(128, 39, 22, 23)),
    (false, false, Rect::new(128, 62, 22, 23)),
    (true, true, Rect::new(379, 39, 22, 23)),
    (true, false, Rect::new(379, 62, 22, 23)),
];

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
            Ok(Some(session)) => {
                self.screen = Screen::Loot(Box::new(LootScreen {
                    session,
                    offsets: [0, 0],
                    held: None,
                    counter: None,
                }))
            }
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.drain_effects();
    }
    pub(crate) fn item_icon(
        &self,
        item: ObjectRef,
    ) -> Option<std::sync::Arc<vault_formats::FrmArchive>> {
        let fid = self.host.world.get(item).and_then(|object| {
            object
                .prototype
                .as_ref()
                .and_then(|prototype| match &prototype.data {
                    PrototypeData::Item(data) => Some(data.inventory_art),
                    _ => None,
                })
        })?;
        self.host.assets.object_art(fid).ok()
    }
    pub(crate) fn loot_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Loot(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [ox, oy] = skills::LOOT_ORIGIN;
        painter.blit_opaque(&mut image, "loot.frm", 0, [ox, oy]);
        for (index, target) in [false, true].into_iter().enumerate() {
            let entries = self.host.loot_items(&screen.session, target);
            for (row, (item, quantity)) in entries
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
                let lifted = screen.held.is_some_and(|(_, held, _)| held == *item);
                let shown = if lifted { quantity - 1 } else { *quantity };
                if shown > 0
                    && let Some(icon) = self.item_icon(*item)
                    && let Some(frame) = icon.frame(0, 0)
                {
                    image.draw_frame(frame, [(ox + x) as u32, (oy + y) as u32], [56, 40]);
                }
                if let Some(label) = skills::quantity_label(shown, None) {
                    painter.text(
                        &mut image,
                        101,
                        ColorKey::WHITE,
                        &label,
                        [ox + x, oy + y],
                        80,
                    );
                }
            }
        }
        for (_, up, rect) in SCROLL {
            let art = if up { "invupout.frm" } else { "invdnout.frm" };
            painter.blit(&mut image, art, 0, [ox + rect.x, oy + rect.y]);
        }
        if screen.session.bodies.len() > 1 {
            painter.blit(&mut image, "slu.frm", 0, [ox + 436, oy + 162]);
            painter.blit(&mut image, "sru.frm", 0, [ox + 456, oy + 162]);
        }
        painter.blit(&mut image, "lilredup.frm", 0, [ox + DONE.x, oy + DONE.y]);
        if let Some((counter, _)) = &screen.counter {
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
    fn close_loot_screen(&mut self) {
        let Screen::Loot(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if let Err(error) = self.host.close_loot(screen.session) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
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
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        if let Some((counter, _)) = &mut screen.counter {
            let input = counter_click(counter, point);
            if input != CounterInput::Ignored {
                self.press();
            }
            self.loot_counter(input);
            return true;
        }
        let [ox, oy] = skills::LOOT_ORIGIN;
        let local = [point[0] - ox, point[1] - oy];
        if DONE.contains(local) {
            self.press();
            self.close_loot_screen();
            return true;
        }
        if let Some((target, up, _)) = SCROLL.iter().find(|(_, _, rect)| rect.contains(local)) {
            let index = usize::from(*target);
            let count = self.host.loot_items(&screen.session, *target).len();
            let offset = &mut screen.offsets[index];
            if *up {
                *offset = offset.saturating_sub(1);
            } else if *offset + skills::LOOT_ROWS < count {
                *offset += 1;
            }
            return true;
        }
        // A press on a filled entry lifts it (an arrow-cursor right press
        // prints its look text instead).
        for target in [false, true] {
            let Some(row) = (0..skills::LOOT_ROWS).find(|row| slot(target, *row).contains(local))
            else {
                continue;
            };
            let entries = self.host.loot_items(&screen.session, target);
            let Some((item, quantity)) = entries
                .get(screen.offsets[usize::from(target)] + row)
                .copied()
            else {
                return true;
            };
            if right {
                let name = self
                    .host
                    .world
                    .get(item)
                    .map(|o| o.name.clone())
                    .unwrap_or_default();
                self.notify(name);
                return true;
            }
            screen.session.press();
            screen.held = Some((target, item, quantity));
            self.play_effect(b"ipickup1", None);
            return true;
        }
        true
    }
    /// The counter's outcome for the lifted entry.
    fn loot_counter(&mut self, input: CounterInput) -> bool {
        let Screen::Loot(screen) = &mut self.screen else {
            return false;
        };
        match input {
            CounterInput::Accept(quantity) => self.loot_drop(quantity),
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
        if screen.counter.is_some() {
            return false;
        }
        let Some((from_target, _, quantity)) = screen.held else {
            return false;
        };
        match point.and_then(skills::loot_drop_target) {
            Some(to_target) if to_target != from_target => {
                if quantity > 1 {
                    screen.counter = Some((Counter::new(quantity), to_target));
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
            let input = counter_key(counter, key);
            if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter)
                && matches!(input, CounterInput::Accept(_))
            {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            return self.loot_counter(input);
        }
        let control = self.hud.modifiers[1];
        match key {
            KeyCode::Escape => self.close_loot_screen(),
            // Take All is Shift+A in non-steal sessions; the refusal is silent.
            KeyCode::KeyA => {
                let session = screen.session.clone();
                if let Err(error) = self.host.loot_take_all(&session) {
                    self.status = error.to_string().into_bytes();
                }
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let target = control;
                let count = self.host.loot_items(&screen.session, target).len();
                let offset = &mut screen.offsets[usize::from(target)];
                if key == KeyCode::ArrowUp {
                    *offset = offset.saturating_sub(1);
                } else if *offset + skills::LOOT_ROWS < count {
                    *offset += 1;
                }
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
