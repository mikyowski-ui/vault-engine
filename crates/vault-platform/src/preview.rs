use crate::AppResult;
use crate::enhanced::{EnhancedFrame, EnhancedSettings, PointerAt, PointerButton};
use std::path::Path;
use std::{path::PathBuf, sync::Arc};
use vault_formats::{
    DatArchive, DatProvider, Database, Font, FrmArchive, LayerEntry, LayeredVfs, LoadOrderPolicy,
    LocalDirProvider, MAX_ASSET_BYTES, MsgCatalog, Palette, ParseError, PrefixMount, Vfs, VfsPath,
};
use vault_render::{ImageError, IndexedImage, compose_preview};
use winit::keyboard::KeyCode;

#[derive(clap::Args, Debug, Default, Clone)]
pub struct PreviewArgs {
    /// The engine preset (from the settings or `--preset`; Vanilla when
    /// neither names one).
    #[arg(skip)]
    pub preset: vault_core::EnginePreset,
    /// The game whose data this is (from the settings or
    /// `--game-profile`); Fallout 2 is refused before anything is read.
    #[arg(skip)]
    pub game_profile: vault_core::GameProfile,
    /// Mod layers stacked over the game data, lowest first (the settings'
    /// `mod_list`): paths, or a `load_order.toml`.
    #[arg(skip)]
    pub mods: Option<crate::config::ModList>,
    /// The `enhanced` preset's presentation settings (from the settings,
    /// the launcher or `--viewport`/`--zoom`); `None` under Vanilla, and
    /// in a replay, which applies the logged ones.
    #[arg(skip)]
    pub enhanced: Option<EnhancedSettings>,
    /// Where the F9 overlay exports the current setup as a mod (`None`: it
    /// does not, as in a replay).
    #[arg(skip)]
    pub mod_export: Option<PathBuf>,
    /// Mount a loose directory: the whole asset source, or with DAT
    /// archives the loose patch root both databases consult first.
    #[arg(long, requires = "preview_asset", value_name = "PATH")]
    pub corpus_dir: Option<PathBuf>,
    /// SPEC-008 master database archive (MASTER.DAT); requires --critter-dat.
    #[arg(
        long,
        requires = "critter_dat",
        requires = "preview_asset",
        value_name = "PATH"
    )]
    pub master_dat: Option<PathBuf>,
    /// SPEC-008 critter database archive (CRITTER.DAT), selected for
    /// `art/critters/` requests; requires --master-dat.
    #[arg(long, requires = "master_dat", value_name = "PATH")]
    pub critter_dat: Option<PathBuf>,
    /// Sprite path relative to the case-insensitive VFS root.
    #[arg(long, group = "preview_asset", value_name = "VFS_PATH")]
    pub preview_frm: Option<String>,
    /// Map path relative to the VFS root; arrows scroll, R toggles roofs,
    /// PageUp/PageDown change the elevation.
    #[arg(
        long,
        group = "preview_asset",
        conflicts_with = "preview_frm",
        value_name = "VFS_PATH"
    )]
    pub preview_map: Option<String>,
    /// SPEC-012: play one `.MVE` (with its `.cfg` fades, and subtitles with
    /// --subtitles), then exit. Any key, or a click, skips it.
    #[arg(
        long,
        group = "preview_asset",
        conflicts_with_all = ["preview_frm", "preview_map"],
        value_name = "VFS_PATH"
    )]
    pub play_movie: Option<String>,
    /// SPEC-014 game flow: splash, start movies, main menu, character
    /// selection and creation, then the world (requires the full corpus).
    #[arg(
        long,
        group = "preview_asset",
        conflicts_with_all = ["preview_frm", "preview_map", "play_movie"]
    )]
    pub game: bool,
    /// Directory holding `SAVEGAME/SLOTnn` (default: the current directory).
    #[arg(long, value_name = "PATH")]
    pub save_dir: Option<PathBuf>,
    /// `fallout.cfg` read for preferences and written when they change.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// SPEC-012 subtitles preference (the boil movies always show theirs).
    #[arg(long)]
    pub subtitles: bool,
    /// Play the startup movies (iplogo, then intro) before the map.
    #[arg(long, requires = "preview_map")]
    pub startup_movies: bool,
    /// Player sheet for interactive map preview (otherwise a neutral default character).
    #[arg(long, requires = "preview_map", value_name = "VFS_PATH")]
    pub player_gcd: Option<String>,
    /// Dialogue and subtitle language directory; defaults to english.
    #[arg(long)]
    pub language: Option<String>,
    /// Deterministic game RNG seed; defaults to 1. -1 asks for an ambient,
    /// nondeterministic seed (SPEC-006 §3.4).
    #[arg(long, requires = "preview_map", allow_negative_numbers = true)]
    pub seed: Option<i32>,
    /// Font path (default: font0.aaf, font0.fon, then another root font).
    #[arg(long, requires = "preview_frm", value_name = "VFS_PATH")]
    pub font: Option<String>,
    /// Message catalog (default: first .msg below text/ in VFS order).
    #[arg(long, requires = "preview_frm", value_name = "VFS_PATH")]
    pub msg: Option<String>,
    /// Message ID (default: lowest ID with nonempty text).
    #[arg(long, requires = "preview_frm", allow_negative_numbers = true)]
    pub message_id: Option<i32>,
    /// Start the vault-debug inspector in the interactive world: GVAR, MVAR
    /// and LVAR watches and a live script trace in an overlay, also logged
    /// at debug level (target `vault_debug`). F9 toggles it during a run.
    /// With --game it also enables the map preview's keys (W world map,
    /// PageUp/PageDown elevation, L/D/U/G/T actions on the selection).
    #[arg(long)]
    pub debug: bool,
    /// Set by a headless replay: the world's movies complete without
    /// playback, including the startup movies queued while loading.
    #[arg(skip)]
    pub headless: bool,
}

pub struct Preview {
    archive: FrmArchive,
    pub palette: Palette,
    font: Font,
    message: Vec<u8>,
    path: String,
    direction: usize,
    frame: usize,
}

pub enum AssetPreview {
    Sprite(Box<Preview>),
    Map(Box<crate::map_preview::MapPreview>),
    Movie(Box<MoviePreview>),
    Game(Box<crate::game::Game>),
}

/// A standalone movie and the black palette shown around it.
pub struct MoviePreview {
    pub player: crate::movie::MoviePlayer,
    black: Palette,
}

