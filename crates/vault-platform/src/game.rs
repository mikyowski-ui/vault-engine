//! SPEC-014 game flow outside the world (`--game`): the splash and start
//! movies, the main menu and attract mode, credits and quotes, premade
//! selection, the character editor in creation mode, the new-game hand-off,
//! loading from the menu, the death scene, the endgame slideshow and the
//! ending sequence. The world itself is a `MapPreview`. Palette fades run
//! over the fixed 700 ms of the SPEC-014 §3.2 safe rule.
use crate::{
    AppResult,
    audio::{AudioHandle, Channel, MusicRequest, Volumes},
    editor_screen::{EditorExit, EditorStage, draw_editor},
    interface::Painter,
    map_preview::MapPreview,
    movie::{MoviePlayer, SubtitleStyle},
    session::EditionRules,
};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};
use vault_core::{
    CharacterSheet, WorldEffect,
    assets::{VfsWorldAssets, WorldAssets},
    movie::{self, MovieRequest},
    stats::StatContext,
    ui::{
        Rect,
        editor::{self, Editor},
        endgame::{self, CreditInput, CreditStyle, Credits, Slide},
        fallout2 as ui2,
        flow::{self, MenuChoice, MenuInput, SelectionAction, SelectionKey},
        options::Preferences,
        saveload::Mode,
    },
};
use vault_formats::{MAX_ASSET_BYTES, Palette, Vfs, credits, rix::Splash};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// What follows a movie.
enum Next {
    Menu,
    NewGame(Box<CharacterSheet>),
    Credits,
}
enum Stage {
    Splash {
        image: IndexedImage,
        palette: Palette,
        since: u64,
    },
    Movie {
        player: Box<MoviePlayer>,
        next: Next,
    },
    Menu {
        since: u64,
    },
    Credits {
        scroll: Credits,
        style: CreditStyle,
        last: u64,
    },
    Selection,
    Editor(Box<EditorStage>),
    World(Box<MapPreview>),
    Load(Box<MapPreview>),
    Death {
        /// When the narration (or its silent wait) began.
        since: u64,
        palette: Palette,
        /// The scene is over and the fade to black runs.
        leaving: bool,
        /// §3.5 step 3: the D(1, 4) draw naming the narration.
        draw: i32,
        /// The narration began after the fade from white; whether its
        /// speech started.
        narration: Option<bool>,
        /// Input arrived during the fade from white: it ends the wait the
        /// moment the narration would start.
        input: bool,
        /// The wait ended without input: the 500 ms tail runs from here.
        tail: Option<u64>,
    },
    /// SPEC-014 §3.5 the end of a game session: the world's last frame
    /// fades to white, then the map is unloaded; `death` leads to the
    /// death scene, otherwise to the main menu.
    Ending {
        world: Box<MapPreview>,
        death: bool,
    },
    /// The endgame slideshow, returning to its world afterward.
    Slides {
        slides: Vec<Shown>,
        index: usize,
        since: u64,
        palette: Palette,
        world: Box<MapPreview>,
    },
}

pub struct Game {
    vfs: Arc<dyn Vfs>,
    assets: VfsWorldAssets,
    palette: Palette,
    language: String,
    seed: i32,
    save_root: PathBuf,
    /// SPEC-014 §3.19 where F12 writes `scrNNNNN.bmp`: the process's
    /// working directory (the empty path).
    pub(crate) screenshot_dir: PathBuf,
    /// SPEC-014 §3.29 main-menu loop passes, each of which clears the
    /// keyboard.
    menu_passes: u64,
    config_path: Option<PathBuf>,
    pub preferences: Preferences,
    /// `fallout.cfg` `[system] interrupt_walk` (SPEC-014 §3.27), read once
    /// at startup beside the splash.
    interrupt_walk: bool,
    /// `fallout.cfg` `[sound] music` (SPEC-014 §2.3, default 1), read once
    /// at startup: 0 keeps music off for the whole run (SPEC-008 §3.10).
    music_configured: bool,
    stage: Stage,
    movies: VecDeque<(String, u32, Next)>,
    now: u64,
    attract: flow::Attract,
    selection: usize,
    sheet: CharacterSheet,
    /// Held modifiers: shift, control, alt.
    pub modifiers: [bool; 3],
    pub audio: AudioHandle,
    /// The save/load selection and quick-slot flag, kept for the session.
    slot_list: vault_core::ui::saveload::SlotList,
    quick: vault_core::ui::saveload::QuickSlot,
    /// State that lasts the whole program run, not one game: the critter
    /// script cycle counter (SPEC-006 §3.7) and the talking head's idle
    /// counter and threshold (SPEC-006 §3.11).
    critter_cycle: usize,
    head_idle: vault_core::dialogue::HeadIdle,
    /// A headless replay: worlds complete movies instead of waiting.
    pub headless: bool,
    /// The vault-debug inspector runs in each world (`--debug`; F9 in a
    /// world toggles it, and the setting carries to the next world).
    pub debug: bool,
    /// `--debug` was given: the worlds accept the map preview's keys
    /// (G-09); without it they keep only the game's own keys.
    pub debug_keys: bool,
    /// The configured engine preset: every world starts with it, and the
    /// enhanced one is named in the title.
    pub preset: vault_core::EnginePreset,
    /// The `enhanced` presentation every world starts with (`None`:
    /// Vanilla); Alt+V changes it through `AssetPreview::set_enhanced`.
    pub enhanced: Option<crate::enhanced::EnhancedSettings>,
    /// The touch UI's pill bar shows in every world (`--touch-ui`).
    pub touch_ui: bool,
    /// The `enhanced` preset's game speed every world runs at (Alt+F
    /// changes it through `AssetPreview::set_game_speed`).
    pub game_speed: crate::enhanced::GameSpeed,
    /// Where each world's F9 overlay exports the current setup as a mod.
    pub mod_export: Option<PathBuf>,
    sounds: crate::map_preview::SoundBank,
    finished: bool,
    /// SPEC-014 §3.2 a fade outside the world, and the six-bit levels shown
    /// in place of the stage's palette once one has ended.
    fade: Option<GameFade>,
    shown: Option<[[u8; 3]; 256]>,
    /// The menu, selection or editor button pressed and still held, as a
    /// screen rectangle, and the pointer: the button shows its down art
    /// while the pointer stays over it (a world tracks its own).
    held: Option<Rect>,
    pointer: Option<[i32; 2]>,
    /// The last frame drew the held button down.
    held_shown: bool,
    /// The preference volumes the mixer last took (outside a world, which
    /// applies its own).
    volume_source: Option<Volumes>,
    /// The selected premade's bio text, read once per selection (P-02).
    bio: Option<(usize, Vec<u8>)>,
    /// The game whose data this is: Fallout 2 follows SPEC-018 §6. What
    /// differs by edition is read from its [`EditionRules`].
    pub game_profile: vault_core::GameProfile,
    /// SPEC-018 §6.6 step 1: the Fallout 2 tables, parsed before the
    /// selection screen (and again for a load from the menu).
    fallout2: Option<Arc<vault_core::fallout2::Fallout2Tables>>,
    /// The last refusal shown on returning to the menu (a table that
    /// SPEC-018 §4 refuses names its file and line).
    pub last_error: Option<String>,
}

/// One fade over the fixed 700 ms of the §3.2 safe rule: step k of n sets
/// each level to `old - trunc((old - new) * k / n)`; the target is
/// installed exactly at the end.
struct GameFade {
    from: [[u8; 3]; 256],
    to: [[u8; 3]; 256],
    start: u64,
}
const FADE_STEPS: u32 = 60;
const WHITE: [[u8; 3]; 256] = [[63; 3]; 256];
fn levels(palette: &Palette) -> [[u8; 3]; 256] {
    std::array::from_fn(|index| palette.rgb6(index as u8))
}

/// A palette from six-bit RGB triples (with an empty lookup table).
fn palette_from(levels: &[[u8; 3]; 256]) -> Palette {
    let mut bytes = vec![0; 0x8300];
    for (index, triple) in levels.iter().enumerate() {
        bytes[3 * index..3 * index + 3].copy_from_slice(triple);
    }
    Palette::parse(&bytes).expect("a complete base palette")
}
/// The window title in normal play (without `--debug`).
fn play_title(enhanced: bool) -> &'static str {
    if enhanced {
        "Fallout — Vault Engine [Enhanced]"
    } else {
        "Fallout — Vault Engine"
    }
}
fn black() -> Palette {
    palette_from(&[[0; 3]; 256])
}
/// A letter key's lower-case letter or a number-row key's digit.
fn letter(key: KeyCode) -> Option<char> {
    crate::input::key_text(key, false)
        .filter(u8::is_ascii_alphanumeric)
        .map(char::from)
}

