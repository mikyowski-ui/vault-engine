//! SPEC-011 inventory screen: the player's entries with quantities, worn
//! and held markers, weights, and caps; the selected entry can be used
//! (drugs consume through §3.6), wielded or worn (§3.10), reloaded from
//! the inventory, or unloaded (§3.15). Opening it in combat costs the
//! §3.11 action points.
use super::{MapPreview, Screen, button, panel_text};
use crate::AppResult;
use vault_core::{ObjectAction, ObjectRef};
use vault_formats::pro::{BasicFlags, ItemData, PrototypeData};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const ROWS: [i32; 2] = [20, 60];
const LINE: i32 = 16;
const VISIBLE: usize = 22;
const ACTIONS: [&[u8]; 5] = [b"Use", b"Wield", b"Reload", b"Unload", b"Close"];

pub(crate) struct InventoryScreen {
    pub selected: Option<usize>,
}

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
        if self.host.open_inventory() {
            self.screen = Screen::Inventory(InventoryScreen { selected: None });
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
    pub(crate) fn inventory_key(&mut self, key: KeyCode) -> bool {
        let count = self.player_entries().len();
        let Screen::Inventory(screen) = &mut self.screen else {
            return false;
        };
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
        let count = self.player_entries().len().min(VISIBLE);
        if let Some(index) = (0..count).find(|index| inside(row(*index), point)) {
            if let Screen::Inventory(screen) = &mut self.screen {
                screen.selected = Some(index);
            }
            return true;
        }
        let Some(action) = (0..ACTIONS.len()).find(|index| inside(action_button(*index), point))
        else {
            return false;
        };
        self.press();
        if action == 4 {
            self.return_to_map();
            return true;
        }
        let Some(item) = self.selected_item() else {
            self.status = b"Select an item first".to_vec();
            return true;
        };
        let player = self.host.world.player;
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
        let count = self.player_entries().len();
        if let Screen::Inventory(screen) = &mut self.screen
            && screen.selected.is_some_and(|index| index >= count)
        {
            screen.selected = count.checked_sub(1);
        }
        true
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
        Ok(image)
    }
}
