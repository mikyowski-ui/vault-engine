//! SPEC-008 save slots: the 30051-byte `SAVE.DAT` header and its 27 ordered
//! payload components. Every multi-byte field is big-endian; there are no
//! component tags, lengths, or alignment gaps, so parsing needs the campaign
//! global count and prototype subtypes from outside the file.
use crate::{
    ParseError,
    binary::{Reader, put_i32, put_u32},
    map::MapObject,
    pro::{BasicFlags, PrototypeLookup},
};

pub const HEADER_LEN: usize = 30051;
pub const THUMBNAIL_SIZE: [usize; 2] = [224, 133];
pub const THUMBNAIL_LEN: usize = 224 * 133;
/// The reader compares the seventeen text characters and their NUL only.
pub const SIGNATURE: &[u8; 18] = b"FALLOUT SAVE FILE\0";
/// A safe manifest name: at most twelve bytes before its NUL.
pub const MAX_MAP_NAME: usize = 12;

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len())]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveHeader {
    pub signature: [u8; 24],
    /// Major and minor; the writer emits 1.1 and, under the SPEC-008 §2.4
    /// safe rule, the reader rejects any other version.
    pub version: [u16; 2],
    pub release: u8,
    pub name: [u8; 32],
    pub description: [u8; 30],
    /// Raw write order: day, month, year (loading labels the first two reversed).
    pub host_date: [u16; 3],
    /// The recovered writer stores hour plus minute, not HHMM.
    pub host_time: u32,
    /// Month, day, year.
    pub game_date: [u16; 3],
    /// Ten ticks per second.
    pub game_ticks: u32,
    pub elevation: u16,
    pub map: u16,
    pub map_file: [u8; 16],
    /// 224 x 133 palette indices, row-major, top to bottom, no padding.
    pub thumbnail: Vec<u8>,
    /// Zeroed by the writer and skipped by the reader.
    pub reserved: [u8; 128],
}

impl Default for SaveHeader {
    fn default() -> Self {
        let mut signature = [0; 24];
        signature[..18].copy_from_slice(SIGNATURE);
        Self {
            signature,
            version: [1, 1],
            release: b'R',
            name: [0; 32],
            description: [0; 30],
            host_date: [0; 3],
            host_time: 0,
            game_date: [0; 3],
            game_ticks: 0,
            elevation: 0,
            map: 0,
            map_file: [0; 16],
            thumbnail: vec![0; THUMBNAIL_LEN],
            reserved: [0; 128],
        }
    }
}

/// SPEC-018 §7.3 order 10: the Fallout 2 layout's perk-rank words.
pub const FALLOUT2_PERKS: usize = 119;
/// SPEC-018 §7.2 the Fallout 2 layout tag at `0x74E3`.
pub const FALLOUT2_TAG: &[u8; 16] = b"VAULTENG F2SAVE1";
/// SPEC-018 §7.2 the Fallout 2 save version, 1.2.
pub const FALLOUT2_VERSION: [u16; 2] = [1, 2];

