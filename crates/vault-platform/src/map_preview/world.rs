//! SPEC-014 §3.39 world map screen and §3.40 town map screen, over the
//! SPEC-008 §3.4-§3.7 travel rules. The world map shows `worldmap.frm`
//! (1400 by 1500, one image) through the 450-by-442 hole of `wmapbox.frm`
//! at (22, 21), darkened by discovery cell, with the party marker, the
//! hover label, the destination crosshair and travel dots, the side panel's
//! town buttons and name plates, the Town/World button and the date. The
//! town map shows `twnmapNN.frm` at (20, 20) under the same frame and side
//! panel, with one triangle marker per known entrance.
//!
//! `[May-improve]` choices taken: markers and labels are clipped to the
//! view and the whole frame is drawn on every pass (no stray triangle
//! pixels at the panel's edge, no world pixel at (35, 462)); Home leaves vy
//! at 0; a quit from a town map opened by entering a town leaves at once;
//! a town map that fails (an invalid town or missing picture) leaves the
//! world map open at the current town; the indicator boxes come back with
//! the next map (the shell draws them whenever a map shows).
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::{
    TravelOutcome, WorldMapExit,
    ui::{ColorKey, Rect, cursor},
    worldmap::{
        self, COLUMNS, Encounter, ROWS, TOWNS, TownButton, TownEntry, VIEWPORT_ORIGIN,
        VIEWPORT_SIZE, WORLD_SIZE,
        screen::{self, Marker},
    },
};
use vault_render::{IndexedImage, TextStyle};
use winit::keyboard::KeyCode;

const WORLD_ART: &str = "worldmap.frm";
const FRAME_ART: &str = "wmapbox.frm";
const NAMES_ART: &str = "wmaplabs.frm";
const PLUS_ART: &str = "wmaploc.frm";
const TARGET_ART: &str = "wmaptarg.frm";
const DARK_TARGET_ART: &str = "wmaptrg2.frm";
const BOLT_ART: [&str; 2] = ["wmapfgt0.frm", "wmapfgt1.frm"];
const TRIANGLE_ART: &str = "hotspot1.frm";
const PRESSED_TRIANGLE_ART: &str = "hotspot2.frm";
const MONTHS_ART: &str = "months.frm";
const DIGITS_ART: &str = "numbers.frm";
const BUTTON_UP_ART: &str = "lilredup.frm";
const BUTTON_DOWN_ART: &str = "lilreddn.frm";
/// Entering a location and pressing a town-map marker.
const ENTER_SOUND: &[u8] = b"ib2p1xx1";
/// A destination click.
const CLICK_SOUND: &[u8] = b"ib1p1xx1";
/// The side panel's Town/World button, after the twelve town buttons.
const TOWN_WORLD: usize = 12;
/// One bit per world pixel.
const DOT_WORDS: usize = (WORLD_SIZE[0] * WORLD_SIZE[1]) as usize / 64 + 1;

pub(crate) struct WorldScreen {
    /// The world pixel shown at (22, 21).
    pub view: [i32; 2],
    pub follow: bool,
    /// G-24 / SPEC-014 §3.17 the world-map pause window is open.
    pub paused: bool,
    /// §3.17 the pause greyed the view: the view origin, follow mode and
    /// hover state when it did. The region stays grey until a pass
    /// redraws the view (§3.39 "Drawing a pass": follow mode on, a scroll,
    /// a hover change or a travel iteration).
    pub grey: Option<([i32; 2], bool, bool)>,
    /// The pointer hovers on the party (at rest).
    pub hover: bool,
    /// Travel dots: one bit per world pixel.
    dots: Vec<u64>,
    /// The town a journey chosen on a town map heads for.
    pub journey: Option<usize>,
    /// The towns whose names were drawn when the screen was built.
    names: u32,
    /// The start of the last pass.
    last_pass: Option<u64>,
    /// The side-panel button held down: a town 0-11, or the Town/World
    /// button.
    held: Option<usize>,
    /// The press closed the pause window: its release does nothing.
    swallow: bool,
    pub pending: Option<Pending>,
}
/// A timed step that holds the world map until it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    /// The pressed triangle shows until `until`; then the party enters the
    /// town, or drops to terrain.
    Enter { until: u64, town: Option<usize> },
    /// The encounter flash from `start`; then the encounter map loads.
    Flash { start: u64, encounter: Encounter },
}
/// What opened a town map (SPEC-014 §3.40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TownOrigin {
    /// An exit grid holding map -1, or the `world_map` opcode.
    Transition,
    /// The world map's Town/World button.
    TownWorld,
    /// Entering a known town on the world map.
    Entered,
}
pub(crate) struct TownScreen {
    /// The town shown.
    pub town: usize,
    pub origin: TownOrigin,
    pub buttons: Vec<TownButton>,
    /// The marker held down.
    pressed: Option<usize>,
    /// The side-panel button held down.
    held: Option<usize>,
    /// The world map under this town map, when opened from it.
    world: Option<Box<WorldScreen>>,
}

