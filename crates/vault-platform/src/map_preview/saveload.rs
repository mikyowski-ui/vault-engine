//! SPEC-014 §3.14-3.16 in game: the save/load slot screen (captions, slot
//! states, previews, the info box, the overwrite question and description
//! box), committing through the SPEC-008 slot writer and reader, and quick
//! save/load. Slots live below the configured save root.
use super::{MapPreview, Screen, dialog::Dialog};
use crate::{AppResult, input::key_text, interface::Painter};
use vault_core::{
    host_save::{SaveInfo, slot_directory},
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND, FRAME_MS, HoldRepeat, Rect,
        saveload::{self, Click, Description, ListKey, Mode, Preview, Quick, SlotInfo, SlotState},
    },
};
use vault_formats::savegame::{HEADER_LEN, SaveHeader};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct SaveLoadScreen {
    pub mode: Mode,
    slots: Vec<SlotState>,
    thumbnails: Vec<Option<Vec<u8>>>,
    /// The current game's capture, for save previews.
    capture: Vec<u8>,
    description: Option<Description>,
    /// §3.14 a held scroll arrow: its direction, button and repeat. While
    /// it is held every other input is discarded.
    arrow: Option<(ListKey, Rect, HoldRepeat)>,
    /// The description cursor's blink phase (on for 3 frames, off for 3).
    blink: bool,
    /// SPEC-014 §3.17: opened from the options menu, so a cancel returns
    /// there (a save or load that happened closes options too).
    pub from_options: bool,
}

impl SaveLoadScreen {
    /// The description box is open.
    #[cfg(test)]
    pub fn description_open(&self) -> bool {
        self.description.is_some()
    }
}

/// §3.15 the description box's Done and Cancel red buttons (screen).
const DESCRIPTION_DONE: Rect = Rect::new(169 + 34, 116 + 58, 15, 16);
const DESCRIPTION_CANCEL: Rect = Rect::new(169 + 160, 116 + 58, 15, 16);

/// Read every slot header below `root`, in `game`'s layout (SPEC-018
/// §2.3: slots belong to one game, so the other game's slots do not read).
fn scan(
    root: &std::path::Path,
    game: vault_core::GameProfile,
) -> (Vec<SlotState>, Vec<Option<Vec<u8>>>) {
    (1..=saveload::SLOTS as u8)
        .map(|slot| {
            let path = slot_directory(root, slot).map(|dir| dir.join("SAVE.DAT"));
            let Ok(bytes) = path.and_then(|path| {
                std::fs::read(path).map_err(|e| vault_core::WorldError::Invalid(e.to_string()))
            }) else {
                return (SlotState::Empty, None);
            };
            match SaveHeader::parse_for(&bytes[..bytes.len().min(HEADER_LEN)], game) {
                Ok(header) => (
                    SlotState::Occupied(SlotInfo {
                        name: header.name_text().to_vec(),
                        description: header.description_text().to_vec(),
                        game_date: header.game_date,
                        game_ticks: header.game_ticks,
                        map: header.map,
                        elevation: header.elevation,
                    }),
                    Some(header.thumbnail.clone()),
                ),
                Err(_) => (SlotState::Corrupt, None),
            }
        })
        .unzip()
}

/// A description character: a letter (upper case with Shift), a number-row
/// digit, Space, `.` or `-`; Shift changes no other key.
fn text_key(key: KeyCode, shift: bool) -> Option<u8> {
    let plain = key_text(key, false).filter(|c| c.is_ascii_alphanumeric() || b" .-".contains(c))?;
    Some(if shift {
        plain.to_ascii_uppercase()
    } else {
        plain
    })
}

