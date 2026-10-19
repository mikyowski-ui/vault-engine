//! SPEC-008 §3.8-3.9 Pip-Boy: quest status by location and holodisks from
//! the campaign globals with the localized Pip-Boy messages, and the alarm
//! clock's rest choices. Rest advances one UI step per at least 50 ms so
//! the window stays responsive; Escape cancels it. The SPEC-012 video
//! archive lists the played movies and replays one with flags `0x0B`. The
//! SPEC-014 §3.33 automap page draws the current map's stored automap
//! raster for the player's elevation.
use super::{MapPreview, Screen, button, panel_text};
use crate::AppResult;
use std::sync::Arc;
use vault_core::{
    assets::WorldAssets,
    host_rest::{RestOutcome, RestRequest, RestSession},
    movie,
    ui::automap,
};
use vault_formats::MsgCatalog;
use vault_guidance::{DiskLine, Holodisk, QuestState};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Each rest UI step waits at least 50 wall-clock milliseconds.
const REST_STEP_MS: u64 = 50;
const MENU: [&[u8]; 6] = [
    b"Status",
    b"Holodisks",
    b"Alarm clock",
    b"Video archive",
    b"Automap",
    b"Close",
];
const CONTENT: [i32; 2] = [180, 60];
const LINE: i32 = 12;
/// The visible alarm-menu choices.
const ALARM: [(&[u8], RestRequest); 13] = [
    (b"Rest for 10 minutes", RestRequest::Minutes(10)),
    (b"Rest for 30 minutes", RestRequest::Minutes(30)),
    (b"Rest for 1 hour", RestRequest::Hours(1)),
    (b"Rest for 2 hours", RestRequest::Hours(2)),
    (b"Rest for 3 hours", RestRequest::Hours(3)),
    (b"Rest for 4 hours", RestRequest::Hours(4)),
    (b"Rest for 5 hours", RestRequest::Hours(5)),
    (b"Rest for 6 hours", RestRequest::Hours(6)),
    (b"Rest until 06:00", RestRequest::UntilHour(6)),
    (b"Rest until 12:00", RestRequest::UntilHour(12)),
    (b"Rest until 18:00", RestRequest::UntilHour(18)),
    (b"Rest until 00:00", RestRequest::UntilHour(0)),
    (b"Rest until healed", RestRequest::UntilHealed),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Status,
    Holodisks,
    Alarm,
    Archive,
    Automap,
}
pub(crate) struct PipBoy {
    pub tab: Tab,
    /// The open holodisk and its page.
    pub disk: Option<(Holodisk, usize)>,
    pub rest: Option<RestSession>,
    last_step: u64,
}

fn menu_button(index: usize) -> [i32; 4] {
    [20, 60 + 30 * index as i32, 140, 20]
}
fn row(index: usize) -> [i32; 4] {
    [
        CONTENT[0],
        CONTENT[1] + index as i32 * (LINE + 4),
        440,
        LINE + 2,
    ]
}
const PREVIOUS: [i32; 4] = [180, 440, 100, 16];
const NEXT: [i32; 4] = [300, 440, 100, 16];
const BACK: [i32; 4] = [420, 440, 100, 16];
fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}

