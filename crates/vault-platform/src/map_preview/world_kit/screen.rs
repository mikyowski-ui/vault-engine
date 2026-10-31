//! The world-map and town-map entry points both editions share (SPEC-014
//! §3.39-§3.40, kept by SPEC-019 §3.12-§3.13), over the [`WorldEdition`]
//! hooks: what differs by edition is the host's travel and entry rules,
//! the terrain and side panel drawn, and the hover label's text.
use super::{
    paint::{PRESSED_TRIANGLE_ART, TRIANGLE_ART},
    view::{Bounds, KeyOutcome, WorldView},
};
use crate::{AppResult, interface::Painter, map_preview::MapPreview};
use std::ops::DerefMut;
use vault_core::{
    ui::{ColorKey, cursor},
    worldmap::screen::{self, Marker},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const FRAME_ART: &str = "wmapbox.frm";
const PLUS_ART: &str = "wmaploc.frm";
const TARGET_ART: &str = "wmaptarg.frm";
const DARK_TARGET_ART: &str = "wmaptrg2.frm";
const BOLT_ART: [&str; 2] = ["wmapfgt0.frm", "wmapfgt1.frm"];
/// Entering a location and pressing a town-map marker.
const ENTER_SOUND: &[u8] = b"ib2p1xx1";
/// A destination click.
const CLICK_SOUND: &[u8] = b"ib1p1xx1";

/// A pending timed step, as the shared path reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold<E> {
    /// The pressed triangle shows until `until`; then the party enters
    /// `place` (a town or city), or drops to terrain.
    Enter { until: u64, place: Option<usize> },
    /// The encounter flash from `start`; then the encounter map loads.
    Flash { start: u64, encounter: E },
}

/// One travel iteration's outcome, as the shared pass reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leg<E> {
    /// No travel is active.
    Idle,
    Moving,
    /// Arrived or blocked: the party stops at rest; a town is never
    /// entered automatically, and a hotspot chosen before the journey is
    /// not used.
    Stopped,
    /// A queue handler stopped travel for the party's town or terrain.
    Interrupted,
    Encounter(E),
    /// SPEC-019 §3.19.3 the tank ran dry (Fallout 2 only).
    OutOfFuel,
}

/// A town map's pressed marker and held side-panel button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TownPress<B> {
    pressed: Option<usize>,
    held: Option<B>,
}
impl<B> Default for TownPress<B> {
    fn default() -> Self {
        Self {
            pressed: None,
            held: None,
        }
    }
}
impl<B: Copy> TownPress<B> {
    pub(crate) fn pressed(&self) -> Option<usize> {
        self.pressed
    }
    pub(crate) fn held(&self) -> Option<B> {
        self.held
    }
}

/// An edition's world and town map screens: the hooks the shared path
/// calls where the editions differ.
pub(crate) trait WorldEdition {
    /// A side-panel control.
    type Button: Copy + PartialEq;
    /// The pending timed step.
    type Pending: Copy;
    /// A selected encounter.
    type Encounter: Copy;
    /// The world-map screen, over its shared view state.
    type Screen: DerefMut<Target = WorldView<Self::Button, Self::Pending>>;
    /// The town-map screen.
    type Town;

