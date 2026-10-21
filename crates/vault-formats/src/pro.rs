//! SPEC-003 prototype templates. Serialized addresses are never dereferenced.
use crate::{
    ParseError,
    binary::{Reader, put_i32, put_u32},
};

macro_rules! flags {
    ($name:ident { $($constant:ident = $value:expr),* $(,)? }) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        pub struct $name(pub u32);
        impl $name {
            $(pub const $constant: Self = Self($value);)*
            pub const fn contains(self, flag: Self) -> bool { self.0 & flag.0 == flag.0 }
        }
    };
}

flags!(BasicFlags {
    HIDDEN=0x1, TEMPORARY=0x4, FLAT=0x8, NONBLOCKING=0x10, LIGHTING=0x20,
    SPECIAL=0x400, MULTIHEX=0x800, NO_HIGHLIGHT=0x1000, USED=0x2000,
    RED=0x4000, NO_TRANSLUCENCY=0x8000, WALL_TRANSLUCENCY=0x10000,
    GLASS=0x20000, STEAM=0x40000, ENERGY=0x80000, LEFT_HAND=0x1000000,
    RIGHT_HAND=0x2000000, WORN=0x4000000, WALL_END=0x10000000,
    LIGHT_THROUGH=0x20000000, SEEN=0x40000000, PROJECTILE_THROUGH=0x80000000
});
flags!(ExtendedFlags { BIG_GUN=0x100, TWO_HANDED=0x200, USE=0x800,
    USE_ON=0x1000, TALK=0x4000, PICKUP_CONTAINER=0x8000 });
flags!(CritterFlags { BARTER=0x2, NO_STEAL=0x20, NO_DROP=0x40, NO_LIMBS=0x80,
    NO_AGE=0x100, NO_HEAL=0x200, INVULNERABLE=0x400, FLAT=0x800,
    SPECIAL_DEATH=0x1000, LONG_LIMBS=0x2000, NO_KNOCKBACK=0x4000 });
