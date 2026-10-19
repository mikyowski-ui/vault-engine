//! SPEC-014 §3.14-3.16 in game: the save/load slot screen (captions, slot
//! states, previews, the info box, the overwrite question and description
//! box), committing through the SPEC-008 slot writer and reader, and quick
//! save/load. Slots live below the configured save root.
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::{
    host_save::{SaveInfo, slot_directory},
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND, flow,
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
    /// The overwrite question for an occupied slot.
    confirm: bool,
    description: Option<Description>,
}

/// SPEC-014 §3.13 a large Done box at (169, 116) for a save failure (132 /
/// 133) or a load failure (134 / 135). Dismissing a load failure closes the
/// screen as cancelled and requests the main menu.
pub(crate) struct Notice {
    title: Vec<u8>,
    body: Vec<u8>,
    quit: bool,
}

const NOTICE_ORIGIN: [i32; 2] = [169, 116];

/// Read every slot header below `root`.
fn scan(root: &std::path::Path) -> (Vec<SlotState>, Vec<Option<Vec<u8>>>) {
    (1..=saveload::SLOTS as u8)
        .map(|slot| {
            let path = slot_directory(root, slot).map(|dir| dir.join("SAVE.DAT"));
            let Ok(bytes) = path.and_then(|path| {
                std::fs::read(path).map_err(|e| vault_core::WorldError::Invalid(e.to_string()))
            }) else {
                return (SlotState::Empty, None);
            };
            match SaveHeader::parse(&bytes[..bytes.len().min(HEADER_LEN)]) {
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

fn text_key(key: KeyCode, shift: bool) -> Option<u8> {
    let name = format!("{key:?}");
    if let Some(letter) = name.strip_prefix("Key") {
        let letter = letter.bytes().next()?;
        return Some(if shift {
            letter
        } else {
            letter.to_ascii_lowercase()
        });
    }
    if let Some(digit) = name.strip_prefix("Digit") {
        return digit.bytes().next();
    }
    match key {
        KeyCode::Space => Some(b' '),
        KeyCode::Period => Some(b'.'),
        KeyCode::Minus => Some(b'-'),
        _ => None,
    }
}

impl MapPreview {
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
        let (slots, thumbnails) = scan(&self.save_root);
        self.screen = Screen::SaveLoad(Box::new(SaveLoadScreen {
            mode,
            slots,
            thumbnails,
            capture,
            confirm: false,
            description: None,
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
                let [l1, l2, l3] = saveload::INFO_LINES;
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
                    painter.text(&mut image, 101, ColorKey::GREEN, &location, l3, 164);
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
            painter.blit(&mut image, "lilredup.frm", 0, button);
            let text = painter.message("lsgame.msg", id);
            painter.text(&mut image, 103, ColorKey::LABEL, &text, caption, 80);
        }
        painter.blit(
            &mut image,
            "uparwoff.frm",
            0,
            [saveload::ARROW_UP.x, saveload::ARROW_UP.y],
        );
        painter.blit(
            &mut image,
            "dnarwoff.frm",
            0,
            [saveload::ARROW_DOWN.x, saveload::ARROW_DOWN.y],
        );
        if screen.confirm {
            let [ox, oy] = [169, 131];
            painter.blit_opaque(&mut image, flow::DialogSize::Large.art().0, 0, [ox, oy]);
            let question = painter.message("lsgame.msg", 131);
            painter.centered(
                &mut image,
                101,
                ColorKey::GREEN,
                &question,
                [ox, oy + flow::dialog_title_y(0)],
                302,
            );
        }
        if let Some(description) = &screen.description {
            let (art, [ox, oy]) = saveload::DESCRIPTION_BOX;
            painter.blit_opaque(&mut image, art, 0, [ox, oy]);
            let title = painter.message("lsgame.msg", 130);
            painter.centered(&mut image, 103, ColorKey::LABEL, &title, [ox, oy + 7], 290);
            let mut text = description.text.clone();
            text.push(b' ');
            painter.text(
                &mut image,
                101,
                ColorKey::GREEN,
                &text,
                [ox + 24, oy + 35],
                250,
            );
        }
        Ok(image)
    }
    fn close_save_screen(&mut self) {
        self.screen = Screen::Map;
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
                SlotState::Occupied(_) if !screen.confirm => screen.confirm = true,
                SlotState::Occupied(info) => {
                    screen.confirm = false;
                    screen.description = Some(Description::open(&info.description));
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
        } else if let Screen::SaveLoad(screen) = &mut self.screen {
            // A failed save rescans the slots and stays open.
            (screen.slots, screen.thumbnails) = scan(&self.save_root);
        }
    }
    pub(crate) fn saveload_key(&mut self, key: KeyCode) -> bool {
        let shift = self.hud.modifiers[0];
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
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
        if screen.confirm {
            match key {
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyY => self.commit_slot(),
                KeyCode::Escape | KeyCode::KeyN => screen.confirm = false,
                _ => return false,
            }
            return true;
        }
        match key {
            KeyCode::ArrowUp => self.slot_list.key(ListKey::Up),
            KeyCode::ArrowDown => self.slot_list.key(ListKey::Down),
            KeyCode::Home => self.slot_list.key(ListKey::Home),
            KeyCode::End => self.slot_list.key(ListKey::End),
            KeyCode::Enter | KeyCode::NumpadEnter => self.commit_slot(),
            KeyCode::Escape => self.close_save_screen(),
            _ => return false,
        }
        true
    }
    pub(crate) fn saveload_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::SaveLoad(screen) = &self.screen else {
            return false;
        };
        if screen.description.is_some() || screen.confirm {
            return true;
        }
        let hit = |origin: [i32; 2]| {
            vault_core::ui::Rect::new(origin[0], origin[1], 15, 16).contains(point)
        };
        if hit(saveload::DONE.0) {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            self.commit_slot();
        } else if hit(saveload::CANCEL.0) {
            self.press();
            self.close_save_screen();
        } else if saveload::ARROW_UP.contains(point) {
            self.slot_list.key(ListKey::Up);
        } else if saveload::ARROW_DOWN.contains(point) {
            self.slot_list.key(ListKey::Down);
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
                let (slots, _) = scan(&self.save_root);
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
    /// One modal frame: the double-click window runs down.
    pub(crate) fn saveload_frame(&mut self) {
        self.slot_list.frame();
    }
}
impl MapPreview {
    /// `iisxxxx1` and the `lsgame.msg` box.
    fn show_notice(&mut self, title: i32, body: i32, quit: bool) {
        self.play_effect(ERROR_SOUND.as_bytes(), None);
        let title = self.message("lsgame.msg", title);
        let body = self.message("lsgame.msg", body);
        self.notice = Some(Notice { title, body, quit });
    }
    fn dismiss_notice(&mut self) {
        if let Some(notice) = self.notice.take()
            && notice.quit
        {
            self.screen = Screen::Map;
            self.quit_requested = true;
        }
    }
    pub(crate) fn draw_notice(&self, image: &mut IndexedImage) {
        let Some(notice) = &self.notice else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [ox, oy] = NOTICE_ORIGIN;
        let (art, [width, _]) = flow::DialogSize::Large.art();
        painter.blit_opaque(image, art, 0, [ox, oy]);
        let body = wrap(&notice.body, |line| painter.width(101, line));
        let mut y = oy + flow::dialog_title_y(body.len());
        for line in std::iter::once(&notice.title).chain(&body) {
            painter.centered(image, 101, ColorKey::ERROR, line, [ox, y], width);
            y += 10;
        }
        for button in flow::dialog_buttons(false) {
            let [bx, by] = button.box_origin;
            painter.blit_opaque(image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
            let caption = painter.message("dbox.msg", button.caption);
            let [cx, cy] = button.caption_origin;
            painter.text(
                image,
                103,
                ColorKey::LABEL,
                &caption,
                [ox + cx, oy + cy],
                60,
            );
            let [rx, ry] = button.button;
            painter.blit(image, "lilredup.frm", 0, [ox + rx, oy + ry]);
        }
    }
    /// Enter (with `ib1p1xx1`) or Esc dismisses; other keys are swallowed.
    pub(crate) fn notice_key(&mut self, key: KeyCode) -> bool {
        if self.notice.is_none() {
            return false;
        }
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.dismiss_notice();
            }
            KeyCode::Escape => self.dismiss_notice(),
            _ => {}
        }
        true
    }
    pub(crate) fn notice_click(&mut self, point: [i32; 2]) -> bool {
        if self.notice.is_none() {
            return false;
        }
        let [ox, oy] = NOTICE_ORIGIN;
        let [rx, ry] = flow::dialog_buttons(false)[0].button;
        if vault_core::ui::Rect::new(ox + rx, oy + ry, 15, 16).contains(point) {
            self.press();
            self.dismiss_notice();
        }
        true
    }
}

/// Body lines wider than `flow::DIALOG_WRAP` wrap at spaces.
fn wrap(text: &[u8], width: impl Fn(&[u8]) -> i32) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for word in text.split(|b| *b == b' ').filter(|w| !w.is_empty()) {
        match lines.last_mut() {
            Some(line) if width(&[line.as_slice(), b" ", word].concat()) <= flow::DIALOG_WRAP => {
                line.push(b' ');
                line.extend_from_slice(word);
            }
            _ => lines.push(word.to_vec()),
        }
    }
    lines
}