    fn screen(preview: &MapPreview) -> Option<&Self::Screen>;
    fn screen_mut(preview: &mut MapPreview) -> Option<&mut Self::Screen>;
    fn town(preview: &MapPreview) -> Option<&Self::Town>;
    fn town_mut(preview: &mut MapPreview) -> Option<&mut Self::Town>;
    fn town_press(town: &mut Self::Town) -> &mut TownPress<Self::Button>;
    /// The view bounds; none when the edition's world is missing.
    fn bounds(preview: &MapPreview) -> Option<Bounds>;
    /// The party's world position; none when the edition's world is
    /// missing.
    fn position(preview: &MapPreview) -> Option<[i32; 2]>;
    /// The journey's destination; none at rest.
    fn destination(preview: &MapPreview) -> Option<[i32; 2]>;
    /// The town or city of the party's cell or circle.
    fn party_place(preview: &MapPreview) -> Option<usize>;
    /// Whether the party is in `place` (its button then enters it).
    fn party_in(preview: &MapPreview, place: usize) -> bool;
    fn hold(pending: Self::Pending) -> Hold<Self::Encounter>;
    fn entering(until: u64, place: Option<usize>) -> Self::Pending;
    fn flashing(start: u64, encounter: Self::Encounter) -> Self::Pending;
    /// A special encounter flashes the crosshair; others the bolt.
    fn special(encounter: Self::Encounter) -> bool;
    /// Whether the encounter flashes at all.
    fn flashes(_encounter: Self::Encounter) -> bool {
        true
    }
    fn button_at(point: [i32; 2]) -> Option<Self::Button>;
    fn button_contains(button: Self::Button, point: [i32; 2]) -> bool;
    /// The world map's side-panel button `button` was released over it.
    fn world_button(preview: &mut MapPreview, button: Self::Button);
    /// The town map's side-panel button `button` was released over it.
    fn town_button(preview: &mut MapPreview, button: Self::Button);
    /// The town map's marker `index` was released over it.
    fn town_choice(preview: &mut MapPreview, index: usize);
    /// The town map's marker under `point`.
    fn marker_at(preview: &MapPreview, town: &Self::Town, point: [i32; 2]) -> Option<usize>;
    /// The party at rest was clicked (its entry sound has played).
    fn party_click(preview: &mut MapPreview);
    /// The journey stops where the party is.
    fn stop_travel(preview: &mut MapPreview);
    /// A destination click toward world pixel `target`.
    fn travel_to(preview: &mut MapPreview, target: [i32; 2]);
    /// Head for `place`'s jittered position; whether a journey started.
    fn travel_to_place(preview: &mut MapPreview, place: usize) -> bool;
    fn travel_iteration(preview: &mut MapPreview) -> AppResult<Leg<Self::Encounter>>;
    /// The travel-dot pixel the last iteration marked.
    fn travel_mark(preview: &MapPreview) -> Option<[i32; 2]>;
    /// Enter `place`, or drop to terrain for `None`.
    fn enter_location(preview: &mut MapPreview, place: Option<usize>);
    fn enter_encounter(preview: &mut MapPreview, encounter: Self::Encounter);
    /// SPEC-019 §3.19.3 the tank ran dry.
    fn out_of_fuel(_preview: &mut MapPreview) {}
    /// The pass's campaign checks; whether they left the screen.
    fn campaign_checks(_preview: &mut MapPreview) -> AppResult<bool> {
        Ok(false)
    }
    /// The edition's own state settled at the end of a pass.
    fn settle(_preview: &mut MapPreview) {}
    /// The world under the view, and anything drawn on it under the
    /// markers.
    fn draw_terrain(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        view: [i32; 2],
    );
    fn draw_hover_label(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        party: [i32; 2],
    );
    fn draw_panel(
        preview: &MapPreview,
        painter: &Painter,
        image: &mut IndexedImage,
        screen: &Self::Screen,
    );
}