impl WorldScreen {
    fn new(position: [i32; 2]) -> Self {
        Self {
            view: screen::centered(position),
            follow: true,
            paused: false,
            grey: None,
            hover: false,
            dots: vec![0; DOT_WORDS],
            journey: None,
            names: 0,
            last_pass: None,
            held: None,
            swallow: false,
            pending: None,
        }
    }
    fn dot_index(pixel: [i32; 2]) -> Option<usize> {
        ((0..WORLD_SIZE[0]).contains(&pixel[0]) && (0..WORLD_SIZE[1]).contains(&pixel[1]))
            .then(|| (pixel[1] * WORLD_SIZE[0] + pixel[0]) as usize)
    }
    fn mark(&mut self, pixel: [i32; 2]) {
        if let Some(index) = Self::dot_index(pixel) {
            self.dots[index / 64] |= 1 << (index % 64);
        }
    }
    pub fn marked(&self, pixel: [i32; 2]) -> bool {
        Self::dot_index(pixel).is_some_and(|index| self.dots[index / 64] & (1 << (index % 64)) != 0)
    }
    fn clear_dots(&mut self) {
        self.dots.fill(0);
    }
}

fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}
/// The side-panel button under `point`: a town 0-11 or the Town/World
/// button.
fn panel_button(point: [i32; 2]) -> Option<usize> {
    (0..TOWNS.len())
        .find(|town| screen::town_button(*town).is_some_and(|rect| inside(rect, point)))
        .or_else(|| inside(screen::town_world_button(), point).then_some(TOWN_WORLD))
}
fn panel_rect(button: usize) -> [i32; 4] {
    screen::town_button(button).unwrap_or_else(screen::town_world_button)
}
const VIEW_CLIP: [i64; 4] = [
    VIEWPORT_ORIGIN[0] as i64,
    VIEWPORT_ORIGIN[1] as i64,
    VIEWPORT_SIZE[0] as i64,
    VIEWPORT_SIZE[1] as i64,
];

