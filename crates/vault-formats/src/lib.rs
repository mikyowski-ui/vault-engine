//! Bounded clean-room parsers based on approved SPEC-001 and SPEC-002.

mod binary;
pub mod font;
pub mod frm;
pub mod lst;
pub mod msg;
pub mod palette;
pub mod vfs;

pub use binary::{MAX_ASSET_BYTES, ParseError};
pub use font::{Font, FontKind};
pub use frm::FrmArchive;
pub use lst::{LstKind, LstTable};
pub use msg::{MsgCatalog, TextMode};
pub use palette::Palette;
pub use vfs::{LocalDirProvider, Vfs, VfsError, VfsPath};

/// Validate a supported asset. Unknown extensions return `None`.
/// Unknown LST consumers receive structural line parsing without invented ID rules.
pub fn validate_asset(path: &VfsPath, bytes: &[u8]) -> Option<Result<(), ParseError>> {
    Some(match path.extension()? {
        "pal" => Palette::parse(bytes).map(|_| ()),
        "frm" | "fr0" | "fr1" | "fr2" | "fr3" | "fr4" | "fr5" => {
            FrmArchive::parse(bytes).map(|_| ())
        }
        "fon" => Font::parse_fon(bytes).map(|_| ()),
        "aaf" => Font::parse_aaf(bytes).map(|_| ()),
        "lst" => LstTable::parse(bytes, LstKind::for_path(path)).map(|_| ()),
        "msg" => MsgCatalog::parse(bytes).map(|_| ()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