impl SaveHeader {
    /// Strict complete-header validation: all 30051 bytes must be present,
    /// unlike the reference's seek-based loose reader.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        Self::parse_version(bytes, [1, 1])
    }
    /// The header of `game`'s layout: SPEC-008 §2.4's for Fallout 1;
    /// SPEC-018 §7.2's for Fallout 2, version exactly 1.2 with the layout
    /// tag (checks 1-3; the campaign count and gated mark depend on the
    /// install and are [`Self::check_fallout2`]'s).
    pub fn parse_for(bytes: &[u8], game: crate::GameProfile) -> Result<Self, ParseError> {
        match game {
            crate::GameProfile::Fallout1 => Self::parse(bytes),
            crate::GameProfile::Fallout2 => {
                let header = Self::parse_version(bytes, FALLOUT2_VERSION)?;
                if &header.reserved[..16] != FALLOUT2_TAG {
                    return Err(ParseError::new(
                        "SAVE",
                        0x74E3,
                        "not a Vault Engine Fallout 2 save (layout tag)",
                    ));
                }
                Ok(header)
            }
        }
    }
    /// SPEC-018 §7.2 checks 4 and 5: the stored G equals the installed
    /// campaign initializer's, and the gated mark is 0 or 1, with a mark of
    /// 1 loading only while Fallout 2 scripts are gated.
    pub fn check_fallout2(&self, installed: usize, gated: bool) -> Result<(), ParseError> {
        let stored = self.campaign_count();
        if usize::try_from(stored).ok() != Some(installed) {
            return Err(ParseError::new(
                "SAVE",
                0x74F3,
                format!(
                    "saved with {stored} campaign globals; the installed data/vault13.gam has \
                     {installed}"
                ),
            ));
        }
        match self.gated_mark() {
            0 => Ok(()),
            1 if gated => Ok(()),
            1 => Err(ParseError::new(
                "SAVE",
                0x74F7,
                "saved while Fallout 2 scripts were off; it cannot load once they run",
            )),
            mark => Err(ParseError::new(
                "SAVE",
                0x74F7,
                format!("gated mark {mark} is not 0 or 1"),
            )),
        }
    }
    /// SPEC-018 §7.2 the stored campaign-global count G (`0x74F3`).
    pub fn campaign_count(&self) -> u32 {
        u32::from_be_bytes(self.reserved[16..20].try_into().expect("word"))
    }
    /// SPEC-018 §7.2 the gated mark (`0x74F7`).
    pub fn gated_mark(&self) -> u8 {
        self.reserved[20]
    }
    /// SPEC-018 §7.2: version 1.2, and in the reserved bytes the tag, G and
    /// the gated mark, the other 107 bytes zero.
    pub fn set_fallout2(&mut self, campaign_count: u32, gated: bool) {
        self.version = FALLOUT2_VERSION;
        self.reserved = [0; 128];
        self.reserved[..16].copy_from_slice(FALLOUT2_TAG);
        self.reserved[16..20].copy_from_slice(&campaign_count.to_be_bytes());
        self.reserved[20] = u8::from(gated);
    }
    fn parse_version(bytes: &[u8], expected: [u16; 2]) -> Result<Self, ParseError> {
        let mut r = Reader::unbounded(bytes, "SAVE");
        if bytes.len() < HEADER_LEN {
            return Err(r.error("save header requires 30051 bytes"));
        }
        let signature: [u8; 24] = r.take(24)?.try_into().expect("signature");
        if &signature[..18] != SIGNATURE {
            return Err(ParseError::new("SAVE", 0, "save signature mismatch"));
        }
        let version = [r.be_u16()?, r.be_u16()?];
        if version != expected {
            return Err(ParseError::new(
                "SAVE",
                0x18,
                format!("save version is not {}.{}", expected[0], expected[1]),
            ));
        }
        let release = r.take(1)?[0];
        let name = r.take(32)?.try_into().expect("name");
        let description = r.take(30)?.try_into().expect("description");
        let host_date = [r.be_u16()?, r.be_u16()?, r.be_u16()?];
        let host_time = r.word()?;
        let game_date = [r.be_u16()?, r.be_u16()?, r.be_u16()?];
        let game_ticks = r.word()?;
        let elevation = r.be_u16()?;
        let map = r.be_u16()?;
        let map_file = r.take(16)?.try_into().expect("map file");
        let thumbnail = r.take(THUMBNAIL_LEN)?.to_vec();
        let reserved = r.take(128)?.try_into().expect("reserved");
        debug_assert_eq!(r.pos, HEADER_LEN);
        Ok(Self {
            signature,
            version,
            release,
            name,
            description,
            host_date,
            host_time,
            game_date,
            game_ticks,
            elevation,
            map,
            map_file,
            thumbnail,
            reserved,
        })
    }
    pub fn write(&self) -> Result<Vec<u8>, ParseError> {
        if self.thumbnail.len() != THUMBNAIL_LEN {
            return Err(ParseError::new(
                "SAVE",
                0x83,
                "thumbnail must be 224 x 133 bytes",
            ));
        }
        let out = &mut Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(&self.signature);
        put_u16(out, self.version[0]);
        put_u16(out, self.version[1]);
        out.push(self.release);
        out.extend_from_slice(&self.name);
        out.extend_from_slice(&self.description);
        for value in self.host_date {
            put_u16(out, value);
        }
        put_u32(out, self.host_time);
        for value in self.game_date {
            put_u16(out, value);
        }
        put_u32(out, self.game_ticks);
        put_u16(out, self.elevation);
        put_u16(out, self.map);
        out.extend_from_slice(&self.map_file);
        out.extend_from_slice(&self.thumbnail);
        out.extend_from_slice(&self.reserved);
        Ok(std::mem::take(out))
    }
    pub fn name_text(&self) -> &[u8] {
        text(&self.name)
    }
    pub fn description_text(&self) -> &[u8] {
        text(&self.description)
    }
    pub fn map_file_text(&self) -> &[u8] {
        text(&self.map_file)
    }
}