impl AssetPreview {
    /// The standalone sprite and map preview tools exit on an Escape they
    /// do not consume. The game never exits on a key (G-01): it leaves
    /// through its own menus only.
    pub fn escape_exits(&self) -> bool {
        matches!(self, Self::Sprite(_) | Self::Map(_))
    }
    /// The keyboard state for the engine key repeat (SPEC-014 §3.29).
    pub fn keyboard(&self) -> crate::input::Keyboard {
        match self {
            Self::Sprite(_) | Self::Movie(_) => crate::input::Keyboard::default(),
            Self::Map(preview) => preview.keyboard(),
            Self::Game(game) => game.keyboard(),
        }
    }
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        match self {
            Self::Sprite(preview) => Ok(preview.image()?),
            Self::Map(preview) => preview.image(),
            Self::Movie(movie) => Ok(movie.player.image()),
            Self::Game(game) => game.image(),
        }
    }
    /// The active palette: a script-installed table replaces the map's.
    pub fn palette(&self) -> &Palette {
        match self {
            Self::Sprite(preview) => &preview.palette,
            Self::Map(preview) => preview
                .host
                .library
                .palette
                .as_deref()
                .unwrap_or(&preview.palette),
            Self::Movie(movie) => &movie.black,
            Self::Game(game) => game.palette(),
        }
    }
    /// A movie's device palette replaces the display palette while it plays.
    pub fn display_override(&self) -> Option<[[u8; 4]; 256]> {
        match self {
            Self::Sprite(_) => None,
            Self::Map(preview) => preview.display_override(),
            Self::Movie(movie) => Some(movie.player.palette(1.0)),
            Self::Game(game) => game.display_override(),
        }
    }
    /// A standalone movie that ended, a confirmed quit from a map preview,
    /// or the game's Exit (the shell then exits).
    pub fn finished(&self) -> bool {
        match self {
            Self::Movie(movie) => movie.player.finished(),
            Self::Map(preview) => preview.quit_requested,
            Self::Game(game) => game.finished(),
            Self::Sprite(_) => false,
        }
    }
    /// SPEC-014 §3.18 brightness: the display gamma.
    pub fn brightness(&self) -> f64 {
        match self {
            Self::Map(preview) => preview.preferences.brightness,
            Self::Game(game) => game.brightness(),
            _ => 1.0,
        }
    }
    /// Held modifier keys: shift, control, alt.
    pub fn set_modifiers(&mut self, modifiers: [bool; 3]) {
        match self {
            Self::Map(preview) => preview.set_modifiers(modifiers),
            Self::Game(game) => game.set_modifiers(modifiers),
            _ => {}
        }
    }
    pub fn palette_generation(&self) -> u32 {
        match self {
            Self::Sprite(_) | Self::Movie(_) => 0,
            Self::Map(preview) => preview.host.library.palette_generation,
            Self::Game(game) => game.palette_generation(),
        }
    }
    /// Whether the previewed world currently permits palette cycling.
    pub fn palette_cycling(&self) -> bool {
        match self {
            Self::Sprite(_) => true,
            // SPEC-012 §3.12: movies disable color cycling (and so do some
            // windows, `MapPreview::palette_cycling`).
            Self::Map(preview) => preview.palette_cycling(),
            Self::Movie(_) => false,
            Self::Game(game) => game.palette_cycling(),
        }
    }
    pub fn title(&self) -> String {
        match self {
            Self::Sprite(preview) => preview.title(),
            Self::Map(preview) => preview.title(),
            Self::Movie(movie) => format!(
                "Vault — movie {} — any key or a click skips",
                movie.player.request.path
            ),
            Self::Game(game) => game.window_title(),
        }
    }
    /// A key went up (SPEC-014 §3.27 holds releases during the combat lock).
    pub fn key_up(&mut self, key: KeyCode) {
        match self {
            Self::Map(preview) => preview.key_up(key),
            Self::Game(game) => game.key_up(key),
            _ => {}
        }
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        match self {
            Self::Sprite(preview) => preview.step(key),
            Self::Map(preview) => preview.step(key),
            // SPEC-014 §3.29: a modifier key alone is no key: it skips nothing.
            Self::Movie(_) if crate::input::is_modifier(key) => false,
            Self::Movie(movie) => {
                movie.player.key();
                true
            }
            Self::Game(game) => game.step(key),
        }
    }
    /// The pointer moved: its frame pixel, clamped to the frame, and
    /// whether it lies outside the frame (H3: a letterbox pointer hovers
    /// and clicks at the clamped pixel but is never an edge).
    pub fn pointer_moved(
        &mut self,
        point: vault_render::FramePoint,
    ) -> Option<vault_render::map::MapHover> {
        match self {
            Self::Map(preview) => preview.set_pointer_outside(point.outside),
            Self::Game(game) => {
                if let Some(world) = game.world() {
                    world.set_pointer_outside(point.outside);
                }
            }
            _ => {}
        }
        self.hover(point.pixel)
    }
    /// M-P6: the window lost the focus; held presses are dropped.
    pub fn focus_lost(&mut self) {
        match self {
            Self::Map(preview) => preview.focus_lost(),
            Self::Game(game) => {
                if let Some(world) = game.world() {
                    world.focus_lost();
                }
            }
            _ => {}
        }
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<vault_render::map::MapHover> {
        match self {
            Self::Sprite(_) | Self::Movie(_) => None,
            Self::Map(preview) => preview.hover(point),
            Self::Game(game) => game.hover(point),
        }
    }
    pub fn click(&mut self, point: [i32; 2], right: bool) -> bool {
        match self {
            Self::Sprite(_) => false,
            Self::Map(preview) => preview.click(point, right),
            Self::Movie(movie) => {
                movie.player.press();
                true
            }
            Self::Game(game) => game.click(point, right),
        }
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        match self {
            Self::Sprite(_) | Self::Movie(_) => false,
            Self::Map(preview) => preview.scroll(delta),
            Self::Game(game) => game.scroll(delta),
        }
    }
    /// A mouse release at a frame pixel (`None` outside the frame): drags
    /// drop, map clicks act, and a held desktop button plays its release
    /// cue.
    pub fn release(&mut self, point: Option<[i32; 2]>, right: bool) -> bool {
        match self {
            Self::Sprite(_) => false,
            Self::Map(preview) => preview.release(point, right),
            Self::Movie(movie) => {
                movie.player.release();
                true
            }
            Self::Game(game) => game.release(point, right),
        }
    }
    /// SPEC-012 §3.12: a mouse button the frame-pixel paths do not see (a
    /// middle or extra button, or a press in the letterbox) still counts
    /// while a movie plays. Returns whether a movie took it.
    pub fn movie_button(&mut self, pressed: bool) -> bool {
        match self {
            Self::Movie(movie) => {
                if pressed {
                    movie.player.press();
                } else {
                    movie.player.release();
                }
                true
            }
            Self::Map(preview) => preview.movie_button(pressed),
            Self::Game(game) => game.movie_button(pressed),
            Self::Sprite(_) => false,
        }
    }
    /// The pointer left the window.
    /// A frame pixel the system pointer is to be moved to (SPEC-014 §3.28
    /// action-menu release).
    pub fn take_warp(&mut self) -> Option<[i32; 2]> {
        match self {
            Self::Map(preview) => preview.take_warp(),
            Self::Game(game) => game.take_warp(),
            _ => None,
        }
    }
    pub fn pointer_left(&mut self) {
        match self {
            Self::Map(preview) => preview.pointer_left(),
            Self::Game(game) => game.pointer_left(),
            _ => {}
        }
    }
    /// The last frame with only the drawn cursor moved, when the preview
    /// draws its own cursor.
    pub fn pointer_frame(&mut self) -> Option<IndexedImage> {
        match self {
            Self::Map(preview) => preview.pointer_frame(),
            Self::Game(game) => game.world()?.pointer_frame(),
            _ => None,
        }
    }
    /// Whether the system cursor shows: an interactive world draws its own.
    /// SPEC-014 §3.18 the applied `mouse_sensitivity`; 1.0 where no game
    /// world takes the pointer.
    pub fn mouse_sensitivity(&mut self) -> f64 {
        match self {
            Self::Map(preview) => preview.applied_preferences().mouse_sensitivity,
            Self::Game(game) => game
                .world()
                .map_or(1.0, |world| world.applied_preferences().mouse_sensitivity),
            _ => 1.0,
        }
    }
    pub fn system_cursor(&self) -> bool {
        match self {
            Self::Map(_) => false,
            Self::Game(game) => !game.world_shown(),
            _ => true,
        }
    }
    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        match self {
            Self::Sprite(_) => Ok(false),
            Self::Map(preview) => preview.tick(now),
            Self::Movie(movie) => Ok(movie.player.tick(now)),
            Self::Game(game) => game.tick(now),
        }
    }

    // The `enhanced` presentation, for a map preview or the game's current
    // world; the other previews (and the game outside a world) keep the
    // Vanilla frame.

    /// Select the enhanced presentation (`None` is Vanilla). The game
    /// keeps it for every world it creates next, and gives it to the
    /// current one.
    pub fn set_enhanced(&mut self, settings: Option<EnhancedSettings>) {
        match self {
            Self::Map(preview) => preview.set_enhanced(settings),
            Self::Game(game) => {
                game.enhanced = settings;
                if let Some(world) = game.world() {
                    world.set_enhanced(settings);
                }
            }
            _ => {}
        }
    }
    /// The world's enhanced settings, when it shows the enhanced
    /// presentation.
    pub fn enhanced_settings(&mut self) -> Option<EnhancedSettings> {
        match self {
            Self::Map(preview) => preview.enhanced(),
            Self::Game(game) => game.world()?.enhanced(),
            _ => None,
        }
    }
    /// The world zoom the next enhanced frame is laid out with.
    pub fn enhanced_zoom(&mut self) -> f32 {
        match self {
            Self::Map(preview) => preview.enhanced_zoom(),
            Self::Game(game) => game.world().map_or(1.0, |world| world.enhanced_zoom()),
            _ => 1.0,
        }
    }
    /// The enhanced frame for `layout`, or `None` for the Vanilla frame.
    pub fn enhanced_frame(
        &mut self,
        layout: &vault_render::EnhancedLayout,
    ) -> Option<AppResult<EnhancedFrame>> {
        match self {
            Self::Map(preview) => preview.enhanced_frame(layout),
            Self::Game(game) => game.world()?.enhanced_frame(layout),
            _ => None,
        }
    }
    /// The pointer moved; returns whether the frame changed.
    pub fn enhanced_pointer(&mut self, at: PointerAt) -> bool {
        match self {
            Self::Map(preview) => preview.enhanced_pointer(at),
            Self::Game(game) => game.world().is_some_and(|world| world.enhanced_pointer(at)),
            _ => false,
        }
    }
    /// A pointer button; returns whether the frame changed.
    pub fn enhanced_button(&mut self, at: PointerAt, button: PointerButton, pressed: bool) -> bool {
        match self {
            Self::Map(preview) => preview.enhanced_button(at, button, pressed),
            Self::Game(game) => game
                .world()
                .is_some_and(|world| world.enhanced_button(at, button, pressed)),
            _ => false,
        }
    }
    /// Wheel lines (positive away from the user); returns whether the
    /// frame changed.
    pub fn enhanced_wheel(&mut self, at: PointerAt, lines: f64) -> bool {
        match self {
            Self::Map(preview) => preview.enhanced_wheel(at, lines),
            Self::Game(game) => game
                .world()
                .is_some_and(|world| world.enhanced_wheel(at, lines)),
            _ => false,
        }
    }
}

