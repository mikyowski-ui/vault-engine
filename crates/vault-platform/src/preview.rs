use crate::AppResult;
use std::{path::PathBuf, sync::Arc};
use vault_formats::{
    DatArchive, DatProvider, Database, Font, FrmArchive, LocalDirProvider, MAX_ASSET_BYTES,
    MsgCatalog, Palette, ParseError, Vfs, VfsPath,
};
use vault_render::{ImageError, IndexedImage, compose_preview};
use winit::keyboard::KeyCode;

#[derive(clap::Args, Debug, Default)]
pub struct PreviewArgs {
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
    /// Map path relative to the VFS root; arrows pan, R toggles roofs, 0-2 select elevation.
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
    /// Deterministic game RNG seed; defaults to 1.
    #[arg(long, requires = "preview_map")]
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
}

/// A standalone movie and the black palette shown around it.
pub struct MoviePreview {
    pub player: crate::movie::MoviePlayer,
    black: Palette,
}
impl AssetPreview {
    pub fn image(&mut self) -> AppResult<IndexedImage> {
        match self {
            Self::Sprite(preview) => Ok(preview.image()?),
            Self::Map(preview) => preview.image(),
            Self::Movie(movie) => Ok(movie.player.image()),
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
        }
    }
    /// A movie's device palette replaces the display palette while it plays.
    pub fn display_override(&self) -> Option<[[u8; 4]; 256]> {
        match self {
            Self::Sprite(_) => None,
            Self::Map(preview) => preview.display_override(),
            Self::Movie(movie) => Some(movie.player.palette()),
        }
    }
    /// A standalone movie that ended (the shell then exits).
    pub fn finished(&self) -> bool {
        matches!(self, Self::Movie(movie) if movie.player.finished())
    }
    pub fn palette_generation(&self) -> u32 {
        match self {
            Self::Sprite(_) | Self::Movie(_) => 0,
            Self::Map(preview) => preview.host.library.palette_generation,
        }
    }
    /// Whether the previewed world currently permits palette cycling.
    pub fn palette_cycling(&self) -> bool {
        match self {
            Self::Sprite(_) => true,
            // SPEC-012 §3.12: movies disable color cycling.
            Self::Map(preview) => preview.host.library.palette_cycling && !preview.movie_active(),
            Self::Movie(_) => false,
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
        }
    }
    pub fn step(&mut self, key: KeyCode) -> bool {
        match self {
            Self::Sprite(preview) => preview.step(key),
            Self::Map(preview) => preview.step(key),
            Self::Movie(movie) => {
                movie.player.key();
                true
            }
        }
    }
    pub fn hover(&mut self, point: [i32; 2]) -> Option<vault_render::map::MapHover> {
        match self {
            Self::Sprite(_) | Self::Movie(_) => None,
            Self::Map(preview) => preview.hover(point),
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
        }
    }
    pub fn scroll(&mut self, delta: f64) -> bool {
        match self {
            Self::Sprite(_) | Self::Movie(_) => false,
            Self::Map(preview) => preview.scroll(delta),
        }
    }
    /// A mouse release: a held desktop button plays its release cue.
    pub fn release(&mut self) -> bool {
        match self {
            Self::Sprite(_) => false,
            Self::Map(preview) => preview.release(),
            Self::Movie(movie) => {
                movie.player.release();
                true
            }
        }
    }
    pub fn tick(&mut self, now: u64) -> AppResult<bool> {
        match self {
            Self::Sprite(_) => Ok(false),
            Self::Map(preview) => preview.tick(now),
            Self::Movie(movie) => Ok(movie.player.tick(now)),
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
    /// The asset namespace: SPEC-008 master and critter databases (each
    /// consulting the optional loose root first, with no fallback to the
    /// other archive), or a loose directory alone.
    pub fn source(&self) -> AppResult<Arc<dyn Vfs>> {
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
        let archive = |path: &PathBuf| -> AppResult<Arc<DatArchive>> {
            let bytes =
                std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
            Ok(Arc::new(
                DatArchive::parse(bytes).map_err(|error| format!("{}: {error}", path.display()))?,
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
        Ok(Arc::new(provider))
    }
    pub fn load(&self) -> AppResult<Option<AssetPreview>> {
        let language = self.language.as_deref().unwrap_or("english");
        if let Some(path) = &self.preview_map {
            let vfs = self.source()?;
            let mut map = crate::map_preview::MapPreview::load(
                vfs,
                path,
                language,
                self.player_gcd.as_deref(),
                self.seed.unwrap_or(1),
            )?;
            map.host.set_subtitles(self.subtitles);
            if self.headless {
                map.host.movie_policy = vault_core::movie::MoviePolicy::CompleteImmediately;
            }
            if self.startup_movies {
                map.queue_startup_movies();
            }
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

#[cfg(test)]
use crate::fixture_assets as fixtures;

#[cfg(test)]
mod tests {
    use super::*;

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
