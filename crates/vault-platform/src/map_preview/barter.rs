//! SPEC-011 §3.17-3.19 barter, opened when a dialogue requests it
//! (`gdialog_barter`): the player's inventory and offer table, and the
//! merchant's offer table and inventory (its armor and held weapon hidden
//! for the session). A left click moves one unit, a right click the whole
//! entry; Offer attempts the trade at the merchant's price, and Done
//! returns every table and the hidden equipment, then resumes dialogue.
use super::{MapPreview, Screen, button, inventory::inside, panel_text};
use crate::AppResult;
use vault_core::{ObjectRef, barter::BarterSession};
use vault_render::IndexedImage;

const COLUMNS: [&[u8]; 4] = [b"Your items", b"Your offer", b"Their offer", b"Their items"];
const LINE: i32 = 16;
const VISIBLE: usize = 22;
const OFFER: [i32; 4] = [400, 440, 100, 20];
const DONE: [i32; 4] = [520, 440, 100, 20];

pub(crate) struct BarterScreen {
    pub session: BarterSession,
}

fn cell(column: usize, index: usize) -> [i32; 4] {
    [
        10 + 158 * column as i32,
        60 + index as i32 * LINE,
        150,
        LINE - 2,
    ]
}

impl MapPreview {
    /// A pending dialogue barter request opens the barter screen with the
    /// dialogue's target as merchant.
    pub(crate) fn check_barter(&mut self) {
        let dialogue = &self.host.world.dialogue;
        if !matches!(self.screen, Screen::Map) || !dialogue.open || !dialogue.pending_barter {
            return;
        }
        match self.host.begin_barter(dialogue.target) {
            Ok(session) => self.screen = Screen::Barter(Box::new(BarterScreen { session })),
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
    pub(crate) fn barter_key(&mut self) -> bool {
        self.end_barter_screen();
        true
    }
    pub(crate) fn barter_click(&mut self, point: [i32; 2], whole: bool) -> bool {
        let Some(owners) = self.barter_owners() else {
            return false;
        };
        if inside(DONE, point) {
            self.press();
            self.end_barter_screen();
            return true;
        }
        if inside(OFFER, point) {
            self.press();
            let Screen::Barter(screen) = &self.screen else {
                return false;
            };
            let session = screen.session.clone();
            if let Err(error) = self.host.attempt_trade(&session) {
                self.status = error.to_string().into_bytes();
            }
            self.drain_effects();
            return true;
        }
        for column in 0..4 {
            let entries = self.barter_entries(owners[column]);
            let Some(index) =
                (0..entries.len().min(VISIBLE)).find(|index| inside(cell(column, *index), point))
            else {
                continue;
            };
            let (item, quantity) = entries[index];
            // Inventories trade with their own side's table.
            let to = owners[[1, 0, 3, 2][column]];
            let amount = if whole { quantity } else { 1 };
            if let Err(error) = self.host.barter_move(owners[column], to, item, amount) {
                self.status = error.to_string().into_bytes();
            }
            return true;
        }
        false
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
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let Some(font) = &self.font else {
            return Ok(image);
        };
        let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
        let white = palette.lookup_rgb5([31, 31, 31]).unwrap_or(1);
        let world = &self.host.world;
        for (column, title) in COLUMNS.iter().enumerate() {
            panel_text(
                &mut image,
                font,
                palette,
                title,
                [cell(column, 0)[0], 40],
                white,
            );
            for (index, (item, quantity)) in self
                .barter_entries(owners[column])
                .iter()
                .take(VISIBLE)
                .enumerate()
            {
                let name = world.get(*item).map_or(&b"?"[..], |object| &object.name);
                let entry = [name, format!(" x{quantity}").as_bytes()].concat();
                let rect = cell(column, index);
                image.draw_text(
                    font,
                    palette,
                    &entry,
                    [rect[0], rect[1]],
                    rect[2] as u32,
                    vault_render::TextStyle {
                        foreground: green,
                        ..Default::default()
                    },
                );
            }
        }
        let summary = format!(
            "Price: {price}   Your offer: {}   Caps: {}",
            world.inventory_value(owners[1]),
            world.caps(world.player)
        );
        panel_text(
            &mut image,
            font,
            palette,
            summary.as_bytes(),
            [10, 420],
            green,
        );
        button(&mut image, font, palette, b"Offer", OFFER);
        button(&mut image, font, palette, b"Done", DONE);
        panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        Ok(image)
    }
}