fn parse_asset<T>(
    vfs: &dyn Vfs,
    path: &str,
    parser: impl FnOnce(&[u8]) -> Result<T, ParseError>,
) -> AppResult<T> {
    let bytes = vfs.read_bounded(path, MAX_ASSET_BYTES)?;
    parser(&bytes).map_err(|error| format!("{path}: {error}").into())
}

impl PreviewArgs {
    /// The game RNG seed: `--seed`, else 1. SPEC-006 §3.4: -1 asks for an
    /// ambient host-generated seed, two host draws combined as
    /// `first x 65536 + second` keeping the low 31 bits (the host entropy
    /// source is not part of the contract), which the generator then seeds
    /// like any other value.
    pub fn resolved_seed(&self) -> i32 {
        match self.seed {
            Some(-1) => {
                use std::hash::{BuildHasher, Hasher};
                let draw = || {
                    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                    hasher.write_u8(0);
                    hasher.finish() as u32
                };
                let [first, second] = [draw(), draw()];
                (first.wrapping_mul(65536).wrapping_add(second) & 0x7fff_ffff) as i32
            }
            seed => seed.unwrap_or(1),
        }
    }
    /// The asset namespace: SPEC-008 master and critter databases (each
    /// consulting the optional loose root first, with no fallback to the
    /// other archive, and each opened lazily: only its directory tables are
    /// read, and members stream from the file on demand), or a loose
    /// directory alone.
    pub fn source(&self) -> AppResult<Arc<dyn Vfs>> {
        let base = self.base_source()?;
        match &self.mods {
            Some(mods) => Self::mount_mods(base, mods),
            None => Ok(base),
        }
    }
    /// `mods` mounted over `base`: a `load_order.toml`, or the settings'
    /// path list. The mod layers stay outermost, so the world sees them
    /// counted.
    pub fn mount_mods(
        base: Arc<dyn Vfs>,
        mods: &crate::config::ModList,
    ) -> AppResult<Arc<dyn Vfs>> {
        let layered = match mods {
            crate::config::ModList::File(path) => LayeredVfs::from_load_order(base, path),
            // Paths from the settings are the user's own (already resolved
            // against the settings file), so they may lie anywhere.
            crate::config::ModList::Paths(paths) => {
                let entries = paths
                    .iter()
                    .map(|path| LayerEntry {
                        path: Some(path.to_string_lossy().into_owned()),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>();
                LayeredVfs::from_entries(
                    base,
                    Path::new("."),
                    &entries,
                    LoadOrderPolicy {
                        allow_outside: true,
                    },
                )
            }
        }
        .map_err(|error| format!("mod_list: {error}"))?;
        tracing::info!(layers = layered.layers().len(), "mod layers mounted");
        Ok(Arc::new(layered))
    }
    /// The game data without mod layers.
    fn base_source(&self) -> AppResult<Arc<dyn Vfs>> {
        let loose = self
            .corpus_dir
            .as_ref()
            .map(LocalDirProvider::mount)
            .transpose()?
            .map(Arc::new);
        let (Some(master), Some(critter)) = (&self.master_dat, &self.critter_dat) else {
            let loose =
                loose.ok_or("a preview requires --corpus-dir or --master-dat/--critter-dat")?;
            return Ok(loose);
        };
        // SPEC-018 §1-§2: Fallout 2's databases over SPEC-015 archives, found
        // in the install folder that holds MASTER.DAT, with an optional
        // patch000.dat above MASTER.DAT.
        if self.game_profile == vault_core::GameProfile::Fallout2 {
            let _ = critter;
            let folder = vault_formats::fallout2::Fallout2Install::folder_of(master);
            let install = vault_formats::fallout2::Fallout2Install::discover(&folder)?;
            if let Some(patch) = &install.patch {
                tracing::info!(file = %patch.display(), "patch000.dat mounted");
            }
            let provider = install.open()?.provider(loose);
            // §2.2: Play needs the required members (map preview does not).
            if self.game
                && let Some(member) = vault_formats::fallout2::missing_members(&provider).first()
            {
                return Err(format!("the Fallout 2 install is missing {member}").into());
            }
            tracing::info!(
                files = provider.files().count(),
                "Fallout 2 databases mounted"
            );
            return Ok(Arc::new(provider));
        }
        // Only the directory tables are read here; members stream from the
        // file on demand (SPEC-008 lookup, one member in memory at a time).
        let archive = |path: &PathBuf| -> AppResult<Arc<DatArchive>> {
            Ok(Arc::new(
                DatArchive::open(path).map_err(|error| format!("{}: {error}", path.display()))?,
            ))
        };
        let master = Database {
            loose: loose.clone(),
            archive: Some(archive(master)?),
        };
        let critter = Database {
            loose,
            archive: Some(archive(critter)?),
        };
        let provider = DatProvider::new(master, critter);
        tracing::info!(files = provider.files().count(), "DAT databases mounted");
        let provider: Arc<dyn Vfs> = Arc::new(provider);
        // Loose `.ACM` music ships beside the archives, not inside them.
        let Some(music) = self.master_dat.as_deref().and_then(music_dir) else {
            return Ok(provider);
        };
        match LocalDirProvider::mount(&music)
            .and_then(|dir| PrefixMount::new(provider.clone(), MUSIC_DIR, dir))
        {
            Ok(mount) => {
                tracing::info!(dir = %music.display(), files = mount.mounted(), "music mounted");
                Ok(Arc::new(mount))
            }
            Err(error) => {
                tracing::warn!(dir = %music.display(), %error, "music directory not mounted");
                Ok(provider)
            }
        }
    }
    pub fn load(&self) -> AppResult<Option<AssetPreview>> {
        let language = self.language.as_deref().unwrap_or("english");
        let save_root = self.save_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        if self.game {
            let mut game = crate::game::Game::new(
                self.source()?,
                language,
                self.resolved_seed(),
                save_root,
                self.config.clone(),
            )?;
            // SPEC-018 §6: a Fallout 2 game's new-game data and hand-off.
            game.game_profile = self.game_profile;
            game.headless = self.headless;
            game.debug = self.debug;
            game.preset = self.preset;
            game.enhanced = self.enhanced;
            game.mod_export = self.mod_export.clone();
            // G-09: the preview and debug keys work in the game only with
            // the explicit flag.
            game.debug_keys = self.debug;
            return Ok(Some(AssetPreview::Game(Box::new(game))));
        }
        if let Some(path) = &self.preview_map {
            let vfs = self.source()?;
            let mut map = crate::map_preview::MapPreview::load_for_game(
                vfs,
                path,
                language,
                self.player_gcd.as_deref(),
                self.resolved_seed(),
                self.preset,
                self.game_profile,
            )?;
            map.host.set_subtitles(self.subtitles);
            map.save_root = save_root;
            map.config_path = self.config.clone();
            if self.headless {
                map.host.movie_policy = vault_core::movie::MoviePolicy::CompleteImmediately;
            }
            if self.startup_movies {
                map.queue_startup_movies();
            }
            map.set_debug(self.debug);
            map.set_enhanced(self.enhanced);
            map.mod_export = self.mod_export.clone();
            map.debug_session = self.debug;
            return Ok(Some(AssetPreview::Map(Box::new(map))));
        }
        if let Some(path) = &self.play_movie {
            return Ok(Some(AssetPreview::Movie(Box::new(
                self.load_movie(path, language)?,
            ))));
        }
        Ok(self
            .load_sprite()?
            .map(|preview| AssetPreview::Sprite(Box::new(preview))))
    }

    /// A standalone movie: its table index (for the boil movies' forced
    /// subtitles), fade file, and subtitles under `language`.
    fn load_movie(&self, path: &str, language: &str) -> AppResult<MoviePreview> {
        let vfs = self.source()?;
        let file = VfsPath::new(path)?.to_string();
        let windows = file.replace('/', "\\");
        let index = vault_core::movie::GAME_MOVIES
            .iter()
            .position(|movie| file.rsplit('/').next() == Some(movie.file));
        let exists =
            |path: &str| VfsPath::new(path).is_ok_and(|path| vfs.files().any(|f| *f == path));
        let subtitles =
            Some(vault_core::movie::subtitle_path(language, &windows)).filter(|subtitles| {
                (self.subtitles || index.is_some_and(vault_core::movie::subtitles_forced))
                    && exists(subtitles)
            });
        let fades = Some(vault_core::movie::fade_path(&windows)).filter(|fades| exists(fades));
        let style = subtitles
            .as_ref()
            .and_then(|_| crate::movie::SubtitleStyle::load(&*vfs, None));
        let request = vault_core::movie::MovieRequest {
            index,
            flags: 0,
            path: windows,
            subtitles,
            fades,
        };
        Ok(MoviePreview {
            player: crate::movie::MoviePlayer::load(&*vfs, request, style)?,
            black: Palette::parse(&[0; 0x8300])?,
        })
    }

    fn load_sprite(&self) -> AppResult<Option<Preview>> {
        let Some(path) = &self.preview_frm else {
            return Ok(None);
        };
        let vfs = self.source()?;
        let vfs = &*vfs;
        let path = VfsPath::new(path)?;
        let direction = match path.extension() {
            Some("frm") => 0,
            Some(extension @ ("fr0" | "fr1" | "fr2" | "fr3" | "fr4" | "fr5")) => {
                usize::from(extension.as_bytes()[2] - b'0')
            }
            _ => return Err("--preview-frm must name a .frm or .fr0–.fr5 asset".into()),
        };
        let palette = parse_asset(vfs, "color.pal", Palette::parse)?;
        let archive = parse_asset(vfs, path.as_str(), FrmArchive::parse)?;
        let font_path = self
            .font
            .clone()
            .or_else(|| {
                vfs.files()
                    .filter(|path| {
                        !path.as_str().contains('/')
                            && matches!(path.extension(), Some("fon" | "aaf"))
                    })
                    .min_by_key(|path| match path.as_str() {
                        "font0.aaf" => 0,
                        "font0.fon" => 1,
                        _ if path.extension() == Some("aaf") => 2,
                        _ => 3,
                    })
                    .map(ToString::to_string)
            })
            .ok_or("no root .fon/.aaf font found; select one with --font")?;
        let font_path = VfsPath::new(&font_path)?;
        let font = match font_path.extension() {
            Some("fon") => parse_asset(vfs, font_path.as_str(), Font::parse_fon)?,
            Some("aaf") => parse_asset(vfs, font_path.as_str(), Font::parse_aaf)?,
            _ => return Err("--font must name a .fon or .aaf asset".into()),
        };
        let msg_path = self
            .msg
            .clone()
            .or_else(|| {
                vfs.files()
                    .find(|path| {
                        path.as_str().starts_with("text/") && path.extension() == Some("msg")
                    })
                    .map(ToString::to_string)
            })
            .ok_or("no message catalog below text/; select one with --msg")?;
        let catalog = parse_asset(vfs, &msg_path, MsgCatalog::parse)?;
        let id = self
            .message_id
            .or_else(|| {
                catalog
                    .entries()
                    .iter()
                    .find(|(_, message)| !message.text.is_empty())
                    .map(|(id, _)| *id)
            })
            .ok_or("message catalog has no nonempty text; select an ID with --message-id")?;
        let message = catalog
            .get(id)
            .ok_or_else(|| format!("{msg_path}: message ID {id} not found"))?
            .text
            .clone();
        tracing::info!(sprite = %path, font = %font_path, catalog = %msg_path, message_id = id,
            direction, frames = archive.frame_count(), "preview assets loaded");
        Ok(Some(Preview {
            archive,
            palette,
            font,
            message,
            path: path.to_string(),
            direction,
            frame: 0,
        }))
    }
}

impl Preview {
    pub fn image(&self) -> Result<IndexedImage, ImageError> {
        compose_preview(
            &self.archive,
            self.direction,
            self.frame,
            &self.palette,
            &self.font,
            &self.message,
        )
    }
    pub fn title(&self) -> String {
        format!(
            "Vault — {} — orientation {}/5, frame {}/{} — ←/→ orientation, ↑/↓ frame",
            self.path,
            self.direction,
            self.frame,
            self.archive.frame_count() - 1
        )
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::ArrowLeft => self.direction = (self.direction + 5) % 6,
            KeyCode::ArrowRight => self.direction = (self.direction + 1) % 6,
            KeyCode::ArrowUp => self.frame = (self.frame + 1) % self.archive.frame_count(),
            KeyCode::ArrowDown => {
                self.frame =
                    (self.frame + self.archive.frame_count() - 1) % self.archive.frame_count()
            }
            _ => return false,
        }
        true
    }
}

/// The VFS directory music tracks are requested from (SPEC-008 §3.10).
const MUSIC_DIR: &str = "sound/music";

/// A loose music directory beside the master database: `sound/music` or
/// `data/sound/music`, matched without regard to case.
pub(crate) fn music_dir(master: &Path) -> Option<PathBuf> {
    let base = match master.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let child = |dir: &Path, name: &str| -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|entry| entry.eq_ignore_ascii_case(name))
                    && entry.path().is_dir()
            })
            .map(|entry| entry.path())
    };
    [&["sound", "music"][..], &["data", "sound", "music"]]
        .iter()
        .find_map(|names| {
            names
                .iter()
                .try_fold(base.clone(), |dir, name| child(&dir, name))
        })
}