/// The 368-byte critter-stat prefix shared with SPEC-006 sheets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CritterStats {
    pub flags: u32,
    pub base: [i32; 35],
    pub bonus: [i32; 35],
    pub skills: [i32; 18],
    pub body: i32,
    pub experience: i32,
    pub kill_category: i32,
}

/// Combat component: four bytes when inactive, else `32 + 4 x C`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatSave {
    pub state: u32,
    pub turn_running: i32,
    pub free_movement: i32,
    pub experience: i32,
    pub combatants: i32,
    pub noncombatants: i32,
    pub player_id: i32,
    /// Combat identities in list order (not unique object identifiers).
    pub list: Vec<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePayload {
    /// Type 0: three stat indices and three modifiers.
    Drug {
        stats: [i32; 3],
        modifiers: [i32; 3],
    },
    /// Type 2: start/end indication, item prototype, perk index.
    Withdrawal([i32; 3]),
    /// Type 3: script instance identifier and fixed parameter.
    Script { script: i32, parameter: i32 },
    /// Type 6: radiation level and healing indication.
    Radiation { level: i32, healing: i32 },
    /// Types 1, 4, 5, 7-12: knockout, midnight, poison, flare, explosion,
    /// item drain, sneak, failed explosion, periodic map update.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueRecord {
    pub due: u32,
    pub kind: i32,
    /// Unique owner-object identifier, or -2 for none.
    pub owner: i32,
    pub payload: QueuePayload,
}

/// World-map component: 1028 bytes at payload order 20.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMapSave {
    /// 31 rows x 29 columns (the larger saved stride), row-major.
    pub discovery: Vec<u8>,
    /// 15 towns x 7 entrance slots, town-major.
    pub entrances: Vec<u8>,
    pub towns: u32,
    pub specials: u32,
    pub town: i32,
    pub section: i32,
    pub position: [u32; 2],
}
impl Default for WorldMapSave {
    fn default() -> Self {
        Self {
            discovery: vec![0; 899],
            entrances: vec![0; 105],
            towns: 0,
            specials: 0,
            town: 0,
            section: 0,
            position: [0; 2],
        }
    }
}

/// The 27 ordered payload components (zero-byte positions have no field).
#[derive(Debug, Clone)]
pub struct SaveGame {
    pub header: SaveHeader,
    /// 1: player combat identity.
    pub player_combat_id: i32,
    /// 2: campaign snapshot A and its water-warning/movie-status byte.
    pub campaign: Vec<i32>,
    pub campaign_status: u8,
    /// 3: companion-map manifest and the automap companion byte length.
    pub maps: Vec<Vec<u8>>,
    pub automap_length: i32,
    /// 4: campaign snapshot B (consumed and discarded on load).
    pub campaign_b: Vec<i32>,
    pub campaign_b_status: u8,
    /// 5: the player object tree and viewport-center hex.
    pub player: MapObject,
    pub viewport: i32,
    /// 6: sneak-working value and the critter-stat prefix.
    pub sneak_working: i32,
    pub stats: CritterStats,
    /// 7, 8: kill counters and tagged skills.
    pub kills: [i32; 15],
    pub tags: [i32; 4],
    /// 10: perk ranks.
    pub perks: [i32; 64],
    /// 11: combat state.
    pub combat: CombatSave,
    /// 13: unspent skill points, level, experience, reputation, karma.
    pub progression: [i32; 5],
    /// 15: timed queue.
    pub queue: Vec<QueueRecord>,
    /// 16, 17: traits and automap display flags (0x1 in-game, 0x2 high
    /// detail, 0x4 scanner).
    pub traits: [i32; 2],
    pub automap_flags: u32,
    /// 18: twenty preference words; 13, 18, 19 are binary32.
    pub preferences: [u32; 20],
    /// 19: last reconciled level and pending-perk byte.
    pub reconciled_level: i32,
    pub pending_perk: u8,
    /// 20: world map.
    pub world_map: WorldMapSave,
    /// 22: played-movie flags.
    pub movies: [u8; 14],
    /// 23: per skill, three game-tick timestamps (skill-major).
    pub skill_uses: [[i32; 3]; 18],
    /// 24: party count, next item identity, and `max(P - 1, 0)` identifiers.
    pub party_count: i32,
    pub next_item_id: i32,
    pub party: Vec<i32>,
    /// 25: enabled, hidden, selected hand/item, end-turn panel open.
    pub interface: [i32; 4],
    /// SPEC-018 §7.3: the Fallout 2 layout's changed components; `None`
    /// for a Fallout 1 save.
    pub fallout2: Option<Box<Fallout2Payload>>,
}

