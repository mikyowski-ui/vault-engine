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
            self, ALARM_OPTIONS, Align, Button, Cell, ReadingAction, ReadingInput, ScreenSaver,
            StatusClick, msg,
        },
        saveload::MAP_SHORT_NAMES,
    },
};
use vault_formats::{Font, MsgCatalog};
use vault_guidance::{DiskLine, Holodisk, QuestState, automaps};
use vault_render::{IndexedImage, TextStyle};
use winit::keyboard::KeyCode;

/// The 200 ms a selected row shows yellow before it acts.
const FLASH_MS: u64 = 200;

/// The pages, by the button that shows them (`z` opens on the Alarm Clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Status,
    Automaps,
    Archives,
    Alarm,
}

enum Page {
    Status(StatusView),
    Automaps(AutomapView),
    Archives,
    Alarm,
}
enum StatusView {
    List,
    Detail(usize),
    Reading { disk: Holodisk, page: usize },
}
#[derive(Clone, Copy)]
enum AutomapView {
    List,
    /// Area map and the selected entry (from 0).
    Elevations {
        area: usize,
        entry: usize,
    },
}
/// A row zone, or the Back zone on line 40.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Row(i32),
    Back,
}
/// What shows yellow while a flash waits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Highlight {
    None,
    Location(usize),
    Disk(usize),
    Back,
}
enum Deferred {
    Detail(usize),
    Reading(Holodisk),
    StatusList,
    Page(ReadingAction),
}
struct Flash {
    until: u64,
    highlight: Highlight,
    then: Deferred,
}
pub(crate) struct Resting {
    pub session: RestSession,
    option: usize,
    since: u64,
}
struct Saver {
    state: ScreenSaver,
    cells: Vec<Cell>,
    last_pass: u64,
}

