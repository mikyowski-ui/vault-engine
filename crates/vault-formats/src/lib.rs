//! Bounded clean-room parsers based on approved SPEC-001 and SPEC-002.

pub mod acm;
pub mod ai;
pub mod automap;
mod binary;
pub mod cfg;
pub mod credits;
pub mod dat;
pub mod dat2;
pub mod fid;
pub mod font;
pub mod frm;
pub mod gam;
pub mod gcd;
pub mod geometry;
pub mod inflate;
pub mod lip;
pub mod lst;
pub mod map;
pub mod mod_manifest;
pub mod moviefx;
pub mod msg;
pub mod mve;
pub mod palette;
pub mod pro;
pub mod pro_catalog;
pub mod rix;
pub mod savegame;
pub mod screenshot;
pub mod sfx;
pub mod sve;
pub mod vfs;

pub use automap::AutomapDb;
pub use binary::{MAX_ASSET_BYTES, ParseError};
pub use dat::{DatArchive, DatProvider, Database};
pub use dat2::{Dat2Archive, Dat2Provider};

/// The game whose data an engine reads: it selects the archive format
/// (SPEC-008 or SPEC-015) and the map and prototype layouts (SPEC-003 or
/// SPEC-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameProfile {
    #[default]
    Fallout1,
    Fallout2,
}

impl GameProfile {
    /// The map version this game's maps carry (SPEC-003 19, SPEC-016 20).
    pub fn map_version(self) -> i32 {
        match self {
            Self::Fallout1 => 19,
            Self::Fallout2 => 20,
        }
    }
}
pub use fid::{ArtCatalog, Fid, FidError};
pub use font::{Font, FontKind, FontSet, TextFlags};
pub use frm::FrmArchive;
pub use gam::{GamFile, GamSection};
pub use gcd::GcdFile;
pub use geometry::HexGrid;
pub use lip::LipFile;
pub use lst::{LstKind, LstTable};
pub use map::MapFile;
pub use mod_manifest::{EnhancedOverrides, ModManifest};
pub use msg::{MsgCatalog, TextMode};
pub use palette::Palette;
pub use pro::Prototype;
pub use pro_catalog::LocalPrototypeCatalog;
pub use vfs::{
    LayerEntry, LayeredVfs, LoadOrderPolicy, LocalDirProvider, ModLayer, PrefixMount, Vfs,
    VfsError, VfsPath, parse_load_order,
};

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
        // A decode that stopped at a bad block, or a message load that
        // stopped at a bad record, is a validation failure even though the
        // runtime keeps the earlier samples/records.
        "acm" => acm::decode(bytes).and_then(|decoded| decoded.failure.map_or(Ok(()), Err)),
        "msg" => MsgCatalog::parse_strict(bytes).map(|_| ()),
        "pro" => Prototype::parse(bytes).map(|_| ()),
        "gcd" => GcdFile::parse(bytes).map(|_| ()),
        "rix" => rix::Splash::parse(bytes).map(|_| ()),
        "gam" => GamFile::parse(
            bytes,
            if path.as_str() == "data/vault13.gam" {
                GamSection::Campaign
            } else {
                GamSection::Map
            },
        )
        .map(|_| ()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod acm_tests;
#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod automap_tests;
#[cfg(test)]
mod dat_tests;
#[cfg(test)]
mod fid_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod lip_tests;
#[cfg(test)]
mod movie_tests;
#[cfg(test)]
mod presentation_tests;
#[cfg(test)]
mod save_tests;
#[cfg(test)]
mod sfx_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod world_tests;

#[cfg(test)]
pub(crate) use vault_fixtures::formats as fixture_assets;
