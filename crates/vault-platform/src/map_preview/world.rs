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
//!
//! SPEC-020 §2.2: the stepping, pointer and drawing rules this screen
//! shares with the Fallout 2 one (`world2`) are `world_kit`'s; this module
//! holds the Fallout 1 hooks ([`WorldEdition`]) and its own panel, terrain
//! and town-map rules.
use super::{
    MapPreview, Screen,
    world_kit::{
        Bounds, Hold, Leg, TownPress, WorldEdition, WorldView, inside, red_button, triangle_art,
    },
};
use crate::{AppResult, interface::Painter};
use std::ops::{Deref, DerefMut};
use vault_core::{
    TravelOutcome, WorldMapExit,
    ui::Rect,
    worldmap::{
        self, COLUMNS, Encounter, ROWS, TOWNS, TownButton, TownEntry, VIEWPORT_ORIGIN,
        VIEWPORT_SIZE, WORLD_SIZE, screen,
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const WORLD_ART: &str = "worldmap.frm";
const FRAME_ART: &str = "wmapbox.frm";
const NAMES_ART: &str = "wmaplabs.frm";
/// The side panel's Town/World button, after the twelve town buttons.
const TOWN_WORLD: usize = 12;

pub(crate) struct WorldScreen {
    /// The view state both editions share.
    kit: WorldView<usize, Pending>,
    /// The towns whose names were drawn when the screen was built.
    names: u32,
}
impl Deref for WorldScreen {
    type Target = WorldView<usize, Pending>;
    fn deref(&self) -> &Self::Target {
        &self.kit
    }
}
impl DerefMut for WorldScreen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kit
    }
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
    /// The marker pressed and the side-panel button held down.
    press: TownPress<usize>,
    /// The world map under this town map, when opened from it.
    world: Option<Box<WorldScreen>>,
}

impl WorldScreen {
    fn new(position: [i32; 2]) -> Self {
        Self {
            kit: WorldView::new(screen::centered(position), WORLD_SIZE),
            names: 0,
        }
    }
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

/// The Fallout 1 world and town map screens, for the shared path.
struct Fallout1;
impl WorldEdition for Fallout1 {
    type Button = usize;
    type Pending = Pending;
    type Encounter = Encounter;
    type Screen = WorldScreen;
    type Town = TownScreen;

