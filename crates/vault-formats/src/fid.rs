//! SPEC-007 art identifiers: field packing, list-driven filename codec, and the
//! constructor's directional-file existence fallback.
//!
//! Engine extension (`docs/extended-art-namespaces.md`): extended
//! identifiers (bit 31 plus bits 12-23 as the high base bits) for the
//! categories whose lookup ignores those bits, when that category's resolved
//! list has more than 4,096 rows, and rows that mod layers register past a
//! list file's end ([`ArtCatalog::register`]).
use crate::{
    LstKind, LstTable, MAX_ASSET_BYTES, ParseError, Vfs, lst::LstEntry,
    mod_manifest::ArtRegistration,
};
use std::collections::BTreeMap;

pub const CRITTER_CATEGORY: u8 = 1;
pub const HEAD_CATEGORY: u8 = 8;
/// Rows a plain identifier's 12-bit base reaches: 0..=4095.
pub const PLAIN_ROWS: u32 = 4096;
/// The largest row an extended identifier addresses (24 bits).
pub const MAX_EXTENDED_ROW: u32 = 0x00ff_ffff;
/// Bit 31: the extended-identifier flag (it counts only under
/// [`ArtCatalog::is_extended`]'s rule).
pub const EXTENDED_BIT: u32 = 0x8000_0000;
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
    #[error("art base index {index} does not fit category {category}'s identifier")]
    BaseRange { category: u8, index: u32 },
    #[error("art category {0} is outside 0..=10")]
    CategoryRange(u8),
    /// An extended FID's row (or a wide row) that the resolved list does not
    /// hold, or a registered-gap row without a name.
    #[error("art list for category {category} has no row {row}")]
    MissingRow { category: u8, row: u32 },
    /// Critters (1) and heads (8): their bits 12-23 carry more than a list
    /// row, so they have no extended form and no row-only filename.
    #[error("art category {0} (critters or heads) has no extended or row-only identifier")]
    RowCategory(u8),
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
    /// As [`Self::new`], refusing a base above 4095 and a category above
    /// 10 instead of wrapping them (`[May-improve]`).
    pub fn try_new(
        category: u8,
        base: u32,
        animation: u8,
        weapon: u8,
        selector: u8,
    ) -> Result<Self, FidError> {
        if category > 10 {
            return Err(FidError::CategoryRange(category));
        }
        if base >= PLAIN_ROWS {
            return Err(FidError::BaseRange {
                category,
                index: base,
            });
        }
        Ok(Self::new(
            category,
            base as u16,
            animation,
            weapon,
            selector,
        ))
    }
    /// The extended form of row `base` in `category`: bit 31 set, bits
    /// 12-23 the row's high 12 bits, bits 0-11 its low 12 bits. Always the
    /// bit-31 form, even for a row below 4096 (which means the same row as
    /// the plain FID; [`ArtCatalog::canonical_fid`] picks the plain one).
    /// Critters, heads and categories above 10 have none.
    pub fn extended(category: u8, base: u32) -> Result<Self, FidError> {
        if category > 10 {
            return Err(FidError::CategoryRange(category));
        }
        if matches!(category, CRITTER_CATEGORY | HEAD_CATEGORY) {
            return Err(FidError::RowCategory(category));
        }
        if base > MAX_EXTENDED_ROW {
            return Err(FidError::BaseRange {
                category,
                index: base,
            });
        }
        // Bits 12-23 hold `base >> 12` and bits 0-11 `base & 0xfff`: the
        // 24-bit row itself.
        Ok(Self(EXTENDED_BIT | (u32::from(category) << 24) | base))
    }
    /// Whether bit 31 is set (which counts only under
    /// [`ArtCatalog::is_extended`]).
    pub const fn has_extended_bit(self) -> bool {
        self.0 & EXTENDED_BIT != 0
    }
    /// The 24-bit row bits 12-23 and 0-11 spell, `(bits 12-23) * 4096 +
    /// (bits 0-11)`, whatever the catalog.
    pub const fn wide_base(self) -> u32 {
        self.0 & 0x00ff_ffff
    }
    /// The list row this identifier addresses under `catalog`: the wide row
    /// for an extended FID (§3.2 items 1-2), otherwise [`Self::base`].
    /// `None` for a category above 10.
    pub fn row(self, catalog: &ArtCatalog) -> Option<u32> {
        if self.category() > 10 {
            return None;
        }
        Some(if catalog.is_extended(self) {
            self.wide_base()
        } else {
            u32::from(self.base())
        })
    }
}

