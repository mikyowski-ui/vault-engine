//! SPEC-014 §3.41 the Pip-Boy 2000: a full-screen modal window on
//! `pip.frm` with the date, time and vault-water note, four page buttons
//! and the Alarm Clock button, and the 374-by-410 screen area showing the
//! start screen, the Status page (quests by location and the holodisks,
//! SPEC-008 §3.8, through vault-guidance), the Automaps page (areas from
//! the automap database and the §3.33 raster), the Video Archives (SPEC-012
//! §3.12) or the Alarm Clock (SPEC-008 §3.9 rest, one UI step per at least
//! 50 ms), and after two idle minutes the screen saver.
//!
//! Opening pauses the map (the window is modal) and suspends palette
//! cycling; closing runs the map-update scripts. Controls act on release
//! over the control pressed. An accepted row shows yellow for 200 ms before
//! it acts; input in that time and during a rest (Esc stops it) is
//! discarded. The `[May-improve]` safe rules apply: a failed opening leaves
//! nothing paused, a non-positive water count hides the note, a page's
//! zones (its Back zone too) go with the page, a left click below the last
//! location and the page keys outside a holodisk are ignored, the clicked
//! holodisk and area rows are the ones highlighted (the area list's
//! redraw is skipped), the elevation view lists each related map's own
//! elevations, a refused rest keeps the previous page current, page keys
//! wait for a page button in each opening, and raster cells outside the
//! screen area go with the page.
use super::{MapPreview, Screen, dialog::Dialog};
use crate::{AppResult, interface::Painter};
use std::sync::Arc;
use vault_core::{
    assets::WorldAssets,
    host_rest::{RestOutcome, RestSession},
    movie,
    rng::RandomSource,
    ui::{
        BUTTON_PRESS, ColorKey, ERROR_SOUND, automap,
        pipboy::{
            self, ALARM_OPTIONS, Align, Button, Deferred, Highlight, PageCounts, PipBoyKey,
            PipBoyModal, PipBoyOutcome, msg,
        },
        saveload::MAP_SHORT_NAMES,
    },
};
use vault_formats::{Font, MsgCatalog};
use vault_guidance::{DiskLine, Holodisk, QuestState, automaps};
use vault_render::{IndexedImage, TextStyle};
use winit::keyboard::KeyCode;

pub(crate) use vault_core::ui::pipboy::{AutomapView, Page, StatusView, Tab};

/// A running rest: its host session and when it began (the watch cursor
/// counts from there). Its Alarm Clock option is the core modal's.
pub(crate) struct Resting {
    pub session: RestSession,
    since: u64,
}

/// The open Pip-Boy: the core modal (SPEC-020 §2.1: the page, the held
/// button or zone, a row's flash, the rest's option, the idle timer, the
/// screen saver and the quit-close flag) and what only this layer keeps.
pub(crate) struct PipBoy {
    modal: PipBoyModal,
    pub rest: Option<Resting>,
    /// The holodisk the reading view's key names (ordinal and pages),
    /// read when it was chosen.
    disk: Option<(usize, Holodisk)>,
    /// The pointer seen last; a movement restarts the idle timer.
    pointer: Option<[i32; 2]>,
    /// An archive movie played: the idle timer restarts on return.
    movie_played: bool,
    /// P-02: the automap raster decoded for (map, elevation), kept while
    /// the Pip-Boy is open; a rest step, which can change it, drops it.
    raster: Option<AutomapRaster>,
}
impl std::ops::Deref for PipBoy {
    type Target = PipBoyModal;
    fn deref(&self) -> &PipBoyModal {
        &self.modal
    }
}
type AutomapRaster = ((usize, usize), Option<Vec<u8>>);
impl PipBoy {
    /// The holodisk being read and its page.
    fn reading(&self) -> Option<(&Holodisk, usize)> {
        let Some(Page::Status(StatusView::Reading { disk, page, .. })) = self.modal.page else {
            return None;
        };
        let (_, holodisk) = self.disk.as_ref().filter(|(ordinal, _)| *ordinal == disk)?;
        Some((holodisk, page))
    }
}
#[cfg(test)]
impl PipBoy {
    /// The page shown, if any.
    pub fn tab(&self) -> Option<Tab> {
        self.modal.page.as_ref().map(Page::tab)
    }
    /// The open holodisk and its page.
    pub fn disk(&self) -> Option<(&Holodisk, usize)> {
        self.reading()
    }
    pub fn saver_running(&self) -> bool {
        self.modal.saver.is_some()
    }
}

