//! SPEC-007 art identifiers: field packing, list-driven filename codec, and the
//! constructor's directional-file existence fallback.
use crate::{LstKind, LstTable, MAX_ASSET_BYTES, ParseError, Vfs, lst::LstEntry};
use std::collections::BTreeMap;

pub const CRITTER_CATEGORY: u8 = 1;
pub const HEAD_CATEGORY: u8 = 8;
/// Art directories / list stems for categories 0..=10, in category order.
pub const ART_DIRECTORIES: [&str; 11] = [
    "items", "critters", "scenery", "walls", "tiles", "misc", "intrface", "inven", "heads",
    "backgrnd", "skilldex",
];
/// Animation codes whose critter filenames substitute the row's alias once.
pub const ALIASED_ANIMATIONS: [u8; 8] = [27, 29, 30, 33, 55, 57, 58, 64];
const HEAD_SUFFIXES: [&str; 12] = [
    "gv", "gf", "gn", "ng", "nf", "nb", "bn", "bf", "bv", "gp", "np", "bp",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidError {
    #[error("art category {0} has no supported directory")]
    Category(u8),
    #[error("art list for category {0} is not loaded")]
    MissingList(u8),
    #[error("art list for category {category} has no index {index}")]
    MissingEntry { category: u8, index: u16 },
    #[error("critter weapon-animation code {0} is outside 0..=10")]
    Weapon(u8),
    #[error("critter animation {0} is outside the supported 0..=64 domain")]
    Animation(u8),
    #[error("weapon animation {0} requires a nonzero weapon code")]
    WeaponRequired(u8),
    #[error("head animation selector {0} is outside 0..=11")]
    HeadAnimation(u8),
    #[error("art list name is not an ASCII filename")]
    Name,
}

/// A packed 32-bit art identifier. Serialized fields store it as one big-endian
/// word; bit positions here refer to that numeric value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fid(pub u32);
impl Fid {
    /// Masks base to 12 bits, weapon/variant to 4, and selector to 3. Category is
    /// packed arithmetically without validation, as in the reference sum.
    pub const fn new(category: u8, base: u16, animation: u8, weapon: u8, selector: u8) -> Self {
        Self(
            (base as u32 & 0xfff)
                .wrapping_add((weapon as u32 & 15) << 12)
                .wrapping_add((animation as u32) << 16)
                .wrapping_add((category as u32).wrapping_mul(1 << 24))
                .wrapping_add((selector as u32 & 7) << 28),
        )
    }
    pub const fn base(self) -> u16 {
        (self.0 & 0xfff) as u16
    }
    /// Critter weapon-animation code, or the head fidget variant.
    pub const fn weapon(self) -> u8 {
        ((self.0 >> 12) & 15) as u8
    }
    pub const fn animation(self) -> u8 {
        ((self.0 >> 16) & 0xff) as u8
    }
    pub const fn category(self) -> u8 {
        ((self.0 >> 24) & 15) as u8
    }
    /// Directional-file selector: 0 is `.frm`, 1..=7 are `.fr0`..=`.fr6`.
    pub const fn selector(self) -> u8 {
        ((self.0 >> 28) & 7) as u8
    }
    pub const fn with_selector(self, selector: u8) -> Self {
        Self((self.0 & !0x7000_0000) | ((selector as u32 & 7) << 28))
    }
    /// Ordinary directional callers pass physical orientation plus one.
    pub const fn selector_for_orientation(orientation: u8) -> u8 {
        orientation.wrapping_add(1) & 7
    }
}

/// Exact two-letter critter suffix. The weapon check precedes every branch.
pub fn critter_suffix(animation: u8, weapon: u8) -> Result<[u8; 2], FidError> {
    if weapon > 10 {
        return Err(FidError::Weapon(weapon));
    }
    let letter = (weapon != 0).then_some(b'c' + weapon);
    Ok(match animation {
        0 => letter.map_or(*b"aa", |l| [l, b'a']),
        1 => letter.map_or(*b"ab", |l| [l, b'b']),
        2..=12 => [b'a', b'c' + (animation - 2)],
        13 => letter.map_or(*b"an", |l| [l, b'e']),
        14..=17 => [b'a', b'o' + (animation - 14)],
        18 => match weapon {
            1 => *b"dm",
            4 => *b"gm",
            _ => *b"as",
        },
        19 => *b"at",
        20..=35 => [b'b', b'a' + (animation - 20)],
        36 => *b"ch",
        37 => *b"cj",
        38..=47 => [
            letter.ok_or(FidError::WeaponRequired(animation))?,
            b'c' + (animation - 38),
        ],
        48..=63 => [b'r', b'a' + (animation - 48)],
        64 => *b"na",
        // The reference's arithmetic branches do not safely reject larger codes.
        _ => return Err(FidError::Animation(animation)),
    })
}

/// Head suffix; fidget selectors 1, 4, 7 append the unpadded decimal variant.
pub fn head_suffix(animation: u8, variant: u8) -> Result<String, FidError> {
    let suffix = HEAD_SUFFIXES
        .get(usize::from(animation))
        .ok_or(FidError::HeadAnimation(animation))?;
    Ok(if matches!(animation, 1 | 4 | 7) {
        format!("{suffix}{variant}")
    } else {
        (*suffix).to_owned()
    })
}

/// Parsed category art lists plus the critter list's default alias index.
#[derive(Debug, Clone, Default)]
pub struct ArtCatalog {
    lists: BTreeMap<u8, LstTable>,
    /// Alias used by critter rows without a comma; zero at list initialization.
    pub default_alias: i32,
}
impl ArtCatalog {
    pub fn insert(&mut self, category: u8, list: LstTable) {
        self.lists.insert(category, list);
    }
    /// Parse every present `art/<dir>/<dir>.lst`; absent lists stay unloaded.
    pub fn load(vfs: &dyn Vfs) -> Result<Self, ParseError> {
        let mut catalog = Self::default();
        for (category, directory) in ART_DIRECTORIES.iter().enumerate() {
            let path = format!("art/{directory}/{directory}.lst");
            if vfs.files().any(|file| file.as_str() == path) {
                // Bounded before reading: an oversized stray list is
                // rejected from its size, not read into memory first.
                let bytes = vfs
                    .read_bounded(&path, MAX_ASSET_BYTES)
                    .map_err(|error| ParseError::new("LST", 0, error.to_string()))?;
                catalog.insert(category as u8, LstTable::parse(&bytes, LstKind::Art)?);
            }
        }
        Ok(catalog)
    }
    fn entry(&self, category: u8, index: u16) -> Result<&LstEntry, FidError> {
        self.lists
            .get(&category)
            .ok_or(FidError::MissingList(category))?
            .art_id(u32::from(index))
            .ok_or(FidError::MissingEntry { category, index })
    }
    /// The unaliased list token for an art index (SPEC-010 effect stems).
    pub fn base_token(&self, category: u8, index: u16) -> Option<&[u8]> {
        self.entry(category, index).ok()?.name.as_deref()
    }
    /// The zero-based row whose base name equals `name`, compared without
    /// case; when the name occurs more than once the last row wins
    /// (SPEC-005 §3.8 jumpsuit indexes).
    pub fn last_row(&self, category: u8, name: &[u8]) -> Option<u16> {
        let row = self
            .lists
            .get(&category)?
            .entries()
            .iter()
            .rposition(|entry| {
                entry
                    .name
                    .as_deref()
                    .is_some_and(|base| base.eq_ignore_ascii_case(name))
            })?;
        u16::try_from(row).ok()
    }
    fn stem(&self, category: u8, index: u16) -> Result<&str, FidError> {
        let name = self.entry(category, index)?.name.as_deref().unwrap_or(b"");
        std::str::from_utf8(name)
            .ok()
            .filter(|name| name.is_ascii())
            .ok_or(FidError::Name)
    }
    /// Filename within `art/<directory>/`, honoring an encoded selector as-is.
    pub fn filename(&self, fid: Fid) -> Result<String, FidError> {
        let category = fid.category();
        match category {
            CRITTER_CATEGORY => {
                let (animation, weapon) = (fid.animation(), fid.weapon());
                if weapon > 10 {
                    return Err(FidError::Weapon(weapon));
                }
                let mut index = fid.base();
                if ALIASED_ANIMATIONS.contains(&animation) {
                    let alias = self
                        .entry(category, index)?
                        .critter_alias(self.default_alias);
                    index = (alias as u32 & 0xfff) as u16;
                }
                let suffix = critter_suffix(animation, weapon)?;
                let extension = match fid.selector() {
                    0 => "frm".to_owned(),
                    selector => format!("fr{}", selector - 1),
                };
                Ok(format!(
                    "{}{}{}.{extension}",
                    self.stem(category, index)?,
                    char::from(suffix[0]),
                    char::from(suffix[1])
                ))
            }
            HEAD_CATEGORY => Ok(format!(
                "{}{}.frm",
                self.stem(category, fid.base())?,
                head_suffix(fid.animation(), fid.weapon())?
            )),
            0..=10 => Ok(self.stem(category, fid.base())?.to_owned()),
            _ => Err(FidError::Category(category)),
        }
    }
    /// Complete VFS-relative path, `art/<directory>/<filename>`.
    pub fn path(&self, fid: Fid) -> Result<String, FidError> {
        let directory = ART_DIRECTORIES
            .get(usize::from(fid.category()))
            .ok_or(FidError::Category(fid.category()))?;
        Ok(format!("art/{directory}/{}", self.filename(fid)?))
    }
    /// The normal constructor: only critter animations 20-35 other than 33 probe
    /// directional files, trying the requested selector and then `.fr0` once.
    /// The returned identifier keeps the original base even when probing aliases.
    pub fn construct(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        selector: u8,
        mut exists: impl FnMut(&str) -> bool,
    ) -> Fid {
        let unprobed = Fid::new(category, base, animation, weapon, 0);
        if category != CRITTER_CATEGORY || !(20..=35).contains(&animation) || animation == 33 {
            return unprobed;
        }
        let mut probe = |selector: u8| {
            let fid = unprobed.with_selector(selector);
            self.path(fid).is_ok_and(|path| exists(&path))
        };
        let selector = selector & 7;
        if probe(selector) {
            unprobed.with_selector(selector)
        } else if selector != 1 && probe(1) {
            unprobed.with_selector(1)
        } else {
            unprobed
        }
    }
}