/// Thousands separators for a diagnostic count.
fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut text = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
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

/// The VFS path of category `category`'s list file.
fn list_path(category: u8) -> String {
    match ART_DIRECTORIES.get(usize::from(category)) {
        Some(directory) => format!("art/{directory}/{directory}.lst"),
        None => format!("art list {category}"),
    }
}

/// Parsed category art lists plus the critter list's default alias index.
///
/// Each category's **resolved list** is its list file followed by the rows
/// mod layers register ([`Self::register`]); rows between the file's end and
/// a registered row are empty rows with no name.
#[derive(Debug, Clone, Default)]
pub struct ArtCatalog {
    lists: BTreeMap<u8, LstTable>,
    /// Registered rows per category, each at or past its file's length.
    registered: BTreeMap<u8, BTreeMap<u32, LstEntry>>,
    /// One line per resolved list longer than 4,096 rows.
    diagnostics: Vec<String>,
    /// Alias used by critter rows without a comma; zero at list initialization.
    pub default_alias: i32,
}
impl ArtCatalog {
    pub fn insert(&mut self, category: u8, list: LstTable) {
        self.lists.insert(category, list);
        self.refresh_diagnostics();
    }
    /// The rows of category `category`'s list file (0 without one).
    fn file_len(&self, category: u8) -> u32 {
        self.lists.get(&category).map_or(0, |list| {
            u32::try_from(list.entries().len()).unwrap_or(u32::MAX)
        })
    }
    /// The rows of category `category`'s resolved list: the list file's
    /// rows, extended to just past the highest registered row; 0 without a
    /// list or registration.
    pub fn list_len(&self, category: u8) -> u32 {
        let registered = self
            .registered
            .get(&category)
            .and_then(|rows| rows.keys().next_back())
            .map_or(0, |row| row + 1);
        self.file_len(category).max(registered)
    }
    /// SPEC-007 §2.1 extension, §3.2 item 1: bit 31 set, a category 0-10
    /// other than critters and heads, and that category's resolved list has
    /// more than 4,096 rows. Every other FID keeps its plain meaning.
    pub fn is_extended(&self, fid: Fid) -> bool {
        let category = fid.category();
        fid.has_extended_bit()
            && category <= 10
            && !matches!(category, CRITTER_CATEGORY | HEAD_CATEGORY)
            && self.list_len(category) > PLAIN_ROWS
    }
    /// One line per resolved list with more than 4,096 rows, naming the
    /// list, its row count and the first row plain identifiers cannot reach
    /// (§3.1 item 3). The lists still load whole.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    fn refresh_diagnostics(&mut self) {
        let categories: std::collections::BTreeSet<u8> = self
            .lists
            .keys()
            .chain(self.registered.keys())
            .copied()
            .collect();
        self.diagnostics = categories
            .into_iter()
            .filter_map(|category| {
                let rows = self.list_len(category);
                if rows <= PLAIN_ROWS {
                    return None;
                }
                let file = self.file_len(category);
                let source = if file == rows {
                    String::new()
                } else {
                    format!(
                        " ({} from the file, the rest registered)",
                        grouped(file as usize)
                    )
                };
                let reach = if matches!(category, CRITTER_CATEGORY | HEAD_CATEGORY) {
                    "critter and head identifiers address rows 0-4095 only, so rows from 4096 \
                     are never drawn"
                } else {
                    "plain identifiers address rows 0-4095, so rows from 4096 need extended \
                     identifiers"
                };
                Some(format!(
                    "{}: {} rows{source}; first unreachable row {PLAIN_ROWS}: {reach}",
                    list_path(category),
                    grouped(rows as usize),
                ))
            })
            .collect();
    }
    /// §3.4: add registered rows to the resolved lists. Each row sits at its
    /// explicit `index`, at or past its category's list file length; a
    /// category above 10, an index below the list file's length, a duplicate
    /// index (here or from an earlier call), an index above 16,777,215, an
    /// index above 4095 for critters or heads, or a name that is not a
    /// single list name is an error naming the category and index, and then
    /// nothing is registered. Call it after the list files are loaded.
    pub fn register(&mut self, rows: &[ArtRegistration]) -> Result<(), String> {
        let mut registered = self.registered.clone();
        for row in rows {
            let (category, index) = (row.category, row.index);
            let Some(directory) = ART_DIRECTORIES.get(usize::from(category)) else {
                return Err(format!(
                    "art category {category} is outside 0..=10 (row {index})"
                ));
            };
            if index > MAX_EXTENDED_ROW {
                return Err(format!(
                    "art row {index} of {directory} is above {MAX_EXTENDED_ROW}"
                ));
            }
            if matches!(category, CRITTER_CATEGORY | HEAD_CATEGORY) && index >= PLAIN_ROWS {
                return Err(format!(
                    "art row {index} of {directory} is above 4095, the last row {directory} \
                     identifiers address"
                ));
            }
            let served = self.file_len(category);
            if index < served {
                return Err(format!(
                    "art row {index} of {directory} is inside {}'s {served} rows",
                    list_path(category)
                ));
            }
            if !crate::mod_manifest::is_list_name(&row.file) {
                return Err(format!(
                    "art row {index} of {directory}: \"{}\" is not a list name of 1-12 bytes \
                     without space, comma, semicolon or TAB",
                    row.file
                ));
            }
            let name = row.file.as_bytes().to_vec();
            let entry = LstEntry {
                raw: name.clone(),
                name: Some(name),
                alias: None,
                head_counts: None,
                head_columns: None,
                local_vars: None,
            };
            if registered
                .entry(category)
                .or_default()
                .insert(index, entry)
                .is_some()
            {
                return Err(format!(
                    "art row {index} of {directory} is registered twice"
                ));
            }
        }
        self.registered = registered;
        self.refresh_diagnostics();
        Ok(())
    }
    /// Row `row` of `category`'s resolved list: `Ok(None)` past its end and
    /// for a registered-gap row.
    fn resolved(&self, category: u8, row: u32) -> Result<Option<&LstEntry>, FidError> {
        let list = self.lists.get(&category);
        let registered = self.registered.get(&category);
        if list.is_none() && registered.is_none() {
            return Err(FidError::MissingList(category));
        }
        if let Some(entry) = list.and_then(|list| list.art_row(row)) {
            return Ok(Some(entry));
        }
        Ok(registered.and_then(|rows| rows.get(&row)))
    }
    /// The resolved row's name as text: `None` without a list, past the
    /// end, for an empty registered-gap row, a row whose name the list
    /// withheld, or a name that is not ASCII.
    pub fn row_name(&self, category: u8, row: u32) -> Option<&str> {
        let name = self.resolved(category, row).ok()??.name.as_deref()?;
        std::str::from_utf8(name)
            .ok()
            .filter(|name| name.is_ascii())
    }
    /// The unaliased list token of resolved row `row`, unmasked (the wide
    /// counterpart of [`Self::base_token`]).
    pub fn row_token(&self, category: u8, row: u32) -> Option<&[u8]> {
        self.resolved(category, row).ok()??.name.as_deref()
    }
    /// The last resolved row whose base name equals `name` without case
    /// (the wide counterpart of [`Self::last_row`]).
    pub fn last_row_wide(&self, category: u8, name: &[u8]) -> Option<u32> {
        let matches = |entry: &LstEntry| {
            entry
                .name
                .as_deref()
                .is_some_and(|base| base.eq_ignore_ascii_case(name))
        };
        if let Some((row, _)) = self
            .registered
            .get(&category)
            .and_then(|rows| rows.iter().rev().find(|(_, entry)| matches(entry)))
        {
            return Some(*row);
        }
        let row = self
            .lists
            .get(&category)?
            .entries()
            .iter()
            .rposition(matches)?;
        u32::try_from(row).ok()
    }
    /// The VFS path `art/<directory>/<name>` of resolved row `row` of a
    /// category whose filename is the row's name verbatim (not critters or
    /// heads): what both its plain and its extended FID resolve to.
    pub fn row_path(&self, category: u8, row: u32) -> Result<String, FidError> {
        let directory = ART_DIRECTORIES
            .get(usize::from(category))
            .ok_or(FidError::CategoryRange(category))?;
        if matches!(category, CRITTER_CATEGORY | HEAD_CATEGORY) {
            return Err(FidError::RowCategory(category));
        }
        Ok(format!(
            "art/{directory}/{}",
            self.stem_wide(category, row)?
        ))
    }
    /// The canonical FID of resolved row `row` (§3.2 items 5 and 8): the
    /// plain FID for a row below 4096, the extended FID for a higher row of
    /// a category other than critters and heads that the resolved list
    /// holds, and `None` otherwise (or for a category above 10).
    pub fn canonical_fid(&self, category: u8, row: u32) -> Option<Fid> {
        if row < PLAIN_ROWS {
            return Fid::try_new(category, row, 0, 0, 0).ok();
        }
        if row >= self.list_len(category) {
            return None;
        }
        Fid::extended(category, row).ok()
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
    /// A plain identifier's row: the index masked to 12 bits, as
    /// [`LstTable::art_id`] reads it.
    fn entry(&self, category: u8, index: u16) -> Result<&LstEntry, FidError> {
        self.resolved(category, u32::from(index) & 0xfff)?
            .ok_or(FidError::MissingEntry { category, index })
    }
    /// The critter-list alias of an art row (SPEC-007 §2.1): its alias
    /// field, or the list's default alias without one (SPEC-014 §3.38
    /// picks the called-shot labels by it). `None` without that row.
    pub fn critter_alias(&self, index: u16) -> Option<i32> {
        let entry = self.entry(CRITTER_CATEGORY, index).ok()?;
        Some(entry.critter_alias(self.default_alias))
    }
    /// The unaliased list token for an art index (SPEC-010 effect stems).
    pub fn base_token(&self, category: u8, index: u16) -> Option<&[u8]> {
        self.entry(category, index).ok()?.name.as_deref()
    }
    /// The zero-based row whose base name equals `name`, compared without
    /// case; when the name occurs more than once the last row wins
    /// (SPEC-005 §3.8 jumpsuit indexes).
    pub fn last_row(&self, category: u8, name: &[u8]) -> Option<u16> {
        u16::try_from(self.last_row_wide(category, name)?).ok()
    }
    fn ascii(name: Option<&[u8]>) -> Result<&str, FidError> {
        std::str::from_utf8(name.unwrap_or(b""))
            .ok()
            .filter(|name| name.is_ascii())
            .ok_or(FidError::Name)
    }
    fn stem(&self, category: u8, index: u16) -> Result<&str, FidError> {
        Self::ascii(self.entry(category, index)?.name.as_deref())
    }
    /// Resolved row `row`'s stem; a row past the end or a gap row is
    /// [`FidError::MissingRow`].
    fn stem_wide(&self, category: u8, row: u32) -> Result<&str, FidError> {
        let entry = self
            .resolved(category, row)?
            .ok_or(FidError::MissingRow { category, row })?;
        Self::ascii(entry.name.as_deref())
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
            // §3.2 items 1-4: an extended FID names its wide row, and has no
            // filename past the resolved list or on an empty gap row.
            0..=10 if self.is_extended(fid) => {
                Ok(self.stem_wide(category, fid.wide_base())?.to_owned())
            }
            0..=10 => Ok(self.stem(category, fid.base())?.to_owned()),
            _ => Err(FidError::Category(category)),
        }
    }
    /// Complete VFS-relative path, `art/<directory>/<filename>` (extended
    /// identifiers included, as [`Self::filename`]).
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