/// Font 101 text over the Pip-Boy screen with a 640-pixel limit.
struct Pen<'a> {
    painter: Painter<'a>,
    font: Option<Arc<Font>>,
}
impl Pen<'_> {
    fn width(&self, text: &[u8]) -> i32 {
        self.font
            .as_ref()
            .map_or(0, |font| font.text_width(text, false) as i32)
    }
    fn advance(&self, code: u8) -> i32 {
        self.font.as_ref().map_or(0, |font| {
            font.character_width(code)
                .map_or(0, |width| (width + font.spacing()) as i32)
        })
    }
    fn draw(
        &self,
        image: &mut IndexedImage,
        text: &[u8],
        [x, y]: [i32; 2],
        key: ColorKey,
        underline: bool,
    ) -> i32 {
        let Some(font) = &self.font else {
            return 0;
        };
        image.draw_text(
            font,
            self.painter.palette,
            text,
            [x, y],
            pipboy::TEXT_LIMIT as u32,
            TextStyle {
                foreground: self.painter.color(key),
                underline,
                ..Default::default()
            },
        ) as i32
    }
    /// Text on screen line `line`, placed by `align`.
    fn line(
        &self,
        image: &mut IndexedImage,
        text: &[u8],
        align: Align,
        line: i32,
        key: ColorKey,
        underline: bool,
    ) -> [i32; 2] {
        let width = self.width(text);
        let origin = [pipboy::text_x(align, width), pipboy::line_y(line)];
        self.draw(image, text, origin, key, underline);
        origin
    }
    /// A struck-through line: 1 pixel at y + 3 across the advance width.
    fn strike(&self, image: &mut IndexedImage, text: &[u8], [x, y]: [i32; 2], key: ColorKey) {
        let width = self.width(text);
        image.fill_rect(
            [x, y + pipboy::STRIKE_OFFSET, width, 1],
            self.painter.color(key),
        );
    }
}