#[cfg(test)]
use crate::fixture_assets as fixtures;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec016_a_fallout2_install_previews_a_version_20_map_end_to_end() {
        let corpus = crate::map_preview::test_support::synthetic_corpus();
        let install = tempfile::tempdir().unwrap();
        let (master, critter) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
        let preview = |args: PreviewArgs| match args.load().unwrap() {
            Some(AssetPreview::Map(map)) => map,
            _ => panic!("a map preview"),
        };
        let map = "maps/synthetic.map".to_owned();
        let fallout2 = PreviewArgs {
            game_profile: vault_core::GameProfile::Fallout2,
            master_dat: Some(master.clone()),
            critter_dat: Some(critter.clone()),
            preview_map: Some(map.clone()),
            headless: true,
            ..Default::default()
        };
        let mut fallout2 = preview(fallout2);
        assert_eq!(fallout2.host.world.map.header.version, 20);
        // Its critters read the 416-byte layout.
        let critter_pid = fallout2
            .host
            .world
            .iter()
            .find(|(reference, object)| {
                object.category() == vault_formats::pro::Category::Critter
                    && *reference != fallout2.host.world.player
            })
            .map(|(_, object)| object.raw.pid)
            .unwrap();
        let prototype =
            vault_core::assets::WorldAssets::prototype(&fallout2.host.assets, critter_pid).unwrap();
        let vault_formats::pro::PrototypeData::Critter(data) = &prototype.data else {
            panic!("a critter prototype")
        };
        assert_eq!(data.damage_type, Some(2));
        // It renders as the same corpus does loose under Fallout 1.
        let mut fallout1 = preview(PreviewArgs {
            corpus_dir: Some(corpus.path().to_path_buf()),
            preview_map: Some(map.clone()),
            headless: true,
            ..Default::default()
        });
        assert_eq!(fallout1.host.world.map.header.version, 19);
        assert_eq!(
            fallout2.image().unwrap().pixels(),
            fallout1.image().unwrap().pixels()
        );
        // The other game's archives are refused; and playing Fallout 2
        // needs SPEC-018 §2.2's members, which a plain map corpus lacks.
        let wrong = PreviewArgs {
            master_dat: Some(master.clone()),
            critter_dat: Some(critter.clone()),
            preview_map: Some(map),
            ..Default::default()
        };
        assert!(wrong.load().is_err());
        let play = PreviewArgs {
            game_profile: vault_core::GameProfile::Fallout2,
            master_dat: Some(master),
            critter_dat: Some(critter),
            game: true,
            ..Default::default()
        };
        let error = play.load().err().unwrap().to_string();
        assert!(error.contains("missing maps/artemple.map"), "{error}");
    }

    /// A VFS that records every path read through it.
    struct Logged {
        inner: Arc<dyn Vfs>,
        reads: std::sync::Mutex<Vec<String>>,
    }
    impl Vfs for Logged {
        fn read(&self, path: &str) -> Result<Vec<u8>, vault_formats::VfsError> {
            self.reads.lock().unwrap().push(path.to_ascii_lowercase());
            self.inner.read(path)
        }
        fn read_bounded(
            &self,
            path: &str,
            max_bytes: usize,
        ) -> Result<Vec<u8>, vault_formats::VfsError> {
            self.reads.lock().unwrap().push(path.to_ascii_lowercase());
            self.inner.read_bounded(path, max_bytes)
        }
        fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
            self.inner.files()
        }
    }

    /// SPEC-018 §8.13 and the end-to-end Fallout 2 path: a synthetic
    /// install of SPEC-015 archives (SPEC-016 layouts, the SPEC-018 tables,
    /// a synthetic `artemple.map`) validates in the launcher, Play's start
    /// makes a new game on the start map with its scripts running (SPEC-017
    /// Revision 3 opens the gate), the world map is reachable, and save and
    /// load round-trip.
    #[test]
    fn spec018_a_fallout2_install_plays_end_to_end() {
        use vault_core::{GameProfile::Fallout2, fallout2, host_save::SaveInfo};
        use vault_fixtures::state as fixture;
        let corpus = crate::map_preview::test_support::synthetic_corpus();
        vault_fixtures::fallout2::write_tables(corpus.path());
        // The start map's script (`scripts.lst` row 3): its
        // `map_enter_p_proc` sets campaign global 3 to 2241.
        let enter = [
            fixture::push(3),
            fixture::push(2241),
            fixture::instruction(0x80c6),
        ]
        .concat();
        std::fs::write(
            corpus.path().join("scripts/f2enter.int"),
            fixture::program(&[], &[(b"map_enter_p_proc", fixture::returning(enter))]),
        )
        .unwrap();
        let list = corpus.path().join("scripts/scripts.lst");
        let mut rows = std::fs::read(&list).unwrap();
        rows.extend(b"f2enter.int # local_vars=0\n");
        std::fs::write(&list, rows).unwrap();
        let map = std::fs::read(corpus.path().join("maps/synthetic.map")).unwrap();
        std::fs::write(
            corpus.path().join("maps/artemple.map"),
            vault_fixtures::fallout2::named_map(map, "artemple.map", 3, 20100, 2),
        )
        .unwrap();
        let install = tempfile::tempdir().unwrap();
        let (master, critter) =
            vault_fixtures::dat2::fallout2_install(corpus.path(), install.path(), 2);
        // §2: the launcher accepts the install and Play unlocks.
        let saves = tempfile::tempdir().unwrap();
        let launcher = crate::launcher::Launcher::new(crate::launcher::Setup {
            config: crate::config::Config {
                game: Some(Fallout2),
                master_dat: Some(master.clone()),
                critter_dat: Some(critter.clone()),
                ..Default::default()
            },
            settings_path: None,
            save_dirs: vec![saves.path().to_path_buf()],
            start_dir: saves.path().to_path_buf(),
            problem: None,
        });
        assert_eq!(launcher.ready(), Ok(()));
        // §6.7: `--game` loads under Fallout 2.
        let args = PreviewArgs {
            game_profile: Fallout2,
            master_dat: Some(master.clone()),
            critter_dat: Some(critter.clone()),
            game: true,
            headless: true,
            save_dir: Some(saves.path().to_path_buf()),
            ..Default::default()
        };
        let source = args.source().unwrap();
        assert!(matches!(args.load().unwrap(), Some(AssetPreview::Game(_))));
        // The same Play start over a read-logging VFS.
        let logged = Arc::new(Logged {
            inner: source,
            reads: Default::default(),
        });
        let mut game = crate::game::Game::new(
            logged.clone(),
            "english",
            1,
            saves.path().to_path_buf(),
            None,
        )
        .unwrap();
        game.headless = true;
        game.game_profile = Fallout2;
        // n: the tables parse, then the selection; t takes the premade:
        // elder.mve is missing, so the world starts.
        assert!(game.step(KeyCode::KeyN));
        assert_eq!(game.last_error, None);
        assert!(game.step(KeyCode::KeyT));
        let world = game.world().expect("the Fallout 2 world");
        let host = &world.host;
        let player = host.world.get(host.world.player).unwrap();
        assert_eq!(
            (player.raw.hex, player.raw.elevation, player.raw.orientation),
            (Some(20100), 0, 2)
        );
        assert_eq!(host.world.map.header.map_number, 2);
        assert_eq!(host.world.map.header.version, 20);
        let state = host.world.fallout2.as_deref().unwrap();
        assert_eq!(state.position, [175, 125]);
        assert_eq!(state.map_flags, [0, 0, 3]);
        assert_eq!(host.world.clock.ticks, fallout2::START_TICKS);
        assert_eq!(host.world.game_date(), [7, 25, 2241]);
        // The scripts run: the map script was read with the Fallout 2 word
        // set and its `map_enter_p_proc` set the global.
        assert!(!host.scripts_gated());
        assert!(!host.programs.is_empty());
        let reads = logged.reads.lock().unwrap().clone();
        assert!(
            reads.iter().any(|path| path.ends_with("f2enter.int")),
            "{reads:?}"
        );
        assert_eq!(host.world.gvars.as_ref().unwrap()[3], 2241);
        assert!(
            host.programs
                .values()
                .all(|vm| vm.program.words().fallout2())
        );
        let critter = host
            .world
            .iter()
            .find(|(reference, object)| {
                object.category() == vault_formats::pro::Category::Critter
                    && *reference != host.world.player
            })
            .map(|(reference, _)| reference)
            .unwrap();
        let script = host
            .world
            .script_for(critter)
            .expect("the critter's script record");
        // The monitor never shows the gate's notice (it wraps over rows; a
        // message's first row starts with the bullet).
        world.image().unwrap();
        let squeeze = |text: &[u8]| {
            text.iter()
                .copied()
                .filter(|b| *b != b' ')
                .collect::<Vec<u8>>()
        };
        let notice = squeeze(fallout2::GATE_NOTICE.as_bytes());
        let messages = |world: &crate::map_preview::MapPreview| {
            let mut out: Vec<Vec<u8>> = Vec::new();
            for line in world.hud.monitor.visible() {
                match line.split_first() {
                    Some((&vault_core::ui::bar::BULLET, rest)) => out.push(squeeze(rest)),
                    Some(_) => {
                        if let Some(last) = out.last_mut() {
                            last.extend(squeeze(line));
                        }
                    }
                    None => {}
                }
            }
            out
        };
        let shown = messages(world);
        assert!(!shown.contains(&notice), "{shown:?}");
        // A save made now carries no gated mark, and the critter's record.
        let root = saves.path();
        world.host.save_game(root, 1, &SaveInfo::default()).unwrap();
        let slot = root.join("SAVEGAME/SLOT01");
        let central = std::fs::read(slot.join("SAVE.DAT")).unwrap();
        assert_eq!(central[0x74F7], 0);
        assert_eq!(&central[0x18..0x1C], &[0, 1, 0, 2]);
        assert_eq!(&central[0x74E3..0x74F3], b"VAULTENG F2SAVE1");
        assert_eq!(&central[0x71..0x73], &[0, 2]);
        assert_eq!(&central[0x73..0x80], b"ARTEMPLE.sav\0");
        let saved_map = std::fs::read(slot.join("ARTEMPLE.SAV")).unwrap();
        let lookup = |pid| {
            vault_core::assets::WorldAssets::prototype(&world.host.assets, pid)
                .map(|prototype| prototype.info())
                .map_err(|error| vault_formats::ParseError {
                    format: "PRO",
                    offset: 0,
                    message: error.to_string(),
                })
        };
        let parsed = vault_formats::MapFile::parse_for(&saved_map, &lookup, Fallout2).unwrap();
        let ids: Vec<u32> = parsed
            .scripts
            .iter()
            .flat_map(|category| category.active())
            .map(|record| record.id)
            .collect();
        assert!(ids.contains(&script), "{ids:?} lacks {script:#x}");
        let automap = std::fs::read(slot.join("AUTOMAP.SAV")).unwrap();
        assert!(automap.len() >= 12005);
        // An exit-grid transition (to map 1) loads Test Village, with no
        // notice; its area moves the party there.
        world.host.request_transition(vault_core::Transition {
            map: 1,
            elevation: -1,
            hex: -1,
            facing: -1,
        });
        assert!(world.host.process_transition().unwrap());
        assert_eq!(world.host.world.map.header.map_number, 1);
        world.image().unwrap();
        assert!(!messages(world).contains(&notice));
        // The world map is reachable: a transition to it, travel with no
        // encounter table on the way, and an area entrance back.
        world.host.request_transition(vault_core::Transition {
            map: -2,
            elevation: -1,
            hex: -1,
            facing: -1,
        });
        assert!(world.host.process_transition().unwrap());
        assert!(world.host.world.unloaded);
        world.host.fallout2_build_world_map(true).unwrap();
        world.host.fallout2_travel_to([300, 250]).unwrap();
        let mut outcome = vault_core::Fallout2Travel::Moving;
        for _ in 0..2000 {
            outcome = world.host.fallout2_travel_iteration().unwrap();
            if outcome != vault_core::Fallout2Travel::Moving {
                break;
            }
        }
        assert_eq!(outcome, vault_core::Fallout2Travel::Arrived);
        let state = world.host.world.fallout2.as_deref().unwrap();
        assert_eq!(state.position, [300, 250]);
        assert_ne!(state.discovery([6, 5]), 0);
        assert!(world.host.world.clock.ticks > fallout2::START_TICKS);
        // Dropping to terrain loads the desert map at its start point.
        world.host.fallout2_travel_to([10, 10]).unwrap();
        world.host.world.fallout2.as_deref_mut().unwrap().position = [10, 10];
        assert!(world.host.fallout2_drop_to_terrain().unwrap());
        assert_eq!(world.host.world.map.header.map_number, 0);
        let player = world.host.world.get(world.host.world.player).unwrap();
        assert_eq!(player.raw.hex, Some(19086));
        // Into Test Village through area 0's entrance 0 (the header's
        // default entrance).
        world.host.fallout2_enter_entrance(0, 0).unwrap();
        assert_eq!(world.host.world.map.header.map_number, 1);
        // Map 0 is excluded (saved=No): leaving it wrote no DESERT1.SAV.
        let working = &world.host.world.persistence.maps;
        assert!(!working.contains_key("DESERT1.SAV"), "{:?}", working.keys());
        assert!(working.contains_key("ARTEMPLE.SAV"));
        let before = world.host.world.fallout2.as_deref().unwrap().clone();
        let ticks = world.host.world.clock.ticks;
        // Save and load round-trip.
        world.host.save_game(root, 2, &SaveInfo::default()).unwrap();
        world.host.world.clock.set(ticks + 1000);
        world.host.world.fallout2.as_deref_mut().unwrap().map_flags[2] = 0;
        world.host.load_game(root, 2).unwrap();
        assert_eq!(world.host.world.clock.ticks, ticks);
        assert_eq!(world.host.world.map.header.map_number, 1);
        let after = world.host.world.fallout2.as_deref().unwrap();
        assert_eq!(after.save(), before.save());
        assert_eq!(after.position, before.position);
        // The load printed no notice.
        let effects: Vec<_> = world.host.world.effects.iter().cloned().collect();
        assert!(!effects.iter().any(|effect| matches!(effect,
            vault_core::WorldEffect::Message(text) if squeeze(text) == notice)));
        // SPEC-018 §7.2: a slot carrying the gated mark is refused now that
        // the gate is open, and nothing changes.
        let central = root.join("SAVEGAME/SLOT02/SAVE.DAT");
        let good = std::fs::read(&central).unwrap();
        let mut marked = good.clone();
        marked[0x74F7] = 1;
        std::fs::write(&central, marked).unwrap();
        assert!(world.host.load_game(root, 2).is_err());
        assert_eq!(world.host.world.map.header.map_number, 1);
        std::fs::write(&central, good).unwrap();
        world.host.load_game(root, 2).unwrap();
        // A Fallout 1 slot never loads under Fallout 2, nor a Fallout 2 slot
        // under Fallout 1.
        let mut fallout1 =
            crate::map_preview::test_support::load(corpus.path(), "maps/testvil.map");
        fallout1
            .host
            .save_game(root, 3, &SaveInfo::default())
            .unwrap();
        assert!(world.host.load_game(root, 3).is_err());
        assert!(fallout1.host.load_game(root, 2).is_err());
        // A slot saved with another campaign-global count is refused, and
        // nothing changes.
        let mut other = std::fs::read(root.join("SAVEGAME/SLOT02/SAVE.DAT")).unwrap();
        other[0x74F6] ^= 1;
        std::fs::write(root.join("SAVEGAME/SLOT02/SAVE.DAT"), other).unwrap();
        assert!(world.host.load_game(root, 2).is_err());
        assert_eq!(world.host.world.map.header.map_number, 1);
    }

    /// SPEC-018 §6.6 step 1: a table that §4 refuses returns to the main
    /// menu with a message naming the file and line; no game starts.
    #[test]
    fn spec018_a_refused_table_returns_to_the_menu() {
        let corpus = crate::map_preview::test_support::synthetic_corpus();
        vault_fixtures::fallout2::write_tables(corpus.path());
        std::fs::write(
            corpus.path().join("data/maps.txt"),
            "[Map 000]\nlookup_name=No File\n",
        )
        .unwrap();
        let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(corpus.path()).unwrap());
        let saves = tempfile::tempdir().unwrap();
        let mut game =
            crate::game::Game::new(vfs, "english", 1, saves.path().to_path_buf(), None).unwrap();
        game.headless = true;
        game.game_profile = vault_core::GameProfile::Fallout2;
        game.step(KeyCode::KeyN);
        let error = game.last_error.clone().expect("a refusal");
        assert_eq!(error, "data/maps.txt line 0: no valid map record");
        assert_eq!(game.title(), "Vault — main menu");
        assert!(game.world().is_none());
    }

    #[test]
    fn music_beside_the_master_database_is_found_in_either_layout() {
        let temp = tempfile::tempdir().unwrap();
        let master = temp.path().join("MASTER.DAT");
        assert_eq!(music_dir(&master), None);
        std::fs::create_dir_all(temp.path().join("DATA/SOUND/MUSIC")).unwrap();
        assert_eq!(
            music_dir(&master),
            Some(temp.path().join("DATA/SOUND/MUSIC"))
        );
        // A `sound/music` directory directly beside the archive comes first.
        std::fs::create_dir_all(temp.path().join("Sound/music")).unwrap();
        assert_eq!(music_dir(&master), Some(temp.path().join("Sound/music")));
        // A file with the directory's name is not a mount point.
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(other.path().join("sound")).unwrap();
        std::fs::write(other.path().join("sound/music"), b"").unwrap();
        assert_eq!(music_dir(&other.path().join("master.dat")), None);
    }

    #[test]
    #[ignore = "needs the local corpus"]
    fn the_dat_source_serves_loose_music_beside_the_master_database() {
        let root = std::env::var_os("VAULT_CORPUS")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus"));
        let (master, critter) = (root.join("MASTER.DAT"), root.join("CRITTER.DAT"));
        if !master.is_file() || !critter.is_file() {
            eprintln!("skipping: no DAT archives in {}", root.display());
            return;
        }
        let args = PreviewArgs {
            master_dat: Some(master.clone()),
            critter_dat: Some(critter),
            game: true,
            ..Default::default()
        };
        let vfs = args.source().unwrap();
        if music_dir(&master).is_none() {
            eprintln!("skipping: no music directory beside {}", master.display());
            return;
        }
        assert!(!vfs.read("sound/music/01hub.acm").unwrap().is_empty());
        assert!(
            vfs.files()
                .any(|path| path.as_str() == "sound/music/03wrldmp.acm")
        );
        // Archive members still resolve through the mount.
        assert!(vfs.read("color.pal").is_ok());
    }

    #[test]
    fn a_negative_seed_parses_and_minus_one_is_ambient() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            preview: PreviewArgs,
        }
        let parse = |seed: &str| {
            Cli::try_parse_from(["vault", "--preview-map", "m.map", "--seed", seed])
                .map(|cli| cli.preview)
        };
        assert_eq!(parse("-5").unwrap().resolved_seed(), -5);
        assert_eq!(parse("7").unwrap().resolved_seed(), 7);
        assert!((0..=i32::MAX).contains(&parse("-1").unwrap().resolved_seed()));
        assert_eq!(PreviewArgs::default().resolved_seed(), 1);
    }

    #[test]
    fn loads_both_fonts_message_selection_and_directional_suffixes() {
        let temp = tempfile::tempdir().unwrap();
        fixtures::write_corpus(temp.path());
        for font in ["FONT0.FON", "font0.aaf"] {
            let args = PreviewArgs {
                corpus_dir: Some(temp.path().into()),
                preview_frm: Some(r"ART\ITEMS\shared.FR5".into()),
                font: Some(font.into()),
                msg: Some("text/english/game/preview.msg".into()),
                message_id: Some(1),
                ..Default::default()
            };
            let mut preview = args.load_sprite().unwrap().unwrap();
            assert_eq!(preview.direction, 5);
            assert_eq!(preview.message, b"AB A B");
            assert_eq!(
                (
                    preview.image().unwrap().width(),
                    preview.image().unwrap().height()
                ),
                (640, 480)
            );
            preview.step(KeyCode::ArrowRight);
            assert_eq!(preview.direction, 0);
            preview.step(KeyCode::ArrowLeft);
            assert_eq!(preview.direction, 5);
        }
        let mut args = PreviewArgs {
            corpus_dir: Some(temp.path().into()),
            preview_frm: Some("art/items/animated.frm".into()),
            ..Default::default()
        };
        let mut preview = args.load_sprite().unwrap().unwrap();
        preview.step(KeyCode::ArrowDown);
        assert_eq!(preview.frame, 1);
        preview.step(KeyCode::ArrowUp);
        assert_eq!(preview.frame, 0);
        assert!(!preview.step(KeyCode::Space));
        args.message_id = Some(999);
        assert!(args.load().is_err());
        args.message_id = None;
        args.font = Some("missing.fon".into());
        assert!(args.load().is_err());
    }
}
