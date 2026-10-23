//! SPEC-014 game flow outside the world (`--game`): the splash and start
//! movies, the main menu and attract mode, credits and quotes, premade
//! selection, the character editor in creation mode, the new-game hand-off,
//! loading from the menu, the death scene, the endgame slideshow and the
//! ending sequence. The world itself is a `MapPreview`. Palette fades run
//! over the fixed 700 ms of the SPEC-014 §3.2 safe rule.
use crate::{
    AppResult,
    audio::{AudioHandle, Channel, MusicRequest, Volumes, volume_gain},
    editor_screen::{EditorDialog, EditorExit, EditorStage, draw_editor},
    interface::Painter,
    map_preview::MapPreview,
    movie::{MoviePlayer, SubtitleStyle},
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
        slides: Vec<Slide>,
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
    config_path: Option<PathBuf>,
    pub preferences: Preferences,
    /// `fallout.cfg` `[system] interrupt_walk` (SPEC-014 §3.27), read once
    /// at startup beside the splash.
    interrupt_walk: bool,
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
    /// A headless replay: worlds complete movies instead of waiting.
    pub headless: bool,
    /// The vault-debug inspector runs in each world (`--debug`; F9 in a
    /// world toggles it, and the setting carries to the next world).
    pub debug: bool,
    /// `--debug` was given: the worlds accept the map preview's keys
    /// (G-09); without it they keep only the game's own keys.
    pub debug_keys: bool,
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
fn black() -> Palette {
    palette_from(&[[0; 3]; 256])
}
fn letter(key: KeyCode) -> Option<char> {
    let name = format!("{key:?}");
    name.strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .and_then(|rest| rest.chars().next())
        .map(|c| c.to_ascii_lowercase())
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
            config_path,
            preferences,
            interrupt_walk: true,
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
            headless: false,
            debug: false,
            debug_keys: false,
            sounds: Default::default(),
            finished: false,
            fade: None,
            shown: None,
            held: None,
            pointer: None,
            held_shown: false,
            volume_source: None,
            bio: None,
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
    /// Attach the audio output. A menu entered before it was attached
    /// (no splash or start movies) starts its music now.
    pub fn set_audio(&mut self, audio: AudioHandle) {
        self.audio = audio;
        self.volume_source = None;
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
                    player.audio = self.audio.clone();
                    player.music_volume = self.preferences.music_volume;
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
            Ok(world) => self.stage = Stage::World(Box::new(world)),
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
    /// change (a world applies its own while it runs).
    fn sync_volumes(&mut self) {
        let wanted = Volumes::from_preferences(&self.preferences);
        if self.volume_source != Some(wanted) {
            self.volume_source = Some(wanted);
            self.audio.apply_volumes(wanted);
        }
    }
    /// Leaving a world keeps its preferences and save-slot session state;
    /// the mixer takes the preference volumes again.
    fn leave_world(&mut self, world: &MapPreview) {
        self.volume_source = None;
        self.preferences = world.preferences.clone();
        (self.slot_list, self.quick) = world.slot_session();
        self.debug = world.debug_enabled();
    }
    fn world_with(&self, sheet: CharacterSheet) -> AppResult<MapPreview> {
        let assets = VfsWorldAssets::new(self.vfs.clone(), &self.language)?;
        let mut world = MapPreview::with_sheet(assets, flow::NEW_GAME_MAP, sheet, self.seed)?;
        world.host.world.clock.ticks = flow::START_TICKS;
        world.audio = self.audio.clone();
        world.save_root = self.save_root.clone();
        world.config_path = self.config_path.clone();
        world.preferences = self.preferences.clone();
        world.interrupt_walk = self.interrupt_walk;
        // SPEC-008 §3.10: the world starts with the mixer's music state.
        let music = self.audio.music_enabled();
        world.set_music_enabled(music);
        world.apply_preferences();
        world.set_slot_session(self.slot_list.clone(), self.quick);
        if self.headless {
            world.host.movie_policy = vault_core::movie::MoviePolicy::CompleteImmediately;
        }
        world.set_debug(self.debug);
        // G-09: the map preview tool's keys need `--debug` here, and no key
        // turns the roofs off in the game (SPEC-014 §3.36).
        world.preview_keys = self.debug_keys;
        world.roof_key = false;
        Ok(world)
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
                self.selection = 0;
                self.sheet = self.premade(0);
                self.stage = Stage::Selection;
            }
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
    fn menu_poll(&mut self, input: MenuInput) {
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
    /// An interface sound, `sound/sfx/<name>.acm`, at the effects volume
    /// (SPEC-010 §3.11 with no object: V = 32767).
    fn audio_cue(&mut self, name: &str) {
        let path = format!("sound/sfx/{name}.acm");
        let gain = volume_gain(self.preferences.sndfx_volume);
        if let Some(samples) = self.sounds.load(&*self.vfs, &path) {
            self.audio.play(samples, gain, Channel::Effect, false);
        }
    }
    /// Take the selected (or edited) character into the game: the
    /// `ovrintro` movie, then the world.
    fn start_game(&mut self, sheet: CharacterSheet) {
        let (path, flags) = flow::NEW_GAME_MOVIE;
        self.play_movie(path, flags, Next::NewGame(Box::new(sheet)));
    }
    fn selection_action(&mut self, action: SelectionAction) {
        let context = StatContext::default();
        // SPEC-014 §3.43: the sheet files live in the master patch root.
        let files = || {
            crate::editor_screen::SheetFiles::new(
                self.vfs.clone(),
                &self.language,
                self.save_root.clone(),
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
                        let gain = volume_gain(self.preferences.speech_volume);
                        let started = !*input
                            && self.sounds.load(&*self.vfs, &path).is_some_and(|samples| {
                                self.audio.play(samples, gain, Channel::Speech, false)
                            });
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
                if !world.load_screen_open() {
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
    /// F12 (SPEC-014 §3.9): the screen as shown, saved by the platform.
    fn screenshot(&mut self) {
        let Ok(image) = self.image() else {
            return;
        };
        let palette = self
            .display_override()
            .unwrap_or_else(|| self.palette().display_rgba8(self.brightness()));
        crate::interface::save_screenshot(&self.save_root, &image, &palette);
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
                if let Some(value) = world
                    .host
                    .world
                    .gvars
                    .as_mut()
                    .and_then(|g| g.get_mut(endgame::DONE_GLOBAL))
                {
                    *value = 0;
                }
                let slides = endgame::slides(&globals);
                let Stage::World(world) =
                    std::mem::replace(&mut self.stage, Stage::Menu { since: self.now })
                else {
                    return;
                };
                self.stage_slide(slides, 0, world);
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
    /// exit: map processing resumes).
    fn stage_slide(&mut self, slides: Vec<Slide>, index: usize, world: Box<MapPreview>) {
        if index >= slides.len() {
            self.stage = Stage::World(world);
            return;
        }
        let palette = self
            .assets
            .palette(slides[index].palette_path().as_bytes())
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
        match &mut self.stage {
            Stage::Splash { image: splash, .. } => return Ok(splash.clone()),
            Stage::Movie { player, .. } => return Ok(player.image()),
            Stage::World(world) | Stage::Load(world) | Stage::Ending { world, .. } => {
                return world.image();
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
                let slide = slides[*index];
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
                if let Some(art) = painter.art(&slide.art_path())
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
        // The portrait with every odd row left out (the scan-line effect).
        if let Some(art) = painter.art(premade.portrait)
            && let Some(frame) = art.frame(0, 0)
        {
            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
            let [x, y] = flow::portrait_origin(w, h);
            for row in (0..h).step_by(2) {
                for column in 0..w {
                    let color = frame.pixels()[(row * w + column) as usize];
                    let (dx, dy) = (x + column, y + row);
                    if color != 0 && (0..640).contains(&dx) && (0..480).contains(&dy) {
                        image.pixels_mut()[(dy * 640 + dx) as usize] = color;
                    }
                }
            }
        }
        let green = flow::SELECTION_TEXT;
        painter.centered(
            image,
            101,
            green,
            premade.id.as_bytes(),
            [150 - 100, 252],
            200,
        );
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
            Stage::Editor(_) if key == KeyCode::F12 => {
                self.screenshot();
                true
            }
            Stage::Editor(stage) => {
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
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.click(point, right),
            Stage::Movie { player, .. } => {
                player.press();
                true
            }
            // §3.4: a press plays `nmselec0`; the release over the button
            // selects it.
            // §3.3 only the nonzero pixels of the up image are clickable.
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
    /// The world (or the menu's load screen) is in front: it draws its own
    /// cursor.
    /// Where keys land now (SPEC-014 §3.29 engine key repeat).
    pub fn key_layer(&self) -> crate::preview::KeyLayer {
        let stage = match &self.stage {
            Stage::Splash { .. } => 0,
            Stage::Movie { .. } => 1,
            Stage::Menu { .. } => 2,
            Stage::Credits { .. } => 3,
            Stage::Selection => 4,
            Stage::Editor(_) => 5,
            Stage::World(_) => 6,
            Stage::Load(_) => 7,
            Stage::Death { .. } => 8,
            Stage::Ending { .. } => 9,
            Stage::Slides { .. } => 10,
        };
        crate::preview::KeyLayer {
            stage,
            field: matches!(
                &self.stage,
                Stage::Editor(editor) if matches!(editor.dialog, Some(EditorDialog::Name(_)))
            ),
            world: match &self.stage {
                Stage::World(world) | Stage::Load(world) => Some(world.key_layer()),
                _ => None,
            },
        }
    }
    pub fn world_shown(&self) -> bool {
        matches!(self.stage, Stage::World(_) | Stage::Load(_))
    }
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        // A left release ends the held button's down art.
        let held = if right { None } else { self.held.take() };
        let over = held.filter(|rect| point.is_some_and(|point| rect.contains(point)));
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.release(point, right),
            Stage::Movie { player, .. } => {
                player.release();
                if player.finished() {
                    self.movie_done();
                }
                true
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
    use crate::{audio::sound_fixture, map_preview::state_fixture};
    use vault_formats::LocalDirProvider;

    #[test]
    fn menu_music_volumes_and_the_death_narration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        state_fixture::write_corpus(root);
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
        assert_eq!(mixer.lock().unwrap().master(), volume_gain(22281));
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
        // A key ends the wait at once: the speech stops and the scene
        // fades to black, then the menu (and its music) return.
        assert!(game.step(KeyCode::Space));
        assert!(!mixer.lock().unwrap().playing(Channel::Speech));
        assert!(matches!(game.stage, Stage::Death { leaving: true, .. }));
        game.tick(48 + 2300).unwrap();
        assert_eq!(game.title(), "Vault — main menu");
        assert_eq!(audio.last_music(), Some(menu));
    }

    #[test]
    fn a_still_splash_and_slides_do_not_recompose_every_poll() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
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
}