/// SPEC-018 §7.3 what the Fallout 2 layout stores in place of, or after,
/// SPEC-008's components. With it, [`SaveGame::world_map`] and
/// [`SaveGame::movies`] are not written, and perk words 64-118 are written
/// 0 and ignored on load.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fallout2Payload {
    /// Order 7: kill types 15-18, after [`SaveGame::kills`]' 0-14.
    pub kills: [i32; 4],
    /// Order 20 (§7.4).
    pub world_map: crate::fallout2::save::WorldMapSave2,
    /// Order 22: one byte per SPEC-018 §6.4 movie index.
    pub movies: [u8; 17],
    /// Order 24's member records (§7.5), one per [`SaveGame::party`]
    /// identifier.
    pub records: Vec<crate::fallout2::save::MemberRecord>,
    /// Order 24's retained records, in ascending identifier order.
    pub retained: Vec<(i32, crate::fallout2::save::MemberRecord)>,
}

impl SaveGame {
    /// Preference words 13, 18, 19 (text delay, brightness, mouse
    /// sensitivity) are IEEE binary32.
    pub fn preference_float(&self, index: usize) -> Option<f32> {
        matches!(index, 13 | 18 | 19).then(|| f32::from_bits(self.preferences[index]))
    }
    /// Parse a complete `SAVE.DAT`. `campaign_count` is the installed
    /// campaign initializer's G, which is not stored before either snapshot.
    pub fn parse(
        bytes: &[u8],
        campaign_count: usize,
        prototypes: &impl PrototypeLookup,
    ) -> Result<Self, ParseError> {
        Self::parse_with(bytes, campaign_count, prototypes, None)
    }
    /// Parse a complete SPEC-018 §7 Fallout 2 `SAVE.DAT` whose counts must
    /// equal the installed tables' `shape` (§7.4, §7.5). The header's G and
    /// gated mark are checked by [`SaveHeader::check_fallout2`].
    pub fn parse_fallout2(
        bytes: &[u8],
        campaign_count: usize,
        prototypes: &impl PrototypeLookup,
        shape: &crate::fallout2::save::Fallout2Shape,
    ) -> Result<Self, ParseError> {
        Self::parse_with(bytes, campaign_count, prototypes, Some(shape))
    }
    fn parse_with(
        bytes: &[u8],
        campaign_count: usize,
        prototypes: &impl PrototypeLookup,
        shape: Option<&crate::fallout2::save::Fallout2Shape>,
    ) -> Result<Self, ParseError> {
        let header = match shape {
            None => SaveHeader::parse(bytes)?,
            Some(_) => SaveHeader::parse_for(bytes, crate::GameProfile::Fallout2)?,
        };
        let mut r = Reader::unbounded(bytes, "SAVE");
        r.take(HEADER_LEN)?;
        let player_combat_id = r.be_i32()?;
        if campaign_count > 1_000_000 {
            return Err(r.error("campaign count exceeds resource limit"));
        }
        let snapshot = |r: &mut Reader<'_>| -> Result<(Vec<i32>, u8), ParseError> {
            let values = (0..campaign_count)
                .map(|_| r.be_i32())
                .collect::<Result<_, _>>()?;
            Ok((values, r.take(1)?[0]))
        };
        let (campaign, campaign_status) = snapshot(&mut r)?;
        let count = r.be_i32()?;
        if count <= 0 || count > 100_000 {
            return Err(r.error("map manifest needs at least one saved map"));
        }
        let mut maps = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let start = r.pos;
            let mut name = Vec::new();
            loop {
                let byte = r.take(1)?[0];
                if byte == 0 {
                    break;
                }
                if name.len() == MAX_MAP_NAME {
                    return Err(ParseError::new(
                        "SAVE",
                        start,
                        "manifest name exceeds twelve bytes before its NUL",
                    ));
                }
                name.push(byte);
            }
            maps.push(name);
        }
        let automap_length = r.be_i32()?;
        let (campaign_b, campaign_b_status) = snapshot(&mut r)?;
        let game = match shape {
            None => crate::GameProfile::Fallout1,
            Some(_) => crate::GameProfile::Fallout2,
        };
        let player = MapObject::read_tree(&mut r, prototypes, header.elevation.min(2) as u8, game)?;
        let viewport = r.be_i32()?;
        let sneak_working = r.be_i32()?;
        let stats = CritterStats {
            flags: r.word()?,
            base: r.words()?,
            bonus: r.words()?,
            skills: r.words()?,
            body: r.be_i32()?,
            experience: r.be_i32()?,
            kill_category: r.be_i32()?,
        };
        let kills = r.words()?;
        // SPEC-018 §7.3 order 7: kill types 15-18 follow under Fallout 2.
        let more_kills: [i32; 4] = match shape {
            Some(_) => r.words()?,
            None => [0; 4],
        };
        let tags = r.words()?;
        let perks = r.words()?;
        // Order 10: perk words 64-118 are ignored on load.
        if shape.is_some() {
            r.take(4 * (FALLOUT2_PERKS - 64))?;
        }
        let state = r.word()?;
        let combat = if state & 1 == 0 {
            CombatSave {
                state,
                turn_running: 0,
                free_movement: 0,
                experience: 0,
                combatants: 0,
                noncombatants: 0,
                player_id: 0,
                list: Vec::new(),
            }
        } else {
            let fields: [i32; 7] = r.words()?;
            let total = usize::try_from(fields[5])
                .ok()
                .filter(|total| *total <= 100_000)
                .ok_or_else(|| r.error("combat list count out of range"))?;
            let list = (0..total).map(|_| r.be_i32()).collect::<Result<_, _>>()?;
            CombatSave {
                state,
                turn_running: fields[0],
                free_movement: fields[1],
                experience: fields[2],
                combatants: fields[3],
                noncombatants: fields[4],
                player_id: fields[6],
                list,
            }
        };
        let progression = r.words()?;
        let events = r.be_i32()?;
        if !(0..=100_000).contains(&events) {
            return Err(r.error("timed queue count out of range"));
        }
        let mut queue = Vec::with_capacity(events as usize);
        for _ in 0..events {
            let due = r.word()?;
            let kind = r.be_i32()?;
            let owner = r.be_i32()?;
            let payload = match kind {
                0 => QueuePayload::Drug {
                    stats: r.words()?,
                    modifiers: r.words()?,
                },
                2 => QueuePayload::Withdrawal(r.words()?),
                3 => QueuePayload::Script {
                    script: r.be_i32()?,
                    parameter: r.be_i32()?,
                },
                6 => QueuePayload::Radiation {
                    level: r.be_i32()?,
                    healing: r.be_i32()?,
                },
                1 | 4 | 5 | 7..=12 => QueuePayload::None,
                // SPEC-008 §2.6 [May-improve]: the reference indexes its
                // handler table with the stored type unchecked; a type
                // outside 0-12 is rejected instead.
                _ => return Err(r.error(format!("unknown timed-event type {kind}"))),
            };
            queue.push(QueueRecord {
                due,
                kind,
                owner,
                payload,
            });
        }
        let traits = r.words()?;
        let automap_flags = r.word()?;
        let preferences = r.words::<20>()?.map(|word| word as u32);
        let reconciled_level = r.be_i32()?;
        let pending_perk = r.take(1)?[0];
        let (world_map, fallout2_map) = match shape {
            None => (
                WorldMapSave {
                    discovery: r.take(899)?.to_vec(),
                    entrances: r.take(105)?.to_vec(),
                    towns: r.word()?,
                    specials: r.word()?,
                    town: r.be_i32()?,
                    section: r.be_i32()?,
                    position: [r.word()?, r.word()?],
                },
                None,
            ),
            Some(shape) => (
                WorldMapSave::default(),
                Some(crate::fallout2::save::WorldMapSave2::read(&mut r, shape)?),
            ),
        };
        let (movies, fallout2_movies) = match shape {
            None => (r.take(14)?.try_into().expect("movies"), None),
            Some(_) => ([0; 14], Some(r.take(17)?.try_into().expect("movies"))),
        };
        let mut skill_uses = [[0; 3]; 18];
        for skill in &mut skill_uses {
            *skill = r.words()?;
        }
        let (party_count, next_item_id, party, fallout2_party) = match shape {
            None => {
                let party_count = r.be_i32()?;
                let next_item_id = r.be_i32()?;
                if !(0..=100_000).contains(&party_count) {
                    return Err(r.error("party count out of range"));
                }
                let party = (0..(party_count - 1).max(0))
                    .map(|_| r.be_i32())
                    .collect::<Result<_, _>>()?;
                (party_count, next_item_id, party, None)
            }
            Some(shape) => {
                let party = crate::fallout2::save::PartySave2::read(&mut r, shape)?;
                (
                    party.count,
                    party.next_item_id,
                    party.members,
                    Some((party.records, party.retained)),
                )
            }
        };
        let interface = r.words()?;
        if r.pos != bytes.len() {
            return Err(r.error("trailing bytes after the finalization position"));
        }
        Ok(Self {
            header,
            player_combat_id,
            campaign,
            campaign_status,
            maps,
            automap_length,
            campaign_b,
            campaign_b_status,
            player,
            viewport,
            sneak_working,
            stats,
            kills,
            tags,
            perks,
            combat,
            progression,
            queue,
            traits,
            automap_flags,
            preferences,
            reconciled_level,
            pending_perk,
            world_map,
            movies,
            skill_uses,
            party_count,
            next_item_id,
            party,
            interface,
            fallout2: match (fallout2_map, fallout2_movies, fallout2_party) {
                (Some(world_map), Some(movies), Some((records, retained))) => {
                    Some(Box::new(Fallout2Payload {
                        kills: more_kills,
                        world_map,
                        movies,
                        records,
                        retained,
                    }))
                }
                _ => None,
            },
        })
    }
    /// Serialize every component in order. The player tree is written with
    /// its temporary flag clear and script association -1 throughout.
    pub fn write(&self) -> Result<Vec<u8>, ParseError> {
        let error = |message: &str| ParseError::new("SAVE", 0, message);
        let out = &mut self.header.write()?;
        put_i32(out, self.player_combat_id);
        if self.campaign_b.len() != self.campaign.len() {
            return Err(error("both campaign snapshots must have G values"));
        }
        let snapshot = |out: &mut Vec<u8>, values: &[i32], status: u8| {
            for value in values {
                put_i32(out, *value);
            }
            out.push(status);
        };
        snapshot(out, &self.campaign, self.campaign_status);
        if self.maps.is_empty() {
            return Err(error("map manifest needs at least one saved map"));
        }
        put_i32(out, self.maps.len() as i32);
        for name in &self.maps {
            if name.len() > MAX_MAP_NAME || name.contains(&0) {
                return Err(error("manifest names must be 1-12 non-NUL bytes"));
            }
            out.extend_from_slice(name);
            out.push(0);
        }
        put_i32(out, self.automap_length);
        snapshot(out, &self.campaign_b, self.campaign_b_status);
        let mut player = self.player.clone();
        clear_tree(&mut player);
        let game = match self.fallout2 {
            None => crate::GameProfile::Fallout1,
            Some(_) => crate::GameProfile::Fallout2,
        };
        player.write_tree(out, game)?;
        put_i32(out, self.viewport);
        put_i32(out, self.sneak_working);
        put_u32(out, self.stats.flags);
        for value in self
            .stats
            .base
            .iter()
            .chain(&self.stats.bonus)
            .chain(&self.stats.skills)
            .chain(&[
                self.stats.body,
                self.stats.experience,
                self.stats.kill_category,
            ])
            .chain(&self.kills)
        {
            put_i32(out, *value);
        }
        let fallout2 = self.fallout2.as_deref();
        if let Some(fallout2) = fallout2 {
            for value in fallout2.kills {
                put_i32(out, value);
            }
        }
        for value in self.tags.iter().chain(&self.perks) {
            put_i32(out, *value);
        }
        if fallout2.is_some() {
            out.extend(std::iter::repeat_n(0, 4 * (FALLOUT2_PERKS - 64)));
        }
        put_u32(out, self.combat.state);
        if self.combat.state & 1 != 0 {
            for value in [
                self.combat.turn_running,
                self.combat.free_movement,
                self.combat.experience,
                self.combat.combatants,
                self.combat.noncombatants,
                self.combat.list.len() as i32,
                self.combat.player_id,
            ]
            .iter()
            .chain(&self.combat.list)
            {
                put_i32(out, *value);
            }
        }
        for value in self.progression {
            put_i32(out, value);
        }
        put_i32(out, self.queue.len() as i32);
        for record in &self.queue {
            put_u32(out, record.due);
            put_i32(out, record.kind);
            put_i32(out, record.owner);
            let payload: Vec<i32> = match record.payload {
                QueuePayload::Drug { stats, modifiers } => {
                    stats.iter().chain(&modifiers).copied().collect()
                }
                QueuePayload::Withdrawal(values) => values.to_vec(),
                QueuePayload::Script { script, parameter } => vec![script, parameter],
                QueuePayload::Radiation { level, healing } => vec![level, healing],
                QueuePayload::None => Vec::new(),
            };
            let expected = match record.kind {
                0 => 6,
                2 => 3,
                3 | 6 => 2,
                1 | 4 | 5 | 7..=12 => 0,
                _ => return Err(error("unknown timed-event type")),
            };
            if payload.len() != expected {
                return Err(error("timed-event payload does not match its type"));
            }
            for value in payload {
                put_i32(out, value);
            }
        }
        for value in self.traits {
            put_i32(out, value);
        }
        put_u32(out, self.automap_flags);
        for value in self.preferences {
            put_u32(out, value);
        }
        put_i32(out, self.reconciled_level);
        out.push(self.pending_perk);
        if let Some(fallout2) = fallout2 {
            fallout2.world_map.write(out);
            out.extend_from_slice(&fallout2.movies);
        } else {
            let map = &self.world_map;
            if map.discovery.len() != 899 || map.entrances.len() != 105 {
                return Err(error("world-map arrays must be 899 and 105 bytes"));
            }
            out.extend_from_slice(&map.discovery);
            out.extend_from_slice(&map.entrances);
            put_u32(out, map.towns);
            put_u32(out, map.specials);
            put_i32(out, map.town);
            put_i32(out, map.section);
            put_u32(out, map.position[0]);
            put_u32(out, map.position[1]);
            out.extend_from_slice(&self.movies);
        }
        for value in self.skill_uses.iter().flatten() {
            put_i32(out, *value);
        }
        if let Some(fallout2) = fallout2 {
            crate::fallout2::save::PartySave2 {
                count: self.party_count,
                next_item_id: self.next_item_id,
                members: self.party.clone(),
                records: fallout2.records.clone(),
                retained: fallout2.retained.clone(),
            }
            .write(out)?;
        } else {
            if self.party.len() != (self.party_count - 1).max(0) as usize {
                return Err(error("party identifiers must number max(P - 1, 0)"));
            }
            put_i32(out, self.party_count);
            put_i32(out, self.next_item_id);
            for value in &self.party {
                put_i32(out, *value);
            }
        }
        for value in &self.interface {
            put_i32(out, *value);
        }
        Ok(std::mem::take(out))
    }
}

/// SPEC-008 §2.5: only the player's own top-level record is altered, with
/// the temporary flag clear and the script instance word `+0x40` at -1; the
/// cached script-file word `+0x44` and every inventory child are written
/// unchanged.
fn clear_tree(object: &mut MapObject) {
    object.flags.0 &= !BasicFlags::TEMPORARY.0;
    object.script_id = -1;
}