impl MapPreview {
    fn pipboy_messages(&self) -> Option<Arc<MsgCatalog>> {
        self.host.assets.game_messages("pipboy.msg").ok()
    }
    fn gvars(&self) -> Vec<i32> {
        self.host.world.gvars.clone().unwrap_or_default()
    }
    /// Opening the Pip-Boy resets the credited rest minutes.
    pub(crate) fn open_pipboy(&mut self) {
        self.host.open_pipboy();
        self.screen = Screen::PipBoy(PipBoy {
            tab: Tab::Status,
            disk: None,
            rest: None,
            last_step: 0,
        });
    }
    pub(crate) fn pipboy_key(&mut self, key: KeyCode) -> bool {
        let Screen::PipBoy(pipboy) = &mut self.screen else {
            return false;
        };
        match key {
            KeyCode::Escape if pipboy.rest.is_some() => {
                pipboy.rest = None;
                self.status = b"Rest cancelled".to_vec();
            }
            KeyCode::Escape if pipboy.disk.is_some() => pipboy.disk = None,
            KeyCode::Escape => self.return_to_map(),
            KeyCode::ArrowRight | KeyCode::PageDown => {
                if let Some((disk, page)) = &mut pipboy.disk {
                    *page = (*page + 1).min(disk.last_page);
                }
            }
            KeyCode::ArrowLeft | KeyCode::PageUp => {
                if let Some((_, page)) = &mut pipboy.disk {
                    *page = page.saturating_sub(1);
                }
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn pipboy_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return false;
        };
        let (tab, resting, disk_open) = (pipboy.tab, pipboy.rest.is_some(), pipboy.disk.is_some());
        if resting {
            return false;
        }
        if let Some(index) = (0..MENU.len()).find(|index| inside(menu_button(*index), point)) {
            self.press();
            if index == 5 {
                self.return_to_map();
            } else if let Screen::PipBoy(pipboy) = &mut self.screen {
                pipboy.tab = [
                    Tab::Status,
                    Tab::Holodisks,
                    Tab::Alarm,
                    Tab::Archive,
                    Tab::Automap,
                ][index];
                pipboy.disk = None;
            }
            return true;
        }
        match tab {
            Tab::Holodisks if disk_open => {
                let Screen::PipBoy(PipBoy {
                    disk: Some((disk, page)),
                    ..
                }) = &mut self.screen
                else {
                    return false;
                };
                if inside(PREVIOUS, point) {
                    *page = page.saturating_sub(1);
                } else if inside(NEXT, point) {
                    *page = (*page + 1).min(disk.last_page);
                } else if inside(BACK, point) {
                    if let Screen::PipBoy(pipboy) = &mut self.screen {
                        pipboy.disk = None;
                    }
                } else {
                    return false;
                }
                self.press();
                true
            }
            Tab::Holodisks => {
                let gvars = self.gvars();
                let count = vault_guidance::listed_disks(&gvars).len();
                let Some(index) = (0..count).find(|index| inside(row(*index), point)) else {
                    return false;
                };
                self.press();
                // Selection maps the row to the n-th positively acquired disk.
                let opened = vault_guidance::selected_disk(&gvars, index).map(|ordinal| {
                    let messages = self.pipboy_messages().ok_or("pipboy.msg is unavailable")?;
                    vault_guidance::holodisk(ordinal, &messages).map_err(|error| error.to_string())
                });
                match opened {
                    Some(Ok(disk)) => {
                        if let Screen::PipBoy(pipboy) = &mut self.screen {
                            pipboy.disk = Some((disk, 0));
                        }
                    }
                    Some(Err(error)) => self.status = error.to_string().into_bytes(),
                    None => {}
                }
                true
            }
            Tab::Alarm => {
                let Some(index) = (0..ALARM.len()).find(|index| inside(row(*index), point)) else {
                    return false;
                };
                self.press();
                match self.host.begin_rest(ALARM[index].1) {
                    Ok(session) => {
                        if session.outcome() == Some(RestOutcome::Refused) {
                            self.status = b"You cannot rest here right now".to_vec();
                        } else if let Screen::PipBoy(pipboy) = &mut self.screen {
                            pipboy.rest = Some(session);
                            self.status = b"Resting".to_vec();
                        }
                    }
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
                true
            }
            Tab::Archive => {
                let count = movie::archive_movies(&self.host.world.persistence.movies).len();
                let Some(index) = (0..count).find(|index| inside(row(*index), point)) else {
                    return false;
                };
                self.press();
                // Row r is selection r + 1 (selections 0 and 1 both pick
                // the first played movie).
                if self.host.play_archive_movie(index as i32 + 1) {
                    self.drain_effects();
                }
                true
            }
            Tab::Status | Tab::Automap => false,
        }
    }
    /// Advance an active rest by one UI step when 50 ms have passed.
    pub(crate) fn pipboy_tick(&mut self, now: u64) -> AppResult<bool> {
        let Screen::PipBoy(pipboy) = &mut self.screen else {
            return Ok(false);
        };
        let Some(session) = &mut pipboy.rest else {
            return Ok(false);
        };
        if now.saturating_sub(pipboy.last_step) < REST_STEP_MS {
            return Ok(false);
        }
        pipboy.last_step = now;
        let outcome = match self.host.advance_rest(session) {
            Ok(outcome) => outcome,
            Err(error) => {
                pipboy.rest = None;
                self.status = error.to_string().into_bytes();
                return Ok(true);
            }
        };
        match outcome {
            None => {}
            Some(RestOutcome::Interrupted(_)) => {
                // A nonzero queue handler marks the Pip-Boy for exit.
                self.return_to_map();
            }
            Some(_) => {
                pipboy.rest = None;
                self.status = b"Rest complete".to_vec();
            }
        }
        Ok(true)
    }
    pub(crate) fn pipboy_image(&self) -> AppResult<IndexedImage> {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return Err("Pip-Boy is not open".into());
        };
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
        let dim = palette.lookup_rgb5([0, 15, 0]).unwrap_or(1);
        let white = palette.lookup_rgb5([31, 31, 31]).unwrap_or(1);
        let Some(font) = &self.font else {
            return Ok(image);
        };
        panel_text(
            &mut image,
            font,
            palette,
            &self.clock_text(),
            [480, 24],
            green,
        );
        for (index, label) in MENU.iter().enumerate() {
            button(&mut image, font, palette, label, menu_button(index));
        }
        let messages = self.pipboy_messages();
        let text = |id: i32| {
            messages
                .as_ref()
                .and_then(|catalog| catalog.get(id))
                .map_or_else(Vec::new, |message| message.text.clone())
        };
        let gvars = self.gvars();
        let mut line = 0;
        let mut put = |image: &mut IndexedImage, text: &[u8], color: u8, struck: bool| {
            let [x, y, ..] = row(line);
            if y < 430 {
                let width = image.draw_text(
                    font,
                    palette,
                    text,
                    [x, y],
                    440,
                    vault_render::TextStyle {
                        foreground: color,
                        ..Default::default()
                    },
                );
                if struck {
                    image.fill_rect([x, y + font.height() as i32 / 2, width as i32, 1], color);
                }
            }
            line += 1;
        };
        if pipboy.rest.is_some() {
            put(&mut image, b"Resting... (Escape cancels)", green, false);
        } else {
            match (&pipboy.tab, &pipboy.disk) {
                (Tab::Status, _) => {
                    for (town, title) in vault_guidance::quest_locations(&gvars) {
                        put(&mut image, &text(title), white, false);
                        for quest in vault_guidance::town_quests(town, &gvars) {
                            let entry = [
                                format!("{}. ", quest.number).as_bytes(),
                                &text(quest.message),
                            ]
                            .concat();
                            match quest.state {
                                QuestState::Active => put(&mut image, &entry, green, false),
                                QuestState::Completed => put(&mut image, &entry, dim, true),
                                QuestState::Hidden => {}
                            }
                        }
                    }
                }
                (Tab::Holodisks, Some((disk, page))) => {
                    put(&mut image, &disk.title, white, false);
                    for record in disk.page(*page) {
                        match record {
                            DiskLine::Text(text) => put(&mut image, text, green, false),
                            DiskLine::Paragraph => put(&mut image, b"", green, false),
                        }
                    }
                    let pages = format!("Page {} of {}", page + 1, disk.last_page + 1);
                    panel_text(&mut image, font, palette, pages.as_bytes(), [540, 444], dim);
                    button(&mut image, font, palette, b"Previous", PREVIOUS);
                    button(&mut image, font, palette, b"Next", NEXT);
                    button(&mut image, font, palette, b"Back", BACK);
                }
                (Tab::Holodisks, None) => {
                    for ordinal in vault_guidance::listed_disks(&gvars) {
                        put(
                            &mut image,
                            &text(vault_guidance::DISK_TITLE_BASE + ordinal as i32),
                            green,
                            false,
                        );
                    }
                }
                (Tab::Alarm, _) => {
                    for (label, _) in ALARM {
                        put(&mut image, label, green, false);
                    }
                }
                (Tab::Automap, _) => self.draw_pipboy_automap(&mut image),
                (Tab::Archive, _) => {
                    let played = movie::archive_movies(&self.host.world.persistence.movies);
                    if played.is_empty() {
                        put(&mut image, b"No movies have been played", dim, false);
                    }
                    for index in played {
                        put(
                            &mut image,
                            movie::GAME_MOVIES[index].file.as_bytes(),
                            green,
                            false,
                        );
                    }
                }
            }
        }
        panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        Ok(image)
    }
    /// §3.33 the stored raster (current up to the last database write),
    /// each cell 2 pixels wide and 1 tall from (238, 105): walls key 992,
    /// scenery key 480. Rows below the window are clipped (safe rule).
    fn draw_pipboy_automap(&self, image: &mut IndexedImage) {
        let world = &self.host.world;
        let elevation = world
            .get(world.player)
            .map_or(world.elevation, |object| object.raw.elevation);
        let raster = usize::try_from(world.map.header.map_number)
            .ok()
            .and_then(|map| {
                world
                    .persistence
                    .automap
                    .raster(map, usize::from(elevation))
                    .ok()
                    .flatten()
            });
        let Some(raster) = raster else {
            return;
        };
        let painter = crate::interface::Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        for row in 0..200 {
            for column in 0..200 {
                let Some(key) =
                    automap::pipboy_color(vault_formats::automap::cell(&raster, row, column))
                else {
                    continue;
                };
                let [x, y] = automap::pipboy_cell(row, column);
                image.fill_rect([x, y, 2, 1], painter.color(key));
            }
        }
    }
}