impl MapPreview {
    /// The save layout this world's slots use: Fallout 2's while a Fallout
    /// 2 game runs (SPEC-018 §7).
    fn save_profile(&self) -> vault_core::GameProfile {
        if self.host.world.fallout2.is_some() {
            vault_core::GameProfile::Fallout2
        } else {
            vault_core::GameProfile::Fallout1
        }
    }
    /// The current view scaled to a 224-by-133 thumbnail.
    fn capture_thumbnail(&mut self) -> Vec<u8> {
        match self.map_view_image() {
            Ok(view) => saveload::thumbnail(view.pixels(), view.width() as usize),
            Err(_) => vec![0; saveload::THUMBNAIL[0] * saveload::THUMBNAIL[1]],
        }
    }
    pub(crate) fn open_save_screen(&mut self, mode: Mode) {
        if !matches!(self.screen, Screen::Map) {
            return;
        }
        if matches!(mode, Mode::Save | Mode::LoadInGame) {
            self.quick.forget();
        }
        let capture = self.capture_thumbnail();
        let (slots, thumbnails) = scan(&self.save_root, self.save_profile());
        self.screen = Screen::SaveLoad(Box::new(SaveLoadScreen {
            mode,
            slots,
            thumbnails,
            capture,
            description: None,
            arrow: None,
            blink: true,
            from_options: false,
        }));
    }
    pub(crate) fn saveload_image(&mut self) -> AppResult<IndexedImage> {
        let Screen::SaveLoad(screen) = &self.screen else {
            return Err("no save screen".into());
        };
        let mut image = IndexedImage::new(640, 480)?;
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        painter.blit_opaque(&mut image, saveload::BACKGROUND, 0, [0, 0]);
        let title = painter.message("lsgame.msg", screen.mode.title());
        painter.text(
            &mut image,
            103,
            ColorKey::LABEL,
            &title,
            saveload::TITLE_ORIGIN,
            300,
        );
        let caption = painter.message("lsgame.msg", screen.mode.caption());
        let selected = self.slot_list.selected;
        for (slot, state) in screen.slots.iter().enumerate() {
            let color = saveload::caption_color(slot == selected);
            painter.text(
                &mut image,
                101,
                color,
                &saveload::slot_caption(&caption, slot),
                saveload::slot_origin(slot),
                230,
            );
            let (line, error) =
                saveload::second_line(state, |id| painter.message("lsgame.msg", id));
            let color = if error { ColorKey::ERROR } else { color };
            painter.text(
                &mut image,
                101,
                color,
                &line,
                saveload::second_line_origin(slot),
                230,
            );
        }
        let state = &screen.slots[selected];
        let thumbnail = screen.thumbnails[selected].as_ref();
        let preview = saveload::preview(
            screen.mode,
            state,
            thumbnail.is_some_and(|t| t.len() == screen.capture.len()),
        );
        let pixels = match preview {
            Preview::Stored => thumbnail.cloned(),
            Preview::Current => Some(screen.capture.clone()),
            Preview::Cover => None,
        };
        match pixels.and_then(|pixels| IndexedImage::from_pixels(224, 133, pixels).ok()) {
            Some(picture) => image.copy_image(&picture, saveload::PREVIEW_ORIGIN),
            None => {
                painter.blit_opaque(&mut image, saveload::COVER.0, 0, saveload::COVER.1);
            }
        }
        match state {
            SlotState::Occupied(info) => {
                let [l1, l2, [x3, y3]] = saveload::INFO_LINES;
                painter.text(&mut image, 101, ColorKey::GREEN, &info.name, l1, 164);
                let month = painter.message("lsgame.msg", 116 + i32::from(info.game_date[0]));
                painter.text(
                    &mut image,
                    101,
                    ColorKey::GREEN,
                    &saveload::date_line(info, &month),
                    l2,
                    164,
                );
                if let Some((short, elevation)) =
                    saveload::location_messages(info.map, info.elevation)
                {
                    let mut location = painter.message("map.msg", short);
                    location.push(b' ');
                    location.extend(painter.message("map.msg", elevation));
                    // G-31 / §3.14: wrapped to 164 pixels, up to three lines
                    // 12 apart (the `[May-improve]` rule for the reference,
                    // which shows only the first).
                    let lines =
                        saveload::location_lines(&location, |text| painter.width(101, text));
                    for (index, line) in lines.iter().enumerate() {
                        let y = y3 + saveload::LOCATION_STEP * index as i32;
                        painter.text(
                            &mut image,
                            101,
                            ColorKey::GREEN,
                            line,
                            [x3, y],
                            saveload::LOCATION_WIDTH,
                        );
                    }
                }
            }
            SlotState::Empty => {
                let text = painter.message("lsgame.msg", 114);
                painter.text(&mut image, 101, ColorKey::GREEN, &text, [404, 262], 160);
            }
            SlotState::Corrupt => {
                let text = painter.message("lsgame.msg", 115);
                painter.text(&mut image, 101, ColorKey::ERROR, &text, [404, 262], 160);
            }
        }
        for ((button, caption), id) in [saveload::DONE, saveload::CANCEL]
            .into_iter()
            .zip([104, 105])
        {
            let rect = vault_core::ui::Rect::new(button[0], button[1], 15, 16);
            let art = self.button_art(rect, "lilredup.frm", "lilreddn.frm");
            painter.blit(&mut image, art, 0, button);
            let text = painter.message("lsgame.msg", id);
            painter.text(&mut image, 103, ColorKey::LABEL, &text, caption, 80);
        }
        for (rect, up, down) in [
            (saveload::ARROW_UP, "uparwoff.frm", "uparwon.frm"),
            (saveload::ARROW_DOWN, "dnarwoff.frm", "dnarwon.frm"),
        ] {
            painter.blit(
                &mut image,
                self.button_art(rect, up, down),
                0,
                [rect.x, rect.y],
            );
        }
        if let Some(description) = &screen.description {
            // §3.15 the description box: DONE (104) and CANCEL (105)
            // captions, DESCRIPTION (130) centered at y = 7, the red buttons,
            // and the field with one trailing space and a 4-by-8 block
            // cursor after it that blinks every 3 frames.
            let (art, [ox, oy]) = saveload::DESCRIPTION_BOX;
            painter.blit_opaque(&mut image, art, 0, [ox, oy]);
            let title = painter.message("lsgame.msg", 130);
            painter.centered(&mut image, 103, ColorKey::LABEL, &title, [ox, oy + 7], 290);
            for (rect, caption, id) in [
                (DESCRIPTION_DONE, [ox + 56, oy + 57], 104),
                (DESCRIPTION_CANCEL, [ox + 181, oy + 57], 105),
            ] {
                let art = self.button_art(rect, "lilredup.frm", "lilreddn.frm");
                painter.blit(&mut image, art, 0, [rect.x, rect.y]);
                let text = painter.message("lsgame.msg", id);
                painter.text(&mut image, 103, ColorKey::LABEL, &text, caption, 80);
            }
            let mut text = description.text.clone();
            text.push(b' ');
            let width = painter.text(
                &mut image,
                101,
                ColorKey::GREEN,
                &text,
                [ox + 24, oy + 35],
                250,
            );
            if screen.blink {
                image.fill_rect(
                    [ox + 24 + width, oy + 35, 4, 8],
                    painter.color(ColorKey::GREEN),
                );
            }
        }
        Ok(image)
    }
    fn close_save_screen(&mut self) {
        self.screen = Screen::Map;
    }
    /// Esc or Cancel: back to the options menu when it opened the screen,
    /// else to the map.
    fn cancel_save_screen(&mut self) {
        let from_options = matches!(&self.screen, Screen::SaveLoad(screen) if screen.from_options);
        self.screen = if from_options {
            Screen::Options(super::options::OptionsScreen::Menu)
        } else {
            Screen::Map
        };
    }
    /// Done, Enter or a double click on the selected slot.
    fn commit_slot(&mut self) {
        let slot = self.slot_list.selected;
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return;
        };
        let state = screen.slots[slot].clone();
        if screen.mode.saving() {
            match state {
                // §3.15 step 1: an occupied slot asks first (lsgame.msg 131,
                // a large Yes/No box at (169, 131) in key 32328).
                SlotState::Occupied(_) => {
                    let title = self.message("lsgame.msg", 131);
                    self.show_dialog(Dialog::overwrite([169, 131], title, ColorKey::ERROR));
                }
                _ => screen.description = Some(Description::open(b"")),
            }
            return;
        }
        match state {
            SlotState::Empty => {}
            // The reference's body is 136 ("older version"); under the safe
            // rule a corrupt slot uses 135.
            SlotState::Corrupt => self.show_notice(134, 135, false),
            SlotState::Occupied(_) => {
                let mode = screen.mode;
                match self.host.load_game(&self.save_root, slot as u8 + 1) {
                    Ok(()) => {
                        self.close_save_screen();
                        self.return_to_map();
                        self.settle_panel();
                        self.loaded = true;
                        let text = self.message("lsgame.msg", 141);
                        self.notify(text);
                        if mode == Mode::PickQuickLoad {
                            self.quick.picked();
                            let text = self.message("misc.msg", 4);
                            self.notify(text);
                        }
                    }
                    Err(error) => {
                        self.status = error.to_string().into_bytes();
                        self.show_notice(134, 135, true);
                    }
                }
            }
        }
    }
    /// Yes to the overwrite question: the description box, prefilled with
    /// the slot's description.
    pub(crate) fn overwrite_accepted(&mut self) {
        let slot = self.slot_list.selected;
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return;
        };
        if let SlotState::Occupied(info) = &screen.slots[slot] {
            screen.description = Some(Description::open(&info.description));
            screen.blink = true;
        }
    }
    /// Write the selected slot with `description`.
    fn save_slot(&mut self, description: Vec<u8>) -> bool {
        let slot = self.slot_list.selected as u8 + 1;
        let thumbnail = match &self.screen {
            Screen::SaveLoad(screen) => screen.capture.clone(),
            _ => self.capture_thumbnail(),
        };
        let info = SaveInfo {
            description,
            host_date: [0; 3],
            host_time: 0,
            thumbnail: Some(thumbnail),
        };
        match self.host.save_game(&self.save_root, slot, &info) {
            Ok(()) => true,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                self.show_notice(132, 133, false);
                false
            }
        }
    }
    fn finish_description(&mut self, accept: bool) {
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return;
        };
        let Some(description) = screen.description.take() else {
            return;
        };
        if !accept {
            return;
        }
        let mode = screen.mode;
        if self.save_slot(description.text) {
            self.close_save_screen();
            let text = self.message("lsgame.msg", 140);
            self.notify(text);
            if mode == Mode::PickQuickSave {
                self.quick.picked();
                let text = self.message("misc.msg", 5);
                self.notify(text);
            }
        } else {
            let profile = self.save_profile();
            if let Screen::SaveLoad(screen) = &mut self.screen {
                // A failed save rescans the slots and stays open.
                (screen.slots, screen.thumbnails) = scan(&self.save_root, profile);
            }
        }
    }
    pub(crate) fn saveload_key(&mut self, key: KeyCode) -> bool {
        let [shift, control, _] = self.hud.modifiers;
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
        // §3.14: while a scroll arrow is held all other input, Esc
        // included, is discarded.
        if screen.arrow.is_some() {
            return true;
        }
        if let Some(description) = &mut screen.description {
            match key {
                KeyCode::Enter | KeyCode::NumpadEnter => self.finish_description(true),
                KeyCode::Escape => self.finish_description(false),
                KeyCode::Backspace | KeyCode::Delete => description.backspace(),
                _ => match text_key(key, shift) {
                    Some(code) => description.key(code),
                    None => return false,
                },
            }
            return true;
        }
        match key {
            // G-31 / §3.14: brightness, and the quit confirmation (whose Yes
            // closes the screen as cancelled).
            KeyCode::Equal | KeyCode::NumpadAdd => self.brightness_step(true),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.brightness_step(false),
            KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
            KeyCode::F10 => self.confirm_quit(),
            KeyCode::ArrowUp => self.slot_list.key(ListKey::Up),
            KeyCode::ArrowDown => self.slot_list.key(ListKey::Down),
            KeyCode::Home => self.slot_list.key(ListKey::Home),
            KeyCode::End => self.slot_list.key(ListKey::End),
            KeyCode::Enter | KeyCode::NumpadEnter => self.commit_slot(),
            KeyCode::Escape => self.cancel_save_screen(),
            _ => return false,
        }
        true
    }
    pub(crate) fn saveload_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::SaveLoad(screen) = &self.screen else {
            return false;
        };
        if screen.arrow.is_some() {
            return true;
        }
        // G-21 / §3.15 the description box's Done and Cancel. The reference
        // leaves them silent; `[May-improve]`: the red-button sounds.
        if screen.description.is_some() {
            if DESCRIPTION_DONE.contains(point) {
                self.press_button(DESCRIPTION_DONE);
                self.finish_description(true);
            } else if DESCRIPTION_CANCEL.contains(point) {
                self.press_button(DESCRIPTION_CANCEL);
                self.finish_description(false);
            }
            return true;
        }
        let red = |origin: [i32; 2]| Rect::new(origin[0], origin[1], 15, 16);
        if red(saveload::DONE.0).contains(point) {
            self.held = Some(red(saveload::DONE.0));
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            self.commit_slot();
        } else if red(saveload::CANCEL.0).contains(point) {
            self.press_button(red(saveload::CANCEL.0));
            self.cancel_save_screen();
        } else if let Some((rect, key)) = [
            (saveload::ARROW_UP, ListKey::Up),
            (saveload::ARROW_DOWN, ListKey::Down),
        ]
        .into_iter()
        .find(|(rect, _)| rect.contains(point))
        {
            // The arrows use the red-button sounds; a held arrow repeats
            // (§3.14 "Held arrows").
            self.press_button(rect);
            self.slot_list.key(key);
            let repeat = HoldRepeat::new(self.clock, saveload::ARROW_IDLE_FRAMES);
            if let Screen::SaveLoad(screen) = &mut self.screen {
                screen.arrow = Some((key, rect, repeat));
            }
        } else if saveload::LIST_AREA.contains(point)
            && let Click::Done(_) = self.slot_list.click(point[1])
        {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            self.commit_slot();
        }
        true
    }
    /// F6: the pick screen, or a direct save keeping the description.
    pub(crate) fn quick_save(&mut self) {
        match self.quick.f6() {
            Quick::Pick(mode) => self.open_save_screen(mode),
            _ => {
                let slot = self.slot_list.selected;
                let (slots, _) = scan(&self.save_root, self.save_profile());
                let description = match &slots[slot] {
                    SlotState::Occupied(info) => info.description.clone(),
                    _ => Vec::new(),
                };
                if self.save_slot(description) {
                    let text = self.message("misc.msg", 5);
                    self.notify(text);
                }
            }
        }
    }
    /// F7: the pick screen, or a direct load of the selected slot.
    pub(crate) fn quick_load(&mut self) {
        match self.quick.f7() {
            Quick::Pick(mode) => self.open_save_screen(mode),
            _ => match self
                .host
                .load_game(&self.save_root, self.slot_list.selected as u8 + 1)
            {
                Ok(()) => {
                    self.return_to_map();
                    self.settle_panel();
                    let text = self.message("misc.msg", 4);
                    self.notify(text);
                }
                Err(error) => {
                    self.status = error.to_string().into_bytes();
                    self.show_notice(134, 135, true);
                }
            },
        }
    }
    /// One modal frame: the double-click window runs down, a held arrow
    /// repeats until its release or until the pointer leaves it, and the
    /// description cursor blinks. Returns whether the screen changed.
    pub(crate) fn saveload_frame(&mut self, now: u64) -> bool {
        self.slot_list.frame();
        let held = self.held_button();
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
        let blink = (now / (3 * FRAME_MS)).is_multiple_of(2);
        let mut changed =
            std::mem::replace(&mut screen.blink, blink) != blink && screen.description.is_some();
        if let Some((key, rect, repeat)) = &mut screen.arrow {
            if held != Some(*rect) {
                screen.arrow = None;
            } else if repeat.poll(now) {
                let key = *key;
                self.slot_list.key(key);
                changed = true;
            }
        }
        changed
    }
}
impl MapPreview {
    /// `iisxxxx1` and the `lsgame.msg` box: a large Done box at (169, 116)
    /// with the title and body in key 32328 (SPEC-014 §3.13). Dismissing a
    /// load failure closes the screen as cancelled and requests the main
    /// menu.
    fn show_notice(&mut self, title: i32, body: i32, quit: bool) {
        self.play_effect(ERROR_SOUND.as_bytes(), None);
        let title = self.message("lsgame.msg", title);
        let body = self.message("lsgame.msg", body);
        let dialog = Dialog::done([169, 116], title, body, ColorKey::ERROR);
        self.show_dialog(if quit { dialog.quitting() } else { dialog });
    }
}