impl Game {
    pub fn new(
        vfs: Arc<dyn Vfs>,
        language: &str,
        seed: i32,
        save_root: PathBuf,
        config_path: Option<PathBuf>,
    ) -> AppResult<Self> {
        let assets = VfsWorldAssets::new(vfs.clone(), language)?;
        let palette = Palette::parse(&vfs.read_bounded("color.pal", MAX_ASSET_BYTES)?)?;
        let config = config_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| vault_formats::cfg::ConfigFile::parse(&text))
            .unwrap_or_default();
        let preferences = Preferences::from_config(&config);
        let mut game = Self {
            vfs,
            assets,
            palette,
            language: language.to_owned(),
            seed,
            save_root,
            screenshot_dir: PathBuf::new(),
            menu_passes: 0,
            config_path,
            preferences,
            interrupt_walk: true,
            music_configured: config.get_int("sound", "music").unwrap_or(1) != 0,
            stage: Stage::Menu { since: 0 },
            movies: VecDeque::new(),
            now: 0,
            attract: flow::Attract::default(),
            selection: 0,
            sheet: CharacterSheet::default(),
            modifiers: [false; 3],
            audio: AudioHandle::default(),
            slot_list: Default::default(),
            quick: Default::default(),
            critter_cycle: 0,
            head_idle: Default::default(),
            headless: false,
            debug: false,
            debug_keys: false,
            preset: vault_core::EnginePreset::Vanilla,
            enhanced: None,
            touch_ui: false,
            game_speed: Default::default(),
            mod_export: None,
            sounds: Default::default(),
            finished: false,
            fade: None,
            shown: None,
            held: None,
            pointer: None,
            held_shown: false,
            volume_source: None,
            bio: None,
            game_profile: vault_core::GameProfile::Fallout1,
            fallout2: None,
            last_error: None,
        };
        // §3.1 splash: the configured start, without wrapping the stored value.
        let start = config.get_int("system", "splash").unwrap_or(0);
        // §3.27: whether a movement click interrupts a busy player.
        game.interrupt_walk = vault_core::clicks::interrupt_walk(&config);
        let exists = |path: &str| game.assets.available(path.as_bytes());
        if let Some((path, stored)) = flow::choose_splash(start, exists)
            && let Ok(splash) = game
                .vfs
                .read_bounded(&path, MAX_ASSET_BYTES)
                .map_err(|e| e.to_string())
                .and_then(|b| Splash::parse(&b).map_err(|e| e.to_string()))
        {
            let image = IndexedImage::from_pixels(640, 480, splash.pixels)?;
            // The next start tries the following splash (stored unwrapped).
            game.write_config(|config| config.set("system", "splash", stored.to_string()));
            game.stage = Stage::Splash {
                image,
                palette: palette_from(&splash.palette),
                since: 0,
            };
        } else {
            game.start_movies();
        }
        Ok(game)
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Attach the audio output: SPEC-008 §3.10 rule 1 sets the music
    /// on/off state from the startup configuration and its volumes. A menu
    /// entered before it was attached (no splash or start movies) starts
    /// its music now.
    pub fn set_audio(&mut self, audio: AudioHandle) {
        self.audio = audio;
        let volumes = Volumes::from_preferences(&self.preferences);
        self.audio.configure_music(self.music_configured, volumes);
        self.volume_source = Some(volumes);
        if matches!(self.stage, Stage::Menu { .. }) {
            self.sync_volumes();
            self.request_menu_music();
        }
    }
    /// §3.4 the menu music, from its beginning, looping.
    fn request_menu_music(&mut self) {
        self.audio.request_music(
            self.vfs.clone(),
            MusicRequest {
                path: flow::MENU_MUSIC.to_owned(),
                looping: true,
            },
        );
    }
    /// §3.1 step 9: iplogo, then intro, on every start; then the menu.
    fn start_movies(&mut self) {
        for (path, flags) in flow::START_MOVIES {
            self.movies.push_back((path.to_owned(), flags, Next::Menu));
        }
        self.next_movie_or(Next::Menu);
    }
    /// Play `path` (missing movies are skipped), then continue with `next`.
    fn play_movie(&mut self, path: &str, flags: u32, next: Next) {
        self.movies.push_front((path.to_owned(), flags, next));
        self.next_movie_or(Next::Menu);
    }
    fn next_movie_or(&mut self, fallback: Next) {
        while let Some((path, flags, next)) = self.movies.pop_front() {
            if !self.assets.available(path.as_bytes()) {
                if self.movies.is_empty() {
                    return self.continue_with(next);
                }
                continue;
            }
            let windows = path.replace('/', "\\");
            let index = movie::GAME_MOVIES
                .iter()
                .position(|m| path.rsplit('/').next() == Some(m.file));
            let subtitles = (self.preferences.subtitles != 0)
                .then(|| movie::subtitle_path(&self.language, &windows))
                .filter(|subtitles| self.assets.available(subtitles.as_bytes()));
            let fades =
                Some(movie::fade_path(&windows)).filter(|f| self.assets.available(f.as_bytes()));
            let style = subtitles
                .as_ref()
                .and_then(|_| SubtitleStyle::load(&*self.vfs, self.assets.font_number(101)));
            let request = MovieRequest {
                index,
                flags,
                path: windows,
                subtitles,
                fades,
            };
            match MoviePlayer::load(&*self.vfs, request, style) {
                Ok(mut player) => {
                    // The movie level follows the music volume (SPEC-012
                    // §3.8).
                    self.sync_volumes();
                    player.audio = self.audio.clone();
                    self.stage = Stage::Movie {
                        player: Box::new(player),
                        next,
                    };
                    return;
                }
                Err(error) => tracing::warn!(%error, "movie unavailable"),
            }
            if self.movies.is_empty() {
                return self.continue_with(next);
            }
        }
        self.continue_with(fallback);
    }
    fn continue_with(&mut self, next: Next) {
        match next {
            Next::Menu => self.enter_menu(),
            Next::NewGame(sheet) => self.enter_world(*sheet),
            Next::Credits => self.enter_credits("credits.txt", CreditStyle::Normal),
        }
    }
    /// The menu is shown with a fade (§3.4): from a white or black screen
    /// left by the end of a session, to the game palette. Every loop top
    /// stops the menu music and restarts it from its beginning, looping.
    fn enter_menu(&mut self) {
        self.stage = Stage::Menu { since: self.now };
        self.sync_volumes();
        self.request_menu_music();
        if self.shown.is_some() || self.fade.is_some() {
            let target = levels(&self.palette);
            self.fade_to(target);
        }
    }
    /// The six-bit levels on screen now.
    fn shown_levels(&self) -> [[u8; 3]; 256] {
        if let Some(fade) = &self.fade {
            let elapsed = self
                .now
                .saturating_sub(fade.start)
                .min(crate::map_preview::FADE_MS);
            let step = (elapsed * u64::from(FADE_STEPS) / crate::map_preview::FADE_MS) as u32;
            return flow::fade_palette(&fade.from, &fade.to, step, FADE_STEPS);
        }
        self.shown.unwrap_or_else(|| levels(self.palette()))
    }
    fn fade_to(&mut self, target: [[u8; 3]; 256]) {
        let from = self.shown_levels();
        self.fade = Some(GameFade {
            from,
            to: target,
            start: self.now,
        });
    }
    /// Finish a fade whose time is up; returns whether one ended.
    fn fade_tick(&mut self) -> bool {
        let Some(fade) = &self.fade else {
            return false;
        };
        if self.now < fade.start + crate::map_preview::FADE_MS {
            return false;
        }
        let to = fade.to;
        self.fade = None;
        // Back on the game palette in the menu, nothing replaces it.
        self.shown = (!matches!(self.stage, Stage::Menu { .. }) || to != levels(&self.palette))
            .then_some(to);
        true
    }
    fn enter_credits(&mut self, file: &str, style: CreditStyle) {
        let path = format!("text/{}/{file}", self.language);
        let rows = self
            .vfs
            .read_bounded(&path, MAX_ASSET_BYTES)
            .map(|bytes| credits::parse_credits(&bytes))
            .unwrap_or_default();
        let painter = Painter {
            assets: &self.assets,
            palette: &self.palette,
        };
        let scroll = Credits::new(rows, |row| painter.width(style.font(row.kind).0, &row.text));
        self.stage = Stage::Credits {
            scroll,
            style,
            last: self.now,
        };
    }
    /// §3.12 the new-game hand-off: the selected player on the start map.
    fn enter_world(&mut self, sheet: CharacterSheet) {
        match self.world_with(sheet) {
            Ok(mut world) => {
                // SPEC-018 §6.6 step 4: the completed hand-off prints the
                // script gate's notice.
                world.host.fallout2_notice();
                self.stage = Stage::World(Box::new(world));
            }
            Err(error) => {
                tracing::error!(%error, "new game failed");
                self.enter_menu();
            }
        }
    }
    /// Update and write `fallout.cfg`, when one is configured.
    fn write_config(&self, change: impl FnOnce(&mut vault_formats::cfg::ConfigFile)) {
        let Some(path) = &self.config_path else {
            return;
        };
        let mut config = std::fs::read_to_string(path)
            .map(|text| vault_formats::cfg::ConfigFile::parse(&text))
            .unwrap_or_default();
        change(&mut config);
        if let Err(error) = std::fs::write(path, config.write()) {
            tracing::warn!(%error, path = %path.display(), "configuration not written");
        }
    }
    /// §3.18 a brightness hotkey outside the world: applied, stored and
    /// written at once.
    fn brightness_step(&mut self, up: bool) {
        self.preferences.brightness =
            vault_core::ui::options::brightness_step(self.preferences.brightness, up);
        let preferences = self.preferences.clone();
        self.write_config(|config| preferences.write_config(config));
    }
    /// The display brightness: the world's preference while one runs.
    pub fn brightness(&self) -> f64 {
        match &self.stage {
            Stage::World(world) | Stage::Load(world) => world.preferences.brightness,
            _ => self.preferences.brightness,
        }
    }
    /// SPEC-014 §3.18: the preference volumes reach the mixer when they
    /// change (a world applies its own while it runs). Music that they
    /// turn back on replays the last recorded track (SPEC-008 §3.10).
    fn sync_volumes(&mut self) {
        let wanted = Volumes::from_preferences(&self.preferences);
        if self.volume_source != Some(wanted) {
            self.volume_source = Some(wanted);
            if self.audio.apply_volumes(wanted) == Some(true) {
                self.audio.replay_music(self.vfs.clone());
            }
        }
    }
    /// Leaving a world keeps its preferences and save-slot session state;
    /// the mixer takes the preference volumes again.
    fn leave_world(&mut self, world: &MapPreview) {
        self.volume_source = None;
        self.preferences = world.preferences.clone();
        (self.slot_list, self.quick) = world.slot_session();
        self.critter_cycle = world.host.critter_cycle;
        self.head_idle = world.host.world.dialogue.idle;
        self.debug = world.debug_enabled();
        // The enhanced presentation (an Alt+R roof mode) carries on.
        if self.enhanced.is_some() {
            self.enhanced = world.enhanced().or(self.enhanced);
        }
    }
    /// SPEC-018 §6.6 step 1: under Fallout 2, parse the §4 tables and the
    /// campaign initializer before the selection screen (or a load). A
    /// refusal returns to the menu with a message naming the file and line;
    /// nothing has changed then.
    fn prepare_fallout2(&mut self) -> bool {
        if !self.rules().campaign_tables {
            return true;
        }
        let assets = match VfsWorldAssets::new(self.vfs.clone(), &self.language) {
            Ok(assets) => assets,
            Err(error) => return self.refuse(error.to_string()),
        };
        assets.set_game_profile(self.game_profile);
        let host = vault_core::WorldHost::new(
            match vault_core::WorldState::new(CharacterSheet::default(), self.seed) {
                Ok(world) => world,
                Err(error) => return self.refuse(error.to_string()),
            },
            assets,
        );
        let tables = match host.load_fallout2_tables() {
            Ok(tables) => tables,
            Err(error) => return self.refuse(error.to_string()),
        };
        match host
            .assets
            .gam("data/vault13.gam", vault_formats::GamSection::Campaign)
        {
            Ok(Some(_)) => {}
            Ok(None) => return self.refuse("data/vault13.gam did not open".into()),
            Err(error) => return self.refuse(format!("data/vault13.gam: {error}")),
        }
        self.fallout2 = Some(tables);
        self.last_error = None;
        true
    }
    fn refuse(&mut self, error: String) -> bool {
        tracing::error!(%error, "the Fallout 2 game data was refused");
        self.last_error = Some(error);
        self.enter_menu();
        false
    }
    fn world_with(&self, sheet: CharacterSheet) -> AppResult<MapPreview> {
        let assets = VfsWorldAssets::new(self.vfs.clone(), &self.language)?;
        let fallout2 = self.fallout2.clone();
        let path = if fallout2.is_some() {
            vault_core::fallout2::START_MAP
        } else {
            flow::NEW_GAME_MAP
        };
        // SPEC-018 §6.6 step 3: the Fallout 2 world state is installed after
        // the reset and before the start map's load.
        let mut world =
            MapPreview::with_setup(assets, path, sheet, self.seed, self.preset, |host| {
                if let Some(tables) = fallout2 {
                    host.begin_fallout2_game(tables);
                }
            })?;
        world.host.world.clock.ticks = flow::START_TICKS;
        world.audio = self.audio.clone();
        world.save_root = self.save_root.clone();
        world.screenshot_dir = self.screenshot_dir.clone();
        world.config_path = self.config_path.clone();
        world.preferences = self.preferences.clone();
        world.interrupt_walk = self.interrupt_walk;
        // SPEC-008 §3.10: the world starts with the mixer's music state,
        // which lasts the whole run; the new game's preferences then set
        // the volumes as any game reset does.
        world.host.set_music_enabled(self.audio.music_enabled());
        world.apply_preferences();
        world.set_slot_session(self.slot_list.clone(), self.quick);
        world.host.critter_cycle = self.critter_cycle;
        world.host.world.dialogue.idle = self.head_idle;
        // G-09: the map preview tool's keys need `--debug` here, and no key
        // turns the roofs off in the game (SPEC-014 §3.36).
        world.preview_keys = self.debug_keys;
        world.roof_key = false;
        // The session's options, as a map preview takes them.
        crate::session::WorldOptions {
            headless: self.headless,
            debug: self.debug,
            enhanced: self.enhanced,
            touch_ui: self.touch_ui,
            mod_export: self.mod_export.clone(),
            debug_session: self.debug_keys,
        }
        .apply(&mut world, |_| {});
        world.init_game_speed(self.game_speed);
        Ok(world)
    }
    /// The game speed for this world and the next ones.
    pub fn set_game_speed(&mut self, speed: crate::enhanced::GameSpeed) {
        self.game_speed = speed;
        if let Some(world) = self.world() {
            world.set_game_speed(speed);
        }
    }
    fn premade(&self, index: usize) -> CharacterSheet {
        self.assets
            .gcd(flow::PREMADES[index].sheet)
            .map(|gcd| CharacterSheet::from_gcd(&gcd, StatContext::default()))
            .unwrap_or_default()
    }
    fn menu_choice(&mut self, choice: MenuChoice) {
        match choice {
            MenuChoice::Intro => {
                self.play_movie("art/cuts/intro.mve", flow::ATTRACT_INTRO_FLAGS, Next::Menu)
            }
            MenuChoice::NewGame => {
                if !self.prepare_fallout2() {
                    return;
                }
                self.selection = 0;
                self.sheet = self.premade(0);
                self.stage = Stage::Selection;
            }
            MenuChoice::LoadGame if !self.prepare_fallout2() => {}
            MenuChoice::LoadGame => match self.world_with(CharacterSheet::default()) {
                Ok(mut world) => {
                    world.open_save_screen(Mode::LoadFromMenu);
                    self.stage = Stage::Load(Box::new(world));
                }
                Err(error) => tracing::error!(%error, "load screen failed"),
            },
            MenuChoice::Credits => self.enter_credits("credits.txt", CreditStyle::Normal),
            MenuChoice::Quotes if self.preferences.language_filter == 0 => {
                self.enter_credits("quotes.txt", CreditStyle::Reversed)
            }
            MenuChoice::Quotes => {}
            // SPEC-019 §3.2: Fallout 2's Options opens the preferences
            // directly, without the options menu; closing them (or a
            // confirmed quit) returns here.
            MenuChoice::Options => {
                if !self.prepare_fallout2() {
                    return;
                }
                match self.world_with(CharacterSheet::default()) {
                    Ok(mut world) => {
                        world.open_preferences_from_menu();
                        self.stage = Stage::Load(Box::new(world));
                    }
                    Err(error) => tracing::error!(%error, "preferences failed"),
                }
            }
            // §3.5: Exit stops the music.
            MenuChoice::Exit => {
                self.audio.stop_music();
                self.finished = true;
            }
            MenuChoice::Timeout => match self.attract.trigger(0) {
                flow::AttractAction::Intro | flow::AttractAction::Demo(_) => {
                    self.play_movie("art/cuts/intro.mve", flow::ATTRACT_INTRO_FLAGS, Next::Menu)
                }
            },
        }
    }
    /// The session rules of the game's edition (SPEC-020 §2.2 rule 3):
    /// every edition branch outside the world reads them.
    pub fn rules(&self) -> EditionRules {
        EditionRules::of(self.game_profile)
    }
    fn menu_poll(&mut self, input: MenuInput) {
        // SPEC-019 §3.2: Fallout 2's six buttons (keys only reach here).
        if self.rules().six_button_menu {
            let input = match input {
                MenuInput::None => ui2::MenuInput::None,
                MenuInput::Key(key) => ui2::MenuInput::Key(key),
                MenuInput::Escape => ui2::MenuInput::Escape,
                MenuInput::CtrlR => ui2::MenuInput::CtrlR,
                MenuInput::Press(_) | MenuInput::Release(_) => return,
            };
            self.menu_poll2(input);
            return;
        }
        let Stage::Menu { since } = self.stage else {
            return;
        };
        let poll = flow::menu_poll(input, self.modifiers[0], self.now.saturating_sub(since));
        for sound in poll.sounds {
            self.audio_cue(sound);
        }
        if poll.brightness != 0 {
            self.brightness_step(poll.brightness > 0);
        }
        if let Some(choice) = poll.choice {
            self.menu_choice(choice);
        }
    }
    /// SPEC-019 §3.2 one poll of Fallout 2's menu.
    fn menu_poll2(&mut self, input: ui2::MenuInput) {
        let Stage::Menu { since } = self.stage else {
            return;
        };
        let poll = ui2::menu_poll(input, self.modifiers[0], self.now.saturating_sub(since));
        for sound in poll.sounds {
            self.audio_cue(sound);
        }
        if poll.brightness != 0 {
            self.brightness_step(poll.brightness > 0);
        }
        if let Some(choice) = poll.choice {
            self.menu_choice(choice);
        }
    }
    /// An interface sound, `sound/sfx/<name>.acm`, at the effects volume
    /// (SPEC-010 §3.13: an effect without an object has level
    /// `sndfx_volume`).
    fn audio_cue(&mut self, name: &str) {
        let path = format!("sound/sfx/{name}.acm");
        self.sync_volumes();
        if let Some(samples) = self.sounds.load(&*self.vfs, &path) {
            self.audio.play(
                samples,
                self.preferences.sndfx_volume,
                Channel::Effect,
                false,
            );
        }
    }
    /// Take the selected (or edited) character into the game: the
    /// `ovrintro` movie, then the world.
    fn start_game(&mut self, sheet: CharacterSheet) {
        // SPEC-018 §6.4: Fallout 2's hand-off plays `elder.mve`.
        let (path, flags) = if self.fallout2.is_some() {
            vault_core::fallout2::NEW_GAME_MOVIE
        } else {
            flow::NEW_GAME_MOVIE
        };
        self.play_movie(path, flags, Next::NewGame(Box::new(sheet)));
    }
    fn selection_action(&mut self, action: SelectionAction) {
        let context = StatContext::default();
        // SPEC-014 §3.43: the sheet files live in the master patch root,
        // in the game's sheet layout (SPEC-018 §6.3).
        let files = || {
            crate::editor_screen::SheetFiles::new(
                self.vfs.clone(),
                &self.language,
                self.save_root.clone(),
                self.game_profile,
            )
        };
        match action {
            SelectionAction::Take => self.start_game(self.sheet.clone()),
            SelectionAction::Modify => {
                self.stage = Stage::Editor(Box::new(
                    EditorStage::new(Editor::open(self.sheet.clone(), false, context))
                        .with_files(files()),
                ))
            }
            SelectionAction::Create => {
                self.stage = Stage::Editor(Box::new(
                    EditorStage::new(Editor::open(editor::blank_sheet(context), true, context))
                        .with_files(files()),
                ))
            }
            SelectionAction::Back => self.enter_menu(),
            SelectionAction::Previous | SelectionAction::Next => {
                let step = if action == SelectionAction::Next {
                    1
                } else {
                    -1
                };
                self.selection = flow::cycle(self.selection, step, flow::PREMADES.len());
                self.sheet = self.premade(self.selection);
            }
            SelectionAction::QuitConfirm => self.enter_menu(),
            SelectionAction::Brightness(step) => self.brightness_step(step > 0),
        }
    }

