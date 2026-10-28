//! SPEC-014 §3.14-3.16 in game: the save/load slot screen (captions, slot
//! states, previews, the info box, the overwrite question and description
//! box), committing through the SPEC-008 slot writer and reader, and quick
//! save/load. Slots live below the configured save root.
use super::{MapPreview, Screen, dialog::Dialog};
use crate::{AppResult, input::key_text};
use vault_core::{
    host_save::{SaveInfo, slot_directory},
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND, FRAME_MS, red_button,
        saveload::{
            self, ListKey, Mode, Preview, Quick, SaveLoadControl, SaveLoadKey, SaveLoadOutcome,
            SlotInfo, SlotState,
        },
    },
};
use vault_formats::savegame::{HEADER_LEN, SaveHeader};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The open screen: the core screen (SPEC-020 §2.1: its mode, slot
/// states, selection, description box, held arrow and whether the options
/// menu opened it) and what only this layer draws.
pub(crate) struct SaveLoadScreen {
    core: saveload::SaveLoadScreen,
    thumbnails: Vec<Option<Vec<u8>>>,
    /// The current game's capture, for save previews.
    capture: Vec<u8>,
    /// The description cursor's blink phase (on for 3 frames, off for 3).
    blink: bool,
}
impl std::ops::Deref for SaveLoadScreen {
    type Target = saveload::SaveLoadScreen;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
impl std::ops::DerefMut for SaveLoadScreen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl SaveLoadScreen {
    /// The description box is open.
    #[cfg(test)]
    pub fn description_open(&self) -> bool {
        self.description.is_some()
    }
}

/// A key on the screen: the description box takes Enter, Esc, Backspace
/// (or Delete) and its characters; the list takes the brightness and quit
/// keys, the arrows, Home, End, Enter and Esc.
fn saveload_input(
    key: KeyCode,
    [shift, control]: [bool; 2],
    description: bool,
) -> Option<SaveLoadKey> {
    if description {
        return Some(match key {
            KeyCode::Enter | KeyCode::NumpadEnter => SaveLoadKey::Enter,
            KeyCode::Escape => SaveLoadKey::Escape,
            KeyCode::Backspace | KeyCode::Delete => SaveLoadKey::Backspace,
            _ => SaveLoadKey::Char(text_key(key, shift)?),
        });
    }
    Some(match key {
        KeyCode::Equal | KeyCode::NumpadAdd => SaveLoadKey::Brightness(true),
        KeyCode::Minus | KeyCode::NumpadSubtract => SaveLoadKey::Brightness(false),
        KeyCode::KeyQ | KeyCode::KeyX if control => SaveLoadKey::Quit,
        KeyCode::F10 => SaveLoadKey::Quit,
        KeyCode::ArrowUp => SaveLoadKey::List(ListKey::Up),
        KeyCode::ArrowDown => SaveLoadKey::List(ListKey::Down),
        KeyCode::Home => SaveLoadKey::List(ListKey::Home),
        KeyCode::End => SaveLoadKey::List(ListKey::End),
        KeyCode::Enter | KeyCode::NumpadEnter => SaveLoadKey::Enter,
        KeyCode::Escape => SaveLoadKey::Escape,
        _ => return None,
    })
}

/// Read every slot header below `root`, in `game`'s layout (SPEC-018
/// §2.3: slots belong to one game, so the other game's slots do not read).
fn scan(
    root: &std::path::Path,
    game: vault_core::GameProfile,
    fallout2: Option<(usize, bool)>,
) -> (Vec<SlotState>, Vec<Option<Vec<u8>>>) {
    (1..=saveload::SLOTS as u8)
        .map(|slot| {
            let path = slot_directory(root, slot).map(|dir| dir.join("SAVE.DAT"));
            let Ok(bytes) = path.and_then(|path| {
                std::fs::read(path).map_err(|e| vault_core::WorldError::Invalid(e.to_string()))
            }) else {
                return (SlotState::Empty, None);
            };
            let header = &bytes[..bytes.len().min(HEADER_LEN)];
            // SPEC-019 §3.11: under Fallout 2 a full header with a good
            // signature that fails SPEC-018 §7.2's other checks is an old
            // version; a short one or a bad signature stays corrupt.
            let parsed = SaveHeader::parse_for(header, game).and_then(|parsed| {
                if let Some((globals, gated)) = fallout2 {
                    parsed.check_fallout2(globals, gated)?;
                }
                Ok(parsed)
            });
            let signed = header.len() == HEADER_LEN
                && header.starts_with(vault_formats::savegame::SIGNATURE);
            if parsed.is_err() && fallout2.is_some() && signed {
                return (SlotState::OldVersion, None);
            }
            match parsed {
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
    /// Fallout 2's installed campaign count and gate state for SPEC-018
    /// §7.2 checks 4-5, `None` under Fallout 1.
    fn fallout2_slot_check(&self) -> Option<(usize, bool)> {
        self.host.world.fallout2.as_ref()?;
        let globals = self.host.world.gvars.as_ref().map_or(0, Vec::len);
        Some((globals, self.host.scripts_gated()))
    }
    /// Every slot's state and thumbnail.
    fn scan_slots(&self) -> (Vec<SlotState>, Vec<Option<Vec<u8>>>) {
        scan(
            &self.save_root,
            self.save_profile(),
            self.fallout2_slot_check(),
        )
    }
    /// The current view scaled to a 224-by-133 thumbnail.
    pub(crate) fn capture_thumbnail(&mut self) -> Vec<u8> {
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
        let (slots, thumbnails) = self.scan_slots();
        let list = self.slot_list.clone();
        self.screen = Screen::SaveLoad(Box::new(SaveLoadScreen {
            core: saveload::SaveLoadScreen::open(mode, slots, list),
            thumbnails,
            capture,
            blink: true,
        }));
    }
    /// The slot selection: the open screen's, else the one kept for the
    /// next opening (it lasts for the program run).
    pub(crate) fn slot_selection(&self) -> &saveload::SlotList {
        match &self.screen {
            Screen::SaveLoad(screen) => &screen.list,
            _ => &self.slot_list,
        }
    }
    /// Close the screen to `next`, keeping its selection for the next
    /// opening.
    fn leave_save_screen(&mut self, next: Screen) {
        if let Screen::SaveLoad(screen) = std::mem::replace(&mut self.screen, next) {
            self.slot_list = screen.core.into_list();
        }
    }
    pub(crate) fn saveload_image(&mut self) -> AppResult<IndexedImage> {
        let Screen::SaveLoad(screen) = &self.screen else {
            return Err("no save screen".into());
        };
        let mut image = IndexedImage::new(640, 480)?;
        let painter = self.painter();
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
        let selected = screen.list.selected;
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
        let state = screen.selected_state();
        let thumbnail = screen.thumbnails.get(selected).and_then(Option::as_ref);
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
                // SPEC-019 §3.11: under Fallout 2 the saved map's location
                // name (SPEC-018 §5.2), no elevation name; an unknown map
                // shows an empty location.
                let location = match self.host.world.fallout2.as_deref() {
                    Some(state) => {
                        Some(state.location_name(i32::from(info.map as i16)).into_bytes())
                    }
                    None => saveload::location_messages(info.map, info.elevation).map(
                        |(short, elevation)| {
                            let mut location = painter.message("map.msg", short);
                            location.push(b' ');
                            location.extend(painter.message("map.msg", elevation));
                            location
                        },
                    ),
                };
                if let Some(location) = location {
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
            // SPEC-019 §3.11: `lsgame.msg` 116 in the info box.
            SlotState::OldVersion => {
                let text = painter.message("lsgame.msg", 116);
                painter.text(&mut image, 101, ColorKey::ERROR, &text, [404, 262], 160);
            }
        }
        for ((button, caption), id) in [saveload::DONE, saveload::CANCEL]
            .into_iter()
            .zip([104, 105])
        {
            let message = ("lsgame.msg", id);
            let rect = red_button(button);
            self.draw_captioned_red_button(&painter, &mut image, rect, message, caption, 80);
        }
        for (rect, art) in [
            (saveload::ARROW_UP, ("uparwoff.frm", "uparwon.frm")),
            (saveload::ARROW_DOWN, ("dnarwoff.frm", "dnarwon.frm")),
        ] {
            self.draw_button(&painter, &mut image, rect, art);
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
                (saveload::DESCRIPTION_DONE, [ox + 56, oy + 57], 104),
                (saveload::DESCRIPTION_CANCEL, [ox + 181, oy + 57], 105),
            ] {
                let message = ("lsgame.msg", id);
                self.draw_captioned_red_button(&painter, &mut image, rect, message, caption, 80);
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
    pub(super) fn close_save_screen(&mut self) {
        self.leave_save_screen(Screen::Map);
    }
    /// Esc or Cancel: back to the options menu when it opened the screen,
    /// else to the map.
    fn cancel_save_screen(&mut self, to_options: bool) {
        self.leave_save_screen(if to_options {
            Screen::Options(super::options::OptionsScreen::page(
                vault_core::ui::options::OptionsPage::Menu,
            ))
        } else {
            Screen::Map
        });
    }
    /// Act on the core screen's outcome (SPEC-020 §2.1 rule 4).
    fn saveload_outcome(&mut self, outcome: SaveLoadOutcome) {
        match outcome {
            SaveLoadOutcome::None | SaveLoadOutcome::Changed => {}
            SaveLoadOutcome::DescriptionOpened => {}
            // §3.15 step 1: an occupied slot asks first (lsgame.msg 131,
            // a large Yes/No box at (169, 131) in key 32328).
            SaveLoadOutcome::AskOverwrite(_) => {
                let title = self.message("lsgame.msg", 131);
                self.show_dialog(Dialog::overwrite(
                    saveload::OVERWRITE_ORIGIN,
                    title,
                    ColorKey::ERROR,
                ));
            }
            // A corrupt slot: the reference's body is 136 ("older
            // version"); under the safe rule 135. SPEC-019 §3.11 an old
            // version: `iisxxxx1`, title 134 and body 136.
            SaveLoadOutcome::Notice { title, body } => self.show_notice(title, body, false),
            SaveLoadOutcome::Load(slot) => self.load_slot(slot),
            SaveLoadOutcome::Save { slot, description } => self.finish_save(slot, description),
            SaveLoadOutcome::Cancel { to_options } => self.cancel_save_screen(to_options),
            // G-31 / §3.14: brightness, and the quit confirmation (whose
            // Yes closes the screen as cancelled).
            SaveLoadOutcome::Brightness(up) => self.brightness_step(up),
            SaveLoadOutcome::QuitConfirm => self.confirm_quit(),
        }
    }
    /// Load an occupied slot from the screen.
    fn load_slot(&mut self, slot: usize) {
        let Screen::SaveLoad(screen) = &self.screen else {
            return;
        };
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
    /// Yes to the overwrite question: the description box, prefilled with
    /// the slot's description.
    pub(crate) fn overwrite_accepted(&mut self) {
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return;
        };
        if screen.overwrite_accepted() == SaveLoadOutcome::DescriptionOpened {
            screen.blink = true;
        }
    }
    /// Write zero-based `slot` with `description`.
    fn save_slot(&mut self, slot: usize, description: Vec<u8>) -> bool {
        let slot = slot as u8 + 1;
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
    /// The description box was accepted: write the slot and close, or
    /// rescan the slots and stay open after a failure.
    fn finish_save(&mut self, slot: usize, description: Vec<u8>) {
        let Screen::SaveLoad(screen) = &self.screen else {
            return;
        };
        let mode = screen.mode;
        if self.save_slot(slot, description) {
            self.close_save_screen();
            let text = self.message("lsgame.msg", 140);
            self.notify(text);
            if mode == Mode::PickQuickSave {
                self.quick.picked();
                let text = self.message("misc.msg", 5);
                self.notify(text);
            }
        } else {
            let (slots, thumbnails) = self.scan_slots();
            if let Screen::SaveLoad(screen) = &mut self.screen {
                // A failed save rescans the slots and stays open.
                screen.rescan(slots);
                screen.thumbnails = thumbnails;
            }
        }
    }
    /// §3.14: while a scroll arrow is held every key, Esc included, is
    /// discarded.
    pub(crate) fn saveload_key(&mut self, key: KeyCode) -> bool {
        let [shift, control, _] = self.hud.modifiers;
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
        let description = screen.description.is_some();
        let Some(input) = saveload_input(key, [shift, control], description) else {
            return screen.arrow_held().is_some();
        };
        let outcome = screen.key(input);
        self.saveload_outcome(outcome);
        true
    }
    /// A press: the controls act on it. G-21 / §3.15 the description
    /// box's Done and Cancel play the red-button sounds (the reference
    /// leaves them silent, `[May-improve]`); Done plays its press; the
    /// arrows use the red-button sounds and repeat while held (§3.14 "Held
    /// arrows"); a double click on the list commits with the press sound.
    pub(crate) fn saveload_click(&mut self, point: [i32; 2]) -> bool {
        let now = self.clock;
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
        let layout = screen.layout();
        let control = match screen.arrow_held() {
            Some(_) => None,
            None => layout.hit(point),
        };
        let outcome = screen.press(point, now);
        let rect = control.and_then(|control| layout.rect(control));
        match (control, rect) {
            (Some(SaveLoadControl::Done), Some(rect)) => {
                self.held = Some(rect);
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            (Some(SaveLoadControl::List), _) => {
                if outcome != SaveLoadOutcome::Changed {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                }
            }
            (Some(_), Some(rect)) => self.press_button(rect),
            _ => {}
        }
        self.saveload_outcome(outcome);
        true
    }
    /// The left button came up: a held arrow stops.
    pub(crate) fn saveload_release(&mut self) -> bool {
        if let Screen::SaveLoad(screen) = &mut self.screen {
            screen.release();
        }
        false
    }
    /// F6: the pick screen, or a direct save keeping the description.
    /// Under the `enhanced` preset, a direct save to the quick-save slot.
    pub(crate) fn quick_save(&mut self) {
        if self.qol() {
            return self.enhanced_quick_save();
        }
        match self.quick.f6() {
            Quick::Pick(mode) => self.open_save_screen(mode),
            _ => {
                let slot = self.slot_list.selected;
                let (slots, _) = self.scan_slots();
                let description = match &slots[slot] {
                    SlotState::Occupied(info) => info.description.clone(),
                    _ => Vec::new(),
                };
                if self.save_slot(slot, description) {
                    let text = self.message("misc.msg", 5);
                    self.notify(text);
                }
            }
        }
    }
    /// F7: the pick screen, or a direct load of the selected slot.
    /// Under the `enhanced` preset, a direct load of the quick-save slot.
    pub(crate) fn quick_load(&mut self) {
        if self.qol() {
            return self.enhanced_quick_load();
        }
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
        let held = self.held_button();
        let Screen::SaveLoad(screen) = &mut self.screen else {
            return false;
        };
        let blink = (now / (3 * FRAME_MS)).is_multiple_of(2);
        let changed =
            std::mem::replace(&mut screen.blink, blink) != blink && screen.description.is_some();
        let arrow = screen
            .arrow_held()
            .and_then(|arrow| screen.layout().rect(arrow));
        let on_arrow = arrow.is_some_and(|rect| held == Some(rect));
        screen.frame(now, on_arrow) || changed
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

#[cfg(test)]
impl MapPreview {
    /// Every slot's state, as the screen would list it.
    pub(crate) fn slot_states(&self) -> Vec<SlotState> {
        self.scan_slots().0
    }
}
