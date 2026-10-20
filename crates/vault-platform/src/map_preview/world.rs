//! SPEC-008 §3.4-3.7 world map and town map screens. The world view is the
//! 450 x 442 viewport at (22,21) over the 1400 x 1500 world, scroll origins
//! clamped to 0-950 / 0-1058. The specifications name no world artwork, so
//! cells are drawn from the fixed terrain grid and their discovery values
//! (unknown black, seen dim, visited full). Travel runs at most two loop
//! iterations per update, about 24 updates per second.
use super::{MapPreview, Screen, button, dialog::Dialog, label, panel_text};
use crate::AppResult;
use vault_core::{
    TravelOutcome,
    assets::WorldAssets,
    worldmap::{
        self, CELL, COLUMNS, ROWS, TOWNS, Terrain, TownButton, TownEntry, VIEWPORT_ORIGIN,
        VIEWPORT_SIZE,
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Milliseconds between travel updates (about 24 per second).
const TRAVEL_UPDATE_MS: u64 = 42;
/// The side panel's buttons: towns, then "Enter", then "Close".
const PANEL_X: i32 = 488;
const BUTTON_WIDTH: i32 = 140;
const BUTTON_HEIGHT: i32 = 16;
const TOWN_BUTTONS_Y: i32 = 60;

pub(crate) struct WorldScreen {
    /// World-pixel origin of the viewport.
    pub scroll: [i32; 2],
    /// The town whose jittered center travel is heading for.
    pub town: Option<usize>,
    last_update: u64,
}
pub(crate) struct TownScreen {
    pub town: usize,
    pub picture: i32,
    pub buttons: Vec<TownButton>,
}

/// The viewport origin that centers a world point.
fn centered(point: [i32; 2]) -> [i32; 2] {
    worldmap::clamp_scroll([
        point[0] - VIEWPORT_SIZE[0] / 2,
        point[1] - VIEWPORT_SIZE[1] / 2,
    ])
}
fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    (rect[0]..rect[0] + rect[2]).contains(&point[0])
        && (rect[1]..rect[1] + rect[3]).contains(&point[1])
}
fn town_button(index: usize) -> [i32; 4] {
    [
        PANEL_X,
        TOWN_BUTTONS_Y + index as i32 * (BUTTON_HEIGHT + 4),
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
    ]
}
const ENTER_BUTTON: [i32; 4] = [PANEL_X, 400, BUTTON_WIDTH, BUTTON_HEIGHT];
const CLOSE_BUTTON: [i32; 4] = [PANEL_X, 440, BUTTON_WIDTH, BUTTON_HEIGHT];

impl MapPreview {
    /// Towns shown on the map and the side panel: those enabled.
    fn known_towns(&self) -> Vec<usize> {
        (0..TOWNS.len())
            .filter(|town| self.host.world.world_map.town_enabled(*town))
            .collect()
    }
    pub(crate) fn open_world_map(&mut self) {
        self.host.refresh_world_knowledge();
        let position = self.host.world.world_map.position();
        self.screen = Screen::World(WorldScreen {
            scroll: centered(position),
            town: None,
            last_update: 0,
        });
    }
    /// The current town's map (SPEC-013 §3.22 map -1). Under the safe rule
    /// a town out of range, or one without its picture, opens the world
    /// map at the current town instead.
    pub(crate) fn open_town_map(&mut self) {
        let town = self.host.world.world_map.saved.town.max(0) as usize;
        let picture = self
            .host
            .assets
            .interface_path(worldmap::TOWN_PICTURE_BASE + town as i32)
            .and_then(|path| self.host.assets.frame(&path));
        if town >= TOWNS.len() || picture.is_err() {
            self.open_world_map();
            return;
        }
        let gvars = self.host.world.gvars.clone().unwrap_or_default();
        let buttons = self.host.world.world_map.town_buttons(town, &gvars);
        self.screen = Screen::Town(TownScreen {
            town,
            picture: worldmap::TOWN_PICTURE_BASE + town as i32,
            buttons,
        });
    }
    /// SPEC-013 §3.22 the outgoing map could not be saved for the town or
    /// world map: `map.msg` 1000 in a large box at (169, 116) with the
    /// error sound; closing it returns to the main menu.
    pub(crate) fn map_save_failed(&mut self) {
        self.play_effect(vault_core::ui::ERROR_SOUND.as_bytes(), None);
        let title = self.message("map.msg", 1000);
        let dialog = Dialog::done(
            [169, 116],
            title,
            Vec::new(),
            vault_core::ui::ColorKey::ERROR,
        );
        self.show_dialog(dialog.quitting());
    }
    /// Head for a town: its center with signed 0-16 pixel jitter per axis.
    fn travel_to_town(&mut self, town: usize) {
        match worldmap::town_target(town, &mut self.host.world.rng) {
            Ok(target) => {
                self.host.travel_to(target);
                if let Screen::World(world) = &mut self.screen {
                    world.town = Some(town);
                }
            }
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// Arriving in (or entering) a town cell: a first entry loads its
    /// slot-zero map, a later one shows the town map.
    fn enter_town(&mut self, town: usize) {
        match self.host.enter_town(town) {
            Ok(Some(TownEntry::Direct(_))) => self.return_to_map(),
            Ok(Some(TownEntry::TownMap { picture, buttons })) => {
                self.screen = Screen::Town(TownScreen {
                    town,
                    picture,
                    buttons,
                });
            }
            Ok(None) => {}
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    pub(crate) fn world_key(&mut self, key: KeyCode) -> bool {
        let Screen::World(world) = &mut self.screen else {
            return false;
        };
        let delta = match key {
            KeyCode::ArrowLeft => [-CELL, 0],
            KeyCode::ArrowRight => [CELL, 0],
            KeyCode::ArrowUp => [0, -CELL],
            KeyCode::ArrowDown => [0, CELL],
            // A map left for the world map stays closed until a location
            // is entered (SPEC-013 §3.22).
            KeyCode::Escape if self.host.world.unloaded => return true,
            KeyCode::Escape => {
                self.return_to_map();
                return true;
            }
            KeyCode::Enter => {
                self.enter_here();
                return true;
            }
            _ => return false,
        };
        world.scroll =
            worldmap::clamp_scroll([world.scroll[0] + delta[0], world.scroll[1] + delta[1]]);
        true
    }
    /// The panel's "Enter": the town at the current cell, or a voluntary
    /// terrain dropout elsewhere.
    fn enter_here(&mut self) {
        let town = self.host.world.world_map.cell().and_then(worldmap::town_at);
        if let Some(town) = town {
            self.enter_town(town);
            return;
        }
        let entered = self
            .host
            .terrain_dropout()
            .and_then(|encounter| self.host.enter_encounter(encounter));
        match entered {
            Ok(()) => self.return_to_map(),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    pub(crate) fn world_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::World(world) = &self.screen else {
            return false;
        };
        let scroll = world.scroll;
        let viewport = [
            VIEWPORT_ORIGIN[0],
            VIEWPORT_ORIGIN[1],
            VIEWPORT_SIZE[0],
            VIEWPORT_SIZE[1],
        ];
        if inside(viewport, point) {
            let target = [
                scroll[0] + point[0] - VIEWPORT_ORIGIN[0],
                scroll[1] + point[1] - VIEWPORT_ORIGIN[1],
            ];
            let town = worldmap::cell_of(target)
                .and_then(worldmap::town_at)
                .filter(|town| self.host.world.world_map.town_enabled(*town));
            match town {
                Some(town) => self.travel_to_town(town),
                None => {
                    self.host.travel_to(target);
                    if let Screen::World(world) = &mut self.screen {
                        world.town = None;
                    }
                }
            }
            return true;
        }
        let towns = self.known_towns();
        if let Some(index) = (0..towns.len()).find(|index| inside(town_button(*index), point)) {
            self.press();
            self.travel_to_town(towns[index]);
            return true;
        }
        if inside(ENTER_BUTTON, point) {
            self.press();
            self.enter_here();
            return true;
        }
        if inside(CLOSE_BUTTON, point) && !self.host.world.unloaded {
            self.press();
            self.return_to_map();
            return true;
        }
        false
    }
    /// Advance travel: up to two loop iterations per update. Arrival in a
    /// town cell enters it; encounters load their map; queue interrupts
    /// return to the local map.
    pub(crate) fn world_tick(&mut self, now: u64) -> AppResult<bool> {
        let Screen::World(world) = &mut self.screen else {
            return Ok(false);
        };
        if self.host.world.world_map.travel.is_none()
            || now.saturating_sub(world.last_update) < TRAVEL_UPDATE_MS
        {
            return Ok(false);
        }
        world.last_update = now;
        for _ in 0..2 {
            match self.host.travel_iteration()? {
                TravelOutcome::Moving => continue,
                TravelOutcome::Idle => break,
                TravelOutcome::Arrived => {
                    if let Some(town) = self.host.world.world_map.cell().and_then(worldmap::town_at)
                    {
                        self.enter_town(town);
                    }
                    break;
                }
                TravelOutcome::Blocked => {
                    self.status = b"The way is blocked".to_vec();
                    break;
                }
                TravelOutcome::Interrupted(_) => {
                    self.return_to_map();
                    break;
                }
                TravelOutcome::Encounter(encounter) => {
                    match self.host.enter_encounter(encounter) {
                        Ok(()) => self.return_to_map(),
                        Err(error) => self.status = error.to_string().into_bytes(),
                    }
                    break;
                }
            }
        }
        // Keep the traveller in view.
        let position = self.host.world.world_map.position();
        if let Screen::World(world) = &mut self.screen {
            let local = [position[0] - world.scroll[0], position[1] - world.scroll[1]];
            if !(40..VIEWPORT_SIZE[0] - 40).contains(&local[0])
                || !(40..VIEWPORT_SIZE[1] - 40).contains(&local[1])
            {
                world.scroll = centered(position);
            }
        }
        Ok(true)
    }
    pub(crate) fn world_image(&self) -> AppResult<IndexedImage> {
        let Screen::World(world) = &self.screen else {
            return Err("world map screen is not open".into());
        };
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let color = |rgb| palette.lookup_rgb5(rgb).unwrap_or(0);
        let terrain = |kind: Terrain| match kind {
            Terrain::Desert => color([24, 20, 12]),
            Terrain::Mountain => color([16, 12, 8]),
            Terrain::City => color([18, 18, 18]),
            Terrain::Coast => color([8, 14, 20]),
        };
        let map = &self.host.world.world_map;
        let [x0, y0] = world.scroll;
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let left = column as i32 * CELL - x0;
                let top = row as i32 * CELL - y0;
                if left >= VIEWPORT_SIZE[0]
                    || top >= VIEWPORT_SIZE[1]
                    || left <= -CELL
                    || top <= -CELL
                {
                    continue;
                }
                let fill = match map.discovery([column, row]) {
                    0 => 0,
                    1 => palette.intensity(terrain(worldmap::terrain([column, row])), 64),
                    _ => terrain(worldmap::terrain([column, row])),
                };
                let clipped = [
                    left.max(0),
                    top.max(0),
                    (left + CELL).min(VIEWPORT_SIZE[0]) - left.max(0),
                    (top + CELL).min(VIEWPORT_SIZE[1]) - top.max(0),
                ];
                image.fill_rect(
                    [
                        VIEWPORT_ORIGIN[0] + clipped[0],
                        VIEWPORT_ORIGIN[1] + clipped[1],
                        clipped[2],
                        clipped[3],
                    ],
                    fill,
                );
            }
        }
        let to_screen = |point: [i32; 2]| {
            let local = [point[0] - x0, point[1] - y0];
            ((0..VIEWPORT_SIZE[0]).contains(&local[0]) && (0..VIEWPORT_SIZE[1]).contains(&local[1]))
                .then(|| [VIEWPORT_ORIGIN[0] + local[0], VIEWPORT_ORIGIN[1] + local[1]])
        };
        let white = color([31, 31, 31]);
        let green = color([0, 31, 0]);
        for town in self.known_towns() {
            let [column, row] = TOWNS[town].cell;
            let center = [column as i32 * CELL + 25, row as i32 * CELL + 25];
            if let Some([x, y]) = to_screen(center) {
                image.fill_rect([x - 3, y - 3, 7, 7], white);
                if let Some(font) = &self.font {
                    label(
                        &mut image,
                        font,
                        palette,
                        &self.location_name(town),
                        [x + 6, y - 4],
                        white,
                    );
                }
            }
        }
        if let Some(travel) = map.travel
            && let Some([x, y]) = to_screen(travel.destination)
        {
            image.fill_rect([x - 1, y - 4, 3, 9], green);
            image.fill_rect([x - 4, y - 1, 9, 3], green);
        }
        if let Some([x, y]) = to_screen(map.position()) {
            image.fill_rect([x - 2, y - 2, 5, 5], color([31, 0, 0]));
        }
        if let Some(font) = &self.font {
            panel_text(
                &mut image,
                font,
                palette,
                &self.clock_text(),
                [PANEL_X, 24],
                green,
            );
            for (index, town) in self.known_towns().into_iter().enumerate() {
                button(
                    &mut image,
                    font,
                    palette,
                    &self.location_name(town),
                    town_button(index),
                );
            }
            button(&mut image, font, palette, b"Enter", ENTER_BUTTON);
            if !self.host.world.unloaded {
                button(&mut image, font, palette, b"Close", CLOSE_BUTTON);
            }
            panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        }
        Ok(image)
    }
    pub(crate) fn town_key(&mut self, key: KeyCode) -> bool {
        if key != KeyCode::Escape {
            return false;
        }
        self.open_world_map();
        true
    }
    pub(crate) fn town_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::Town(town) = &self.screen else {
            return false;
        };
        if inside(CLOSE_BUTTON, point) {
            self.press();
            self.open_world_map();
            return true;
        }
        let Some(button) = town
            .buttons
            .iter()
            .find(|button| inside(button.rect, point))
            .copied()
        else {
            return false;
        };
        let town = town.town;
        self.press();
        match self.host.select_town_entrance(town, button) {
            Ok(()) => self.return_to_map(),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        true
    }
    pub(crate) fn town_image(&self) -> AppResult<IndexedImage> {
        let Screen::Town(town) = &self.screen else {
            return Err("town map screen is not open".into());
        };
        let palette = &self.palette;
        let mut image = IndexedImage::new(640, 480)?;
        let green = palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
        // The town picture: interface art 156 + town, drawn at (20,20).
        let picture = self
            .host
            .assets
            .interface_path(town.picture)
            .and_then(|path| self.host.assets.frame(&path));
        match picture.as_ref().map(|art| art.frame(0, 0)) {
            Ok(Some(frame)) => image.blit(frame, [20, 20]),
            _ => {
                if let Some(font) = &self.font {
                    panel_text(
                        &mut image,
                        font,
                        palette,
                        b"Town picture unavailable",
                        [24, 24],
                        green,
                    );
                }
            }
        }
        if let Some(font) = &self.font {
            let labels = self.host.assets.game_messages("worldmap.msg").ok();
            for entry in &town.buttons {
                let text = labels
                    .as_ref()
                    .and_then(|catalog| catalog.get(entry.label))
                    .map_or_else(Vec::new, |message| message.text.clone());
                button(&mut image, font, palette, &text, entry.rect);
            }
            button(&mut image, font, palette, b"World map", CLOSE_BUTTON);
            panel_text(&mut image, font, palette, &self.status, [22, 466], green);
        }
        Ok(image)
    }
}