impl MapPreview {
    fn known_towns(&self) -> u32 {
        (0..TOWNS.len())
            .filter(|town| self.host.world.world_map.town_enabled(*town))
            .fold(0, |bits, town| bits | 1 << town)
    }
    fn painter(&self) -> Painter<'_> {
        Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        }
    }
    /// Open the world map (SPEC-014 §3.39): W and Q computed once, follow
    /// mode on and the view centered on the party. The music was requested
    /// by the transition that opened it.
    pub(crate) fn open_world_map(&mut self) {
        let world = WorldScreen::new(self.host.world.world_map.position());
        self.show_world(world, true);
    }
    /// The world map opened by the interface rather than by a world
    /// transition (a town map's World button or failure, the preview's W
    /// key): SPEC-008 §3.10 `03wrldmp` starts each time the screen is built.
    pub(crate) fn open_world_map_with_music(&mut self) {
        self.host.play_world_map_music();
        self.open_world_map();
    }
    /// Building the screen (SPEC-014 §3.39): the reveal at the party's
    /// cell, no travel dots, and the town names as they are now.
    fn show_world(&mut self, mut world: WorldScreen, opening: bool) {
        if let Err(error) = self.host.build_world_map(opening) {
            self.status = error.to_string().into_bytes();
        }
        world.clear_dots();
        world.names = self.known_towns();
        world.last_pass = None;
        world.held = None;
        world.pending = None;
        world.hover = false;
        self.screen = Screen::World(world);
    }
    /// Back to the world map from a town map: the screen is built again,
    /// with its music.
    fn return_to_world(&mut self, mut world: WorldScreen, follow: bool) {
        self.host.play_world_map_music();
        world.follow = follow;
        world.grey = None;
        self.show_world(world, false);
    }
    /// A town map for `town`, or none when it fails: a town outside 0-11
    /// (the `[May-improve]` rule, which also rejects town 12 and negative
    /// towns) or a picture that cannot be loaded. The town's knowledge is
    /// refreshed first.
    fn town_screen(
        &mut self,
        town: i32,
        origin: TownOrigin,
        world: Option<Box<WorldScreen>>,
    ) -> Result<TownScreen, Option<Box<WorldScreen>>> {
        let Some(town) = usize::try_from(town).ok().filter(|t| *t < TOWNS.len()) else {
            return Err(world);
        };
        let picture = screen::town_picture(town).unwrap_or_default();
        if self.painter().art(&picture).is_none() {
            return Err(world);
        }
        self.host.refresh_world_knowledge();
        let gvars = self.host.world.gvars.clone().unwrap_or_default();
        let buttons = self.host.world.world_map.town_buttons(town, &gvars);
        Ok(TownScreen {
            town,
            origin,
            buttons,
            pressed: None,
            held: None,
            world,
        })
    }
    /// The current town's map (SPEC-013 §3.22 map -1, the `world_map`
    /// opcode). Under the safe rule a failure opens the world map at the
    /// current town instead.
    pub(crate) fn open_town_map(&mut self) {
        let town = self.host.world.world_map.saved.town;
        match self.town_screen(town, TownOrigin::Transition, None) {
            Ok(screen) => self.screen = Screen::Town(screen),
            Err(_) => self.open_world_map_with_music(),
        }
    }
    /// A town map opened from the world map; a failure leaves the world map
    /// open (`[May-improve]`).
    fn open_town_from_world(&mut self, town: i32, origin: TownOrigin, world: WorldScreen) {
        match self.town_screen(town, origin, Some(Box::new(world))) {
            Ok(screen) => self.screen = Screen::Town(screen),
            Err(world) => {
                if let Some(world) = world {
                    self.screen = Screen::World(*world);
                }
            }
        }
    }
    /// SPEC-013 §3.22 the outgoing map could not be saved for the town or
    /// world map: `map.msg` 1000 in a large box at (169, 116) with the
    /// error sound; closing it returns to the main menu.
    pub(crate) fn map_save_failed(&mut self) {
        self.play_effect(vault_core::ui::ERROR_SOUND.as_bytes(), None);
        let title = self.message("map.msg", 1000);
        let dialog = super::dialog::Dialog::done(
            [169, 116],
            title,
            Vec::new(),
            vault_core::ui::ColorKey::ERROR,
        );
        self.show_dialog(dialog.quitting());
    }
    /// Leaving to a loaded map.
    fn leave_world(&mut self, loaded: Result<(), vault_core::WorldError>) {
        match loaded {
            Ok(()) => self.return_to_map(),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// Entering town `town` (SPEC-014 §3.39): a town not yet known/visited
    /// loads its slot-zero map and is discovered; otherwise it becomes the
    /// current town and its town map opens.
    fn enter_world_town(&mut self, town: usize) {
        let Screen::World(_) = &self.screen else {
            return;
        };
        match self.host.enter_town(town) {
            Ok(Some(TownEntry::Direct(_))) => self.return_to_map(),
            Ok(Some(TownEntry::TownMap { .. })) => {
                let Screen::World(world) = std::mem::replace(&mut self.screen, Screen::Map) else {
                    return;
                };
                self.open_town_from_world(town as i32, TownOrigin::Entered, world);
            }
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// Entering where the party stands: its town cell's town, or a terrain
    /// map (SPEC-008 §3.5 choice, index 65 and index 32 = 1).
    fn enter_location(&mut self, town: Option<usize>) {
        match town {
            Some(town) => self.enter_world_town(town),
            None => {
                let entered = self
                    .host
                    .terrain_dropout()
                    .and_then(|encounter| self.host.enter_encounter(encounter));
                self.leave_world(entered);
            }
        }
    }
    /// The town of the party's cell.
    fn party_town(&self) -> Option<usize> {
        self.host.world.world_map.cell().and_then(worldmap::town_at)
    }
    /// Start the pressed-triangle wait before entering (SPEC-014 §3.39).
    fn begin_entry(&mut self, wait: u64) {
        let town = self.party_town();
        let until = self.clock + wait;
        self.host.world.world_map.travel = None;
        if let Screen::World(world) = &mut self.screen {
            world.clear_dots();
            world.pending = Some(Pending::Enter { until, town });
        }
    }
    /// Head for town `town`'s jittered center (SPEC-008 §3.7).
    fn travel_to_town(&mut self, town: usize) -> bool {
        match worldmap::town_target(town, &mut self.host.world.rng) {
            Ok(target) => {
                self.host.travel_to(target);
                true
            }
            Err(error) => {
                self.status = error.to_string().into_bytes();
                false
            }
        }
    }
    /// A town button's event (on release, SPEC-014 §3.39).
    fn world_town_button(&mut self, town: usize) {
        let map = &self.host.world.world_map;
        if !map.town_enabled(town) {
            return;
        }
        let travelling = map.travel;
        let cell = TOWNS[town].cell;
        if travelling.is_some_and(|travel| worldmap::cell_of(travel.destination) == Some(cell)) {
            return;
        }
        if !self.host.party_in_town(town) {
            if self.travel_to_town(town)
                && let Screen::World(world) = &mut self.screen
            {
                world.follow = travelling.is_none();
                world.journey = None;
            }
            return;
        }
        self.begin_entry(if travelling.is_some() {
            screen::ENTER_TRAVELLING_MS
        } else {
            screen::ENTER_MS
        });
    }
    /// The Town/World button (event 512): the journey stops where the party
    /// is, and the town map of the journey's town (or the current town)
    /// opens.
    fn world_town_world(&mut self) {
        let Screen::World(mut world) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let travelling = self.host.world.world_map.travel.take().is_some();
        let town = world
            .journey
            .filter(|_| travelling)
            .map_or(self.host.world.world_map.saved.town, |town| town as i32);
        world.journey = None;
        world.clear_dots();
        self.open_town_from_world(town, TownOrigin::TownWorld, world);
    }
    pub(crate) fn world_key(&mut self, key: KeyCode) -> bool {
        let [_, control, _] = self.hud.modifiers;
        let Screen::World(world) = &mut self.screen else {
            return false;
        };
        // G-24 / SPEC-014 §3.17: `p`, `P`, Ctrl+P and Alt+P open the pause
        // window; it closes on any key except the brightness keys, which
        // adjust brightness and keep it open.
        if world.paused {
            match key {
                KeyCode::Equal | KeyCode::NumpadAdd => self.brightness_step(true),
                KeyCode::Minus | KeyCode::NumpadSubtract => self.brightness_step(false),
                _ => world.paused = false,
            }
            return true;
        }
        if world.pending.is_some() {
            return true;
        }
        let position = self.host.world.world_map.position();
        match key {
            KeyCode::KeyP => {
                world.paused = true;
                world.grey = Some((world.view, world.follow, world.hover));
            }
            KeyCode::KeyA => world.follow = !world.follow,
            KeyCode::KeyC => world.view = screen::centered(position),
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let direction = match key {
                    KeyCode::ArrowLeft => [-1, 0],
                    KeyCode::ArrowRight => [1, 0],
                    KeyCode::ArrowUp => [0, -1],
                    _ => [0, 1],
                };
                world.follow = false;
                world.view = screen::scroll(world.view, direction).0;
            }
            KeyCode::Home => {
                world.follow = false;
                world.view = screen::home(position);
            }
            KeyCode::End => {
                world.follow = false;
                world.view = screen::end(world.view);
            }
            KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
            KeyCode::F10 => self.confirm_quit(),
            KeyCode::F12 => self.screenshot(),
            KeyCode::Equal | KeyCode::NumpadAdd => self.brightness_step(true),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.brightness_step(false),
            // No key closes the world map; Esc does nothing.
            _ => {}
        }
        true
    }
    /// A press on the world map: a side-panel button sounds and waits for
    /// its release; the view acts on the release.
    pub(crate) fn world_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::World(world) = &mut self.screen else {
            return false;
        };
        // A button event closes the pause window.
        if world.paused {
            world.paused = false;
            world.swallow = !right;
            return true;
        }
        if right || world.pending.is_some() {
            return true;
        }
        if let Some(button) = panel_button(point) {
            world.held = Some(button);
            self.press();
        }
        true
    }
    /// A left release on the world map: the held side-panel button's event
    /// when the release is over it, else a view click.
    pub(crate) fn world_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::World(world) = &mut self.screen else {
            return false;
        };
        if std::mem::take(&mut world.swallow) || world.paused || world.pending.is_some() {
            return true;
        }
        if let Some(button) = world.held.take() {
            if point.is_some_and(|point| inside(panel_rect(button), point)) {
                if button == TOWN_WORLD {
                    self.world_town_world();
                } else {
                    self.world_town_button(button);
                }
            }
            return true;
        }
        let Some(point) = point.filter(|point| screen::view_click(*point)) else {
            return false;
        };
        let view = world.view;
        let map = &self.host.world.world_map;
        let travelling = map.travel.is_some();
        let party = screen::window(map.position(), view);
        if !travelling && screen::hovers(point, party) {
            self.play_effect(ENTER_SOUND, None);
            self.begin_entry(screen::ENTER_MS);
            return true;
        }
        self.play_effect(CLICK_SOUND, None);
        self.host.travel_to(screen::world_at(point, view));
        if let Screen::World(world) = &mut self.screen {
            world.follow = !travelling;
            world.journey = None;
        }
        true
    }
    /// One world-map pass, at most one per 41 milliseconds: edge scrolling,
    /// two travel iterations, the campaign checks, follow mode and the
    /// hover state. A pending entry or encounter flash holds the screen
    /// until it ends.
    pub(crate) fn world_tick(&mut self, now: u64) -> AppResult<bool> {
        let Screen::World(world) = &mut self.screen else {
            return Ok(false);
        };
        match world.pending {
            Some(Pending::Enter { until, town }) => {
                if now >= until {
                    world.pending = None;
                    self.enter_location(town);
                }
                return Ok(true);
            }
            Some(Pending::Flash { start, encounter }) => {
                if now >= start + screen::FLASH_IMAGES * screen::FLASH_MS {
                    world.pending = None;
                    let entered = self.host.enter_encounter(encounter);
                    self.leave_world(entered);
                }
                return Ok(true);
            }
            None => {}
        }
        if world.paused
            || world
                .last_pass
                .is_some_and(|last| now < last + screen::PASS_MS)
        {
            return Ok(false);
        }
        world.last_pass = Some(now);
        if let Some(at) = self.pointer.at {
            let direction = screen::edge(at);
            if direction != [0, 0] {
                world.follow = false;
                world.view = screen::scroll(world.view, direction).0;
            }
        }
        let mut travelled = false;
        for _ in 0..2 {
            let outcome = self.host.travel_iteration()?;
            let mark = self.host.world.world_map.mark;
            let Screen::World(world) = &mut self.screen else {
                return Ok(true);
            };
            if let Some(pixel) = mark {
                world.mark(pixel);
            }
            travelled |= outcome != TravelOutcome::Idle;
            match outcome {
                TravelOutcome::Moving => continue,
                TravelOutcome::Idle => break,
                TravelOutcome::Arrived | TravelOutcome::Blocked => {
                    // The party stops at rest; a town is never entered
                    // automatically, and a hotspot chosen before the
                    // journey is not used.
                    world.clear_dots();
                    world.journey = None;
                    break;
                }
                TravelOutcome::Interrupted(_) => {
                    world.clear_dots();
                    let town = self.party_town();
                    self.enter_location(town);
                    return Ok(true);
                }
                TravelOutcome::Encounter(encounter) => {
                    world.clear_dots();
                    world.journey = None;
                    world.view = screen::centered(self.host.world.world_map.position());
                    world.pending = Some(Pending::Flash {
                        start: now,
                        encounter,
                    });
                    return Ok(true);
                }
            }
        }
        match self.host.world_map_checks()? {
            Some(WorldMapExit::Quit) => {
                self.quit_requested = true;
                self.return_to_map();
                return Ok(true);
            }
            Some(WorldMapExit::Vault13) => {
                let entered = self.host.enter_vault13_after_explosion();
                self.leave_world(entered);
                return Ok(true);
            }
            None => {}
        }
        let map = &self.host.world.world_map;
        let (position, at_rest) = (map.position(), map.travel.is_none());
        let pointer = self.pointer.at;
        let Screen::World(world) = &mut self.screen else {
            return Ok(true);
        };
        if world.follow {
            world.view = screen::centered(position);
        }
        let party = screen::window(position, world.view);
        world.hover = at_rest && pointer.is_some_and(|at| screen::hovers(at, party));
        // §3.17: a pass that redraws the view ends the pause's grey.
        if let Some((view, follow, hover)) = world.grey
            && (world.follow
                || travelled
                || view != world.view
                || follow != world.follow
                || hover != world.hover)
        {
            world.grey = None;
        }
        Ok(true)
    }
    /// The world-map cursor: the scroll art on an edge (the blocked art
    /// when every requested axis is at its limit), else the arrow.
    pub(crate) fn world_cursor(&self, at: [i32; 2]) -> &'static str {
        let Screen::World(world) = &self.screen else {
            return cursor::ARROW.art;
        };
        if world.pending.is_some() || world.paused {
            return cursor::ARROW.art;
        }
        let direction = screen::edge(at);
        let Some(index) = screen::cursor_index(direction) else {
            return cursor::ARROW.art;
        };
        if screen::scroll(world.view, direction).1 {
            cursor::SCROLL_BLOCKED[index].art
        } else {
            cursor::SCROLL[index].art
        }
    }
    /// The world art under the view, darkened by discovery cell: unknown
    /// cells black, seen cells through intensity level 64 (indices 0 and
    /// 229-255 unchanged), visited cells unchanged.
    fn draw_world_view(&self, painter: &Painter, image: &mut IndexedImage, view: [i32; 2]) {
        painter.blit_region(
            image,
            WORLD_ART,
            Rect::new(view[0], view[1], VIEWPORT_SIZE[0], VIEWPORT_SIZE[1]),
            VIEWPORT_ORIGIN,
            true,
        );
        let black = painter.color(ColorKey::BLACK);
        let dim: Vec<u8> = (0..=255_u8)
            .map(|index| {
                if index == 0 || index >= 229 {
                    index
                } else {
                    self.palette.intensity(index, 64)
                }
            })
            .collect();
        let map = &self.host.world.world_map;
        let stride = image.width() as i32;
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let Some(rect) = screen::cell_rect([column, row], view) else {
                    continue;
                };
                match map.discovery([column, row]) {
                    0 => image.fill_rect(rect, black),
                    1 => {
                        let pixels = image.pixels_mut();
                        for y in rect[1]..rect[1] + rect[3] {
                            for x in rect[0]..rect[0] + rect[2] {
                                let pixel = &mut pixels[(y * stride + x) as usize];
                                *pixel = dim[usize::from(*pixel)];
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    /// A marker, skipped outside its drawing window and clipped to the view.
    fn draw_marker(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        art: &str,
        marker: Marker,
        at: [i32; 2],
    ) {
        let origin = marker.origin(at);
        if !marker.shown(origin) {
            return;
        }
        if let Some(art) = painter.art(art)
            && let Some(frame) = art.frame(0, 0)
        {
            image.blit_within(frame, origin.map(i64::from), VIEW_CLIP);
        }
    }
    /// The hover label (SPEC-014 §3.39): a town's name, "Unknown", or the
    /// cell's terrain, centered over the triangle in font 101, key 992,
    /// with shadow; only its w-by-10 rectangle is written.
    fn draw_hover_label(&self, painter: &Painter, image: &mut IndexedImage, party: [i32; 2]) {
        let map = &self.host.world.world_map;
        let Some(cell) = map.cell() else {
            return;
        };
        let text = match worldmap::town_at(cell) {
            Some(town) => {
                let known = self
                    .host
                    .world
                    .gvars
                    .as_ref()
                    .and_then(|gvars| gvars.get(TOWNS[town].known_global))
                    .is_some_and(|value| *value == 1);
                if known || map.town_enabled(town) {
                    self.message("map.msg", 500 + town as i32)
                } else {
                    self.message("worldmap.msg", 1004)
                }
            }
            None => self.message("worldmap.msg", 1000 + worldmap::terrain(cell) as i32),
        };
        let width = painter.width(screen::LABEL_FONT, &text);
        let origin = screen::label_origin(party, width);
        if width <= 0 || !screen::label_shown(origin, width) {
            return;
        }
        self.draw_label(painter, image, &text, origin, width, VIEW_CLIP);
    }
    /// Text in font 101, key 992, with shadow, written only within its
    /// w-by-10 rectangle at `origin` and within `clip`.
    fn draw_label(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        text: &[u8],
        origin: [i32; 2],
        width: i32,
        clip: [i64; 4],
    ) {
        let Some(font) = painter.font(screen::LABEL_FONT) else {
            return;
        };
        let Ok(mut label) = IndexedImage::new(width.max(1) as u32, 10) else {
            return;
        };
        label.draw_text(
            &font,
            &self.palette,
            text,
            [0, 0],
            width as u32,
            TextStyle {
                foreground: painter.color(ColorKey(screen::LABEL_KEY)),
                shadow: true,
                ..Default::default()
            },
        );
        let stride = image.width() as i64;
        let height = i64::from(image.height());
        let pixels = image.pixels_mut();
        for (index, color) in label.pixels().iter().enumerate() {
            let (x, y) = (
                i64::from(origin[0]) + index as i64 % i64::from(width),
                i64::from(origin[1]) + index as i64 / i64::from(width),
            );
            if *color != 0
                && (clip[0]..clip[0] + clip[2]).contains(&x)
                && (clip[1]..clip[1] + clip[3]).contains(&y)
                && (0..stride).contains(&x)
                && (0..height).contains(&y)
            {
                pixels[(y * stride + x) as usize] = *color;
            }
        }
    }
    /// The side panel: the twelve red buttons (top 15 rows), the name
    /// plates of the towns in `names`, the Town/World button, and the date.
    fn draw_panel(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        names: u32,
        held: Option<usize>,
    ) {
        let pointer = self.pointer.at;
        let down = |button: usize| {
            held == Some(button) && pointer.is_some_and(|at| inside(panel_rect(button), at))
        };
        for button in (0..TOWNS.len()).chain([TOWN_WORLD]) {
            let rect = panel_rect(button);
            let art = if down(button) {
                BUTTON_DOWN_ART
            } else {
                BUTTON_UP_ART
            };
            painter.blit_region(
                image,
                art,
                Rect::new(0, 0, rect[2], rect[3]),
                [rect[0], rect[1]],
                false,
            );
        }
        for (town, y) in screen::TOWN_BUTTON_Y.iter().enumerate() {
            if names & (1 << town) != 0 {
                let [w, h] = screen::TOWN_NAME_SIZE;
                painter.blit_region(
                    image,
                    NAMES_ART,
                    Rect::new(0, h * town as i32, w, h),
                    [screen::TOWN_NAME_X, *y],
                    false,
                );
            }
        }
        self.draw_date(painter, image);
    }
    /// The date and time from the game clock (SPEC-005 calendar), written
    /// opaque: day, month row, year, and HHMM.
    fn draw_date(&self, painter: &Painter, image: &mut IndexedImage) {
        let world = &self.host.world;
        let [month, day, year] = vault_core::host_save::game_date(world.clock.ticks);
        let digits = |image: &mut IndexedImage, value: i32, count: u32, origin: [i32; 2]| {
            for k in 0..count {
                let digit = value / 10_i32.pow(count - 1 - k) % 10;
                painter.blit_region(
                    image,
                    DIGITS_ART,
                    Rect::new(
                        screen::DIGIT[0] * digit,
                        0,
                        screen::DIGIT[0],
                        screen::DIGIT[1],
                    ),
                    [origin[0] + screen::DIGIT[0] * k as i32, origin[1]],
                    true,
                );
            }
        };
        digits(image, day as i32, 2, screen::DAY);
        let [w, h] = screen::MONTH_SIZE;
        painter.blit_region(
            image,
            MONTHS_ART,
            Rect::new(0, 15 * (month as i32 - 1), w, h),
            screen::MONTH,
            true,
        );
        digits(image, year as i32, 4, screen::YEAR);
        digits(image, world.clock.hhmm(), 4, screen::TIME);
    }
    pub(crate) fn world_image(&self) -> AppResult<IndexedImage> {
        let Screen::World(world) = &self.screen else {
            return Err("world map screen is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        let map = &self.host.world.world_map;
        let position = map.position();
        // An encounter recenters the view and flashes without markers; an
        // entry shows the pressed triangle without recentering.
        let view = match world.pending {
            Some(Pending::Flash { .. }) => screen::centered(position),
            _ => world.view,
        };
        self.draw_world_view(&painter, &mut image, view);
        let party = screen::window(position, view);
        match world.pending {
            None => {
                if let Some(travel) = map.travel {
                    self.draw_marker(&painter, &mut image, PLUS_ART, Marker::Plus, party);
                    let target = screen::window(travel.destination, view);
                    self.draw_marker(&painter, &mut image, TARGET_ART, Marker::Crosshair, target);
                    let dot = painter.color(ColorKey(screen::DOT_KEY));
                    let stride = image.width() as i32;
                    let pixels = image.pixels_mut();
                    for y in 0..VIEWPORT_SIZE[1] {
                        for x in 0..VIEWPORT_SIZE[0] {
                            if world.marked([view[0] + x, view[1] + y]) {
                                let at = (VIEWPORT_ORIGIN[1] + y) * stride + VIEWPORT_ORIGIN[0] + x;
                                pixels[at as usize] = dot;
                            }
                        }
                    }
                } else {
                    self.draw_marker(&painter, &mut image, TRIANGLE_ART, Marker::Triangle, party);
                    if world.hover {
                        self.draw_hover_label(&painter, &mut image, party);
                    }
                }
            }
            Some(Pending::Enter { .. }) => {
                self.draw_marker(
                    &painter,
                    &mut image,
                    PRESSED_TRIANGLE_ART,
                    Marker::Triangle,
                    party,
                );
                if self.pointer.at.is_some_and(|at| screen::hovers(at, party)) {
                    self.draw_hover_label(&painter, &mut image, party);
                }
            }
            Some(Pending::Flash { start, encounter }) => {
                let image_index = self.clock.saturating_sub(start) / screen::FLASH_MS;
                let second = image_index % 2 == 1;
                match encounter {
                    Encounter::Special { .. } => {
                        let art = if second { TARGET_ART } else { DARK_TARGET_ART };
                        self.draw_marker(&painter, &mut image, art, Marker::Crosshair, party);
                    }
                    Encounter::Terrain { .. } => {
                        let art = BOLT_ART[usize::from(second)];
                        self.draw_marker(&painter, &mut image, art, Marker::Bolt, party);
                    }
                }
            }
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        self.draw_panel(&painter, &mut image, world.names, world.held);
        // G-24 / SPEC-014 §3.17: the world-map region (22, 21) 450 by 442 is
        // converted through the grey lookup table, then the pause window
        // shows at (185, 180). The conversion is not undone when it closes:
        // the region stays grey until the screen next redraws its view.
        if world.paused || world.grey.is_some() {
            let grey = &self.grey;
            let [x, y] = VIEWPORT_ORIGIN;
            let [w, h] = VIEWPORT_SIZE;
            let stride = image.width() as i32;
            for row in y..y + h {
                for column in x..x + w {
                    if let Some(pixel) =
                        image.pixels_mut().get_mut((row * stride + column) as usize)
                    {
                        *pixel = grey[usize::from(*pixel)];
                    }
                }
            }
        }
        if world.paused {
            super::options::draw_pause(
                &painter,
                &mut image,
                vault_core::ui::options::PAUSE_WORLD_ORIGIN,
            );
        }
        Ok(image)
    }

    /// The town-map marker whose triangle pixels are under `point`: only
    /// the opaque pixels of `hotspot1.frm` count (the whole 25-by-13 image
    /// when the art is missing).
    fn marker_at(&self, town: &TownScreen, point: [i32; 2]) -> Option<usize> {
        let art = self.painter().art(TRIANGLE_ART);
        let frame = art.as_ref().and_then(|art| art.frame(0, 0));
        town.buttons.iter().position(|button| {
            let [x, y] = screen::marker_triangle(button.rect);
            let local = [point[0] - x, point[1] - y];
            let [w, h] = screen::MARKER_SIZE;
            if !(0..w).contains(&local[0]) || !(0..h).contains(&local[1]) {
                return false;
            }
            frame.is_none_or(|frame| {
                let width = i32::from(frame.width);
                local[0] < width
                    && local[1] < i32::from(frame.height)
                    && frame.pixels()[(local[1] * width + local[0]) as usize] != 0
            })
        })
    }
    pub(crate) fn town_key(&mut self, key: KeyCode) -> bool {
        let [_, control, _] = self.hud.modifiers;
        match key {
            KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
            KeyCode::F10 => self.confirm_quit(),
            KeyCode::F12 => self.screenshot(),
            KeyCode::Equal | KeyCode::NumpadAdd => self.brightness_step(true),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.brightness_step(false),
            // No Esc, no close button and no pause key.
            _ => {}
        }
        true
    }
    /// A press on the town map: a marker plays `ib2p1xx1`, a side-panel
    /// button its red-button cues; each acts on its release.
    pub(crate) fn town_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::Town(town) = &self.screen else {
            return false;
        };
        if right {
            return true;
        }
        if let Some(index) = self.marker_at(town, point) {
            if let Screen::Town(town) = &mut self.screen {
                town.pressed = Some(index);
            }
            self.play_effect(ENTER_SOUND, None);
            return true;
        }
        if let Some(button) = panel_button(point) {
            if let Screen::Town(town) = &mut self.screen {
                town.held = Some(button);
            }
            self.press();
        }
        true
    }
    pub(crate) fn town_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Screen::Town(town) = &mut self.screen else {
            return false;
        };
        let (pressed, held) = (town.pressed.take(), town.held.take());
        let Some(point) = point else {
            return pressed.is_some() || held.is_some();
        };
        if let Some(index) = pressed {
            let over = match &self.screen {
                Screen::Town(town) => self.marker_at(town, point) == Some(index),
                _ => false,
            };
            if over {
                self.town_choice(index);
            }
            return true;
        }
        let Some(button) = held else {
            return false;
        };
        if !inside(panel_rect(button), point) {
            return true;
        }
        if button == TOWN_WORLD {
            self.town_world_button();
        } else if self.host.world.world_map.town_enabled(button) {
            // A known town's button shows its picture and markers, without
            // travelling or changing the current town.
            let Screen::Town(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
                return true;
            };
            let TownScreen { origin, world, .. } = town;
            match self.town_screen(button as i32, origin, world) {
                Ok(next) => self.screen = Screen::Town(next),
                Err(world) => {
                    // The picture failed: the town map stays as it was.
                    if let Ok(same) = self.town_screen(town.town as i32, origin, world) {
                        self.screen = Screen::Town(same);
                    }
                }
            }
        }
        true
    }
    /// The town map's Town/World button returns "World map": the world map
    /// follows at rest, with follow mode off when this town map was opened
    /// by the Town/World button and on otherwise (SPEC-014 §3.39).
    fn town_world_button(&mut self) {
        let Screen::Town(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        self.host.world.world_map.travel = None;
        match town.world {
            Some(world) => self.return_to_world(*world, town.origin != TownOrigin::TownWorld),
            None => self.open_world_map_with_music(),
        }
    }
    /// A chosen entrance (SPEC-014 §3.40): in the town whose cell the party
    /// is in, the town becomes the current town and the slot's map loads
    /// by name with its selector; elsewhere the world map takes over with
    /// the party travelling to that town's jittered center (SPEC-013 §3.22).
    fn town_choice(&mut self, index: usize) {
        let Screen::Town(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let Some(button) = town.buttons.get(index).copied() else {
            self.screen = Screen::Town(town);
            return;
        };
        if self.host.party_in_town(town.town) {
            let loaded = self.host.select_town_entrance(town.town, button);
            self.leave_world(loaded);
            return;
        }
        self.host.world.world_map.travel = None;
        let opening = town.world.is_none();
        let world = town.world.map_or_else(
            || WorldScreen::new(self.host.world.world_map.position()),
            |world| *world,
        );
        self.host.play_world_map_music();
        self.show_world(world, opening);
        if self.travel_to_town(town.town)
            && let Screen::World(world) = &mut self.screen
        {
            world.follow = true;
            world.journey = Some(town.town);
        }
    }
    pub(crate) fn town_image(&self) -> AppResult<IndexedImage> {
        let Screen::Town(town) = &self.screen else {
            return Err("town map screen is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        if let Some(picture) = screen::town_picture(town.town) {
            painter.blit_opaque(&mut image, &picture, 0, screen::TOWN_PICTURE);
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        self.draw_panel(&painter, &mut image, self.known_towns(), town.held);
        let hovered = self.pointer.at.and_then(|at| self.marker_at(town, at));
        for (index, button) in town.buttons.iter().enumerate() {
            let pressed = town.pressed == Some(index) && hovered == Some(index);
            let art = if pressed {
                PRESSED_TRIANGLE_ART
            } else {
                TRIANGLE_ART
            };
            painter.blit(&mut image, art, 0, screen::marker_triangle(button.rect));
            if hovered == Some(index) {
                let text = self.message("worldmap.msg", button.label);
                let width = painter.width(screen::LABEL_FONT, &text);
                if width > 0 {
                    let [x, y, w, h] = button.rect;
                    let origin = [x + screen::marker_label_x(width), y];
                    let clip = [x, y, w, h].map(i64::from);
                    self.draw_label(&painter, &mut image, &text, origin, width.min(w), clip);
                }
            }
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests;