impl MapPreview {
    /// A world-map key.
    pub(crate) fn world_map_key<E: WorldEdition>(&mut self, key: KeyCode) -> bool {
        let [_, control, _] = self.hud.modifiers;
        let Some(bounds) = E::bounds(self) else {
            return false;
        };
        let position = E::position(self).unwrap_or([0, 0]);
        let Some(world) = E::screen_mut(self) else {
            return false;
        };
        match world.key(key, control, bounds, position) {
            KeyOutcome::Done => {}
            KeyOutcome::Brightness(up) => self.brightness_step(up),
            KeyOutcome::Quit => self.confirm_quit(),
            KeyOutcome::Screenshot => self.screenshot(),
        }
        true
    }
    /// A press on the world map: a side-panel button sounds and waits for
    /// its release; the view acts on the release.
    pub(crate) fn world_map_click<E: WorldEdition>(
        &mut self,
        point: [i32; 2],
        right: bool,
    ) -> bool {
        let Some(world) = E::screen_mut(self) else {
            return false;
        };
        if world.press(right, || E::button_at(point)) {
            self.press();
        }
        true
    }
    /// A left release on the world map: the held side-panel button's event
    /// when the release is over it, else a view click: on the party at
    /// rest, an entry; elsewhere, a journey there.
    pub(crate) fn world_map_release<E: WorldEdition>(&mut self, point: Option<[i32; 2]>) -> bool {
        let Some(world) = E::screen_mut(self) else {
            return false;
        };
        if world.release_taken() {
            return true;
        }
        if let Some(button) = world.take_held() {
            if point.is_some_and(|point| E::button_contains(button, point)) {
                E::world_button(self, button);
            }
            return true;
        }
        let Some(point) = point.filter(|point| screen::view_click(*point)) else {
            return false;
        };
        let view = world.view;
        let Some(position) = E::position(self) else {
            return true;
        };
        let at_rest = E::destination(self).is_none();
        let party = screen::window(position, view);
        if at_rest && screen::hovers(point, party) {
            self.play_effect(ENTER_SOUND, None);
            E::party_click(self);
            return true;
        }
        self.play_effect(CLICK_SOUND, None);
        E::travel_to(self, screen::world_at(point, view));
        if let Some(world) = E::screen_mut(self) {
            world.follow = at_rest;
            world.journey = None;
        }
        true
    }
    /// Start the pressed-triangle wait before entering (SPEC-014 §3.39):
    /// the journey stops and the dots clear.
    pub(crate) fn world_map_entry<E: WorldEdition>(&mut self, wait: u64) {
        let place = E::party_place(self);
        let until = self.clock + wait;
        E::stop_travel(self);
        if let Some(world) = E::screen_mut(self) {
            world.clear_dots();
            world.pending = Some(E::entering(until, place));
        }
    }
    /// A town or city button's event (on release, SPEC-014 §3.39): a
    /// journey to a place the party is not in, with follow mode on unless
    /// a journey was under way; else an entry, after the longer wait when
    /// the party was travelling.
    pub(crate) fn world_map_head_for<E: WorldEdition>(&mut self, place: usize, travelling: bool) {
        if !E::party_in(self, place) {
            if E::travel_to_place(self, place)
                && let Some(world) = E::screen_mut(self)
            {
                world.follow = !travelling;
                world.journey = None;
            }
            return;
        }
        self.world_map_entry::<E>(if travelling {
            screen::ENTER_TRAVELLING_MS
        } else {
            screen::ENTER_MS
        });
    }
    /// One world-map pass, at most one per 41 milliseconds: edge scrolling,
    /// two travel iterations, the campaign checks, follow mode and the
    /// hover state. A pending entry or encounter flash holds the screen
    /// until it ends.
    pub(crate) fn world_map_tick<E: WorldEdition>(&mut self, now: u64) -> AppResult<bool> {
        let Some(bounds) = E::bounds(self) else {
            return Ok(false);
        };
        // H3: a pointer in the letterbox (clamped onto the frame's edge) is
        // not on an edge.
        let edge_pointer = self.pointer.at.filter(|_| !self.pointer.outside);
        let Some(world) = E::screen_mut(self) else {
            return Ok(false);
        };
        if let Some(pending) = world.pending {
            match E::hold(pending) {
                Hold::Enter { until, place } => {
                    if now >= until {
                        world.pending = None;
                        E::enter_location(self, place);
                    }
                }
                Hold::Flash { start, encounter } => {
                    if now >= start + screen::FLASH_IMAGES * screen::FLASH_MS {
                        world.pending = None;
                        E::enter_encounter(self, encounter);
                    }
                }
            }
            return Ok(true);
        }
        if !world.begin_pass(now, edge_pointer, bounds) {
            return Ok(false);
        }
        let mut travelled = false;
        for _ in 0..2 {
            let leg = E::travel_iteration(self)?;
            let mark = E::travel_mark(self);
            let Some(world) = E::screen_mut(self) else {
                return Ok(true);
            };
            if let Some(pixel) = mark {
                world.mark(pixel);
            }
            travelled |= !matches!(leg, Leg::Idle);
            match leg {
                Leg::Moving => continue,
                Leg::Idle => break,
                Leg::Stopped => {
                    world.clear_dots();
                    world.journey = None;
                    break;
                }
                Leg::OutOfFuel => {
                    world.clear_dots();
                    world.journey = None;
                    E::out_of_fuel(self);
                    break;
                }
                Leg::Interrupted => {
                    world.clear_dots();
                    let place = E::party_place(self);
                    E::enter_location(self, place);
                    return Ok(true);
                }
                Leg::Encounter(encounter) => {
                    world.clear_dots();
                    world.journey = None;
                    if !E::flashes(encounter) {
                        E::enter_encounter(self, encounter);
                        return Ok(true);
                    }
                    let position = E::position(self).unwrap_or([0, 0]);
                    if let Some(world) = E::screen_mut(self) {
                        world.view = bounds.centered(position);
                        world.pending = Some(E::flashing(now, encounter));
                    }
                    return Ok(true);
                }
            }
        }
        if E::campaign_checks(self)? {
            return Ok(true);
        }
        let Some(position) = E::position(self) else {
            return Ok(true);
        };
        let at_rest = E::destination(self).is_none();
        let pointer = self.pointer.at;
        E::settle(self);
        let Some(world) = E::screen_mut(self) else {
            return Ok(true);
        };
        world.settle_pass(position, at_rest, pointer, travelled, bounds);
        Ok(true)
    }
    /// The world-map cursor: the scroll art on an edge (the blocked art
    /// when every requested axis is at its limit), else the arrow.
    pub(crate) fn world_map_cursor<E: WorldEdition>(&self, at: [i32; 2]) -> &'static str {
        let (Some(world), Some(bounds)) = (E::screen(self), E::bounds(self)) else {
            return cursor::ARROW.art;
        };
        world.cursor(at, bounds)
    }
    /// The world-map frame: the terrain, the markers by state, the frame,
    /// the side panel, and the pause window over the greyed view.
    pub(crate) fn world_map_image<E: WorldEdition>(&self) -> AppResult<IndexedImage> {
        let (Some(world), Some(bounds), Some(position)) =
            (E::screen(self), E::bounds(self), E::position(self))
        else {
            return Err("world map screen is not open".into());
        };
        let painter = self.painter();
        let mut image = IndexedImage::new(640, 480)?;
        image.fill_rect([0, 0, 640, 480], painter.color(ColorKey::BLACK));
        let hold = world.pending.map(E::hold);
        // An encounter recenters the view and flashes without markers; an
        // entry shows the pressed triangle without recentering.
        let view = match hold {
            Some(Hold::Flash { .. }) => bounds.centered(position),
            _ => world.view,
        };
        E::draw_terrain(self, &painter, &mut image, view);
        let party = screen::window(position, view);
        match hold {
            None => {
                if let Some(destination) = E::destination(self) {
                    self.draw_marker(&painter, &mut image, PLUS_ART, Marker::Plus, party);
                    let target = screen::window(destination, view);
                    self.draw_marker(&painter, &mut image, TARGET_ART, Marker::Crosshair, target);
                    self.draw_travel_dots(&painter, &mut image, view, |pixel| world.marked(pixel));
                } else {
                    self.draw_marker(&painter, &mut image, TRIANGLE_ART, Marker::Triangle, party);
                    if world.hover {
                        E::draw_hover_label(self, &painter, &mut image, party);
                    }
                }
            }
            Some(Hold::Enter { .. }) => {
                self.draw_marker(
                    &painter,
                    &mut image,
                    PRESSED_TRIANGLE_ART,
                    Marker::Triangle,
                    party,
                );
                if self.pointer.at.is_some_and(|at| screen::hovers(at, party)) {
                    E::draw_hover_label(self, &painter, &mut image, party);
                }
            }
            Some(Hold::Flash { start, encounter }) => {
                let second = (self.clock.saturating_sub(start) / screen::FLASH_MS) % 2 == 1;
                if E::special(encounter) {
                    let art = if second { TARGET_ART } else { DARK_TARGET_ART };
                    self.draw_marker(&painter, &mut image, art, Marker::Crosshair, party);
                } else {
                    let art = BOLT_ART[usize::from(second)];
                    self.draw_marker(&painter, &mut image, art, Marker::Bolt, party);
                }
            }
        }
        painter.blit(&mut image, FRAME_ART, 0, [0, 0]);
        E::draw_panel(self, &painter, &mut image, world);
        // G-24 / SPEC-014 §3.17: the view greys, then the pause window
        // shows at (185, 180). The conversion is not undone when it closes:
        // the region stays grey until the screen next redraws its view.
        if world.paused || world.grey.is_some() {
            self.grey_view(&mut image);
        }
        if world.paused {
            crate::map_preview::options::draw_pause(
                &painter,
                &mut image,
                vault_core::ui::options::PAUSE_WORLD_ORIGIN,
            );
        }
        Ok(image)
    }
    /// A press on the town map: a marker plays `ib2p1xx1`, a side-panel
    /// button its red-button cues; each acts on its release.
    pub(crate) fn town_map_click<E: WorldEdition>(&mut self, point: [i32; 2], right: bool) -> bool {
        let Some(town) = E::town(self) else {
            return false;
        };
        if right {
            return true;
        }
        let marker = E::marker_at(self, town, point);
        let Some(town) = E::town_mut(self) else {
            return true;
        };
        let press = E::town_press(town);
        if let Some(index) = marker {
            press.pressed = Some(index);
            self.play_effect(ENTER_SOUND, None);
            return true;
        }
        if let Some(button) = E::button_at(point) {
            press.held = Some(button);
            self.press();
        }
        true
    }
    /// A left release on the town map: the pressed marker's choice, or the
    /// held side-panel button's event, when the release is over it.
    pub(crate) fn town_map_release<E: WorldEdition>(&mut self, point: Option<[i32; 2]>) -> bool {
        let Some(town) = E::town_mut(self) else {
            return false;
        };
        let press = E::town_press(town);
        let (pressed, held) = (press.pressed.take(), press.held.take());
        let Some(point) = point else {
            return pressed.is_some() || held.is_some();
        };
        if let Some(index) = pressed {
            let over =
                E::town(self).is_some_and(|town| E::marker_at(self, town, point) == Some(index));
            if over {
                E::town_choice(self, index);
            }
            return true;
        }
        let Some(button) = held else {
            return false;
        };
        if E::button_contains(button, point) {
            E::town_button(self, button);
        }
        true
    }
}
