//! SPEC-019 §3.12 world map screen and §3.13 town map screen under the
//! Fallout 2 profile, over SPEC-018's world-map and city tables and its §5
//! rules (the host's `fallout2_*` calls). The world is SPEC-018 §5.3's grid
//! of 350-by-300 tiles, each drawing its own `intrface.lst` row, darkened
//! by subtile; known cities are circles with their names; the side panel
//! holds the date, the day and night dial, a seven-row town list with its
//! scroll arrows, the fuel gauge while the party drives, and the
//! Town/World button. The town map shows a city's picture with one
//! triangle marker per known entrance.
//!
//! What SPEC-014 §3.39-§3.40 fix and SPEC-019 keeps (opening, follow
//! mode, scrolling, keys, travel dots, the pressed triangle's wait, the
//! encounter flash, the pause window) follows the Fallout 1 screen
//! (`world.rs`), with the same `[May-improve]` choices: markers and labels
//! clipped to the view, the whole frame drawn on every pass, and a town map
//! that fails leaving the world map open.
use super::{
    MapPreview, Screen,
    world::{TownOrigin, VIEW_CLIP},
};
use crate::{AppResult, interface::Painter};
use vault_core::{
    Fallout2Travel,
    fallout2::{self, Fallout2World, Selected},
    ui::{
        ColorKey, ERROR_SOUND, Rect, cursor,
        fallout2::{self as ui2, TownList, panel, view},
    },
    worldmap::{
        VIEWPORT_ORIGIN, VIEWPORT_SIZE,
        screen::{self, Marker},
    },
};
use vault_formats::fallout2::worldmap::TILE_SIZE;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const FRAME_ART: &str = "wmapbox.frm";
const PLUS_ART: &str = "wmaploc.frm";
const TARGET_ART: &str = "wmaptarg.frm";
const DARK_TARGET_ART: &str = "wmaptrg2.frm";
const BOLT_ART: [&str; 2] = ["wmapfgt0.frm", "wmapfgt1.frm"];
const TRIANGLE_ART: &str = "hotspot1.frm";
const PRESSED_TRIANGLE_ART: &str = "hotspot2.frm";
const BUTTON_UP_ART: &str = "lilredup.frm";
const BUTTON_DOWN_ART: &str = "lilreddn.frm";
const ENTER_SOUND: &[u8] = b"ib2p1xx1";
const CLICK_SOUND: &[u8] = b"ib1p1xx1";
/// SPEC-019 §3.12.2 "Unknown" and the terrain-name fallback.
const UNKNOWN: i32 = 1004;

/// A side-panel control of the Fallout 2 screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelButton {
    /// Town-list row k (0-6).
    Row(usize),
    Up,
    Down,
    TownWorld,
}
impl PanelButton {
    fn rect(self) -> Rect {
        match self {
            Self::Row(k) => panel::row_button(k),
            Self::Up => panel::LIST_UP,
            Self::Down => panel::LIST_DOWN,
            Self::TownWorld => panel::TOWN_WORLD,
        }
    }
    fn at(point: [i32; 2]) -> Option<Self> {
        (0..panel::ROWS)
            .map(Self::Row)
            .chain([Self::Up, Self::Down, Self::TownWorld])
            .find(|button| button.rect().contains(point))
    }
}

/// A timed step that holds the world map until it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending2 {
    /// The pressed triangle shows until `until`; then the party enters
    /// `city`, or drops to terrain outside every city.
    Enter { until: u64, city: Option<usize> },
    /// SPEC-019 §3.12.7 the encounter flash from `start`; then the map.
    Flash { start: u64, selected: Selected },
}

pub(crate) struct World2Screen {
    pub view: [i32; 2],
    pub follow: bool,
    pub paused: bool,
    pub grey: Option<([i32; 2], bool, bool)>,
    pub hover: bool,
    /// Travel dots by world pixel.
    dots: Vec<u64>,
    width: i32,
    /// The city a journey chosen on a town map heads for.
    pub journey: Option<usize>,
    /// SPEC-019 §2.4 the town-list scroll offset (not saved).
    pub list: TownList,
    last_pass: Option<u64>,
    held: Option<PanelButton>,
    swallow: bool,
    pub pending: Option<Pending2>,
}
pub(crate) struct Town2Screen {
    pub city: usize,
    pub origin: TownOrigin,
    /// The entrance slots with markers, and their anchors.
    pub markers: Vec<(usize, [i32; 2])>,
    pressed: Option<usize>,
    held: Option<PanelButton>,
    list: TownList,
    world: Option<Box<World2Screen>>,
}

