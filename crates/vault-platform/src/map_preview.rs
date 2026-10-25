use crate::{
    AppResult,
    audio::{AudioHandle, Channel, MusicRequest, Volumes},
    input::key_text,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vault_core::{
    CharacterSheet, EnginePreset, ObjectAction, ObjectRef, Perk, WorldEffect, WorldHost,
    WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    movie::{MoviePolicy, MovieRequest},
};
use vault_formats::{
    Fid, Font, FrmArchive, HexGrid, MAX_ASSET_BYTES, MapFile, Palette, Vfs, VfsPath, acm,
    map::MapObjects,
    pro::{BasicFlags, Category},
    sfx,
};
use vault_render::{
    IndexedImage,
    dialogue::{self, ChoiceHit},
    map::{
        Cutout, MapArt, MapHover, MapLight, MapScene, MapView, RoofCutaway, SceneCursor, Shroud,
        draw_map_scene, render_floor_lit,
    },
    outline::OutlineInks,
};
use winit::keyboard::KeyCode;

/// SPEC-008 §3.10 rule 4 what an open dialogue session did to the music.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogueMusic {
    /// A talking head stopped the track; the close replays the last one.
    Stopped,
    /// Without a head the music volume was halved; the close restores
    /// `volume`, the one current at the opening (the safe rule).
    Halved { volume: i32 },
}

/// SPEC-014 §3.37 `egg.frm`, interface art (`intrface.lst`) index 2.
const EGG_ART: u32 = 0x0600_0002;

mod automap;
mod barter;
mod called;
mod camera;
mod character;
mod counter;
mod debug;
mod dialog;
mod elevator;
mod enhanced;
mod export;
mod fade;
pub(crate) use fade::FADE_MS;
mod floating;
mod help;
mod hud;
mod inventory;
mod loot;
mod movies;
mod options;
mod pipboy;
mod pointer;
mod saveload;
mod skilldex;
mod talk;
mod timer;
mod world;

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
    PipBoy(Box<pipboy::PipBoy>),
    Inventory(inventory::InventoryScreen),
    Character(Box<character::CharacterScreen>),
    Barter(Box<barter::BarterScreen>),
    Movie(Box<movies::MovieScreen>),
    /// SPEC-013 windows over the world.
    Skilldex(skilldex::SkilldexScreen),
    Elevator(elevator::ElevatorScreen),
    Loot(Box<loot::LootScreen>),
    /// SPEC-014 options, pause, quit confirmation and preferences.
    Options(options::OptionsScreen),
    SaveLoad(Box<saveload::SaveLoadScreen>),
    /// The automap overlay.
    Automap(automap::AutomapScreen),
    /// The F1 help screen.
    Help(Box<help::HelpScreen>),
}

