//! SPEC-003 version-19 map streams (and SPEC-016's version 20, the same
//! layout, under the Fallout 2 profile), with bounded recursive inventories.
use crate::{
    ParseError,
    binary::{MAX_ENTRIES, Reader, put_i32, put_u32},
    pro::*,
};
use std::collections::BTreeMap;

pub const MAX_OBJECTS: usize = 100_000;
pub const MAX_INVENTORY_DEPTH: usize = 64;
pub const MAX_SCRIPTS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePair {
    pub stored: u32,
    pub word: u32,
}
impl TilePair {
    pub const BLANK: Self = Self {
        stored: 0x00010001,
        word: 0x00010001,
    };
    pub fn floor(self) -> u16 {
        (self.word & 0xfff) as u16
    }
    pub fn roof(self) -> u16 {
        ((self.word >> 16) & 0xfff) as u16
    }
    pub fn floor_hidden(self) -> bool {
        self.word & 0x1000 != 0
    }
    pub fn roof_hidden(self) -> bool {
        self.word & 0x10000000 != 0
    }
    pub fn roof_barrier(self) -> bool {
        self.word & 0x20000000 != 0
    }
}
#[derive(Debug, Clone)]
pub struct MapHeader {
    /// 19 (SPEC-003) or 20 (SPEC-016), kept so a written map keeps it.
    pub version: i32,
    pub name: [u8; 16],
    pub entry_hex: i32,
    pub entry_elevation: u8,
    pub entry_orientation: u8,
    pub stored_local_count: i32,
    pub script_reference: i32,
    pub flags: MapFlags,
    pub darkness: i32,
    pub stored_global_count: i32,
    pub map_number: i32,
    pub last_visit: u32,
    pub reserved: [i32; 44],
}
impl MapHeader {
    pub fn map_script_index(&self) -> Option<u32> {
        (self.script_reference > 0).then(|| (self.script_reference - 1) as u32)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptPrefix {
    None,
    Spatial {
        location: PackedLocation,
        radius: i32,
    },
    Timed(i32),
}
#[derive(Debug, Clone)]
pub struct MapScript {
    pub id: u32,
    pub link: i32,
    pub prefix: ScriptPrefix,
    pub stored_flags: ScriptFlags,
    pub flags: ScriptFlags,
    pub file_index: i32,
    pub object_id: i32,
    pub local_base: i32,
    pub stored_local_count: i32,
    pub local_count: i32,
    pub result: i32,
    pub event: i32,
    pub parameter: i32,
    pub action: i32,
    pub override_default: i32,
    pub execution_state: i32,
    pub quantity: i32,
    pub annotations: u32,
    pub file_offset: usize,
}
#[derive(Debug, Clone)]
pub struct ScriptBlock {
    pub slots: Vec<MapScript>,
    pub active: usize,
}
#[derive(Debug, Clone)]
pub struct ScriptCategory {
    pub declared_count: usize,
    pub blocks: Vec<ScriptBlock>,
}
impl ScriptCategory {
    pub fn active(&self) -> impl Iterator<Item = &MapScript> {
        self.blocks
            .iter()
            .flat_map(|block| block.slots.iter().take(block.active))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CritterState {
    pub reaction: i32,
    pub damage: i32,
    pub maneuvers: u32,
    pub ap: i32,
    pub results: CombatFlags,
    pub ai: i32,
    pub team: i32,
    pub attacker: i32,
    pub hp: i32,
    pub radiation: i32,
    pub poison: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDetail {
    Plain,
    Weapon {
        rounds: i32,
        ammo_pid: i32,
    },
    Ammunition(i32),
    Charges(i32),
    Key(i32),
    Door(LockFlags),
    Stairs {
        map: i32,
        destination: PackedLocation,
    },
    Elevator {
        kind: i32,
        level: i32,
    },
    Ladder(PackedLocation),
    ExitGrid {
        map: i32,
        hex: i32,
        elevation: i32,
        orientation: i32,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectState {
    Critter(CritterState),
    Other { word: u32, detail: ObjectDetail },
}
#[derive(Debug, Clone)]
pub struct InventoryEntry {
    pub quantity: u32,
    pub object: MapObject,
}
#[derive(Debug, Clone)]
pub struct MapObject {
    pub id: u32,
    pub hex: Option<i32>,
    pub displacement: [i32; 2],
    pub screen: [i32; 2],
    pub frame: usize,
    pub orientation: u8,
    pub art_id: u32,
    pub flags: BasicFlags,
    pub stored_elevation: i32,
    pub elevation: u8,
    pub pid: u32,
    pub combat_id: i32,
    pub light: Light,
    /// The runtime outline word (SPEC-014 §3.44, [`outline`]): discarded on
    /// load (SPEC-003 §2.10), written as it stands on save.
    pub outline: u32,
    pub script_id: i32,
    pub cached_script_file: i32,
    pub inventory_capacity: usize,
    pub state: ObjectState,
    pub inventory: Vec<InventoryEntry>,
    pub owner: Option<u32>,
    pub file_offset: usize,
}
/// SPEC-014 §3.44 the 32-bit runtime outline word: the type in bits 0-23,
/// compared whole, the translucent bit 30 and the disabled bit 31.
pub mod outline {
    /// Bits 0-23, the outline type; 0 is no outline.
    pub const TYPE: u32 = 0x00FF_FFFF;
    /// Translucent, honored for the red, grey and item types only.
    pub const TRANSLUCENT: u32 = 0x4000_0000;
    /// The type is kept, but the object is not outlined.
    pub const DISABLED: u32 = 0x8000_0000;
    pub const HOSTILE: u32 = 1;
    pub const RED: u32 = 2;
    pub const GREY: u32 = 4;
    pub const FRIENDLY: u32 = 8;
    pub const ITEM: u32 = 16;

    /// The word's outline type (0 for none).
    pub fn kind(word: u32) -> u32 {
        word & TYPE
    }
    /// Whether a word draws an outline: a type is set and it is not
    /// disabled.
    pub fn outlined(word: u32) -> bool {
        kind(word) != 0 && word & DISABLED == 0
    }
}
#[derive(Debug, Clone)]
pub struct MapObjects {
    pub total: usize,
    pub elevations: [Vec<MapObject>; 3],
    pub consumed_len: usize,
}
#[derive(Debug, Clone)]
pub struct MapFile {
    pub header: MapHeader,
    pub mvars: Vec<i32>,
    pub lvars: Vec<i32>,
    pub tiles: [Option<Vec<TilePair>>; 3],
    pub scripts: [ScriptCategory; 5],
    pub objects: MapObjects,
    pub consumed_len: usize,
    /// Safe-rule substitutions made while reading (not serialized).
    pub diagnostics: Vec<String>,
}

fn pool(r: &mut Reader<'_>, count: i32) -> Result<Vec<i32>, ParseError> {
    let count = count.max(0) as usize;
    if count > MAX_ENTRIES {
        return Err(r.error("variable pool exceeds resource limit"));
    }
    let bytes = r.take(count * 4)?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| i32::from_be_bytes(*word))
        .collect())
}
fn script(r: &mut Reader<'_>, saved: bool) -> Result<MapScript, ParseError> {
    let file_offset = r.pos;
    let id = r.word()?;
    let link = r.be_i32()?;
    // Padding slots still dispatch on their own ID. Unknown inactive categories
    // have no conditional prefix; active identifiers are validated after occupancy.
    let prefix = match id >> 24 {
        1 => ScriptPrefix::Spatial {
            location: PackedLocation(r.word()?),
            radius: r.be_i32()?,
        },
        2 => ScriptPrefix::Timed(r.be_i32()?),
        _ => ScriptPrefix::None,
    };
    let tail: [i32; 14] = r.words()?;
    Ok(MapScript {
        id,
        link,
        prefix,
        stored_flags: ScriptFlags(tail[0] as u32),
        flags: ScriptFlags(tail[0] as u32 & !1),
        file_index: tail[1],
        object_id: tail[3],
        local_base: tail[4],
        stored_local_count: tail[5],
        local_count: if saved { tail[5] } else { 0 },
        result: tail[6],
        event: tail[7],
        parameter: tail[8],
        action: tail[9],
        override_default: tail[10],
        execution_state: tail[11],
        quantity: tail[12],
        annotations: tail[13] as u32,
        file_offset,
    })
}
fn script_category(
    r: &mut Reader<'_>,
    category: usize,
    saved: bool,
    locals: usize,
    total: &mut usize,
    diagnostics: &mut Vec<String>,
) -> Result<ScriptCategory, ParseError> {
    // SPEC-003 §2.9 [May-improve]: a negative count (which the reference
    // reads at least one block for) is rejected, and the counts are bounded.
    let count = r.count("script count", MAX_SCRIPTS)?;
    *total = total
        .checked_add(count)
        .ok_or_else(|| r.error("script count overflow"))?;
    if *total > MAX_SCRIPTS {
        return Err(r.error("total script limit exceeded"));
    }
    let mut blocks = Vec::new();
    let mut occupied = 0;
    for _ in 0..count.div_ceil(16) {
        let mut slots = Vec::with_capacity(16);
        for _ in 0..16 {
            slots.push(script(r, saved)?);
        }
        // SPEC-003 §2.9 [May-improve]: an active-slot count that is
        // negative or exceeds 16 is rejected (the reference does not check).
        let active = r.count("active block slots", 16)?;
        r.take(4)?; // obsolete next-block address
        occupied += active;
        for slot in slots.iter().take(active) {
            // SPEC-003 §2.9: every slot dispatches on its own identifier
            // and no rule requires an active one to match the outer
            // stream, so a mismatch is reported rather than rejected.
            if slot.id >> 24 != category as u32 {
                diagnostics.push(format!(
                    "script {:#010x} at byte {:#x} is active in the category {category} stream",
                    slot.id, slot.file_offset
                ));
            }
            // SPEC-003 §3.3 [May-improve]: an out-of-range file index or
            // local slice is rejected before it can index memory.
            if slot.file_index < 0 || slot.local_base < -1 || slot.stored_local_count < 0 {
                return Err(r.error("invalid active script index/local slice"));
            }
            // SPEC-003 §3.2: an out-of-range local-pool access is a runtime
            // error that reads zero, not a load failure.
            if saved
                && slot.local_base >= 0
                && slot.local_base as u64 + slot.local_count as u64 > locals as u64
            {
                diagnostics.push(format!(
                    "script {:#010x} at byte {:#x}: local slice {}+{} extends beyond the {locals}-element pool",
                    slot.id, slot.file_offset, slot.local_base, slot.local_count
                ));
            }
        }
        blocks.push(ScriptBlock { slots, active });
    }
    if occupied != count {
        return Err(r.error("script block occupancy does not match category count"));
    }
    Ok(ScriptCategory {
        declared_count: count,
        blocks,
    })
}

impl MapFile {
    /// A Fallout 1 map (SPEC-003, version 19).
    pub fn parse(bytes: &[u8], prototypes: &impl PrototypeLookup) -> Result<Self, ParseError> {
        Self::parse_for(bytes, prototypes, crate::GameProfile::Fallout1)
    }
    /// A map of `game`'s version only (SPEC-003 19, SPEC-016 20; the same
    /// layout).
    pub fn parse_for(
        bytes: &[u8],
        prototypes: &impl PrototypeLookup,
        game: crate::GameProfile,
    ) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, "MAP")?;
        let version = r.be_i32()?;
        if version != game.map_version() {
            return Err(r.error(format!(
                "map version {version}; {game:?} maps are version {}",
                game.map_version()
            )));
        }
        let name = r.take(16)?.try_into().expect("fixed name");
        let entry_hex = r.be_i32()?;
        if !(0..40000).contains(&entry_hex) {
            return Err(r.error("entry hex outside grid"));
        }
        let entry_elevation = r.count("entry elevation", 2)? as u8;
        // SPEC-003 §3.2 safe rule: an orientation outside 0-5 reads as 0.
        let mut diagnostics = Vec::new();
        let orientation = r.be_i32()?;
        let entry_orientation = u8::try_from(orientation)
            .ok()
            .filter(|orientation| *orientation <= 5)
            .unwrap_or_else(|| {
                diagnostics.push(format!(
                    "map entry orientation {orientation} is outside 0-5; using 0"
                ));
                0
            });
        let stored_local_count = r.be_i32()?;
        let script_reference = r.be_i32()?;
        let flags = MapFlags(r.word()?);
        let darkness = r.be_i32()?;
        let stored_global_count = r.be_i32()?;
        let map_number = r.be_i32()?;
        let last_visit = r.word()?;
        let reserved = r.words()?;
        let header = MapHeader {
            version,
            name,
            entry_hex,
            entry_elevation,
            entry_orientation,
            stored_local_count,
            script_reference,
            flags,
            darkness,
            stored_global_count,
            map_number,
            last_visit,
            reserved,
        };
        let mvars = pool(&mut r, stored_global_count)?;
        let lvars = pool(&mut r, stored_local_count)?;
        let mut tiles = std::array::from_fn(|_| None);
        for (elevation, tile_block) in tiles.iter_mut().enumerate() {
            if flags.0 & (2 << elevation) == 0 {
                let data = r.take(40000)?;
                *tile_block = Some(
                    data.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|bytes| {
                            let stored = u32::from_be_bytes(*bytes);
                            TilePair {
                                stored,
                                word: stored & !0x10000000,
                            }
                        })
                        .collect(),
                );
            }
        }
        let mut scripts = Vec::with_capacity(5);
        let mut total = 0;
        for category in 0..5 {
            scripts.push(script_category(
                &mut r,
                category,
                flags.contains(MapFlags::SAVED),
                lvars.len(),
                &mut total,
                &mut diagnostics,
            )?);
        }
        let scripts: [ScriptCategory; 5] = scripts.try_into().expect("five categories");
        // SPEC-003 §3.3: a lookup searches only the stream the identifier's
        // top byte names, in block and slot order, and the first equal
        // record binds. A repeated identifier, or one in another
        // category's stream, is never found; it is reported.
        let mut bindings = BTreeMap::new();
        for (stream, category) in scripts.iter().enumerate() {
            for record in category.active() {
                if record.id >> 24 != stream as u32 {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(slot) = bindings.entry(record.id)
                {
                    slot.insert(record.file_index);
                } else {
                    diagnostics.push(format!(
                        "duplicate active script {:#010x} at byte {:#x}; the first record binds",
                        record.id, record.file_offset
                    ));
                }
            }
        }
        let mut objects = MapObjects::read(
            &mut r,
            prototypes,
            flags.contains(MapFlags::SAVED),
            &mut diagnostics,
        )?;
        for object in objects.elevations.iter_mut().flatten() {
            object.restore_scripts(&bindings, true);
        }
        Ok(Self {
            header,
            mvars,
            lvars,
            tiles,
            scripts,
            objects,
            consumed_len: r.pos,
            diagnostics,
        })
    }
    pub fn tile(&self, elevation: usize, index: usize) -> Option<TilePair> {
        if index >= 10000 {
            return None;
        }
        Some(
            self.tiles
                .get(elevation)?
                .as_ref()
                .map_or(TilePair::BLANK, |tiles| tiles[index]),
        )
    }
    pub fn mvar(&self, index: usize) -> Option<i32> {
        self.mvars.get(index).copied()
    }
    pub fn lvar(&self, script: &MapScript, index: usize) -> Option<i32> {
        if script.id >> 24 == 0 {
            return None;
        }
        self.lvars
            .get(
                usize::try_from(script.local_base)
                    .ok()?
                    .checked_add(index)?,
            )
            .copied()
    }
}
impl MapObjects {
    /// An object section on its own. Safe-rule substitutions are not
    /// reported here; `MapFile::parse` collects them for a whole map.
    pub fn parse(
        bytes: &[u8],
        prototypes: &impl PrototypeLookup,
        saved: bool,
    ) -> Result<Self, ParseError> {
        Self::read(
            &mut Reader::new(bytes, "MAP objects")?,
            prototypes,
            saved,
            &mut Vec::new(),
        )
    }
    fn read(
        r: &mut Reader<'_>,
        prototypes: &impl PrototypeLookup,
        saved: bool,
        diagnostics: &mut Vec<String>,
    ) -> Result<Self, ParseError> {
        let start = r.pos;
        let total = r.count("total objects", MAX_OBJECTS)?;
        let mut elevations = std::array::from_fn(|_| Vec::new());
        let mut roots = 0;
        let mut budget = MAX_OBJECTS;
        for (elevation, objects) in elevations.iter_mut().enumerate() {
            let count = r.count("elevation objects", MAX_OBJECTS)?;
            roots += count;
            if roots > MAX_OBJECTS {
                return Err(r.error("total object limit exceeded"));
            }
            for _ in 0..count {
                objects.push(MapObject::read(
                    r,
                    prototypes,
                    saved,
                    elevation as u8,
                    None,
                    0,
                    &mut budget,
                    diagnostics,
                )?);
            }
        }
        // SPEC-003 §3.3 [May-improve]: the reference preloads by the total
        // and overruns when it is smaller than the elevation sum; a total
        // that differs from that sum is rejected.
        if total != roots {
            return Err(r.error("top-level object total disagrees with elevation counts"));
        }
        Ok(Self {
            total,
            elevations,
            consumed_len: r.pos - start,
        })
    }
}
impl MapObject {
    /// SPEC-014 §3.44 whether the object is outlined now: its outline word
    /// has a type and is not disabled. (SPEC-010 §3.11 widens an outlined
    /// object's on-screen rectangle by 1 pixel on every side.)
    pub fn outlined(&self) -> bool {
        outline::outlined(self.outline)
    }
    /// One object tree outside a map stream (the save's player record):
    /// the same record/containment grammar with no count prefixes.
    pub(crate) fn read_tree(
        r: &mut Reader<'_>,
        prototypes: &impl PrototypeLookup,
        elevation: u8,
    ) -> Result<Self, ParseError> {
        let mut budget = MAX_OBJECTS;
        // The save's player record has no diagnostics sink; substitutions
        // in it are applied silently.
        Self::read(
            r,
            prototypes,
            true,
            elevation,
            None,
            0,
            &mut budget,
            &mut Vec::new(),
        )
    }
    pub(crate) fn write_tree(&self, out: &mut Vec<u8>) -> Result<(), ParseError> {
        self.write(out, self.elevation, 0)
    }
    #[allow(clippy::too_many_arguments)]
    fn read(
        r: &mut Reader<'_>,
        prototypes: &impl PrototypeLookup,
        saved: bool,
        elevation: u8,
        owner: Option<u32>,
        depth: usize,
        budget: &mut usize,
        diagnostics: &mut Vec<String>,
    ) -> Result<Self, ParseError> {
        if depth > MAX_INVENTORY_DEPTH || *budget == 0 {
            return Err(r.error("object depth/count resource limit exceeded"));
        }
        *budget -= 1;
        let file_offset = r.pos;
        let p: [i32; 18] = r.words()?;
        let hex = if p[1] == -1 {
            None
        } else if (0..40000).contains(&p[1]) {
            Some(p[1])
        } else {
            return Err(r.error("object hex outside grid"));
        };
        // SPEC-003 §2.10 lists the frame as zero-based and the orientation
        // as 0-5 without a validation rule; as for the §3.2 header
        // orientation, a value outside its range reads as 0 with a
        // diagnostic instead of failing the map.
        let frame = usize::try_from(p[6]).unwrap_or_else(|_| {
            diagnostics.push(format!(
                "object {:#x} at byte {file_offset:#x}: frame {} is negative; using 0",
                p[0], p[6]
            ));
            0
        });
        let orientation = u8::try_from(p[7])
            .ok()
            .filter(|orientation| *orientation <= 5)
            .unwrap_or_else(|| {
                diagnostics.push(format!(
                    "object {:#x} at byte {file_offset:#x}: orientation {} is outside 0-5; using 0",
                    p[0], p[7]
                ));
                0
            });
        let pid = p[11] as u32;
        let category = Category::from_pid(pid).map_err(|error| r.error(error.message))?;
        if pid & 0xffffff == 0 && pid != 0x01000000 {
            return Err(r.error("zero file-backed object prototype index"));
        }
        // SPEC-003 §2.11 [May-improve]: a negative inventory count or a
        // capacity below the count is rejected before allocation.
        let count = r.count("inventory entries", MAX_OBJECTS)?;
        let inventory_capacity = r.count("inventory capacity", MAX_ENTRIES)?;
        if inventory_capacity < count {
            return Err(r.error("inventory capacity smaller than entry count"));
        }
        r.take(4)?;
        let state = if category == Category::Critter {
            let s: [i32; 11] = r.words()?;
            ObjectState::Critter(CritterState {
                reaction: s[0],
                damage: s[1],
                maneuvers: s[2] as u32,
                ap: s[3],
                results: CombatFlags(s[4] as u32),
                ai: s[5],
                team: s[6],
                attacker: s[7],
                hp: s[8],
                radiation: s[9],
                poison: s[10],
            })
        } else {
            let word = r.word()?;
            let word = if word == 0xcccccccc { 0 } else { word };
            let detail = match category {
                Category::Item | Category::Scenery => {
                    let info = prototypes
                        .lookup(pid)
                        .map_err(|error| r.error(format!("PID {pid:#010x}: {error}")))?;
                    match (category, info.kind) {
                        (Category::Item, PrototypeKind::Item(subtype)) => {
                            match subtype {
                                ItemSubtype::Weapon => {
                                    let mut rounds = r.be_i32()?;
                                    let mut ammo_pid = r.be_i32()?;
                                    if !saved {
                                        let (default_pid,capacity)=info.weapon_defaults.ok_or_else(||r.error("weapon defaults missing from prototype provider"))?;
                                        if ammo_pid == -1 || ammo_pid as u32 == 0xcccccccc {
                                            ammo_pid = default_pid;
                                        }
                                        rounds = capacity;
                                    }
                                    ObjectDetail::Weapon { rounds, ammo_pid }
                                }
                                ItemSubtype::Ammunition => ObjectDetail::Ammunition(r.be_i32()?),
                                ItemSubtype::Misc => ObjectDetail::Charges(r.be_i32()?),
                                ItemSubtype::Key => ObjectDetail::Key(r.be_i32()?),
                                _ => ObjectDetail::Plain,
                            }
                        }
                        (Category::Scenery, PrototypeKind::Scenery(subtype)) => match subtype {
                            ScenerySubtype::Door => ObjectDetail::Door(LockFlags(r.word()?)),
                            ScenerySubtype::Stairs => ObjectDetail::Stairs {
                                map: r.be_i32()?,
                                destination: PackedLocation(r.word()?),
                            },
                            ScenerySubtype::Elevator => ObjectDetail::Elevator {
                                kind: r.be_i32()?,
                                level: r.be_i32()?,
                            },
                            ScenerySubtype::LadderUp | ScenerySubtype::LadderDown => {
                                ObjectDetail::Ladder(PackedLocation(r.word()?))
                            }
                            ScenerySubtype::Generic => ObjectDetail::Plain,
                        },
                        _ => {
                            return Err(
                                r.error("prototype subtype category does not match object PID")
                            );
                        }
                    }
                }
                Category::Misc if (0x05000010..=0x05000017).contains(&pid) => {
                    ObjectDetail::ExitGrid {
                        map: r.be_i32()?,
                        hex: r.be_i32()?,
                        elevation: r.be_i32()?,
                        orientation: r.be_i32()?,
                    }
                }
                _ => ObjectDetail::Plain,
            };
            ObjectState::Other { word, detail }
        };
        let mut inventory = Vec::new();
        for _ in 0..count {
            // SPEC-003 §2.12 gives the quantity no range rule; a negative
            // one cannot be represented and is rejected, while a zero
            // stack is kept and reported.
            let quantity = r.count("inventory quantity", i32::MAX as usize)?;
            if quantity == 0 {
                diagnostics.push(format!(
                    "object {:#x} at byte {file_offset:#x}: inventory entry with quantity 0",
                    p[0]
                ));
            }
            inventory.push(InventoryEntry {
                quantity: quantity as u32,
                object: Self::read(
                    r,
                    prototypes,
                    saved,
                    elevation,
                    Some(p[0] as u32),
                    depth + 1,
                    budget,
                    diagnostics,
                )?,
            });
        }
        // SPEC-003 §3.3: on every read (base and saved maps alike), an exit
        // grid to map 0 or below whose art index is below 33 is rebuilt as
        // miscellaneous art with index + 16, the same animation field and
        // every other field zero. The literal rule is kept (the
        // `[May-improve]` base-map-only shift is not taken), so an index
        // from 1 to 16 moves again on the next saved-map read.
        let mut art_id = p[8] as u32;
        if let ObjectState::Other {
            detail: ObjectDetail::ExitGrid { map, .. },
            ..
        } = &state
            && *map <= 0
            && art_id & 0xfff < 33
        {
            art_id = 0x0500_0000 | (art_id & 0x00ff_0000) | ((art_id & 0xfff) + 16);
        }
        let flags = BasicFlags(if owner.is_none() && category == Category::Critter {
            p[9] as u32 & !0x400
        } else {
            p[9] as u32
        });
        Ok(Self {
            id: p[0] as u32,
            hex,
            displacement: [p[2], p[3]],
            screen: [p[4], p[5]],
            frame,
            orientation,
            art_id,
            flags,
            stored_elevation: p[10],
            elevation,
            pid,
            combat_id: p[12],
            light: Light {
                radius: p[13],
                intensity: p[14],
            },
            outline: 0,
            script_id: p[16],
            cached_script_file: p[17],
            inventory_capacity,
            state,
            inventory,
            owner,
            file_offset,
        })
    }
    fn restore_scripts(&mut self, bindings: &BTreeMap<u32, i32>, top: bool) {
        if let Some(index) = bindings.get(&(self.script_id as u32)) {
            if top {
                self.cached_script_file = *index;
            }
        } else {
            self.script_id = -1;
        }
        for entry in &mut self.inventory {
            entry.object.restore_scripts(bindings, false);
        }
    }
}

/// SPEC-003 writer. Writing sets darkness to 1, recomputes the elevation
/// omission bits, saves top-level objects by traversing grid positions in
/// each elevation (unplaced and temporary objects are not saved), and omits
/// script instances flagged `0x8`, compacting the retained records.
impl MapFile {
    pub fn write(&self) -> Result<Vec<u8>, ParseError> {
        let error = |message: &str| ParseError::new("MAP", 0, message);
        let count = |length: usize| i32::try_from(length).map_err(|_| error("count exceeds i32"));
        let mut objects: [Vec<&MapObject>; 3] = std::array::from_fn(|_| Vec::new());
        for (elevation, saved) in objects.iter_mut().enumerate() {
            *saved = self.objects.elevations[elevation]
                .iter()
                .filter(|object| {
                    object.hex.is_some() && !object.flags.contains(BasicFlags::TEMPORARY)
                })
                .collect();
            // Stable within a hex, so the existing per-hex order is retained.
            saved.sort_by_key(|object| object.hex);
        }
        let mut flags = self.header.flags.0 & !0xe;
        for (elevation, saved) in objects.iter().enumerate() {
            let blank = self.tiles[elevation].as_ref().is_none_or(|tiles| {
                tiles
                    .iter()
                    .all(|tile| tile.floor() == 1 && tile.roof() == 1)
            });
            if blank && saved.is_empty() {
                flags |= 2 << elevation;
            }
        }
        let header = &self.header;
        let out = &mut Vec::new();
        put_i32(out, self.header.version);
        out.extend_from_slice(&header.name);
        put_i32(out, header.entry_hex);
        put_i32(out, header.entry_elevation.into());
        put_i32(out, header.entry_orientation.into());
        put_i32(out, count(self.lvars.len())?);
        put_i32(out, header.script_reference);
        put_u32(out, flags);
        put_i32(out, 1);
        put_i32(out, count(self.mvars.len())?);
        put_i32(out, header.map_number);
        put_u32(out, header.last_visit);
        for word in header.reserved {
            put_i32(out, word);
        }
        for value in self.mvars.iter().chain(&self.lvars) {
            put_i32(out, *value);
        }
        for elevation in 0..3 {
            if flags & (2 << elevation) == 0 {
                for index in 0..10000 {
                    let tile = self.tile(elevation, index).expect("tile index in range");
                    put_u32(out, tile.word);
                }
            }
        }
        for category in &self.scripts {
            let retained: Vec<&MapScript> = category
                .active()
                .filter(|script| !script.flags.contains(ScriptFlags::NO_SAVE))
                .collect();
            put_i32(out, count(retained.len())?);
            for block in retained.chunks(16) {
                for slot in 0..16 {
                    match block.get(slot) {
                        Some(script) => script.write(out),
                        None => out.extend_from_slice(&[0; 64]),
                    }
                }
                put_i32(out, count(block.len())?);
                put_i32(out, 0); // obsolete next-block address
            }
        }
        put_i32(out, count(objects.iter().map(Vec::len).sum())?);
        for (elevation, saved) in objects.iter().enumerate() {
            put_i32(out, count(saved.len())?);
            for object in saved {
                object.write(out, elevation as u8, 0)?;
            }
        }
        Ok(std::mem::take(out))
    }
}
impl MapScript {
    fn write(&self, out: &mut Vec<u8>) {
        put_u32(out, self.id);
        put_i32(out, self.link);
        // The slot's own identifier selects its conditional prefix.
        match (self.id >> 24, &self.prefix) {
            (1, ScriptPrefix::Spatial { location, radius }) => {
                put_u32(out, location.0);
                put_i32(out, *radius);
            }
            (1, _) => out.extend_from_slice(&[0; 8]),
            (2, ScriptPrefix::Timed(time)) => put_i32(out, *time),
            (2, _) => out.extend_from_slice(&[0; 4]),
            _ => {}
        }
        for value in [
            self.flags.0 as i32,
            self.file_index,
            0, // obsolete program address
            self.object_id,
            self.local_base,
            self.local_count,
            self.result,
            self.event,
            self.parameter,
            self.action,
            self.override_default,
            self.execution_state,
            self.quantity,
            self.annotations as i32,
        ] {
            put_i32(out, value);
        }
    }
}
impl MapObject {
    fn write(&self, out: &mut Vec<u8>, elevation: u8, depth: usize) -> Result<(), ParseError> {
        let error = |message: &str| ParseError::new("MAP", 0, message);
        if depth > MAX_INVENTORY_DEPTH {
            return Err(error("inventory depth exceeds the resource limit"));
        }
        if self.hex.is_some_and(|hex| !(0..40000).contains(&hex)) || self.orientation > 5 {
            return Err(error(
                "object hex/orientation outside the serialized domain",
            ));
        }
        let frame = i32::try_from(self.frame).map_err(|_| error("object frame exceeds i32"))?;
        // SPEC-003 §3.3 safe rule: a temporary inventory child is skipped
        // entirely (no quantity, no record) and only written entries count.
        let kept: Vec<_> = self
            .inventory
            .iter()
            .filter(|entry| !entry.object.flags.contains(BasicFlags::TEMPORARY))
            .collect();
        let entries =
            i32::try_from(kept.len()).map_err(|_| error("inventory count exceeds i32"))?;
        let capacity = i32::try_from(self.inventory_capacity.max(kept.len()))
            .map_err(|_| error("inventory capacity exceeds i32"))?;
        for value in [
            self.id as i32,
            self.hex.unwrap_or(-1),
            self.displacement[0],
            self.displacement[1],
            self.screen[0],
            self.screen[1],
            frame,
            self.orientation.into(),
            self.art_id as i32,
            self.flags.0 as i32,
            elevation.into(),
            self.pid as i32,
            self.combat_id,
            self.light.radius,
            self.light.intensity,
            self.outline as i32,
            self.script_id,
            self.cached_script_file,
            entries,
            capacity,
            0, // obsolete inventory address
        ] {
            put_i32(out, value);
        }
        match &self.state {
            ObjectState::Critter(state) => {
                for value in [
                    state.reaction,
                    state.damage,
                    state.maneuvers as i32,
                    state.ap,
                    state.results.0 as i32,
                    state.ai,
                    state.team,
                    state.attacker,
                    state.hp,
                    state.radiation,
                    state.poison,
                ] {
                    put_i32(out, value);
                }
            }
            ObjectState::Other { word, detail } => {
                put_u32(out, *word);
                let words: &[i32] = match *detail {
                    ObjectDetail::Plain => &[],
                    ObjectDetail::Weapon { rounds, ammo_pid } => &[rounds, ammo_pid],
                    ObjectDetail::Ammunition(value)
                    | ObjectDetail::Charges(value)
                    | ObjectDetail::Key(value) => &[value],
                    ObjectDetail::Door(flags) => &[flags.0 as i32],
                    ObjectDetail::Stairs { map, destination } => &[map, destination.0 as i32],
                    ObjectDetail::Elevator { kind, level } => &[kind, level],
                    ObjectDetail::Ladder(destination) => &[destination.0 as i32],
                    ObjectDetail::ExitGrid {
                        map,
                        hex,
                        elevation,
                        orientation,
                    } => &[map, hex, elevation, orientation],
                };
                for value in words {
                    put_i32(out, *value);
                }
            }
        }
        for entry in kept {
            let quantity = i32::try_from(entry.quantity)
                .ok()
                .filter(|quantity| *quantity > 0)
                .ok_or_else(|| error("inventory quantity must be 1..=i32::MAX"))?;
            put_i32(out, quantity);
            entry.object.write(out, elevation, depth + 1)?;
        }
        Ok(())
    }
}
