//! The item windows' shared parts (SPEC-013 §3.16-3.18, SPEC-014 §3.34):
//! the arrow cursor's press and item menu, the scroll offsets, the entry
//! grid's icons and quantity labels, the portraits and the rotating
//! figure, the move-items counter's adapters, the look and examine text,
//! and the `enhanced` engine-drawn buttons. The inventory, loot and barter
//! windows draw and hit-test through these.
use super::super::{
    MapPreview,
    counter::{CounterInput, counter_button, draw_counter},
};
use crate::interface::Painter;
use vault_core::{
    ObjectAction, ObjectRef, WorldEffect,
    assets::WorldAssets,
    skills::{self, Counter, ItemEntry},
    ui::{
        BUTTON_PRESS, ColorKey, Rect,
        cursor::{self, MenuTracker},
        inventory as layout,
    },
};
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::{ItemData, PrototypeData},
};
use vault_render::IndexedImage;

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

/// Each scroll offset stays within its list's entries (a move, a trade or
/// a container change can shorten a list under a scrolled view). Returns
/// whether any offset changed.
pub(crate) fn clamp_offsets(
    offsets: &mut [usize],
    counts: impl IntoIterator<Item = usize>,
    rows: usize,
) -> bool {
    let mut changed = false;
    for (offset, count) in offsets.iter_mut().zip(counts) {
        let limit = count.saturating_sub(rows);
        if *offset > limit {
            *offset = limit;
            changed = true;
        }
    }
    changed
}

/// The arrow cursor over a filled entry: when the hovered entry changes
/// (in the arrow cursor, with no drag under way), the newly hovered item
/// whose short look line shows.
pub(crate) fn hover(
    arrow: bool,
    dragging: bool,
    hovered: &mut Option<ObjectRef>,
    entry: Option<ObjectRef>,
) -> Option<ObjectRef> {
    if !arrow || dragging || *hovered == entry {
        return None;
    }
    *hovered = entry;
    entry
}

