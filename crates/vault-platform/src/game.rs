//! SPEC-014 game flow outside the world (`--game`): the splash and start
//! movies, the main menu and attract mode, credits and quotes, premade
//! selection, the character editor in creation mode, the new-game hand-off,
//! loading from the menu, the death scene, the endgame slideshow and the
//! ending sequence. The world itself is a `MapPreview`. Palette fades are
//! presented as instant palette changes.
use crate::{
    AppResult,
    audio::AudioHandle,
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
        ColorKey, Rect,
        editor::{self, Editor, InfoLine, NameField},
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
        since: u64,
        palette: Palette,
        /// The scene is over and the fade to black runs.
        leaving: bool,
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
struct EditorStage {
    editor: Editor,
    /// The open name field, if any.
    name: Option<NameField>,
    /// A validation refusal on screen.
    refusal: Option<editor::Refusal>,
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
    sounds: crate::map_preview::SoundBank,
    finished: bool,
    /// SPEC-014 §3.2 a fade outside the world, and the six-bit levels shown
    /// in place of the stage's palette once one has ended.
    fade: Option<GameFade>,
    shown: Option<[[u8; 3]; 256]>,
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
            sounds: Default::default(),
            finished: false,
            fade: None,
            shown: None,
        };
        // §3.1 splash: the configured start, without wrapping the stored value.
        let start = config.get_int("system", "splash").unwrap_or(0);
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
    /// left by the end of a session, to the game palette.
    fn enter_menu(&mut self) {
        self.stage = Stage::Menu { since: self.now };
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
    /// Leaving a world keeps its preferences and save-slot session state.
    fn leave_world(&mut self, world: &MapPreview) {
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
        world.apply_preferences();
        world.set_slot_session(self.slot_list.clone(), self.quick);
        if self.headless {
            world.host.movie_policy = vault_core::movie::MoviePolicy::CompleteImmediately;
        }
        world.set_debug(self.debug);
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
            MenuChoice::Exit => self.finished = true,
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
    /// An interface sound, `sound/sfx/<name>.acm`.
    fn audio_cue(&mut self, name: &str) {
        let path = format!("sound/sfx/{name}.acm");
        if let Some(samples) = self.sounds.load(&*self.vfs, &path) {
            self.audio
                .play(samples, 1.0, crate::audio::Channel::Effect, false);
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
        match action {
            SelectionAction::Take => self.start_game(self.sheet.clone()),
            SelectionAction::Modify => {
                self.stage = Stage::Editor(Box::new(EditorStage {
                    editor: Editor::open(self.sheet.clone(), false, context),
                    name: None,
                    refusal: None,
                }))
            }
            SelectionAction::Create => {
                self.stage = Stage::Editor(Box::new(EditorStage {
                    editor: Editor::open(editor::blank_sheet(context), true, context),
                    name: None,
                    refusal: None,
                }))
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
            Stage::Movie { player, .. } => Some(player.palette()),
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
            Stage::World(world) => world.host.library.palette_cycling && !world.movie_active(),
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
        self.now = now;
        let faded = self.fade_tick();
        let fading = self.fade.is_some();
        match &mut self.stage {
            // The end-of-session fade to white blocks; then the map goes.
            Stage::Ending { death, .. } => {
                if !faded {
                    return Ok(fading);
                }
                let death = *death;
                let Stage::Ending { world, .. } =
                    std::mem::replace(&mut self.stage, Stage::Menu { since: now })
                else {
                    unreachable!()
                };
                self.leave_world(&world);
                drop(world);
                if death {
                    self.enter_death();
                } else {
                    self.enter_menu();
                }
                Ok(true)
            }
            Stage::Splash { since, .. } => {
                if *since == 0 {
                    *since = now.max(1);
                } else if now >= *since + 1000 {
                    self.start_movies();
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
                Ok(faded || fading)
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
            // §3.5: after the narration (or its 3000 ms stand-in and the
            // 500 ms tail), a fade to black; then the menu fades in.
            Stage::Death { since, leaving, .. } => {
                if *leaving {
                    if faded {
                        self.enter_menu();
                    }
                    return Ok(true);
                }
                if now >= *since + flow::DEATH_SILENT_MS + flow::DEATH_TAIL_MS {
                    self.leave_death();
                }
                Ok(faded || fading)
            }
            Stage::Slides {
                since,
                slides,
                index,
                ..
            } => {
                // A still slide: the pauses and the silent timeout; a desert
                // pan: one column per step across D = 640.
                let length = match slides[*index].pan {
                    Some(_) => endgame::pan_step_ms(None) * endgame::PAN_DISTANCE as u64,
                    None => endgame::SLIDE_SILENT_MS,
                } + 2 * endgame::SLIDE_PAUSE_MS;
                if now >= *since + length {
                    self.next_slide();
                }
                Ok(true)
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
            Stage::Selection | Stage::Editor(_) => Ok(false),
        }
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
    /// end left, fading from white to `death.pal`.
    fn enter_death(&mut self) {
        let palette = self
            .vfs
            .read_bounded(flow::DEATH_PALETTE, MAX_ASSET_BYTES)
            .ok()
            .and_then(|bytes| Palette::parse(&bytes).ok())
            .unwrap_or_else(black);
        let target = levels(&palette);
        self.shown = Some(WHITE);
        self.stage = Stage::Death {
            since: self.now,
            palette,
            leaving: false,
        };
        self.fade_to(target);
    }
    /// Step 6: fade to black; the game palette is then loaded without a
    /// fade, so the screen stays black until the menu fades in.
    fn leave_death(&mut self) {
        if let Stage::Death { leaving, .. } = &mut self.stage
            && !*leaving
        {
            *leaving = true;
            self.fade_to(flow::BLACK);
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
            .vfs
            .read_bounded(&slides[index].palette_path(), MAX_ASSET_BYTES)
            .ok()
            .and_then(|bytes| Palette::parse(&bytes).ok())
            .unwrap_or_else(|| self.palette.clone());
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
                    painter.blit(&mut image, flow::MENU_BUTTON_ART.0, 0, [rect.x, rect.y]);
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
            Stage::Selection => self.draw_selection(&painter, &mut image),
            Stage::Editor(stage) => draw_editor(&painter, &mut image, stage),
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
    fn draw_selection(&self, painter: &Painter, image: &mut IndexedImage) {
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
        if let Ok(bio) = self.vfs.read_bounded(premade.bio, MAX_ASSET_BYTES) {
            for (index, line) in flow::bio_lines(&bio).iter().enumerate() {
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
        for origin in flow::SELECTION_BUTTONS {
            painter.blit(image, "lilredup.frm", 0, origin);
        }
        painter.blit(
            image,
            "slu.frm",
            0,
            [flow::SELECTION_PREVIOUS.x, flow::SELECTION_PREVIOUS.y],
        );
        painter.blit(
            image,
            "sru.frm",
            0,
            [flow::SELECTION_NEXT.x, flow::SELECTION_NEXT.y],
        );
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
            Stage::Editor(stage) => {
                let handled = editor_key(stage, key, self.modifiers[0]);
                match handled {
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
            Stage::Death { .. } => {
                self.leave_death();
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
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.click(point, right),
            Stage::Movie { player, .. } => {
                player.press();
                true
            }
            Stage::Menu { .. } => {
                if let Some(button) = flow::MENU_BUTTONS
                    .into_iter()
                    .find(|b| b.rect().contains(point))
                {
                    self.menu_poll(MenuInput::Press(button));
                    self.menu_poll(MenuInput::Release(button));
                }
                true
            }
            Stage::Selection => {
                let red =
                    |origin: [i32; 2]| Rect::new(origin[0], origin[1], 15, 16).contains(point);
                let actions = [
                    SelectionAction::Take,
                    SelectionAction::Modify,
                    SelectionAction::Create,
                    SelectionAction::Back,
                ];
                if let Some(index) = flow::SELECTION_BUTTONS
                    .iter()
                    .position(|origin| red(*origin))
                {
                    self.selection_action(actions[index]);
                } else if flow::SELECTION_PREVIOUS.contains(point) {
                    self.selection_action(SelectionAction::Previous);
                } else if flow::SELECTION_NEXT.contains(point) {
                    self.selection_action(SelectionAction::Next);
                }
                true
            }
            Stage::Editor(stage) => match editor_click(stage, point) {
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
            },
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
    pub fn world_shown(&self) -> bool {
        matches!(self.stage, Stage::World(_) | Stage::Load(_))
    }
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        match &mut self.stage {
            Stage::World(world) | Stage::Load(world) => world.release(point, right),
            Stage::Movie { player, .. } => {
                player.release();
                if player.finished() {
                    self.movie_done();
                }
                true
            }
            _ => false,
        }
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        self.world().is_some_and(|world| world.scroll(delta))
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<vault_render::map::MapHover> {
        self.world()?.hover(point)
    }
    pub fn set_modifiers(&mut self, modifiers: [bool; 3]) {
        self.modifiers = modifiers;
        if let Some(world) = self.world() {
            world.set_modifiers(modifiers);
        }
    }
}

enum EditorExit {
    Stay(bool),
    Done(Box<CharacterSheet>),
    Cancel,
}

/// SPEC-014 §3.8 element positions used for clicks.
const EDITOR_DONE: Rect = Rect::new(455, 454, 15, 16);
const EDITOR_CANCEL: Rect = Rect::new(552, 454, 15, 16);

fn editor_done(stage: &mut EditorStage) -> EditorExit {
    match stage.editor.done() {
        Ok(sheet) => EditorExit::Done(Box::new(sheet)),
        Err(refusal) => {
            stage.refusal = Some(refusal);
            EditorExit::Stay(true)
        }
    }
}
fn editor_key(stage: &mut EditorStage, key: KeyCode, shift: bool) -> EditorExit {
    if let Some(field) = &mut stage.name {
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(name) = field.accept() {
                    stage.editor.sheet.name = name;
                }
                stage.name = None;
            }
            KeyCode::Escape => stage.name = None,
            KeyCode::Backspace | KeyCode::Delete => field.backspace(),
            KeyCode::Space => field.key(b' '),
            _ => {
                let Some(c) = letter(key) else {
                    return EditorExit::Stay(false);
                };
                field.key(if shift { c.to_ascii_uppercase() } else { c } as u8);
            }
        }
        return EditorExit::Stay(true);
    }
    if stage.refusal.take().is_some() {
        return EditorExit::Stay(true);
    }
    match key {
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyD => editor_done(stage),
        KeyCode::Escape => EditorExit::Cancel,
        KeyCode::KeyN => {
            stage.name = Some(NameField::open(&stage.editor.sheet.name));
            EditorExit::Stay(true)
        }
        KeyCode::KeyA | KeyCode::Equal | KeyCode::Minus => {
            let up = key != KeyCode::Minus;
            stage.editor.sheet.base[33] = editor::age_step(stage.editor.sheet.base[33], up);
            EditorExit::Stay(true)
        }
        KeyCode::KeyS => {
            stage.editor.sheet.base[34] ^= 1;
            EditorExit::Stay(true)
        }
        _ => EditorExit::Stay(false),
    }
}
fn editor_click(stage: &mut EditorStage, point: [i32; 2]) -> EditorExit {
    if stage.refusal.take().is_some() {
        return EditorExit::Stay(true);
    }
    if EDITOR_DONE.contains(point) {
        return editor_done(stage);
    }
    if EDITOR_CANCEL.contains(point) {
        return EditorExit::Cancel;
    }
    let editor = &mut stage.editor;
    for (stat, y) in editor::SPECIAL_Y.iter().enumerate() {
        if Rect::new(149, *y, 16, 12).contains(point) {
            editor.increment(stat);
            return EditorExit::Stay(true);
        }
        if Rect::new(149, y + 11, 17, 12).contains(point) {
            editor.decrement(stat);
            return EditorExit::Stay(true);
        }
    }
    for k in 0..18 {
        if Rect::new(347, 26 + 11 * k, 17, 11).contains(point) {
            let _ = editor.toggle_tag(k);
            return EditorExit::Stay(true);
        }
    }
    for k in 0..16 {
        let (x, row) = if k < 8 { (23, k) } else { (298, k - 8) };
        if Rect::new(x, 352 + 13 * row, 17, 11).contains(point) {
            let _ = editor.toggle_trait(k);
            return EditorExit::Stay(true);
        }
    }
    if let Some(line) = editor::info_hit(point) {
        editor.selected = line;
    }
    EditorExit::Stay(true)
}
fn draw_editor(painter: &Painter, image: &mut IndexedImage, stage: &EditorStage) {
    let editor = &stage.editor;
    let context = StatContext::default();
    painter.blit_opaque(image, editor::BACKGROUND, 0, [0, 0]);
    painter.blit(image, "nameoff.frm", 0, [9, 0]);
    painter.blit(image, "ageoff.frm", 0, [154, 0]);
    painter.blit(image, "sexoff.frm", 0, [235, 0]);
    let name = editor::caption_name(&editor.sheet.name, |text| painter.width(103, text));
    painter.text(
        image,
        103,
        ColorKey::LABEL,
        &name,
        [9 + 75 - painter.width(103, &name) / 2, 6],
        145,
    );
    let mut age = painter.message("editor.msg", 104);
    age.extend(format!(" {}", editor.sheet.stat(33, context)).bytes());
    painter.text(
        image,
        103,
        ColorKey::LABEL,
        &age,
        [154 + 41 - painter.width(103, &age) / 2, 6],
        81,
    );
    let sex = painter.message("editor.msg", 107 + editor.sheet.stat(34, context));
    painter.text(
        image,
        103,
        ColorKey::LABEL,
        &sex,
        [235 + 40 - painter.width(103, &sex) / 2, 6],
        80,
    );
    for (stat, y) in editor::SPECIAL_Y.iter().enumerate() {
        let shown = editor.shown(stat);
        let red = shown > 10;
        for (cell, x) in editor::big_number_cells(shown, shown).iter().zip([58, 72]) {
            painter.big_digit(image, *cell, [x, *y], red);
        }
        let word = painter.message("editor.msg", 199 + shown.clamp(1, 10));
        let key = if editor.selected == InfoLine::Special(stat) {
            ColorKey::SELECTED
        } else {
            ColorKey::GREEN
        };
        painter.text(image, 101, key, &word, [103, y + 8], 50);
        painter.blit(image, "splsoff.frm", 0, [149, *y]);
        painter.blit(image, "snegoff.frm", 0, [149, y + 11]);
    }
    // Conditions: Hit Points with "max/max", then the (inactive at
    // creation) condition lines; the derived list beneath.
    let selected = |line: InfoLine, active: bool| match (active, editor.selected == line) {
        (true, false) => ColorKey::GREEN,
        (true, true) => ColorKey::SELECTED,
        (false, false) => ColorKey::DARK_GREEN,
        (false, true) => ColorKey::DARK_OLIVE,
    };
    for (k, id) in editor::CONDITIONS.iter().enumerate() {
        let mut text = painter.message("editor.msg", *id);
        if k == 0 {
            let max = editor.sheet.stat(7, context);
            text.extend(format!(" {max}/{max}").bytes());
        }
        let key = selected(InfoLine::Condition(k), k == 0);
        painter.text(image, 101, key, &text, [194, 46 + 13 * k as i32], 120);
    }
    for (k, (id, stat, percent)) in editor::DERIVED.iter().enumerate() {
        let key = selected(InfoLine::Derived(k), true);
        let y = 179 + 13 * k as i32;
        let label = painter.message("editor.msg", *id);
        painter.text(image, 101, key, &label, [194, y], 94);
        let value = editor.sheet.stat(*stat, context);
        let value = if *percent {
            format!("{value}%")
        } else {
            value.to_string()
        };
        painter.text(image, 101, key, value.as_bytes(), [288, y], 30);
    }
    // The info card's title and rules.
    let (file, id) = match editor.selected {
        InfoLine::Special(i) => ("stat.msg", 100 + i as i32),
        InfoLine::Skill(k) => ("skill.msg", 100 + k as i32),
        InfoLine::Trait(k) => ("trait.msg", 100 + k as i32),
        InfoLine::Condition(k) => ("editor.msg", editor::CONDITIONS[k.min(7)]),
        InfoLine::Derived(k) => ("editor.msg", editor::DERIVED[k.min(9)].0),
        InfoLine::CharacterPoints => ("editor.msg", 116),
        InfoLine::OptionalTraits => ("editor.msg", 139),
        InfoLine::SkillsCard => ("editor.msg", 117),
        InfoLine::TagSkillsCard => ("editor.msg", 138),
    };
    let title = painter.message(file, id);
    painter.text(image, 102, ColorKey::BLACK, &title, [348, 272], 266);
    image.fill_rect([348, 300, 266, 2], painter.color(ColorKey::BLACK));
    let points = painter.message("editor.msg", 116);
    painter.text(image, 103, ColorKey::LABEL, &points, [14, 286], 120);
    for (cell, x) in editor::big_number_cells(editor.character_points, editor.character_points)
        .iter()
        .zip([126, 140])
    {
        painter.big_digit(image, *cell, [x, 282], false);
    }
    let caption = painter.message("editor.msg", 139);
    painter.text(image, 103, ColorKey::LABEL, &caption, [52, 326], 160);
    for k in 0..16 {
        let (x, row) = if k < 8 { (47, k) } else { (199, k - 8) };
        let picked = editor.sheet.traits.contains(&k);
        let key = row_key(picked, editor.selected == InfoLine::Trait(k as usize));
        let text = painter.message("trait.msg", 100 + k);
        painter.text(
            image,
            101,
            key,
            &text,
            [x, editor::trait_label_y(row as usize)],
            120,
        );
    }
    let skills = painter.message("editor.msg", 117);
    painter.text(image, 103, ColorKey::LABEL, &skills, [380, 5], 200);
    for k in 0..18 {
        let tagged = editor.sheet.tags.contains(&(k as i32));
        let key = row_key(tagged, editor.selected == InfoLine::Skill(k));
        let name = painter.message("skill.msg", 100 + k as i32);
        let y = editor::skill_y(k);
        painter.text(image, 101, key, &name, [380, y], 190);
        let value = format!("{}%", editor.sheet.skill(k, context)).into_bytes();
        painter.text(image, 101, key, &value, [573, y], 50);
        painter.blit(
            image,
            if tagged {
                "tgsklon.frm"
            } else {
                "tgskloff.frm"
            },
            0,
            [347, 26 + 11 * k as i32],
        );
    }
    let tags = painter.message("editor.msg", 138);
    painter.text(image, 103, ColorKey::LABEL, &tags, [422, 233], 100);
    for (cell, x) in editor::big_number_cells(editor.tag_picks, editor.tag_picks)
        .iter()
        .zip([522, 536])
    {
        painter.big_digit(image, *cell, [x, 228], false);
    }
    for ((button, caption), id) in editor::OPTIONS_BUTTONS
        .iter()
        .zip([[363, 454], [475, 454], [571, 454]])
        .zip([101, 100, 102])
    {
        painter.blit(image, "lilredup.frm", 0, *button);
        let text = painter.message("editor.msg", id);
        painter.text(image, 103, ColorKey::LABEL, &text, caption, 80);
    }
    if let Some(field) = &stage.name {
        painter.blit_opaque(image, "charwin.frm", 0, [17, 0]);
        painter.blit_opaque(image, "namebox.frm", 0, [17 + 13, 13]);
        painter.text(image, 101, ColorKey::GREEN, &field.text, [17 + 23, 19], 100);
    }
    if let Some(refusal) = stage.refusal {
        let [ox, oy] = editor::DIALOG_ORIGIN;
        painter.blit_opaque(image, flow::DialogSize::Large.art().0, 0, [ox, oy]);
        let title = painter.message("editor.msg", refusal.title);
        let body = painter.message("editor.msg", refusal.body);
        painter.centered(
            image,
            101,
            editor::DIALOG_COLOR,
            &title,
            [ox, oy + flow::dialog_title_y(1)],
            302,
        );
        painter.centered(
            image,
            101,
            editor::DIALOG_COLOR,
            &body,
            [ox, oy + flow::dialog_title_y(1) + 10],
            302,
        );
    }
}

/// SPEC-014 §3.8 skill and trait rows: untagged 992 (selected 32747),
/// tagged or picked 21140 (selected 32767).
fn row_key(marked: bool, selected: bool) -> ColorKey {
    match (marked, selected) {
        (false, false) => ColorKey::GREEN,
        (false, true) => ColorKey::SELECTED,
        (true, false) => ColorKey::GREY,
        (true, true) => ColorKey::WHITE,
    }
}
