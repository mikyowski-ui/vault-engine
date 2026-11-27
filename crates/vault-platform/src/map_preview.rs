use crate::{
    AppResult,
    audio::{AudioHandle, Channel, MusicRequest, Volumes},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vault_core::{
    CharacterSheet, EnginePreset, ObjectRef, WorldEffect, WorldHost, WorldState,
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
    dialogue::ChoiceHit,
    map::{
        Cutout, MapArt, MapHover, MapLight, MapScene, MapView, RoofCutaway, SceneCursor, Shroud,
    },
    outline::OutlineInks,
};

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
mod autoplay_hud;
mod backdrop;
pub(crate) use backdrop::CACHE_MIB_RANGE as BACKDROP_CACHE_MIB_RANGE;
mod barter;
pub(crate) mod bot;
mod called;
mod camera;
mod character;
mod control;
mod counter;
pub(crate) mod debug;
mod debug_ui;
mod dialog;
mod dispatch;
mod elevator;
mod enhanced;
mod export;
mod fade;
pub(crate) use fade::FADE_MS;
mod floating;
mod help;
mod hud;
mod inventory;
pub(crate) mod loot;
mod modal_kit;
mod modals;
mod movies;
mod options;
pub(crate) mod party;
mod pipboy;
mod pointer;
mod qol;
pub(crate) use qol::{BAYER, MESSAGE_FADE_MS};
mod saveload;
pub(crate) mod scene_render;
pub(crate) use scene_render::SharedRenderer;
mod session_bridge;
mod skilldex;
mod talk;
mod timer;
pub(crate) mod touchbar;
mod world;
mod world2;
mod world_kit;

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
    /// SPEC-019 §3.12-§3.13 the Fallout 2 world and town maps.
    World2(Box<world2::World2Screen>),
    Town2(Box<world2::Town2Screen>),
    PipBoy(Box<pipboy::PipBoy>),
    Inventory(inventory::InventoryScreen),
    Character(Box<character::CharacterScreen>),
    Barter(Box<barter::BarterScreen>),
    Movie(Box<movies::MovieScreen>),
    /// SPEC-013 windows over the world.
    /// SPEC-020 §2.1: the core windows own their state.
    Skilldex(vault_core::ui::skilldex::SkilldexWindow),
    Elevator(Box<vault_core::ui::elevator::ElevatorWindow>),
    Loot(Box<loot::LootScreen>),
    /// SPEC-014 options, pause, quit confirmation and preferences.
    Options(options::OptionsScreen),
    SaveLoad(Box<saveload::SaveLoadScreen>),
    /// The automap overlay.
    Automap(vault_core::ui::automap::AutomapWindow),
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
    /// Where the slots live (SPEC-023 §3.15); `None`: `save_root`'s folders.
    pub storage: Option<Arc<dyn crate::storage::StorageBackend>>,
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
    /// Watch Mode's bot, when one is playing this session.
    pub(crate) watch: Option<Box<bot::Watch>>,
    /// The settings profile this session runs, which in-game saves go to.
    pub(crate) profile_name: Option<String>,
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
    /// The `enhanced` backdrop composes a window over a flat map view of
    /// this index (twice, with two indices, to find the pixels the map
    /// shows through); `None` draws the real view.
    pub(crate) view_fill: Option<u8>,
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
    /// SPEC-019 §3.2 the preferences were opened from Fallout 2's main
    /// menu, so closing them leaves the session.
    menu_preferences: bool,
    /// The `enhanced` preset's presentation (camera, dual-layer frame and
    /// roof peeking); `None` is Vanilla, the default.
    enhanced: Option<Box<enhanced::Enhanced>>,
    /// The touch UI's action pill bar (`--touch-ui`); off by default.
    touch: touchbar::TouchBar,
    /// The `enhanced` preset's game speed, tactical overlay and HUD
    /// message (`qol`); inert under Vanilla.
    qol_state: qol::Qol,
    /// SPEC-022 the session's world-canvas renderer (`render_path`): the
    /// reference path unless the session asks for the instanced one.
    scene_renderer: SharedRenderer,
    /// How the last frame's refreshes were drawn (the F9 renderer page).
    render_frame: vault_debug::FrameRender,
    /// The bound map's pre-rendered backdrop and its chunk cache (owner
    /// brief `docs/prerendered-backdrop-cutaway.md`); behind a cell so the
    /// object pick (`&self`) decodes the chunks it reads.
    backdrops: std::cell::RefCell<backdrop::Backdrops>,
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
    /// The map whose tiles are bound: its header name and number, and the
    /// file name it was opened under.
    tiles: Option<([u8; 16], i32, Option<String>)>,
    /// The bound map's tile remap (`[[tile_remap]]`, owner brief
    /// `docs/extended-art-namespaces.md` §3.5): a listed local tile index
    /// draws the row it names. `tile_art` stays indexed by local index.
    remap: Option<vault_formats::TileRemap>,
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
    /// objects without cloning the map file (P-02). `map_file` is the
    /// map's file name under `maps/` (its tile remap's key), when known.
    fn bind(
        &mut self,
        assets: &VfsWorldAssets,
        map: &MapFile,
        objects: &MapObjects,
        map_file: Option<&str>,
    ) -> AppResult<()> {
        let identity = (
            map.header.name,
            map.header.map_number,
            map_file.map(str::to_owned),
        );
        let fingerprint = Self::object_fingerprint(objects);
        if self.tiles.as_ref() == Some(&identity) && self.objects == Some(fingerprint) {
            return Ok(());
        }
        let mut needed = self.unbound_objects(objects)?;
        if self.tiles.as_ref() != Some(&identity) {
            let remap = match map_file {
                Some(file) => assets
                    .tile_remap(file)
                    .map_err(|error| format!("tile remap of {file}: {error}"))?,
                None => None,
            };
            if remap != self.remap {
                // Another remap changes what a local index draws.
                self.bindings
                    .retain(|(category, ..), _| *category != Category::Tile);
                self.missing
                    .retain(|(category, ..)| *category != Category::Tile);
                self.tile_art.clear();
                self.remap = remap;
            }
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
            // A remapped tile index names a row of the resolved tile list,
            // which may lie past 4095 (brief §3.5); the row's file, or no
            // file, is the safe rule's "not drawn".
            let remapped = (category == Category::Tile)
                .then(|| self.remap.as_ref().map(|remap| remap.row(id as u16)))
                .flatten()
                .filter(|row| *row != id);
            let resolved = match remapped {
                Some(row) => match assets.tile_row_path(row) {
                    Some(path) => path,
                    None => {
                        tracing::warn!(tile = id, row, "remapped tile row has no art; not drawn");
                        self.missing.insert(key);
                        continue;
                    }
                },
                None => {
                    // Tile squares store a bare list index (12 bits, so it
                    // always fits); objects store a complete FID, whose own
                    // category bits select the art directory and codec.
                    let fid = if category == Category::Tile {
                        Fid::try_new(4, id, 0, 0, 0)
                            .map_err(|error| format!("tile index {id}: {error}"))?
                    } else {
                        Fid(id)
                    };
                    match catalog.path(fid) {
                        Ok(path) => path,
                        // SPEC-007 §2.1's "no filename" (an extended
                        // identifier whose row a session lacks, brief
                        // §3.6.1): the object is logged and not drawn.
                        Err(error) if category != Category::Tile => {
                            tracing::warn!(art_id = id, %error, "object art has no file; not drawn");
                            self.missing.insert(key);
                            continue;
                        }
                        Err(error) => {
                            return Err(format!("art identifier {:#010x}: {error}", fid.0).into());
                        }
                    }
                }
            };
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
                    tracing::warn!(art_id = id, sprite = %resolved, %error, "object art is missing; not drawn");
                    self.missing.insert(key);
                }
                Err(error) if remapped.is_some() => {
                    tracing::warn!(tile = id, sprite = %resolved, %error, "remapped tile art is missing; not drawn");
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

/// The file name under `maps/` the current map was opened under,
/// lowercase (`synth.map` for the working `SYNTH.SAV`): the key of its tile
/// remap.
fn map_file_name(world: &WorldState) -> Option<String> {
    let name = world.current_map_name()?;
    let stem = name.split_once('.').map_or(name.as_str(), |(stem, _)| stem);
    (!stem.is_empty()).then(|| format!("{}.map", stem.to_ascii_lowercase()))
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
            false,
            None,
        )
    }
    /// [`Self::load_with`] reading `game`'s map and prototype layouts
    /// (SPEC-016) from the first map on.
    #[allow(clippy::too_many_arguments)]
    pub fn load_for_game(
        vfs: Arc<dyn Vfs>,
        path: &str,
        language: &str,
        character: Option<&str>,
        seed: i32,
        preset: EnginePreset,
        game: vault_core::GameProfile,
        sfall: bool,
        mod_session: Option<Arc<vault_core::host_mods::ModSession>>,
    ) -> AppResult<Self> {
        let assets = VfsWorldAssets::new(vfs, language)?;
        assets.set_game_profile(game);
        let character = if let Some(path) = character {
            CharacterSheet::from_gcd(&assets.gcd(path)?, Default::default())
        } else {
            CharacterSheet::default()
        };
        Self::with_setup(assets, path, character, seed, preset, |host| {
            crate::session::start_sfall(host, sfall);
            crate::session::start_mods(host, mod_session);
        })
    }
    /// The world for `path` with a ready player sheet (SPEC-014 §3.12's
    /// new-game hand-off passes the selected or edited character).
    /// `preset` is set before the campaign loads, so its table overrides
    /// and script parse profile apply from the first map on. `setup` runs on
    /// the new host after the preset is set and before the campaign and the
    /// map load: a Fallout 2 game installs its world state there (SPEC-018
    /// §6.6 step 3).
    pub fn with_setup(
        assets: VfsWorldAssets,
        path: &str,
        character: CharacterSheet,
        seed: i32,
        preset: EnginePreset,
        setup: impl FnOnce(&mut WorldHost<VfsWorldAssets>),
    ) -> AppResult<Self> {
        let path = VfsPath::new(path)?;
        let palette_bytes = assets.vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?;
        let palette = Palette::parse(&palette_bytes)?;
        // A backdrop is checked against the raw bytes of this palette file.
        let backdrops = backdrop::Backdrops::new(&palette_bytes);
        let font = assets.font().ok();
        let mut host = WorldHost::new(WorldState::new(character, seed)?, assets);
        host.set_preset(preset);
        setup(&mut host);
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
        // Extended art (`docs/extended-art-namespaces.md` §3.1 item 3): each
        // resolved list past 4,096 rows is reported once, in the log only.
        for line in host.assets.take_art_diagnostics() {
            tracing::warn!(diagnostic = %line, "art list");
        }
        let objects = host.world.render_objects();
        let mut art = MapSprites::default();
        let map_file = map_file_name(&host.world);
        art.bind(&host.assets, &host.world.map, &objects, map_file.as_deref())?;
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
            storage: None,
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
            watch: None,
            profile_name: None,
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
            view_fill: None,
            egg: None,
            interrupt_walk: true,
            character_memory: character::Remembered::default(),
            menu_preferences: false,
            enhanced: None,
            touch: touchbar::TouchBar::default(),
            qol_state: qol::Qol::default(),
            scene_renderer: SharedRenderer::default(),
            render_frame: Default::default(),
            backdrops: std::cell::RefCell::new(backdrops),
        })
    }
    /// SPEC-014 §3.19 the `/` and `?` date line, "December: 5/2161 7:21":
    /// the month name is `editor.msg` 500 + month - 1.
    pub(crate) fn clock_text(&self) -> Vec<u8> {
        let world = &self.host.world;
        let [month, day, year] = world.game_date();
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
    /// The game palette (a script-loaded one replaces the map's): what the
    /// display shows and what the lit scene's tables are made from.
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
        if let Some(fill) = self.view_fill {
            let mut view = IndexedImage::new(640, 380)?;
            view.fill_rect([0, 0, 640, 380], fill);
            return Ok(view);
        }
        let modal = !matches!(self.screen, Screen::Map);
        if modal && let Some(view) = &self.frozen_view {
            return Ok(view.clone());
        }
        // SPEC-014 §3.27: the in-world cursor, the top layer at the hex
        // under the pointer (movement mode adds its red translucent
        // outline and the AP-text object). It exists only in the
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
        let map_file = map_file_name(&self.host.world);
        self.art
            .bind(&self.host.assets, map, &objects, map_file.as_deref())?;
        let ambient = self.host.world.ambient;
        // SPEC-001 §3.7: every palette file load, `loadpalettetable` among
        // them, replaces the intensity, mix and blend tables. Fades and
        // cycling change only the display palette, which this canvas of
        // indexes never reads (SPEC-022 §3.11 rule 2).
        let inks = OutlineInks::new(self.game_palette());
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
        let highlight =
            (self.enhanced_extras() && self.hud.modifiers[0] && matches!(self.screen, Screen::Map))
                .then_some(vault_formats::map::outline::ITEM);
        let ring = cursor
            .and_then(|cursor| cursor.ring.as_ref())
            .and_then(|(art, origin)| Some((art.frame(0, 0)?, *origin)));
        // SPEC-022: each view is one refresh on the session's render path.
        let shared = self.scene_renderer.clone();
        // One palette for all of this frame's refreshes.
        shared.lock().renderer.begin_frame();
        let mut refreshes = scene_render::FrameRefreshes::new(&shared);
        let mut images = Vec::with_capacity(views.len());
        // The bound map's backdrop, under every refresh of its elevation
        // under either preset (brief §3.5 and question 7).
        self.backdrops.borrow_mut().sync(&self.host);
        for &(view, [ox, oy]) in views {
            let prepared = self
                .backdrops
                .borrow_mut()
                .prepare(&*self.host.assets.vfs, &view);
            let tiles = prepared
                .as_ref()
                .map(backdrop::Prepared::tiles)
                .unwrap_or_default();
            let backdrop = prepared.as_ref().map(|prepared| prepared.view(&tiles));
            // SPEC-014 §3.44: floor tiles are lit per pixel from ten hex
            // lights around each tile; objects take their hex's light and
            // roofs the ambient.
            let tiles = &self.host.world.tile_light[view.elevation as usize];
            let light = MapLight {
                palette: self.game_palette(),
                ambient,
                tiles,
            };
            let scene_cursor = cursor.map(|cursor| SceneCursor {
                hex: cursor.hex,
                art: (
                    &cursor.art,
                    [cursor.art_origin[0] - ox, cursor.art_origin[1] - oy],
                ),
                outline: ring.map(|(frame, origin)| {
                    (
                        frame,
                        [origin[0] - ox, origin[1] - oy],
                        vault_formats::map::outline::RED | vault_formats::map::outline::TRANSLUCENT,
                    )
                }),
                text: cursor.text.as_ref().map(|text| {
                    (
                        text,
                        [cursor.text_origin[0] - ox, cursor.text_origin[1] - oy],
                    )
                }),
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
                backdrop: backdrop.as_ref(),
            };
            // The floor, lit per pixel, then the scene's layers.
            images.push(refreshes.render(map, &objects, view, &self.art, scene)?);
        }
        self.render_frame = refreshes.counts;
        Ok(images)
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
            palette: self.game_palette(),
        })
    }
    /// Input or a world change may have changed what the map view shows.
    fn thaw_view(&mut self) {
        self.frozen_view = None;
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.frozen = None;
        }
    }
}

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
mod extended_art_tests;
#[cfg(test)]
mod fallout2_combat_tests;
#[cfg(test)]
pub(crate) use vault_fixtures::world as fixture;
#[cfg(test)]
mod interface_tests;
#[cfg(test)]
mod lit_palette_tests;
#[cfg(test)]
mod sfall_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
use crate::audio::sound_fixture;
#[cfg(test)]
pub(crate) use vault_fixtures::state as state_fixture;
#[cfg(test)]
mod tests;
