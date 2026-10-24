use crate::AppResult;
use std::path::Path;
use std::{path::PathBuf, sync::Arc};
use vault_formats::{
    DatArchive, DatProvider, Database, Font, FrmArchive, LayerEntry, LayeredVfs, LoadOrderPolicy,
    LocalDirProvider, MAX_ASSET_BYTES, MsgCatalog, Palette, ParseError, PrefixMount, Vfs, VfsPath,
};
use vault_render::{ImageError, IndexedImage, compose_preview};
use winit::keyboard::KeyCode;

#[derive(clap::Args, Debug, Default)]
pub struct PreviewArgs {
    /// The engine preset (from the settings or `--preset`; Vanilla when
    /// neither names one).
    #[arg(skip)]
    pub preset: vault_core::EnginePreset,
    /// Mod layers stacked over the game data, lowest first (the settings'
    /// `mod_list`): paths, or a `load_order.toml`.
    #[arg(skip)]
    pub mods: Option<crate::config::ModList>,
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
        let Some(mods) = &self.mods else {
            return Ok(base);
        };
        // The mod layers stay outermost, so the world sees them counted.
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
            game.headless = self.headless;
            game.debug = self.debug;
            game.preset = self.preset;
            // G-09: the preview and debug keys work in the game only with
            // the explicit flag.
            game.debug_keys = self.debug;
            return Ok(Some(AssetPreview::Game(Box::new(game))));
        }
        if let Some(path) = &self.preview_map {
            let vfs = self.source()?;
            let mut map = crate::map_preview::MapPreview::load_with(
                vfs,
                path,
                language,
                self.player_gcd.as_deref(),
                self.resolved_seed(),
                self.preset,
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