impl MapPreview {
    fn pipboy_messages(&self) -> Option<Arc<MsgCatalog>> {
        self.host.assets.game_messages(pipboy::MESSAGES).ok()
    }
    /// A `pipboy.msg` text; an absent ID reads `Error` (SPEC-002 §3.5).
    fn pip_text(&self, id: i32) -> Vec<u8> {
        self.pipboy_messages()
            .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
            .unwrap_or_else(|| vault_guidance::MISSING_TEXT.to_vec())
    }
    fn gvars(&self) -> Vec<i32> {
        self.host.world.gvars.clone().unwrap_or_default()
    }
    fn pipboy(&mut self) -> Option<&mut PipBoy> {
        match &mut self.screen {
            Screen::PipBoy(pipboy) => Some(pipboy),
            _ => None,
        }
    }
    /// `p`: the Pip-Boy on its start screen.
    pub(crate) fn open_pipboy(&mut self) {
        self.open_pipboy_on(Tab::Status);
    }
    /// Open the Pip-Boy; `Tab::Alarm` is the rest intent of `z`, which
    /// shows the Alarm Clock page at once when resting is allowed (else the
    /// start screen and the refusal box); any other tab opens the start
    /// screen, with no page current. The
    /// game loop plays `ib1p1xx1` first. Opening resets the credited rest
    /// minutes, then loads `pipboy.msg` and the eleven art files; if any
    /// fails the Pip-Boy stays shut without a message (and, under the safe
    /// rule, nothing stays paused).
    pub(crate) fn open_pipboy_on(&mut self, tab: Tab) {
        // SPEC-019 §3.10: on a map whose record has `pipboy_active=No` the
        // button and its key do nothing.
        if !self.pipboy_active() {
            return;
        }
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        self.host.open_pipboy();
        let painter = self.painter();
        if self.pipboy_messages().is_none()
            || pipboy::ART.iter().any(|name| painter.art(name).is_none())
        {
            tracing::warn!("the Pip-Boy art or messages are unavailable");
            return;
        }
        self.screen = Screen::PipBoy(Box::new(PipBoy {
            modal: PipBoyModal::open(self.clock),
            rest: None,
            disk: None,
            pointer: self.pointer.at,
            movie_played: false,
            raster: None,
        }));
        if tab == Tab::Alarm {
            self.pipboy_alarm();
        }
        self.play_effect(pipboy::OPEN_SOUND.as_bytes(), None);
    }
    /// Close: the map-update scripts run on every close, then the map is
    /// back in front (its processing and palette cycling resume).
    pub(crate) fn close_pipboy(&mut self) {
        if !matches!(self.screen, Screen::PipBoy(_)) {
            return;
        }
        if let Err(error) = self.host.map_update() {
            self.status = error.to_string().into_bytes();
        }
        self.return_to_map();
    }
    /// The Alarm Clock button (or the rest intent): the SPEC-008 §3.9
    /// eligibility test, then the page or the refusal box. Safe rule: a
    /// refusal keeps the previous page current.
    fn pipboy_alarm(&mut self) {
        if self.host.rest_allowed() {
            if let Some(pipboy) = self.pipboy() {
                pipboy.modal.show_alarm();
            }
            return;
        }
        self.play_effect(ERROR_SOUND.as_bytes(), None);
        let title = self.pip_text(msg::CANNOT_REST);
        self.show_dialog(Dialog::done(
            pipboy::REST_DIALOG,
            title,
            Vec::new(),
            ColorKey::ERROR,
        ));
    }
    /// The list lengths the current page's zones depend on (only the
    /// current page's are counted).
    fn pipboy_counts(&self) -> PageCounts {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return PageCounts::default();
        };
        let mut counts = PageCounts::default();
        match pipboy.modal.page {
            Some(Page::Status(StatusView::List)) => {
                counts.locations = self.status_locations().len();
                counts.disks = self.status_disks().len();
            }
            Some(Page::Automaps(AutomapView::List)) => {
                counts.areas = self.automap_areas().len();
            }
            Some(Page::Automaps(AutomapView::Elevations { area, .. })) => {
                let areas = self.automap_areas();
                counts.areas = areas.len();
                counts.entries = areas
                    .get(area)
                    .map_or(0, |key| self.automap_entries(*key).len());
            }
            Some(Page::Archives) => counts.archives = self.archive_list().len(),
            _ => {}
        }
        counts
    }
    /// The current page's zones: (first line, count, Back zone).
    #[cfg(test)]
    pub(super) fn zone_layout(&self) -> (i32, usize, bool) {
        let counts = self.pipboy_counts();
        match &self.screen {
            Screen::PipBoy(pipboy) => pipboy.modal.zone_layout(&counts),
            _ => (0, 0, false),
        }
    }
    /// Esc, Enter and `p` close; Ctrl+Q, Ctrl+X and F10 ask to quit (the
    /// Pip-Boy closes whatever the answer); F12 takes a screenshot;
    /// PageDown and PageUp go to a holodisk reading view. Every other key
    /// does nothing. During a rest only Esc acts: it stops the rest.
    pub(crate) fn pipboy_key(&mut self, key: KeyCode) -> bool {
        let [_, control, _] = self.hud.modifiers;
        let now = self.clock;
        let input = match key {
            KeyCode::Escape => PipBoyKey::Escape,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyP => PipBoyKey::Close,
            KeyCode::KeyQ | KeyCode::KeyX if control => PipBoyKey::Quit,
            KeyCode::F10 => PipBoyKey::Quit,
            KeyCode::F12 => PipBoyKey::F12,
            KeyCode::PageDown => PipBoyKey::PageDown,
            KeyCode::PageUp => PipBoyKey::PageUp,
            _ => PipBoyKey::Other,
        };
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        let outcome = pipboy.modal.key(input, now);
        self.pipboy_outcome(outcome);
        true
    }
    /// A press: a window button shows its down art and plays its press
    /// sound; a row zone waits for the release over it.
    pub(crate) fn pipboy_click(&mut self, point: [i32; 2]) -> bool {
        let counts = self.pipboy_counts();
        let now = self.clock;
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        let outcome = pipboy.modal.press(point, &counts, now);
        self.pipboy_outcome(outcome);
        true
    }
    /// A release over the control pressed acts.
    pub(crate) fn pipboy_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let counts = self.pipboy_counts();
        let now = self.clock;
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        let button = pipboy.modal.pressed().is_some();
        let outcome = pipboy.modal.release(point, &counts, now);
        let acted = outcome != PipBoyOutcome::None;
        self.pipboy_outcome(outcome);
        button || acted
    }
    /// Act on the core modal's outcome (SPEC-020 §2.1 rule 4).
    fn pipboy_outcome(&mut self, outcome: PipBoyOutcome) {
        match outcome {
            PipBoyOutcome::None | PipBoyOutcome::Redraw => {}
            PipBoyOutcome::Pressed(button) => {
                let (press, release) = button.sounds();
                self.press_button_cue(press.as_bytes(), release.as_bytes(), button.rect());
            }
            PipBoyOutcome::Close => self.close_pipboy(),
            PipBoyOutcome::QuitConfirm => self.confirm_quit(),
            PipBoyOutcome::Screenshot => self.screenshot(),
            PipBoyOutcome::AlarmRequested => self.pipboy_alarm(),
            PipBoyOutcome::Location(row) => {
                let Some((town, _)) = self.status_locations().into_iter().nth(row - 1) else {
                    return;
                };
                self.pipboy_flash(Highlight::Location(row), Deferred::Detail(town));
            }
            PipBoyOutcome::Disk(row) => {
                let Some((ordinal, _)) = self.status_disks().into_iter().nth(row - 1) else {
                    return;
                };
                match self.read_disk(ordinal) {
                    Ok(disk) => {
                        for diagnostic in &disk.diagnostics {
                            tracing::warn!(%diagnostic, "holodisk diagnostic");
                        }
                        let then = Deferred::Reading {
                            disk: ordinal,
                            last_page: disk.last_page,
                        };
                        if let Some(pipboy) = self.pipboy() {
                            pipboy.disk = Some((ordinal, disk));
                        }
                        self.pipboy_flash(Highlight::Disk(row), then);
                    }
                    Err(error) => self.status = error.into_bytes(),
                }
            }
            // A flash began, or an automap area or entry was selected.
            PipBoyOutcome::Flashing | PipBoyOutcome::Selected => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            PipBoyOutcome::Archive(row) => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                if self.host.play_archive_movie(row) {
                    if let Some(pipboy) = self.pipboy() {
                        pipboy.movie_played = true;
                    }
                    self.drain_effects();
                }
            }
            PipBoyOutcome::Rest(option) => self.begin_pipboy_rest(option),
            PipBoyOutcome::StopRest => self.finish_rest(RestOutcome::Cancelled),
        }
    }
    /// Show `highlight` for 200 ms, then do `then`, after `ib1p1xx1`.
    fn pipboy_flash(&mut self, highlight: Highlight, then: Deferred) {
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        let now = self.clock;
        if let Some(pipboy) = self.pipboy() {
            pipboy.modal.flash(now, highlight, then);
        }
    }
    /// Choosing an Alarm Clock option: `ib1p1xx1`, the option in yellow
    /// under the watch cursor, and the rest in UI steps.
    fn begin_pipboy_rest(&mut self, option: usize) {
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        let since = self.clock;
        match self.host.begin_rest(ALARM_OPTIONS[option].1) {
            Ok(session) => {
                if let Some(pipboy) = self.pipboy() {
                    pipboy.modal.begin_rest(option);
                    pipboy.rest = Some(Resting { session, since });
                    pipboy.raster = None;
                }
            }
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// The rest ended: `ib2lu1x1` and the page without a highlight; a timed
    /// event that stopped it closes the Pip-Boy.
    fn finish_rest(&mut self, outcome: RestOutcome) {
        if let Some(pipboy) = self.pipboy() {
            pipboy.modal.end_rest();
            pipboy.rest = None;
        }
        self.play_effect(pipboy::ALARM_RELEASE.as_bytes(), None);
        if matches!(outcome, RestOutcome::Interrupted(_)) {
            self.close_pipboy();
        }
    }
    /// One loop pass: the idle timer and pointer, a closing quit
    /// confirmation, a flash's end, a rest step (at least 50 ms apart) and
    /// the screen saver.
    pub(crate) fn pipboy_tick(&mut self, now: u64) -> AppResult<bool> {
        let at = self.pointer.at;
        let dialog = self.dialog.is_some();
        // A pending quit (or the end of the game a rest's events request).
        let quit = self.quit_requested || self.endgame.is_some();
        let Screen::PipBoy(pipboy) = &mut self.screen else {
            return Ok(false);
        };
        let mut changed = false;
        if std::mem::take(&mut pipboy.movie_played) {
            pipboy.modal.idle_since = now;
        }
        if pipboy.pointer != at {
            pipboy.pointer = at;
            changed |= pipboy.modal.input(now);
        }
        if (pipboy.modal.close_after_dialog && !dialog) || (quit && pipboy.modal.resting.is_none())
        {
            self.close_pipboy();
            return Ok(true);
        }
        if pipboy.modal.tick(now) {
            return Ok(true);
        }
        if let Some(resting) = &mut pipboy.rest {
            // A pending quit request stops the rest; the Pip-Boy closes on
            // the next pass.
            if quit {
                self.finish_rest(RestOutcome::Cancelled);
                return Ok(true);
            }
            if !pipboy.modal.rest_due(now) {
                return Ok(changed);
            }
            pipboy.raster = None;
            match self.host.advance_rest(&mut resting.session) {
                Ok(None) => {}
                Ok(Some(outcome)) => self.finish_rest(outcome),
                Err(error) => {
                    self.status = error.to_string().into_bytes();
                    self.finish_rest(RestOutcome::Cancelled);
                }
            }
            return Ok(true);
        }
        if pipboy.modal.saver_due(now, dialog) {
            // One screen-saver pass; starting runs the 50 warm-up passes,
            // which draw nothing and do not wait, first.
            let rng = &mut self.host.world.rng;
            pipboy
                .modal
                .saver_pass(now, || rng.range(0, 32767).unwrap_or(32767));
            changed = true;
        }
        Ok(changed)
    }
    /// The areas of the automap database, sorted by short name bytes.
    fn automap_areas(&self) -> Vec<usize> {
        if self.host.world.fallout2.is_some() {
            return self.fallout2_automap_areas();
        }
        let db = &self.host.world.persistence.automap;
        let mut areas: Vec<(Vec<u8>, usize)> =
            automaps::area_groups(|map, elevation| automaps::captured(db, map, elevation))
                .into_iter()
                .map(|map| (self.message("map.msg", MAP_SHORT_NAMES[map]), map))
                .collect();
        areas.sort_by(|a, b| a.0.cmp(&b.0));
        areas.into_iter().map(|(_, map)| map).collect()
    }
    fn automap_entries(&self, area: usize) -> Vec<(usize, usize)> {
        if self.host.world.fallout2.is_some() {
            return self.fallout2_automap_entries(area);
        }
        let db = &self.host.world.persistence.automap;
        automaps::elevation_entries(area, |map, elevation| {
            automaps::captured(db, map, elevation)
        })
    }
    /// The raster of the selected elevation entry, decoded once.
    fn decode_pipboy_raster(&mut self) {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return;
        };
        let Some(Page::Automaps(AutomapView::Elevations { area, entry })) = pipboy.modal.page
        else {
            return;
        };
        let Some(area) = self.automap_areas().get(area).copied() else {
            return;
        };
        let Some(key) = self.automap_entries(area).get(entry).copied() else {
            return;
        };
        if pipboy.raster.as_ref().is_some_and(|(seen, _)| *seen == key) {
            return;
        }
        let raster = self
            .host
            .world
            .persistence
            .automap
            .raster(key.0, key.1)
            .ok()
            .flatten();
        if let Some(pipboy) = self.pipboy() {
            pipboy.raster = Some((key, raster));
        }
    }
    pub(crate) fn pipboy_image(&mut self) -> AppResult<IndexedImage> {
        self.decode_pipboy_raster();
        let Screen::PipBoy(pipboy) = &self.screen else {
            return Err("Pip-Boy is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        painter.blit_opaque(&mut image, pipboy::BACKGROUND, 0, [0, 0]);
        // The vault-water note, then the date and time.
        let gvars = self.gvars();
        let global = |index: usize| gvars.get(index).copied().unwrap_or(0);
        let days = global(pipboy::WATER_DAYS_GLOBAL);
        // SPEC-019 §2.7: the vault-water note is never drawn under Fallout 2.
        if self.host.world.fallout2.is_none()
            && pipboy::note_shown(global(pipboy::WATER_CHIP_GLOBAL), days)
        {
            let note = pipboy::NOTE_RECT;
            painter.blit_opaque(&mut image, pipboy::NOTE, 0, [note.x, note.y]);
            for cell in pipboy::note_digits(days) {
                painter.blit_region(&mut image, pipboy::NOTE_DIGITS, cell.source, cell.at, false);
            }
        }
        let (digits, month) = pipboy::clock_cells(self.host.world.clock.ticks);
        for cell in digits {
            painter.blit_region(&mut image, pipboy::NUMBERS, cell.source, cell.at, true);
        }
        painter.blit_region(&mut image, pipboy::MONTHS, month.source, month.at, true);
        for button in Button::ALL {
            self.draw_button(&painter, &mut image, button.rect(), button.art());
        }
        let pen = Pen {
            font: painter.font(vault_core::ui::FONT_SMALL),
            painter,
        };
        if let Some(saver) = &pipboy.modal.saver {
            for cell in &saver.cells {
                pen.painter
                    .blit_region(&mut image, pipboy::BOMB, cell.source, cell.at, false);
            }
            return Ok(image);
        }
        let highlight = pipboy
            .modal
            .flash
            .as_ref()
            .map_or(Highlight::None, |flash| flash.highlight);
        let key = |selected: bool| {
            if selected {
                pipboy::SELECTED
            } else {
                pipboy::TEXT
            }
        };
        match &pipboy.modal.page {
            None => {
                pen.painter
                    .blit(&mut image, pipboy::LOGO, 0, pipboy::LOGO_AT);
                let [month, day, _] = self.host.world.game_date();
                if let Some(id) = pipboy::holiday(month, day) {
                    let text = self.pip_text(id);
                    let x = pipboy::holiday_x(pen.width(&text));
                    pen.draw(
                        &mut image,
                        &text,
                        [x, pipboy::HOLIDAY_Y],
                        pipboy::TEXT,
                        false,
                    );
                }
            }
            Some(Page::Status(StatusView::List)) => {
                let locations = self.status_locations();
                let disks = self.status_disks();
                let status = self.pip_text(msg::STATUS);
                if disks.is_empty() {
                    pen.line(&mut image, &status, Align::Centered, 0, pipboy::TEXT, true);
                } else {
                    pen.line(
                        &mut image,
                        &status,
                        Align::LeftHeader,
                        0,
                        pipboy::TEXT,
                        true,
                    );
                    let data = self.pip_text(msg::DATA);
                    pen.line(&mut image, &data, Align::RightHeader, 0, pipboy::TEXT, true);
                }
                if locations.is_empty() {
                    let text = self.pip_text(msg::NO_TASKS);
                    pen.line(&mut image, &text, Align::Normal, 2, pipboy::TEXT, false);
                }
                for (index, (_, text)) in locations.iter().enumerate() {
                    let selected = highlight == Highlight::Location(index + 1);
                    let line = 2 + 2 * index as i32;
                    pen.line(&mut image, text, Align::Normal, line, key(selected), false);
                }
                for (index, (_, text)) in disks.iter().enumerate() {
                    let selected = highlight == Highlight::Disk(index + 1);
                    let line = 2 + 2 * index as i32;
                    pen.line(
                        &mut image,
                        text,
                        Align::RightColumn,
                        line,
                        key(selected),
                        false,
                    );
                }
            }
            Some(Page::Status(StatusView::Detail(town))) => {
                let (location, quests) = self.status_detail(*town);
                let title = [location, b" ".to_vec(), self.pip_text(msg::STATUS_SUFFIX)].concat();
                pen.line(&mut image, &title, Align::Normal, 1, pipboy::TEXT, true);
                let mut line = 3;
                for (number, text, completed) in quests {
                    let entry = [format!("{number}. ").into_bytes(), text].concat();
                    let color = if completed {
                        pipboy::COMPLETED
                    } else {
                        pipboy::TEXT
                    };
                    // A wrap failure skips the text; the number stays used.
                    let wrapped =
                        pipboy::wrap(&entry, pipboy::WRAP_WIDTH, |code| pen.advance(code));
                    for text in wrapped.unwrap_or_default() {
                        let at = pen.line(&mut image, &text, Align::Normal, line, color, false);
                        if completed {
                            pen.strike(&mut image, &text, at, color);
                        }
                        line += 2;
                    }
                }
                let back = self.pip_text(msg::BACK);
                let selected = highlight == Highlight::Back;
                pen.line(&mut image, &back, Align::Centered, 40, key(selected), false);
            }
            Some(Page::Status(StatusView::Reading { .. })) => {
                let Some((disk, page)) = pipboy.reading() else {
                    return Ok(image);
                };
                if page == 0 {
                    pen.line(
                        &mut image,
                        &disk.title,
                        Align::Centered,
                        0,
                        pipboy::TEXT,
                        true,
                    );
                }
                if disk.last_page > 0 {
                    let of = self.pip_text(msg::OF);
                    let counter = [
                        format!("{} ", page + 1).into_bytes(),
                        of.clone(),
                        format!(" {}", disk.page_count()).into_bytes(),
                    ]
                    .concat();
                    let at = [pipboy::counter_x(pen.width(&of)), pipboy::COUNTER_Y];
                    pen.draw(&mut image, &counter, at, pipboy::TEXT, false);
                }
                for (index, record) in disk.page(page).iter().enumerate() {
                    if let DiskLine::Text(text) = record {
                        let line = pipboy::RECORD_LINE + index as i32;
                        pen.line(&mut image, text, Align::NoIndent, line, pipboy::TEXT, false);
                    }
                }
                let back = self.pip_text(msg::BACK);
                pen.line(
                    &mut image,
                    &back,
                    Align::LeftHeader,
                    40,
                    pipboy::TEXT,
                    false,
                );
                let next = self.pip_text(if page >= disk.last_page {
                    msg::DONE
                } else {
                    msg::MORE
                });
                pen.line(
                    &mut image,
                    &next,
                    Align::RightHeader,
                    40,
                    pipboy::TEXT,
                    false,
                );
            }
            Some(Page::Automaps(view)) => {
                let title = self.pip_text(msg::AUTOMAPS);
                pen.line(&mut image, &title, Align::Centered, 0, pipboy::TEXT, true);
                match *view {
                    AutomapView::List => {
                        for (index, map) in self.automap_areas().into_iter().enumerate() {
                            let text = self.automap_name(map);
                            let line = 2 + 2 * index as i32;
                            pen.line(&mut image, &text, Align::Normal, line, pipboy::TEXT, false);
                        }
                    }
                    AutomapView::Elevations { area, entry } => {
                        let Some(area) = self.automap_areas().get(area).copied() else {
                            return Ok(image);
                        };
                        let text = self.automap_title(area);
                        pen.line(&mut image, &text, Align::Centered, 2, pipboy::TEXT, false);
                        for (index, (map, elevation)) in
                            self.automap_entries(area).into_iter().enumerate()
                        {
                            let text = self.automap_entry(map, elevation);
                            let line = pipboy::ENTRY_LINE + 2 * index as i32;
                            let color = key(index == entry);
                            pen.line(&mut image, &text, Align::Normal, line, color, false);
                        }
                        let back = self.pip_text(msg::BACK);
                        pen.line(&mut image, &back, Align::Centered, 40, pipboy::TEXT, false);
                        self.draw_pipboy_automap(&mut image);
                    }
                }
            }
            Some(Page::Archives) => {
                let title = self.pip_text(msg::ARCHIVES);
                pen.line(&mut image, &title, Align::Centered, 0, pipboy::TEXT, true);
                for (index, movie) in self.archive_list().into_iter().enumerate() {
                    let text = self.pip_text(vault_guidance::ARCHIVE_TITLE_BASE + movie as i32);
                    let line = 2 + 2 * index as i32;
                    pen.line(&mut image, &text, Align::Normal, line, pipboy::TEXT, false);
                }
            }
            Some(Page::Alarm) => {
                let title = self.pip_text(msg::ALARM_CLOCK);
                pen.line(&mut image, &title, Align::Centered, 0, pipboy::TEXT, true);
                let world = &self.host.world;
                let hp = world.hit_points(world.player).unwrap_or(0);
                let line = [
                    self.pip_text(msg::HIT_POINTS),
                    format!(" {hp}/{}", world.player_stat(7)).into_bytes(),
                ]
                .concat();
                pen.line(&mut image, &line, Align::Centered, 2, pipboy::TEXT, false);
                let resting = pipboy.modal.resting;
                for (option, (id, _)) in ALARM_OPTIONS.iter().enumerate() {
                    let text = self.pip_text(*id);
                    let line = pipboy::OPTION_LINE + 2 * option as i32;
                    let color = key(resting == Some(option));
                    pen.line(&mut image, &text, Align::Normal, line, color, false);
                }
            }
        }
        Ok(image)
    }
    /// §3.33 the stored raster, each cell 2 pixels wide and 1 tall on a
    /// two-row pitch from (238, 105): walls key 992, scenery key 480, drawn
    /// over the text and the frame; cells below the window are clipped
    /// (safe rule).
    fn draw_pipboy_automap(&self, image: &mut IndexedImage) {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return;
        };
        let Some((_, Some(raster))) = &pipboy.raster else {
            return;
        };
        let painter = self.painter();
        for row in 0..200 {
            for column in 0..200 {
                let Some(key) =
                    automap::pipboy_color(vault_formats::automap::cell(raster, row, column))
                else {
                    continue;
                };
                let [x, y] = automap::pipboy_cell(row, column);
                image.fill_rect([x, y, 2, 1], painter.color(key));
            }
        }
    }
    /// The animated watch `wait2.frm` frame while a rest runs (SPEC-014
    /// §3.26), counted from when the rest began.
    pub(crate) fn pipboy_watch(&self) -> Option<usize> {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return None;
        };
        let since = pipboy.rest.as_ref()?.since;
        let frames = vault_core::ui::cursor::ANIMATED[1].1;
        Some(
            vault_core::ui::cursor::animated_frame(self.clock.saturating_sub(since), frames)
                as usize,
        )
    }
}

