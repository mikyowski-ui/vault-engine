use crate::{
    AppResult,
    audio::{AudioHandle, Channel},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vault_core::{
    CharacterSheet, ObjectAction, ObjectRef, Perk, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    movie::{MoviePolicy, MovieRequest},
};
use vault_formats::{
    ArtCatalog, Fid, Font, FrmArchive, HexGrid, MAX_ASSET_BYTES, MapFile, Palette, Vfs, VfsPath,
    acm,
    pro::{BasicFlags, Category},
    sfx,
};
use vault_render::{
    IndexedImage, TextStyle,
    dialogue::{self, ChoiceHit},
    map::{MapArt, MapHover, MapView, pick_object, render_map},
};
use winit::keyboard::KeyCode;

mod barter;
mod character;
mod inventory;
mod movies;
mod pipboy;
mod world;

const MAX_ART_BYTES: u64 = 256 * 1024 * 1024;
/// The effect volume setting S (SPEC-010 §3.11): the desktop plays effects
/// at the full setting.
const EFFECT_SETTING: i32 = 32767;
/// Fixed interface button cues (small red button press/release).
const BUTTON_PRESS: &[u8] = b"ib1p1xx1";
const BUTTON_RELEASE: &[u8] = b"ib1lu1x1";
/// Decoded samples kept in memory before the cache is cleared.
const MAX_CACHED_SAMPLES: usize = 32 * 1024 * 1024;

/// The screen in front: the local map (with dialogue), the SPEC-008 world
/// and town maps, the Pip-Boy, the SPEC-011 inventory, character, and
/// barter screens, or a SPEC-012 movie.
pub(crate) enum Screen {
    Map,
    World(world::WorldScreen),
    Town(world::TownScreen),
    PipBoy(pipboy::PipBoy),
    Inventory(inventory::InventoryScreen),
    Character(Box<character::CharacterScreen>),
    Barter(Box<barter::BarterScreen>),
    Movie(Box<movies::MovieScreen>),
}

/// Decoded ACM samples by VFS path; a failing path is reported once.
#[derive(Default)]
struct SoundBank {
    cache: BTreeMap<String, Arc<[i16]>>,
    samples: usize,
    failed: BTreeSet<String>,
}
impl SoundBank {
    fn load(&mut self, vfs: &dyn Vfs, path: &str) -> Option<Arc<[i16]>> {
        if let Some(samples) = self.cache.get(path) {
            return Some(samples.clone());
        }
        if self.failed.contains(path) {
            return None;
        }
        let decoded = vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| acm::decode(&bytes).map_err(|error| error.to_string()));
        match decoded {
            Ok(decoded) => {
                let samples: Arc<[i16]> = decoded.samples.into();
                if self.samples + samples.len() > MAX_CACHED_SAMPLES {
                    self.cache.clear();
                    self.samples = 0;
                }
                self.samples += samples.len();
                self.cache.insert(path.to_owned(), samples.clone());
                Some(samples)
            }
            Err(error) => {
                tracing::warn!(path, %error, "sound unavailable");
                self.failed.insert(path.to_owned());
                None
            }
        }
    }
}

fn style(color: u8) -> TextStyle {
    TextStyle {
        foreground: color,
        ..Default::default()
    }
}
/// A desktop UI button: a dark panel and its label.
fn button(image: &mut IndexedImage, font: &Font, palette: &Palette, text: &[u8], rect: [i32; 4]) {
    image.fill_rect(rect, palette.lookup_rgb5([0, 5, 0]).unwrap_or(0));
    image.draw_text(
        font,
        palette,
        text,
        [rect[0] + 3, rect[1] + 2],
        (rect[2] - 6).max(0) as u32,
        style(palette.lookup_rgb5([0, 31, 0]).unwrap_or(1)),
    );
}
/// Short map labels.
fn label(
    image: &mut IndexedImage,
    font: &Font,
    palette: &Palette,
    text: &[u8],
    origin: [i32; 2],
    color: u8,
) {
    image.draw_text(font, palette, text, origin, 160, style(color));
}
/// Status and panel lines.
fn panel_text(
    image: &mut IndexedImage,
    font: &Font,
    palette: &Palette,
    text: &[u8],
    origin: [i32; 2],
    color: u8,
) {
    image.draw_text(font, palette, text, origin, 600, style(color));
}