    /// The palette for the current stage.
    pub fn palette(&self) -> &Palette {
        match &self.stage {
            Stage::Splash { palette, .. }
            | Stage::Death { palette, .. }
            | Stage::Slides { palette, .. } => palette,
            Stage::World(world) | Stage::Load(world) | Stage::Ending { world, .. } => world
                .host
                .library
                .palette
                .as_deref()
                .unwrap_or(&world.palette),
            _ => &self.palette,
        }
    }
    pub fn display_override(&self) -> Option<[[u8; 4]; 256]> {
        match &self.stage {
            Stage::Movie { player, .. } => Some(player.palette(self.preferences.brightness)),
            Stage::World(world) | Stage::Load(world) => world.display_override(),
            _ if self.fade.is_some() || self.shown.is_some() => {
                let gamma = self.brightness();
                Some(
                    self.shown_levels()
                        .map(|rgb6| vault_formats::palette::display_rgb6(rgb6, gamma)),
                )
            }
            _ => None,
        }
    }
    pub fn palette_cycling(&self) -> bool {
        match &self.stage {
            Stage::World(world) => world.palette_cycling(),
            _ => false,
        }
    }
    /// Changes whenever the stage's palette does (the shell rebuilds its
    /// display entries then).
    pub fn palette_generation(&self) -> u32 {
        match &self.stage {
            Stage::World(world) | Stage::Load(world) | Stage::Ending { world, .. } => {
                world.host.library.palette_generation
            }
            Stage::Splash { .. } => 1 << 24,
            Stage::Death { .. } => 2 << 24,
            Stage::Slides { index, .. } => (3 << 24) + *index as u32,
            _ => 4 << 24,
        }
    }
    pub fn world(&mut self) -> Option<&mut MapPreview> {
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => Some(world),
            _ => None,
        }
    }
    /// The window title: the product name in play, and the stage's
    /// diagnostic [`Game::title`] with `--debug`.
    pub fn window_title(&self) -> String {
        if self.debug_keys {
            self.title()
        } else {
            play_title(self.preset == vault_core::EnginePreset::Enhanced).into()
        }
    }
    /// The stage's diagnostic title.
    pub fn title(&self) -> String {
        match &self.stage {
            Stage::World(world) => world.title(),
            Stage::Menu { .. } => "Vault — main menu".into(),
            Stage::Selection => {
                "Vault — character selection — T take, M modify, C create, Escape back".into()
            }
            Stage::Editor(_) => "Vault — character editor — Enter done, Escape cancel".into(),
            Stage::Credits { .. } => "Vault — credits".into(),
            _ => "Vault".into(),
        }
    }

    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        let before = std::mem::replace(&mut self.now, now);
        if !matches!(self.stage, Stage::World(_) | Stage::Load(_)) {
            self.sync_volumes();
        }
        let faded = self.fade_tick();
        let fading = self.fade.is_some();
        match &mut self.stage {
            // The end-of-session fade to white blocks; then the map goes.
            Stage::Ending { death, .. } => {
                if !faded {
                    return Ok(fading);
                }
                let death = *death;
                let Stage::Ending { mut world, .. } =
                    std::mem::replace(&mut self.stage, Stage::Menu { since: now })
                else {
                    unreachable!()
                };
                self.leave_world(&world);
                // §3.5 step 3's D(1, 4), from the game's generator before
                // the reset discards it.
                let draw = death.then(|| {
                    vault_core::RandomSource::range(&mut world.host.world.rng, 1, 4).unwrap_or(1)
                });
                drop(world);
                if let Some(draw) = draw {
                    self.enter_death(draw);
                } else {
                    self.enter_menu();
                }
                Ok(true)
            }
            // P-02: the splash is still; only its first poll and the start
            // of the movies redraw.
            Stage::Splash { since, .. } => {
                if *since == 0 {
                    *since = now.max(1);
                } else if now >= *since + 1000 {
                    self.start_movies();
                } else {
                    return Ok(faded || fading);
                }
                Ok(true)
            }
            Stage::Movie { player, .. } => {
                let changed = player.tick(now);
                if player.finished() {
                    self.movie_done();
                    return Ok(true);
                }
                Ok(changed)
            }
            Stage::Menu { .. } => {
                // §3.29: the top of each main-menu loop pass clears the
                // keyboard, cancelling every key repeat.
                self.menu_passes = self.menu_passes.wrapping_add(1);
                self.menu_poll(MenuInput::None);
                Ok(faded || fading || self.held_moved())
            }
            Stage::Credits { scroll, last, .. } => {
                let mut changed = false;
                while now >= *last + endgame::CREDIT_TICK_MS {
                    *last += endgame::CREDIT_TICK_MS;
                    scroll.advance();
                    changed = true;
                }
                if scroll.finished() {
                    self.enter_menu();
                }
                Ok(changed)
            }
            // §3.5: after the fade from white, the narration plays; the
            // scene waits for input or the end of the speech (3000 ms when
            // it did not start), then 500 ms more unless input ended it,
            // then fades to black; then the menu fades in.
            Stage::Death {
                since,
                leaving,
                draw,
                narration,
                input,
                tail,
                ..
            } => {
                if *leaving {
                    if faded {
                        self.enter_menu();
                    }
                    return Ok(true);
                }
                if fading {
                    return Ok(true);
                }
                let leave = match *narration {
                    None => {
                        *since = now;
                        let path = flow::death_narration(*draw);
                        // SPEC-010 §3.13 narration: trunc(0.69 x speech).
                        let speech = self.preferences.speech_volume;
                        let started = !*input
                            && self
                                .sounds
                                .load(&*self.vfs, &path)
                                .is_some_and(|samples| self.audio.play_narration(samples, speech));
                        *narration = Some(started);
                        *input
                    }
                    Some(speaking) => {
                        if tail.is_none()
                            && (speaking && !self.audio.playing(Channel::Speech)
                                || !speaking && now >= *since + flow::DEATH_SILENT_MS)
                        {
                            *tail = Some(now);
                        }
                        tail.is_some_and(|tail| now >= tail + flow::DEATH_TAIL_MS)
                    }
                };
                if leave {
                    self.leave_death();
                }
                Ok(faded || leave)
            }
            Stage::Slides {
                since,
                slides,
                index,
                ..
            } => {
                // A still slide: the pauses and the silent timeout; a desert
                // pan: one column per step across D = 640.
                let pan = slides[*index].pan.is_some();
                let length = if pan {
                    endgame::pan_step_ms(None) * endgame::PAN_DISTANCE as u64
                } else {
                    endgame::SLIDE_SILENT_MS
                } + 2 * endgame::SLIDE_PAUSE_MS;
                // P-02: a slide redraws when it changes or its pan moves.
                let step = |time: u64| time.saturating_sub(*since) / endgame::PAN_STEP_MS;
                let moved = pan && (before < *since || step(before) != step(now));
                if now >= *since + length {
                    self.next_slide();
                    return Ok(true);
                }
                Ok(moved || faded || fading)
            }
            Stage::World(world) => {
                let changed = world.tick(now)?;
                self.world_events();
                Ok(changed || faded || fading)
            }
            Stage::Load(world) => {
                let changed = world.tick(now)?;
                if !world.load_screen_open() && !world.menu_preferences_open() {
                    // SPEC-019 §3.2: preferences set from the menu stay.
                    if world.opened_for_preferences() {
                        self.preferences = world.preferences.clone();
                    }
                    let Stage::Load(world) =
                        std::mem::replace(&mut self.stage, Stage::Menu { since: now })
                    else {
                        unreachable!()
                    };
                    if world.loaded {
                        self.stage = Stage::World(world);
                    } else {
                        self.enter_menu();
                    }
                }
                Ok(changed)
            }
            Stage::Editor(stage) => {
                // §3.9: an accepted quit confirmation cancels on the next
                // frame and returns through the selection screen to the
                // menu.
                if stage.quitting {
                    self.editor_exit(EditorExit::Cancel);
                    self.enter_menu();
                    return Ok(true);
                }
                let stepped = stage.tick(now, self.pointer);
                self.editor_sounds();
                Ok(stepped || self.held_moved())
            }
            Stage::Selection => Ok(self.held_moved()),
        }
    }
    /// The editor's interface sounds.
    fn editor_sounds(&mut self) {
        let Stage::Editor(stage) = &mut self.stage else {
            return;
        };
        for sound in std::mem::take(&mut stage.sounds) {
            self.audio_cue(sound);
        }
    }
    /// An editor result: the new game, or back to the selection screen.
    fn editor_exit(&mut self, exit: EditorExit) -> bool {
        self.editor_sounds();
        match exit {
            EditorExit::Stay(handled) => handled,
            EditorExit::Done(sheet) => {
                self.start_game(*sheet);
                true
            }
            EditorExit::Cancel => {
                self.sheet = self.premade(self.selection);
                self.stage = Stage::Selection;
                true
            }
        }
    }
    /// SPEC-014 §3.19 F12 outside a world: the screen recomposed, written
    /// as the §2.5 bitmap under the installed palette before brightness
    /// into the working directory. These screens have no message monitor,
    /// so the result is only logged. (The menu and editor use the system
    /// pointer, which is not part of the composed screen.)
    fn screenshot(&mut self) {
        let image = match self.image() {
            Ok(image) => image,
            Err(error) => {
                tracing::warn!(%error, "screenshot not saved");
                return;
            }
        };
        let palette = match &self.stage {
            Stage::Movie { player, .. } => player.palette(1.0),
            Stage::World(world) | Stage::Load(world) | Stage::Ending { world, .. } => {
                world.installed_palette()
            }
            _ if self.fade.is_some() || self.shown.is_some() => self
                .shown_levels()
                .map(|rgb6| vault_formats::palette::display_rgb6(rgb6, 1.0)),
            _ => self.palette().display_rgba8(1.0),
        };
        if let Err(error) =
            crate::interface::save_screenshot(&self.screenshot_dir, &image, &palette)
        {
            tracing::warn!(%error, "screenshot not saved");
        }
    }
    /// The held button, while the pointer is over it.
    fn held_button(&self) -> Option<Rect> {
        self.held
            .filter(|rect| self.pointer.is_some_and(|at| rect.contains(at)))
    }
    /// The pointer moved onto or off the held button since the last frame.
    fn held_moved(&self) -> bool {
        self.held_button().is_some() != self.held_shown
    }
    fn movie_done(&mut self) {
        let Stage::Movie { next, .. } =
            std::mem::replace(&mut self.stage, Stage::Menu { since: self.now })
        else {
            return;
        };
        if self.movies.is_empty() {
            self.continue_with(next);
        } else {
            self.next_movie_or(next);
        }
    }
    /// The world's quit, death and endgame requests.
    /// §3.5: the session ends on a quit request, or when the player carries
    /// the dead or knocked-out status at the end of a main-loop pass. Combat
    /// runs inside one pass, so the status check waits for it to end (a
    /// knockout in combat is normally recovered by its cleanup).
    fn world_events(&mut self) {
        let Stage::World(world) = &mut self.stage else {
            return;
        };
        let player = world.host.world.player;
        let state = &world.host.world;
        let over = !state.combat
            && (state.is_dead(player)
                || state
                    .critter_state(player)
                    .is_some_and(|critter| critter.results.0 & 0x01 != 0));
        if world.quit_requested || over {
            let death = !world.quit_requested;
            let Stage::World(world) =
                std::mem::replace(&mut self.stage, Stage::Menu { since: self.now })
            else {
                return;
            };
            self.stage = Stage::Ending { world, death };
            self.fade_to(WHITE);
            return;
        }
        match world.endgame.take() {
            Some(WorldEffect::Slideshow) => {
                let globals = world.host.world.gvars.clone().unwrap_or_default();
                // SPEC-019 §3.20: Fallout 2's slides are `endgame.txt`'s
                // records whose global equals their value, in table order.
                let slides: Vec<Shown> = match world.host.world.fallout2.as_deref() {
                    Some(state) => {
                        let painter = Painter {
                            assets: &world.host.assets,
                            palette: &world.palette,
                        };
                        let mut notes = Vec::new();
                        let slides = ui2::endgame_slides(
                            &state.tables.endgame,
                            &globals,
                            |row| painter.interface_row(row),
                            &mut notes,
                        );
                        for note in notes {
                            tracing::warn!(%note, "endgame slide");
                        }
                        slides.into_iter().map(Shown::from).collect()
                    }
                    None => endgame::slides(&globals)
                        .into_iter()
                        .map(Shown::from)
                        .collect(),
                };
                let Stage::World(world) =
                    std::mem::replace(&mut self.stage, Stage::Menu { since: self.now })
                else {
                    return;
                };
                self.stage_slide(slides, 0, world);
            }
            // SPEC-019 §3.20: Fallout 2 has no walk movies; the ending goes
            // to the credits.
            Some(_) if world.host.world.fallout2.is_some() => {
                self.continue_with(Next::Credits);
            }
            Some(_) => {
                // §3.31 the ending: walkm or walkw by gender, then credits.
                let female = world.host.world.player_stat(34) == 1;
                let movie = if female {
                    "art/cuts/walkw.mve"
                } else {
                    "art/cuts/walkm.mve"
                };
                self.play_movie(movie, 0, Next::Credits);
            }
            None => {}
        }
    }
    /// §3.5 the death scene: `death.frm` over the white screen the session's
    /// end left, fading from white to `death.pal`; `draw` is D(1, 4).
    fn enter_death(&mut self, draw: i32) {
        let palette = self
            .assets
            .palette(flow::DEATH_PALETTE.as_bytes())
            .unwrap_or_else(|_| black());
        let target = levels(&palette);
        self.shown = Some(WHITE);
        self.stage = Stage::Death {
            since: self.now,
            palette,
            leaving: false,
            draw,
            narration: None,
            input: false,
            tail: None,
        };
        self.fade_to(target);
    }
    /// Steps 5-6: stop the speech and fade to black; the game palette is
    /// then loaded without a fade, so the screen stays black until the menu
    /// fades in.
    fn leave_death(&mut self) {
        if let Stage::Death { leaving, .. } = &mut self.stage
            && !*leaving
        {
            *leaving = true;
            self.audio.stop(Channel::Speech);
            self.fade_to(flow::BLACK);
        }
    }
    /// §3.5 step 4: input ends the narration's wait, without the 500 ms
    /// tail; input during the fade from white is seen when the wait
    /// begins.
    fn death_input(&mut self) {
        match &mut self.stage {
            Stage::Death {
                narration: None,
                input,
                ..
            } => *input = true,
            Stage::Death { .. } => self.leave_death(),
            _ => {}
        }
    }
    /// Show slide `index`, or return to the world after the last (§3.30
    /// exit: map processing resumes). Global 299 is cleared after the
    /// sequence (§3.30), not before it.
    fn stage_slide(&mut self, slides: Vec<Shown>, index: usize, mut world: Box<MapPreview>) {
        if index >= slides.len() {
            // SPEC-019 §3.20: no global is written under Fallout 2.
            if world.host.world.fallout2.is_none()
                && let Some(value) = world
                    .host
                    .world
                    .gvars
                    .as_mut()
                    .and_then(|g| g.get_mut(endgame::DONE_GLOBAL))
            {
                *value = 0;
            }
            self.stage = Stage::World(world);
            return;
        }
        let palette = self
            .assets
            .palette(slides[index].palette.as_bytes())
            .unwrap_or_else(|_| self.palette.clone());
        self.stage = Stage::Slides {
            slides,
            index,
            since: self.now,
            palette,
            world,
        };
    }
    fn next_slide(&mut self) {
        let Stage::Slides {
            slides,
            index,
            world,
            ..
        } = std::mem::replace(&mut self.stage, Stage::Menu { since: self.now })
        else {
            return;
        };
        self.stage_slide(slides, index + 1, world);
    }

    pub fn image(&mut self) -> AppResult<IndexedImage> {
        if matches!(self.stage, Stage::Selection)
            && self
                .bio
                .as_ref()
                .is_none_or(|(selection, _)| *selection != self.selection)
        {
            let bio = self
                .vfs
                .read_bounded(flow::PREMADES[self.selection].bio, MAX_ASSET_BYTES)
                .unwrap_or_default();
            self.bio = Some((self.selection, bio));
        }
        let held = self.held_button();
        self.held_shown = held.is_some();
        let art = |rect: Rect, (up, down): (&'static str, &'static str)| {
            if held == Some(rect) { down } else { up }
        };
        let painter = Painter {
            assets: &self.assets,
            palette: &self.palette,
        };
        let mut image = IndexedImage::new(640, 480)?;
        let rules = self.rules();
        match &mut self.stage {
            Stage::Splash { image: splash, .. } => return Ok(splash.clone()),
            Stage::Movie { player, .. } => return Ok(player.image()),
            Stage::World(world) | Stage::Load(world) | Stage::Ending { world, .. } => {
                return world.image();
            }
            Stage::Menu { .. } if rules.six_button_menu => {
                draw_menu2(&painter, &mut image, held);
            }
            Stage::Menu { .. } => {
                painter.blit_opaque(&mut image, flow::MENU_BACKGROUND, 0, [0, 0]);
                for button in flow::MENU_BUTTONS {
                    let rect = button.rect();
                    let up = art(rect, flow::MENU_BUTTON_ART);
                    painter.blit(&mut image, up, 0, [rect.x, rect.y]);
                    let label = painter.message("misc.msg", button.label());
                    let origin = button.label_origin(painter.width(104, &label));
                    painter.text(&mut image, 104, flow::MENU_LABEL_COLOR, &label, origin, 200);
                }
                let copyright = painter.message("misc.msg", 14);
                painter.text(
                    &mut image,
                    100,
                    flow::MENU_TEXT_COLOR,
                    &copyright,
                    flow::MENU_COPYRIGHT,
                    625,
                );
                let version = flow::MENU_VERSION.as_bytes();
                let [right, y] = flow::MENU_VERSION_RIGHT;
                painter.text(
                    &mut image,
                    100,
                    flow::MENU_TEXT_COLOR,
                    version,
                    [right - painter.width(100, version), y],
                    200,
                );
            }
            Stage::Credits { scroll, style, .. } => {
                for (row, y) in scroll.visible() {
                    let line = &scroll.rows[row];
                    let (font, key) = style.font(line.kind);
                    painter.centered(&mut image, font, key, &line.text, [0, y], 640);
                }
                if let Some(frame) = scroll.egg_frame() {
                    let (art, origin, _) = endgame::EGG_ART;
                    painter.blit(&mut image, art, frame as usize, origin);
                }
            }
            Stage::Selection => self.draw_selection(&painter, &mut image, held),
            Stage::Editor(stage) => draw_editor(&painter, &mut image, stage, held),
            Stage::Death { .. } => {
                painter.blit_opaque(&mut image, flow::DEATH_ART, 0, [0, 0]);
            }
            Stage::Slides {
                slides,
                index,
                since,
                ..
            } => {
                let slide = &slides[*index];
                let offset = match slide.pan {
                    Some(right) => {
                        let step = (self.now.saturating_sub(*since) / endgame::PAN_STEP_MS) as i32;
                        let position = step.min(endgame::PAN_DISTANCE);
                        if right {
                            position
                        } else {
                            endgame::PAN_DISTANCE - position
                        }
                    }
                    None => 0,
                };
                if let Some(art) = painter.art(&slide.art)
                    && let Some(frame) = art.frame(0, 0)
                {
                    image.blit(frame, [-i64::from(offset), 0]);
                }
            }
        }
        Ok(image)
    }
    fn draw_selection(&self, painter: &Painter, image: &mut IndexedImage, held: Option<Rect>) {
        painter.blit_opaque(image, flow::SELECTION_BACKGROUND, 0, [0, 0]);
        let premade = flow::PREMADES[self.selection];
        // The portrait with every odd row left out (the scan-line effect),
        // clipped to the monitor window (SPEC-019 §3.3 places Fallout 2's).
        if let Some(art) = painter.art(premade.portrait)
            && let Some(frame) = art.frame(0, 0)
        {
            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
            let in_screen = self.rules().portrait_in_screen;
            let ([x, y], window) = flow::portrait_placement(in_screen, w, h);
            for row in (0..h).step_by(2) {
                for column in 0..w {
                    let color = frame.pixels()[(row * w + column) as usize];
                    let (dx, dy) = (x + column, y + row);
                    if color != 0 && window.contains([dx, dy]) {
                        image.pixels_mut()[(dy * 640 + dx) as usize] = color;
                    }
                }
            }
        }
        let green = flow::SELECTION_TEXT;
        // SPEC-019 §3.3: no ID text under Fallout 2.
        if self.rules().premade_id {
            painter.centered(
                image,
                101,
                green,
                premade.id.as_bytes(),
                [150 - 100, 252],
                200,
            );
        }
        painter.centered(image, 101, green, &self.sheet.name, [318 - 80, 40], 160);
        let context = StatContext::default();
        for k in 0..7 {
            let name = painter.message("stat.msg", 100 + k as i32);
            let value = self.sheet.stat(k, context);
            let text = flow::special_row(&name, value);
            let y = flow::special_row_y(k);
            painter.text(
                image,
                101,
                green,
                &text,
                [flow::SPECIAL_RIGHT - painter.width(101, &text), y],
                200,
            );
            let word = painter.message("stat.msg", flow::level_word(value));
            let mut word_text = b"  ".to_vec();
            word_text.extend(word);
            painter.text(image, 101, green, &word_text, [flow::SPECIAL_RIGHT, y], 100);
        }
        let hp = format!(" {}/{}", self.sheet.current_hp, self.sheet.stat(7, context)).into_bytes();
        for (y, label, value) in [
            (150, painter.message("misc.msg", 16), hp),
            (
                160,
                painter.message("stat.msg", 109),
                format!(" {}", self.sheet.stat(9, context)).into_bytes(),
            ),
            (
                170,
                painter.message("misc.msg", 15),
                format!(" {}", self.sheet.stat(8, context)).into_bytes(),
            ),
            (
                180,
                painter.message("stat.msg", 111),
                format!(" {}", self.sheet.stat(11, context)).into_bytes(),
            ),
        ] {
            painter.text(
                image,
                101,
                green,
                &label,
                [365 - painter.width(101, &label), y],
                200,
            );
            painter.text(image, 101, green, &value, [365, y], 100);
        }
        for (row, tag) in self.sheet.tags.iter().take(3).enumerate() {
            if *tag < 0 {
                continue;
            }
            let name = painter.message("skill.msg", 100 + tag);
            let y = 200 + 10 * row as i32;
            painter.text(
                image,
                101,
                green,
                &name,
                [365 - painter.width(101, &name), y],
                200,
            );
            let value = format!(" {}%", self.sheet.skill(*tag as usize, context)).into_bytes();
            painter.text(image, 101, green, &value, [365, y], 100);
        }
        for (row, value) in self.sheet.traits.iter().enumerate() {
            if *value < 0 {
                continue;
            }
            let name = painter.message("trait.msg", 100 + value);
            painter.text(
                image,
                101,
                green,
                &name,
                [365 - painter.width(101, &name), 230 + 10 * row as i32],
                200,
            );
        }
        if let Some((_, bio)) = &self.bio {
            for (index, line) in flow::bio_lines(bio).iter().enumerate() {
                if let Some(y) = flow::bio_line_y(index) {
                    painter.text(
                        image,
                        101,
                        green,
                        line,
                        [flow::BIO_ORIGIN[0], y],
                        flow::BIO_WIDTH,
                    );
                }
            }
        }
        let art = |rect: Rect, up, down| if held == Some(rect) { down } else { up };
        for origin in flow::SELECTION_BUTTONS {
            let rect = red_button(origin);
            painter.blit(image, art(rect, "lilredup.frm", "lilreddn.frm"), 0, origin);
        }
        for (rect, up, down) in [
            (flow::SELECTION_PREVIOUS, "slu.frm", "sld.frm"),
            (flow::SELECTION_NEXT, "sru.frm", "srd.frm"),
        ] {
            painter.blit(image, art(rect, up, down), 0, [rect.x, rect.y]);
        }
    }

    /// A key went up: only the world holds releases (SPEC-014 §3.27).
    pub fn key_up(&mut self, key: KeyCode) {
        if let Stage::World(world) | Stage::Load(world) = &mut self.stage {
            world.key_up(key);
        }
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        // SPEC-014 §3.19: F12 is caught as the key enters the common input
        // queue, so no screen sees it; a world catches it itself (it holds
        // keys while combat disables the keyboard).
        if key == KeyCode::F12 && !matches!(self.stage, Stage::World(_) | Stage::Load(_)) {
            self.screenshot();
            return true;
        }
        // SPEC-014 §3.29: a modifier or lock key alone translates to no key
        // code, so no screen reads it and no "any key" test is met; a world
        // holds it like any key while combat disables the keyboard.
        if crate::input::is_modifier(key) && !matches!(self.stage, Stage::World(_) | Stage::Load(_))
        {
            return false;
        }
        match &mut self.stage {
            Stage::Splash { .. } => {
                self.start_movies();
                true
            }
            Stage::Movie { player, .. } => {
                player.key();
                self.movie_done();
                true
            }
            Stage::World(world) | Stage::Load(world) => world.step(key),
            Stage::Menu { .. } => {
                let input = match key {
                    KeyCode::Escape => MenuInput::Escape,
                    KeyCode::KeyR if self.modifiers[1] => MenuInput::CtrlR,
                    KeyCode::Equal => MenuInput::Key('='),
                    KeyCode::Minus => MenuInput::Key('-'),
                    _ => match letter(key) {
                        Some(c) => MenuInput::Key(if self.modifiers[0] {
                            c.to_ascii_uppercase()
                        } else {
                            c
                        }),
                        None => return false,
                    },
                };
                self.menu_poll(input);
                true
            }
            Stage::Credits { scroll, .. } => {
                let input = letter(key).map_or(CreditInput::Other, CreditInput::Char);
                if scroll.input(input, self.preferences.violence_level) {
                    self.enter_menu();
                }
                true
            }
            Stage::Selection => {
                let selection = match key {
                    KeyCode::Escape => SelectionKey::Escape,
                    KeyCode::ArrowLeft => SelectionKey::Left,
                    KeyCode::ArrowRight => SelectionKey::Right,
                    KeyCode::F10 => SelectionKey::F10,
                    KeyCode::Equal => SelectionKey::Char('='),
                    KeyCode::Minus => SelectionKey::Char('-'),
                    _ => match letter(key) {
                        Some(c) => SelectionKey::Char(c),
                        None => return false,
                    },
                };
                match flow::selection_key(selection) {
                    Some(action) => {
                        self.selection_action(action);
                        true
                    }
                    None => false,
                }
            }
            Stage::Editor(stage) => {
                let painter = Painter {
                    assets: &self.assets,
                    palette: &self.palette,
                };
                if stage.quit_key(key, self.modifiers[1], || painter.message("misc.msg", 0)) {
                    return true;
                }
                let exit = stage.key(key, self.modifiers[0]);
                self.editor_exit(exit)
            }
            Stage::Death { .. } => {
                self.death_input();
                true
            }
            Stage::Ending { .. } => true,
            Stage::Slides { .. } => {
                self.next_slide();
                true
            }
        }
    }
    pub fn click(&mut self, point: [i32; 2], right: bool) -> bool {
        // G-31: the menu, selection and editor buttons take the left button
        // only.
        if right
            && matches!(
                self.stage,
                Stage::Menu { .. } | Stage::Selection | Stage::Editor(_)
            )
        {
            return false;
        }
        let rules = self.rules();
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.click(point, right),
            Stage::Movie { player, .. } => {
                player.press();
                true
            }
            // §3.4: a press plays `nmselec0`; the release over the button
            // selects it.
            // §3.3 only the nonzero pixels of the up image are clickable.
            Stage::Menu { .. } if rules.six_button_menu => {
                let painter = Painter {
                    assets: &self.assets,
                    palette: &self.palette,
                };
                let mask = painter.art(flow::MENU_BUTTON_ART.0);
                let pressed = ui2::menu_button_at(point).filter(|i| {
                    let rect = ui2::menu_button(*i);
                    mask.as_ref()
                        .and_then(|art| art.frame(0, 0))
                        .is_none_or(|frame| {
                            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
                            let (x, y) = (point[0] - rect.x, point[1] - rect.y);
                            (0..w).contains(&x)
                                && (0..h).contains(&y)
                                && frame.pixels()[(y * w + x) as usize] != 0
                        })
                });
                if let Some(i) = pressed {
                    self.held = Some(ui2::menu_button(i));
                    self.menu_poll2(ui2::MenuInput::Press(i));
                }
                true
            }
            Stage::Menu { .. } => {
                let painter = Painter {
                    assets: &self.assets,
                    palette: &self.palette,
                };
                let mask = painter.art(flow::MENU_BUTTON_ART.0);
                let opaque = |rect: Rect| {
                    rect.contains(point)
                        && mask
                            .as_ref()
                            .and_then(|art| art.frame(0, 0))
                            .is_none_or(|frame| {
                                let (w, h) = (i32::from(frame.width), i32::from(frame.height));
                                let (x, y) = (point[0] - rect.x, point[1] - rect.y);
                                (0..w).contains(&x)
                                    && (0..h).contains(&y)
                                    && frame.pixels()[(y * w + x) as usize] != 0
                            })
                };
                if let Some(button) = flow::MENU_BUTTONS.into_iter().find(|b| opaque(b.rect())) {
                    self.held = Some(button.rect());
                    self.menu_poll(MenuInput::Press(button));
                }
                true
            }
            // §3.7 the red buttons (`ib1p1xx1` / `ib1lu1x1`) and the arrows
            // (`ib2p1xx1` / `ib2lu1x1`) press here and act on the release.
            Stage::Selection => {
                if let Some((rect, _)) = selection_button(point) {
                    let sound = if rect.w == 15 {
                        vault_core::ui::BUTTON_PRESS
                    } else {
                        "ib2p1xx1"
                    };
                    self.held = Some(rect);
                    self.audio_cue(sound);
                }
                true
            }
            Stage::Editor(stage) => {
                let painter = Painter {
                    assets: &self.assets,
                    palette: &self.palette,
                };
                self.held = stage.press(&painter, point, self.now);
                self.editor_sounds();
                true
            }
            Stage::Credits { scroll, .. } => {
                scroll.input(CreditInput::Other, self.preferences.violence_level);
                self.enter_menu();
                true
            }
            Stage::Splash { .. } | Stage::Death { .. } | Stage::Slides { .. } => {
                self.step(KeyCode::Space)
            }
            Stage::Ending { .. } => true,
        }
    }
    /// SPEC-014 §3.29 the keyboard for the engine key repeat: a world's
    /// (its enable state and player turns), and the main-menu loop, which
    /// clears the keyboard at the top of every pass.
    pub fn keyboard(&self) -> crate::input::Keyboard {
        let world = match &self.stage {
            Stage::World(world) | Stage::Load(world) => world.keyboard(),
            _ => crate::input::Keyboard::default(),
        };
        crate::input::Keyboard {
            clears: world.clears.wrapping_add(self.menu_passes),
            ..world
        }
    }
    pub fn world_shown(&self) -> bool {
        matches!(self.stage, Stage::World(_) | Stage::Load(_))
    }
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        // A left release ends the held button's down art.
        let held = if right { None } else { self.held.take() };
        let over = held.filter(|rect| point.is_some_and(|point| rect.contains(point)));
        let rules = self.rules();
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.release(point, right),
            Stage::Movie { player, .. } => {
                player.release();
                if player.finished() {
                    self.movie_done();
                }
                true
            }
            Stage::Menu { .. } if rules.six_button_menu => {
                if let Some(i) = (0..6).find(|i| over == Some(ui2::menu_button(*i))) {
                    self.menu_poll2(ui2::MenuInput::Release(i));
                }
                held.is_some()
            }
            Stage::Menu { .. } => {
                if let Some(button) = flow::MENU_BUTTONS
                    .into_iter()
                    .find(|b| over == Some(b.rect()))
                {
                    self.menu_poll(MenuInput::Release(button));
                }
                held.is_some()
            }
            Stage::Selection => {
                let Some(rect) = held else {
                    return false;
                };
                let red = rect.w == 15;
                self.audio_cue(if red {
                    vault_core::ui::BUTTON_RELEASE
                } else {
                    "ib2lu1x1"
                });
                if let Some(action) = over
                    .and_then(|rect| selection_button([rect.x, rect.y]).map(|(_, action)| action))
                {
                    // The arrows act as the Left and Right keys, which play
                    // `ib2p1xx1`.
                    if !red {
                        self.audio_cue("ib2p1xx1");
                    }
                    self.selection_action(action);
                }
                true
            }
            Stage::Editor(stage) if !right => {
                let exit = stage.release(over.is_some(), point);
                self.editor_exit(exit);
                true
            }
            _ => held.is_some(),
        }
    }
    /// Whether the stage is a full-screen 640x480 screen the `enhanced`
    /// widescreen bezel surrounds: anything but a movie and a world.
    pub fn bezel_stage(&self) -> bool {
        !matches!(
            self.stage,
            Stage::Movie { .. } | Stage::World(_) | Stage::Load(_)
        )
    }
    /// SPEC-012 §3.12: during a movie every mouse button counts, wherever
    /// it is pressed (the letterbox around the frame too). Returns whether
    /// a movie took the button.
    pub fn movie_button(&mut self, pressed: bool) -> bool {
        match &mut self.stage {
            Stage::Movie { player, .. } => {
                if pressed {
                    player.press();
                } else {
                    player.release();
                    if player.finished() {
                        self.movie_done();
                    }
                }
                true
            }
            Stage::World(world) | Stage::Load(world) => world.movie_button(pressed),
            _ => false,
        }
    }
    /// The pointer left the window.
    pub fn pointer_left(&mut self) {
        self.pointer = None;
        if let Some(world) = self.world() {
            world.pointer_left();
        }
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        self.world().is_some_and(|world| world.scroll(delta))
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<vault_render::map::MapHover> {
        self.pointer = Some(point);
        self.world()?.hover(point)
    }
    pub fn set_modifiers(&mut self, modifiers: [bool; 3]) {
        self.modifiers = modifiers;
        if let Some(world) = self.world() {
            world.set_modifiers(modifiers);
        }
    }
    /// A world's pending system-pointer warp (SPEC-014 §3.28).
    pub fn take_warp(&mut self) -> Option<[i32; 2]> {
        self.world()?.take_warp()
    }
}

