//! SPEC-003 version-19 map streams, with bounded recursive inventories.
use crate::{
    ParseError,
    binary::{MAX_ENTRIES, Reader},
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
    pub outline: u32,
    pub script_id: i32,
    pub cached_script_file: i32,
    pub inventory_capacity: usize,
    pub state: ObjectState,
    pub inventory: Vec<InventoryEntry>,
    pub owner: Option<u32>,
    pub file_offset: usize,
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
) -> Result<ScriptCategory, ParseError> {
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
        let active = r.count("active block slots", 16)?;
        r.take(4)?; // obsolete next-block address
        occupied += active;
        for slot in slots.iter().take(active) {
            if slot.id >> 24 != category as u32 {
                return Err(r.error("active script category differs from its stream"));
            }
            if slot.file_index < 0 || slot.local_base < -1 || slot.stored_local_count < 0 {
                return Err(r.error("invalid active script index/local slice"));
            }
            if saved
                && slot.local_base >= 0
                && slot.local_base as u64 + slot.local_count as u64 > locals as u64
            {
                return Err(r.error("active script local slice extends beyond LVAR pool"));
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
    pub fn parse(bytes: &[u8], prototypes: &impl PrototypeLookup) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, "MAP")?;
        if r.be_i32()? != 19 {
            return Err(r.error("only map version 19 is supported"));
        }
        let name = r.take(16)?.try_into().expect("fixed name");
        let entry_hex = r.be_i32()?;
        if !(0..40000).contains(&entry_hex) {
            return Err(r.error("entry hex outside grid"));
        }
        let entry_elevation = r.count("entry elevation", 2)? as u8;
        let entry_orientation = r.count("entry orientation", 5)? as u8;
        let stored_local_count = r.be_i32()?;
        let script_reference = r.be_i32()?;
        let flags = MapFlags(r.word()?);
        let darkness = r.be_i32()?;
        let stored_global_count = r.be_i32()?;
        let map_number = r.be_i32()?;
        let last_visit = r.word()?;
        let reserved = r.words()?;
        let header = MapHeader {
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
            )?);
        }
        let scripts: [ScriptCategory; 5] = scripts.try_into().expect("five categories");
        let mut bindings = BTreeMap::new();
        for record in scripts.iter().flat_map(ScriptCategory::active) {
            if bindings.insert(record.id, record.file_index).is_some() {
                return Err(r.error("duplicate active script ID"));
            }
        }
        let mut objects = MapObjects::read(&mut r, prototypes, flags.contains(MapFlags::SAVED))?;
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
    pub fn parse(
        bytes: &[u8],
        prototypes: &impl PrototypeLookup,
        saved: bool,
    ) -> Result<Self, ParseError> {
        Self::read(&mut Reader::new(bytes, "MAP objects")?, prototypes, saved)
    }
    fn read(
        r: &mut Reader<'_>,
        prototypes: &impl PrototypeLookup,
        saved: bool,
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
                )?);
            }
        }
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
    fn read(
        r: &mut Reader<'_>,
        prototypes: &impl PrototypeLookup,
        saved: bool,
        elevation: u8,
        owner: Option<u32>,
        depth: usize,
        budget: &mut usize,
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
        if p[6] < 0 || !(0..6).contains(&p[7]) {
            return Err(r.error("invalid object frame/orientation"));
        }
        let pid = p[11] as u32;
        let category = Category::from_pid(pid).map_err(|error| r.error(error.message))?;
        if pid & 0xffffff == 0 && pid != 0x01000000 {
            return Err(r.error("zero file-backed object prototype index"));
        }
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
            let quantity = r.count("inventory quantity", i32::MAX as usize)?;
            if quantity == 0 {
                return Err(r.error("inventory quantity must be positive"));
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
                )?,
            });
        }
        let mut art_id = p[8] as u32;
        if let ObjectState::Other {
            detail: ObjectDetail::ExitGrid { map, .. },
            ..
        } = &state
            && *map <= 0
            && art_id & 0xfff < 33
        {
            art_id = (art_id & !0xfff) | ((art_id & 0xfff) + 16);
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
            frame: p[6] as usize,
            orientation: p[7] as u8,
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