impl World2Screen {
    fn new(state: &Fallout2World) -> Self {
        let size = state.world_size();
        let pixels = (size[0].max(1) as usize) * (size[1].max(1) as usize);
        Self {
            view: view::centered(state.position, size),
            follow: true,
            paused: false,
            grey: None,
            hover: false,
            dots: vec![0; pixels / 64 + 1],
            width: size[0].max(1),
            journey: None,
            list: TownList::default(),
            last_pass: None,
            held: None,
            swallow: false,
            pending: None,
        }
    }
    fn dot_index(&self, pixel: [i32; 2]) -> Option<usize> {
        let height = (self.dots.len() * 64) as i32 / self.width;
        ((0..self.width).contains(&pixel[0]) && (0..height).contains(&pixel[1]))
            .then(|| (pixel[1] * self.width + pixel[0]) as usize)
            .filter(|index| *index / 64 < self.dots.len())
    }
    fn mark(&mut self, pixel: [i32; 2]) {
        if let Some(index) = self.dot_index(pixel) {
            self.dots[index / 64] |= 1 << (index % 64);
        }
    }
    pub fn marked(&self, pixel: [i32; 2]) -> bool {
        self.dot_index(pixel)
            .is_some_and(|index| self.dots[index / 64] & (1 << (index % 64)) != 0)
    }
    fn clear_dots(&mut self) {
        self.dots.fill(0);
    }
    pub fn swallow_release(&mut self) {
        self.swallow = true;
    }
}