/// A red button at `origin`.
/// SPEC-019 §3.2 Fallout 2's main menu: `mainmenu.frm`, six buttons at
/// (30, 19 + 41i) with their `misc.msg` 9 + i labels (or the engine
/// strings) centered on x = 126 in font 104, the copyright `misc.msg` 20
/// (none when missing) and the version text.
fn draw_menu2(painter: &Painter, image: &mut IndexedImage, held: Option<Rect>) {
    painter.blit_opaque(image, flow::MENU_BACKGROUND, 0, [0, 0]);
    for (i, fallback) in ui2::text::MENU.iter().enumerate() {
        let rect = ui2::menu_button(i);
        let (up, down) = flow::MENU_BUTTON_ART;
        let art = if held == Some(rect) { down } else { up };
        painter.blit(image, art, 0, [rect.x, rect.y]);
        let label = painter.message_or("misc.msg", 9 + i as i32, fallback);
        let origin = ui2::menu_label(i, painter.width(104, &label));
        painter.text(image, 104, flow::MENU_LABEL_COLOR, &label, origin, 200);
    }
    let copyright = painter.message("misc.msg", ui2::MENU_COPYRIGHT_ID);
    if copyright.is_empty() {
        crate::interface::note_once("misc.msg 20", "no copyright line");
    } else {
        painter.text(
            image,
            100,
            flow::MENU_TEXT_COLOR,
            &copyright,
            flow::MENU_COPYRIGHT,
            625,
        );
    }
    let version = ui2::MENU_VERSION.as_bytes();
    let [right, y] = flow::MENU_VERSION_RIGHT;
    painter.text(
        image,
        100,
        flow::MENU_TEXT_COLOR,
        version,
        [right - painter.width(100, version), y],
        200,
    );
}