/// Decoded ACM samples by VFS path; a failing path is reported once.
#[derive(Default)]
pub(crate) struct SoundBank {
    cache: BTreeMap<String, Arc<[i16]>>,
    samples: usize,
    failed: BTreeSet<String>,
}
impl SoundBank {
    pub(crate) fn load(&mut self, vfs: &dyn Vfs, path: &str) -> Option<Arc<[i16]>> {
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
                // SPEC-010 §3.7: a stream that fails at a block keeps every
                // block before it and plays them; the failure is reported.
                if let Some(failure) = &decoded.failure {
                    tracing::warn!(path, %failure, blocks = decoded.blocks, "sound truncated at a bad block");
                }
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

pub struct MapPreview {
    pub host: WorldHost<VfsWorldAssets>,
    pub palette: Palette,
    /// SPEC-014 §3.17 the pause window's grey lookup table, built from
    /// `color.pal` when the game starts. A load (the game reset) reads the
    /// same palette, so the table never changes afterwards; brightness and
    /// palette cycling do not touch it.
    pub(crate) grey: [u8; 256],
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
    /// A desktop button is held: the release cue its release plays.
    pressed: Option<&'static [u8]>,
    /// Game movies waiting for the one playing.
    movies: VecDeque<MovieRequest>,
    /// SPEC-014 interface bar, monitor and cursor mode.
    pub(crate) hud: hud::Hud,
    pub preferences: vault_core::ui::options::Preferences,
    /// `fallout.cfg`, written when preferences change.
    pub config_path: Option<std::path::PathBuf>,
    /// Where the F9 overlay exports the current setup as a mod (`None`:
    /// the export is refused, as in a replay).
    pub mod_export: Option<std::path::PathBuf>,
    /// Started with `--debug`: the enhanced Push reaches any standing
    /// critter, not only party members.
    pub debug_session: bool,
    /// The directory holding `SAVEGAME/SLOTnn`.
    pub save_root: std::path::PathBuf,
    /// SPEC-014 §3.19 where F12 writes `scrNNNNN.bmp`: the process's
    /// working directory (the empty path) in the game.
    pub screenshot_dir: std::path::PathBuf,
    quick: vault_core::ui::saveload::QuickSlot,
    slot_list: vault_core::ui::saveload::SlotList,
    /// A confirmed quit: return to the main menu (or leave the preview).
    pub quit_requested: bool,
    /// The world's last endgame request (slideshow or ending), for the
    /// game flow.
    pub endgame: Option<WorldEffect>,
    /// A slot load succeeded (the main menu's load screen reports it).
    pub loaded: bool,
    /// SPEC-014 §3.13 a dialog box over the current screen.
    dialog: Option<dialog::Dialog>,
    /// SPEC-014 §3.45 the Review or Ask About window over a conversation.
    talk: Option<talk::TalkWindow>,
    /// SPEC-014 §3.26-3.29 pointer, hover, action menu and edge scrolling.
    pointer: pointer::PointerState,
    /// The shell clock at the last tick, in milliseconds.
    clock: u64,
    /// The last composed frame without the cursor, so a pointer move only
    /// redraws the cursor.
    frame: Option<IndexedImage>,
    /// SPEC-014 §3.2 palette fades.
    fades: fade::Fades,
    /// The startup movies fade back to the game palette when they end.
    startup_fade: bool,
    /// The vault-debug inspector and its overlay, when on.
    debug: Option<debug::DebugOverlay>,
    /// The map view's light levels.
    /// SPEC-014 §3.34 the inventory figure's facing and when it last
    /// turned; the counter persists between openings.
    figure: (u8, u64),
    /// The button pressed and still held, as a screen rectangle: it shows
    /// its down art while the pointer stays over it.
    held: Option<vault_core::ui::Rect>,
    /// The last frame drew the held button down.
    held_shown: bool,
    /// SPEC-014 §3.27 the last composed frame's map view holds the
    /// in-world cursor object, so a pointer move composes it again.
    cursor_in_frame: bool,
    /// SPEC-008 §3.10 rule 4 what the open dialogue session did to the
    /// music, undone when it closes.
    dialogue_music: Option<DialogueMusic>,
    /// G-09 the map preview tool's own keys: W opens the world map,
    /// PageUp/PageDown change the elevation, L/D/U/G/T act on the
    /// selection. The game's worlds have them only with `--debug`.
    pub(crate) preview_keys: bool,
    /// R toggles the roofs: the map preview tool only. In the game no key
    /// turns roofs off (SPEC-014 §3.36).
    pub(crate) roof_key: bool,
    /// SPEC-014 §3.2: where the pointer went (`None`: it left the window)
    /// while a game fade swallowed input; the move is applied when the
    /// fade ends, as the first poll after it would see it.
    fade_pointer: Option<Option<[i32; 2]>>,
    /// SPEC-014 §3.18 the mixer's volume settings (F2/F3 move the master
    /// without touching the preference), and the preference volumes they
    /// were last taken from.
    volumes: Volumes,
    volume_source: Option<Volumes>,
    /// P-02: the world is frozen behind a modal window, so its view is
    /// rendered once and reused until input or a world change.
    frozen_view: Option<IndexedImage>,
    /// SPEC-014 §3.37 the see-through cutout mask, `egg.frm`
    /// (`intrface.lst` index 2), loaded on first use; `Some(None)` when it
    /// cannot load.
    egg: Option<Option<Arc<FrmArchive>>>,
    /// `fallout.cfg` `[system] interrupt_walk` (SPEC-014 §3.27, default on):
    /// a movement click out of combat cancels a busy player's sequence.
    pub interrupt_walk: bool,
    /// SPEC-014 §3.42 the character screen's info line, folder and skill
    /// cursor, kept from one opening to the next.
    character_memory: character::Remembered,
    /// The `enhanced` preset's presentation (camera, dual-layer frame and
    /// roof peeking); `None` is Vanilla, the default.
    enhanced: Option<Box<enhanced::Enhanced>>,
}

/// The map's sprites by tile index or object art. Each art file is read and
/// decoded once, through the world's asset cache, and object art that is
/// missing or unreadable is remembered, so a frame binds only art it has not
/// seen before.
#[derive(Default)]
struct MapSprites {
    bindings: BTreeMap<(Category, u32, u8), Arc<FrmArchive>>,
    /// The bound tile art by tile index, for the per-square lookups.
    tile_art: Vec<Option<Arc<FrmArchive>>>,
    /// Object art that is missing or unreadable; it is not drawn.
    missing: BTreeSet<(Category, u32, u8)>,
    /// The map whose tiles are bound (its header name and number).
    tiles: Option<([u8; 16], i32)>,
    /// A fingerprint of the drawn objects' art when everything they need
    /// was last bound: an unchanged scene skips the per-object lookups
    /// (P-02).
    objects: Option<u64>,
}
impl MapArt for MapSprites {
    fn frame(
        &self,
        category: Category,
        art_id: u32,
        orientation: u8,
        frame: usize,
    ) -> Option<(&vault_formats::frm::FrmFrame, [i16; 2])> {
        let archive = if category == Category::Tile {
            self.tile_art.get(art_id as usize)?.as_ref()?
        } else {
            self.bindings.get(&(category, art_id, orientation))?
        };
        Some((
            archive
                .frame(orientation as usize, frame)
                .or_else(|| archive.frame(orientation as usize, 0))?,
            *archive.shifts.get(orientation as usize)?,
        ))
    }
}
impl MapSprites {
    /// The visible objects' art not yet bound or known missing.
    fn unbound_objects(&self, objects: &MapObjects) -> AppResult<BTreeSet<(Category, u32, u8)>> {
        let mut needed = BTreeSet::new();
        for object in objects.elevations.iter().flatten() {
            if !object.flags.contains(BasicFlags::HIDDEN) {
                let key = (
                    Category::from_pid(object.pid)?,
                    object.art_id,
                    object.orientation,
                );
                if !self.bindings.contains_key(&key) && !self.missing.contains(&key) {
                    needed.insert(key);
                }
            }
        }
        Ok(needed)
    }
    /// FNV-1a over each object's prototype, art, orientation and hidden
    /// flag, the inputs of `unbound_objects`.
    fn object_fingerprint(objects: &MapObjects) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for object in objects.elevations.iter().flatten() {
            let hidden = u32::from(object.flags.contains(BasicFlags::HIDDEN));
            for word in [
                object.pid,
                object.art_id,
                u32::from(object.orientation),
                hidden,
            ] {
                for byte in word.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        hash
    }
    /// Bind the art `map` and `objects` draw that is not bound yet: every
    /// tile of a map not seen before, and each new object art. The objects
    /// are passed apart from the map so a live world binds its current
    /// objects without cloning the map file (P-02).
    fn bind(
        &mut self,
        assets: &VfsWorldAssets,
        map: &MapFile,
        objects: &MapObjects,
    ) -> AppResult<()> {
        let identity = (map.header.name, map.header.map_number);
        let fingerprint = Self::object_fingerprint(objects);
        if self.tiles == Some(identity) && self.objects == Some(fingerprint) {
            return Ok(());
        }
        let mut needed = self.unbound_objects(objects)?;
        if self.tiles != Some(identity) {
            for tiles in map.tiles.iter().flatten() {
                for tile in tiles {
                    // S-19 / SPEC-014 §3.36: every roof but the blank index 1
                    // is bound whatever its hide bit, so a group hidden when
                    // the map binds draws once it is restored.
                    for (index, skip) in [
                        (tile.floor(), tile.floor_hidden()),
                        (tile.roof(), tile.roof() == 1),
                    ] {
                        let key = (Category::Tile, u32::from(index), 0_u8);
                        if !skip && !self.bindings.contains_key(&key) {
                            needed.insert(key);
                        }
                    }
                }
            }
        }
        if needed.is_empty() {
            self.tiles = Some(identity);
            self.objects = Some(fingerprint);
            return Ok(());
        }
        let catalog = assets.art_catalog()?;
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
            match assets.frame(&resolved) {
                Ok(archive) => {
                    if category == Category::Tile {
                        let index = id as usize;
                        if self.tile_art.len() <= index {
                            self.tile_art.resize(index + 1, None);
                        }
                        self.tile_art[index] = Some(archive.clone());
                    }
                    self.bindings.insert(key, archive);
                }
                // The reference has no guaranteed final fallback resource.
                Err(error) if category != Category::Tile => {
                    tracing::warn!(art_id = fid.0, sprite = %resolved, %error, "object art is missing; not drawn");
                    self.missing.insert(key);
                }
                Err(error) => return Err(format!("{resolved}: {error}").into()),
            }
        }
        self.tiles = Some(identity);
        self.objects = Some(fingerprint);
        for object in objects.elevations.iter().flatten() {
            let category = Category::from_pid(object.pid)?;
            if let Some(archive) = self
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
        Ok(())
    }
}

/// SPEC-010 §3.11 an effect owner's screen rectangle, as inclusive corners
/// `[left, top, right, bottom]`: the drawn frame (`origin`, `size`), 1 pixel
/// larger on every side while the object is outlined (SPEC-014 §3.44
/// outline word: a type in bits 0-23, not disabled by bit 31). Art that
/// cannot be read (`None`) gives (0, 0)-(0, 0).
fn effect_rect(sprite: Option<([i64; 2], [i64; 2])>, outline: u32) -> [i64; 4] {
    let Some(([x, y], [width, height])) = sprite else {
        return [0; 4];
    };
    let grow = i64::from(vault_formats::map::outline::outlined(outline));
    [
        x - grow,
        y - grow,
        x + width - 1 + grow,
        y + height - 1 + grow,
    ]
}
/// SPEC-010 §3.11 whether inclusive corners overlap a window of `size` at
/// (0, 0), inclusive pixel bounds.
fn overlaps_window([left, top, right, bottom]: [i64; 4], [width, height]: [i64; 2]) -> bool {
    left < width && right >= 0 && top < height && bottom >= 0
}

impl MapPreview {
    #[cfg(test)]
    pub fn load(
        vfs: Arc<dyn Vfs>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
    ) -> AppResult<Self> {
        Self::load_with(vfs, path, language, character, seed, EnginePreset::Vanilla)
    }
    /// The world for `path`, its player sheet read from `character` (a
    /// `.gcd`) when given, under `preset`.
    #[cfg(test)]
    pub fn load_with(
        vfs: Arc<dyn Vfs>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
        preset: EnginePreset,
    ) -> AppResult<Self> {
        Self::load_for_game(
            vfs,
            path,
            language,
            character,
            seed,
            preset,
            vault_core::GameProfile::Fallout1,
        )
    }
    /// [`Self::load_with`] reading `game`'s map and prototype layouts
    /// (SPEC-016) from the first map on.
    pub fn load_for_game(
        vfs: Arc<dyn Vfs>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
        preset: EnginePreset,
        game: vault_core::GameProfile,
    ) -> AppResult<Self> {
        let assets = VfsWorldAssets::new(vfs, language)?;
        assets.set_game_profile(game);
        let character = if let Some(path) = character {
            CharacterSheet::from_gcd(&assets.gcd(path)?, Default::default())
        } else {
            CharacterSheet::default()
        };
        Self::with_sheet(assets, path, character, seed, preset)
    }
    /// The world for `path` with a ready player sheet (SPEC-014 §3.12's
    /// new-game hand-off passes the selected or edited character).
    /// `preset` is set before the campaign loads, so its table overrides
    /// and script parse profile apply from the first map on.
    pub fn with_sheet(
        assets: VfsWorldAssets,
        path: &str,
        character: CharacterSheet,
        seed: i32,
        preset: EnginePreset,
    ) -> AppResult<Self> {
        let path = VfsPath::new(path)?;
        let palette = Palette::parse(&assets.vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?)?;
        let font = assets.font().ok();
        let mut host = WorldHost::new(WorldState::new(character, seed)?, assets);
        host.set_preset(preset);
        // SPEC-005 §3.8: loading the player sheet runs the gender update.
        host.update_player_gender();
        host.interactive_dialogue = true;
        // The desktop plays every movie and reports its end, and plays
        // attacks over time (SPEC-007 §3.10).
        host.movie_policy = MoviePolicy::WaitUntilFinished;
        host.playback = vault_core::host_choreography::Playback::RealTime;
        // A preview can run without campaign globals; the failure is reported.
        if let Err(error) = host.load_campaign() {
            tracing::warn!(%error, "campaign initializer unavailable");
        }
        // SPEC-002 §3.7: the language filter's word list, read at startup
        // before any catalog is filtered.
        host.load_language_filter();
        // SPEC-009 AI initialization; without it NPC combat turns are skipped.
        if let Err(error) = host.load_combat_ai() {
            tracing::warn!(%error, "combat AI unavailable");
        }
        host.load_map(path.as_str())?;
        let objects = host.world.render_objects();
        let mut art = MapSprites::default();
        art.bind(&host.assets, &host.world.map, &objects)?;
        let mut view = MapView::new(&host.world.map, 640, 380)?;
        view.grid = HexGrid::centered(host.world.camera_hex, 640, 380)?;
        view.elevation = host.world.elevation;
        let count: usize = objects.elevations.iter().map(Vec::len).sum();
        tracing::info!(map=%path,objects=count,"interactive map preview loaded");
        Ok(Self {
            host,
            grey: vault_core::ui::grey_table(&palette),
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
            pressed: None,
            movies: VecDeque::new(),
            hud: hud::Hud::default(),
            preferences: Default::default(),
            config_path: None,
            mod_export: None,
            debug_session: false,
            save_root: std::path::PathBuf::from("."),
            screenshot_dir: std::path::PathBuf::new(),
            quick: Default::default(),
            slot_list: Default::default(),
            quit_requested: false,
            endgame: None,
            loaded: false,
            dialog: None,
            talk: None,
            pointer: Default::default(),
            clock: 0,
            frame: None,
            fades: Default::default(),
            startup_fade: false,
            debug: None,
            figure: (0, 0),
            held: None,
            held_shown: false,
            cursor_in_frame: false,
            dialogue_music: None,
            preview_keys: true,
            roof_key: true,
            fade_pointer: None,
            volumes: Volumes::default(),
            volume_source: None,
            frozen_view: None,
            egg: None,
            interrupt_walk: true,
            character_memory: character::Remembered::default(),
            enhanced: None,
        })
    }
    /// SPEC-014 §3.29 the keyboard for the engine key repeat: no input is
    /// polled during a game fade (§3.2) and no key is read while combat
    /// disables the keyboard (§3.27); each player combat turn start clears
    /// the keyboard.
    pub(crate) fn keyboard(&self) -> crate::input::Keyboard {
        crate::input::Keyboard {
            enabled: !self.fade_blocks() && !self.keyboard_locked(),
            clears: self.host.combat_lock.turns,
        }
    }
    /// SPEC-014 §3.2: a game fade blocks completely, outside a movie (whose
    /// own palette hides a fade queued behind it).
    pub(crate) fn fade_blocks(&self) -> bool {
        self.fades.blocking() && !self.movie_active()
    }
    /// Tick past every queued or running fade (tests): a game fade holds
    /// the world and input until it ends (SPEC-014 §3.2).
    #[cfg(test)]
    pub(crate) fn finish_fades(&mut self) {
        let mut now = self.clock;
        for _ in 0..64 {
            if !self.fades.running() {
                return;
            }
            now += FADE_MS + 16;
            self.tick(now).unwrap();
        }
        panic!("the fades did not end");
    }
    /// Whether the save/load screen is open (the main menu's load stage).
    pub fn load_screen_open(&self) -> bool {
        matches!(self.screen, Screen::SaveLoad(_))
    }
    /// The save/load selection and quick-slot flag, which last for the
    /// program run (SPEC-014 §3.14, §3.16).
    pub fn slot_session(
        &self,
    ) -> (
        vault_core::ui::saveload::SlotList,
        vault_core::ui::saveload::QuickSlot,
    ) {
        (self.slot_list.clone(), self.quick)
    }
    pub fn set_slot_session(
        &mut self,
        slot_list: vault_core::ui::saveload::SlotList,
        quick: vault_core::ui::saveload::QuickSlot,
    ) {
        self.slot_list = slot_list;
        self.quick = quick;
    }
    /// Held modifiers: shift, control, alt.
    pub fn set_modifiers(&mut self, modifiers: [bool; 3]) {
        // Holding Alt peeks under the roofs, and holding Shift highlights
        // the ground items, in the `enhanced` world.
        if self.enhanced.is_some()
            && (modifiers[2] != self.hud.modifiers[2] || modifiers[0] != self.hud.modifiers[0])
        {
            self.pointer.redraw = true;
            self.thaw_view();
        }
        self.hud.modifiers = modifiers;
    }
    /// M-P6: the window lost the focus, so the releases of the buttons held
    /// now will not arrive: every held press is dropped, and no action
    /// menu opens later with the button up.
    pub fn focus_lost(&mut self) {
        self.drop_presses();
        self.enhanced_release_keys();
    }
    /// Every press still held is forgotten (its release was swallowed).
    fn drop_presses(&mut self) {
        self.held = None;
        self.hud.held = None;
        self.pressed = None;
        self.pointer.held = None;
        self.pointer.menu = None;
    }
    /// The last game fade ended: the pointer's move during it applies.
    fn fade_ended(&mut self) {
        // The `enhanced` pointer is routed again from where the shell last
        // reported it.
        if self.fade_pointer.is_some() && self.enhanced_pointer_again() {
            self.fade_pointer = None;
            return;
        }
        match self.fade_pointer.take() {
            Some(Some(point)) => {
                self.hover(point);
            }
            Some(None) => self.pointer_left(),
            None => {}
        }
    }
    /// Back to the (possibly newly loaded) local map.
    pub(crate) fn return_to_map(&mut self) {
        // SPEC-013 §3.22: a world or town map opened by map -1 or -2
        // discards the request made while it was open, however it closes.
        self.host.world_map_closed();
        self.screen = Screen::Map;
        self.last_hover = None;
        self.selected = None;
        // A map without a working name (none loaded, SPEC-008 §3.10): its
        // header name titles it instead of the map before it.
        self.path = self.host.world.current_map_name().unwrap_or_else(|| {
            let name = &self.host.world.map.header.name;
            let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
            String::from_utf8_lossy(&name[..end]).into_owned()
        });
    }
    /// Whether the shell may cycle the palette now: the library's flag,
    /// except while a window that suspends cycling is open. SPEC-012 §3.12
    /// movies; SPEC-014 §3.17 the in-game pause window (not the world
    /// map's), §3.14 in-game save/load, §3.2 the help screen, §3.8 the
    /// character editor, §3.39-§3.40 the world and town maps, §3.41 the
    /// Pip-Boy; SPEC-013 §3.15 the Skilldex and §3.21 elevators.
    pub(crate) fn palette_cycling(&self) -> bool {
        self.host.library.palette_cycling
            && !matches!(
                self.screen,
                Screen::Movie(_)
                    | Screen::PipBoy(_)
                    | Screen::World(_)
                    | Screen::Town(_)
                    | Screen::Options(options::OptionsScreen::Pause)
                    | Screen::SaveLoad(_)
                    | Screen::Help(_)
                    | Screen::Character(_)
                    | Screen::Skilldex(_)
                    | Screen::Elevator(_)
            )
    }
    /// A red button press: its cue, and the release cue to follow.
    pub(crate) fn press(&mut self) {
        self.press_cue(BUTTON_PRESS, BUTTON_RELEASE);
    }
    /// A button press with its own press and release cues.
    pub(crate) fn press_cue(&mut self, press: &'static [u8], release: &'static [u8]) {
        self.pressed = Some(release);
        self.play_effect(press, None);
    }
    /// A red-button press on the button at screen `rect`: the cues, and its
    /// down art until the release.
    pub(crate) fn press_button(&mut self, rect: vault_core::ui::Rect) {
        self.press();
        self.held = Some(rect);
    }
    /// A press on a button at screen `rect` with its own cues.
    pub(crate) fn press_button_cue(
        &mut self,
        press: &'static [u8],
        release: &'static [u8],
        rect: vault_core::ui::Rect,
    ) {
        self.press_cue(press, release);
        self.held = Some(rect);
    }
    /// The held button, while the pointer is over it.
    pub(crate) fn held_button(&self) -> Option<vault_core::ui::Rect> {
        self.held
            .filter(|rect| self.pointer.at.is_some_and(|at| rect.contains(at)))
    }
    /// The pointer moved onto or off the held button since the last frame.
    fn held_moved(&self) -> bool {
        self.held_button().is_some() != self.held_shown
    }
    /// A button's art: `down` while it is held with the pointer over it,
    /// else `up`.
    pub(crate) fn button_art(
        &self,
        rect: vault_core::ui::Rect,
        up: &'static str,
        down: &'static str,
    ) -> &'static str {
        if self.held_button() == Some(rect) {
            down
        } else {
            up
        }
    }
    /// A mouse release at a frame pixel (`None` outside the frame): a drag
    /// drops, a map click acts, a held desktop button plays its release
    /// cue, and a button that reports on release acts when it is over it.
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        self.thaw_view();
        // SPEC-014 §3.2: a release during a game fade is swallowed with its
        // press, so nothing pressed before the fade acts or stays held.
        if self.fade_blocks() {
            if !right {
                self.drop_presses();
            }
            return true;
        }
        // A left release ends the held button's down art.
        let (held, bar) = if right {
            (None, None)
        } else {
            (self.held.take(), self.hud.held.take())
        };
        let over = held.filter(|rect| point.is_some_and(|point| rect.contains(point)));
        if self.movie_release() {
            self.pressed = None;
            return true;
        }
        // SPEC-014 §3.21: a click whose release is read during a counter
        // roll or the panel animation is lost: no map action, bar button or
        // menu entry results.
        if self.bar_blocks() {
            if !right {
                self.drop_presses();
            }
            return true;
        }
        // SPEC-013 §3.15: the Set Timer window is modal over any window.
        let mut handled = !right
            && (self.timer_release(over)
                || match self.screen {
                    Screen::Map => {
                        self.talk_release(over)
                            || self.called_release(point, over)
                            || self.map_release(point)
                            || self.bar_release(bar, over)
                    }
                    Screen::Elevator(_) => self.elevator_release(over),
                    Screen::Loot(_) => self.loot_release(point),
                    Screen::Inventory(_) => self.inventory_release(point),
                    Screen::Barter(_) => self.barter_release(point),
                    Screen::Automap(_) => self.automap_release(point),
                    Screen::Options(_) => self.options_release(point),
                    Screen::PipBoy(_) => self.pipboy_release(point),
                    Screen::World(_) => self.world_release(point),
                    Screen::Town(_) => self.town_release(point),
                    Screen::Character(_) => self.character_release(point, over.is_some()),
                    _ => false,
                });
        if let Some(cue) = self.pressed.take() {
            self.play_effect(cue, None);
            handled = true;
        }
        handled || held.is_some()
    }
    /// SPEC-014 §3.19 the `/` and `?` date line, "December: 5/2161 7:21":
    /// the month name is `editor.msg` 500 + month - 1.
    pub(crate) fn clock_text(&self) -> Vec<u8> {
        let world = &self.host.world;
        let [month, day, year] = vault_core::host_save::game_date(world.clock.ticks);
        let name = self.message("editor.msg", 500 + i32::from(month) - 1);
        vault_core::ui::bar::date_line(&name, [day, year], world.clock.hhmm())
    }
    /// Whether an object (or its outermost owner) is on screen, for SPEC-010
    /// §3.11 relative volume: its screen bounding rectangle overlaps the
    /// 640 x 380 map window, inclusive pixel bounds. The rectangle is its
    /// current frame placed at its hex as drawn (SPEC-014 §3.44), or at its
    /// stored screen position when it has no hex, 1 pixel larger on every
    /// side while it is outlined. There is no elevation condition. Art that
    /// cannot be read gives the rectangle (0, 0)-(0, 0), which is on screen.
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
        let sprite = vault_render::map::object_sprite(self.view, &self.art, &found.raw)
            .map(|(frame, origin)| (origin, [frame.width, frame.height].map(i64::from)));
        // A hex off the grid is nowhere on screen.
        if sprite.is_none()
            && found
                .raw
                .hex
                .is_some_and(|hex| self.view.grid.hex_center(hex).is_err())
        {
            return false;
        }
        overlaps_window(
            effect_rect(sprite, found.raw.outline),
            [self.view.width, self.view.height].map(i64::from),
        )
    }
    /// SPEC-014 §3.18: the preference volumes reach the mixer whenever they
    /// change (a Done, Cancel, Default or a loaded save), resetting the
    /// master volume F2/F3 moved.
    fn sync_volumes(&mut self) {
        let wanted = Volumes::from_preferences(&self.preferences);
        if self.volume_source != Some(wanted) {
            self.volume_source = Some(wanted);
            self.apply_volumes(wanted);
        }
    }
    /// Apply volume settings to the mixer now (the preferences sliders'
    /// live preview, SPEC-014 §3.18). SPEC-008 §3.10: master or music
    /// volume 0 turns music off by volume in the mixer and the world;
    /// turning it back on lets the world replay its last recorded track
    /// (`WorldEffect::Music`, drained on the next tick).
    pub(crate) fn apply_volumes(&mut self, volumes: Volumes) {
        self.volumes = volumes;
        if let Some(on) = self.audio.apply_volumes(volumes) {
            self.host.set_music_enabled(on);
        }
    }
    /// SPEC-014 §3.18: F2 and F3 change the mixer master volume by -2047
    /// and +2047 without updating the preference, so the next preference
    /// application resets it.
    pub(crate) fn master_volume_step(&mut self, delta: i32) {
        self.sync_volumes();
        self.volumes.master = (self.volumes.master + delta).clamp(0, 32767);
        self.audio.set_master(self.volumes.master);
    }
    /// SPEC-008 §3.10 rule 4: a dialogue session's music. Its opening
    /// (`start_gdialog`, after the head window is created) stops the
    /// current track for a talking head, or halves the music volume
    /// without one; its closing (after the windows are destroyed) replays
    /// the last recorded track from its beginning, subject to the music
    /// on/off state, or restores the volume. A session already open
    /// changes nothing, and barter, Review and Ask About keep the session
    /// open. The reference halves the volume a previous no-head session
    /// stored instead of the current one (`[May-improve]`); the safe rule
    /// halves the volume current at the opening and restores it. A volume
    /// of 1 halves to 0, which turns music off by volume (rule 2) until the
    /// restore turns it back on.
    pub(crate) fn dialogue_music_tick(&mut self) {
        let open = self.host.world.dialogue.open;
        match (open, self.dialogue_music) {
            (true, None) => {
                self.dialogue_music = Some(if self.host.world.dialogue.head.is_some() {
                    self.audio.stop_music();
                    DialogueMusic::Stopped
                } else {
                    self.sync_volumes();
                    let volume = self.volumes.music;
                    self.apply_volumes(Volumes {
                        music: volume / 2,
                        ..self.volumes
                    });
                    DialogueMusic::Halved { volume }
                });
            }
            (false, Some(DialogueMusic::Stopped)) => {
                self.dialogue_music = None;
                self.audio.replay_music(self.host.assets.vfs.clone());
            }
            (false, Some(DialogueMusic::Halved { volume })) => {
                self.dialogue_music = None;
                self.apply_volumes(Volumes {
                    music: volume,
                    ..self.volumes
                });
            }
            _ => {}
        }
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
        self.sync_volumes();
        // SPEC-010 §3.11 / §3.13: the level `floor(V x S / 32767)` with S
        // the effects volume setting; without an object V = 32767, so the
        // level is S.
        let level = sfx::applied_volume(
            self.host.world.effect_volume(owner, on_screen),
            self.volumes.effects,
        );
        if !self.audio.play(samples, level, Channel::Effect, false) {
            tracing::debug!(%path, "effect not played (silent output or four effects active)");
        }
    }
    /// Talking-head speech: `sound/speech/<head>/<stem>.ACM`, decoded whole
    /// and played once at level `trunc(0.69 x V)` of the speech volume V,
    /// kept while it plays (SPEC-010 §3.12-3.13; volume 0 plays silently
    /// and still lip-syncs). The
    /// world hears whether it started: without a file or output lip-sync
    /// stops and the next head tick ends the speech.
    fn play_speech(&mut self, sound: &[u8]) {
        self.sync_volumes();
        let level = vault_formats::lip::speech_volume(self.volumes.speech);
        let started = self
            .host
            .speech_file(sound)
            .and_then(|path| self.sounds.load(&*self.host.assets.vfs, &path))
            .is_some_and(|samples| self.audio.play(samples, level, Channel::Speech, false));
        self.speaking = started;
        self.host.speech_started(started);
    }
    /// Background music, the platform's entry point for every music
    /// request the world makes (SPEC-008 §3.10: `wind2`, `03wrldmp` and the
    /// city tracks, each `MusicRequest::track(name, looping)`): it replaces
    /// the track before it, from its beginning, decoded off the event loop;
    /// while music is disabled it is only recorded; a missing track leaves
    /// silence. See `AudioHandle::request_music`.
    pub(crate) fn request_music(&mut self, request: MusicRequest) {
        self.sync_volumes();
        self.audio
            .request_music(self.host.assets.vfs.clone(), request);
    }
    /// A library sound slot: the named file with its looping mode bit, at
    /// SPEC-010 §3.13's full library level (only master volume scales it).
    fn play_library(&mut self, slot: usize, filename: &[u8], mode: i32) {
        let path = String::from_utf8_lossy(filename).replace('\\', "/");
        if let Some(samples) = self.sounds.load(&*self.host.assets.vfs, &path) {
            self.sync_volumes();
            self.audio.play(
                samples,
                sfx::LIBRARY_LEVEL,
                Channel::Library(slot),
                mode & 1 != 0,
            );
        }
    }
    /// Audio follows the world: the speech's playback position reaches the
    /// head's lip-sync (its next tick ends finished speech, SPEC-010
    /// §3.12), and stopped, paused, or freed library slots fall silent.
    fn update_audio(&mut self) {
        let position = self.audio.speech_position();
        self.speaking = position.is_some();
        self.host.speech_clock(position);
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
        self.dialogue_music_tick();
        if !self.host.world.effects.is_empty() {
            self.thaw_view();
        }
        while let Some(effect) = self.host.world.effects.pop_front() {
            match effect {
                WorldEffect::Message(text) => self.notify(text),
                WorldEffect::Elevator(kind) => self.open_elevator(kind),
                WorldEffect::Slideshow | WorldEffect::EndGame => self.endgame = Some(effect),
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
                // SPEC-014 §3.2: the world requests a game fade (`gfade_out`
                // / `gfade_in`, First Aid, Doctor, books) as the standard
                // fade (0) without a forced black start, which blocks.
                WorldEffect::Fade {
                    out,
                    duration,
                    from_black,
                } => {
                    if duration <= 0 && !from_black {
                        self.fades.push_game(out);
                    } else {
                        self.fades.push_library(out, duration, from_black);
                    }
                }
                // The library `fadein` / `fadeout` is timed by its duration
                // and never blocks, `fadeout(0)` included.
                WorldEffect::LibraryFade {
                    out,
                    duration,
                    from_black,
                } => self.fades.push_library(out, duration, from_black),
                WorldEffect::RollHitPoints => self.hud.hp.animate = true,
                WorldEffect::RollArmorClass => self.hud.ac.animate = true,
                WorldEffect::WorldMap => self.open_world_map(),
                WorldEffect::TownMap => self.open_town_map(),
                WorldEffect::MapSaveFailed => self.map_save_failed(),
                // SPEC-008 §3.10: a city track loops.
                WorldEffect::Music(name) => self.request_music(MusicRequest::track(&name, true)),
                // Look and examine already printed their text.
                WorldEffect::DefaultAction {
                    action: ObjectAction::LookAt | ObjectAction::Description,
                    ..
                } => {}
                WorldEffect::DefaultAction { action, target } => {
                    let name = self
                        .host
                        .world
                        .get(target)
                        .map_or(b"object".as_slice(), |object| object.name.as_slice());
                    self.status = [format!("{action:?}: ").as_bytes(), name].concat();
                }
                // SPEC-014 §3.46: a created floating text (a `float_msg` or a
                // taunt) is drawn from the world's floating texts; its
                // effect only asks for the redraw (the view thawed above).
                WorldEffect::Floating { object, .. } | WorldEffect::Taunt { object, .. } => {
                    tracing::trace!(object, "floating text created");
                }
                // The `enhanced` camera's scripted requests (Vanilla never
                // emits them; without the enhanced view they do nothing).
                WorldEffect::Camera(command) => self.camera_command(command),
                other => tracing::debug!(?other, "host presentation request"),
            }
        }
    }
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        self.drain_effects();
        // SPEC-014 §3.2: a game fade to black blocks before anything after
        // its request shows: the frame on screen stays until the screen is
        // black, so `gfade_out; move_to; gfade_in` hides the move.
        if self.fade_blocks()
            && self.fades.freezes()
            && let Some(frame) = &self.frame
        {
            let mut image = frame.clone();
            self.draw_cursor(&mut image);
            return Ok(image);
        }
        self.check_barter();
        self.check_loot();
        // The map view sets it when it draws the in-world cursor.
        self.cursor_in_frame = false;
        let mut image = self.screen_image()?;
        self.held_shown = self.held_button().is_some();
        self.frame = Some(image.clone());
        self.draw_cursor(&mut image);
        Ok(image)
    }
    /// The screen in front composed with the windows over it (the Set
    /// Timer and called-shot windows, a dialog box, the debug overlay),
    /// without the cursor.
    fn screen_image(&mut self) -> AppResult<IndexedImage> {
        let mut image = match &self.screen {
            Screen::Map => self.map_image(),
            Screen::Skilldex(_) => self.skilldex_image(),
            Screen::Elevator(_) => self.elevator_image(),
            Screen::Loot(_) => self.loot_image(),
            Screen::Options(_) => self.options_image(),
            Screen::SaveLoad(_) => self.saveload_image(),
            Screen::World(_) => self.world_image(),
            Screen::Town(_) => self.town_image(),
            Screen::PipBoy(_) => self.pipboy_image(),
            Screen::Inventory(_) => self.inventory_image(),
            Screen::Character(_) => self.character_image(),
            Screen::Barter(_) => self.barter_image(),
            Screen::Movie(_) => self.movie_image().ok_or_else(|| "no movie".into()),
            Screen::Automap(_) => self.automap_image(),
            Screen::Help(_) => self.help_image(),
        }?;
        self.draw_overlays(&mut image);
        Ok(image)
    }
    /// The windows drawn over any screen: the Set Timer and called-shot
    /// windows, a dialog box and the debug overlay.
    fn draw_overlays(&self, image: &mut IndexedImage) {
        self.draw_timer(image);
        self.draw_called(image);
        self.draw_dialog(image);
        self.draw_debug(image);
    }
    /// The last composed frame with the cursor at the pointer's current
    /// place, without composing the scene again. SPEC-014 §3.27: the
    /// in-world cursor is a map object drawn under the upright objects and
    /// roofs, so while it shows, or the last frame showed it, the scene is
    /// composed again.
    pub fn pointer_frame(&mut self) -> Option<IndexedImage> {
        if self.cursor_in_frame || self.world_cursor_at().is_some() {
            return self.image().ok();
        }
        let mut image = self.frame.clone()?;
        self.draw_cursor(&mut image);
        Some(image)
    }
    /// The game palette (a script-loaded one replaces the map's).
    pub(crate) fn game_palette(&self) -> &Palette {
        self.host
            .library
            .palette
            .as_deref()
            .unwrap_or(&self.palette)
    }
    /// The 640-by-380 map view: sprites under the world lighting. Behind a
    /// modal window the world does not run, so the view is rendered once
    /// and reused (P-02: an animated window such as the inventory figure
    /// does not re-render the map) until `thaw_view`.
    pub(crate) fn map_view_image(&mut self) -> AppResult<IndexedImage> {
        let modal = !matches!(self.screen, Screen::Map);
        if modal && let Some(view) = &self.frozen_view {
            return Ok(view.clone());
        }
        // SPEC-014 §3.27: the in-world cursor, a flat object at the hex
        // under the pointer (movement mode adds its red translucent
        // outline and the upright AP-text object). It exists only in the
        // refresh, so no script, save, pick or route ever sees it. A
        // pointer on the `enhanced` world canvas has none in this view.
        let cursor = if self.pointer.world {
            None
        } else {
            self.cursor_objects(self.view)
        };
        let image = self
            .render_scenes(&[(self.view, [0, 0])], cursor.as_ref(), None, None, false)?
            .pop()
            .ok_or("no map view")?;
        self.cursor_in_frame |= cursor.is_some();
        self.frozen_view = modal.then(|| image.clone());
        Ok(image)
    }
    /// The map view's layers over each of `views`, one refresh each: the
    /// sprites under the world lighting. A view's offset is where it lies
    /// in the coordinates `cursor` (the in-world cursor's objects) and
    /// `cutaway` are given in: the 640-by-380 map view is one view at
    /// (0, 0), and the `enhanced` world canvas is tiled into several,
    /// drawn with `overscan` ([`MapScene::overscan`]) so a sprite is never
    /// cut at a tile seam.
    pub(crate) fn render_scenes(
        &mut self,
        views: &[(MapView, [i64; 2])],
        cursor: Option<&pointer::CursorObjects>,
        cutaway: Option<RoofCutaway>,
        shroud: Option<Shroud<'_>>,
        overscan: bool,
    ) -> AppResult<Vec<IndexedImage>> {
        // P-02: the world's map by reference and its drawn objects; no map
        // clone per frame.
        let egg = self.egg_mask();
        let objects = self.host.world.render_objects();
        let map = &self.host.world.map;
        self.art.bind(&self.host.assets, map, &objects)?;
        let ambient = self.host.world.ambient;
        let inks = OutlineInks::new(&self.palette);
        let extended = |pid: u32| {
            self.host
                .assets
                .prototype(pid)
                .map_or(0, |prototype| prototype.extended.0)
        };
        let cutout = self.cutout(egg.as_deref(), &extended);
        // SPEC-014 §3.46: the floating texts, the layer after the outlines.
        let floating = self.floating_sprites();
        // The `enhanced` ground-item highlight while Shift is held on the map.
        let highlight = (self.enhanced.is_some()
            && self.hud.modifiers[0]
            && matches!(self.screen, Screen::Map))
        .then_some(vault_formats::map::outline::ITEM);
        let ring = cursor
            .and_then(|cursor| cursor.ring.as_ref())
            .and_then(|(art, origin)| Some((art.frame(0, 0)?, *origin)));
        let order = cursor.map_or(0, |cursor| self.cursor_order(&objects, cursor.hex));
        let mut images = Vec::with_capacity(views.len());
        for &(view, [ox, oy]) in views {
            // SPEC-014 §3.44: floor tiles are lit per pixel from ten hex
            // lights around each tile; objects take their hex's light and
            // roofs the ambient.
            let tiles = &self.host.world.tile_light[view.elevation as usize];
            let light = MapLight {
                palette: &self.palette,
                ambient,
                tiles,
            };
            let mut image = render_floor_lit(map, view, &self.art, light)?;
            let scene_cursor = cursor.map(|cursor| SceneCursor {
                hex: cursor.hex,
                order,
                art: (&cursor.art, [-ox, -oy]),
                outline: ring.map(|(frame, origin)| {
                    (
                        frame,
                        [origin[0] - ox, origin[1] - oy],
                        vault_formats::map::outline::RED | vault_formats::map::outline::TRANSLUCENT,
                    )
                }),
                text: cursor.text.as_ref().map(|text| (text, [-ox, -oy])),
            });
            let scene = MapScene {
                cutout: cutout.as_ref(),
                light: Some(light),
                outlines: Some(&inks),
                floating: &floating,
                cursor: scene_cursor,
                roof_cutaway: cutaway.map(|cut| RoofCutaway {
                    center: [cut.center[0] - ox, cut.center[1] - oy],
                    ..cut
                }),
                shroud,
                overscan,
                highlight,
            };
            draw_map_scene(&mut image, map, &objects, view, &self.art, scene)?;
            images.push(image);
        }
        Ok(images)
    }
    /// SPEC-014 §3.27 / §3.44 the in-world cursor's place in the view
    /// elevation's object list (insertion order): before the first object
    /// on its hex inserted after its last move, else at the end. Only the
    /// order within its hex's groups matters.
    fn cursor_order(&self, objects: &MapObjects, hex: i32) -> usize {
        let world = &self.host.world;
        let Some(list) = objects.elevations.get(usize::from(self.view.elevation)) else {
            return 0;
        };
        let stamp = self.pointer.cursor_stamp;
        list.iter()
            .position(|object| {
                object.hex == Some(hex)
                    && world
                        .reference_for_id(object.id)
                        .and_then(|reference| world.insertion(reference))
                        .is_none_or(|inserted| inserted > stamp)
            })
            .unwrap_or(list.len())
    }
    /// SPEC-014 §3.37 the cutout mask, loaded once. Without it everything
    /// draws normally (the safe rule), and the failure is reported once.
    pub(crate) fn egg_mask(&mut self) -> Option<Arc<FrmArchive>> {
        if self.egg.is_none() {
            let loaded = self.host.assets.object_art(EGG_ART);
            if let Err(error) = &loaded {
                tracing::warn!(%error, "see-through cutout unavailable");
            }
            self.egg = Some(loaded.ok());
        }
        self.egg.clone().flatten()
    }
    /// The player's cutout over the current view: none without the mask
    /// art or a placed player (the SPEC-014 §3.37 safe rule).
    pub(crate) fn cutout<'a>(
        &'a self,
        egg: Option<&'a FrmArchive>,
        extended: &'a dyn Fn(u32) -> u32,
    ) -> Option<Cutout<'a>> {
        let mask = egg?.frame(0, 0)?;
        let player = self.host.world.get(self.host.world.player)?;
        Some(Cutout {
            mask,
            hex: player.raw.hex?,
            displacement: player.raw.displacement,
            hidden: player.raw.flags.contains(BasicFlags::HIDDEN),
            extended,
            palette: &self.palette,
        })
    }
    /// Input or a world change may have changed what the map view shows.
    fn thaw_view(&mut self) {
        self.frozen_view = None;
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.frozen = None;
        }
    }
    /// The map screen: a conversation's window, or the map view with the
    /// indicator boxes and the interface bar.
    fn map_image(&mut self) -> AppResult<IndexedImage> {
        let image = self.map_view_image()?;
        if self.host.world.dialogue.open {
            let font = self
                .font
                .as_ref()
                .ok_or("dialogue requires a loaded FON/AAF font")?;
            // SPEC-014 §3.45: the Ask About window hides the reply window
            // until it shows an answer, and the option window while open.
            let (reply_hidden, options_hidden) = self.talk_hides();
            let dialogue = &mut self.host.world.dialogue;
            let reply = reply_hidden.then(|| std::mem::take(&mut dialogue.resolved_reply));
            let options = options_hidden.then(|| std::mem::take(&mut dialogue.resolved_options));
            let layout = dialogue::compose(
                &image,
                &self.host.world.dialogue,
                font,
                &self.palette,
                &self.host.assets,
                self.host.world.perk(Perk::Empathy) > 0,
                self.option_scroll,
            );
            let dialogue = &mut self.host.world.dialogue;
            if let Some(reply) = reply {
                dialogue.resolved_reply = reply;
            }
            if let Some(options) = options {
                dialogue.resolved_options = options;
            }
            let layout = layout?;
            if !reply_hidden {
                self.host.world.dialogue.page_offsets = layout.page_offsets;
            }
            self.choices = layout.choices;
            let mut image = layout.image;
            self.draw_talk(&mut image);
            return Ok(image);
        }
        self.choices.clear();
        Ok(self.compose_world(image))
    }
    /// The window title: the plain game name, or with the vault-debug
    /// inspector on (`--debug`, F9) the internal hints (screen, map file,
    /// elevation, cursor mode and the preview keys).
    pub fn title(&self) -> String {
        if !self.debug_enabled() {
            return "Fallout — Vault Engine".into();
        }
        match &self.screen {
            Screen::World(_) => {
                return "Vault — world map — click to travel; click the party to enter".into();
            }
            Screen::Town(_) => return "Vault — town map — choose an area".into(),
            Screen::PipBoy(_) => return "Vault — Pip-Boy — Escape returns".into(),
            Screen::Inventory(_) => {
                return "Vault — inventory — click an item, then an action; Escape returns".into();
            }
            Screen::Character(_) => {
                return "Vault — character — Done keeps, Cancel or Escape restores".into();
            }
            Screen::Barter(_) => {
                return "Vault — barter — drag items between columns; M offers; T or Escape returns"
                    .into();
            }
            Screen::Movie(screen) => {
                return format!(
                    "Vault — movie {} — any key or a click skips",
                    screen.player.request.path
                );
            }
            Screen::Skilldex(_) => {
                return "Vault — Skilldex — choose a skill; Escape cancels".into();
            }
            Screen::Elevator(_) => {
                return "Vault — elevator — choose a level; Escape cancels".into();
            }
            Screen::Loot(_) => {
                return "Vault — loot — drag an item to the other pane; Shift+A takes all; Escape closes"
                    .into();
            }
            Screen::Options(_) => return "Vault — options".into(),
            Screen::SaveLoad(_) => {
                return "Vault — save/load — Enter commits; Escape cancels".into();
            }
            Screen::Automap(_) => {
                return "Vault — automap — Tab, Escape or A closes; H/L detail; S scanner".into();
            }
            Screen::Help(_) => return "Vault — help — any key or a click returns".into(),
            Screen::Map => {}
        }
        let roofs = if !self.roof_key {
            String::new()
        } else if self.view.roofs {
            " — roofs on".into()
        } else {
            " — roofs off".into()
        };
        let keys = if self.preview_keys {
            "; L/D/U/G/T actions; W world map"
        } else {
            ""
        };
        format!(
            "Vault — {} — elevation {}{roofs} — {} cursor; right-click cycles{keys}",
            self.path,
            self.view.elevation,
            self.mode_name()
        )
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        self.thaw_view();
        self.enhanced_key_down(key);
        let handled = self.step_key(key);
        // G-22: a key that opened a window over a held map press drops it.
        self.drop_map_press();
        handled
    }
    fn step_key(&mut self, key: KeyCode) -> bool {
        // SPEC-014 §3.2: no input is polled while a game fade runs.
        if self.fade_blocks() {
            return true;
        }
        // SPEC-014 §3.19: F12 is caught as the key enters the common input
        // queue, before any window, movie or dialog box sees it, so it works
        // everywhere (a hand swap's wait included, §3.23) and no screen
        // receives it. While combat disables the keyboard (§3.27) it is not
        // read: it is held below with the other keys and acts when read.
        if key == KeyCode::F12 && !self.keyboard_locked() && !self.bar_blocks() {
            self.screenshot();
            return true;
        }
        // SPEC-014 §3.29: a modifier or lock key on its own translates to no
        // key code. While the keyboard is disabled its press or repeat still
        // takes a buffer slot (read later, it does nothing); otherwise it is
        // read at once and nothing acts: no window opens or closes, no
        // "any key" test is met (a movie, the help screen, the pause window).
        if crate::input::is_modifier(key) {
            let modal = matches!(self.screen, Screen::Movie(_))
                || self.dialog.is_some()
                || self.hud.timer.is_some()
                || self.hud.called.is_some();
            if !modal && (self.keyboard_locked() || self.bar_blocks()) {
                let modifiers = self.hud.modifiers;
                self.hold_key(hud::HeldKey::Press(key, modifiers));
            }
            return false;
        }
        if self.movie_key() || self.dialog_key(key) || self.timer_key(key) || self.called_key(key) {
            return true;
        }
        // F9 toggles the vault-debug inspector; while it is open, F8 and
        // its editing keys belong to it.
        if key == KeyCode::F9 {
            let on = !self.debug_enabled();
            self.set_debug(on);
            return true;
        }
        if self.debug_key(key) {
            return true;
        }
        // SPEC-014 §3.27: while combat locks the keyboard no key is read,
        // F12 included; presses and releases are held (up to 32) and read
        // in order once it is enabled (`read_held_keys`).
        // §3.21: during a counter roll or the panel animation keys are
        // deferred the same way, and read in order when it ends.
        if self.keyboard_locked() || self.bar_blocks() {
            let modifiers = self.hud.modifiers;
            self.hold_key(hud::HeldKey::Press(key, modifiers));
            return true;
        }
        // §3.23: a hand swap's wait reads and discards every key (F12 was
        // caught above).
        if matches!(self.screen, Screen::Map) && self.swapping_hands() {
            return true;
        }
        // SPEC-014 §3.28-3.29: keyboard input is discarded while the action
        // menu is open or edge scrolling is under way.
        if self.pointer_swallows_keys() {
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return self.barter_key(key);
        }
        // SPEC-014 §3.45: the Review and Ask About windows are modal.
        if self.host.world.dialogue.open && self.talk.is_some() {
            return self.talk_key(key);
        }
        if self.host.world.dialogue.open {
            if self.panel_key(key) {
                return true;
            }
            // The number row: 1-9 pick an option, 0 none.
            let choice = (key_text(key, false).filter(u8::is_ascii_digit))
                .map(|digit| usize::from(digit - b'0').checked_sub(1));
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
            Screen::Skilldex(_) => return self.skilldex_key(key),
            Screen::Elevator(_) => return self.elevator_key(key),
            Screen::Loot(_) => return self.loot_key(key),
            Screen::Options(_) => return self.options_key(key),
            Screen::SaveLoad(_) => return self.saveload_key(key),
            Screen::Automap(_) => return self.automap_key(key),
            Screen::Help(_) => return self.close_help(),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        // SPEC-014 §3.2: F1 shows the help screen.
        if key == KeyCode::F1 && matches!(self.screen, Screen::Map) && !self.host.world.ui_disabled
        {
            self.open_help();
            return true;
        }
        // The `enhanced` world's camera and roof-peek keys.
        if self.enhanced_key(key) {
            return true;
        }
        // SPEC-014 §3.19 game-loop hotkeys (Escape opens the options).
        if self.game_hotkey(key) {
            return true;
        }
        // SPEC-014 §3.17: Escape opens the options only while the bar is
        // enabled, and otherwise does nothing; it never leaves the game
        // (G-01: a script that disabled the interface made it quit).
        if key == KeyCode::Escape {
            return true;
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if key == KeyCode::KeyW && self.preview_keys {
            self.open_world_map_with_music();
            return true;
        }
        let key_number = key_text(key, true).filter(|code| b"LDUGT".contains(code));
        if let Some(key) = key_number
            && self.host.key_callback(i32::from(key))
        {
            return true;
        }
        let action = match key {
            KeyCode::KeyL => Some(ObjectAction::LookAt),
            KeyCode::KeyD => Some(ObjectAction::Description),
            KeyCode::KeyU => Some(ObjectAction::Use),
            KeyCode::KeyG => Some(ObjectAction::Pickup),
            KeyCode::KeyT => Some(ObjectAction::Talk),
            _ => None,
        };
        if let Some(action) = action
            && self.preview_keys
        {
            self.interact(action);
            return true;
        }
        // SPEC-014 §3.29: an arrow key scrolls one step through the same
        // 33 ms gate and blocked test as the edges.
        let arrow = match key {
            KeyCode::ArrowUp => Some(1),
            KeyCode::ArrowRight => Some(3),
            KeyCode::ArrowDown => Some(5),
            KeyCode::ArrowLeft => Some(7),
            _ => None,
        };
        if let Some(direction) = arrow {
            self.scroll_step(direction);
            return true;
        }
        match key {
            KeyCode::KeyR if self.roof_key => self.view.roofs = !self.view.roofs,
            KeyCode::PageUp if self.preview_keys => {
                self.view.elevation = (self.view.elevation + 1).min(2)
            }
            KeyCode::PageDown if self.preview_keys => {
                self.view.elevation = self.view.elevation.saturating_sub(1)
            }
            _ => return false,
        }
        self.last_hover = None;
        // An elevation change keeps the map border (SPEC-014 §3.29).
        if let Some(center) = self.view.grid.pick_hex([320, 190]) {
            self.host.world.set_view_center(center);
        }
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
        self.thaw_view();
        // SPEC-014 §3.2: no input is polled while a game fade runs.
        if self.fade_blocks() {
            return true;
        }
        if self.movie_press() {
            return true;
        }
        // The dialog box and the Set Timer window take the left button
        // only: a right click presses none of their buttons (which would
        // leave a held button with no release), and is swallowed.
        if right && (self.dialog.is_some() || self.hud.timer.is_some()) {
            return true;
        }
        if self.dialog_click(point) || self.timer_click(point) || self.called_click(point, right) {
            return true;
        }
        // SPEC-014 §3.21, §3.25: while a counter rolls or the combat panel
        // animates no click acts. Buttons are sampled, not queued: a left
        // press in the map still down when the animation ends counts as
        // held from the reading that saw it, and its release then acts as
        // an ordinary release; a release during the animation loses it.
        if self.bar_blocks() {
            if !right
                && matches!(self.screen, Screen::Map)
                && !self.host.world.dialogue.open
                && self.map_area(point)
            {
                self.map_press(point);
            }
            return true;
        }
        if matches!(self.screen, Screen::Barter(_)) {
            return self.barter_click(point, right);
        }
        if self.host.world.dialogue.open && !right && self.talk_click(point) {
            return true;
        }
        if self.host.world.dialogue.open {
            if self.talk.is_some() {
                return true;
            }
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
            Screen::World(_) => return self.world_click(point, right),
            Screen::Town(_) => return self.town_click(point, right),
            Screen::PipBoy(_) => return self.pipboy_click(point),
            Screen::Inventory(_) => return self.inventory_click(point, right),
            Screen::Character(_) => return self.character_click(point),
            Screen::Skilldex(_) => return self.skilldex_click(point),
            Screen::Elevator(_) => return self.elevator_click(point),
            Screen::Loot(_) => return self.loot_click(point, right),
            Screen::Options(_) => return self.options_click(point),
            Screen::SaveLoad(_) => return self.saveload_click(point),
            Screen::Automap(_) => return self.automap_click(point),
            Screen::Help(_) => return self.close_help(),
            Screen::Barter(_) | Screen::Movie(_) | Screen::Map => {}
        }
        // SPEC-014 §3.19: the interface bar, then the map area by cursor
        // mode; a right-click in the map cycles the mode.
        // The map area runs through y = 379 (the bar window overlaps it).
        // SPEC-014 §3.26-§3.27, §3.23: while the animated watch shows (the
        // combat lock or a hand swap) the game mouse and the bar take
        // nothing; clicks are discarded, never queued. The Swap hands
        // button alone still presses (§3.27).
        if self.watch_shown() {
            if !right && !self.map_area(point) {
                self.swap_button_press(point);
            }
            return true;
        }
        if !self.map_area(point) {
            return self.bar_click(point, right);
        }
        // While the vault-debug overlay is open a map click selects the
        // object under the pointer instead of acting.
        if self.debug_click(point, right) {
            return true;
        }
        if self.host.world.ui_disabled {
            return false;
        }
        if right {
            self.cycle_cursor();
            return true;
        }
        self.map_press(point);
        true
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        self.thaw_view();
        if self.fade_blocks()
            || !matches!(self.screen, Screen::Map)
            || !self.host.world.dialogue.open
            || self.talk.is_some()
        {
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
        self.clock = now;
        self.sync_volumes();
        // SPEC-014 §3.2: a game fade blocks completely: no world update,
        // animation, game clock, hover or background task runs until it
        // ends; only the fade advances, and sound keeps playing.
        // SPEC-014 §3.28 / §3.2: a frame that skips the world's tick on the
        // local map pauses its sequences, which resume where they stood.
        let local_map = !matches!(self.screen, Screen::World(_) | Screen::Town(_));
        if self.fade_blocks() {
            if local_map {
                self.host.pause_background(now);
            }
            let game = self.host.library.palette.clone();
            let changed = self
                .fades
                .tick(now, game.as_deref().unwrap_or(&self.palette));
            self.update_audio();
            if !self.fade_blocks() {
                self.fade_ended();
            }
            return Ok(changed);
        }
        let crosshair = self.sync_crosshair();
        // SPEC-010 §3.12: the head updater reads the speech position once
        // per tick.
        self.host.speech_clock(self.audio.speech_position());
        // The world and Pip-Boy screens are modal: the local map's scripts
        // and clock wait while travel or rest advances time. An open action
        // menu pauses background map processing.
        let mut world_ticked = false;
        let screen = match &self.screen {
            // M-W3 / SPEC-014 §3.13: a dialog box is modal over every
            // screen: world travel, encounters, the map and the windows
            // below it wait; audio, fades and the cursor below keep running.
            _ if self.dialog.is_some() => Ok(false),
            // A counter roll or an open action menu pauses the world.
            _ if self.bar_blocks() => Ok(false),
            Screen::Map if self.pointer.menu.is_some() || self.timer_open() => Ok(false),
            // SPEC-014 §3.46: floating texts go at the background update,
            // before the scripts may create new ones.
            Screen::Map => {
                world_ticked = true;
                let removed = self.floating_tick(now);
                self.host.tick(now).map(|changed| changed | removed)
            }
            Screen::World(_) | Screen::Town(_) => self
                .world_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            Screen::PipBoy(_) => self
                .pipboy_tick(now)
                .map_err(|error| vault_core::WorldError::Invalid(error.to_string())),
            // Movies and the inventory, character, and barter screens are
            // modal: the world waits for them.
            Screen::Movie(_) => Ok(self.movie_tick(now)),
            Screen::Inventory(_) => Ok(self.inventory_tick()),
            Screen::Barter(_) => Ok(self.barter_tick()),
            Screen::Loot(_) => Ok(self.loot_tick()),
            Screen::Character(_) => Ok(self.character_tick(now)),
            // SPEC-013/014 windows suspend the world.
            Screen::Skilldex(_) => Ok(self.skilldex_tick()),
            Screen::Elevator(_) => Ok(self.elevator_tick(now)),
            Screen::Options(_) => Ok(self.options_tick(now)),
            Screen::Automap(_) | Screen::Help(_) => Ok(false),
            Screen::SaveLoad(_) => Ok(self.saveload_frame(now)),
        };
        if local_map && !world_ticked {
            self.host.pause_background(now);
        }
        let mut changed = match screen {
            Ok(changed) => changed | crosshair,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                tracing::warn!(%error,"world update failed");
                true
            }
        };
        changed |= self.talk_tick(now);
        self.update_audio();
        // The `enhanced` camera moves (and keeps the world's camera hex at
        // the hex nearest its center) before the map view follows it.
        changed |= self.enhanced_tick(now);
        if self.view.grid.pick_hex([320, 190]) != Some(self.host.world.camera_hex) {
            self.view.grid = HexGrid::centered(self.host.world.camera_hex, 640, 380)?;
            changed = true;
        }
        if self.view.elevation != self.host.world.elevation {
            self.view.elevation = self.host.world.elevation;
            changed = true;
        }
        self.dialogue_music_tick();
        if !self.host.world.effects.is_empty() {
            self.drain_effects();
            changed = true;
        }
        let game = self.host.library.palette.clone();
        changed |= self
            .fades
            .tick(now, game.as_deref().unwrap_or(&self.palette));
        changed |= self.pointer_tick();
        changed |= std::mem::take(&mut self.pointer.redraw);
        changed |= self.held_moved();
        changed |= self.counters_tick(now);
        changed |= self.panel_tick(now);
        changed |= self.watch_tick(now);
        changed |= self.called_tick();
        changed |= self.read_held_keys();
        changed |= self.replay_attack();
        self.cues_tick(now);
        changed |= self.debug_tick();
        Ok(changed)
    }
    /// The pointer moved over the frame; returns the map hover once per hex
    /// change, for the shell's log.
    pub fn hover(&mut self, point: [i32; 2]) -> Option<MapHover> {
        self.hover_in(point, false)
    }
    /// [`Self::hover`] at a frame pixel, or with `world` at a pixel of the
    /// `enhanced` world canvas.
    pub(crate) fn hover_in(&mut self, point: [i32; 2], world: bool) -> Option<MapHover> {
        // SPEC-014 §3.2: the pointer is not polled during a game fade; its
        // last position applies when the fade ends.
        if self.fade_blocks() {
            self.fade_pointer = Some(Some(point));
            return None;
        }
        self.pointer_moved_in(point, world);
        if self.host.world.dialogue.open || !matches!(self.screen, Screen::Map) {
            return None;
        }
        let hover = self.pointer_view().hover(point);
        if self.last_hover == Some(hover) {
            return None;
        }
        self.last_hover = Some(hover);
        Some(hover)
    }
}

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
pub(crate) use vault_fixtures::world as fixture;
#[cfg(test)]
mod interface_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
use crate::audio::sound_fixture;
#[cfg(test)]
pub(crate) use vault_fixtures::state as state_fixture;
#[cfg(test)]
mod tests {
    use super::test_support::{interactive, synthetic_corpus};
    use super::*;
    use vault_formats::LocalDirProvider;

    #[test]
    fn spec010_effect_rectangles_grow_while_outlined() {
        let window = [640, 380];
        // A 20 x 30 frame whose right column is x = -1: off screen, until
        // an outline grows it by a pixel on every side.
        let sprite = Some(([-20, 100], [20, 30]));
        assert_eq!(effect_rect(sprite, 0), [-20, 100, -1, 129]);
        assert!(!overlaps_window(effect_rect(sprite, 0), window));
        assert_eq!(effect_rect(sprite, 1), [-21, 99, 0, 130]);
        assert!(overlaps_window(effect_rect(sprite, 1), window));
        // Any outline type counts (16 item, an unlisted 3); bit 31
        // disables it and type 0 is no outline.
        assert!(overlaps_window(effect_rect(sprite, 16), window));
        assert!(overlaps_window(effect_rect(sprite, 3), window));
        assert!(!overlaps_window(effect_rect(sprite, 0x8000_0001), window));
        assert!(!overlaps_window(effect_rect(sprite, 0x0100_0000), window));
        // Inclusive bounds at the far edges.
        assert!(overlaps_window(
            effect_rect(Some(([639, 379], [5, 5])), 0),
            window
        ));
        assert!(!overlaps_window(
            effect_rect(Some(([640, 0], [5, 5])), 0),
            window
        ));
        assert!(!overlaps_window(
            effect_rect(Some(([0, 380], [5, 5])), 0),
            window
        ));
        // Unreadable art: (0, 0)-(0, 0), which overlaps the window.
        assert_eq!(effect_rect(None, 1), [0; 4]);
        assert!(overlaps_window(effect_rect(None, 0), window));
    }

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
    /// SPEC-014 §3.44: the entrance placement reinserts the player last
    /// on its hex, over the synthetic item there; the item is moved onto
    /// that hex again, so it is drawn (and picked) on top.
    fn raise_item(preview: &mut MapPreview) {
        let item = preview
            .host
            .world
            .iter()
            .find(|(_, object)| object.raw.pid == 1 && object.raw.hex == Some(20100))
            .map(|(reference, _)| reference)
            .unwrap();
        preview
            .host
            .world
            .move_object(item, 20100, 0, false)
            .unwrap();
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
        let dir = synthetic_corpus();
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
        // A red button (the Skilldex's Cancel) plays the small-button
        // press, then release, cue.
        assert!(preview.step(KeyCode::KeyS));
        mixer.lock().unwrap().stop(Channel::Effect);
        assert!(preview.click([451 + 48, 5 + 338], false));
        assert!(preview.release(None, false));
        assert!(!preview.release(None, false), "only one release per press");
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
        preview.host.world.dialogue.head = Some(
            TalkingHead::new(
                head,
                Mood::Neutral,
                Default::default(),
                &mut preview.host.world.rng,
            )
            .unwrap(),
        );
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

    /// SPEC-010 §3.12: the speech voice's position drives the talking
    /// head's mouth, at 69% of the speech volume (0 still lip-syncs).
    #[test]
    fn talking_heads_lip_sync_with_their_speech() {
        use vault_core::dialogue::{HeadDefinition, HeadIdle, Mood, TalkingHead};
        let dir = synthetic_corpus();
        let speech = dir.path().join("sound/speech/synth");
        std::fs::create_dir_all(&speech).unwrap();
        std::fs::write(speech.join("hello.acm"), sound_fixture::acm('H')).unwrap();
        // Vector H holds 4 samples (8 bytes). Phoneme 9 (frame 8) shows from
        // byte 2; the final marker lies past the
        // audio end.
        let mut lip = Vec::new();
        for word in [2, 0x5800, 0, 0, 0, 2, 0, 3] {
            lip.extend_from_slice(&i32::to_be_bytes(word));
        }
        lip.extend_from_slice(b"HELLO\0\0\0VOC\0");
        lip.extend_from_slice(&[0, 9]);
        for (kind, position) in [(1, 0), (1, 2), (1, i32::MAX)] {
            lip.extend_from_slice(&i32::to_be_bytes(kind));
            lip.extend_from_slice(&i32::to_be_bytes(position));
        }
        std::fs::write(speech.join("hello.lip"), lip).unwrap();
        let mut preview = interactive(dir.path());
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio;
        preview.image().unwrap();
        for volume in [22281, 0] {
            preview.preferences.speech_volume = volume;
            preview.host.world.dialogue.head = Some(TalkingHead::without_fidget(
                HeadDefinition {
                    index: 0,
                    stem: "synth".into(),
                    counts: [0; 3],
                },
                Mood::Neutral,
                HeadIdle::default(),
            ));
            preview.host.world.dialogue.updater = true;
            preview.host.request_speech(b"hello");
            preview.image().unwrap();
            assert!(preview.speaking);
            // SPEC-010 §3.13: trunc(0.69 x V), 15373 at the default.
            assert_eq!(
                mixer.lock().unwrap().level(Channel::Speech),
                Some(sfx::speech_level(volume))
            );
            let shown = |preview: &MapPreview| {
                let head = preview.host.world.dialogue.head.as_ref().unwrap();
                head.shown.map(|shown| (shown.animation, shown.frame))
            };
            mixer
                .lock()
                .unwrap()
                .fill(&mut [0.0; 2], 1, acm::MIXER_RATE);
            preview.tick(100).unwrap();
            assert_eq!(shown(&preview), Some((10, 8)), "volume {volume}");
            // The voice runs out: the next tick ends the speech normally.
            mixer
                .lock()
                .unwrap()
                .fill(&mut vec![0.0; 1 << 20], 1, acm::MIXER_RATE);
            preview.tick(200).unwrap();
            assert_eq!(shown(&preview), Some((10, 0)));
            assert!(preview.host.world.dialogue.speech.is_none());
            let head = preview.host.world.dialogue.head.as_ref().unwrap();
            assert_eq!(head.idle_seconds, 3);
        }
        // Without the audio file the speech ends on the next tick.
        preview.host.request_speech(b"missing");
        preview.image().unwrap();
        assert!(!preview.speaking);
        preview.tick(300).unwrap();
        assert!(preview.host.world.dialogue.speech.is_none());
    }

    #[test]
    fn game_movies_play_modally_with_subtitles_and_return_to_their_screen() {
        let dir = synthetic_corpus();
        let root = dir.path();
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
        // The movie's path is a debug-mode title hint.
        assert_eq!(preview.title(), "Fallout — Vault Engine");
        preview.set_debug(true);
        assert!(preview.title().contains(r"art\cuts\boil1.mve"));
        preview.set_debug(false);
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
        // A key skips the movie and returns to the map: playback ends on
        // black, and flag 0x02 fades back to the game palette (SPEC-012
        // §3.12, SPEC-014 §3.2).
        assert!(preview.step(KeyCode::Space));
        assert!(!preview.movie_active());
        assert_eq!(preview.display_override(), Some([[0, 0, 0, 255]; 256]));
        for _ in 0..50 {
            now += 16;
            preview.tick(now).unwrap();
        }
        assert!(preview.display_override().is_none());
        assert!(!mixer.lock().unwrap().playing(Channel::Movie));
        assert_eq!(preview.image().unwrap().height(), 480);
        // The startup sequence: iplogo is absent, so only intro plays.
        preview.queue_startup_movies();
        assert!(preview.movie_active());
        preview.set_debug(true);
        assert!(preview.title().contains("intro.mve"));
        preview.set_debug(false);
        while preview.movie_active() {
            now += 16;
            preview.tick(now).unwrap();
            assert!(now < 100_000, "the movie ends by itself");
        }
        assert_eq!(preview.host.world.persistence.movies[..3], [0, 0, 1]);
        // The map fades in from black (a game fade), which holds input
        // until it ends (SPEC-014 §3.2).
        assert!(preview.fade_blocks());
        assert!(preview.step(KeyCode::KeyP), "swallowed");
        assert!(matches!(preview.screen, Screen::Map));
        preview.finish_fades();
        // The Pip-Boy archive lists boil1 (index 11) and replays it from
        // the row's release (SPEC-014 §3.41.5); a click then release skips
        // it, back to the Pip-Boy.
        assert!(preview.step(KeyCode::KeyP));
        assert!(preview.tap([60, 430]), "the Archives button");
        preview.image().unwrap();
        assert!(preview.click([300, 70], false));
        assert!(!preview.movie_active(), "rows act on their release");
        assert!(preview.release(Some([300, 70]), false));
        assert!(preview.movie_active());
        assert!(preview.click([10, 10], false));
        assert!(preview.release(None, false));
        assert!(matches!(preview.screen, Screen::PipBoy(_)));
    }

    #[test]
    fn world_map_music_and_the_music_switch_reach_the_mixer() {
        let dir = synthetic_corpus();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sound/music")).unwrap();
        for track in ["03wrldmp", "wind2", "13carvrn"] {
            std::fs::write(
                root.join(format!("sound/music/{track}.acm")),
                sound_fixture::acm('B'),
            )
            .unwrap();
        }
        let mut preview = interactive(root);
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio.clone();
        preview.tick(0).unwrap();
        // SPEC-008 §3.10: the load's `wind2` and the city track (map 7,
        // `13carvrn`) reached the world; the last one is the city's.
        assert_eq!(
            preview.host.last_music.as_deref(),
            Some("13carvrn"),
            "the world's record"
        );
        // The preview's W opens the world map outside a transition: its
        // music is requested here.
        assert!(preview.step(KeyCode::KeyW));
        preview.tick(16).unwrap();
        assert_eq!(
            audio.last_music(),
            Some(MusicRequest::track("03wrldmp", true))
        );
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        // SPEC-008 §3.10: a music volume of 0 turns music off by volume in
        // the mixer and the world; a request is only recorded, and setting
        // the volume again plays the last recorded track once.
        preview.preferences.music_volume = 0;
        preview.tick(24).unwrap();
        assert!(!audio.music_enabled() && !preview.host.music_enabled);
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        preview.host.request_music("wind2");
        preview.tick(32).unwrap();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(
            audio.last_music(),
            Some(MusicRequest::track("03wrldmp", true))
        );
        preview.preferences.music_volume = 9000;
        preview.tick(40).unwrap();
        assert!(audio.music_enabled() && preview.host.music_enabled);
        preview.tick(48).unwrap();
        assert_eq!(audio.last_music(), Some(MusicRequest::track("wind2", true)));
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        // S-11: a negative town has no town map; the failure path opens
        // the world map, with its music.
        preview.return_to_map();
        preview.host.world.world_map.saved.town = -1;
        preview.open_town_map();
        assert!(matches!(preview.screen, Screen::World(_)));
        preview.tick(64).unwrap();
        assert_eq!(
            audio.last_music(),
            Some(MusicRequest::track("03wrldmp", true))
        );
    }

    #[test]
    fn spec008_dialogue_stops_or_halves_the_music_and_undoes_it_on_close() {
        use vault_core::dialogue::{HeadDefinition, HeadIdle, Mood, TalkingHead};
        let dir = synthetic_corpus();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sound/music")).unwrap();
        std::fs::write(root.join("sound/music/01hub.acm"), sound_fixture::acm('B')).unwrap();
        let mut preview = interactive(root);
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio.clone();
        preview.preferences.music_volume = 9001;
        preview.tick(0).unwrap();
        preview.request_music(MusicRequest::track("01hub", true));
        audio.wait_music();
        let playing = || mixer.lock().unwrap().playing(Channel::Music);
        assert!(playing());
        let head = || {
            TalkingHead::without_fidget(
                HeadDefinition {
                    index: 3,
                    stem: String::new(),
                    counts: [0; 3],
                },
                Mood::Neutral,
                HeadIdle::default(),
            )
        };
        // SPEC-008 §3.10 rule 4: a talking-head session stops the track
        // when it opens; a session already open changes nothing.
        preview.host.world.dialogue.open = true;
        preview.host.world.dialogue.head = Some(head());
        preview.dialogue_music_tick();
        assert!(!playing());
        assert!(audio.music_enabled(), "the on/off state is unchanged");
        preview.dialogue_music_tick();
        assert!(!playing());
        // Closing replays the last recorded track from its beginning.
        preview.host.world.dialogue.open = false;
        preview.host.world.dialogue.head = None;
        preview.dialogue_music_tick();
        audio.wait_music();
        assert!(playing());
        assert_eq!(audio.last_music(), Some(MusicRequest::track("01hub", true)));
        // No head: the music volume current at the opening is halved
        // (truncated) and restored at the close (the safe rule), however
        // long the session stays open.
        let level = vault_formats::sfx::music_level;
        preview.host.world.dialogue.open = true;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 4500);
        assert_eq!(
            mixer.lock().unwrap().level(Channel::Music),
            Some(level(4500))
        );
        preview.tick(16).unwrap();
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 4500, "halved once");
        assert!(playing());
        preview.host.world.dialogue.open = false;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 9001);
        assert_eq!(
            mixer.lock().unwrap().level(Channel::Music),
            Some(level(9001))
        );
        // A second no-head session halves the current volume again, not
        // one stored by the session before (the reference's defect).
        preview.preferences.music_volume = 7001;
        preview.tick(32).unwrap();
        preview.host.world.dialogue.open = true;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 3500);
        preview.host.world.dialogue.open = false;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 7001);
        // A volume of 1 halves to 0, which turns music off by volume; the
        // close turns it back on and the world replays its track.
        preview.preferences.music_volume = 1;
        preview.tick(48).unwrap();
        assert!(audio.music_enabled());
        preview.host.world.dialogue.open = true;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 0);
        assert!(!audio.music_enabled() && !preview.host.music_enabled);
        assert!(!playing());
        preview.host.world.dialogue.open = false;
        preview.dialogue_music_tick();
        assert_eq!(preview.volumes.music, 1);
        assert!(audio.music_enabled() && preview.host.music_enabled);
    }

    #[test]
    fn world_audio_takes_volumes_music_requests_and_movie_music_flags() {
        let dir = synthetic_corpus();
        let root = dir.path();
        crate::movie::tests::movie_corpus(root);
        std::fs::create_dir_all(root.join("sound/sfx")).unwrap();
        std::fs::create_dir_all(root.join("sound/music")).unwrap();
        std::fs::write(root.join("sound/sfx/IB1P1XX1.ACM"), sound_fixture::acm('A')).unwrap();
        std::fs::write(root.join("sound/music/01hub.acm"), sound_fixture::acm('B')).unwrap();
        let mut preview = interactive(root);
        let (audio, mixer) = AudioHandle::detached();
        preview.audio = audio.clone();
        preview.tick(0).unwrap();
        let default = 22281;
        assert_eq!(mixer.lock().unwrap().master(), default);
        // SPEC-010 §3.13: an effect without an object plays at the effects
        // volume (V = 32767, so floor(V x S / 32767) = S).
        preview
            .host
            .world
            .effects
            .push_back(WorldEffect::SoundEffect {
                name: b"IB1P1XX1".to_vec(),
                owner: None,
            });
        preview.image().unwrap();
        assert_eq!(mixer.lock().unwrap().level(Channel::Effect), Some(default));
        // At master 22281 that is -1112 hundredths of a decibel (§4.8).
        let gain = mixer.lock().unwrap().gain(Channel::Effect).unwrap();
        assert!((gain - 10_f32.powf(-1112.0 / 2000.0)).abs() < 1e-6);
        // F2 lowers the mixer master by 2047, not the preference; the next
        // preference application resets it (SPEC-014 §3.18). It re-applies
        // to the playing effect at once (SPEC-010 §3.13).
        assert!(preview.step(KeyCode::F2));
        assert_eq!(mixer.lock().unwrap().master(), 22281 - 2047);
        assert!(mixer.lock().unwrap().gain(Channel::Effect).unwrap() < gain);
        assert_eq!(preview.preferences.master_volume, 22281);
        assert!(preview.step(KeyCode::F3));
        assert!(preview.step(KeyCode::F3));
        assert_eq!(mixer.lock().unwrap().master(), 22281 + 2047);
        // Cancelling the preferences reapplies them, the unchanged volumes
        // included, which drops the F2/F3 offset.
        assert!(preview.step(KeyCode::Escape));
        assert!(preview.step(KeyCode::KeyP));
        assert!(matches!(
            preview.screen,
            Screen::Options(options::OptionsScreen::Preferences(_))
        ));
        assert!(preview.step(KeyCode::Escape));
        assert_eq!(mixer.lock().unwrap().master(), default);
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map));
        assert!(preview.step(KeyCode::F3));
        assert_eq!(mixer.lock().unwrap().master(), 22281 + 2047);
        preview.preferences.master_volume = 30000;
        preview.tick(16).unwrap();
        assert_eq!(mixer.lock().unwrap().master(), 30000);
        // A city-music request plays its track, looping, as stereo frames
        // (SPEC-008 §3.10, SPEC-010 §3.6), at the music volume.
        preview
            .host
            .world
            .effects
            .push_back(WorldEffect::Music("01hub".into()));
        preview.image().unwrap();
        assert_eq!(audio.last_music(), Some(MusicRequest::track("01hub", true)));
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(mixer.lock().unwrap().level(Channel::Music), Some(20944));
        // SPEC-012 §3.12: flag 0x08 pauses the music for the movie and its
        // cleanup resumes it; 0x04 stops it.
        assert!(preview.host.play_game_movie(vault_core::movie::INTRO, 0x08));
        preview.image().unwrap();
        preview.tick(32).unwrap();
        assert!(preview.movie_active() && mixer.lock().unwrap().paused(Channel::Music));
        // A middle button (or a press in the letterbox) skips it too.
        assert!(preview.movie_button(true));
        assert!(preview.movie_button(false));
        assert!(!preview.movie_active());
        assert!(!mixer.lock().unwrap().paused(Channel::Music));
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert!(preview.host.play_game_movie(vault_core::movie::INTRO, 0x04));
        preview.image().unwrap();
        preview.tick(48).unwrap();
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        assert!(preview.step(KeyCode::Space));
        // SPEC-010 §3.11: the player's sprite overlaps the map window, on
        // any elevation (no elevation condition).
        let player = preview.host.world.player;
        assert!(preview.on_screen(player));
        preview.host.world.get_mut(player).unwrap().raw.elevation = 2;
        assert!(preview.on_screen(player));
        // Far off at hex 0 it is off screen, unless its art cannot be
        // read: then its rectangle is (0, 0)-(0, 0), on screen.
        preview.host.world.get_mut(player).unwrap().raw.hex = Some(0);
        assert!(!preview.on_screen(player));
        preview.host.world.get_mut(player).unwrap().raw.art_id = 0x0fff_ffff;
        assert!(preview.on_screen(player));
    }

    #[test]
    fn a_modal_window_reuses_the_frozen_world_view() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.image().unwrap();
        assert!(preview.frozen_view.is_none(), "the map screen renders live");
        assert!(preview.step(KeyCode::KeyS));
        assert!(matches!(preview.screen, Screen::Skilldex(_)));
        let first = preview.image().unwrap();
        assert!(preview.frozen_view.is_some());
        assert_eq!(preview.image().unwrap().pixels(), first.pixels());
        // Input may change the world: the view is rendered again.
        preview.release(None, false);
        assert!(preview.frozen_view.is_none());
    }

    #[test]
    fn inventory_character_and_barter_screens_drive_the_spec011_rules() {
        let dir = synthetic_corpus();
        std::fs::write(
            dir.path().join("text/english/game/editor.msg"),
            b"{136}{}{No free skill points available.}\n",
        )
        .unwrap();
        let mut preview = interactive(dir.path());
        raise_item(&mut preview);
        preview.image().unwrap();
        // Pick up the synthetic item with an arrow-mode click.
        preview.hud.mode = vault_core::ui::cursor::Mode::Arrow;
        preview.tap([320, 185]);
        let item = preview.selected.unwrap();
        let player = preview.host.world.player;
        assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
        // Inventory (SPEC-014 §3.34): the item lifts from the first list
        // row (124, 35) and drops into the right hand (325, 286); dropped
        // back on the list it is unequipped; the right hand takes it again.
        assert!(preview.step(KeyCode::KeyI));
        assert!(matches!(preview.screen, Screen::Inventory(_)));
        assert_eq!(preview.image().unwrap().height(), 480);
        assert!(preview.click([134, 45], false), "first row");
        assert!(preview.release(Some([340, 300]), false), "right hand");
        let flags = |preview: &MapPreview| preview.host.world.get(item).unwrap().raw.flags;
        assert!(flags(&preview).contains(BasicFlags::RIGHT_HAND));
        preview.image().unwrap();
        assert!(preview.click([340, 300], false));
        assert!(preview.release(Some([150, 100]), false));
        assert!(!flags(&preview).contains(BasicFlags::RIGHT_HAND));
        assert!(preview.click([134, 45], false));
        assert!(preview.release(Some([340, 300]), false));
        assert!(flags(&preview).contains(BasicFlags::RIGHT_HAND));
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map));
        // Character (SPEC-014 §3.42): a press on a skill row moves the
        // cursor; the slider's plus with no points is refusal A; Cancel
        // restores the sheet. Small Guns is the row at y = 27 in the hot
        // region (370, 27) 223 by 195; the plus button is at (614, 20).
        assert!(preview.step(KeyCode::KeyC));
        preview.image().unwrap();
        assert!(preview.click([400, 30], false), "Small Guns");
        assert!(preview.click([616, 22], false), "plus");
        let refused = |preview: &MapPreview| {
            matches!(
                &preview.screen,
                Screen::Character(screen)
                    if matches!(&screen.modal, Some(character::Modal::Refusal(notice))
                        if notice.lines == [b"No free skill points available.".to_vec()])
            )
        };
        assert!(refused(&preview));
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape), "the refusal closes");
        assert!(matches!(
            &preview.screen,
            Screen::Character(screen)
                if screen.selected == vault_core::ui::editor::InfoLine::Skill(0)
                    && screen.modal.is_none()
        ));
        assert!(preview.click([200, 180], false), "Armor Class");
        assert!(matches!(
            &preview.screen,
            Screen::Character(screen)
                if screen.selected == vault_core::ui::editor::InfoLine::Derived(0)
        ));
        assert!(preview.step(KeyCode::Escape));
        preview
            .host
            .world
            .player_sheet_mut()
            .unwrap()
            .unspent_skills = 2;
        let invested = preview.host.world.player_sheet().unwrap().invested[0];
        assert!(preview.step(KeyCode::KeyC));
        // The skill cursor is remembered: `+` steps Small Guns.
        assert!(preview.step(KeyCode::NumpadAdd));
        let sheet = preview.host.world.player_sheet().unwrap();
        assert_eq!((sheet.invested[0], sheet.unspent_skills), (invested + 1, 1));
        assert!(preview.click([558, 460], false), "Cancel");
        assert!(preview.release(Some([558, 460]), false));
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
        let Some(character::Modal::Chooser(chooser)) = &screen.modal else {
            panic!("perk chooser at level 3");
        };
        assert!(!chooser.entries.is_empty());
        let perk = vault_core::Perk::from_index(chooser.entries[0].0).unwrap();
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Enter), "Return chooses");
        let sheet = preview.host.world.player_sheet().unwrap();
        assert!(sheet.rank(perk) == 1 && !sheet.pending_perk);
        preview.image().unwrap();
        if let Screen::Character(screen) = &preview.screen
            && screen.modal.is_none()
        {
            assert!(preview.step(KeyCode::Enter), "Done");
        } else {
            assert!(preview.step(KeyCode::Escape));
        }
        assert!(matches!(preview.screen, Screen::Map));
        assert_eq!(
            preview.host.world.player_sheet().unwrap().reconciled_level,
            3
        );
        // Barter: a dialogue's pending request opens it with the target
        // (an arrow click talks to the NPC).
        preview.hud.mode = vault_core::ui::cursor::Mode::Arrow;
        assert!(preview.tap([336, 170]));
        assert!(preview.host.world.dialogue.open);
        preview.host.world.dialogue.pending_barter = true;
        preview.image().unwrap();
        let Screen::Barter(screen) = &preview.screen else {
            panic!("barter");
        };
        let offer = screen.session.offer;
        // SPEC-013 §3.18: the player's first inventory slot (subwindow
        // (29, 35), screen (109, 325)) drags onto the player table's drop
        // rectangle (screen x 245-309, y 310-454); a release elsewhere
        // moves nothing.
        assert!(preview.click([120, 340], false));
        assert!(preview.release(Some([200, 340]), false));
        assert!(preview.host.world.get(offer).unwrap().inventory.is_empty());
        assert!(preview.click([120, 340], false));
        preview.pointer_moved([260, 340]);
        assert!(preview.pointer_frame().is_some(), "the item is the cursor");
        assert!(preview.release(Some([260, 340]), false));
        assert!(!preview.host.world.get(offer).unwrap().inventory.is_empty());
        preview.image().unwrap();
        // Talk (barter window (584, 162)) returns the tables.
        assert!(preview.tap([590, 289 + 168]), "Talk");
        assert!(!preview.host.world.dialogue.pending_barter);
        assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
        assert!(matches!(preview.screen, Screen::Map) && preview.host.world.dialogue.open);
        // Again: an offer for an empty merchant table costs nothing, so Offer
        // (`m`) trades; the result is the conversation's reply meanwhile.
        preview.host.world.dialogue.pending_barter = true;
        preview.image().unwrap();
        let Screen::Barter(screen) = &preview.screen else {
            panic!("barter");
        };
        let merchant = screen.session.merchant;
        assert!(preview.click([120, 340], false));
        assert!(preview.release(Some([260, 340]), false));
        assert!(preview.step(KeyCode::KeyM));
        assert!(matches!(&preview.screen, Screen::Barter(screen) if screen.reply.is_some()));
        assert_eq!(preview.host.world.get(item).unwrap().owner, Some(merchant));
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
        // PageUp and PageDown change the viewed elevation within 0-2.
        for (key, elevation) in [
            (KeyCode::PageUp, 1),
            (KeyCode::PageUp, 2),
            (KeyCode::PageUp, 2),
            (KeyCode::PageDown, 1),
        ] {
            assert!(preview.step(key));
            assert_eq!(preview.view.elevation, elevation);
            preview.image().unwrap();
        }
        assert!(!preview.step(KeyCode::Space));
    }
    #[test]
    fn roof_art_binds_whatever_the_hide_bit_except_the_blank_index() {
        // S-19 / SPEC-014 §3.36: a roof group hidden when the map's art is
        // bound must still draw once it is restored.
        let dir = tempfile::tempdir().unwrap();
        fixture::write_corpus(dir.path());
        let vfs = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let preview = MapPreview::load(vfs, r"MAPS\SYNTHETIC.MAP", "english", None, 1).unwrap();
        let mut map = preview.host.world.map.clone();
        // Roof index 0 (`tile.frm`, used by no floor), hidden, over the
        // blank floor; every other roof is the blank index 1.
        let square = &mut map.tiles[0].as_mut().unwrap()[5050];
        square.word = 0x1000_0001;
        assert!(square.roof_hidden() && square.roof() == 0);
        let mut art = MapSprites::default();
        art.bind(
            &preview.host.assets,
            &map,
            &preview.host.world.render_objects(),
        )
        .unwrap();
        assert!(art.tile_art.first().is_some_and(Option::is_some));
    }
    #[test]
    fn the_pause_window_and_modal_windows_suspend_palette_cycling() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.image().unwrap();
        assert!(preview.palette_cycling());
        // SPEC-014 §3.17: in game, the pause window suspends cycling until
        // a key closes it.
        preview.open_pause();
        assert!(!preview.palette_cycling());
        assert!(preview.step(KeyCode::KeyA));
        assert!(matches!(preview.screen, Screen::Map));
        assert!(preview.palette_cycling());
        // §3.8: so does the character editor, here the character screen.
        assert!(preview.step(KeyCode::KeyC));
        assert!(!preview.palette_cycling());
        assert!(preview.step(KeyCode::Escape));
        assert!(preview.palette_cycling());
        // SPEC-014 §3.39: so does the world map, paused or not.
        assert!(preview.step(KeyCode::KeyW));
        assert!(!preview.palette_cycling());
        assert!(preview.step(KeyCode::KeyP));
        assert!(matches!(&preview.screen, Screen::World(world) if world.paused));
        assert!(!preview.palette_cycling());
    }
    #[test]
    fn clicking_npc_renders_choices_executes_callbacks_and_returns_to_map() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.image().unwrap();
        // SPEC-014 §3.27: a right-click cycles movement to arrow; an arrow
        // click on a talkable critter talks to it.
        assert!(preview.host.library.palette_cycling);
        assert!(preview.click([336, 170], true));
        assert_eq!(preview.hud.mode, vault_core::ui::cursor::Mode::Arrow);
        assert!(preview.tap([336, 170]));
        let selected = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(selected).unwrap().category(),
            Category::Critter
        );
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
        // The map frame now carries the interface bar below the view.
        assert_eq!(preview.image().unwrap().height(), 480);
        // An arrow click on an item picks it up into authoritative inventory.
        raise_item(&mut preview);
        preview.image().unwrap();
        preview.tap([320, 185]);
        let item = preview.selected.unwrap();
        assert_eq!(
            preview.host.world.get(item).unwrap().category(),
            Category::Item
        );
        assert_eq!(
            preview.host.world.get(item).unwrap().owner,
            Some(preview.host.world.player)
        );
    }
}