impl MapPreview {
    fn state2(&self) -> Option<&Fallout2World> {
        self.host.world.fallout2.as_deref()
    }
    /// §3.12.3 the town list: every known city, in city-table order.
    pub(crate) fn known_cities(&self) -> Vec<usize> {
        let Some(state) = self.state2() else {
            return Vec::new();
        };
        state
            .tables
            .city
            .areas()
            .map(|area| area.number as usize)
            .filter(|city| state.areas.get(*city).is_some_and(|area| area.known >= 1))
            .collect()
    }
    /// The city whose circle holds the party, known or not.
    fn party_city(&self) -> Option<usize> {
        let state = self.state2()?;
        state.area_at(state.position)
    }
    fn city_name(&self, city: usize) -> Vec<u8> {
        self.state2()
            .and_then(|state| state.tables.city.get(city as i32))
            .map(|area| area.name.clone().into_bytes())
            .unwrap_or_default()
    }
    /// SPEC-019 §3.12: open the Fallout 2 world map, built with W and Q
    /// and its music, the party at rest with follow mode on.
    pub(crate) fn open_world2(&mut self) {
        if let Some(state) = self.host.world.fallout2.as_deref_mut() {
            state.travel = None;
            state.mark = None;
        }
        let Some(state) = self.state2() else {
            return;
        };
        let world = World2Screen::new(state);
        self.show_world2(world, true);
    }
    fn show_world2(&mut self, mut world: World2Screen, opening: bool) {
        if let Err(error) = self.host.fallout2_build_world_map(opening) {
            self.status = error.to_string().into_bytes();
        }
        world.clear_dots();
        world.list.clamp(self.known_cities().len());
        world.last_pass = None;
        world.held = None;
        world.pending = None;
        world.hover = false;
        self.screen = Screen::World2(Box::new(world));
    }
    fn close_world2_to_map(&mut self) {
        if let Some(state) = self.host.world.fallout2.as_deref_mut() {
            state.travel = None;
            state.mark = None;
        }
        self.return_to_map();
    }
    fn leave_world2(&mut self, loaded: Result<(), vault_core::WorldError>) {
        match loaded {
            Ok(()) => self.close_world2_to_map(),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// SPEC-019 §3.13 the town map of `city`: its picture (a failure when
    /// it does not load) and a marker per known entrance.
    fn town2_screen(
        &mut self,
        city: usize,
        origin: TownOrigin,
        world: Option<Box<World2Screen>>,
        list: TownList,
    ) -> Result<Town2Screen, Option<Box<World2Screen>>> {
        let Some(picture) = self.town_picture(city) else {
            return Err(world);
        };
        if self.painter().art(&picture).is_none() {
            return Err(world);
        }
        let markers = self.town_markers(city);
        Ok(Town2Screen {
            city,
            origin,
            markers,
            pressed: None,
            held: None,
            list,
            world,
        })
    }
    fn town_picture(&self, city: usize) -> Option<String> {
        let row = self.state2()?.tables.city.get(city as i32)?.townmap_art;
        self.painter().interface_row(row)
    }
    /// §3.13 each entrance whose state is on, by slot, with its anchor:
    /// the entrance's (x, y) on the picture, which sits at (20, 20).
    fn town_markers(&self, city: usize) -> Vec<(usize, [i32; 2])> {
        let Some(state) = self.state2() else {
            return Vec::new();
        };
        let Some(area) = state.tables.city.get(city as i32) else {
            return Vec::new();
        };
        let known = state.areas.get(city).map(|area| area.entrances.clone());
        area.entrances
            .iter()
            .enumerate()
            .filter_map(|(slot, entrance)| {
                let entrance = entrance.as_ref()?;
                known
                    .as_ref()
                    .and_then(|known| known.get(slot))
                    .copied()
                    .unwrap_or(false)
                    .then_some((
                        slot,
                        [
                            ui2::TOWN_PICTURE[0] + entrance.marker[0],
                            ui2::TOWN_PICTURE[1] + entrance.marker[1],
                        ],
                    ))
            })
            .collect()
    }
    /// The current city's town map (a transition to map -1); a failure
    /// opens the world map instead.
    pub(crate) fn open_town2(&mut self) {
        let city = self.state2().map_or(-1, |state| state.area);
        let opened = usize::try_from(city)
            .ok()
            .map(|city| self.town2_screen(city, TownOrigin::Transition, None, TownList::default()));
        match opened {
            Some(Ok(town)) => self.screen = Screen::Town2(Box::new(town)),
            _ => {
                self.host.play_world_map_music();
                self.open_world2();
            }
        }
    }
    fn open_town2_from_world(&mut self, city: usize, origin: TownOrigin, world: Box<World2Screen>) {
        let list = world.list;
        match self.town2_screen(city, origin, Some(world), list) {
            Ok(town) => self.screen = Screen::Town2(Box::new(town)),
            Err(Some(world)) => self.screen = Screen::World2(world),
            Err(None) => {}
        }
    }
    /// §3.12.4 entering city `city` (or dropping to terrain for `None`).
    fn enter_location2(&mut self, city: Option<usize>) {
        let Some(city) = city else {
            match self.host.fallout2_drop_to_terrain() {
                Ok(true) => self.close_world2_to_map(),
                Ok(false) => {}
                Err(error) => self.status = error.to_string().into_bytes(),
            }
            return;
        };
        match self.host.fallout2_enter_area(city) {
            Ok(fallout2::AreaEntry::Direct(_)) => self.close_world2_to_map(),
            Ok(fallout2::AreaEntry::TownMap) => {
                let Screen::World2(world) = std::mem::replace(&mut self.screen, Screen::Map) else {
                    return;
                };
                self.open_town2_from_world(city, TownOrigin::Entered, world);
            }
            Ok(fallout2::AreaEntry::Refused) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    fn begin_entry2(&mut self, wait: u64) {
        let city = self.party_city();
        let until = self.clock + wait;
        if let Some(state) = self.host.world.fallout2.as_deref_mut() {
            state.travel = None;
        }
        if let Screen::World2(world) = &mut self.screen {
            world.clear_dots();
            world.pending = Some(Pending2::Enter { until, city });
        }
    }
    /// §3.12.3 travel toward a city's jittered position.
    fn travel_to_city(&mut self, city: usize) -> bool {
        let Some(position) = self
            .state2()
            .and_then(|state| state.areas.get(city))
            .map(|area| area.position)
        else {
            return false;
        };
        let target = fallout2::city_target(position, &mut self.host.world.rng)
            .and_then(|target| self.host.fallout2_travel_to(target).map(|()| target));
        match target {
            Ok(_) => true,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                false
            }
        }
    }
    fn city_locked(&self, city: usize) -> bool {
        self.state2()
            .and_then(|state| state.areas.get(city))
            .is_some_and(|area| area.locked)
    }
    /// §3.12.3 a town-list row's release: travel to a city the party is
    /// not in, enter the one it is in; a locked city does nothing.
    fn world2_row(&mut self, k: usize) {
        let cities = self.known_cities();
        let Screen::World2(world) = &self.screen else {
            return;
        };
        let Some(city) = world.list.entry(k, cities.len()).map(|entry| cities[entry]) else {
            return;
        };
        if self.city_locked(city) {
            return;
        }
        let travelling = self.state2().and_then(|state| state.travel);
        if self.party_city() != Some(city) {
            if self.travel_to_city(city)
                && let Screen::World2(world) = &mut self.screen
            {
                world.follow = travelling.is_none();
                world.journey = None;
            }
            return;
        }
        self.begin_entry2(if travelling.is_some() {
            screen::ENTER_TRAVELLING_MS
        } else {
            screen::ENTER_MS
        });
    }
    /// The Town/World button: the journey stops, and the town map of the
    /// journey's city (or the current city) opens.
    fn world2_town_world(&mut self) {
        let Screen::World2(mut world) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let travelling = self
            .host
            .world
            .fallout2
            .as_deref_mut()
            .and_then(|state| state.travel.take())
            .is_some();
        let current = self.state2().map_or(-1, |state| state.area);
        let city = world
            .journey
            .filter(|_| travelling)
            .or_else(|| usize::try_from(current).ok());
        world.journey = None;
        world.clear_dots();
        match city {
            Some(city) => self.open_town2_from_world(city, TownOrigin::TownWorld, world),
            None => self.screen = Screen::World2(world),
        }
    }
    pub(crate) fn world2_key(&mut self, key: KeyCode) -> bool {
        let [_, control, _] = self.hud.modifiers;
        let Some(size) = self.state2().map(Fallout2World::world_size) else {
            return false;
        };
        let position = self.state2().map_or([0, 0], |state| state.position);
        let Screen::World2(world) = &mut self.screen else {
            return false;
        };
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
        match key {
            KeyCode::KeyP => {
                world.paused = true;
                world.grey = Some((world.view, world.follow, world.hover));
            }
            KeyCode::KeyA => world.follow = !world.follow,
            KeyCode::KeyC => world.view = view::centered(position, size),
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let direction = match key {
                    KeyCode::ArrowLeft => [-1, 0],
                    KeyCode::ArrowRight => [1, 0],
                    KeyCode::ArrowUp => [0, -1],
                    _ => [0, 1],
                };
                let (next, blocked) = view::scroll(world.view, direction, size);
                if !blocked {
                    world.follow = false;
                    world.view = next;
                }
            }
            KeyCode::Home => {
                world.follow = false;
                world.view = view::home(position, size);
            }
            KeyCode::End => {
                world.follow = false;
                world.view = view::end(world.view, size);
            }
            KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
            KeyCode::F10 => self.confirm_quit(),
            KeyCode::F12 => self.screenshot(),
            KeyCode::Equal | KeyCode::NumpadAdd => self.brightness_step(true),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.brightness_step(false),
            // No key closes the world map.
            _ => {}
        }
        true
    }
    pub(crate) fn world2_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::World2(world) = &mut self.screen else {
            return false;
        };
        if world.paused {
            world.paused = false;
            world.swallow = !right;
            return true;
        }
        if !right {
            world.swallow = false;
        }
        if right || world.pending.is_some() {
            return true;
        }
        if let Some(button) = PanelButton::at(point) {
            world.held = Some(button);
            self.press();
        }
        true
    }
    pub(crate) fn world2_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let count = self.known_cities().len();
        let Screen::World2(world) = &mut self.screen else {
            return false;
        };
        if std::mem::take(&mut world.swallow) || world.paused || world.pending.is_some() {
            return true;
        }
        if let Some(button) = world.held.take() {
            if point.is_some_and(|point| button.rect().contains(point)) {
                match button {
                    PanelButton::Up => {
                        world.list.up();
                    }
                    PanelButton::Down => {
                        world.list.down(count);
                    }
                    PanelButton::TownWorld => self.world2_town_world(),
                    PanelButton::Row(k) => self.world2_row(k),
                }
            }
            return true;
        }
        let Some(point) = point.filter(|point| screen::view_click(*point)) else {
            return false;
        };
        let view = world.view;
        let Some(state) = self.state2() else {
            return true;
        };
        let travelling = state.travel.is_some();
        let party = screen::window(state.position, view);
        if !travelling && screen::hovers(point, party) {
            self.play_effect(ENTER_SOUND, None);
            // §3.12.2: a locked city cannot be entered (the sound plays).
            if self.party_city().is_some_and(|city| self.city_locked(city)) {
                return true;
            }
            self.begin_entry2(screen::ENTER_MS);
            return true;
        }
        self.play_effect(CLICK_SOUND, None);
        if let Err(error) = self.host.fallout2_travel_to(screen::world_at(point, view)) {
            self.status = error.to_string().into_bytes();
        }
        if let Screen::World2(world) = &mut self.screen {
            world.follow = !travelling;
            world.journey = None;
        }
        true
    }
    /// SPEC-019 §3.19.3 the tank ran dry: a large Done box at (169, 116)
    /// with E-fuel-out in key 32328, and `iisxxxx1`.
    fn out_of_fuel(&mut self) {
        self.play_effect(ERROR_SOUND.as_bytes(), None);
        let dialog = super::dialog::Dialog::done(
            [169, 116],
            ui2::text::FUEL_OUT.as_bytes().to_vec(),
            Vec::new(),
            ColorKey::ERROR,
        );
        self.show_dialog(dialog);
    }
    /// One world-map pass, as SPEC-014 §3.39: edge scrolling, two travel
    /// iterations, follow mode and the hover state; a pending entry or
    /// encounter flash holds the screen until it ends.
    pub(crate) fn world2_tick(&mut self, now: u64) -> AppResult<bool> {
        let Some(size) = self.state2().map(Fallout2World::world_size) else {
            return Ok(false);
        };
        let Screen::World2(world) = &mut self.screen else {
            return Ok(false);
        };
        match world.pending {
            Some(Pending2::Enter { until, city }) => {
                if now >= until {
                    world.pending = None;
                    self.enter_location2(city);
                }
                return Ok(true);
            }
            Some(Pending2::Flash { start, selected }) => {
                if now >= start + screen::FLASH_IMAGES * screen::FLASH_MS {
                    world.pending = None;
                    let entered = self.host.fallout2_enter_encounter(selected);
                    self.leave_world2(entered);
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
        if let Some(at) = self.pointer.at.filter(|_| !self.pointer.outside) {
            let direction = screen::edge(at);
            if direction != [0, 0] {
                world.follow = false;
                world.view = view::scroll(world.view, direction, size).0;
            }
        }
        let mut travelled = false;
        for _ in 0..2 {
            let outcome = self.host.fallout2_travel_iteration()?;
            let mark = self.state2().and_then(|state| state.mark);
            let Screen::World2(world) = &mut self.screen else {
                return Ok(true);
            };
            if let Some(pixel) = mark {
                world.mark(pixel);
            }
            travelled |= outcome != Fallout2Travel::Idle;
            match outcome {
                Fallout2Travel::Moving => continue,
                Fallout2Travel::Idle => break,
                Fallout2Travel::Arrived | Fallout2Travel::Blocked => {
                    world.clear_dots();
                    world.journey = None;
                    break;
                }
                Fallout2Travel::OutOfFuel => {
                    world.clear_dots();
                    world.journey = None;
                    self.out_of_fuel();
                    break;
                }
                Fallout2Travel::Interrupted(_) => {
                    world.clear_dots();
                    let city = self.party_city();
                    self.enter_location2(city);
                    return Ok(true);
                }
                Fallout2Travel::Encounter(selected) => {
                    world.clear_dots();
                    world.journey = None;
                    let position = self.state2().map_or([0, 0], |state| state.position);
                    if let Screen::World2(world) = &mut self.screen {
                        world.view = view::centered(position, size);
                        world.pending = Some(Pending2::Flash {
                            start: now,
                            selected,
                        });
                    }
                    return Ok(true);
                }
            }
        }
        let Some(state) = self.state2() else {
            return Ok(true);
        };
        let (position, at_rest) = (state.position, state.travel.is_none());
        let pointer = self.pointer.at;
        let count = self.known_cities().len();
        let Screen::World2(world) = &mut self.screen else {
            return Ok(true);
        };
        world.list.clamp(count);
        if world.follow {
            world.view = view::centered(position, size);
        }
        let party = screen::window(position, world.view);
        world.hover = at_rest && pointer.is_some_and(|at| screen::hovers(at, party));
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
    pub(crate) fn world2_cursor(&self, at: [i32; 2]) -> &'static str {
        let (Screen::World2(world), Some(state)) = (&self.screen, self.state2()) else {
            return cursor::ARROW.art;
        };
        if world.pending.is_some() || world.paused {
            return cursor::ARROW.art;
        }
        let direction = screen::edge(at);
        let Some(index) = screen::cursor_index(direction) else {
            return cursor::ARROW.art;
        };
        if view::scroll(world.view, direction, state.world_size()).1 {
            cursor::SCROLL_BLOCKED[index].art
        } else {
            cursor::SCROLL[index].art
        }
    }

    /// §3.12.1 the tiles under the view, each its own art (a missing tile
    /// stays key 0), darkened by subtile as SPEC-014 §3.39 step 2.
    fn draw_world2_view(&self, painter: &Painter, image: &mut IndexedImage, view: [i32; 2]) {
        let Some(state) = self.state2() else {
            return;
        };
        let worldmap = &state.tables.worldmap;
        let across = worldmap.across.max(1) as usize;
        let [columns, rows] = state.grid();
        let tile_rows = rows / 6;
        for row in 0..tile_rows {
            for column in 0..across {
                let number = row * across + column;
                let Some(name) = worldmap
                    .tiles
                    .get(number)
                    .and_then(|tile| painter.interface_row(tile.art))
                else {
                    continue;
                };
                let origin = [column as i32 * TILE_SIZE[0], row as i32 * TILE_SIZE[1]];
                // The part of the tile inside the view.
                let left = view[0].max(origin[0]);
                let top = view[1].max(origin[1]);
                let right = (view[0] + VIEWPORT_SIZE[0]).min(origin[0] + TILE_SIZE[0]);
                let bottom = (view[1] + VIEWPORT_SIZE[1]).min(origin[1] + TILE_SIZE[1]);
                if left >= right || top >= bottom {
                    continue;
                }
                painter.blit_region(
                    image,
                    &name,
                    Rect::new(
                        left - origin[0],
                        top - origin[1],
                        right - left,
                        bottom - top,
                    ),
                    screen::window([left, top], view),
                    true,
                );
            }
        }
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
        let stride = image.width() as i32;
        for row in 0..rows {
            for column in 0..columns {
                let Some(rect) = screen::cell_rect([column, row], view) else {
                    continue;
                };
                match state.discovery([column, row]) {
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
    /// §3.12.2 every known city: its circle outline in key 992 and its
    /// display name below it, both clipped to the view.
    fn draw_cities(&self, painter: &Painter, image: &mut IndexedImage, view: [i32; 2]) {
        let Some(state) = self.state2() else {
            return;
        };
        let green = painter.color(ColorKey::GREEN);
        let stride = image.width() as i32;
        for area in state.tables.city.areas() {
            let Some(city) = state.areas.get(area.number as usize) else {
                continue;
            };
            if city.known == 0 {
                continue;
            }
            let radius = area.size.radius();
            let center = screen::window(city.position, view);
            for [x, y] in ui2::circle(center, radius) {
                if inside_view([x, y]) {
                    image.pixels_mut()[(y * stride + x) as usize] = green;
                }
            }
            let name = area.name.as_bytes();
            let width = painter.width(screen::LABEL_FONT, name);
            if width > 0 {
                let origin = ui2::city_label(center, radius, width);
                self.draw_label(painter, image, name, origin, width, VIEW_CLIP);
            }
        }
    }
    /// The hover label: inside a city its name when known, else "Unknown";
    /// elsewhere the terrain name of the party's subtile.
    fn draw_hover_label2(&self, painter: &Painter, image: &mut IndexedImage, party: [i32; 2]) {
        let Some(state) = self.state2() else {
            return;
        };
        let text = match state.area_at(state.position) {
            Some(city) if state.areas.get(city).is_some_and(|area| area.known >= 1) => {
                self.city_name(city)
            }
            Some(_) => self.message("worldmap.msg", UNKNOWN),
            None => state
                .cell()
                .and_then(|cell| {
                    state
                        .tables
                        .worldmap
                        .terrains
                        .get(state.subtile(cell).terrain)
                })
                .map(|terrain| terrain.name.clone().into_bytes())
                .unwrap_or_default(),
        };
        let width = painter.width(screen::LABEL_FONT, &text);
        let origin = screen::label_origin(party, width);
        if width <= 0 || !screen::label_shown(origin, width) {
            return;
        }
        self.draw_label(painter, image, &text, origin, width, VIEW_CLIP);
    }
    /// §3.12.3 the side panel: the date, the dial, the town list and its
    /// arrows, the fuel gauge while driving, and the Town/World button.
    fn draw_panel2(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        list: TownList,
        held: Option<PanelButton>,
    ) {
        let pointer = self.pointer.at;
        let down = |button: PanelButton| {
            held == Some(button) && pointer.is_some_and(|at| button.rect().contains(at))
        };
        self.draw_date(painter, image);
        // The dial: frame trunc(hour x frames / 24).
        if let Some((_, art)) = painter.role(&ui2::DIAL) {
            let frames = art.frame_count().max(1) as i32;
            let hour = self.host.world.clock.hhmm() / 100;
            if let Some(frame) = art.frame(0, panel::dial_frame(hour, frames) as usize) {
                image.blit(frame, panel::DIAL.map(i64::from));
            }
        }
        let cities = self.known_cities();
        for k in 0..panel::ROWS {
            let rect = panel::row_button(k);
            let art = if down(PanelButton::Row(k)) {
                BUTTON_DOWN_ART
            } else {
                BUTTON_UP_ART
            };
            painter.blit_region(
                image,
                art,
                Rect::new(0, 0, rect.w, rect.h),
                [rect.x, rect.y],
                false,
            );
            let Some(city) = list.entry(k, cities.len()).map(|entry| cities[entry]) else {
                continue;
            };
            let label = self
                .state2()
                .and_then(|state| state.tables.city.get(city as i32))
                .map_or(-1, |area| area.townmap_label_art);
            let at = panel::row_label(k);
            let drawn = painter
                .interface_row(label)
                .is_some_and(|name| painter.blit(image, &name, 0, at));
            if !drawn {
                let name = self.city_name(city);
                painter.text(image, 101, ColorKey::GREEN, &name, at, 640 - at[0]);
            }
        }
        for (button, role) in [
            (PanelButton::Up, &ui2::LIST_ARROWS[0]),
            (PanelButton::Down, &ui2::LIST_ARROWS[1]),
        ] {
            let rect = button.rect();
            if down(button) && painter.blit(image, BUTTON_DOWN_ART, 0, [rect.x, rect.y]) {
                continue;
            }
            painter.blit_role(image, role, [rect.x, rect.y]);
        }
        let rect = panel::TOWN_WORLD;
        let art = if down(PanelButton::TownWorld) {
            BUTTON_DOWN_ART
        } else {
            BUTTON_UP_ART
        };
        painter.blit_region(
            image,
            art,
            Rect::new(0, 0, rect.w, rect.h),
            [rect.x, rect.y],
            false,
        );
        // §3.19.4 the fuel gauge while the party drives.
        if let Some(state) = self.state2().filter(|state| state.driving) {
            let gauge = panel::GAUGE;
            image.fill_rect(
                [gauge.x, gauge.y, gauge.w, gauge.h],
                painter.color(ColorKey::BLACK),
            );
            let fill = ui2::gauge_fill(state.car.gauge_rows());
            image.fill_rect(
                [fill.x, fill.y, fill.w, fill.h],
                painter.color(ColorKey::GREEN),
            );
        }
    }
    pub(crate) fn world2_image(&self) -> AppResult<IndexedImage> {
        let (Screen::World2(world), Some(state)) = (&self.screen, self.state2()) else {
            return Err("world map screen is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        let position = state.position;
        let size = state.world_size();
        let view = match world.pending {
            Some(Pending2::Flash { .. }) => view::centered(position, size),
            _ => world.view,
        };
        self.draw_world2_view(&painter, &mut image, view);
        self.draw_cities(&painter, &mut image, view);
        let party = screen::window(position, view);
        match world.pending {
            None => {
                if let Some(travel) = state.travel {
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
                        self.draw_hover_label2(&painter, &mut image, party);
                    }
                }
            }
            Some(Pending2::Enter { .. }) => {
                self.draw_marker(
                    &painter,
                    &mut image,
                    PRESSED_TRIANGLE_ART,
                    Marker::Triangle,
                    party,
                );
                if self.pointer.at.is_some_and(|at| screen::hovers(at, party)) {
                    self.draw_hover_label2(&painter, &mut image, party);
                }
            }
            Some(Pending2::Flash { start, selected }) => {
                let second = (self.clock.saturating_sub(start) / screen::FLASH_MS) % 2 == 1;
                if selected.special {
                    let art = if second { TARGET_ART } else { DARK_TARGET_ART };
                    self.draw_marker(&painter, &mut image, art, Marker::Crosshair, party);
                } else {
                    let art = BOLT_ART[usize::from(second)];
                    self.draw_marker(&painter, &mut image, art, Marker::Bolt, party);
                }
            }
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        self.draw_panel2(&painter, &mut image, world.list, world.held);
        if world.paused || world.grey.is_some() {
            let [x, y] = VIEWPORT_ORIGIN;
            let [w, h] = VIEWPORT_SIZE;
            let stride = image.width() as i32;
            for row in y..y + h {
                for column in x..x + w {
                    if let Some(pixel) =
                        image.pixels_mut().get_mut((row * stride + column) as usize)
                    {
                        *pixel = self.grey[usize::from(*pixel)];
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

    /// The town-map marker whose triangle is under `point` (the opaque
    /// pixels of `hotspot1.frm`, or its whole 25-by-13 box without art).
    fn marker2_at(&self, town: &Town2Screen, point: [i32; 2]) -> Option<usize> {
        let art = self.painter().art(TRIANGLE_ART);
        let frame = art.as_ref().and_then(|art| art.frame(0, 0));
        town.markers.iter().position(|(_, anchor)| {
            let [x, y] = ui2::entrance_triangle(*anchor);
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
    pub(crate) fn town2_key(&mut self, key: KeyCode) -> bool {
        self.town_key(key)
    }
    pub(crate) fn town2_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Screen::Town2(town) = &self.screen else {
            return false;
        };
        if right {
            return true;
        }
        if let Some(index) = self.marker2_at(town, point) {
            if let Screen::Town2(town) = &mut self.screen {
                town.pressed = Some(index);
            }
            self.play_effect(ENTER_SOUND, None);
            return true;
        }
        if let Some(button) = PanelButton::at(point) {
            if let Screen::Town2(town) = &mut self.screen {
                town.held = Some(button);
            }
            self.press();
        }
        true
    }
    pub(crate) fn town2_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let count = self.known_cities().len();
        let Screen::Town2(town) = &mut self.screen else {
            return false;
        };
        let (pressed, held) = (town.pressed.take(), town.held.take());
        let Some(point) = point else {
            return pressed.is_some() || held.is_some();
        };
        if let Some(index) = pressed {
            let over = match &self.screen {
                Screen::Town2(town) => self.marker2_at(town, point) == Some(index),
                _ => false,
            };
            if over {
                self.town2_choice(index);
            }
            return true;
        }
        let Some(button) = held else {
            return false;
        };
        if !button.rect().contains(point) {
            return true;
        }
        match button {
            PanelButton::Up => {
                town.list.up();
            }
            PanelButton::Down => {
                town.list.down(count);
            }
            PanelButton::TownWorld => self.town2_world_button(),
            PanelButton::Row(k) => {
                // A known city's button shows its picture and markers.
                let cities = self.known_cities();
                let Screen::Town2(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
                    return true;
                };
                let Some(city) = town.list.entry(k, cities.len()).map(|entry| cities[entry]) else {
                    self.screen = Screen::Town2(town);
                    return true;
                };
                let Town2Screen {
                    origin,
                    world,
                    list,
                    city: shown,
                    ..
                } = *town;
                match self.town2_screen(city, origin, world, list) {
                    Ok(next) => self.screen = Screen::Town2(Box::new(next)),
                    Err(world) => {
                        if let Ok(same) = self.town2_screen(shown, origin, world, list) {
                            self.screen = Screen::Town2(Box::new(same));
                        }
                    }
                }
            }
        }
        true
    }
    /// The town map's Town/World button: the world map, at rest.
    fn town2_world_button(&mut self) {
        let Screen::Town2(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if let Some(state) = self.host.world.fallout2.as_deref_mut() {
            state.travel = None;
        }
        self.host.play_world_map_music();
        match town.world {
            Some(mut world) => {
                world.follow = town.origin != TownOrigin::TownWorld;
                world.grey = None;
                self.show_world2(*world, false);
            }
            None => self.open_world2(),
        }
    }
    /// §3.13 a chosen entrance: in the city the party is in, the
    /// entrance's map loads with its placement; elsewhere the world map
    /// takes over with the party travelling to that city.
    fn town2_choice(&mut self, index: usize) {
        let Screen::Town2(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let Some((slot, _)) = town.markers.get(index).copied() else {
            self.screen = Screen::Town2(town);
            return;
        };
        if self.party_city() == Some(town.city) {
            let loaded = self.host.fallout2_enter_entrance(town.city, slot);
            self.leave_world2(loaded);
            return;
        }
        if let Some(state) = self.host.world.fallout2.as_deref_mut() {
            state.travel = None;
        }
        let opening = town.world.is_none();
        let world = match town.world {
            Some(world) => *world,
            None => match self.state2() {
                Some(state) => World2Screen::new(state),
                None => return,
            },
        };
        self.host.play_world_map_music();
        self.show_world2(world, opening);
        if self.travel_to_city(town.city)
            && let Screen::World2(world) = &mut self.screen
        {
            world.follow = true;
            world.journey = Some(town.city);
        }
    }
    pub(crate) fn town2_image(&self) -> AppResult<IndexedImage> {
        let Screen::Town2(town) = &self.screen else {
            return Err("town map screen is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        // The picture, opaque at (20, 20), clipped to the frame's hole.
        if let Some(picture) = self.town_picture(town.city) {
            let hole = Rect::new(
                0,
                0,
                VIEWPORT_ORIGIN[0] + VIEWPORT_SIZE[0] - ui2::TOWN_PICTURE[0],
                VIEWPORT_ORIGIN[1] + VIEWPORT_SIZE[1] - ui2::TOWN_PICTURE[1],
            );
            painter.blit_region(&mut image, &picture, hole, ui2::TOWN_PICTURE, true);
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        self.draw_panel2(&painter, &mut image, town.list, town.held);
        let hovered = self.pointer.at.and_then(|at| self.marker2_at(town, at));
        for (index, (slot, anchor)) in town.markers.iter().enumerate() {
            let pressed = town.pressed == Some(index) && hovered == Some(index);
            let art = if pressed {
                PRESSED_TRIANGLE_ART
            } else {
                TRIANGLE_ART
            };
            painter.blit(&mut image, art, 0, ui2::entrance_triangle(*anchor));
            if hovered != Some(index) {
                continue;
            }
            // §3.13 the label: the destination map's lookup name.
            let label = self.state2().and_then(|state| {
                let entrance = state.entrance_of(town.city, *slot)?;
                state
                    .tables
                    .maps
                    .get(entrance.map as i32)?
                    .lookup_name
                    .clone()
            });
            if let Some(text) = label {
                let text = text.into_bytes();
                let width = painter.width(screen::LABEL_FONT, &text);
                let button = ui2::entrance_button(*anchor);
                if width > 0 {
                    let origin = [button.x + screen::marker_label_x(width), button.y];
                    let clip = [button.x, button.y, button.w, button.h].map(i64::from);
                    self.draw_label(
                        &painter,
                        &mut image,
                        &text,
                        origin,
                        width.min(button.w),
                        clip,
                    );
                }
            }
        }
        Ok(image)
    }
}

/// Whether a window pixel lies in the view.
fn inside_view([x, y]: [i32; 2]) -> bool {
    (VIEWPORT_ORIGIN[0]..VIEWPORT_ORIGIN[0] + VIEWPORT_SIZE[0]).contains(&x)
        && (VIEWPORT_ORIGIN[1]..VIEWPORT_ORIGIN[1] + VIEWPORT_SIZE[1]).contains(&y)
}

#[cfg(test)]
#[path = "world2/tests.rs"]
mod tests;