/// One slideshow step as shown: SPEC-014 §3.30's fixed table or SPEC-019
/// §3.20's `endgame.txt` records.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    art: String,
    palette: String,
    /// A desert pan, and whether it runs left to right.
    pan: Option<bool>,
}
impl From<Slide> for Shown {
    fn from(slide: Slide) -> Self {
        Self {
            art: slide.art_path(),
            palette: slide.palette_path(),
            pan: slide.pan,
        }
    }
}
impl From<ui2::Slide2> for Shown {
    fn from(slide: ui2::Slide2) -> Self {
        Self {
            art: slide.art,
            palette: slide.palette,
            pan: slide.pan,
        }
    }
}

fn red_button(origin: [i32; 2]) -> Rect {
    Rect::new(origin[0], origin[1], 15, 16)
}
/// §3.7 the selection button under `point` and the key it acts as: Take,
/// Modify, Create and Back (`t`, `m`, `c`, Esc), then the arrows.
fn selection_button(point: [i32; 2]) -> Option<(Rect, SelectionAction)> {
    let actions = [
        SelectionAction::Take,
        SelectionAction::Modify,
        SelectionAction::Create,
        SelectionAction::Back,
    ];
    flow::SELECTION_BUTTONS
        .iter()
        .map(|origin| red_button(*origin))
        .zip(actions)
        .chain([
            (flow::SELECTION_PREVIOUS, SelectionAction::Previous),
            (flow::SELECTION_NEXT, SelectionAction::Next),
        ])
        .find(|(rect, _)| rect.contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{audio::sound_fixture, map_preview::test_support::synthetic_corpus};
    use vault_formats::LocalDirProvider;

    #[test]
    fn menu_music_volumes_and_the_death_narration() {
        let dir = synthetic_corpus();
        let root = dir.path();
        let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
        std::fs::write(root.join("maps/v13ent.map"), &map).unwrap();
        std::fs::create_dir_all(root.join("sound/music")).unwrap();
        std::fs::create_dir_all(root.join("sound/speech/narrator")).unwrap();
        std::fs::write(
            root.join("sound/music/07desert.acm"),
            sound_fixture::acm('B'),
        )
        .unwrap();
        for draw in 3..=6 {
            std::fs::write(
                root.join(format!("sound/speech/narrator/nar_{draw}.acm")),
                sound_fixture::acm('H'),
            )
            .unwrap();
        }
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
        let mut game = Game::new(vfs, "english", 1, root.join("saves"), None).unwrap();
        // No start movies: the menu is up before the output is attached,
        // and its music starts once it is (SPEC-014 §3.4).
        let (audio, mixer) = AudioHandle::detached();
        game.set_audio(audio.clone());
        let menu = MusicRequest {
            path: flow::MENU_MUSIC.into(),
            looping: true,
        };
        assert_eq!(audio.last_music(), Some(menu.clone()));
        audio.wait_music();
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        // The preference volumes reach the mixer (SPEC-014 §3.18).
        game.tick(16).unwrap();
        assert_eq!(mixer.lock().unwrap().master(), 22281);
        assert_eq!(mixer.lock().unwrap().level(Channel::Music), Some(20944));
        // A new game, then a knockout: white, the death scene's fade from
        // white, then the narration nar_<D(1, 4) + 2> as speech.
        assert!(game.step(KeyCode::KeyN));
        assert!(game.step(KeyCode::KeyT));
        game.tick(32).unwrap();
        let world = game.world().unwrap();
        let player = world.host.world.player;
        world
            .host
            .world
            .critter_state_mut(player)
            .unwrap()
            .results
            .0 |= 0x01;
        game.tick(48).unwrap();
        game.tick(48 + 700).unwrap();
        assert!(matches!(game.stage, Stage::Death { .. }));
        // The narration waits for the fade from white (SPEC-014 §3.5).
        game.tick(48 + 900).unwrap();
        assert!(!mixer.lock().unwrap().playing(Channel::Speech));
        game.tick(48 + 1500).unwrap();
        let Stage::Death {
            narration, draw, ..
        } = &game.stage
        else {
            panic!("the death scene");
        };
        assert_eq!(*narration, Some(true));
        assert!((1..=4).contains(draw));
        assert!(mixer.lock().unwrap().playing(Channel::Speech));
        // SPEC-010 §3.13: narration plays at trunc(0.69 x 22281).
        assert_eq!(mixer.lock().unwrap().level(Channel::Speech), Some(15373));
        // A key ends the wait at once: the speech stops and the scene
        // fades to black, then the menu (and its music) return.
        assert!(game.step(KeyCode::Space));
        assert!(!mixer.lock().unwrap().playing(Channel::Speech));
        assert!(matches!(game.stage, Stage::Death { leaving: true, .. }));
        game.tick(48 + 2300).unwrap();
        assert_eq!(game.title(), "Vault — main menu");
        // The window shows the product name in play, the stage with
        // `--debug`.
        assert_eq!(game.window_title(), "Fallout — Vault Engine");
        game.preset = vault_core::EnginePreset::Enhanced;
        assert_eq!(game.window_title(), "Fallout — Vault Engine [Enhanced]");
        game.debug_keys = true;
        assert_eq!(game.window_title(), "Vault — main menu");
        assert_eq!(audio.last_music(), Some(menu));
    }

    #[test]
    fn the_configured_music_key_and_volumes_set_the_music_state() {
        let dir = synthetic_corpus();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sound/music")).unwrap();
        std::fs::write(
            root.join("sound/music/07desert.acm"),
            sound_fixture::acm('B'),
        )
        .unwrap();
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
        let config = root.join("fallout.cfg");
        let start = |text: &str| {
            std::fs::write(&config, text).unwrap();
            let mut game = Game::new(
                vfs.clone(),
                "english",
                1,
                root.join("saves"),
                Some(config.clone()),
            )
            .unwrap();
            let (audio, mixer) = AudioHandle::detached();
            game.set_audio(audio.clone());
            audio.wait_music();
            (game, audio, mixer)
        };
        // SPEC-008 §3.10 rule 1: `music=0` keeps music off; the menu track
        // is recorded but never plays, and no volume turns it on.
        let (mut game, audio, mixer) = start("[sound]\nmusic=0\n");
        assert!(!audio.music_enabled());
        assert!(audio.last_music().is_some());
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(mixer.lock().unwrap().movie_level(), 0);
        game.preferences.music_volume = 30000;
        game.tick(16).unwrap();
        assert!(!audio.music_enabled());
        // Rule 2 from configuration: master volume 0 starts it off by
        // volume; setting it nonzero turns it on and replays the menu
        // track. The movie level ignores master (SPEC-012 §3.8).
        let (mut game, audio, mixer) = start("[sound]\nmaster_volume=0\n");
        assert!(!audio.music_enabled());
        assert!(!mixer.lock().unwrap().playing(Channel::Music));
        game.preferences.master_volume = 22281;
        game.tick(16).unwrap();
        audio.wait_music();
        assert!(audio.music_enabled());
        assert!(mixer.lock().unwrap().playing(Channel::Music));
        assert_eq!(mixer.lock().unwrap().movie_level(), 20944);
        // The default key plays the menu music at once.
        let (_, audio, mixer) = start("[sound]\nmusic=1\n");
        assert!(audio.music_enabled());
        assert!(mixer.lock().unwrap().playing(Channel::Music));
    }

    /// A synthetic Fallout 2 install (SPEC-018 §8.13's), with `extra`
    /// files added to the corpus first; the game over it, under the
    /// Fallout 2 profile, headless.
    fn fallout2_game(extra: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, tempfile::TempDir, Game) {
        let corpus = synthetic_corpus();
        vault_fixtures::fallout2::write_tables(corpus.path());
        for (path, bytes) in extra {
            let path = corpus.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let install = tempfile::tempdir().unwrap();
        let (master, critter) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
        let vfs = crate::preview::PreviewArgs {
            game_profile: vault_core::GameProfile::Fallout2,
            master_dat: Some(master),
            critter_dat: Some(critter),
            game: true,
            headless: true,
            ..Default::default()
        }
        .source()
        .unwrap();
        let mut game = Game::new(vfs, "english", 1, install.path().join("saves"), None).unwrap();
        game.headless = true;
        game.game_profile = vault_core::GameProfile::Fallout2;
        (corpus, install, game)
    }

    /// Each premade's selection screen, with the sheet it loaded.
    fn selection_screens(game: &mut Game) -> Vec<(IndexedImage, CharacterSheet)> {
        (0..flow::PREMADES.len())
            .map(|index| {
                game.stage = Stage::Selection;
                game.selection = index;
                game.sheet = game.premade(index);
                (game.image().unwrap(), game.sheet.clone())
            })
            .collect()
    }
    /// The premades of `vault_fixtures::state::premade_files`: each
    /// portrait drawn from its own art at `portrait` (the rectangle its
    /// drawn rows cover), every odd row and everything outside left to the
    /// background, and each sheet's name and three distinct tags.
    fn check_selection(game: &mut Game, portraits: [Rect; 3]) {
        use vault_fixtures::state::{PICKCHAR_COLOR, PREMADES};
        for (index, (image, sheet)) in selection_screens(game).into_iter().enumerate() {
            let (premade, rect) = (&PREMADES[index], portraits[index]);
            let at = |x: i32, y: i32| image.pixels()[(y * 640 + x) as usize];
            let (right, bottom) = (rect.x + rect.w - 1, rect.y + (rect.h - 1) / 2 * 2);
            for [x, y] in [
                [rect.x, rect.y],
                [right, rect.y],
                [rect.x, bottom],
                [right, bottom],
            ] {
                assert_eq!(at(x, y), premade.color, "premade {index}: ({x}, {y})");
            }
            for [x, y] in [
                [0, 0],
                [rect.x, rect.y + 1],
                [rect.x - 1, rect.y],
                [right + 1, rect.y],
                [rect.x, rect.y - 1],
                [rect.x, bottom + 2],
            ] {
                assert_eq!(at(x, y), PICKCHAR_COLOR, "premade {index}: ({x}, {y})");
            }
            assert_eq!(sheet.name, premade.name);
            assert_eq!(sheet.tags, premade.tags);
            assert_eq!(sheet.traits, premade.traits);
            let [a, b, c, _] = sheet.tags;
            assert!(
                a != b && b != c && a != c,
                "premade {index}: {:?}",
                sheet.tags
            );
        }
    }

    #[test]
    fn spec014_fallout1_premade_portraits_sit_on_the_monitor_with_three_tags() {
        let dir = synthetic_corpus();
        for (path, bytes) in vault_fixtures::state::premade_files(false) {
            std::fs::write(dir.path().join(path), bytes).unwrap();
        }
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut game = Game::new(vfs, "english", 1, dir.path().join("saves"), None).unwrap();
        game.headless = true;
        // §3.7: 212 by 187 at (150 - 106, 240 - 187) = (44, 53).
        check_selection(&mut game, [Rect::new(44, 53, 212, 187); 3]);
    }

    #[test]
    fn spec019_fallout2_premade_portraits_fill_the_monitor_with_three_tags() {
        let files = vault_fixtures::state::premade_files(true);
        let extra: Vec<(&str, Vec<u8>)> = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.clone()))
            .collect();
        let (_corpus, _install, mut game) = fallout2_game(&extra);
        // §3.3 (Revision 3): at the monitor window's (27, 23), never at
        // (0, 0) nor at Fallout 1's (150 - 296, 240 - 260) = (-146, -20);
        // the third, 640 by 300, is clipped to the 592-by-260 window. The
        // sheets are 432 bytes (SPEC-018 §6.3, Revision 4).
        let screen = ui2::SELECTION_SCREEN;
        assert_eq!(screen, Rect::new(27, 23, 592, 260));
        check_selection(&mut game, [screen; 3]);
    }

    /// The retail premades: each portrait is its own art, `size`, drawn at
    /// `origin` (its first `columns` columns compared, which no text
    /// covers) with odd rows left to the background; outside the monitor
    /// (`outside`) and above the buttons (y < 300) the screen is the
    /// background alone; each sheet has a name and three distinct tags.
    fn check_retail_selection(
        game: &mut Game,
        size: [i32; 2],
        origin: [i32; 2],
        columns: i32,
        outside: impl Fn(i32, i32) -> bool,
    ) {
        for (index, (image, sheet)) in selection_screens(game).into_iter().enumerate() {
            let premade = flow::PREMADES[index];
            let painter = Painter {
                assets: &game.assets,
                palette: &game.palette,
            };
            let art = painter.art(premade.portrait).expect("the portrait loads");
            let frame = art.frame(0, 0).unwrap();
            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
            assert_eq!([w, h], size, "{}", premade.portrait);
            let background = painter.art(flow::SELECTION_BACKGROUND).unwrap();
            let background = background.frame(0, 0).unwrap().pixels().to_vec();
            let at = |x: i32, y: i32| image.pixels()[(y * 640 + x) as usize];
            let behind = |x: i32, y: i32| background[(y * 640 + x) as usize];
            for row in 0..h {
                for column in 0..columns {
                    let (x, y) = (origin[0] + column, origin[1] + row);
                    let color = frame.pixels()[(row * w + column) as usize];
                    let expected = if row % 2 == 0 && color != 0 {
                        color
                    } else {
                        behind(x, y)
                    };
                    assert_eq!(at(x, y), expected, "{}: ({x}, {y})", premade.portrait);
                }
            }
            for y in 0..300 {
                for x in (0..640).filter(|x| outside(*x, y)) {
                    assert_eq!(at(x, y), behind(x, y), "{}: ({x}, {y})", premade.portrait);
                }
            }
            let name = String::from_utf8_lossy(&sheet.name).into_owned();
            let [a, b, c, _] = sheet.tags;
            eprintln!(
                "{}: {name}, {} {w}x{h} at {origin:?}, tags {:?}, traits {:?}",
                premade.sheet, premade.portrait, sheet.tags, sheet.traits
            );
            assert!(!name.is_empty(), "{}", premade.sheet);
            assert!(
                a >= 0 && b >= 0 && c >= 0 && a != b && b != c && a != c,
                "{}: {:?}",
                premade.sheet,
                sheet.tags
            );
        }
    }

    #[test]
    #[ignore = "needs the local corpus"]
    fn retail_fallout1_premade_portraits_and_three_distinct_tags() {
        let Some(vfs) = crate::map_preview::test_support::local_corpus() else {
            return;
        };
        let saves = tempfile::tempdir().unwrap();
        let mut game = Game::new(vfs, "english", 1, saves.path().to_path_buf(), None).unwrap();
        game.headless = true;
        // SPEC-014 §3.7: 212 by 187 at (44, 53), inside the monitor area.
        let monitor = flow::SELECTION_MONITOR;
        check_retail_selection(&mut game, [212, 187], [44, 53], 212, |x, y| {
            !monitor.contains([x, y])
        });
    }

    #[test]
    #[ignore = "needs a Fallout 2 install"]
    fn retail_fallout2_premade_portraits_and_three_distinct_tags() {
        let folder = std::env::var_os("VAULT_FALLOUT2")
            .map(PathBuf::from)
            .or_else(|| crate::config::home_dir().map(|home| home.join("games/fallout2")));
        let Some(install) = folder
            .and_then(|folder| vault_formats::fallout2::Fallout2Install::discover(&folder).ok())
        else {
            eprintln!("skipping: no Fallout 2 install (set VAULT_FALLOUT2)");
            return;
        };
        let vfs: Arc<dyn Vfs> = Arc::new(install.open().unwrap().provider(None));
        let saves = tempfile::tempdir().unwrap();
        let mut game = Game::new(vfs, "english", 1, saves.path().to_path_buf(), None).unwrap();
        game.headless = true;
        game.game_profile = vault_core::GameProfile::Fallout2;
        // SPEC-019 §3.3 (Revision 3): 592 by 260 at the monitor window's
        // (27, 23). Its left 200 columns, the face, carry no text; the
        // bio may run past the window's right edge, so only the left, top
        // and bottom margins are compared.
        let screen = ui2::SELECTION_SCREEN;
        check_retail_selection(&mut game, [592, 260], [27, 23], 200, |x, y| {
            x < screen.x || y < screen.y || y >= screen.y + screen.h
        });
    }

    #[test]
    fn spec019_fallout2_menu_options_slides_elevator_and_slots() {
        let elevators =
            b"[Elevator 0]\nbackground=elevbg\nstop_0=1,0,20100,1\nstop_1=2,0,20100,2\n";
        let endgame = b"5, 1, enda, nar_a\n5, 2, endb, nar_b\n9, 0, dp, nar_c, -1\n";
        let (_corpus, install, mut game) = fallout2_game(&[
            ("data/vault-engine/elevators.txt", elevators.to_vec()),
            ("data/endgame.txt", endgame.to_vec()),
            (
                "art/intrface/elevbg.frm",
                vault_fixtures::world::frm(230, 284, 70),
            ),
            (
                "art/intrface/mainmenu.frm",
                vault_fixtures::world::frm(640, 480, 3),
            ),
            (
                "art/intrface/menuup.frm",
                vault_fixtures::world::frm(26, 26, 90),
            ),
            (
                "art/intrface/menudown.frm",
                vault_fixtures::world::frm(26, 26, 91),
            ),
        ]);
        // §3.2: six buttons on the left; Options (button 3) at (30, 142).
        assert!(matches!(game.stage, Stage::Menu { .. }));
        let image = game.image().unwrap();
        let painter = Painter {
            assets: &game.assets,
            palette: &game.palette,
        };
        let up = painter.art(flow::MENU_BUTTON_ART.0).unwrap();
        let frame = up.frame(0, 0).unwrap();
        let (w, h) = (i32::from(frame.width), i32::from(frame.height));
        let opaque = (0..h)
            .flat_map(|y| (0..w).map(move |x| [x, y]))
            .find(|[x, y]| frame.pixels()[(y * w + x) as usize] != 0)
            .unwrap();
        let color = frame.pixels()[(opaque[1] * w + opaque[0]) as usize];
        let rect = ui2::menu_button(3);
        let at = [rect.x + opaque[0], rect.y + opaque[1]];
        assert_eq!(image.pixels()[(at[1] * 640 + at[0]) as usize], color);
        // Nothing at Fallout 1's button column.
        assert_eq!(image.pixels()[(45 * 640 + 430) as usize], 3);
        // The Options button's press and release: the preferences, directly.
        assert!(game.click(at, false));
        assert!(game.release(Some(at), false));
        assert!(matches!(game.stage, Stage::Load(_)));
        let world = game.world().unwrap();
        assert!(world.menu_preferences_open());
        // §2.7: no cover over the fifth knob row.
        assert!(world.host.world.fallout2.is_some());
        // Cancel (Esc) closes them; the next pass is back at the menu.
        assert!(game.step(KeyCode::Escape));
        game.tick(100).unwrap();
        assert!(matches!(game.stage, Stage::Menu { .. }));
        // `o` opens them too.
        assert!(game.step(KeyCode::KeyO));
        assert!(matches!(game.stage, Stage::Load(_)));
        assert!(game.step(KeyCode::Escape));
        game.tick(200).unwrap();
        // A new game; §3.20 slides from endgame.txt: endb still, dp panned
        // right to left; no global written.
        assert!(game.step(KeyCode::KeyN));
        assert!(game.step(KeyCode::KeyT));
        let world = game.world().unwrap();
        let mut gvars = world.host.world.gvars.clone().unwrap();
        gvars[5] = 2;
        gvars[9] = 0;
        world.host.world.gvars = Some(gvars.clone());
        // §3.11: a Fallout 1 header is an old version, a short one corrupt.
        let root = install.path().join("saves");
        world.host.save_game(&root, 1, &Default::default()).unwrap();
        let slot = |n: u8| root.join(format!("SAVEGAME/SLOT{n:02}"));
        std::fs::create_dir_all(slot(2)).unwrap();
        let mut old = std::fs::read(slot(1).join("SAVE.DAT")).unwrap();
        old[0x1B] = 1;
        std::fs::write(slot(2).join("SAVE.DAT"), &old).unwrap();
        std::fs::create_dir_all(slot(3)).unwrap();
        std::fs::write(slot(3).join("SAVE.DAT"), &old[..100]).unwrap();
        let slots = world.slot_states();
        assert!(matches!(
            slots[0],
            vault_core::ui::saveload::SlotState::Occupied(_)
        ));
        assert_eq!(slots[1], vault_core::ui::saveload::SlotState::OldVersion);
        assert_eq!(slots[2], vault_core::ui::saveload::SlotState::Corrupt);
        // The elevator of metarule 15 type 0: two stops from elevators.txt.
        let view = world.host.elevator_view(0).unwrap();
        assert_eq!(view.background, "elevbg.frm");
        assert_eq!(view.destinations, [(1, 0, 20100), (2, 0, 20100)]);
        world.open_elevator(0);
        world.image().unwrap();
        assert!(world.elevator_open());
        world.host.perform_elevator(0, Some(0)).unwrap();
        assert_eq!(
            world
                .host
                .world
                .pending_transition
                .map(|transition| transition.map),
            Some(1)
        );
        world.endgame = Some(WorldEffect::Slideshow);
        game.tick(300).unwrap();
        let Stage::Slides { slides, .. } = &game.stage else {
            panic!("the slideshow");
        };
        assert_eq!(
            slides
                .iter()
                .map(|slide| slide.art.as_str())
                .collect::<Vec<_>>(),
            ["art/intrface/endb.frm", "art/intrface/dp.frm"]
        );
        assert_eq!(slides[1].pan, Some(false));
        for _ in 0..20 {
            if !matches!(game.stage, Stage::Slides { .. }) {
                break;
            }
            game.step(KeyCode::Space);
        }
        let world = game.world().unwrap();
        assert_eq!(world.host.world.gvars.as_ref().unwrap(), &gvars);
    }

    #[test]
    fn spec014_3_30_global_299_is_cleared_after_the_slideshow() {
        let dir = synthetic_corpus();
        let root = dir.path();
        let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
        std::fs::write(root.join("maps/v13ent.map"), &map).unwrap();
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
        let mut game = Game::new(vfs, "english", 1, root.join("saves"), None).unwrap();
        assert!(game.step(KeyCode::KeyN));
        assert!(game.step(KeyCode::KeyT));
        let world = game.world().unwrap();
        let mut gvars = vec![0; 700];
        gvars[endgame::DONE_GLOBAL] = 1;
        world.host.world.gvars = Some(gvars);
        world.endgame = Some(WorldEffect::Slideshow);
        game.tick(16).unwrap();
        let done = |world: &MapPreview| world.host.world.gvars.as_ref().unwrap()[299];
        let Stage::Slides { world, .. } = &game.stage else {
            panic!("the slideshow");
        };
        assert_eq!(done(world), 1, "still set while the slides show");
        for _ in 0..20 {
            if !matches!(game.stage, Stage::Slides { .. }) {
                break;
            }
            game.step(KeyCode::Space);
        }
        let Stage::World(world) = &game.stage else {
            panic!("back in the world");
        };
        assert_eq!(done(world), 0);
    }

    #[test]
    fn a_still_splash_and_slides_do_not_recompose_every_poll() {
        let dir = synthetic_corpus();
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let mut game =
            Game::new(vfs.clone(), "english", 1, dir.path().join("saves"), None).unwrap();
        game.stage = Stage::Splash {
            image: IndexedImage::new(640, 480).unwrap(),
            palette: black(),
            since: 0,
        };
        assert!(game.tick(16).unwrap(), "the first poll shows it");
        assert!(!game.tick(32).unwrap());
        assert!(!game.tick(500).unwrap());
        // The selection screen reads its bio once per premade.
        game.stage = Stage::Selection;
        game.image().unwrap();
        assert_eq!(game.bio.as_ref().map(|(index, _)| *index), Some(0));
    }

    #[test]
    fn the_widescreen_bezel_surrounds_the_menus_but_not_a_world() {
        let dir = synthetic_corpus();
        let root = dir.path();
        let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
        std::fs::write(root.join("maps/v13ent.map"), &map).unwrap();
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
        let mut game = Game::new(vfs, "english", 1, root.join("saves"), None).unwrap();
        assert!(matches!(game.stage, Stage::Menu { .. }));
        assert!(game.bezel_stage(), "the main menu");
        assert!(game.step(KeyCode::KeyN));
        assert!(game.step(KeyCode::KeyT));
        assert!(game.world().is_some());
        assert!(!game.bezel_stage(), "a world draws its own layers");
        game.stage = Stage::Selection;
        assert!(game.bezel_stage(), "character selection");
        game.stage = Stage::Splash {
            image: IndexedImage::new(640, 480).unwrap(),
            palette: black(),
            since: 0,
        };
        assert!(game.bezel_stage(), "a splash");
    }
}