pub(crate) struct PipBoy {
    /// No page is current on the start screen.
    page: Option<Page>,
    pub rest: Option<Resting>,
    last_step: u64,
    flash: Option<Flash>,
    pressed: Option<Button>,
    zone: Option<Zone>,
    idle_since: u64,
    pointer: Option<[i32; 2]>,
    saver: Option<Saver>,
    /// The quit confirmation closes the Pip-Boy whatever the answer.
    close_after_dialog: bool,
    /// An archive movie played: the idle timer restarts on return.
    movie_played: bool,
    /// P-02: the automap raster decoded for (map, elevation), kept while
    /// the Pip-Boy is open; a rest step, which can change it, drops it.
    raster: Option<AutomapRaster>,
}
type AutomapRaster = ((usize, usize), Option<Vec<u8>>);
#[cfg(test)]
impl PipBoy {
    /// The page shown, if any.
    pub fn tab(&self) -> Option<Tab> {
        self.page.as_ref().map(|page| match page {
            Page::Status(_) => Tab::Status,
            Page::Automaps(_) => Tab::Automaps,
            Page::Archives => Tab::Archives,
            Page::Alarm => Tab::Alarm,
        })
    }
    /// The open holodisk and its page.
    pub fn disk(&self) -> Option<(&Holodisk, usize)> {
        match &self.page {
            Some(Page::Status(StatusView::Reading { disk, page })) => Some((disk, *page)),
            _ => None,
        }
    }
    pub fn saver_running(&self) -> bool {
        self.saver.is_some()
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
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        if self.pipboy_messages().is_none()
            || pipboy::ART.iter().any(|name| painter.art(name).is_none())
        {
            tracing::warn!("the Pip-Boy art or messages are unavailable");
            return;
        }
        self.screen = Screen::PipBoy(Box::new(PipBoy {
            page: None,
            rest: None,
            last_step: 0,
            flash: None,
            pressed: None,
            zone: None,
            idle_since: self.clock,
            pointer: self.pointer.at,
            saver: None,
            close_after_dialog: false,
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
    /// Any input or pointer movement restarts the idle timer; one that
    /// ends the screen saver is consumed.
    fn pipboy_input(&mut self) -> bool {
        let now = self.clock;
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        pipboy.idle_since = now;
        pipboy.saver.take().is_some()
    }
    /// A page button: its page from the initial state (also when it is
    /// already shown).
    fn show_tab(&mut self, tab: Tab) {
        match tab {
            Tab::Status => self.show_page(Page::Status(StatusView::List)),
            Tab::Automaps => self.show_page(Page::Automaps(AutomapView::List)),
            Tab::Archives => self.show_page(Page::Archives),
            Tab::Alarm => self.pipboy_alarm(),
        }
    }
    fn show_page(&mut self, page: Page) {
        if let Some(pipboy) = self.pipboy() {
            pipboy.page = Some(page);
        }
    }
    /// The Alarm Clock button (or the rest intent): the SPEC-008 §3.9
    /// eligibility test, then the page or the refusal box. Safe rule: a
    /// refusal keeps the previous page current.
    fn pipboy_alarm(&mut self) {
        if self.host.rest_allowed() {
            self.show_page(Page::Alarm);
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
    /// Esc, Enter and `p` close; Ctrl+Q, Ctrl+X and F10 ask to quit (the
    /// Pip-Boy closes whatever the answer); F12 takes a screenshot;
    /// PageDown and PageUp go to a holodisk reading view. Every other key
    /// does nothing. During a rest only Esc acts: it stops the rest.
    pub(crate) fn pipboy_key(&mut self, key: KeyCode) -> bool {
        if self.pipboy_input() {
            return true;
        }
        let [_, control, _] = self.hud.modifiers;
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        if pipboy.rest.is_some() {
            if key == KeyCode::Escape {
                self.finish_rest(RestOutcome::Cancelled);
            }
            return true;
        }
        if pipboy.flash.is_some() {
            return true;
        }
        match key {
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyP => {
                self.close_pipboy();
            }
            KeyCode::KeyQ | KeyCode::KeyX if control => self.pipboy_quit(),
            KeyCode::F10 => self.pipboy_quit(),
            KeyCode::F12 => self.screenshot(),
            KeyCode::PageDown => self.reading_input(ReadingInput::PageDown),
            KeyCode::PageUp => self.reading_input(ReadingInput::PageUp),
            _ => {}
        }
        true
    }
    fn pipboy_quit(&mut self) {
        self.confirm_quit();
        if let Some(pipboy) = self.pipboy() {
            pipboy.close_after_dialog = true;
        }
    }
    /// A press: a window button shows its down art and plays its press
    /// sound; a row zone waits for the release over it.
    pub(crate) fn pipboy_click(&mut self, point: [i32; 2]) -> bool {
        if self.pipboy_input() {
            return true;
        }
        let zone = self.pipboy_zone(point);
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        if pipboy.rest.is_some() || pipboy.flash.is_some() {
            return true;
        }
        if let Some(button) = Button::at(point) {
            pipboy.pressed = Some(button);
            let (press, release) = button.sounds();
            self.press_button_cue(press.as_bytes(), release.as_bytes(), button.rect());
            return true;
        }
        pipboy.zone = zone;
        true
    }
    /// A release over the control pressed acts.
    pub(crate) fn pipboy_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let zone = point.and_then(|point| self.pipboy_zone(point));
        let Some(pipboy) = self.pipboy() else {
            return false;
        };
        let (button, pressed_zone) = (pipboy.pressed.take(), pipboy.zone.take());
        let Some(point) = point else {
            return button.is_some();
        };
        if let Some(button) = button {
            if button.rect().contains(point) {
                match button {
                    Button::Status => self.show_tab(Tab::Status),
                    Button::Automaps => self.show_tab(Tab::Automaps),
                    Button::Archives => self.show_tab(Tab::Archives),
                    Button::Close => self.close_pipboy(),
                    Button::Alarm => self.show_tab(Tab::Alarm),
                }
            }
            return true;
        }
        match (pressed_zone, zone) {
            (Some(pressed), Some(zone)) if pressed == zone => {
                self.zone_release(zone, point[0]);
                true
            }
            _ => false,
        }
    }
    /// The current page's zones: (first line, count, Back zone).
    fn zone_layout(&self) -> (i32, usize, bool) {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return (0, 0, false);
        };
        match &pipboy.page {
            None => (0, 0, false),
            Some(Page::Status(StatusView::List)) => {
                let locations = self.status_locations().len();
                let disks = self.status_disks().len();
                (2, pipboy::status_zone_count(locations, disks), false)
            }
            Some(Page::Status(_)) => (0, 0, true),
            Some(Page::Automaps(AutomapView::List)) => (2, self.automap_areas().len(), false),
            Some(Page::Automaps(AutomapView::Elevations { area, .. })) => {
                (0, self.automap_entries(*area).len() + 2, true)
            }
            Some(Page::Archives) => (2, self.archive_list().len(), false),
            Some(Page::Alarm) => (pipboy::OPTION_LINE, ALARM_OPTIONS.len(), false),
        }
    }
    fn pipboy_zone(&self, point: [i32; 2]) -> Option<Zone> {
        let (first, count, back) = self.zone_layout();
        if back && pipboy::BACK_ZONE.contains(point) {
            return Some(Zone::Back);
        }
        pipboy::zone_row(first, count, point).map(Zone::Row)
    }
    /// Show `highlight` for 200 ms, then do `then`, after `ib1p1xx1`.
    fn flash(&mut self, highlight: Highlight, then: Deferred) {
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        let until = self.clock + FLASH_MS;
        if let Some(pipboy) = self.pipboy() {
            pipboy.flash = Some(Flash {
                until,
                highlight,
                then,
            });
        }
    }
    fn zone_release(&mut self, zone: Zone, x: i32) {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return;
        };
        match (&pipboy.page, zone) {
            (Some(Page::Status(StatusView::List)), Zone::Row(row)) => {
                let locations = self.status_locations();
                let disks = self.status_disks();
                match pipboy::status_click(x, row, locations.len(), disks.len()) {
                    Some(StatusClick::Location(row)) => {
                        let town = locations[row - 1].0;
                        self.flash(Highlight::Location(row), Deferred::Detail(town));
                    }
                    Some(StatusClick::Disk(row)) => {
                        let Some((ordinal, _)) = disks.get(row - 1) else {
                            return;
                        };
                        match self.read_disk(*ordinal) {
                            Ok(disk) => {
                                for diagnostic in &disk.diagnostics {
                                    tracing::warn!(%diagnostic, "holodisk diagnostic");
                                }
                                self.flash(Highlight::Disk(row), Deferred::Reading(disk));
                            }
                            Err(error) => self.status = error.into_bytes(),
                        }
                    }
                    None => {}
                }
            }
            (Some(Page::Status(StatusView::Detail(_))), Zone::Back) => {
                self.flash(Highlight::Back, Deferred::StatusList);
            }
            (Some(Page::Status(StatusView::Reading { .. })), Zone::Back) => {
                self.reading_input(ReadingInput::Release(x));
            }
            (Some(Page::Automaps(AutomapView::List)), Zone::Row(row)) => {
                let areas = self.automap_areas();
                if let Some(area) = usize::try_from(row - 1).ok().and_then(|i| areas.get(i)) {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                    let view = AutomapView::Elevations {
                        area: *area,
                        entry: 0,
                    };
                    self.show_page(Page::Automaps(view));
                }
            }
            (Some(Page::Automaps(AutomapView::Elevations { area, .. })), zone) => {
                let area = *area;
                match zone {
                    Zone::Back | Zone::Row(-1) => {
                        self.show_page(Page::Automaps(AutomapView::List));
                        self.play_effect(BUTTON_PRESS.as_bytes(), None);
                    }
                    Zone::Row(row) if row >= 1 => {
                        self.play_effect(BUTTON_PRESS.as_bytes(), None);
                        let entry = row as usize - 1;
                        self.show_page(Page::Automaps(AutomapView::Elevations { area, entry }));
                    }
                    Zone::Row(_) => {}
                }
            }
            (Some(Page::Archives), Zone::Row(row)) if row >= 1 => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                if self.host.play_archive_movie(row) {
                    if let Some(pipboy) = self.pipboy() {
                        pipboy.movie_played = true;
                    }
                    self.drain_effects();
                }
            }
            (Some(Page::Alarm), Zone::Row(row)) => {
                if let Some(option) = pipboy::alarm_option(row) {
                    self.begin_pipboy_rest(option);
                }
            }
            _ => {}
        }
    }
    /// The reading view's line-40 controls and page keys; elsewhere page
    /// keys do nothing.
    fn reading_input(&mut self, input: ReadingInput) {
        let Screen::PipBoy(pipboy) = &self.screen else {
            return;
        };
        let Some(Page::Status(StatusView::Reading { disk, page })) = &pipboy.page else {
            return;
        };
        if let Some(action) = pipboy::reading_action(input, *page, disk.last_page) {
            self.flash(Highlight::None, Deferred::Page(action));
        }
    }
    fn apply_deferred(&mut self, then: Deferred) {
        let Some(pipboy) = self.pipboy() else {
            return;
        };
        let view = match then {
            Deferred::Detail(town) => StatusView::Detail(town),
            Deferred::Reading(disk) => StatusView::Reading { disk, page: 0 },
            Deferred::StatusList | Deferred::Page(ReadingAction::List) => StatusView::List,
            Deferred::Page(action) => {
                if let Some(Page::Status(StatusView::Reading { page, .. })) = &mut pipboy.page {
                    *page = match action {
                        ReadingAction::Next => *page + 1,
                        _ => page.saturating_sub(1),
                    };
                }
                return;
            }
        };
        pipboy.page = Some(Page::Status(view));
    }
    /// Choosing an Alarm Clock option: `ib1p1xx1`, the option in yellow
    /// under the watch cursor, and the rest in UI steps.
    fn begin_pipboy_rest(&mut self, option: usize) {
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        let since = self.clock;
        match self.host.begin_rest(ALARM_OPTIONS[option].1) {
            Ok(session) => {
                if let Some(pipboy) = self.pipboy() {
                    pipboy.rest = Some(Resting {
                        session,
                        option,
                        since,
                    });
                    pipboy.last_step = 0;
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
            pipboy.idle_since = now;
        }
        if pipboy.pointer != at {
            pipboy.pointer = at;
            pipboy.idle_since = now;
            changed |= pipboy.saver.take().is_some();
        }
        if (pipboy.close_after_dialog && !dialog) || (quit && pipboy.rest.is_none()) {
            self.close_pipboy();
            return Ok(true);
        }
        if let Some(flash) = pipboy.flash.take_if(|flash| now >= flash.until) {
            self.apply_deferred(flash.then);
            return Ok(true);
        }
        if let Some(resting) = &mut pipboy.rest {
            // A pending quit request stops the rest; the Pip-Boy closes on
            // the next pass.
            if quit {
                self.finish_rest(RestOutcome::Cancelled);
                return Ok(true);
            }
            if now.saturating_sub(pipboy.last_step) < pipboy::SAVER_PASS_MS {
                return Ok(changed);
            }
            pipboy.last_step = now;
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
        let idle = now.saturating_sub(pipboy.idle_since) > pipboy::IDLE_MS;
        let due = match &pipboy.saver {
            Some(saver) => now.saturating_sub(saver.last_pass) >= pipboy::SAVER_PASS_MS,
            None => idle && !dialog && pipboy.flash.is_none(),
        };
        if due {
            self.saver_pass(now);
            changed = true;
        }
        Ok(changed)
    }
    /// One screen-saver pass; starting runs the 50 warm-up passes, which
    /// draw nothing and do not wait, first.
    fn saver_pass(&mut self, now: u64) {
        let Screen::PipBoy(pipboy) = &mut self.screen else {
            return;
        };
        let rng = &mut self.host.world.rng;
        let saver = pipboy.saver.get_or_insert_with(|| Saver {
            state: ScreenSaver::default(),
            cells: Vec::new(),
            last_pass: now,
        });
        let mut draw = || rng.range(0, 32767).unwrap_or(32767);
        loop {
            if let Some(cells) = saver.state.pass(&mut draw) {
                saver.cells = cells;
                saver.last_pass = now;
                return;
            }
        }
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
        let Some(Page::Automaps(AutomapView::Elevations { area, entry })) = pipboy.page else {
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
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
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
            let (up, down) = button.art();
            let rect = button.rect();
            painter.blit(
                &mut image,
                self.button_art(rect, up, down),
                0,
                [rect.x, rect.y],
            );
        }
        let pen = Pen {
            font: painter.font(vault_core::ui::FONT_SMALL),
            painter,
        };
        if let Some(saver) = &pipboy.saver {
            for cell in &saver.cells {
                pen.painter
                    .blit_region(&mut image, pipboy::BOMB, cell.source, cell.at, false);
            }
            return Ok(image);
        }
        let highlight = pipboy
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
        match &pipboy.page {
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
            Some(Page::Status(StatusView::Reading { disk, page })) => {
                if *page == 0 {
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
                for (index, record) in disk.page(*page).iter().enumerate() {
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
                let next = self.pip_text(if *page >= disk.last_page {
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
                let resting = pipboy.rest.as_ref().map(|resting| resting.option);
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
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
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
