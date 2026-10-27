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
//! that fails leaving the world map open. SPEC-020 §2.2: that shared path
//! is `world_kit`'s, run with this module's Fallout 2 hooks
//! ([`WorldEdition`]).
use super::{
    MapPreview, Screen,
    world::TownOrigin,
    world_kit::{
        Bounds, Hold, Leg, TownPress, VIEW_CLIP, WorldEdition, WorldView, red_button, triangle_art,
    },
};
use crate::{AppResult, interface::Painter};
use std::ops::{Deref, DerefMut};
use vault_core::{
    Fallout2Travel,
    fallout2::{self, Fallout2World, Selected},
    ui::{
        ColorKey, ERROR_SOUND, Rect,
        fallout2::{self as ui2, TownList, panel, view},
    },
    worldmap::{VIEWPORT_ORIGIN, VIEWPORT_SIZE, screen},
};
use vault_formats::fallout2::worldmap::TILE_SIZE;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const FRAME_ART: &str = "wmapbox.frm";
const BUTTON_DOWN_ART: &str = "lilreddn.frm";
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
    /// The view state both editions share.
    kit: WorldView<PanelButton, Pending2>,
    /// SPEC-019 §2.4 the town-list scroll offset (not saved).
    pub list: TownList,
}
impl Deref for World2Screen {
    type Target = WorldView<PanelButton, Pending2>;
    fn deref(&self) -> &Self::Target {
        &self.kit
    }
}
impl DerefMut for World2Screen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kit
    }
}
pub(crate) struct Town2Screen {
    pub city: usize,
    pub origin: TownOrigin,
    /// The entrance slots with markers, and their anchors.
    pub markers: Vec<(usize, [i32; 2])>,
    /// The marker pressed and the side-panel button held down.
    press: TownPress<PanelButton>,
    list: TownList,
    world: Option<Box<World2Screen>>,
}

impl World2Screen {
    fn new(state: &Fallout2World) -> Self {
        let size = state.world_size();
        Self {
            kit: WorldView::new(view::centered(state.position, size), size),
            list: TownList::default(),
        }
    }
}

/// The Fallout 2 world and town map screens, for the shared path.
struct Fallout2;
impl WorldEdition for Fallout2 {
    type Button = PanelButton;
    type Pending = Pending2;
    type Encounter = Selected;
    type Screen = World2Screen;
    type Town = Town2Screen;

