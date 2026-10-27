//! SPEC-006 428-byte sheet, and SPEC-018 §6.3's 432-byte Fallout 2 sheet
//! (Revision 4). Decoding and player-only normalization are distinct.
use crate::{ParseError, binary::Reader};

/// SPEC-006 §2.2: the Fallout 1 sheet.
pub const GCD_LEN: usize = 428;
/// SPEC-018 §6.3 (Revision 4): the Fallout 2 sheet, with a damage-type word
/// after the 368-byte critter-stat portion.
pub const GCD2_LEN: usize = 432;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcdFile {
    pub flags: u32,
    pub base: [i32; 35],
    pub bonus: [i32; 35],
    pub skills: [i32; 18],
    pub body: i32,
    pub experience_reward: i32,
    pub kill_category: i32,
    /// The Fallout 2 sheet's damage-type word at `0x170` (SPEC-018 §6.3,
    /// Revision 4); `None` for SPEC-006's 428-byte sheet, which has none.
    /// It also chooses the layout [`GcdFile::write`] produces.
    pub damage_type: Option<i32>,
    pub name: [u8; 32],
    pub tags: [i32; 4],
    pub traits: [i32; 2],
    pub remaining_points: i32,
}
impl GcdFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, "GCD")?;
        // The exact length chooses the layout (SPEC-018 §6.3, Revision 4):
        // 428 bytes is SPEC-006's sheet, 432 the Fallout 2 sheet. Any other
        // length is refused (SPEC-006 §2.3's safe rule). Reading a 432-byte
        // sheet with the 428-byte layout would shift its name, tags, traits
        // and points by one word.
        let fallout2 = match bytes.len() {
            GCD_LEN => false,
            GCD2_LEN => true,
            length => {
                return Err(r.error(format!(
                    "a sheet is {GCD_LEN} bytes (Fallout 1) or {GCD2_LEN} (Fallout 2), not {length}"
                )));
            }
        };
        let flags = r.word()?;
        let base = r.words()?;
        let bonus = r.words()?;
        let skills = r.words()?;
        let body = r.be_i32()?;
        let experience_reward = r.be_i32()?;
        let kill_category = r.be_i32()?;
        let damage_type = if fallout2 { Some(r.be_i32()?) } else { None };
        let name: [u8; 32] = r.take(32)?.try_into().expect("fixed name");
        let tags: [i32; 4] = r.words()?;
        let traits: [i32; 2] = r.words()?;
        let remaining_points = r.be_i32()?;
        // SPEC-006 §2.2 describes the serialized words as "not
        // guaranteed-valid"; the checks below are engine safe bounds, not
        // reference behavior: a name without its NUL terminator, a SPECIAL
        // base outside 1-10, a tag outside -1..17 or trait outside -1..15
        // (the fixed tables), or negative creation points would otherwise
        // index tables or run the §2.3 normalization on out-of-range input.
        if !name.contains(&0) {
            return Err(r.error("player name has no NUL terminator"));
        }
        if base[..7].iter().any(|v| !(1..=10).contains(v)) {
            return Err(r.error("raw SPECIAL must be 1..=10"));
        }
        if tags.iter().any(|v| !(-1..18).contains(v))
            || traits.iter().any(|v| !(-1..16).contains(v))
        {
            return Err(r.error("invalid tag/trait index"));
        }
        if remaining_points < 0 {
            return Err(r.error("negative remaining creation points"));
        }
        Ok(Self {
            flags,
            base,
            bonus,
            skills,
            body,
            experience_reward,
            kill_category,
            damage_type,
            name,
            tags,
            traits,
            remaining_points,
        })
    }
    pub fn player_normalized(&self) -> Self {
        let mut result = self.clone();
        result.base[29] = 100;
        result.body = 0;
        result.experience_reward = 0;
        result.kill_category = 0;
        result
    }
    /// The big-endian sheet (SPEC-014 §3.43 writes it in binary mode): 428
    /// bytes, or the 432-byte Fallout 2 sheet when it has a damage type.
    pub fn write(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(GCD2_LEN);
        bytes.extend(self.flags.to_be_bytes());
        let words = self
            .base
            .iter()
            .chain(&self.bonus)
            .chain(&self.skills)
            .chain([&self.body, &self.experience_reward, &self.kill_category])
            .chain(&self.damage_type);
        for word in words {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(self.name);
        for word in self.tags.iter().chain(&self.traits) {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend(self.remaining_points.to_be_bytes());
        bytes
    }
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name.iter().position(|b| *b == 0).unwrap_or(32)]
    }
}
