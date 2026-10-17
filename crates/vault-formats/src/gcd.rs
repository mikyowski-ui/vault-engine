//! SPEC-006 428-byte sheet. Decoding and player-only normalization are distinct.
use crate::{ParseError, binary::Reader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcdFile {
    pub flags: u32,
    pub base: [i32; 35],
    pub bonus: [i32; 35],
    pub skills: [i32; 18],
    pub body: i32,
    pub experience_reward: i32,
    pub kill_category: i32,
    pub name: [u8; 32],
    pub tags: [i32; 4],
    pub traits: [i32; 2],
    pub remaining_points: i32,
}
impl GcdFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, "GCD")?;
        let flags = r.word()?;
        let base = r.words()?;
        let bonus = r.words()?;
        let skills = r.words()?;
        let body = r.be_i32()?;
        let experience_reward = r.be_i32()?;
        let kill_category = r.be_i32()?;
        let name: [u8; 32] = r.take(32)?.try_into().expect("fixed name");
        let tags: [i32; 4] = r.words()?;
        let traits: [i32; 2] = r.words()?;
        let remaining_points = r.be_i32()?;
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
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name.iter().position(|b| *b == 0).unwrap_or(32)]
    }
}