/// A Status detail row: its number, text and whether it is completed.
pub(super) type QuestRow = (usize, Vec<u8>, bool);

/// SPEC-019 §3.10 the Pip-Boy's data under either profile: Fallout 1's
/// fixed tables (vault-guidance), or SPEC-018 §5.9's quest and holodisk
/// tables, SPEC-018 §5.2's cities for the automaps and the Fallout 2
/// played-movie record for the archives.
impl MapPreview {
    /// §3.10 the Pip-Boy may open: always under Fallout 1; under Fallout 2
    /// unless the current map's record has `pipboy_active=No`.
    pub(crate) fn pipboy_active(&self) -> bool {
        self.host.world.fallout2.as_deref().is_none_or(|state| {
            state
                .tables
                .maps
                .get(self.host.world.map.header.map_number)
                .is_none_or(|record| record.pipboy_active)
        })
    }
    /// A game message, or SPEC-002 §3.5's placeholder when it is missing.
    fn game_text(&self, file: &str, id: i32) -> Vec<u8> {
        self.host
            .assets
            .game_messages(file)
            .ok()
            .and_then(|catalog| catalog.get(id).map(|message| message.text.clone()))
            .unwrap_or_else(|| vault_guidance::MISSING_TEXT.to_vec())
    }
    /// The Status page's locations: a key for the detail view (Fallout 1's
    /// town, Fallout 2's group) and the title.
    pub(super) fn status_locations(&self) -> Vec<(usize, Vec<u8>)> {
        let gvars = self.gvars();
        match self.host.world.fallout2.as_deref() {
            Some(state) => state
                .quest_list(&gvars)
                .into_iter()
                .enumerate()
                .map(|(group, (location, _))| (group, self.game_text("map.msg", location)))
                .collect(),
            None => vault_guidance::quest_locations(&gvars)
                .into_iter()
                .map(|(town, title)| (town, self.pip_text(title)))
                .collect(),
        }
    }
    /// The held holodisks: ordinal and title.
    pub(super) fn status_disks(&self) -> Vec<(usize, Vec<u8>)> {
        let gvars = self.gvars();
        match self.host.world.fallout2.as_deref() {
            Some(state) => state
                .holodisk_list(&gvars)
                .into_iter()
                .map(|ordinal| {
                    (
                        ordinal,
                        self.pip_text(state.tables.holodisks[ordinal].title),
                    )
                })
                .collect(),
            None => vault_guidance::listed_disks(&gvars)
                .into_iter()
                .map(|ordinal| {
                    let title = self.pip_text(vault_guidance::DISK_TITLE_BASE + ordinal as i32);
                    (ordinal, title)
                })
                .collect(),
        }
    }
    /// Disk `ordinal`'s pages.
    pub(super) fn read_disk(&self, ordinal: usize) -> Result<Holodisk, String> {
        let messages = self
            .pipboy_messages()
            .ok_or_else(|| "pipboy.msg is unavailable".to_string())?;
        match self.host.world.fallout2.as_deref() {
            Some(state) => {
                let disk = state
                    .tables
                    .holodisks
                    .get(ordinal)
                    .ok_or_else(|| format!("holodisk {ordinal} is not in holodisk.txt"))?;
                Ok(vault_guidance::holodisk_at(
                    ordinal, disk.title, disk.text, &messages,
                ))
            }
            None => vault_guidance::holodisk(ordinal, &messages).map_err(|error| error.to_string()),
        }
    }
    /// A location's detail: its title and its visible quests as (number,
    /// text, completed).
    pub(super) fn status_detail(&self, key: usize) -> (Vec<u8>, Vec<QuestRow>) {
        let gvars = self.gvars();
        match self.host.world.fallout2.as_deref() {
            Some(state) => {
                use vault_formats::fallout2::lines::QuestStatus;
                let Some((location, quests)) = state.quest_list(&gvars).into_iter().nth(key) else {
                    return (Vec::new(), Vec::new());
                };
                let rows = quests
                    .into_iter()
                    .enumerate()
                    .map(|(visible, (ordinal, status))| {
                        let quest = &state.tables.quests[ordinal];
                        (
                            visible + 1,
                            self.game_text("quests.msg", quest.description),
                            status == QuestStatus::Completed,
                        )
                    })
                    .collect();
                (self.game_text("map.msg", location), rows)
            }
            None => {
                let title = self.pip_text(vault_guidance::LOCATION_TITLE_BASE + 10 * key as i32);
                let rows = vault_guidance::town_quests(key, &gvars)
                    .into_iter()
                    .map(|quest| {
                        (
                            quest.number,
                            self.pip_text(quest.message),
                            quest.state == QuestState::Completed,
                        )
                    })
                    .collect();
                (title, rows)
            }
        }
    }
    /// The Video Archives' movie indexes.
    pub(super) fn archive_list(&self) -> Vec<usize> {
        match self.host.world.fallout2.as_deref() {
            Some(state) => vault_core::ui::fallout2::archive_movies(&state.movies),
            None => movie::archive_movies(&self.host.world.persistence.movies),
        }
    }
    /// §3.10 the Fallout 2 automap groups: each city with a captured
    /// elevation of one of its maps (SPEC-018 §5.2's area of a map), and
    /// each captured map of no city on its own (keyed 1000 + map), sorted
    /// by name as SPEC-014 sorts its groups.
    pub(super) fn fallout2_automap_areas(&self) -> Vec<usize> {
        let Some(state) = self.host.world.fallout2.as_deref() else {
            return Vec::new();
        };
        let db = &self.host.world.persistence.automap;
        let mut groups: Vec<usize> = Vec::new();
        for map in 0..state.tables.maps.count() {
            if !(0..3).any(|elevation| automaps::captured(db, map, elevation)) {
                continue;
            }
            let area = state.tables.city.area_of_map(map as i32);
            let key = usize::try_from(area).unwrap_or(1000 + map);
            if !groups.contains(&key) {
                groups.push(key);
            }
        }
        let mut named: Vec<(Vec<u8>, usize)> = groups
            .into_iter()
            .map(|key| (self.automap_name(key), key))
            .collect();
        named.sort();
        named.into_iter().map(|(_, key)| key).collect()
    }
    /// A group's captured elevations, every map of the city in ascending
    /// map number.
    pub(super) fn fallout2_automap_entries(&self, key: usize) -> Vec<(usize, usize)> {
        let Some(state) = self.host.world.fallout2.as_deref() else {
            return Vec::new();
        };
        let db = &self.host.world.persistence.automap;
        (0..state.tables.maps.count())
            .filter(|map| match key.checked_sub(1000) {
                Some(single) => *map == single,
                None => state.tables.city.area_of_map(*map as i32) == key as i32,
            })
            .flat_map(|map| (0..3).map(move |elevation| (map, elevation)))
            .filter(|(map, elevation)| automaps::captured(db, *map, *elevation))
            .collect()
    }
    /// An automap group's name in the list.
    pub(super) fn automap_name(&self, key: usize) -> Vec<u8> {
        match self.host.world.fallout2.as_deref() {
            Some(state) => match key.checked_sub(1000) {
                Some(map) => self.map_lookup_name(state, map),
                None => state
                    .tables
                    .city
                    .get(key as i32)
                    .map(|area| area.name.clone().into_bytes())
                    .unwrap_or_default(),
            },
            None => self.message("map.msg", MAP_SHORT_NAMES[key]),
        }
    }
    /// The elevation view's title.
    fn automap_title(&self, key: usize) -> Vec<u8> {
        match self.host.world.fallout2 {
            Some(_) => self.automap_name(key),
            None => self.message("map.msg", 100 + key as i32),
        }
    }
    /// An elevation entry: Fallout 1's elevation name; under Fallout 2,
    /// which has no elevation names, the map's lookup name and the
    /// elevation counted from 1.
    pub(super) fn automap_entry(&self, map: usize, elevation: usize) -> Vec<u8> {
        match self.host.world.fallout2.as_deref() {
            Some(state) => {
                let mut text = self.map_lookup_name(state, map);
                text.extend_from_slice(format!(" {}", elevation + 1).as_bytes());
                text
            }
            None => self.message("map.msg", 200 + 3 * map as i32 + elevation as i32),
        }
    }
    fn map_lookup_name(&self, state: &vault_core::fallout2::Fallout2World, map: usize) -> Vec<u8> {
        state
            .tables
            .maps
            .get(map as i32)
            .map(|record| {
                record
                    .lookup_name
                    .clone()
                    .unwrap_or_else(|| record.stem.clone())
                    .into_bytes()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "pipboy_tests.rs"]
mod tests;