    fn screen(preview: &MapPreview) -> Option<&WorldScreen> {
        match &preview.screen {
            Screen::World(world) => Some(world),
            _ => None,
        }
    }
    fn screen_mut(preview: &mut MapPreview) -> Option<&mut WorldScreen> {
        match &mut preview.screen {
            Screen::World(world) => Some(world),
            _ => None,
        }
    }
    fn town(preview: &MapPreview) -> Option<&TownScreen> {
        match &preview.screen {
            Screen::Town(town) => Some(town),
            _ => None,
        }
    }
    fn town_mut(preview: &mut MapPreview) -> Option<&mut TownScreen> {
        match &mut preview.screen {
            Screen::Town(town) => Some(town),
            _ => None,
        }
    }
    fn town_press(town: &mut TownScreen) -> &mut TownPress<usize> {
        &mut town.press
    }
    fn bounds(_: &MapPreview) -> Option<Bounds> {
        Some(Bounds::Fallout1)
    }
    fn position(preview: &MapPreview) -> Option<[i32; 2]> {
        Some(preview.host.world.world_map.position())
    }
    fn destination(preview: &MapPreview) -> Option<[i32; 2]> {
        preview
            .host
            .world
            .world_map
            .travel
            .map(|travel| travel.destination)
    }
    fn party_place(preview: &MapPreview) -> Option<usize> {
        preview.party_town()
    }
    fn party_in(preview: &MapPreview, town: usize) -> bool {
        preview.host.party_in_town(town)
    }
    fn hold(pending: Pending) -> Hold<Encounter> {
        match pending {
            Pending::Enter { until, town } => Hold::Enter { until, place: town },
            Pending::Flash { start, encounter } => Hold::Flash { start, encounter },
        }
    }
    fn entering(until: u64, town: Option<usize>) -> Pending {
        Pending::Enter { until, town }
    }
    fn flashing(start: u64, encounter: Encounter) -> Pending {
        Pending::Flash { start, encounter }
    }
    fn special(encounter: Encounter) -> bool {
        matches!(encounter, Encounter::Special { .. })
    }
    fn button_at(point: [i32; 2]) -> Option<usize> {
        panel_button(point)
    }
    fn button_contains(button: usize, point: [i32; 2]) -> bool {
        inside(panel_rect(button), point)
    }
    fn world_button(preview: &mut MapPreview, button: usize) {
        if button == TOWN_WORLD {
            preview.world_town_world();
        } else {
            preview.world_town_button(button);
        }
    }
    fn town_button(preview: &mut MapPreview, button: usize) {
        preview.town_panel_button(button);
    }
    fn town_choice(preview: &mut MapPreview, index: usize) {
        preview.town_choice(index);
    }
    fn marker_at(preview: &MapPreview, town: &TownScreen, point: [i32; 2]) -> Option<usize> {
        preview.marker_at(town, point)
    }
    fn party_click(preview: &mut MapPreview) {
        preview.world_map_entry::<Self>(screen::ENTER_MS);
    }
    fn stop_travel(preview: &mut MapPreview) {
        preview.host.world.world_map.travel = None;
    }
    fn travel_to(preview: &mut MapPreview, target: [i32; 2]) {
        preview.host.travel_to(target);
    }
    fn travel_to_place(preview: &mut MapPreview, town: usize) -> bool {
        preview.travel_to_town(town)
    }
    fn travel_iteration(preview: &mut MapPreview) -> AppResult<Leg<Encounter>> {
        Ok(match preview.host.travel_iteration()? {
            TravelOutcome::Idle => Leg::Idle,
            TravelOutcome::Moving => Leg::Moving,
            TravelOutcome::Arrived | TravelOutcome::Blocked => Leg::Stopped,
            TravelOutcome::Interrupted(_) => Leg::Interrupted,
            TravelOutcome::Encounter(encounter) => Leg::Encounter(encounter),
        })
    }
    fn travel_mark(preview: &MapPreview) -> Option<[i32; 2]> {
        preview.host.world.world_map.mark
    }
    fn enter_location(preview: &mut MapPreview, town: Option<usize>) {
        preview.enter_location(town);
    }
    fn enter_encounter(preview: &mut MapPreview, encounter: Encounter) {
        let entered = preview.host.enter_encounter(encounter);
        preview.leave_world(entered);
    }
    fn campaign_checks(preview: &mut MapPreview) -> AppResult<bool> {
        match preview.host.world_map_checks()? {
            Some(WorldMapExit::Quit) => {
                preview.quit_requested = true;
                preview.close_world_to_map();
                Ok(true)
            }
            Some(WorldMapExit::Vault13) => {
                let entered = preview.host.enter_vault13_after_explosion();
                preview.leave_world(entered);
                Ok(true)
            }
            None => Ok(false),
        }
    }
    fn draw_terrain(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        view: [i32; 2],
    ) {
        preview.draw_world_view(painter, image, view);
    }
    fn draw_hover_label(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        party: [i32; 2],
    ) {
        preview.draw_hover_label(painter, image, party);
    }
    fn draw_panel(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        screen: &WorldScreen,
    ) {
        preview.draw_panel(painter, image, screen.names, screen.held());
    }
}

impl MapPreview {
    pub(super) fn painter(&self) -> Painter<'_> {
        Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        }
    }
    fn known_towns(&self) -> u32 {
        (0..TOWNS.len())
            .filter(|town| self.host.world.world_map.town_enabled(*town))
            .fold(0, |bits, town| bits | 1 << town)
    }
    /// Open the world map (SPEC-014 §3.39): W and Q computed once, follow
    /// mode on and the view centered on the party. The music was requested
    /// by the transition that opened it.
    pub(crate) fn open_world_map(&mut self) {
        // SPEC-019 §3.12: a Fallout 2 game has its own world map.
        if self.host.world.fallout2.is_some() {
            self.open_world2();
            return;
        }
        // §3.39: opening the screen shows the party at rest (a town-map
        // choice that starts a journey sets its travel after this).
        self.end_world_travel();
        let world = WorldScreen::new(self.host.world.world_map.position());
        self.show_world(world, true);
    }
    /// M-W2: the screen is left (a map loads, a campaign check exits, a
    /// quit): a journey in progress ends there, with its travel-dot mark,
    /// so the next opening finds the party at rest with follow mode on
    /// instead of resuming a stale line from wherever the party is now.
    fn end_world_travel(&mut self) {
        let map = &mut self.host.world.world_map;
        map.travel = None;
        map.mark = None;
    }
    /// The world or town map closes to the loaded map.
    fn close_world_to_map(&mut self) {
        self.end_world_travel();
        self.return_to_map();
    }
    /// The world map opened by the interface rather than by a world
    /// transition (a town map's World button or failure, the preview's W
    /// key): SPEC-008 §3.10 `03wrldmp` starts each time the screen is built.
    pub(crate) fn open_world_map_with_music(&mut self) {
        // SPEC-019 §3.12: Fallout 2's opening build plays the track itself.
        if self.host.world.fallout2.is_none() {
            self.host.play_world_map_music();
        }
        self.open_world_map();
    }
    /// Building the screen (SPEC-014 §3.39): the reveal at the party's
    /// cell, no travel dots, and the town names as they are now.
    fn show_world(&mut self, mut world: WorldScreen, opening: bool) {
        if let Err(error) = self.host.build_world_map(opening) {
            self.status = error.to_string().into_bytes();
        }
        world.rebuild();
        world.names = self.known_towns();
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
            press: TownPress::default(),
            world,
        })
    }
    /// The current town's map (SPEC-013 §3.22 map -1, the `world_map`
    /// opcode). Under the safe rule a failure opens the world map at the
    /// current town instead.
    pub(crate) fn open_town_map(&mut self) {
        if self.host.world.fallout2.is_some() {
            self.open_town2();
            return;
        }
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
            Ok(()) => self.close_world_to_map(),
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
            Ok(Some(TownEntry::Direct(_))) => self.close_world_to_map(),
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
    /// A town button's event (on release, SPEC-014 §3.39): nothing for an
    /// unknown town or the one the journey already heads for.
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
        self.world_map_head_for::<Fallout1>(town, travelling.is_some());
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
        self.world_map_key::<Fallout1>(key)
    }
    /// A press on the world map: a side-panel button sounds and waits for
    /// its release; the view acts on the release.
    pub(crate) fn world_click(&mut self, point: [i32; 2], right: bool) -> bool {
        self.world_map_click::<Fallout1>(point, right)
    }
    /// A left release on the world map: the held side-panel button's event
    /// when the release is over it, else a view click.
    pub(crate) fn world_release(&mut self, point: Option<[i32; 2]>) -> bool {
        self.world_map_release::<Fallout1>(point)
    }
    /// One world-map pass, at most one per 41 milliseconds: edge scrolling,
    /// two travel iterations, the campaign checks, follow mode and the
    /// hover state. A pending entry or encounter flash holds the screen
    /// until it ends.
    pub(crate) fn world_tick(&mut self, now: u64) -> AppResult<bool> {
        self.world_map_tick::<Fallout1>(now)
    }
    /// The world-map cursor: the scroll art on an edge (the blocked art
    /// when every requested axis is at its limit), else the arrow.
    pub(crate) fn world_cursor(&self, at: [i32; 2]) -> &'static str {
        self.world_map_cursor::<Fallout1>(at)
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
        let map = &self.host.world.world_map;
        self.darken_cells(painter, image, view, [COLUMNS, ROWS], |cell| {
            map.discovery(cell)
        });
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
        self.draw_party_label(painter, image, &text, party);
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
            red_button(painter, image, panel_rect(button), down(button));
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
    pub(crate) fn world_image(&self) -> AppResult<IndexedImage> {
        self.world_map_image::<Fallout1>()
    }

    /// The town-map marker whose triangle pixels are under `point`: only
    /// the opaque pixels of `hotspot1.frm` count (the whole 25-by-13 image
    /// when the art is missing).
    fn marker_at(&self, town: &TownScreen, point: [i32; 2]) -> Option<usize> {
        self.triangle_at(
            town.buttons
                .iter()
                .map(|button| screen::marker_triangle(button.rect)),
            point,
        )
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
        self.town_map_click::<Fallout1>(point, right)
    }
    pub(crate) fn town_release(&mut self, point: Option<[i32; 2]>) -> bool {
        self.town_map_release::<Fallout1>(point)
    }
    /// A town map's side-panel button released over it: the Town/World
    /// button, or a known town's button, which shows its picture and
    /// markers without travelling or changing the current town.
    fn town_panel_button(&mut self, button: usize) {
        if button == TOWN_WORLD {
            self.town_world_button();
        } else if self.host.world.world_map.town_enabled(button) {
            let Screen::Town(town) = std::mem::replace(&mut self.screen, Screen::Map) else {
                return;
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
        image.fill_rect(
            [0, 0, 640, 480],
            painter.color(vault_core::ui::ColorKey::BLACK),
        );
        if let Some(picture) = screen::town_picture(town.town) {
            painter.blit_opaque(&mut image, &picture, 0, screen::TOWN_PICTURE);
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        self.draw_panel(&painter, &mut image, self.known_towns(), town.press.held());
        let hovered = self.pointer.at.and_then(|at| self.marker_at(town, at));
        for (index, button) in town.buttons.iter().enumerate() {
            let pressed = town.press.pressed() == Some(index) && hovered == Some(index);
            let origin = screen::marker_triangle(button.rect);
            painter.blit(&mut image, triangle_art(pressed), 0, origin);
            if hovered == Some(index) {
                let text = self.message("worldmap.msg", button.label);
                self.draw_marker_label(&painter, &mut image, &text, button.rect);
            }
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests;
