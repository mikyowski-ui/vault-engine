use crate::AppResult;
use std::path::PathBuf;
use vault_formats::{
    Font, FrmArchive, LocalDirProvider, MAX_ASSET_BYTES, MsgCatalog, Palette, ParseError, Vfs,
    VfsPath,
};
use vault_render::{ImageError, IndexedImage, compose_preview};
use winit::keyboard::KeyCode;

#[derive(clap::Args, Debug, Default)]
pub struct PreviewArgs {
    /// Mount a loose directory for sprite preview.
    #[arg(long, requires = "preview_frm", value_name = "PATH")]
    pub corpus_dir: Option<PathBuf>,
    /// Sprite path relative to the case-insensitive VFS root.
    #[arg(long, requires = "corpus_dir", value_name = "VFS_PATH")]
    pub preview_frm: Option<String>,
    /// Font path (default: font0.aaf, font0.fon, then another root font).
    #[arg(long, requires = "preview_frm", value_name = "VFS_PATH")]
    pub font: Option<String>,
    /// Message catalog (default: first .msg below text/ in VFS order).
    #[arg(long, requires = "preview_frm", value_name = "VFS_PATH")]
    pub msg: Option<String>,
    /// Message ID (default: lowest ID with nonempty text).
    #[arg(long, requires = "preview_frm", allow_negative_numbers = true)]
    pub message_id: Option<i32>,
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

fn parse_asset<T>(
    vfs: &LocalDirProvider,
    path: &str,
    parser: impl FnOnce(&[u8]) -> Result<T, ParseError>,
) -> AppResult<T> {
    let bytes = vfs.read_bounded(path, MAX_ASSET_BYTES)?;
    parser(&bytes).map_err(|error| format!("{path}: {error}").into())
}

impl PreviewArgs {
    pub fn load(&self) -> AppResult<Option<Preview>> {
        let Some(path) = &self.preview_frm else {
            return Ok(None);
        };
        let vfs = LocalDirProvider::mount(
            self.corpus_dir
                .as_ref()
                .ok_or("--preview-frm requires --corpus-dir")?,
        )?;
        let path = VfsPath::new(path)?;
        let direction = match path.extension() {
            Some("frm") => 0,
            Some(extension @ ("fr0" | "fr1" | "fr2" | "fr3" | "fr4" | "fr5")) => {
                usize::from(extension.as_bytes()[2] - b'0')
            }
            _ => return Err("--preview-frm must name a .frm or .fr0–.fr5 asset".into()),
        };
        let palette = parse_asset(&vfs, "color.pal", Palette::parse)?;
        let archive = parse_asset(&vfs, path.as_str(), FrmArchive::parse)?;
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
            Some("fon") => parse_asset(&vfs, font_path.as_str(), Font::parse_fon)?,
            Some("aaf") => parse_asset(&vfs, font_path.as_str(), Font::parse_aaf)?,
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
        let catalog = parse_asset(&vfs, &msg_path, MsgCatalog::parse)?;
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
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
mod fixtures;

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
            };
            let mut preview = args.load().unwrap().unwrap();
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
        let mut preview = args.load().unwrap().unwrap();
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