    fn screen(preview: &MapPreview) -> Option<&World2Screen> {
        match &preview.screen {
            Screen::World2(world) => Some(world),
            _ => None,
        }
    }
    fn screen_mut(preview: &mut MapPreview) -> Option<&mut World2Screen> {
        match &mut preview.screen {
            Screen::World2(world) => Some(world),
            _ => None,
        }
    }
    fn town(preview: &MapPreview) -> Option<&Town2Screen> {
        match &preview.screen {
            Screen::Town2(town) => Some(town),
            _ => None,
        }
    }
    fn town_mut(preview: &mut MapPreview) -> Option<&mut Town2Screen> {
        match &mut preview.screen {
            Screen::Town2(town) => Some(town),
            _ => None,
        }
    }
    fn town_press(town: &mut Town2Screen) -> &mut TownPress<PanelButton> {
        &mut town.press
    }
    fn bounds(preview: &MapPreview) -> Option<Bounds> {
        preview.state2().map(|state| Bounds::Fallout2 {
            world: state.world_size(),
        })
    }
    fn position(preview: &MapPreview) -> Option<[i32; 2]> {
        preview.state2().map(|state| state.position)
    }
    fn destination(preview: &MapPreview) -> Option<[i32; 2]> {
        preview
            .state2()
            .and_then(|state| state.travel)
            .map(|travel| travel.destination)
    }
    fn party_place(preview: &MapPreview) -> Option<usize> {
        preview.party_city()
    }
    fn party_in(preview: &MapPreview, city: usize) -> bool {
        preview.party_city() == Some(city)
    }
    fn hold(pending: Pending2) -> Hold<Selected> {
        match pending {
            Pending2::Enter { until, city } => Hold::Enter { until, place: city },
            Pending2::Flash { start, selected } => Hold::Flash {
                start,
                encounter: selected,
            },
        }
    }
    fn entering(until: u64, city: Option<usize>) -> Pending2 {
        Pending2::Enter { until, city }
    }
    fn flashing(start: u64, selected: Selected) -> Pending2 {
        Pending2::Flash { start, selected }
    }
    fn special(selected: Selected) -> bool {
        selected.special
    }
    fn button_at(point: [i32; 2]) -> Option<PanelButton> {
        PanelButton::at(point)
    }
    fn button_contains(button: PanelButton, point: [i32; 2]) -> bool {
        button.rect().contains(point)
    }
    fn world_button(preview: &mut MapPreview, button: PanelButton) {
        preview.world2_panel_button(button);
    }
    fn town_button(preview: &mut MapPreview, button: PanelButton) {
        preview.town2_panel_button(button);
    }
    fn town_choice(preview: &mut MapPreview, index: usize) {
        preview.town2_choice(index);
    }
    fn marker_at(preview: &MapPreview, town: &Town2Screen, point: [i32; 2]) -> Option<usize> {
        preview.marker2_at(town, point)
    }
    /// §3.12.2: a locked city cannot be entered (the sound plays).
    fn party_click(preview: &mut MapPreview) {
        if preview
            .party_city()
            .is_some_and(|city| preview.city_locked(city))
        {
            return;
        }
        preview.world_map_entry::<Self>(screen::ENTER_MS);
    }
    fn stop_travel(preview: &mut MapPreview) {
        if let Some(state) = preview.host.world.fallout2.as_deref_mut() {
            state.travel = None;
        }
    }
    fn travel_to(preview: &mut MapPreview, target: [i32; 2]) {
        if let Err(error) = preview.host.fallout2_travel_to(target) {
            preview.status = error.to_string().into_bytes();
        }
    }
    fn travel_to_place(preview: &mut MapPreview, city: usize) -> bool {
        preview.travel_to_city(city)
    }
    fn travel_iteration(preview: &mut MapPreview) -> AppResult<Leg<Selected>> {
        Ok(match preview.host.fallout2_travel_iteration()? {
            Fallout2Travel::Idle => Leg::Idle,
            Fallout2Travel::Moving => Leg::Moving,
            Fallout2Travel::Arrived | Fallout2Travel::Blocked => Leg::Stopped,
            Fallout2Travel::OutOfFuel => Leg::OutOfFuel,
            Fallout2Travel::Interrupted(_) => Leg::Interrupted,
            Fallout2Travel::Encounter(selected) => Leg::Encounter(selected),
        })
    }
    fn travel_mark(preview: &MapPreview) -> Option<[i32; 2]> {
        preview.state2().and_then(|state| state.mark)
    }
    fn enter_location(preview: &mut MapPreview, city: Option<usize>) {
        preview.enter_location2(city);
    }
    fn enter_encounter(preview: &mut MapPreview, selected: Selected) {
        let entered = preview.host.fallout2_enter_encounter(selected);
        preview.leave_world2(entered);
    }
    fn out_of_fuel(preview: &mut MapPreview) {
        preview.out_of_fuel();
    }
    /// The town list keeps within the known cities.
    fn settle(preview: &mut MapPreview) {
        let count = preview.known_cities().len();
        if let Screen::World2(world) = &mut preview.screen {
            world.list.clamp(count);
        }
    }
    fn draw_terrain(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        view: [i32; 2],
    ) {
        preview.draw_world2_view(painter, image, view);
        preview.draw_cities(painter, image, view);
    }
    fn draw_hover_label(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        party: [i32; 2],
    ) {
        preview.draw_hover_label2(painter, image, party);
    }
    fn draw_panel(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        screen: &World2Screen,
    ) {
        preview.draw_panel2(painter, image, screen.list, screen.held());
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
        world.rebuild();
        world.list.clamp(self.known_cities().len());
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
            press: TownPress::default(),
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
                // SPEC-018 §4.3 (Revision 2): an entrance at -1, -1 has no
                // marker.
                let marker = entrance.as_ref()?.marker?;
                known
                    .as_ref()
                    .and_then(|known| known.get(slot))
                    .copied()
                    .unwrap_or(false)
                    .then_some((
                        slot,
                        [
                            ui2::TOWN_PICTURE[0] + marker[0],
                            ui2::TOWN_PICTURE[1] + marker[1],
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
        self.world_map_head_for::<Fallout2>(city, travelling.is_some());
    }
    /// A world-map side-panel button released over it.
    fn world2_panel_button(&mut self, button: PanelButton) {
        let count = self.known_cities().len();
        match button {
            PanelButton::Up => {
                if let Screen::World2(world) = &mut self.screen {
                    world.list.up();
                }
            }
            PanelButton::Down => {
                if let Screen::World2(world) = &mut self.screen {
                    world.list.down(count);
                }
            }
            PanelButton::TownWorld => self.world2_town_world(),
            PanelButton::Row(k) => self.world2_row(k),
        }
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
        self.world_map_key::<Fallout2>(key)
    }
    pub(crate) fn world2_click(&mut self, point: [i32; 2], right: bool) -> bool {
        self.world_map_click::<Fallout2>(point, right)
    }
    pub(crate) fn world2_release(&mut self, point: Option<[i32; 2]>) -> bool {
        self.world_map_release::<Fallout2>(point)
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
        self.world_map_tick::<Fallout2>(now)
    }
    pub(crate) fn world2_cursor(&self, at: [i32; 2]) -> &'static str {
        self.world_map_cursor::<Fallout2>(at)
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
        self.darken_cells(painter, image, view, [columns, rows], |cell| {
            state.discovery(cell)
        });
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
        self.draw_party_label(painter, image, &text, party);
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
            red_button(
                painter,
                image,
                [rect.x, rect.y, rect.w, rect.h],
                down(PanelButton::Row(k)),
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
        red_button(
            painter,
            image,
            [rect.x, rect.y, rect.w, rect.h],
            down(PanelButton::TownWorld),
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
        self.world_map_image::<Fallout2>()
    }

    /// The town-map marker whose triangle is under `point` (the opaque
    /// pixels of `hotspot1.frm`, or its whole 25-by-13 box without art).
    fn marker2_at(&self, town: &Town2Screen, point: [i32; 2]) -> Option<usize> {
        self.triangle_at(
            town.markers
                .iter()
                .map(|(_, anchor)| ui2::entrance_triangle(*anchor)),
            point,
        )
    }
    pub(crate) fn town2_key(&mut self, key: KeyCode) -> bool {
        self.town_key(key)
    }
    pub(crate) fn town2_click(&mut self, point: [i32; 2], right: bool) -> bool {
        self.town_map_click::<Fallout2>(point, right)
    }
    pub(crate) fn town2_release(&mut self, point: Option<[i32; 2]>) -> bool {
        self.town_map_release::<Fallout2>(point)
    }
    /// A town map's side-panel button released over it.
    fn town2_panel_button(&mut self, button: PanelButton) {
        let count = self.known_cities().len();
        match button {
            PanelButton::Up => {
                if let Screen::Town2(town) = &mut self.screen {
                    town.list.up();
                }
            }
            PanelButton::Down => {
                if let Screen::Town2(town) = &mut self.screen {
                    town.list.down(count);
                }
            }
            PanelButton::TownWorld => self.town2_world_button(),
            PanelButton::Row(k) => {
                // A known city's button shows its picture and markers.
                let cities = self.known_cities();
                let Screen::Town2(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
                    return;
                };
                let Some(city) = town.list.entry(k, cities.len()).map(|entry| cities[entry]) else {
                    self.screen = Screen::Town2(town);
                    return;
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
        self.draw_panel2(&painter, &mut image, town.list, town.press.held());
        let hovered = self.pointer.at.and_then(|at| self.marker2_at(town, at));
        for (index, (slot, anchor)) in town.markers.iter().enumerate() {
            let pressed = town.press.pressed() == Some(index) && hovered == Some(index);
            let origin = ui2::entrance_triangle(*anchor);
            painter.blit(&mut image, triangle_art(pressed), 0, origin);
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
                let button = ui2::entrance_button(*anchor);
                let rect = [button.x, button.y, button.w, button.h];
                self.draw_marker_label(&painter, &mut image, text.as_bytes(), rect);
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