flags!(LockFlags { OPEN=0x1, LOCKED=0x2000000, JAMMED=0x4000000 });
flags!(ContainerFlags { GROUND_REACH=0x1 });
flags!(MapFlags { SAVED=0x1, OMIT_ELEVATION_0=0x2, OMIT_ELEVATION_1=0x4, OMIT_ELEVATION_2=0x8 });
flags!(ScriptFlags { LOADED=0x1, NO_SAVE=0x8 });
flags!(CombatFlags {});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Translucency {
    Opaque,
    Wall,
    Glass,
    Steam,
    Energy,
    Red,
}
impl BasicFlags {
    pub fn translucency(self) -> Option<Translucency> {
        [
            (Self::NO_TRANSLUCENCY, Translucency::Opaque),
            (Self::WALL_TRANSLUCENCY, Translucency::Wall),
            (Self::GLASS, Translucency::Glass),
            (Self::STEAM, Translucency::Steam),
            (Self::ENERGY, Translucency::Energy),
            (Self::RED, Translucency::Red),
        ]
        .into_iter()
        .find(|(mask, _)| self.contains(*mask))
        .map(|(_, kind)| kind)
    }
}
impl ExtendedFlags {
    fn raw_attack_modes(self) -> [u8; 2] {
        [(self.0 & 15) as u8, ((self.0 >> 4) & 15) as u8]
    }
    /// Primary and secondary attack modes 0..=8. Under the SPEC-003 §2.6
    /// safe rule a nibble of 9-15 reads as mode 0 (none); unknown bits
    /// remain preserved in the word.
    pub fn attack_modes(self) -> [u8; 2] {
        self.raw_attack_modes()
            .map(|mode| if mode <= 8 { mode } else { 0 })
    }
    /// Whether either nibble holds an invalid mode 9-15, for diagnostics.
    pub fn invalid_attack_mode(self) -> bool {
        self.raw_attack_modes().iter().any(|mode| *mode > 8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Category {
    Item,
    Critter,
    Scenery,
    Wall,
    Tile,
    Misc,
}
impl Category {
    /// SPEC-003 §2.1: categories 0-5 are fixed. The whole most-significant
    /// byte is dispatched on, so a PID with bits 28-31 set (other than the
    /// `0xFFFFFFFF` sentinel, handled by callers) or a category 6-15, which
    /// has no index path, is rejected with a diagnostic (`[May-improve]`)
    /// rather than read as a negative or out-of-table category.
    pub fn from_pid(pid: u32) -> Result<Self, ParseError> {
        match pid >> 24 {
            0 => Ok(Self::Item),
            1 => Ok(Self::Critter),
            2 => Ok(Self::Scenery),
            3 => Ok(Self::Wall),
            4 => Ok(Self::Tile),
            5 => Ok(Self::Misc),
            _ => Err(ParseError::new(
                "PRO",
                0,
                format!("unknown PID category in {pid:#010x}"),
            )),
        }
    }
    pub fn directory(self) -> &'static str {
        match self {
            Self::Item => "items",
            Self::Critter => "critters",
            Self::Scenery => "scenery",
            Self::Wall => "walls",
            Self::Tile => "tiles",
            Self::Misc => "misc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ItemSubtype {
    Armor,
    Container,
    Drug,
    Weapon,
    Ammunition,
    Misc,
    Key,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ScenerySubtype {
    Door,
    Stairs,
    Elevator,
    LadderUp,
    LadderDown,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Light {
    pub radius: i32,
    pub intensity: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Armor {
    pub armor_class: i32,
    pub resistances: [i32; 7],
    pub thresholds: [i32; 7],
    pub perk: i32,
    pub male_art: i32,
    pub female_art: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drug {
    pub stats: [i32; 3],
    pub immediate: [i32; 3],
    pub delay1: i32,
    pub changes1: [i32; 3],
    pub delay2: i32,
    pub changes2: [i32; 3],
    pub addiction: i32,
    pub withdrawal: i32,
    pub onset: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weapon {
    pub animation: i32,
    pub damage: [i32; 2],
    pub damage_type: i32,
    pub ranges: [i32; 2],
    pub projectile_pid: i32,
    pub strength: i32,
    pub ap_cost: [i32; 2],
    pub critical_failure: i32,
    pub perk: i32,
    pub burst: i32,
    pub caliber: i32,
    pub ammo_pid: i32,
    pub capacity: i32,
    pub sound: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemData {
    Armor(Armor),
    Container {
        capacity: i32,
        flags: ContainerFlags,
    },
    Drug(Drug),
    Weapon(Weapon),
    Ammunition {
        caliber: i32,
        quantity: i32,
        armor_modifier: i32,
        resistance_modifier: i32,
        multiplier: i32,
        divisor: i32,
    },
    Misc {
        power_pid: i32,
        power_type: i32,
        charges: i32,
    },
    Key {
        code: i32,
    },
}
impl ItemData {
    pub fn subtype(&self) -> ItemSubtype {
        match self {
            Self::Armor(_) => ItemSubtype::Armor,
            Self::Container { .. } => ItemSubtype::Container,
            Self::Drug(_) => ItemSubtype::Drug,
            Self::Weapon(_) => ItemSubtype::Weapon,
            Self::Ammunition { .. } => ItemSubtype::Ammunition,
            Self::Misc { .. } => ItemSubtype::Misc,
            Self::Key { .. } => ItemSubtype::Key,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub material: i32,
    pub size: i32,
    pub weight: i32,
    pub cost: i32,
    pub inventory_art: u32,
    pub stored_byte: u8,
    pub data: ItemData,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Critter {
    pub head_art: u32,
    pub ai: i32,
    pub team: i32,
    pub flags: CritterFlags,
    pub base_stats: [i32; 35],
    pub bonus_stats: [i32; 35],
    pub skills: [i32; 18],
    pub body: i32,
    pub experience: i32,
    pub kill_category: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneryData {
    Door { state: LockFlags, key: i32 },
    Stairs { map: i32, destination: u32 },
    Elevator { kind: i32, level: i32 },
    LadderUp { destination: u32 },
    LadderDown { destination: u32 },
    Generic(i32),
}
impl SceneryData {
    pub fn subtype(&self) -> ScenerySubtype {
        match self {
            Self::Door { .. } => ScenerySubtype::Door,
            Self::Stairs { .. } => ScenerySubtype::Stairs,
            Self::Elevator { .. } => ScenerySubtype::Elevator,
            Self::LadderUp { .. } => ScenerySubtype::LadderUp,
            Self::LadderDown { .. } => ScenerySubtype::LadderDown,
            Self::Generic(_) => ScenerySubtype::Generic,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenery {
    pub material: i32,
    pub stored_byte: u8,
    pub data: SceneryData,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrototypeData {
    Item(Item),
    Critter(Box<Critter>),
    Scenery(Scenery),
    Wall { material: i32 },
    Tile { material: i32 },
    Misc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prototype {
    pub pid: u32,
    pub category: Category,
    pub message_base: i32,
    pub art_id: u32,
    pub light: Option<Light>,
    pub flags: BasicFlags,
    pub extended: ExtendedFlags,
    pub script: Option<i32>,
    pub data: PrototypeData,
    pub consumed_len: usize,
}

impl Prototype {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let mut r = Reader::new(bytes, "PRO")?;
        let pid = r.word()?;
        let category = Category::from_pid(pid)?;
        if pid & 0xffffff == 0 && pid != 0x01000000 {
            return Err(r.error("zero file-backed prototype index"));
        }
        let message_base = r.be_i32()?;
        let art_id = r.word()?;
        let light = if category == Category::Tile {
            None
        } else {
            Some(Light {
                radius: r.be_i32()?,
                intensity: r.be_i32()?,
            })
        };
        let flags = BasicFlags(r.word()?);
        let extended = ExtendedFlags(r.word()?);
        let script = if category == Category::Misc {
            None
        } else {
            Some(r.be_i32()?)
        };
        let data = match category {
            Category::Item => {
                let subtype = r.be_i32()?;
                let material = r.be_i32()?;
                let size = r.be_i32()?;
                let weight = r.be_i32()?;
                let cost = r.be_i32()?;
                let inventory_art = r.word()?;
                let stored_byte = r.take(1)?[0];
                let data = match subtype {
                    0 => ItemData::Armor(Armor {
                        armor_class: r.be_i32()?,
                        resistances: r.words()?,
                        thresholds: r.words()?,
                        perk: r.be_i32()?,
                        male_art: r.be_i32()?,
                        female_art: r.be_i32()?,
                    }),
                    1 => ItemData::Container {
                        capacity: r.be_i32()?,
                        flags: ContainerFlags(r.word()?),
                    },
                    2 => ItemData::Drug(Drug {
                        stats: r.words()?,
                        immediate: r.words()?,
                        delay1: r.be_i32()?,
                        changes1: r.words()?,
                        delay2: r.be_i32()?,
                        changes2: r.words()?,
                        addiction: r.be_i32()?,
                        withdrawal: r.be_i32()?,
                        onset: r.be_i32()?,
                    }),
                    3 => ItemData::Weapon(Weapon {
                        animation: r.be_i32()?,
                        damage: r.words()?,
                        damage_type: r.be_i32()?,
                        ranges: r.words()?,
                        projectile_pid: r.be_i32()?,
                        strength: r.be_i32()?,
                        ap_cost: r.words()?,
                        critical_failure: r.be_i32()?,
                        perk: r.be_i32()?,
                        burst: r.be_i32()?,
                        caliber: r.be_i32()?,
                        ammo_pid: r.be_i32()?,
                        capacity: r.be_i32()?,
                        sound: r.take(1)?[0],
                    }),
                    4 => ItemData::Ammunition {
                        caliber: r.be_i32()?,
                        quantity: r.be_i32()?,
                        armor_modifier: r.be_i32()?,
                        resistance_modifier: r.be_i32()?,
                        multiplier: r.be_i32()?,
                        divisor: r.be_i32()?,
                    },
                    5 => ItemData::Misc {
                        power_pid: r.be_i32()?,
                        power_type: r.be_i32()?,
                        charges: r.be_i32()?,
                    },
                    6 => ItemData::Key { code: r.be_i32()? },
                    _ => return Err(r.error(format!("unknown item subtype {subtype}"))),
                };
                PrototypeData::Item(Item {
                    material,
                    size,
                    weight,
                    cost,
                    inventory_art,
                    stored_byte,
                    data,
                })
            }
            Category::Critter => PrototypeData::Critter(Box::new(Critter {
                head_art: r.word()?,
                ai: r.be_i32()?,
                team: r.be_i32()?,
                flags: CritterFlags(r.word()?),
                base_stats: r.words()?,
                bonus_stats: r.words()?,
                skills: r.words()?,
                body: r.be_i32()?,
                experience: r.be_i32()?,
                kill_category: r.be_i32()?,
            })),
            Category::Scenery => {
                let subtype = r.be_i32()?;
                let material = r.be_i32()?;
                let stored_byte = r.take(1)?[0];
                let data = match subtype {
                    0 => SceneryData::Door {
                        state: LockFlags(r.word()?),
                        key: r.be_i32()?,
                    },
                    1 => SceneryData::Stairs {
                        map: r.be_i32()?,
                        destination: r.word()?,
                    },
                    2 => SceneryData::Elevator {
                        kind: r.be_i32()?,
                        level: r.be_i32()?,
                    },
                    3 => SceneryData::LadderUp {
                        destination: r.word()?,
                    },
                    4 => SceneryData::LadderDown {
                        destination: r.word()?,
                    },
                    5 => SceneryData::Generic(r.be_i32()?),
                    _ => return Err(r.error(format!("unknown scenery subtype {subtype}"))),
                };
                PrototypeData::Scenery(Scenery {
                    material,
                    stored_byte,
                    data,
                })
            }
            Category::Wall => PrototypeData::Wall {
                material: r.be_i32()?,
            },
            Category::Tile => PrototypeData::Tile {
                material: r.be_i32()?,
            },
            Category::Misc => PrototypeData::Misc,
        };
        Ok(Self {
            pid,
            category,
            message_base,
            art_id,
            light,
            flags,
            extended,
            script,
            data,
            consumed_len: r.pos,
        })
    }
}

impl Prototype {
    /// SPEC-003 §3.1 writer in reader order, under the safe rules: weapons
    /// write minimum damage before maximum (the reference exchanges them), and
    /// scenery omits the reference's six-word suffix, which readers tolerate.
    pub fn write(&self) -> Vec<u8> {
        let out = &mut Vec::new();
        put_u32(out, self.pid);
        put_i32(out, self.message_base);
        put_u32(out, self.art_id);
        let light = self.light.unwrap_or(Light {
            radius: 0,
            intensity: 0,
        });
        if self.category != Category::Tile {
            put_i32(out, light.radius);
            put_i32(out, light.intensity);
        }
        put_u32(out, self.flags.0);
        put_u32(out, self.extended.0);
        let script = self.script.unwrap_or(-1);
        if self.category != Category::Misc {
            put_i32(out, script);
        }
        let words = |out: &mut Vec<u8>, values: &[i32]| {
            for value in values {
                put_i32(out, *value);
            }
        };
        match &self.data {
            PrototypeData::Item(item) => {
                put_i32(out, item.data.subtype() as i32);
                words(out, &[item.material, item.size, item.weight, item.cost]);
                put_u32(out, item.inventory_art);
                out.push(item.stored_byte);
                match &item.data {
                    ItemData::Armor(armor) => {
                        put_i32(out, armor.armor_class);
                        words(out, &armor.resistances);
                        words(out, &armor.thresholds);
                        words(out, &[armor.perk, armor.male_art, armor.female_art]);
                    }
                    ItemData::Container { capacity, flags } => {
                        put_i32(out, *capacity);
                        put_u32(out, flags.0);
                    }
                    ItemData::Drug(drug) => {
                        words(out, &drug.stats);
                        words(out, &drug.immediate);
                        put_i32(out, drug.delay1);
                        words(out, &drug.changes1);
                        put_i32(out, drug.delay2);
                        words(out, &drug.changes2);
                        words(out, &[drug.addiction, drug.withdrawal, drug.onset]);
                    }
                    ItemData::Weapon(weapon) => {
                        words(
                            out,
                            &[
                                weapon.animation,
                                weapon.damage[0],
                                weapon.damage[1],
                                weapon.damage_type,
                                weapon.ranges[0],
                                weapon.ranges[1],
                                weapon.projectile_pid,
                                weapon.strength,
                                weapon.ap_cost[0],
                                weapon.ap_cost[1],
                                weapon.critical_failure,
                                weapon.perk,
                                weapon.burst,
                                weapon.caliber,
                                weapon.ammo_pid,
                                weapon.capacity,
                            ],
                        );
                        out.push(weapon.sound);
                    }
                    ItemData::Ammunition {
                        caliber,
                        quantity,
                        armor_modifier,
                        resistance_modifier,
                        multiplier,
                        divisor,
                    } => words(
                        out,
                        &[
                            *caliber,
                            *quantity,
                            *armor_modifier,
                            *resistance_modifier,
                            *multiplier,
                            *divisor,
                        ],
                    ),
                    ItemData::Misc {
                        power_pid,
                        power_type,
                        charges,
                    } => words(out, &[*power_pid, *power_type, *charges]),
                    ItemData::Key { code } => put_i32(out, *code),
                }
            }
            PrototypeData::Critter(critter) => {
                put_u32(out, critter.head_art);
                put_i32(out, critter.ai);
                put_i32(out, critter.team);
                put_u32(out, critter.flags.0);
                words(out, &critter.base_stats);
                words(out, &critter.bonus_stats);
                words(out, &critter.skills);
                words(
                    out,
                    &[critter.body, critter.experience, critter.kill_category],
                );
            }
            PrototypeData::Scenery(scenery) => {
                let subtype = scenery.data.subtype() as i32;
                put_i32(out, subtype);
                put_i32(out, scenery.material);
                out.push(scenery.stored_byte);
                match scenery.data {
                    SceneryData::Door { state, key } => {
                        put_u32(out, state.0);
                        put_i32(out, key);
                    }
                    SceneryData::Stairs { map, destination } => {
                        put_i32(out, map);
                        put_u32(out, destination);
                    }
                    SceneryData::Elevator { kind, level } => words(out, &[kind, level]),
                    SceneryData::LadderUp { destination }
                    | SceneryData::LadderDown { destination } => put_u32(out, destination),
                    SceneryData::Generic(value) => put_i32(out, value),
                }
            }
            PrototypeData::Wall { material } | PrototypeData::Tile { material } => {
                put_i32(out, *material)
            }
            PrototypeData::Misc => {}
        }
        std::mem::take(out)
    }
}

/// SPEC-003 §2.6 extended-flag interaction permissions, including the
/// permissions granted by category or item subtype.
impl Prototype {
    fn item_subtype(&self) -> Option<ItemSubtype> {
        match &self.data {
            PrototypeData::Item(item) => Some(item.data.subtype()),
            _ => None,
        }
    }
    /// `0x800`, or any container by subtype.
    pub fn permits_use(&self) -> bool {
        self.extended.contains(ExtendedFlags::USE)
            || self.item_subtype() == Some(ItemSubtype::Container)
    }
    /// `0x1000`, or any drug by subtype.
    pub fn permits_use_on(&self) -> bool {
        self.extended.contains(ExtendedFlags::USE_ON)
            || self.item_subtype() == Some(ItemSubtype::Drug)
    }
    /// Critters by category; other objects only with `0x4000`.
    pub fn permits_talk(&self) -> bool {
        self.category == Category::Critter || self.extended.contains(ExtendedFlags::TALK)
    }
    /// Non-container items by subtype; containers only with `0x8000`.
    pub fn permits_pickup(&self) -> bool {
        match self.item_subtype() {
            Some(ItemSubtype::Container) => self.extended.contains(ExtendedFlags::PICKUP_CONTAINER),
            Some(_) => true,
            None => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrototypeKind {
    Item(ItemSubtype),
    Critter,
    Scenery(ScenerySubtype),
    Wall,
    Tile,
    Misc,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrototypeInfo {
    pub kind: PrototypeKind,
    pub weapon_defaults: Option<(i32, i32)>, // ammunition PID, capacity
}
impl Prototype {
    pub fn info(&self) -> PrototypeInfo {
        let kind = match &self.data {
            PrototypeData::Item(item) => PrototypeKind::Item(item.data.subtype()),
            PrototypeData::Critter(_) => PrototypeKind::Critter,
            PrototypeData::Scenery(scenery) => PrototypeKind::Scenery(scenery.data.subtype()),
            PrototypeData::Wall { .. } => PrototypeKind::Wall,
            PrototypeData::Tile { .. } => PrototypeKind::Tile,
            PrototypeData::Misc => PrototypeKind::Misc,
        };
        let weapon_defaults = match &self.data {
            PrototypeData::Item(Item {
                data: ItemData::Weapon(weapon),
                ..
            }) => Some((weapon.ammo_pid, weapon.capacity)),
            _ => None,
        };
        PrototypeInfo {
            kind,
            weapon_defaults,
        }
    }
}
pub trait PrototypeLookup {
    fn lookup(&self, pid: u32) -> Result<PrototypeInfo, ParseError>;
}
impl<F: Fn(u32) -> Result<PrototypeInfo, ParseError>> PrototypeLookup for F {
    fn lookup(&self, pid: u32) -> Result<PrototypeInfo, ParseError> {
        self(pid)
    }
}

/// Packed destinations are a separate namespace from plain map-entry hexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedLocation(pub u32);
impl PackedLocation {
    pub fn hex(self) -> u32 {
        self.0 & 0x03ffffff
    }
    pub fn orientation(self) -> u8 {
        ((self.0 >> 26) & 7) as u8
    }
    pub fn elevation(self) -> u8 {
        (self.0 >> 29) as u8
    }
}