impl MapPreview {
    /// An item's inventory art.
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
    /// An item's prototype data.
    pub(crate) fn item_kind(&self, item: ObjectRef) -> Option<&ItemData> {
        match &self.host.world.get(item)?.prototype.as_deref()?.data {
            PrototypeData::Item(data) => Some(&data.data),
            _ => None,
        }
    }
    /// SPEC-013 §3.16 the loot and barter windows' item menu: Look, then
    /// Unload for a weapon holding ammunition or Use for a container, Drop
    /// for the player's `own` items, and Cancel.
    pub(crate) fn window_item_menu(&self, item: ObjectRef, own: bool) -> Vec<ItemEntry> {
        let data = self.item_kind(item);
        let unloadable = matches!(data, Some(ItemData::Weapon(gun)) if gun.capacity > 0)
            && self.host.world.loaded(item).0 > 0;
        let container = matches!(data, Some(ItemData::Container { .. }));
        skills::item_menu(own, unloadable, container)
    }
    /// The player's `action` on `item`, its message lines joined by spaces
    /// and taken from the effects (the other effects stay queued).
    pub(crate) fn action_text(&mut self, item: ObjectRef, action: ObjectAction) -> Vec<u8> {
        let player = self.host.world.player;
        let start = self.host.world.effects.len();
        if let Err(error) = self.host.action(player, item, action) {
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
        text
    }
    /// The player drops `quantity` of `item` on its hex.
    pub(crate) fn drop_units(&mut self, item: ObjectRef, quantity: i32) {
        let player = self.host.world.player;
        if let Err(error) = self.host.world.drop_item(player, item, quantity) {
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    /// The ammunition terms of a quantity label: the pack size and the
    /// front pack's rounds, which a dragged entry leaves out (SPEC-013
    /// §3.16).
    pub(crate) fn ammo_label(&self, item: ObjectRef, dragged: bool) -> Option<(i32, i32)> {
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
    /// An item's inventory art scaled into `rect`.
    pub(crate) fn draw_icon(&self, image: &mut IndexedImage, item: ObjectRef, rect: Rect) {
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
    /// A list entry in its icon box `rect`: the item's art scaled into it
    /// and the quantity label at its top-left. A `lifted` stack shows one
    /// fewer, its ammunition without the front pack (G-19); an entry with
    /// nothing left shows only its label.
    pub(crate) fn draw_entry(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        rect: Rect,
        (item, quantity): (ObjectRef, i32),
        lifted: bool,
    ) {
        let shown = if lifted { quantity - 1 } else { quantity };
        if shown > 0 {
            self.draw_icon(image, item, rect);
        }
        if let Some(label) = skills::quantity_label(shown, self.ammo_label(item, lifted)) {
            painter.text(image, 101, ColorKey::WHITE, &label, [rect.x, rect.y], 80);
        }
    }
    /// A portrait or figure: `fid`'s art at `direction` (else direction
    /// 0), its last frame when `last` (else frame 0), centered in `rect`
    /// and clipped to it, drawn transparently.
    pub(crate) fn draw_portrait(
        &self,
        image: &mut IndexedImage,
        fid: Option<u32>,
        (direction, last): (usize, bool),
        rect: Rect,
    ) {
        let Some(art) = fid.and_then(|fid| self.host.assets.object_art(fid).ok()) else {
            return;
        };
        let index = if last {
            art.frame_count().saturating_sub(1)
        } else {
            0
        };
        let Some(frame) = art.frame(direction, index).or_else(|| art.frame(0, index)) else {
            return;
        };
        let origin = [
            i64::from(rect.x + (rect.w - i32::from(frame.width)) / 2),
            i64::from(rect.y + (rect.h - i32::from(frame.height)) / 2),
        ];
        image.blit_within(
            frame,
            origin,
            [rect.x, rect.y, rect.w, rect.h].map(i64::from),
        );
    }
    /// The move-items counter over an item window, with `item`'s
    /// inventory art and its held button's down art.
    pub(crate) fn draw_item_counter(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        counter: &Counter,
        item: Option<ObjectRef>,
    ) {
        let icon = item.and_then(|item| self.item_icon(item));
        draw_counter(
            painter,
            image,
            counter,
            icon.as_ref().and_then(|icon| icon.frame(0, 0)),
            self.held_button(),
        );
    }
    /// A press on the counter: its button's cues and down art.
    pub(crate) fn press_counter_button(&mut self, point: [i32; 2]) {
        if let Some(rect) = counter_button(point) {
            self.press_button(rect);
        }
    }
    /// A key's counter input: Enter's acceptance plays `ib1p1xx1`.
    pub(crate) fn counter_key_cue(&mut self, input: CounterInput) -> CounterInput {
        if matches!(input, CounterInput::Accept(_)) {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
        }
        input
    }
    /// The hold pulse opened an item menu: its sound.
    pub(crate) fn menu_open_cue(&mut self) {
        self.play_effect(cursor::MENU_OPEN_SOUND.as_bytes(), None);
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
    /// An engine-drawn button at screen `rect`: an amber frame, the label
    /// amber on black, the colors swapped while pressed.
    pub(crate) fn engine_button_at(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        rect: Rect,
        label: &[u8],
    ) {
        let pressed = self.held == Some(rect);
        let (fill, text) = if pressed {
            (ColorKey::AMBER, ColorKey::BLACK)
        } else {
            (ColorKey::BLACK, ColorKey::AMBER)
        };
        let Rect { x, y, w, h } = rect;
        image.fill_rect([x, y, w, h], painter.color(ColorKey::AMBER));
        image.fill_rect([x + 1, y + 1, w - 2, h - 2], painter.color(fill));
        painter.centered(image, 101, text, label, [x, y + (h - 8) / 2], w);
    }
    /// The WIELD button and its label plate at screen rectangles: the held
    /// weapon's name, or "Unarmed", amber on black and clipped to the
    /// plate.
    pub(crate) fn draw_wield(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        [button, label]: [Rect; 2],
        weapon: Option<ObjectRef>,
    ) {
        self.engine_button_at(painter, image, button, b"WIELD");
        let name = weapon
            .and_then(|item| self.host.world.get(item))
            .map_or_else(|| b"Unarmed".to_vec(), |object| object.name.clone());
        let Rect { x, y, w, h } = label;
        image.fill_rect([x, y, w, h], painter.color(ColorKey::BLACK));
        let width = painter.width(101, &name).min(w - 4);
        painter.text(
            image,
            101,
            ColorKey::AMBER,
            &name,
            [x + (w - width) / 2, y + (h - 8) / 2],
            w - 4,
        );
    }
}