pub struct MapPreview {
    pub host: WorldHost<VfsWorldAssets>,
    pub palette: Palette,
    art: MapSprites,
    pub view: MapView,
    path: String,
    last_hover: Option<MapHover>,
    font: Option<Arc<Font>>,
    pub selected: Option<u32>,
    status: Vec<u8>,
    choices: Vec<ChoiceHit>,
    option_scroll: usize,
    pub(crate) screen: Screen,
    /// Audio output (silent until the shell attaches a device).
    pub audio: AudioHandle,
    sounds: SoundBank,
    /// A talking-head speech voice is playing.
    speaking: bool,
    /// A desktop button is held: its release plays the release cue.
    pressed: bool,
    /// Game movies waiting for the one playing.
    movies: VecDeque<MovieRequest>,
}
struct MapSprites {
    bindings: BTreeMap<(Category, u32, u8), Arc<FrmArchive>>,
}
impl MapArt for MapSprites {
    fn frame(
        &self,
        category: Category,
        art_id: u32,
        orientation: u8,
        frame: usize,
    ) -> Option<(&vault_formats::frm::FrmFrame, [i16; 2])> {
        let archive = self.bindings.get(&(category, art_id, orientation))?;
        Some((
            archive
                .frame(orientation as usize, frame)
                .or_else(|| archive.frame(orientation as usize, 0))?,
            *archive.shifts.get(orientation as usize)?,
        ))
    }
}
impl MapSprites {
    fn load(vfs: &dyn Vfs, map: &MapFile) -> AppResult<Self> {
        let mut needed = BTreeSet::new();
        for tiles in map.tiles.iter().flatten() {
            for tile in tiles {
                for (index, skip) in [
                    (tile.floor(), tile.floor_hidden()),
                    (tile.roof(), tile.roof_hidden() || tile.roof() == 1),
                ] {
                    if !skip {
                        needed.insert((Category::Tile, u32::from(index), 0_u8));
                    }
                }
            }
        }
        for object in map.objects.elevations.iter().flatten() {
            if !object.flags.contains(BasicFlags::HIDDEN) {
                needed.insert((
                    Category::from_pid(object.pid)?,
                    object.art_id,
                    object.orientation,
                ));
            }
        }
        let catalog = ArtCatalog::load(vfs)?;
        let files: BTreeSet<&str> = vfs.files().map(VfsPath::as_str).collect();
        let mut archives: BTreeMap<String, Arc<FrmArchive>> = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        let mut bytes_loaded = 0_u64;
        for key @ (category, id, _) in needed {
            // Tile squares store a bare list index; objects store a complete FID,
            // whose own category bits select the art directory and codec.
            let fid = if category == Category::Tile {
                Fid::new(4, id as u16, 0, 0, 0)
            } else {
                Fid(id)
            };
            let resolved = catalog
                .path(fid)
                .map_err(|error| format!("art identifier {:#010x}: {error}", fid.0))?;
            let resolved = VfsPath::new(&resolved)?.to_string();
            if category != Category::Tile && !files.contains(resolved.as_str()) {
                // The reference has no guaranteed final fallback resource.
                tracing::warn!(art_id = fid.0, sprite = %resolved, "object art is missing; not drawn");
                continue;
            }
            let archive = if let Some(archive) = archives.get(&resolved) {
                archive.clone()
            } else {
                let bytes = vfs.read_bounded(&resolved, MAX_ASSET_BYTES)?;
                bytes_loaded += bytes.len() as u64;
                if bytes_loaded > MAX_ART_BYTES {
                    return Err("map preview art exceeds 256 MiB limit".into());
                }
                let archive = Arc::new(
                    FrmArchive::parse(&bytes).map_err(|error| format!("{resolved}: {error}"))?,
                );
                archives.insert(resolved, archive.clone());
                archive
            };
            bindings.insert(key, archive);
        }
        let art = MapSprites { bindings };
        for object in map.objects.elevations.iter().flatten() {
            let category = Category::from_pid(object.pid)?;
            if let Some(archive) = art
                .bindings
                .get(&(category, object.art_id, object.orientation))
                && object.frame >= archive.frame_count()
            {
                tracing::warn!(
                    object = object.id,
                    frame = object.frame,
                    "frame unavailable in preview representation; showing frame zero"
                );
            }
        }
        Ok(art)
    }
}
impl MapPreview {
    pub fn load(
        vfs: Arc<dyn Vfs>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
    ) -> AppResult<Self> {
        let path = VfsPath::new(path)?;
        let palette = Palette::parse(&vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?)?;
        let assets = VfsWorldAssets::new(vfs, language)?;
        let character = if let Some(path) = character {
            CharacterSheet::from_gcd(&assets.gcd(path)?, Default::default())
        } else {
            CharacterSheet::default()
        };
        let font = assets.font().ok();
        let mut host = WorldHost::new(WorldState::new(character, seed)?, assets);
        host.interactive_dialogue = true;
        // The desktop plays every movie and reports its end.
        host.movie_policy = MoviePolicy::WaitUntilFinished;
        // A preview can run without campaign globals; the failure is reported.
        if let Err(error) = host.load_campaign() {
            tracing::warn!(%error, "campaign initializer unavailable");
        }
        // SPEC-009 AI initialization; without it NPC combat turns are skipped.
        if let Err(error) = host.load_combat_ai() {
            tracing::warn!(%error, "combat AI unavailable");
        }
        host.load_map(path.as_str())?;
        let map = host.world.render_snapshot();
        let art = MapSprites::load(&*host.assets.vfs, &map)?;
        let mut view = MapView::new(&map, 640, 380)?;
        view.grid = HexGrid::centered(host.world.camera_hex, 640, 380)?;
        view.elevation = host.world.elevation;
        tracing::info!(map=%path,objects=map.objects.total,"interactive map preview loaded");
        Ok(Self {
            host,
            palette,
            art,
            view,
            path: path.to_string(),
            last_hover: None,
            font,
            selected: None,
            status: Vec::new(),
            choices: Vec::new(),
            option_scroll: 0,
            screen: Screen::Map,
            audio: AudioHandle::default(),
            sounds: SoundBank::default(),
            speaking: false,
            pressed: false,
            movies: VecDeque::new(),
        })
    }
    /// Back to the (possibly newly loaded) local map.
    pub(crate) fn return_to_map(&mut self) {
        self.screen = Screen::Map;
        self.last_hover = None;
        self.selected = None;
        if let Some(name) = self.host.world.current_map_name() {
            self.path = name;
        }
    }
    /// A desktop button press: its cue, and the release cue to follow.
    pub(crate) fn press(&mut self) {
        self.pressed = true;
        self.play_effect(BUTTON_PRESS, None);
    }
    /// The mouse button was released.
    pub fn release(&mut self) -> bool {
        if self.movie_release() {
            self.pressed = false;
            return true;
        }
        if !std::mem::take(&mut self.pressed) {
            return false;
        }
        self.play_effect(BUTTON_RELEASE, None);
        true
    }
    pub(crate) fn location_name(&self, town: usize) -> Vec<u8> {
        vault_core::worldmap::TOWNS
            .get(town)
            .map_or_else(Vec::new, |town| town.name.as_bytes().to_vec())
    }
    /// Game date and time, `MM/DD/YYYY HH:MM`.
    pub(crate) fn clock_text(&self) -> Vec<u8> {
        let world = &self.host.world;
        let [month, day, year] =
            vault_core::host_save::game_date(world.clock.ticks, world.rules.calendar_year);
        let time = world.clock.hhmm();
        format!(
            "{month:02}/{day:02}/{year:04} {:02}:{:02}",
            time / 100,
            time % 100
        )
        .into_bytes()
    }
    /// Whether an object (or its outermost owner) lies in the displayed map
    /// window, for SPEC-010 relative volume.
    fn on_screen(&self, object: ObjectRef) -> bool {
        let world = &self.host.world;
        let mut object = object;
        for _ in 0..64 {
            match world.get(object).and_then(|found| found.owner) {
                Some(owner) => object = owner,
                None => break,
            }
        }
        let Some(found) = world.get(object) else {
            return false;
        };
        found.raw.elevation == self.view.elevation
            && found
                .raw
                .hex
                .and_then(|hex| self.view.grid.hex_center(hex).ok())
                .is_some_and(|[x, y]| (-32..672).contains(&x) && (-32..412).contains(&y))
    }
    /// A SPEC-010 effect: resolve the name through the lookup chain (with
    /// the owner's gender alias), decode, and play at the relative volume.
    fn play_effect(&mut self, name: &[u8], owner: Option<ObjectRef>) {
        let Some(path) = self.host.effect_file(name, owner) else {
            tracing::debug!(name = %String::from_utf8_lossy(name), "no effect file");
            return;
        };
        let Some(samples) = self.sounds.load(&*self.host.assets.vfs, &path) else {
            return;
        };
        let on_screen = owner.is_some_and(|owner| self.on_screen(owner));
        let volume = sfx::applied_volume(
            self.host.world.effect_volume(owner, on_screen),
            EFFECT_SETTING,
        );
        if !self
            .audio
            .play(samples, volume as f32 / 32767.0, Channel::Effect, false)
        {
            tracing::debug!(%path, "effect not played (silent output or four effects active)");
        }
    }
    /// Talking-head speech: `sound/speech/<head>/<stem>.ACM`. Without a file
    /// or output, speech finishes at once.
    fn play_speech(&mut self, sound: &[u8]) {
        let started = self
            .host
            .speech_file(sound)
            .and_then(|path| self.sounds.load(&*self.host.assets.vfs, &path))
            .is_some_and(|samples| self.audio.play(samples, 1.0, Channel::Speech, false));
        if started {
            self.speaking = true;
        } else {
            self.host.speech_finished();
        }
    }
    /// A library sound slot: the named file with its looping mode bit.
    fn play_library(&mut self, slot: usize, filename: &[u8], mode: i32) {
        let path = String::from_utf8_lossy(filename).replace('\\', "/");
        if let Some(samples) = self.sounds.load(&*self.host.assets.vfs, &path) {
            self.audio
                .play(samples, 1.0, Channel::Library(slot), mode & 1 != 0);
        }
    }
    /// Audio follows the world: finished speech completes the head's speech
    /// phase, and stopped, paused, or freed library slots fall silent.
    fn update_audio(&mut self) {
        if self.speaking && !self.audio.playing(Channel::Speech) {
            self.speaking = false;
            self.host.speech_finished();
        }
        for (slot, sound) in self.host.library.sounds.iter().enumerate() {
            if sound
                .as_ref()
                .is_none_or(|sound| !sound.playing || sound.paused)
            {
                self.audio.stop(Channel::Library(slot));
            }
        }
    }
    fn drain_effects(&mut self) {
        while let Some(effect) = self.host.world.effects.pop_front() {
            match effect {
                WorldEffect::Message(text) => self.status = text,
                WorldEffect::Diagnostic(error) => {
                    tracing::warn!(%error,"world diagnostic");
                    self.status = error.into_bytes();
                }
                WorldEffect::Speech(sound) => self.play_speech(&sound),
                WorldEffect::StopSpeech => {
                    self.audio.stop(Channel::Speech);
                    self.speaking = false;
                }
                WorldEffect::SoundEffect { name, owner } => self.play_effect(&name, owner),
                WorldEffect::WeaponSound { object, name, .. } => {
                    self.play_effect(&name, Some(object))
                }
                WorldEffect::LibrarySound {
                    slot,
                    filename,
                    mode,
                } => self.play_library(slot, &filename, mode),
                WorldEffect::Movie(request) => self.queue_movie(request),
                WorldEffect::WorldMap => self.open_world_map(),
                WorldEffect::TownMap => self.open_town_map(),
                WorldEffect::DefaultAction { action, target } => {
                    let name = self
                        .host
                        .world
                        .get(target)
                        .map_or(b"object".as_slice(), |object| object.name.as_slice());
                    self.status = [format!("{action:?}: ").as_bytes(), name].concat();
                }
                other => tracing::debug!(?other, "host presentation request"),
            }
        }
    }
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        self.drain_effects();
        self.check_barter();
        match &self.screen {
            Screen::Map => self.map_image(),
            Screen::World(_) => self.world_image(),
            Screen::Town(_) => self.town_image(),
            Screen::PipBoy(_) => self.pipboy_image(),
            Screen::Inventory(_) => self.inventory_image(),
            Screen::Character(_) => self.character_image(),
            Screen::Barter(_) => self.barter_image(),
            Screen::Movie(_) => self.movie_image().ok_or_else(|| "no movie".into()),
        }
    }
    fn map_image(&mut self) -> AppResult<IndexedImage> {
        let map = self.host.world.render_snapshot();
        if map.objects.elevations.iter().flatten().any(|object| {
            !object.flags.contains(BasicFlags::HIDDEN)
                && Category::from_pid(object.pid).is_ok_and(|category| {
                    !self
                        .art
                        .bindings
                        .contains_key(&(category, object.art_id, object.orientation))
                })
        }) {
            self.art = MapSprites::load(&*self.host.assets.vfs, &map)?;
        }
        let mut image = render_map(&map, self.view, &self.art)?;
        for y in 0..image.height() {
            for x in 0..image.width() {
                let light = self.view.grid.pick_hex([x as i32, y as i32]).map_or(
                    self.host.world.ambient,
                    |hex| {
                        self.host.world.tile_light[self.view.elevation as usize][hex as usize]
                            .max(i64::from(self.host.world.ambient))
                            .min(65536) as i32
                    },
                );
                let index = (y * image.width() + x) as usize;
                let color = image.pixels()[index];
                if color != 0 {
                    image.pixels_mut()[index] = self
                        .palette
                        .intensity(color, ((light as u32 * 128) / 65536) as u8);
                }
            }
        }
        if self.host.world.dialogue.open {
            let font = self
                .font
                .as_ref()
                .ok_or("dialogue requires a loaded FON/AAF font")?;
            let layout = dialogue::compose(
                &image,
                &self.host.world.dialogue,
                font,
                &self.palette,
                &self.host.assets,
                self.host.world.perk(Perk::Empathy) > 0,
                self.option_scroll,
            )?;
            self.host.world.dialogue.page_offsets = layout.page_offsets;
            self.choices = layout.choices;
            return Ok(layout.image);
        }
        self.choices.clear();
        if let Some(font) = &self.font {
            let green = self.palette.lookup_rgb5([0, 31, 0]).unwrap_or(1);
            image.fill_rect([0, 350, 640, 30], 0);
            for (index, label) in [
                b"[L] Look".as_slice(),
                b"[D] Describe",
                b"[U] Use",
                b"[P] Pickup",
                b"[T] Talk",
            ]
            .into_iter()
            .enumerate()
            {
                let x = 8 + index as i32 * 126;
                image.fill_rect(
                    [x, 350, 118, 14],
                    self.palette.lookup_rgb5([0, 5, 0]).unwrap_or(0),
                );
                image.draw_text(
                    font,
                    &self.palette,
                    label,
                    [x + 3, 353],
                    112,
                    TextStyle {
                        foreground: green,
                        ..Default::default()
                    },
                );
            }
            image.draw_text(
                font,
                &self.palette,
                &self.status,
                [8, 365],
                624,
                TextStyle {
                    foreground: green,
                    ..Default::default()
                },
            );
        }
        Ok(image)
    }
    pub fn title(&self) -> String {
        match &self.screen {
            Screen::World(_) => {
                return "Vault — world map — click to travel; Escape returns".into();
            }
            Screen::Town(_) => return "Vault — town map — choose an area; Escape returns".into(),
            Screen::PipBoy(_) => return "Vault — Pip-Boy — Escape returns".into(),
            Screen::Inventory(_) => {
                return "Vault — inventory — click an item, then an action; Escape returns".into();
            }
            Screen::Character(_) => {
                return "Vault — character — Done keeps, Cancel or Escape restores".into();
            }
            Screen::Barter(_) => {
                return "Vault — barter — click moves one, right-click all; Done returns".into();
            }
            Screen::Movie(screen) => {
                return format!(
                    "Vault — movie {} — any key or a click skips",
                    screen.player.request.path
                );
            }
            Screen::Map => {}
        }
        format!(
            "Vault — {} — elevation {} — roofs {} — click objects; L/D/U/P/T actions; W world map; B Pip-Boy; I inventory; C character",
            self.path,
            self.view.elevation,
            if self.view.roofs { "on" } else { "off" }
        )
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        if self.movie_key() {
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return key == KeyCode::Escape && self.barter_key();
        }
        if self.host.world.dialogue.open {
            let choice = match key {
                KeyCode::Digit1 => Some(Some(0)),
                KeyCode::Digit2 => Some(Some(1)),
                KeyCode::Digit3 => Some(Some(2)),
                KeyCode::Digit4 => Some(Some(3)),
                KeyCode::Digit5 => Some(Some(4)),
                KeyCode::Digit6 => Some(Some(5)),
                KeyCode::Digit7 => Some(Some(6)),
                KeyCode::Digit8 => Some(Some(7)),
                KeyCode::Digit9 => Some(Some(8)),
                KeyCode::Digit0 => Some(None),
                _ => None,
            };
            if let Some(index) = choice {
                if let Err(error) = self.host.choose_dialogue(index) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            match key {
                KeyCode::Escape => {
                    if let Err(error) = self.host.close_dialogue() {
                        self.status = error.to_string().into_bytes();
                    }
                    return true;
                }
                KeyCode::Space | KeyCode::ArrowDown => {
                    let dialog = &mut self.host.world.dialogue;
                    dialog.page =
                        (dialog.page + 1).min(dialog.page_offsets.len().saturating_sub(1));
                    dialog.page_ms = self.host.world.elapsed_ms;
                    return true;
                }
                KeyCode::ArrowUp => {
                    self.host.world.dialogue.page = self.host.world.dialogue.page.saturating_sub(1);
                    return true;
                }
                _ => return false,
            }
        }
        match self.screen {
            Screen::World(_) => return self.world_key(key),
            Screen::Town(_) => return self.town_key(key),
            Screen::PipBoy(_) => return self.pipboy_key(key),
            Screen::Inventory(_) => return self.inventory_key(key),
            Screen::Character(_) => return self.character_key(key),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        if self.host.world.ui_disabled {
            return false;
        }
        match key {
            KeyCode::KeyW => {
                self.open_world_map();
                return true;
            }
            KeyCode::KeyB => {
                self.open_pipboy();
                return true;
            }
            KeyCode::KeyI => {
                self.open_inventory();
                return true;
            }
            KeyCode::KeyC => {
                self.open_character();
                return true;
            }
            _ => {}
        }
        let key_number = match key {
            KeyCode::KeyL => Some(b'L'),
            KeyCode::KeyD => Some(b'D'),
            KeyCode::KeyU => Some(b'U'),
            KeyCode::KeyP => Some(b'P'),
            KeyCode::KeyT => Some(b'T'),
            _ => None,
        };
        if let Some(key) = key_number
            && self.host.key_callback(i32::from(key))
        {
            return true;
        }
        let action = match key {
            KeyCode::KeyL => Some(ObjectAction::LookAt),
            KeyCode::KeyD => Some(ObjectAction::Description),
            KeyCode::KeyU => Some(ObjectAction::Use),
            KeyCode::KeyP => Some(ObjectAction::Pickup),
            KeyCode::KeyT => Some(ObjectAction::Talk),
            _ => None,
        };
        if let Some(action) = action {
            self.interact(action);
            return true;
        }
        match key {
            KeyCode::ArrowLeft => self.view.pan(32, 0),
            KeyCode::ArrowRight => self.view.pan(-32, 0),
            KeyCode::ArrowUp => self.view.pan(0, 24),
            KeyCode::ArrowDown => self.view.pan(0, -24),
            KeyCode::KeyR => self.view.roofs = !self.view.roofs,
            KeyCode::Digit0 | KeyCode::Numpad0 => self.view.elevation = 0,
            KeyCode::Digit1 | KeyCode::Numpad1 => self.view.elevation = 1,
            KeyCode::Digit2 | KeyCode::Numpad2 => self.view.elevation = 2,
            _ => return false,
        }
        self.last_hover = None;
        self.host.world.camera_hex = self
            .view
            .grid
            .pick_hex([320, 190])
            .unwrap_or(self.host.world.camera_hex);
        if self.host.world.elevation != self.view.elevation {
            self.host.world.elevation = self.view.elevation;
            if self.host.world.map_script.is_some()
                && let Err(error) = self.host.map_update()
            {
                self.status = error.to_string().into_bytes();
            }
        }
        true
    }
    fn interact(&mut self, action: ObjectAction) {
        if let Some(target) = self.selected {
            if let Err(error) = self.host.action(self.host.world.player, target, action) {
                self.status = error.to_string().into_bytes();
            }
        } else {
            self.status = b"Select an object first".to_vec();
        }
        self.drain_effects();
    }
    pub fn click(&mut self, point: [i32; 2], right: bool) -> bool {
        if self.movie_press() {
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return self.barter_click(point, right);
        }
        if self.host.world.dialogue.open {
            if let Some(choice) = self.choices.iter().find(|choice| choice.contains(point)) {
                let index = choice.index;
                self.press();
                if let Err(error) = self.host.choose_dialogue(Some(index)) {
                    self.status = error.to_string().into_bytes();
                }
                self.option_scroll = 0;
                return true;
            }
            return false;
        }
        match self.screen {
            Screen::World(_) => return self.world_click(point),
            Screen::Town(_) => return self.town_click(point),
            Screen::PipBoy(_) => return self.pipboy_click(point),
            Screen::Inventory(_) => return self.inventory_click(point),
            Screen::Character(_) => return self.character_click(point),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if (350..364).contains(&point[1]) {
            for (index, action) in [
                ObjectAction::LookAt,
                ObjectAction::Description,
                ObjectAction::Use,
                ObjectAction::Pickup,
                ObjectAction::Talk,
            ]
            .into_iter()
            .enumerate()
            {
                let x = 8 + index as i32 * 126;
                if (x..x + 118).contains(&point[0]) {
                    self.press();
                    self.interact(action);
                    return true;
                }
            }
        }
        let map = self.host.world.render_snapshot();
        self.selected = pick_object(&map, self.view, &self.art, point)
            .and_then(|id| self.host.world.reference_for_id(id));
        if self.selected.is_some() {
            self.interact(if right {
                ObjectAction::Talk
            } else {
                ObjectAction::LookAt
            });
        }
        true
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        if !matches!(self.screen, Screen::Map) || !self.host.world.dialogue.open {
            return false;
        }
        if delta < 0.0 {
            self.option_scroll = (self.option_scroll + 1)
                .min(self.host.world.dialogue.options.len().saturating_sub(1));
        } else {
            self.option_scroll = self.option_scroll.saturating_sub(1);
        }
        true
    }
    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        // The world and Pip-Boy screens are modal: the local map's scripts
        // and clock wait while travel or rest advances time.
        let screen = match &self.screen {
            Screen::Map => self.host.tick(now),
            Screen::World(_) | Screen::Town(_) => self
                .world_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            Screen::PipBoy(_) => self
                .pipboy_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            // Movies and the inventory, character, and barter screens are
            // modal: the world waits for them.
            Screen::Movie(_) => Ok(self.movie_tick(now)),
            Screen::Inventory(_) | Screen::Character(_) | Screen::Barter(_) => Ok(false),
        };
        let mut changed = match screen {
            Ok(changed) => changed,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                tracing::warn!(%error,"world update failed");
                true
            }
        };
        self.update_audio();
        if self.view.grid.pick_hex([320, 190]) != Some(self.host.world.camera_hex) {
            self.view.grid = HexGrid::centered(self.host.world.camera_hex, 640, 380)?;
            changed = true;
        }
        if self.view.elevation != self.host.world.elevation {
            self.view.elevation = self.host.world.elevation;
            changed = true;
        }
        if !self.host.world.effects.is_empty() {
            self.drain_effects();
            changed = true;
        }
        Ok(changed)
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<MapHover> {
        if self.host.world.dialogue.open || !matches!(self.screen, Screen::Map) {
            return None;
        }
        let hover = self.view.hover(point);
        if self.last_hover == Some(hover) {
            return None;
        }
        self.last_hover = Some(hover);
        Some(hover)
    }
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
pub(crate) mod fixture;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/sound/support.rs"]
mod sound_fixture;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
pub(crate) mod state_fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use vault_formats::LocalDirProvider;

    /// Every file under `root`, as VFS-relative paths and bytes.
    fn files(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.push((relative, std::fs::read(&path).unwrap()));
                }
            }
        }
        found.sort();
        found
    }
    /// A SPEC-008 archive of raw (`0x20`) members: the root directory
    /// table (`.` first), each directory's file table, then the payloads.
    fn dat(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut directories: BTreeMap<String, Vec<(String, &[u8])>> = BTreeMap::new();
        directories.entry(".".into()).or_default();
        for (path, bytes) in files {
            let path = path.to_ascii_uppercase().replace('/', "\\");
            let (directory, file) = path
                .rsplit_once('\\')
                .map_or((".".to_owned(), path.clone()), |(d, f)| {
                    (d.to_owned(), f.to_owned())
                });
            directories
                .entry(directory)
                .or_default()
                .push((file, bytes));
        }
        let word = |out: &mut Vec<u8>, value: usize| out.extend((value as u32).to_be_bytes());
        let mut root = Vec::new();
        for value in [directories.len(), 0, 0, 0] {
            word(&mut root, value);
        }
        for name in directories.keys() {
            root.push(name.len() as u8);
            root.extend(name.as_bytes());
        }
        let tables: usize = directories
            .values()
            .map(|files| {
                16 + files
                    .iter()
                    .map(|(name, _)| 1 + name.len() + 16)
                    .sum::<usize>()
            })
            .sum();
        let (mut table, mut payload) = (Vec::new(), Vec::new());
        let mut offset = root.len() + tables;
        for files in directories.values() {
            for value in [files.len(), 0, 16, 0] {
                word(&mut table, value);
            }
            for (name, bytes) in files {
                table.push(name.len() as u8);
                table.extend(name.as_bytes());
                for value in [0x20, offset, bytes.len(), bytes.len()] {
                    word(&mut table, value);
                }
                payload.extend_from_slice(bytes);
                offset += bytes.len();
            }
        }
        [root, table, payload].concat()
    }
    fn interactive(dir: &std::path::Path) -> MapPreview {
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir).unwrap());
        MapPreview::load(
            vfs,
            "maps/synthetic.map",
            "english",
            Some("premade/synthetic.gcd"),
            1,
        )
        .unwrap()
    }

    #[test]
    fn dat_archives_supply_the_world_like_the_loose_corpus() {
        let dir = tempfile::tempdir().unwrap();
        let loose = dir.path().join("loose");
        state_fixture::write_corpus(&loose);
        let (critter, master): (Vec<_>, Vec<_>) = files(&loose)
            .into_iter()
            .partition(|(path, _)| path.starts_with("art/critters/"));
        assert!(!critter.is_empty());
        std::fs::write(dir.path().join("MASTER.DAT"), dat(&master)).unwrap();
        std::fs::write(dir.path().join("CRITTER.DAT"), dat(&critter)).unwrap();
        let patch = dir.path().join("patch");
        std::fs::create_dir_all(patch.join("data")).unwrap();
        std::fs::write(patch.join("data/patch.txt"), b"loose").unwrap();
        let args = crate::preview::PreviewArgs {
            corpus_dir: Some(patch),
            master_dat: Some(dir.path().join("MASTER.DAT")),
            critter_dat: Some(dir.path().join("CRITTER.DAT")),
            preview_map: Some("maps/synthetic.map".into()),
            player_gcd: Some("premade/synthetic.gcd".into()),
            ..Default::default()
        };
        // Critter art comes from CRITTER.DAT only; the loose root comes first.
        let vfs = args.source().unwrap();
        assert!(vfs.read("art/critters/critters.lst").is_ok());
        assert_eq!(vfs.read("data/patch.txt").unwrap(), b"loose");
        assert!(vfs.read("color.pal").is_ok());
        let crate::preview::AssetPreview::Map(mut archived) = args.load().unwrap().unwrap() else {
            panic!("map preview");
        };
        let mut unpacked = interactive(&loose);
        assert_eq!(
            archived.image().unwrap().pixels(),
            unpacked.image().unwrap().pixels(),
            "archives and loose files give the same world"
        );
        // Without both archives the loose directory alone is the source.
        let lone = crate::preview::PreviewArgs {
            critter_dat: None,
            ..args
        };
        assert!(lone.source().unwrap().read("color.pal").is_err());
    }

    #[test]
    fn effects_speech_ui_and_library_sounds_reach_the_mixer() {
        use crate::audio::EFFECT_VOICES;
        use vault_core::dialogue::{HeadDefinition, Mood, TalkingHead};
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        std::fs::create_dir_all(dir.path().join("sound/sfx")).unwrap();
        std::fs::create_dir_all(dir.path().join("sound/speech/synth")).unwrap();
        for (path, vector) in [
            ("sound/sfx/IB1P1XX1.ACM", 'A'),
            ("sound/sfx/ib1lu1x1.acm", 'D'),
            ("sound/sfx/MAMTNTAN.ACM", 'B'),
            ("sound/speech/synth/hello.acm", 'H'),
        ] {
            std::fs::write(dir.path().join(path), sound_fixture::acm(vector)).unwrap();
        }
        let mut preview = interactive(dir.path());
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio;
        preview.image().unwrap();
        let effect = |name: &[u8]| WorldEffect::SoundEffect {
            name: name.to_vec(),
            owner: None,
        };
        // Direct playback resolves through the MALIEU -> MAMTNT alias.
        preview.host.world.effects.push_back(effect(b"MALIEUAN"));
        preview.image().unwrap();
        assert!(mixer.lock().unwrap().playing(Channel::Effect));
        // At most four effects; later requests are refused, never evicting.
        for _ in 0..5 {
            preview.host.world.effects.push_back(effect(b"IB1P1XX1"));
        }
        preview.image().unwrap();
        assert_eq!(mixer.lock().unwrap().voices(), EFFECT_VOICES);
        mixer.lock().unwrap().stop(Channel::Effect);
        // A desktop button plays the small-button press, then release, cue.
        assert!(preview.click([20, 355], false));
        assert!(preview.release());
        assert!(!preview.release(), "only one release per press");
        assert_eq!(mixer.lock().unwrap().voices(), 2);
        mixer.lock().unwrap().stop(Channel::Effect);
        // Speech without a talking head (or file) finishes at once.
        preview
            .host
            .world
            .effects
            .push_back(WorldEffect::Speech(b"hello".to_vec()));
        preview.image().unwrap();
        assert!(!preview.speaking);
        // With a head, `sound/speech/<stem>/<field>.ACM` plays until done.
        let head = HeadDefinition {
            index: 0,
            stem: "synth".into(),
            counts: [1; 3],
        };
        preview.host.world.dialogue.head =
            Some(TalkingHead::new(head, Mood::Neutral, 0, &mut preview.host.world.rng).unwrap());
        preview
            .host
            .world
            .effects
            .push_back(WorldEffect::Speech(b"hello.lip".to_vec()));
        preview.image().unwrap();
        assert!(preview.speaking && mixer.lock().unwrap().playing(Channel::Speech));
        mixer
            .lock()
            .unwrap()
            .fill(&mut [0.0; 16], 1, acm::MIXER_RATE);
        preview.tick(0).unwrap();
        assert!(!preview.speaking, "completed speech ends the speech phase");
        preview.host.world.dialogue.head = None;
        // A library slot loops until its slot stops.
        preview
            .host
            .world
            .effects
            .push_back(WorldEffect::LibrarySound {
                slot: 2,
                filename: b"sound\\sfx\\ib1lu1x1.acm".to_vec(),
                mode: 1,
            });
        preview.image().unwrap();
        assert!(mixer.lock().unwrap().playing(Channel::Library(2)));
        preview.tick(10).unwrap();
        assert!(
            !mixer.lock().unwrap().playing(Channel::Library(2)),
            "the slot is free"
        );
    }

    #[test]
    fn game_movies_play_modally_with_subtitles_and_return_to_their_screen() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        state_fixture::write_corpus(root);
        crate::movie::tests::movie_corpus(root);
        let (synthetic, _, _) = crate::movie::fixture::synthetic('A');
        std::fs::write(root.join("art/cuts/boil1.mve"), synthetic).unwrap();
        std::fs::create_dir_all(root.join("text/english/cuts")).unwrap();
        std::fs::write(root.join("text/english/cuts/boil1.sve"), b"1:AB\r\n").unwrap();
        std::fs::write(root.join("font1.aaf"), state_fixture::preview_aaf()).unwrap();
        std::fs::write(
            root.join("art/cuts/subtitle.pal"),
            state_fixture::preview_palette(),
        )
        .unwrap();
        let mut preview = interactive(root);
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio;
        preview.image().unwrap();
        // A boil movie: subtitles are forced; the world waits while it plays.
        assert!(preview.host.play_game_movie(vault_core::movie::BOIL1, 0x07));
        preview.image().unwrap();
        assert!(preview.movie_active() && preview.display_override().is_some());
        assert!(preview.title().contains(r"art\cuts\boil1.mve"));
        let ticks = preview.host.world.clock.ticks;
        let mut now = 0;
        while !preview.tick(now).unwrap() {
            now += 16;
        }
        let image = preview.image().unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        // The subtitle band sits below the centered 64 x 64 frame.
        let font = preview.host.assets.font_number(101).unwrap();
        let band = vault_core::movie::subtitle_band(font.height() as i32, 64, 208);
        let band_pixels =
            &image.pixels()[band[1] as usize * 640..(band[1] + band[3]) as usize * 640];
        assert!(
            band_pixels.iter().any(|pixel| *pixel != 0),
            "subtitle drawn"
        );
        assert!(mixer.lock().unwrap().playing(Channel::Movie));
        for _ in 0..5 {
            now += 16;
            preview.tick(now).unwrap();
        }
        assert_eq!(
            preview.host.world.clock.ticks, ticks,
            "modal: the clock waits"
        );
        // A key skips the movie and returns to the map.
        assert!(preview.step(KeyCode::Space));
        assert!(!preview.movie_active() && preview.display_override().is_none());
        assert!(!mixer.lock().unwrap().playing(Channel::Movie));
        assert_eq!(preview.image().unwrap().height(), 380);
        // The startup sequence: iplogo is absent, so only intro plays.
        preview.queue_startup_movies();
        assert!(preview.movie_active());
        assert!(preview.title().contains("intro.mve"));
        while preview.movie_active() {
            now += 16;
            preview.tick(now).unwrap();
            assert!(now < 100_000, "the movie ends by itself");
        }
        assert_eq!(preview.host.world.persistence.movies[..3], [0, 0, 1]);
        // The Pip-Boy archive lists boil1 (index 11) and replays it; a
        // click then release skips it, back to the Pip-Boy.
        assert!(preview.step(KeyCode::KeyB));
        assert!(preview.click([30, 155], false), "Video archive tab");
        assert!(preview.release());
        preview.image().unwrap();
        assert!(preview.click([200, 64], false));
        assert!(preview.movie_active());
        assert!(preview.release(), "the starting click's release");
        assert!(
            preview.movie_active(),
            "a press before the movie does not skip"
        );
        assert!(preview.click([10, 10], false));
        assert!(preview.release());
        assert!(matches!(preview.screen, Screen::PipBoy(_)));
    }

    #[test]
    fn inventory_character_and_barter_screens_drive_the_spec011_rules() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let mut preview = interactive(dir.path());
        preview.image().unwrap();
        // Pick up the synthetic item through the action strip.
        preview.click([320, 185], false);
        let item = preview.selected.unwrap();
        preview.click([400, 355], false);
        let player = preview.host.world.player;
        assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
        // Inventory: the entry lists; non-weapons cannot reload; wielding
        // puts the item in the active hand.
        assert!(preview.step(KeyCode::KeyI));
        assert!(matches!(preview.screen, Screen::Inventory(_)));
        assert_eq!(preview.image().unwrap().height(), 480);
        assert!(preview.click([30, 62], false), "first row");
        assert!(preview.click([450, 125], false), "Reload");
        assert_eq!(preview.status, b"That item is not a weapon");
        assert!(preview.click([450, 95], false), "Wield");
        let flags = preview.host.world.get(item).unwrap().raw.flags;
        assert!(flags.contains(BasicFlags::RIGHT_HAND) || flags.contains(BasicFlags::LEFT_HAND));
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map));
        // Character: spending needs points; Cancel restores the sheet.
        assert!(preview.step(KeyCode::KeyC));
        preview.image().unwrap();
        assert!(preview.click([210, 72], false), "Small Guns");
        assert_eq!(preview.status, b"No skill points left");
        assert!(preview.step(KeyCode::Escape));
        preview
            .host
            .world
            .player_sheet_mut()
            .unwrap()
            .unspent_skills = 2;
        let invested = preview.host.world.player_sheet().unwrap().invested[0];
        assert!(preview.step(KeyCode::KeyC));
        assert!(preview.click([210, 72], false));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested + 1, 1));
        assert!(preview.click([530, 445], false), "Cancel");
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested, 2));
        // Reaching level 3 sets the pending perk on the next visit; the
        // chooser grants a rank and Done keeps it.
        let sheet = preview.host.world.player_sheet_mut().unwrap();
        sheet.level = 3;
        assert!(preview.step(KeyCode::KeyC));
        let Screen::Character(screen) = &preview.screen else {
            panic!("character screen");
        };
        let character::Mode::Chooser(choices) = screen.mode.clone() else {
            panic!("perk chooser at level 3");
        };
        assert!(!choices.is_empty());
        preview.image().unwrap();
        let perk = choices[0].0;
        assert!(preview.click([210, 72], false));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert!(sheet.rank(perk) == 1 && !sheet.pending_perk);
        preview.image().unwrap();
        if let Screen::Character(screen) = &preview.screen
            && screen.mode == character::Mode::Sheet
        {
            assert!(preview.click([430, 445], false), "Done");
        } else {
            assert!(preview.step(KeyCode::Escape));
        }
        assert!(matches!(preview.screen, Screen::Map));
        assert_eq!(
            preview.host.world.player_sheet().unwrap().reconciled_level,
            3
        );
        // Barter: a dialogue's pending request opens it with the target.
        assert!(preview.click([336, 170], true));
        assert!(preview.host.world.dialogue.open);
        preview.host.world.dialogue.pending_barter = true;
        preview.image().unwrap();
        let Screen::Barter(screen) = &preview.screen else {
            panic!("barter");
        };
        let offer = screen.session.offer;
        assert!(preview.click([20, 62], true), "the player's whole entry");
        assert!(!preview.host.world.get(offer).unwrap().inventory.is_empty());
        preview.image().unwrap();
        assert!(preview.click([530, 445], false), "Done");
        assert!(!preview.host.world.dialogue.pending_barter);
        assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
        assert!(matches!(preview.screen, Screen::Map) && preview.host.world.dialogue.open);
    }

    #[test]
    fn world_map_travels_to_a_town_and_enters_an_area_from_the_town_map() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        std::fs::copy(
            dir.path().join("maps/synthetic.map"),
            dir.path().join("maps/v13ent.map"),
        )
        .unwrap();
        let mut preview = interactive(dir.path());
        preview.image().unwrap();
        assert!(preview.step(KeyCode::KeyW));
        let Screen::World(world) = &preview.screen else {
            panic!("world map");
        };
        let scroll = world.scroll;
        let image = preview.image().unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        // Vault 13 (cell 16,1) is known from the start: travel to its center.
        let [x, y] = [16 * 50 + 25, 50 + 25];
        let point = [
            vault_core::worldmap::VIEWPORT_ORIGIN[0] + x - scroll[0],
            vault_core::worldmap::VIEWPORT_ORIGIN[1] + y - scroll[1],
        ];
        assert!(preview.click(point, false));
        let start = preview.host.world.clock.ticks;
        for update in 1..100 {
            preview.tick(update * 50).unwrap();
            if !matches!(preview.screen, Screen::World(_)) {
                break;
            }
        }
        // A visited town shows its town map with its known entrances.
        let Screen::Town(town) = &preview.screen else {
            panic!("town map after arriving");
        };
        assert_eq!(town.town, 0);
        assert_eq!(town.buttons.len(), 1);
        assert!(preview.host.world.clock.ticks >= start);
        let rect = town.buttons[0].rect;
        preview.image().unwrap();
        assert!(preview.click([rect[0] + 4, rect[1] + 4], false));
        assert!(
            matches!(preview.screen, Screen::Map),
            "{:?}",
            String::from_utf8_lossy(&preview.status)
        );
        // The entrance's map is loaded (named by its working saved-map file).
        assert_eq!(
            preview.host.world.current_map_name().as_deref(),
            Some("V13ENT.SAV")
        );
        assert_eq!(preview.image().unwrap().height(), 380);
        // Escape returns from the world map without travelling.
        assert!(preview.step(KeyCode::KeyW));
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map));
    }

    #[test]
    fn pipboy_lists_quests_and_holodisks_and_rests_in_ui_steps() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        std::fs::write(
            dir.path().join("text/english/game/pipboy.msg"),
            b"{400}{}{Disk Zero}\n{1000}{}{First line}\n{1001}{}{**END-PAR**}\n\
              {1002}{}{**END-DISK**}\n{700}{}{Vault 13}\n{701}{}{Find help}\n\
              {702}{}{Done task}\n{704}{}{Find the water chip}\n",
        )
        .unwrap();
        let mut preview = interactive(dir.path());
        let mut gvars = vec![0; 620];
        gvars[162] = 1;
        gvars[238] = 1;
        gvars[309] = 2;
        preview.host.world.gvars = Some(gvars);
        preview.image().unwrap();
        preview.host.world.rest_credit = 100;
        assert!(preview.step(KeyCode::KeyB));
        assert_eq!(preview.host.world.rest_credit, 0, "opening resets credit");
        let image = preview.image().unwrap();
        assert!(image.pixels().iter().any(|pixel| *pixel != 0));
        // The holodisk list maps a row to its disk's records.
        assert!(preview.click([30, 95], false));
        assert!(preview.click([200, 64], false));
        let Screen::PipBoy(pipboy) = &preview.screen else {
            panic!("Pip-Boy");
        };
        let (disk, page) = pipboy.disk.as_ref().unwrap();
        assert_eq!(
            (disk.title.as_slice(), disk.records.len(), *page),
            (b"Disk Zero".as_slice(), 2, 0)
        );
        preview.image().unwrap();
        // The alarm clock rests ten minutes in 50 ms UI steps.
        assert!(preview.click([30, 125], false));
        assert!(preview.click([200, 64], false));
        let start = preview.host.world.clock.ticks;
        let mut now = 1000;
        while matches!(&preview.screen, Screen::PipBoy(pipboy) if pipboy.rest.is_some()) {
            now += 50;
            preview.tick(now).unwrap();
            assert!(now < 100_000, "rest finishes");
        }
        assert_eq!(preview.host.world.clock.ticks - start, 6000);
        assert_eq!(preview.status, b"Rest complete");
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map));
    }
    #[test]
    fn map_preview_loads_all_categories_and_handles_camera_roofs_elevations() {
        let dir = tempfile::tempdir().unwrap();
        fixture::write_corpus(dir.path());
        let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut preview = MapPreview::load(vfs, r"MAPS\SYNTHETIC.MAP", "english", None, 1).unwrap();
        assert_eq!(preview.host.world.map.objects.total, 5);
        let image = preview.image().unwrap();
        for color in [7, 15, 16, 17, 18] {
            assert!(
                image.pixels().contains(&color),
                "missing category color {color}"
            );
        }
        assert_eq!(preview.hover([320, 190]).unwrap().hex, Some(20100));
        assert!(preview.hover([320, 190]).is_none());
        preview.step(KeyCode::KeyR);
        assert!(!preview.view.roofs);
        preview.step(KeyCode::ArrowRight);
        assert_eq!(preview.hover([288, 190]).unwrap().hex, Some(20100));
        for key in [KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2] {
            assert!(preview.step(key));
            preview.image().unwrap();
        }
        assert!(!preview.step(KeyCode::Space));
    }
    #[test]
    fn clicking_npc_renders_choices_executes_callbacks_and_returns_to_map() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut preview = MapPreview::load(
            vfs,
            "maps/synthetic.map",
            "english",
            Some("premade/synthetic.gcd"),
            1,
        )
        .unwrap();
        preview.image().unwrap();
        assert!(preview.click([336, 170], false));
        let selected = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(selected).unwrap().category(),
            Category::Critter
        );
        assert!(preview.host.library.palette_cycling);
        assert!(preview.click([336, 170], true));
        assert!(preview.host.world.dialogue.open && preview.host.world.dialogue.waiting);
        // The shell's cycling poll follows this flag (SPEC-001/SPEC-006).
        assert!(!preview.host.library.palette_cycling);
        let image = preview.image().unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
        assert_eq!(preview.choices.len(), 1);
        assert_eq!(preview.choices[0].rectangle[0..2], [127, 335]);
        assert!(preview.click([200, 338], false));
        assert_eq!(preview.host.world.dialogue.options[0].procedure, 0);
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Digit1));
        assert!(!preview.host.world.dialogue.open);
        assert!(preview.host.library.palette_cycling);
        assert_eq!(preview.image().unwrap().height(), 380);
        // The clickable action strip routes a real pickup into authoritative inventory.
        preview.click([320, 185], false);
        let item = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(item).unwrap().category(),
            Category::Item
        );
        preview.click([400, 355], false);
        assert_eq!(
            preview.host.world.get(item).unwrap().owner,
            Some(preview.host.world.player)
        );
    }
}
