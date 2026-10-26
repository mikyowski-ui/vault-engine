use crate::{
    CharacterSheet, EntityId, EntityStore, ParkMiller, Perk, SequenceIds, StatContext, WorldError,
    assets::WorldAssets,
    dialogue::{GDialog, Reputation},
    queue::{EventKind, TimedEvent, TimedQueue, WorldClock},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vault_formats::{
    MapFile,
    map::{
        CritterState, MapHeader, MapObject, MapObjects, ObjectDetail, ObjectState, ScriptCategory,
        ScriptPrefix,
    },
    pro::{
        BasicFlags, Category, ItemData, Light, LockFlags, MapFlags, Prototype, PrototypeData,
        SceneryData,
    },
};

pub type ObjectRef = u32;

/// SPEC-006 §3.5 the party marks of a script record: "not saved with the
/// map" (script-instance flag `0x08`, SPEC-003 §2.9) and "kept by script
/// removal" (`0x10`).
pub mod party_marks {
    pub const NOT_SAVED: u32 = 0x08;
    pub const KEPT: u32 = 0x10;
    pub const BOTH: u32 = NOT_SAVED | KEPT;
}
/// SPEC-006 §3.5 step 7: a recovered party record's identifier is
/// `(PID & 0xFFFFFF) + 18000` in the critter category.
pub const PARTY_SCRIPT_BASE: u32 = 18000;
/// SPEC-006 §3.5 the player script: `scripts.lst` entry 0, no locals.
pub const PLAYER_SCRIPT_FILE: i32 = 0;

/// A party member's script record copied when a map is left (SPEC-006
/// §3.5 step 1), with its local-variable values, until step 7 recovers it.
#[derive(Debug, Clone)]
pub(crate) struct CarriedScript {
    pub member: ObjectRef,
    pub record: ScriptInstance,
    pub locals: Option<Vec<i32>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptEvent {
    Start,
    MapEnter,
    MapUpdate,
    MapExit,
    Spatial,
    Timed,
    LookAt,
    Description,
    Use,
    UseSkill,
    UseObject,
    Pickup,
    Talk,
    /// The critter-script cycle step's request (SPEC-006 §3.7, see
    /// `WorldHost::critter_cycle_step`).
    Critter,
    /// SPEC-007 combat invocations (fixed parameter 4 turn, 5 join check).
    Combat,
    Damage,
    Destroy,
    /// An item dropped from inventory to the map.
    Drop,
    /// Never requested by the host (SPEC-004 §3.7).
    Create,
    /// SPEC-017 §2.4, Fallout 2 only: the player's Push action on a party
    /// member (SPEC-019 §3.6); an override suppresses the default push.
    Push,
    /// SPEC-017 §2.4, Fallout 2 only; never requested in this revision.
    IsDropping,
    /// SPEC-017 §2.4, Fallout 2 only; never requested: SPEC-019 defines no
    /// path.
    CombatIsStarting,
    /// SPEC-017 §2.4, Fallout 2 only; never requested either.
    CombatIsOver,
}
impl ScriptEvent {
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Start => b"start",
            Self::MapEnter => b"map_enter_p_proc",
            Self::MapUpdate => b"map_update_p_proc",
            Self::MapExit => b"map_exit_p_proc",
            Self::Spatial => b"spatial_p_proc",
            Self::Timed => b"timed_event_p_proc",
            Self::LookAt => b"look_at_p_proc",
            Self::Description => b"description_p_proc",
            Self::Use => b"use_p_proc",
            Self::UseSkill => b"use_skill_on_p_proc",
            Self::UseObject => b"use_obj_on_p_proc",
            Self::Pickup => b"pickup_p_proc",
            Self::Talk => b"talk_p_proc",
            Self::Critter => b"critter_p_proc",
            Self::Combat => b"combat_p_proc",
            Self::Damage => b"damage_p_proc",
            Self::Destroy => b"destroy_p_proc",
            Self::Drop => b"drop_p_proc",
            Self::Create => b"create_p_proc",
            Self::Push => b"push_p_proc",
            Self::IsDropping => b"is_dropping_p_proc",
            Self::CombatIsStarting => b"combat_is_starting_p_proc",
            Self::CombatIsOver => b"combat_is_over_p_proc",
        }
    }
    /// SPEC-004 §3.7 the host event number a request stores as the
    /// instance's action code (`script_action`, SPEC-005 0x80C7).
    pub fn code(self) -> i32 {
        match self {
            Self::Start => 1,
            Self::Spatial => 2,
            Self::Description => 3,
            Self::Pickup => 4,
            Self::Drop => 5,
            Self::Use => 6,
            Self::UseObject => 7,
            Self::UseSkill => 8,
            Self::Talk => 11,
            Self::Critter => 12,
            Self::Combat => 13,
            Self::Damage => 14,
            Self::MapEnter => 15,
            Self::MapExit => 16,
            Self::Create => 17,
            Self::Destroy => 18,
            Self::LookAt => 21,
            Self::Timed => 22,
            Self::MapUpdate => 23,
            Self::Push => 24,
            Self::IsDropping => 25,
            Self::CombatIsStarting => 26,
            Self::CombatIsOver => 27,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectAction {
    LookAt,
    Description,
    Use,
    Pickup,
    Talk,
    UseSkill(i32),
    UseObject(ObjectRef),
    /// SPEC-019 §3.6, Fallout 2 only: the player pushes a party member,
    /// after the talk approach.
    Push,
}
#[derive(Debug, Clone)]
pub struct ScriptRequest {
    pub script: u32,
    pub event: ScriptEvent,
    pub source: ObjectRef,
    pub target: ObjectRef,
    pub fixed: i32,
    pub action: i32,
    pub only_present: bool,
}
#[derive(Debug, Clone)]
pub struct ScriptInstance {
    pub id: u32,
    pub owner: ObjectRef,
    pub file: i32,
    pub flags: u32,
    pub local_base: i32,
    pub local_count: usize,
    pub spatial: Option<(i32, u8, i32)>,
    pub initialized: bool,
    pub program: u32,
    pub source: ObjectRef,
    pub target: ObjectRef,
    pub event: i32,
    pub fixed: i32,
    pub action: i32,
    pub result: i32,
    pub overridden: bool,
    pub margin: i32,
}
/// One active script record in stored list order (SPEC-003 §2.9): its
/// category stream (0 system to 4 critter) and identifier. Lookup by
/// identifier searches only the stream the identifier's top byte names and
/// binds the first equal record there (SPEC-003 §3.3), so a later duplicate
/// or a record in another category's stream is never found: it keeps its
/// stored record, which spatial checks read and saves write back unchanged.
/// Passes that walk the list still visit it and request its identifier.
#[derive(Debug, Clone)]
pub struct ScriptSlot {
    pub stream: u8,
    pub id: u32,
    pub inert: Option<Box<vault_formats::map::MapScript>>,
}
impl ScriptSlot {
    fn live(id: u32) -> Self {
        Self {
            stream: (id >> 24).min(4) as u8,
            id,
            inert: None,
        }
    }
}
/// A script instance from its stored record.
fn script_instance(script: &vault_formats::map::MapScript, local_count: usize) -> ScriptInstance {
    let spatial = if let ScriptPrefix::Spatial { location, radius } = script.prefix {
        Some((location.hex() as i32, location.elevation(), radius))
    } else {
        None
    };
    ScriptInstance {
        id: script.id,
        owner: 0,
        file: script.file_index,
        flags: script.flags.0,
        local_base: script.local_base,
        local_count,
        spatial,
        initialized: false,
        program: 0,
        source: 0,
        target: 0,
        event: script.event,
        fixed: script.parameter,
        action: script.action,
        result: script.result,
        overridden: false,
        margin: 0,
    }
}
#[derive(Debug, Clone)]
pub struct WorldObject {
    pub raw: MapObject,
    pub prototype: Option<Arc<Prototype>>,
    pub character: Option<CharacterSheet>,
    pub name: Vec<u8>,
    pub description: Vec<u8>,
    pub owner: Option<ObjectRef>,
    pub inventory: Vec<(ObjectRef, i32)>,
    pub money: Option<i32>,
    pub active: bool,
    pub opponent: ObjectRef,
}
impl WorldObject {
    pub fn category(&self) -> Category {
        Category::from_pid(self.raw.pid).unwrap_or(Category::Misc)
    }
    pub fn alive(&self) -> bool {
        self.active
            && self
                .character
                .as_ref()
                .is_none_or(|sheet| sheet.current_hp > 0)
    }
    pub fn lock(&self) -> Option<LockFlags> {
        match &self.raw.state {
            ObjectState::Other {
                word,
                detail: ObjectDetail::Plain,
            } if self.prototype.as_ref().is_some_and(|p| {
                matches!(
                    p.data,
                    PrototypeData::Item(vault_formats::pro::Item {
                        data: ItemData::Container { .. },
                        ..
                    })
                )
            }) =>
            {
                Some(LockFlags(*word))
            }
            ObjectState::Other {
                detail: ObjectDetail::Door(flags),
                ..
            } => Some(*flags),
            _ => None,
        }
    }
    pub fn set_lock(&mut self, flags: LockFlags) -> bool {
        if self.lock().is_none() {
            return false;
        }
        match &mut self.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::Door(value),
                ..
            } => *value = flags,
            ObjectState::Other { word, .. } => *word = flags.0,
            _ => return false,
        }
        true
    }
    pub fn weight(&self) -> i32 {
        match self.prototype.as_deref().map(|p| &p.data) {
            Some(PrototypeData::Item(item)) => item.weight.max(0),
            _ => 0,
        }
    }
}
/// Campaign-global indices of the NPC reaction counters. SPEC-006 names these
/// counters without assigning indices, so the host supplies them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReputationGlobals {
    pub child_killer: usize,
    pub bad_monsters: usize,
    pub good_monsters: usize,
    pub champion: usize,
    pub berserker: usize,
}
/// Weapon properties named by SPEC-007; the prototype's numeric weapon-perk
/// codes are supplied by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponPerk {
    LongRange,
    Accurate,
    Penetrate,
    Knockback,
}
impl WeaponPerk {
    /// SPEC-011 §3.22 weapon perk numbers 58-61.
    pub fn from_perk(perk: i32) -> Option<Self> {
        match Perk::from_index(perk)? {
            Perk::WeaponLongRange => Some(Self::LongRange),
            Perk::WeaponAccurate => Some(Self::Accurate),
            Perk::WeaponPenetrate => Some(Self::Penetrate),
            Perk::WeaponKnockback => Some(Self::Knockback),
            _ => None,
        }
    }
}
impl WorldRules {
    pub fn money_pid(&self) -> u32 {
        self.currency_pid.unwrap_or(crate::items::MONEY_PID)
    }
    pub fn weapon_perk(&self, perk: i32) -> Option<WeaponPerk> {
        self.weapon_perks
            .get(&perk)
            .copied()
            .or_else(|| WeaponPerk::from_perk(perk))
    }
}
/// The game whose data an engine runs: Fallout 1 (SPEC-001 to SPEC-014)
/// or Fallout 2 (SPEC-015 to SPEC-019; its scripts run since SPEC-017
/// Revision 3 opened the §8.5 gate, [`crate::fallout2::SCRIPT_GATE_OPEN`]).
pub use vault_formats::GameProfile;
/// Which engine behaviour a world runs with. [`Vanilla`](Self::Vanilla), the
/// default everywhere, is the spec-conformant reference behaviour: every
/// Must-match rule holds and no extension is reachable. `Enhanced` opts in
/// to the engine's own additions (the versioned enhanced opcode range and
/// VFS overrides of the compiled campaign tables); it never changes a
/// vanilla rule by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum EnginePreset {
    #[default]
    Vanilla,
    Enhanced,
}
#[derive(Debug, Clone, Default)]
pub struct WorldRules {
    /// The engine behaviour preset (Vanilla unless a shell opts in).
    pub preset: EnginePreset,
    /// Campaign tables loaded from VFS overrides
    /// ([`WorldHost::load_table_overrides`](crate::WorldHost::load_table_overrides));
    /// `None`, always so in Vanilla, runs on the compiled tables.
    pub tables: Option<std::sync::Arc<crate::tables::CampaignTables>>,
    /// Weapon-perk overrides; SPEC-011 §3.22 numbers 58-61 apply otherwise.
    pub weapon_perks: BTreeMap<i32, WeaponPerk>,
    pub reputation_globals: Option<ReputationGlobals>,
    /// Money PID override; SPEC-011 §3.16 PID 41 applies otherwise.
    pub currency_pid: Option<u32>,
    pub doctor_bag_pid: Option<u32>,
    pub first_aid_kit_pid: Option<u32>,
}
impl WorldRules {
    /// The campaign tables in force: an override set, or the compiled ones.
    pub fn tables(&self) -> &crate::tables::CampaignTables {
        self.tables
            .as_deref()
            .unwrap_or_else(|| crate::tables::CampaignTables::compiled())
    }
}
/// SPEC-013 §3.22 a map transition request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// Map number; 0 (or -2) is the world map and -1 the town map.
    pub map: i32,
    pub elevation: i32,
    /// -1 or 0 keeps the map's default entrance.
    pub hex: i32,
    pub facing: i32,
}
/// SPEC-013 a deferred loot request: from the Steal default (`steal`), a
/// container, or a critter that cannot be talked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootRequest {
    pub looter: ObjectRef,
    pub target: ObjectRef,
    pub steal: bool,
}
/// SPEC-013 §3.10 a scripted explosion request (the `explosion` opcode):
/// one pending at a time, the last request winning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplosionRequest {
    pub hex: i32,
    pub elevation: i32,
    pub min: i32,
    pub max: i32,
}
/// SPEC-013 §3.10 the animated explosion in progress, from its start until
/// its damage step. Damage is computed at the start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveExplosion {
    pub hex: i32,
    pub elevation: u8,
    /// The critter blocking the center hex, its damage and knockback.
    pub victim: Option<(ObjectRef, i32, i32)>,
    /// The credited attacker: the player for a timed explosive, none for a
    /// scripted request.
    pub source: Option<ObjectRef>,
    /// The explosion's art; the scenery scripts are notified once its
    /// center has played, and the damage step follows the last art.
    pub art: ExplosionArt,
    pub notified: bool,
}

/// The id of an explosion art object in render snapshots: no world object
/// has it, so the art is drawn but never picked.
pub const EXPLOSION_ART_ID: u32 = u32::MAX;
/// The PID given to explosion art in render snapshots (miscellaneous, with
/// no prototype).
pub const EXPLOSION_ART_PID: u32 = 0x0500_0000;

/// SPEC-013 §3.10 an explosion's art: one misc art object on the center hex
/// and one on each adjacent hex. Entries start in order; each plays its
/// frames once, one per frame step at the art's rate, and is then removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplosionArt {
    pub elevation: u8,
    /// The misc art identifier.
    pub fid: u32,
    /// Frames per direction (0 when the art cannot be loaded: the entries
    /// then finish as they start) and the frame interval.
    pub frames: usize,
    pub interval_ms: u64,
    /// The center first, then the adjacent hexes in direction order 0-5
    /// (none off the grid).
    pub objects: Vec<ArtObject>,
    /// When the next frame step is due; `None` before the first poll.
    pub next_ms: Option<u64>,
}
/// One explosion art object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtObject {
    pub hex: i32,
    /// Frame steps it waits while an earlier entry is running.
    pub delay: u32,
    /// The frame shown while it plays.
    pub frame: Option<usize>,
    pub done: bool,
}
impl ExplosionArt {
    /// The seven arts of an explosion at `center`; direction 0's adjacent
    /// art carries the start delay `first_delay`, the others none.
    pub fn new(
        center: i32,
        elevation: u8,
        fid: u32,
        [frames, interval_ms]: [u64; 2],
        first_delay: u32,
    ) -> Self {
        let art = |hex, delay| ArtObject {
            hex,
            delay,
            frame: None,
            done: false,
        };
        let mut objects = vec![art(center, 0)];
        for direction in 0..6 {
            if let Ok(hex) = vault_formats::HexGrid::step(center, direction, 1)
                && hex != center
            {
                objects.push(art(hex, if direction == 0 { first_delay } else { 0 }));
            }
        }
        Self {
            elevation,
            fid,
            frames: frames as usize,
            interval_ms: interval_ms.max(1),
            objects,
            next_ms: None,
        }
    }
    /// The center art has played.
    pub fn center_done(&self) -> bool {
        self.objects.first().is_none_or(|object| object.done)
    }
    /// Every art has played and been removed.
    pub fn finished(&self) -> bool {
        self.objects.iter().all(|object| object.done)
    }
    /// One scheduler frame step. A running entry shows its next frame, or
    /// is removed after its last. Then waiting entries start in order: an
    /// entry whose delay is still counting down while an earlier entry
    /// runs takes one off and holds back the rest; the delay is ignored
    /// once no earlier entry runs.
    fn step(&mut self) {
        let frames = self.frames;
        for object in &mut self.objects {
            if let Some(frame) = object.frame {
                if frame + 1 < frames {
                    object.frame = Some(frame + 1);
                } else {
                    object.frame = None;
                    object.done = true;
                }
            }
        }
        for index in 0..self.objects.len() {
            let object = self.objects[index];
            if object.done || object.frame.is_some() {
                continue;
            }
            let earlier_running = self.objects[..index]
                .iter()
                .any(|earlier| earlier.frame.is_some());
            if object.delay > 0 && earlier_running {
                self.objects[index].delay -= 1;
                break;
            }
            let entry = &mut self.objects[index];
            if frames == 0 {
                entry.done = true;
            } else {
                entry.frame = Some(0);
            }
        }
    }
    /// Advance to `now_ms`: the first poll starts the arts, then one frame
    /// step falls due per interval. Returns whether anything changed.
    pub fn poll(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        loop {
            match self.next_ms {
                Some(due) if due > now_ms => break,
                _ => {}
            }
            if self.finished() {
                break;
            }
            self.step();
            changed = true;
            self.next_ms = Some(self.next_ms.unwrap_or(now_ms) + self.interval_ms);
        }
        changed
    }
    /// The playing arts as map objects for drawing.
    fn records(&self) -> impl Iterator<Item = MapObject> + '_ {
        self.objects.iter().filter_map(|object| {
            let mut record = object_record(EXPLOSION_ART_PID, EXPLOSION_ART_ID);
            record.art_id = self.fid;
            record.frame = object.frame?;
            record.hex = Some(object.hex);
            record.elevation = self.elevation;
            Some(record)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorldEffect {
    Diagnostic(String),
    Message(Vec<u8>),
    /// An `enhanced` preset camera request from a script (opcodes
    /// `0x8210`-`0x8213`); presentation only. Vanilla never emits it.
    Camera(crate::camera::CameraCommand),
    /// A SPEC-010 effect request, resolved through the effect lookup chain.
    /// Animation-registered and object effects supply their owner (for the
    /// gender alias and relative volume); direct playback supplies none.
    SoundEffect {
        name: Vec<u8>,
        owner: Option<ObjectRef>,
    },
    /// A library sound slot (SPEC-005 §3.7) started or rewound: the named
    /// file with the slot's mode bits.
    LibrarySound {
        slot: usize,
        filename: Vec<u8>,
        mode: i32,
    },
    Speech(Vec<u8>),
    StopSpeech,
    /// A SPEC-012 game movie that was found and marked played.
    Movie(crate::movie::MovieRequest),
    /// A palette fade to black (`out`) or to the game palette, over
    /// `duration` host milliseconds (0: the standard fade). `from_black`
    /// forces the black palette first (the library `fadein`).
    Fade {
        out: bool,
        duration: i32,
        from_black: bool,
    },
    /// The general-library `fadein` (`from_black`) or `fadeout`: a timed
    /// fade over `duration` host milliseconds (0 or less is instant) that
    /// services background work, so it never blocks (SPEC-014 §3.2).
    LibraryFade {
        out: bool,
        duration: i32,
        from_black: bool,
    },
    Gui {
        opcode: u16,
        values: Vec<i32>,
        texts: Vec<Option<Vec<u8>>>,
    },
    Animation {
        opcode: u16,
        values: Vec<i32>,
        sound: Option<Vec<u8>>,
    },
    Combat {
        attacker: ObjectRef,
        defender: ObjectRef,
        accuracy: i32,
        min: i32,
        max: i32,
        mode: Option<[i32; 2]>,
    },
    Explosion {
        hex: i32,
        elevation: i32,
        min: i32,
        max: i32,
    },
    /// SPEC-014 §3.21: the interface's HP counter rolls to the player's new
    /// HP (combat damage, healing, a drug or stat effect); any other change
    /// is drawn at once.
    RollHitPoints,
    /// SPEC-014 §3.21: the AC counter rolls (the player's turn starts or
    /// ends, combat ends, worn armor changes in the inventory).
    RollArmorClass,
    MapTransition {
        index: i32,
        parameter: i32,
    },
    WorldMap,
    TownMap,
    /// SPEC-013 §3.22 the outgoing map could not be saved for the town or
    /// world map: `map.msg` 1000 shows and the session returns to the main
    /// menu.
    MapSaveFailed,
    /// SPEC-014 §3.35 a loaded map's city music, `sound/music/<name>.acm`,
    /// replacing what plays.
    Music(String),
    Elevator(i32),
    EndGame,
    Slideshow,
    DefaultAction {
        action: ObjectAction,
        target: ObjectRef,
    },
    ObjectLight {
        object: ObjectRef,
        requested: i32,
        radius: i32,
    },
    Floating {
        object: ObjectRef,
        text: Vec<u8>,
        font: i32,
        color: u16,
    },
    /// A SPEC-009 combat taunt: floating text with the packet's font, text
    /// color word, and outline index (-1 for none), wrapped at 200 pixels.
    Taunt {
        object: ObjectRef,
        text: Vec<u8>,
        font: i32,
        color: i32,
        outline: i32,
    },
    /// An AI-driven critter animation (turn, hands, pick-up, wield).
    CritterAnimation {
        object: ObjectRef,
        animation: u8,
    },
    /// A weapon sound request (effect 0 ready, 2 out of ammunition) with
    /// its SPEC-010 effect name.
    WeaponSound {
        object: ObjectRef,
        weapon: ObjectRef,
        effect: u8,
        name: Vec<u8>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldTrace {
    LoadStep(u8),
    Script {
        id: u32,
        event: ScriptEvent,
        source: ObjectRef,
        target: ObjectRef,
        fixed: i32,
        player_light: i32,
    },
    InventoryRefresh,
    QueueFired(u64),
    MapExitCleanup,
}
pub struct WorldState {
    pub entities: EntityStore<WorldObject>,
    references: BTreeMap<ObjectRef, EntityId>,
    /// Unique object number to reference, mirroring the live objects so an
    /// insert checks uniqueness in logarithmic time. `id_index` rebuilds it
    /// when a bulk clear of the stores or a rewritten player record
    /// (restore) has left it behind.
    unique_ids: BTreeMap<u32, ObjectRef>,
    next_reference: u32,
    pub(crate) next_uid: u32,
    pub player: ObjectRef,
    pub party: BTreeSet<ObjectRef>,
    pub map: MapFile,
    pub gvars: Option<Vec<i32>>,
    pub mvars: Vec<i32>,
    pub lvars: Vec<i32>,
    pub scripts: BTreeMap<u32, ScriptInstance>,
    /// Every active script record in stored list order: the five category
    /// streams in order, then block and slot order (SPEC-003 §2.9). New
    /// records join the end of their stream; removing one moves its
    /// stream's last record into the freed slot (SPEC-006 §3.7).
    pub script_order: Vec<ScriptSlot>,
    pub map_script: Option<u32>,
    /// SPEC-006 §3.5 the party records copied on leaving a map, waiting
    /// for step 7 of the next map load.
    pub(crate) party_scripts: Vec<CarriedScript>,
    /// The records the last map import read with an object identifier,
    /// by record: a saved map's copy of the player record names the player
    /// this way, as no object of the map binds it (SPEC-006 §3.5).
    pub(crate) record_objects: Vec<(u32, i32)>,
    pub requests: VecDeque<ScriptRequest>,
    pub clock: WorldClock,
    pub queue: TimedQueue,
    pub rng: ParkMiller,
    pub ids: SequenceIds,
    pub ambient: i32,
    pub tile_light: [Vec<i64>; 3],
    pub elevation: u8,
    pub camera_hex: i32,
    pub spatial_enabled: bool,
    pub scripts_enabled: bool,
    pub background_enabled: bool,
    pub rendering_enabled: bool,
    pub scrolling_enabled: bool,
    pub loading: bool,
    pub restoring: bool,
    pub combat: bool,
    /// Whether the player's sneaking is currently working (perception rule).
    pub sneak_working: bool,
    /// The player's per-turn free-movement pool (twice Bonus Move rank).
    pub free_movement: i32,
    /// Player kill counters by kill category 0-14.
    pub kills: [i32; 15],
    /// Kill experience accumulated during combat, awarded at cleanup.
    pub combat_experience: i32,
    pub combat_state: crate::combat_state::CombatState,
    /// SPEC-008 world-map component and runtime travel state.
    pub world_map: crate::worldmap::WorldMap,
    /// SPEC-018: the Fallout 2 game state, when a Fallout 2 game runs (its
    /// world map, areas, map flags, counters, car and party records).
    pub fallout2: Option<Box<crate::fallout2::Fallout2World>>,
    /// SPEC-009 packets, snapshot, and taunt state.
    pub ai: crate::ai::AiState,
    /// SPEC-014 §3.46 the floating texts, in creation order.
    pub floating: crate::floating::FloatingTexts,
    /// Pip-Boy credited rest minutes (reset when the Pip-Boy opens).
    pub rest_credit: i32,
    /// SPEC-008 working saved-map set, automap database, and carried
    /// save components.
    pub persistence: crate::persistence::Persistence,
    /// A scripted combat request awaiting the host's next update.
    pub pending_combat: Option<crate::combat_state::CombatRequest>,
    /// SPEC-013 §3.22 the pending map transition (the last request wins).
    pub pending_transition: Option<Transition>,
    /// SPEC-013 §3.22 the outgoing map was saved and cleared for the town
    /// or world map: no map is loaded until a location is entered.
    pub unloaded: bool,
    /// SPEC-013 §3.9/§3.16 a deferred loot or steal request for the
    /// presentation layer's loot window.
    pub pending_loot: Option<LootRequest>,
    /// SPEC-013 §3.10 the scripted explosion awaiting the request pass.
    pub pending_explosion: Option<ExplosionRequest>,
    /// SPEC-013 §3.10 the animated explosion in progress, if any.
    pub explosion: Option<ActiveExplosion>,
    /// SPEC-007 §3.10 attack presentations, played in order: each owns the
    /// drawn state of the objects it shows until it ends.
    pub shows: VecDeque<crate::choreography::Show>,
    /// The SPEC-014 preferences the choreography reads: `combat_speed`,
    /// added to the walk animation's rate in combat, and `player_speedup`,
    /// which lets the player's walking use it too.
    pub combat_speed: i32,
    pub player_speedup: bool,
    /// SPEC-014 §3.18 the `combat_messages` preference: 1 "Verbose"
    /// (default), 0 "Brief". Combat message formatting reads it.
    pub verbose_combat_messages: bool,
    /// SPEC-014 §3.44 the `target_highlight` preference now (0-2, default
    /// 2), which the target-outline enable step reads.
    pub target_highlight: i32,
    /// The `enhanced` companions' combat distances by object ID
    /// ([`crate::companion`]); empty under Vanilla.
    pub companion_distance: BTreeMap<u32, crate::companion::Distance>,
    /// The `enhanced` preset's other game state, the doors the player
    /// opened by hand ([`crate::doors`]); empty under Vanilla.
    pub enhanced: crate::doors::EnhancedState,
    /// SPEC-014 §3.27 the in-world cursor is in crosshair mode (the
    /// presentation layer keeps it current): enabling target outlines
    /// needs it.
    pub crosshair: bool,
    /// SPEC-005 §3.8 the default player art index: the jumpsuit for the
    /// player's gender, set by each gender update (0 before the first).
    pub default_player_art: u16,
    /// SPEC-014 §3.33 the hexes the player entered since the seen state was
    /// last processed: 40000 bits, 8 hexes per byte in hex order.
    pub visited: Vec<u8>,
    /// SPEC-014 §3.36 roof tracking: the player's last roof square, its
    /// elevation, and whether that square was empty.
    pub roof: crate::roof::RoofTracker,
    pub combat_quit: bool,
    pub combat_actor: ObjectRef,
    pub ui_disabled: bool,
    pub mode: i32,
    pub difficulty: i32,
    /// The configured combat-difficulty preference; each reader applies its
    /// own default when unset (SPEC-005 opcode 0, SPEC-009 AI 1).
    pub combat_difficulty: Option<i32>,
    pub running_burning: i32,
    pub violence: Option<i32>,
    pub recording: bool,
    pub debug_messages: bool,
    pub cursor_objects: [ObjectRef; 2],
    pub active_hand: i32,
    pub dialogue: GDialog,
    pub reputation: Reputation,
    pub rules: WorldRules,
    /// Prototypes seen this session, by PID (ammunition terms of loaded
    /// weapons; new ammunition objects from unloading).
    pub(crate) prototypes: BTreeMap<u32, Arc<Prototype>>,
    /// Live critter-prototype stat arrays (base, bonus) changed this
    /// session: SPEC-011 §3.2 bonus arrays belong to the prototype.
    pub prototype_stats: BTreeMap<u32, ([i32; 35], [i32; 35])>,
    pub effects: VecDeque<WorldEffect>,
    pub trace: Vec<WorldTrace>,
    pub elapsed_ms: u64,
    /// SPEC-014 §3.44 each object's latest insertion into its hex list
    /// (a [`next_insertion`] stamp): set when it is inserted and renewed by
    /// [`WorldState::reinsert`].
    insertions: BTreeMap<ObjectRef, u64>,
}
/// SPEC-014 §3.44 the next per-hex list insertion stamp. Within one group
/// of a hex list (elevation, then flat class) objects are drawn by
/// ascending stamp, so the latest insertion comes last. The sequence is
/// shared by the world's objects and the attack shows' displays, so a
/// display that steps a frame goes after the world objects inserted
/// before it; only the stamps' relative order matters.
pub fn next_insertion() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
pub fn empty_map() -> MapFile {
    MapFile {
        header: MapHeader {
            version: 19,
            name: [0; 16],
            entry_hex: 20100,
            entry_elevation: 0,
            entry_orientation: 0,
            stored_local_count: 0,
            script_reference: 0,
            flags: MapFlags(14),
            darkness: 1,
            stored_global_count: 0,
            map_number: -1,
            last_visit: 0,
            reserved: [0; 44],
        },
        mvars: Vec::new(),
        lvars: Vec::new(),
        tiles: std::array::from_fn(|_| None),
        scripts: std::array::from_fn(|_| ScriptCategory {
            declared_count: 0,
            blocks: Vec::new(),
        }),
        objects: MapObjects {
            total: 0,
            elevations: std::array::from_fn(|_| Vec::new()),
            consumed_len: 0,
        },
        consumed_len: 0,
        diagnostics: Vec::new(),
    }
}
/// A record drawn as a presentation display shows it; `None` when the
/// display is off the map or hidden.
fn displayed(mut record: MapObject, display: &crate::choreography::Display) -> Option<MapObject> {
    if display.hidden {
        return None;
    }
    record.hex = Some(display.hex?);
    record.art_id = display.art;
    record.frame = display.frame;
    record.elevation = display.elevation;
    record.orientation = display.orientation;
    record.displacement = display.displacement;
    if display.flat {
        record.flags.0 |= BasicFlags::FLAT.0;
    }
    Some(record)
}
pub fn object_record(pid: u32, id: u32) -> MapObject {
    MapObject {
        id,
        hex: None,
        displacement: [0; 2],
        screen: [0; 2],
        frame: 0,
        orientation: 0,
        art_id: pid & 0xff000000,
        flags: BasicFlags(0),
        stored_elevation: 0,
        elevation: 0,
        pid,
        combat_id: -1,
        light: Light {
            radius: 0,
            intensity: 0,
        },
        outline: 0,
        script_id: -1,
        cached_script_file: -1,
        inventory_capacity: 0,
        state: ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Plain,
        },
        inventory: Vec::new(),
        owner: None,
        file_offset: 0,
    }
}
impl WorldState {
    pub fn empty(seed: i32) -> Result<Self, WorldError> {
        Ok(Self {
            entities: EntityStore::with_key(),
            references: BTreeMap::new(),
            unique_ids: BTreeMap::new(),
            next_reference: 0x10000,
            next_uid: 1,
            player: 0,
            party: BTreeSet::new(),
            map: empty_map(),
            gvars: None,
            mvars: Vec::new(),
            lvars: Vec::new(),
            scripts: BTreeMap::new(),
            script_order: Vec::new(),
            map_script: None,
            party_scripts: Vec::new(),
            record_objects: Vec::new(),
            requests: VecDeque::new(),
            clock: WorldClock::default(),
            queue: TimedQueue::default(),
            rng: ParkMiller::seeded(seed)?,
            ids: SequenceIds::default(),
            ambient: 65536,
            tile_light: std::array::from_fn(|_| vec![655; 40000]),
            elevation: 0,
            camera_hex: 20100,
            spatial_enabled: true,
            scripts_enabled: true,
            background_enabled: true,
            rendering_enabled: true,
            scrolling_enabled: true,
            loading: false,
            restoring: false,
            combat: false,
            sneak_working: false,
            free_movement: 0,
            kills: [0; 15],
            combat_experience: 0,
            combat_state: Default::default(),
            world_map: Default::default(),
            fallout2: None,
            ai: Default::default(),
            floating: Default::default(),
            rest_credit: 0,
            persistence: Default::default(),
            pending_combat: None,
            pending_transition: None,
            unloaded: false,
            pending_loot: None,
            pending_explosion: None,
            explosion: None,
            shows: VecDeque::new(),
            combat_speed: 0,
            player_speedup: false,
            verbose_combat_messages: true,
            target_highlight: 2,
            companion_distance: BTreeMap::new(),
            enhanced: Default::default(),
            crosshair: false,
            default_player_art: 0,
            visited: vec![0; crate::persistence::VISITED_BYTES],
            roof: crate::roof::RoofTracker::NONE,
            combat_quit: false,
            combat_actor: 0,
            ui_disabled: false,
            mode: 0,
            difficulty: 1,
            combat_difficulty: None,
            running_burning: 1,
            violence: None,
            recording: false,
            debug_messages: false,
            cursor_objects: [0; 2],
            active_hand: 1,
            dialogue: GDialog::default(),
            reputation: Reputation::default(),
            rules: WorldRules::default(),
            prototypes: BTreeMap::new(),
            prototype_stats: BTreeMap::new(),
            effects: VecDeque::new(),
            trace: Vec::new(),
            elapsed_ms: 0,
            insertions: BTreeMap::new(),
        })
    }
    pub fn new(player: CharacterSheet, seed: i32) -> Result<Self, WorldError> {
        let mut world = Self::empty(seed)?;
        world.rng.startup_diagnostic();
        world.insert_player(player)?;
        Ok(world)
    }
    /// A world like `new` whose generator skips the startup diagnostic: a
    /// game reset keeps the running generator, so those draws would only
    /// be discarded.
    pub fn fresh(player: CharacterSheet, seed: i32) -> Result<Self, WorldError> {
        let mut world = Self::empty(seed)?;
        world.insert_player(player)?;
        Ok(world)
    }
    fn insert_player(&mut self, player: CharacterSheet) -> Result<(), WorldError> {
        let world = self;
        let mut raw = object_record(0x01000000, 0);
        raw.hex = Some(20100);
        raw.state = ObjectState::Critter(CritterState {
            reaction: 0,
            damage: 0,
            maneuvers: 0,
            ap: 0,
            results: vault_formats::pro::CombatFlags(0),
            ai: 0,
            team: 0,
            attacker: -1,
            hp: player.current_hp,
            radiation: 0,
            poison: 0,
        });
        world.player = world.insert(WorldObject {
            name: player.name.clone(),
            description: Vec::new(),
            raw,
            prototype: None,
            character: Some(player),
            owner: None,
            inventory: Vec::new(),
            money: None,
            active: true,
            opponent: 0,
        })?;
        world.create_player_script()?;
        Ok(())
    }
    /// SPEC-006 §3.5 the player's script record, created from nothing (at
    /// program start, for a new game, at a game reset, and for a loaded
    /// player): a new critter-category instance of `scripts.lst` entry 0
    /// with no loaded program, action code 0, no local slice and no timers,
    /// carrying both party marks.
    pub fn create_player_script(&mut self) -> Result<u32, WorldError> {
        let sid = self.add_script(self.player, PLAYER_SCRIPT_FILE, 4, 0)?;
        let script = self.scripts.get_mut(&sid).expect("added script");
        script.flags |= party_marks::BOTH;
        Ok(sid)
    }
    /// SPEC-018 §6.5 (Revision 3): the player's record `sid` gets a local
    /// slice of `count` values: `values` when given, else zeros allocated on
    /// first use.
    pub(crate) fn set_player_locals(&mut self, sid: u32, count: usize, values: Option<Vec<i32>>) {
        let base = self.lvars.len();
        let Some(script) = self.scripts.get_mut(&sid) else {
            return;
        };
        script.local_count = count;
        script.local_base = -1;
        if let Some(values) = values.filter(|values| values.len() == count) {
            script.local_base = base as i32;
            self.lvars.extend(values);
        }
    }
    /// Tests about other critter records leave the player's out of the
    /// critter stream and the cycle.
    #[cfg(test)]
    pub(crate) fn drop_player_script(&mut self) {
        if let Some(sid) = self.script_for(self.player) {
            self.remove_script(sid);
        }
        if let Some(player) = self.get_mut(self.player) {
            player.raw.script_id = -1;
        }
    }
    /// The party: the player (party member 0) and the other members.
    pub fn party_members(&self) -> Vec<ObjectRef> {
        std::iter::once(self.player)
            .chain(self.party.iter().copied().filter(|m| *m != self.player))
            .collect()
    }
    /// The script records of the party members, in member order.
    pub fn party_records(&self) -> Vec<u32> {
        self.party_members()
            .into_iter()
            .filter_map(|member| self.script_for(member))
            .collect()
    }
    /// SPEC-006 §3.5 saving a game: the party records lose their marks, so
    /// they are written with the map's other scripts, and get them back
    /// afterwards (`marked` false, then true).
    pub fn set_party_marks(&mut self, marked: bool) {
        for sid in self.party_records() {
            if let Some(script) = self.scripts.get_mut(&sid) {
                if marked {
                    script.flags |= party_marks::BOTH;
                } else {
                    script.flags &= !party_marks::BOTH;
                }
            }
        }
    }
    /// SPEC-006 §3.5 leaving a map (step 1): each party member's record and
    /// local values are copied, its marks cleared, and the instance removed
    /// in the ordinary way, cancelling its owner's script timers. The
    /// member keeps no script reference until step 7.
    fn carry_party_scripts(&mut self) {
        for member in self.party_members() {
            let Some(sid) = self.script_for(member) else {
                continue;
            };
            let record = self.scripts.get(&sid).cloned().expect("member script");
            let locals = usize::try_from(record.local_base)
                .ok()
                .and_then(|base| self.lvars.get(base..base.checked_add(record.local_count)?))
                .map(<[i32]>::to_vec);
            if let Some(script) = self.scripts.get_mut(&sid) {
                script.flags &= !party_marks::BOTH;
            }
            self.remove_script(sid);
            self.queue.retain(|event| {
                !matches!(event.kind, EventKind::Script { script, .. }
                    if script == sid || event.object == member)
            });
            if let Some(object) = self.get_mut(member) {
                object.raw.script_id = -1;
            }
            self.party_scripts.push(CarriedScript {
                member,
                record,
                locals,
            });
        }
    }
    /// SPEC-006 §3.5 step 7: each copied party record becomes a new
    /// critter-category instance with identifier `(PID & 0xFFFFFF) + 18000`
    /// (18000 for the player), keeping its action code, fixed parameter,
    /// flags and other words. It has no loaded program and has not run,
    /// carries both marks again and gets a fresh local slice for the saved
    /// values; it joins the end of the critter stream, after the map's own
    /// records. A taken identifier (two members of one PID, or a game
    /// load's saved copy of the player record, which the load then removes)
    /// moves on to the next free one, so records never alias.
    pub(crate) fn recover_party_scripts(&mut self) -> Result<(), WorldError> {
        for carried in std::mem::take(&mut self.party_scripts) {
            let CarriedScript {
                member,
                mut record,
                locals,
            } = carried;
            let Some(pid) = self.get(member).map(|object| object.raw.pid) else {
                continue;
            };
            let number = ((pid & 0x00ff_ffff) + PARTY_SCRIPT_BASE) & 0x00ff_ffff;
            let wanted = (4 << 24) | number;
            let id = if self.scripts.contains_key(&wanted) {
                (number..0x0100_0000)
                    .chain(1..number)
                    .map(|n| (4 << 24) | n)
                    .find(|id| !self.scripts.contains_key(id))
                    .ok_or(WorldError::Arithmetic)?
            } else {
                wanted
            };
            record.id = id;
            record.owner = member;
            record.initialized = false;
            record.program = 0;
            record.source = 0;
            record.target = member;
            record.flags |= party_marks::BOTH;
            match locals {
                Some(values) => {
                    record.local_base = self.lvars.len() as i32;
                    record.local_count = values.len();
                    self.lvars.extend(values);
                }
                None => record.local_base = -1,
            }
            self.scripts.insert(id, record);
            self.append_script(id);
            if let Some(object) = self.get_mut(member) {
                object.raw.script_id = id as i32;
            }
        }
        Ok(())
    }
    /// SPEC-006 §3.5 loading a game: the record the loaded player names
    /// (`saved`; `SAVE.DAT` stores -1 there, SPEC-008 §2.5), the copy the
    /// saved map restored (its record names the player's object and no map
    /// object bound it) and every record owned by the player (the one the
    /// reset created and the map load recovered) are removed, with their
    /// script timers, and the player gets a newly created record.
    ///
    /// Under Fallout 2, `player_locals` is `scripts.lst` entry 0's declared
    /// count (SPEC-018 §6.5, Revision 3): the new record has a slice of that
    /// size, holding the saved map copy's values when it has a slice of that
    /// size, and zeros otherwise.
    pub(crate) fn replace_player_script(
        &mut self,
        saved: i32,
        player_locals: Option<usize>,
    ) -> Result<u32, WorldError> {
        let player = self.player;
        let player_id = self.get(player).map_or(-1, |object| object.raw.id as i32);
        let copies: Vec<u32> = self
            .record_objects
            .iter()
            .filter(|(sid, object)| {
                *object == player_id && self.scripts.get(sid).is_some_and(|s| s.owner == 0)
            })
            .map(|(sid, _)| *sid)
            .collect();
        let kept = player_locals.and_then(|count| {
            copies.iter().find_map(|sid| {
                let copy = self.scripts.get(sid)?;
                let base = usize::try_from(copy.local_base).ok()?;
                (copy.local_count == count)
                    .then(|| self.lvars.get(base..base.checked_add(count)?))
                    .flatten()
                    .map(<[i32]>::to_vec)
            })
        });
        let mut old: Vec<u32> = self
            .scripts
            .values()
            .filter(|script| script.owner == player)
            .map(|script| script.id)
            .chain(copies)
            .collect();
        if let Ok(saved) = u32::try_from(saved)
            && self.scripts.contains_key(&saved)
        {
            old.push(saved);
        }
        old.sort_unstable();
        old.dedup();
        // Created first, so the new identifier differs from the old ones.
        let sid = self.create_player_script()?;
        if let Some(count) = player_locals {
            self.set_player_locals(sid, count, kept);
        }
        for id in &old {
            self.remove_script(*id);
        }
        self.queue.retain(
            |event| !matches!(event.kind, EventKind::Script { script, .. } if old.contains(&script)),
        );
        Ok(sid)
    }
    pub fn insert(&mut self, mut object: WorldObject) -> Result<ObjectRef, WorldError> {
        if self.references.len() >= 200_000 {
            return Err(WorldError::Invalid("world object limit exceeded".into()));
        }
        if object.raw.id == 0 || self.id_index().contains_key(&object.raw.id) {
            while {
                let next = self.next_uid;
                self.id_index().contains_key(&next)
            } {
                self.next_uid = self.next_uid.checked_add(1).ok_or(WorldError::Arithmetic)?;
            }
            object.raw.id = self.next_uid;
            self.next_uid = self.next_uid.checked_add(1).ok_or(WorldError::Arithmetic)?;
        }
        let reference = self.next_reference;
        self.next_reference = self
            .next_reference
            .checked_add(1)
            .ok_or(WorldError::Arithmetic)?;
        if let Some(prototype) = &object.prototype {
            self.prototypes
                .entry(prototype.pid)
                .or_insert_with(|| prototype.clone());
        }
        let id = object.raw.id;
        let key = self.entities.insert(object);
        self.references.insert(reference, key);
        self.unique_ids.insert(id, reference);
        // SPEC-014 §3.44: loading (in file order), creating and copying
        // insert the object at the end of its group.
        self.insertions.insert(reference, next_insertion());
        Ok(reference)
    }
    /// SPEC-014 §3.44 remove the object from its hex list and insert it
    /// again at the end of its group, whatever changed (even nothing).
    pub fn reinsert(&mut self, reference: ObjectRef) {
        if self.references.contains_key(&reference) {
            self.insertions.insert(reference, next_insertion());
        }
    }
    /// SPEC-014 §3.44 the object's latest insertion stamp, if it has one: a
    /// render-side object stamped later draws after it in its group.
    pub fn insertion(&self, reference: ObjectRef) -> Option<u64> {
        self.insertions.get(&reference).copied()
    }
    /// SPEC-014 §3.44 a pixel-offset change: it always reinserts the
    /// object, even when the offset is unchanged.
    pub fn set_offset(&mut self, reference: ObjectRef, offset: [i32; 2]) {
        if let Some(object) = self.get_mut(reference) {
            object.raw.displacement = offset;
            self.reinsert(reference);
        }
    }
    /// [`Self::set_offset`] to the current offset plus `shift` (an
    /// animation frame's shift, applied even when it is zero).
    pub fn shift_offset(&mut self, reference: ObjectRef, shift: [i32; 2]) {
        if let Some(object) = self.get(reference) {
            let [x, y] = object.raw.displacement;
            self.set_offset(
                reference,
                [x.wrapping_add(shift[0]), y.wrapping_add(shift[1])],
            );
        }
    }
    /// SPEC-014 §3.44 an object flipped between flat and not flat changes
    /// group and is reinserted.
    pub fn set_flat(&mut self, reference: ObjectRef, flat: bool) {
        if let Some(object) = self.get_mut(reference)
            && object.raw.flags.contains(BasicFlags::FLAT) != flat
        {
            if flat {
                object.raw.flags.0 |= BasicFlags::FLAT.0;
            } else {
                object.raw.flags.0 &= !BasicFlags::FLAT.0;
            }
            self.reinsert(reference);
        }
    }
    /// The unique-id index, rebuilt when it no longer mirrors the live
    /// objects (see `unique_ids`).
    fn id_index(&mut self) -> &BTreeMap<u32, ObjectRef> {
        let player_id = self.get(self.player).map(|object| object.raw.id);
        let stale = self.unique_ids.len() != self.references.len()
            || player_id.is_some_and(|id| self.unique_ids.get(&id) != Some(&self.player));
        if stale {
            self.unique_ids = self
                .iter()
                .map(|(reference, object)| (object.raw.id, reference))
                .collect();
        }
        &self.unique_ids
    }
    pub fn get(&self, reference: ObjectRef) -> Option<&WorldObject> {
        self.entities.get(*self.references.get(&reference)?)
    }
    pub fn get_mut(&mut self, reference: ObjectRef) -> Option<&mut WorldObject> {
        let key = *self.references.get(&reference)?;
        self.entities.get_mut(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (ObjectRef, &WorldObject)> {
        self.references.iter().filter_map(|(reference, key)| {
            self.entities.get(*key).map(|object| (*reference, object))
        })
    }
    pub fn reference_for_id(&self, id: u32) -> Option<ObjectRef> {
        self.iter()
            .find(|(_, object)| object.raw.id == id)
            .map(|(reference, _)| reference)
    }
    pub fn diagnostic(&mut self, message: impl Into<String>) {
        self.emit(WorldEffect::Diagnostic(message.into()));
    }
    pub fn emit(&mut self, effect: WorldEffect) {
        if self.effects.len() < 100_000 {
            self.effects.push_back(effect);
        }
    }
    pub fn context(&self, object: ObjectRef) -> StatContext {
        let (ap, blinded) = match self.get(object).map(|o| &o.raw.state) {
            Some(ObjectState::Critter(state)) => (state.ap, state.results.0 & 0x40 != 0),
            _ => (0, false),
        };
        StatContext {
            ticks: self.clock.ticks,
            blinded,
            combat: self.combat,
            taking_turn: self.combat_actor == object,
            remaining_ap: ap,
            visible_light: self.visible_light(object),
            difficulty: self.difficulty,
        }
    }
    pub fn player_sheet(&self) -> Option<&CharacterSheet> {
        self.get(self.player)?.character.as_ref()
    }
    pub fn player_sheet_mut(&mut self) -> Option<&mut CharacterSheet> {
        self.get_mut(self.player)?.character.as_mut()
    }
    pub fn player_stat(&self, index: usize) -> i32 {
        self.player_sheet()
            .map_or(0, |sheet| sheet.stat(index, self.context(self.player)))
    }
    /// SPEC-011 §3.20 player experience award (Swift Learner, clamping,
    /// and level advancement); returns the levels gained. The host prints
    /// and plays each level-up.
    pub fn give_experience(&mut self, amount: i32) -> u32 {
        let context = self.context(self.player);
        let levels = self
            .player_sheet_mut()
            .map_or(0, |sheet| sheet.add_experience(amount, context));
        self.sync_hit_points(self.player);
        levels
    }
    /// Copy a sheet's current HP (after sheet-level healing) into the
    /// critter's instance record.
    pub fn sync_hit_points(&mut self, critter: ObjectRef) {
        if let Some(object) = self.get_mut(critter)
            && let Some(hp) = object.character.as_ref().map(|sheet| sheet.current_hp)
            && let ObjectState::Critter(state) = &mut object.raw.state
        {
            state.hp = hp;
        }
    }
    pub fn perk(&self, perk: Perk) -> u32 {
        self.player_sheet().map_or(0, |sheet| sheet.rank(perk))
    }
    pub fn import_map(
        &mut self,
        map: MapFile,
        assets: &impl WorldAssets,
    ) -> Result<(), WorldError> {
        // SPEC-006 §3.5 step 1: the party records leave with their members.
        self.carry_party_scripts();
        let keep: BTreeSet<_> = self
            .iter()
            .filter(|(reference, _)| {
                *reference == self.player
                    || self.party.contains(reference)
                    || self.owned_by_kept(*reference)
            })
            .map(|(r, _)| r)
            .collect();
        let remove: Vec<_> = self
            .iter()
            .filter(|(r, _)| !keep.contains(r))
            .map(|(r, _)| r)
            .collect();
        for reference in remove {
            self.erase(reference);
        }
        // SPEC-014 §3.36: removing all objects resets the roof tracking to
        // "none"; the entrance placement then hides the roof over the player.
        self.roof = crate::roof::RoofTracker::NONE;
        // SPEC-014 §3.33: the visited hexes belong to the map that is left
        // (a map saved on leaving has already folded them into its seen
        // flags; an excluded or unnumbered map drops them).
        self.visited.fill(0);
        // The kept items' script instances and local variables travel with
        // them, under ids that cannot alias the new map's scripts; a kept
        // object whose script instance is gone loses its reference instead
        // of dangling. (The party members' records were copied above, for
        // step 7.)
        let mut carried = Vec::new();
        for reference in &keep {
            let Some(sid) = self
                .get(*reference)
                .map(|object| object.raw.script_id)
                .filter(|sid| *sid != -1)
            else {
                continue;
            };
            let Some(instance) = self.scripts.get(&(sid as u32)).cloned() else {
                if let Some(object) = self.get_mut(*reference) {
                    object.raw.script_id = -1;
                }
                continue;
            };
            let locals = usize::try_from(instance.local_base)
                .ok()
                .and_then(|base| self.lvars.get(base..base + instance.local_count))
                .map(<[i32]>::to_vec);
            carried.push((*reference, instance, locals));
        }
        self.scripts.clear();
        self.script_order.clear();
        self.record_objects.clear();
        self.map_script = None;
        self.mvars = map.mvars.clone();
        self.lvars = map.lvars.clone();
        // SPEC-003 §3.3: the first record of an identifier in the stream
        // its top byte names binds; a later duplicate, or a record in
        // another category's stream, is kept inert.
        for (stream, category) in map.scripts.iter().enumerate() {
            for script in category.active() {
                let found =
                    script.id >> 24 == stream as u32 && !self.scripts.contains_key(&script.id);
                self.script_order.push(ScriptSlot {
                    stream: stream as u8,
                    id: script.id,
                    inert: (!found).then(|| Box::new(script.clone())),
                });
                if found {
                    if script.object_id != -1 {
                        self.record_objects.push((script.id, script.object_id));
                    }
                    let locals = if script.local_count > 0 {
                        script.local_count as usize
                    } else {
                        assets.local_count(script.file_index)
                    };
                    self.scripts
                        .insert(script.id, script_instance(script, locals));
                }
            }
        }
        let roots: Vec<_> = map.objects.elevations.iter().flatten().cloned().collect();
        self.map = map;
        for root in roots {
            self.import_object(root, None, assets)?;
        }
        for (reference, mut instance, locals) in carried {
            let category = (instance.id >> 24).min(4);
            let id = (1..0x1000000)
                .map(|n| (category << 24) | n)
                .find(|id| !self.scripts.contains_key(id))
                .ok_or(WorldError::Arithmetic)?;
            instance.id = id;
            // The left map's programs were purged: the script runs its
            // initial stream again on its next request.
            instance.initialized = false;
            instance.program = 0;
            instance.source = 0;
            instance.target = instance.owner;
            match locals {
                Some(values) => {
                    instance.local_base = self.lvars.len() as i32;
                    instance.local_count = values.len();
                    self.lvars.extend(values);
                }
                None => instance.local_base = -1,
            }
            self.scripts.insert(id, instance);
            self.append_script(id);
            if let Some(object) = self.get_mut(reference) {
                object.raw.script_id = id as i32;
            }
        }
        self.rebuild_lighting();
        Ok(())
    }
    /// SPEC-006 §3.5 step 3 `[May-improve]` safe rule for a saved map:
    /// every placed critter but the player (PID -1 skipped) takes its last
    /// attacker from its stored word ([`Self::resolve_last_attacker`]).
    /// Intent bits and every other combat field are untouched.
    pub(crate) fn resolve_last_attackers(&mut self) {
        let critters: Vec<_> = self
            .iter()
            .filter(|(reference, object)| {
                *reference != self.player
                    && object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.pid != u32::MAX
            })
            .map(|(reference, _)| reference)
            .collect();
        for critter in critters {
            self.resolve_last_attacker(critter);
        }
    }
    /// A critter's stored last-attacker word as its reference: a
    /// nonnegative combat identifier resolves to the placed critter with
    /// that identifier, or to nothing when none matches; -1 is nothing.
    pub(crate) fn resolve_last_attacker(&mut self, critter: ObjectRef) {
        let Some(ObjectState::Critter(state)) = self.get(critter).map(|o| &o.raw.state) else {
            return;
        };
        let attacker = state.attacker;
        let found = (attacker >= 0)
            .then(|| {
                self.iter()
                    .find(|(_, object)| {
                        object.owner.is_none()
                            && object.raw.hex.is_some()
                            && object.category() == Category::Critter
                            && object.raw.combat_id == attacker
                    })
                    .map(|(found, _)| found)
            })
            .flatten()
            .unwrap_or(0);
        if let Some(object) = self.get_mut(critter) {
            object.opponent = found;
        }
    }
    fn owned_by_kept(&self, mut reference: ObjectRef) -> bool {
        for _ in 0..64 {
            let Some(parent) = self.get(reference).and_then(|o| o.owner) else {
                return false;
            };
            if parent == self.player || self.party.contains(&parent) {
                return true;
            }
            reference = parent;
        }
        false
    }
    pub(crate) fn import_object(
        &mut self,
        mut raw: MapObject,
        owner: Option<ObjectRef>,
        assets: &impl WorldAssets,
    ) -> Result<ObjectRef, WorldError> {
        let children = std::mem::take(&mut raw.inventory);
        let prototype = assets.prototype(raw.pid).ok();
        let mut character = match prototype.as_deref().map(|p| &p.data) {
            Some(PrototypeData::Critter(critter)) => Some(self.critter_sheet(raw.pid, critter)),
            _ => None,
        };
        if let ObjectState::Other {
            detail: ObjectDetail::Weapon { ammo_pid, .. },
            ..
        } = raw.state
            && let Ok(ammo) = u32::try_from(ammo_pid)
            && !self.prototypes.contains_key(&ammo)
            && let Ok(prototype) = assets.prototype(ammo)
        {
            self.remember_prototype(prototype);
        }
        if let Some(sheet) = &mut character
            && let ObjectState::Critter(state) = &raw.state
        {
            sheet.current_hp = state.hp;
            sheet.radiation = state.radiation;
            sheet.poison = state.poison;
        }
        let name = prototype
            .as_deref()
            .and_then(|p| assets.prototype_text(p, false))
            .unwrap_or_else(|| format!("Object {:08X}", raw.pid).into_bytes());
        let description = prototype
            .as_deref()
            .and_then(|p| assets.prototype_text(p, true))
            .unwrap_or_default();
        let money = (self.rules.money_pid() == raw.pid).then_some(match raw.state {
            ObjectState::Other { word, .. } => word as i32,
            _ => 0,
        });
        let sid = raw.script_id as u32;
        let reference = self.insert(WorldObject {
            raw,
            prototype,
            character,
            name,
            description,
            owner,
            inventory: Vec::new(),
            money,
            active: true,
            opponent: 0,
        })?;
        if let Some(script) = self.scripts.get_mut(&sid) {
            script.owner = reference;
        }
        for child in children {
            let reference_child = self.import_object(child.object, Some(reference), assets)?;
            self.get_mut(reference)
                .expect("inserted parent")
                .inventory
                .push((reference_child, child.quantity as i32));
        }
        Ok(reference)
    }
    pub fn spawn(
        &mut self,
        prototype: Arc<Prototype>,
        hex: i32,
        elevation: u8,
    ) -> Result<ObjectRef, WorldError> {
        if !(0..40000).contains(&hex) || elevation > 2 {
            return Err(WorldError::Invalid("invalid placement".into()));
        }
        let reference = self.instantiate(prototype)?;
        let object = self.get_mut(reference).expect("instantiated object");
        object.raw.hex = Some(hex);
        object.raw.elevation = elevation;
        self.reinsert(reference);
        self.rebuild_lighting();
        Ok(reference)
    }
    /// A critter's sheet from its prototype, with any stat arrays this
    /// session changed on that prototype.
    pub(crate) fn critter_sheet(
        &self,
        pid: u32,
        critter: &vault_formats::pro::Critter,
    ) -> CharacterSheet {
        let mut sheet = CharacterSheet::from_critter(critter);
        if let Some((base, bonus)) = self.prototype_stats.get(&pid) {
            (sheet.base, sheet.bonus) = (*base, *bonus);
            sheet.current_hp = sheet.stat(7, StatContext::default());
        }
        sheet
    }
    /// A new unplaced, unowned instance of a prototype.
    pub fn instantiate(&mut self, prototype: Arc<Prototype>) -> Result<ObjectRef, WorldError> {
        let mut raw = object_record(prototype.pid, 0);
        raw.art_id = prototype.art_id;
        // Only persistent presentation/blocking bits propagate from a prototype.
        raw.flags = BasicFlags(prototype.flags.0 & 0xb00fd818);
        raw.light = prototype.light.unwrap_or(Light {
            radius: 0,
            intensity: 0,
        });
        let character = if let PrototypeData::Critter(critter) = &prototype.data {
            let sheet = self.critter_sheet(prototype.pid, critter);
            raw.state = ObjectState::Critter(CritterState {
                reaction: 0,
                damage: 0,
                maneuvers: 0,
                ap: 0,
                results: vault_formats::pro::CombatFlags(0),
                ai: critter.ai,
                team: critter.team,
                attacker: -1,
                hp: sheet.current_hp,
                radiation: 0,
                poison: 0,
            });
            Some(sheet)
        } else {
            let detail = match &prototype.data {
                PrototypeData::Item(item) => match &item.data {
                    ItemData::Weapon(weapon) => ObjectDetail::Weapon {
                        rounds: weapon.capacity,
                        ammo_pid: weapon.ammo_pid,
                    },
                    ItemData::Ammunition { quantity, .. } => ObjectDetail::Ammunition(*quantity),
                    ItemData::Misc { charges, .. } => ObjectDetail::Charges(*charges),
                    ItemData::Key { code } => ObjectDetail::Key(*code),
                    _ => ObjectDetail::Plain,
                },
                PrototypeData::Scenery(scenery) => match scenery.data {
                    SceneryData::Door { state, .. } => ObjectDetail::Door(state),
                    SceneryData::Stairs { map, destination } => ObjectDetail::Stairs {
                        map,
                        destination: vault_formats::pro::PackedLocation(destination),
                    },
                    SceneryData::Elevator { kind, level } => ObjectDetail::Elevator { kind, level },
                    SceneryData::LadderUp { destination }
                    | SceneryData::LadderDown { destination } => ObjectDetail::Ladder {
                        map: None,
                        destination: vault_formats::pro::PackedLocation(destination),
                    },
                    _ => ObjectDetail::Plain,
                },
                _ => ObjectDetail::Plain,
            };
            raw.state = ObjectState::Other { word: 0, detail };
            None
        };
        let money = (self.rules.money_pid() == prototype.pid).then_some(0);
        let reference = self.insert(WorldObject {
            name: format!("Object {:08X}", prototype.pid).into_bytes(),
            description: Vec::new(),
            raw,
            prototype: Some(prototype),
            character,
            owner: None,
            inventory: Vec::new(),
            money,
            active: true,
            opponent: 0,
        })?;
        Ok(reference)
    }
    pub fn erase(&mut self, reference: ObjectRef) {
        if let Some(owner) = self
            .get(reference)
            .filter(|o| o.raw.flags.contains(BasicFlags::WORN))
            .and_then(|o| o.owner)
        {
            self.armour_effect(owner, reference, false);
        }
        let Some(key) = self.references.remove(&reference) else {
            return;
        };
        self.insertions.remove(&reference);
        if let Some(object) = self.entities.remove(key) {
            if self.unique_ids.get(&object.raw.id) == Some(&reference) {
                self.unique_ids.remove(&object.raw.id);
            }
            if let Some(owner) = object.owner
                && let Some(owner) = self.get_mut(owner)
            {
                owner.inventory.retain(|(r, _)| *r != reference);
            }
            for (child, _) in object.inventory {
                self.erase(child);
            }
            if object.raw.script_id != -1 {
                self.remove_script(object.raw.script_id as u32);
            }
        }
        self.queue.remove_object(reference);
        self.party.remove(&reference);
        // SPEC-005 0x80F4: destruction clears combat membership.
        self.remove_from_combat(reference);
    }
    /// The map with its object table replaced by `render_objects`. This
    /// clones the whole map file; a per-frame caller should draw from
    /// `map` by reference and `render_objects` instead.
    pub fn render_snapshot(&self) -> MapFile {
        let mut map = self.map.clone();
        let consumed_len = map.objects.consumed_len;
        map.objects = self.render_objects();
        map.objects.consumed_len = consumed_len;
        map
    }
    /// The object table to draw: the active, unowned world objects on the
    /// three elevations plus the records the attack shows and explosions
    /// present, without cloning the map's tiles, scripts or variables.
    /// Each elevation is in SPEC-014 §3.44 insertion order, oldest first.
    pub fn render_objects(&self) -> MapObjects {
        let mut objects = MapObjects {
            total: 0,
            elevations: std::array::from_fn(|_| Vec::new()),
            consumed_len: 0,
        };
        // SPEC-007 §3.10: an object an attack presentation owns is drawn as
        // the earliest one still to finish draws it. Owners only the
        // presentation knows (a created projectile, explosion art, an
        // erased grenade) are drawn from their displays alone.
        use crate::choreography::{Display, Owner};
        let mut owned: BTreeMap<Owner, &Display> = BTreeMap::new();
        for (owner, display) in self.shows.iter().flat_map(|show| show.displays()) {
            owned.entry(*owner).or_insert(display);
        }
        // SPEC-014 §3.44: each elevation lists its objects by their latest
        // insertion. A display that has stepped since its show began keeps
        // its own; records only an explosion draws come after the others.
        let mut drawn: Vec<(u64, MapObject)> = Vec::new();
        let mut push = |stamp: u64, record: MapObject| {
            if record.elevation < 3 {
                drawn.push((stamp, record));
            }
        };
        for (reference, object) in self.iter() {
            if object.owner.is_some() || !object.active || object.raw.elevation >= 3 {
                continue;
            }
            let stamp = self.insertions.get(&reference).copied().unwrap_or(0);
            match owned.remove(&Owner::World(reference)) {
                Some(display) => {
                    if let Some(record) = displayed(object.raw.clone(), display) {
                        push(display.inserted.unwrap_or(stamp), record);
                    }
                }
                None => push(stamp, object.raw.clone()),
            }
        }
        for display in owned.into_values() {
            let record = object_record(display.pid, EXPLOSION_ART_ID);
            if let Some(record) = displayed(record, display) {
                push(display.inserted.unwrap_or(0), record);
            }
        }
        // SPEC-013 §3.10 the timed or scripted explosion's art playing now.
        for record in self
            .explosion
            .iter()
            .flat_map(|explosion| explosion.art.records())
        {
            push(u64::MAX, record);
        }
        // A stable sort keeps equal stamps in drawing order.
        drawn.sort_by_key(|(stamp, _)| *stamp);
        for (_, record) in drawn {
            objects.elevations[record.elevation as usize].push(record);
            objects.total += 1;
        }
        objects
    }
    /// SPEC-014 §3.29: a view center outside the map border is refused and
    /// leaves the view where it was. Returns whether the view moved there.
    pub fn set_view_center(&mut self, hex: i32) -> bool {
        let inside = crate::ui::cursor::inside_border(hex);
        if inside {
            self.camera_hex = hex;
        }
        inside
    }
    pub fn script_for(&self, object: ObjectRef) -> Option<u32> {
        let sid = self.get(object)?.raw.script_id;
        (sid != -1 && self.scripts.contains_key(&(sid as u32))).then_some(sid as u32)
    }
    pub fn add_script(
        &mut self,
        owner: ObjectRef,
        file: i32,
        category: u8,
        locals: usize,
    ) -> Result<u32, WorldError> {
        if self.scripts.len() >= 100_000 || category > 4 || locals > 1_000_000 {
            return Err(WorldError::Invalid("script limit/domain".into()));
        }
        let id = (1..0x1000000)
            .map(|n| ((category as u32) << 24) | n)
            .find(|id| !self.scripts.contains_key(id))
            .ok_or(WorldError::Arithmetic)?;
        self.scripts.insert(
            id,
            ScriptInstance {
                id,
                owner,
                file,
                flags: 0,
                local_base: -1,
                local_count: locals,
                spatial: None,
                initialized: false,
                program: 0,
                source: 0,
                target: owner,
                event: 0,
                fixed: 0,
                action: -1,
                result: 0,
                overridden: false,
                margin: 0,
            },
        );
        self.append_script(id);
        if let Some(object) = self.get_mut(owner) {
            object.raw.script_id = id as i32;
            object.raw.cached_script_file = file;
        }
        Ok(id)
    }
    /// A new record joins the end of its category's stream.
    fn append_script(&mut self, id: u32) {
        let slot = ScriptSlot::live(id);
        let position = self
            .script_order
            .iter()
            .position(|other| other.stream > slot.stream)
            .unwrap_or(self.script_order.len());
        self.script_order.insert(position, slot);
    }
    /// Remove the record an identifier finds (SPEC-006 §3.7): its stream's
    /// last record moves into the freed slot. A later record of the same
    /// identifier in that stream is found from then on.
    pub fn remove_script(&mut self, id: u32) {
        self.scripts.remove(&id);
        let Some(at) = self
            .script_order
            .iter()
            .position(|slot| slot.id == id && slot.inert.is_none())
        else {
            return;
        };
        let stream = self.script_order[at].stream;
        let last = self
            .script_order
            .iter()
            .rposition(|slot| slot.stream == stream)
            .unwrap_or(at);
        self.script_order.swap(at, last);
        self.script_order.remove(last);
        let promoted = self.script_order.iter_mut().find(|slot| {
            slot.id == id && u32::from(slot.stream) == id >> 24 && slot.inert.is_some()
        });
        if let Some(slot) = promoted
            && let Some(record) = slot.inert.take()
        {
            let locals = usize::try_from(record.local_count).unwrap_or(0);
            self.scripts.insert(id, script_instance(&record, locals));
        }
    }
    /// The identifiers of the script records, in stored list order.
    pub fn script_ids(&self) -> Vec<u32> {
        self.script_order.iter().map(|slot| slot.id).collect()
    }
    pub fn ensure_locals(&mut self, sid: u32) -> Result<usize, WorldError> {
        let script = self
            .scripts
            .get_mut(&sid)
            .ok_or_else(|| WorldError::Invalid("missing script context".into()))?;
        if script.local_base < 0 {
            let end = self
                .lvars
                .len()
                .checked_add(script.local_count)
                .filter(|end| *end <= 1_000_000)
                .ok_or(WorldError::Arithmetic)?;
            // Allocation failure leaves the pool unchanged (§3.2 safe rule).
            self.lvars
                .try_reserve(script.local_count)
                .map_err(|_| WorldError::Invalid("LVAR pool allocation failed".into()))?;
            script.local_base = self.lvars.len() as i32;
            self.lvars.resize(end, 0);
        }
        Ok(script.local_base as usize)
    }
    /// The pool position of local `index`. Under the SPEC-003 §3.2 safe
    /// rule an index outside the script's own slice is out of range, rather
    /// than reaching another script's slice.
    fn local_slot(&mut self, sid: u32, index: i32) -> Result<Option<usize>, WorldError> {
        let base = self.ensure_locals(sid)?;
        let count = self
            .scripts
            .get(&sid)
            .map_or(0, |script| script.local_count);
        Ok(usize::try_from(index)
            .ok()
            .filter(|index| *index < count)
            .map(|index| base + index)
            .filter(|at| *at < self.lvars.len()))
    }
    /// System/map scripts read -1; an unallocatable slice reads -1 with a
    /// diagnostic, and an index outside the own slice reads 0.
    pub fn local(&mut self, sid: Option<u32>, index: i32) -> i32 {
        let Some(sid) = sid.filter(|sid| sid >> 24 != 0) else {
            return -1;
        };
        match self.local_slot(sid, index) {
            Ok(Some(at)) => self.lvars[at],
            Ok(None) => {
                self.diagnostic(format!("LVAR read {index} outside the script's slice"));
                0
            }
            Err(error) => {
                self.diagnostic(format!("LVAR read {index}: {error}"));
                -1
            }
        }
    }
    /// Writes outside the own slice are ignored; under the SPEC-003 §3.2 safe
    /// rule system/map scripts are denied local writes as well as reads.
    pub fn set_local(&mut self, sid: Option<u32>, index: i32, value: i32) -> bool {
        let Some(sid) = sid.filter(|sid| sid >> 24 != 0) else {
            return false;
        };
        match self.local_slot(sid, index) {
            Ok(Some(at)) => {
                self.lvars[at] = value;
                true
            }
            _ => false,
        }
    }
    pub fn request(&mut self, request: ScriptRequest) {
        if self.requests.len() < 100_000 {
            self.requests.push_back(request);
        }
    }
    pub fn broadcast(&mut self, event: ScriptEvent, fixed: i32) {
        if let Some(script) = self.map_script {
            self.request(ScriptRequest {
                script,
                event,
                source: 0,
                target: 0,
                fixed,
                action: -1,
                only_present: false,
            });
        }
        for script in self.script_ids() {
            if Some(script) != self.map_script {
                self.request(ScriptRequest {
                    script,
                    event,
                    source: 0,
                    target: 0,
                    fixed,
                    action: -1,
                    only_present: true,
                });
            }
        }
    }
    pub fn move_object(
        &mut self,
        reference: ObjectRef,
        hex: i32,
        elevation: u8,
        animated: bool,
    ) -> Result<(), WorldError> {
        if !(0..40000).contains(&hex) || elevation > 2 {
            return Err(WorldError::Invalid("invalid movement destination".into()));
        }
        let object = self
            .get_mut(reference)
            .ok_or_else(|| WorldError::Invalid("invalid object reference".into()))?;
        let previous = object.raw.hex;
        let previous_elevation = object.raw.elevation;
        let changed_elevation = previous_elevation != elevation;
        if changed_elevation {
            object.opponent = 0;
            if let ObjectState::Critter(state) = &mut object.raw.state {
                state.maneuvers = 0;
            }
        }
        object.raw.hex = Some(hex);
        object.raw.elevation = elevation;
        // SPEC-014 §3.44: a move to a hex always reinserts the object, even
        // onto the hex it is on, and resets its pixel offset.
        object.raw.displacement = [0; 2];
        self.reinsert(reference);
        // SPEC-005 0x80B6/0x80FF: changing a critter's elevation removes its
        // combat membership.
        if changed_elevation {
            self.remove_from_combat(reference);
        }
        if reference == self.player {
            self.set_view_center(hex);
            self.set_current_elevation(elevation);
            self.mark_visited(hex);
            self.check_exit_grids(hex, elevation);
            // SPEC-014 §3.36: every placement of the player runs the roof
            // rule after that hex's exit grids are checked.
            self.place_player_roof(hex, elevation);
        }
        // P-02: the light map is rebuilt only when this move can change it.
        if self.move_changes_lighting(reference, previous, previous_elevation, hex, elevation) {
            self.rebuild_lighting();
        }
        if animated && previous != Some(hex) {
            self.check_spatials(reference);
        }
        // SPEC-013 §3.22: the player's elevation change, by placement or
        // by script (`move_to`, `override_map_start`), re-syncs the party.
        if reference == self.player && changed_elevation {
            self.place_party()?;
        }
        Ok(())
    }
    /// SPEC-013 §3.22 party following, by the placement rule with starting
    /// radius 2: each visible party member other than the player, placed
    /// on the map, fills the next free neighbor of the player's hex in NE
    /// through NW order. Run on an elevation change and after the final
    /// player placement of a map change (SPEC-006 §3.5 step 7).
    pub(crate) fn place_party(&mut self) -> Result<(), WorldError> {
        let player = self.player;
        let Some((hex, elevation)) = self
            .get(player)
            .and_then(|o| Some((o.raw.hex?, o.raw.elevation)))
        else {
            return Ok(());
        };
        let members: Vec<_> = self
            .party
            .iter()
            .copied()
            .filter(|member| *member != player)
            .filter(|member| {
                self.get(*member)
                    .is_some_and(|o| o.owner.is_none() && !o.raw.flags.contains(BasicFlags::HIDDEN))
            })
            .collect();
        // SPEC-019 §3.14: under Fallout 2 a hidden entry (the car trunk)
        // moves to the player's hex and elevation without the placement
        // rule, keeping its hidden flag.
        if self.fallout2.is_some() {
            let hidden: Vec<_> = self
                .party
                .iter()
                .copied()
                .filter(|member| *member != player)
                .filter(|member| {
                    self.get(*member).is_some_and(|o| {
                        o.owner.is_none() && o.raw.flags.contains(BasicFlags::HIDDEN)
                    })
                })
                .collect();
            for member in hidden {
                self.move_object(member, hex, elevation, false)?;
            }
        }
        let mut neighbors =
            (0..6).filter_map(|direction| vault_formats::HexGrid::step(hex, direction, 1).ok());
        for member in members {
            let Some(spot) = neighbors
                .by_ref()
                .find(|spot| self.movement_blocker(*spot, elevation, member).is_none())
            else {
                break;
            };
            self.move_object(member, spot, elevation, false)?;
        }
        Ok(())
    }
    /// SPEC-013 §3.22: the player moved onto `hex`: every exit grid there
    /// requests a transition, the last one winning; a request in combat
    /// ends combat.
    pub(crate) fn check_exit_grids(&mut self, hex: i32, elevation: u8) {
        let grid = self
            .iter()
            .filter(|(_, object)| {
                crate::skills::is_exit_grid(object.raw.pid)
                    && object.raw.hex == Some(hex)
                    && object.raw.elevation == elevation
            })
            .filter_map(|(_, object)| match object.raw.state {
                ObjectState::Other {
                    detail:
                        vault_formats::map::ObjectDetail::ExitGrid {
                            map,
                            hex,
                            elevation,
                            orientation,
                        },
                    ..
                } => Some(Transition {
                    map,
                    elevation,
                    hex,
                    facing: orientation,
                }),
                _ => None,
            })
            .last();
        if let Some(transition) = grid {
            if self.combat {
                self.combat_state.flags |= crate::critical::state::END_REQUESTED;
            }
            self.pending_transition = Some(transition);
        }
    }
    /// SPEC-006 §3.5: a later change of the current elevation requests a map
    /// update when a current map script exists. (Map loading assigns the entry
    /// elevation directly, before any map script, and so takes no update path.)
    pub fn set_current_elevation(&mut self, elevation: u8) {
        let changed = self.elevation != elevation;
        self.elevation = elevation;
        // SPEC-014 §3.46 safe rule: an elevation change removes every
        // floating text.
        if changed {
            self.floating.clear();
        }
        if changed && self.map_script.is_some() {
            self.broadcast(ScriptEvent::MapUpdate, 0);
        }
    }
    pub fn check_spatials(&mut self, mover: ObjectRef) {
        if !self.spatial_enabled || self.cursor_objects.contains(&mover) {
            return;
        }
        let Some(object) = self.get(mover) else {
            return;
        };
        let Some(hex) = object.raw.hex.filter(|hex| *hex >= 10) else {
            return;
        };
        if object.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::FLAT.0) != 0 {
            return;
        }
        let elevation = object.raw.elevation;
        self.spatial_enabled = false;
        // SPEC-003 §3.3: an inert record's own location and radius request
        // the procedure of the record its identifier finds.
        let triggered = |flags: u32, spatial: Option<(i32, u8, i32)>| {
            flags & 2 == 0
                && spatial.is_some_and(|(center, layer, radius)| {
                    layer == elevation
                        && (hex == center
                            || (radius != 0
                                && vault_formats::HexGrid::lattice_distance(hex, center)
                                    .is_ok_and(|distance| distance <= radius)))
                })
        };
        let ids: Vec<u32> = self
            .script_order
            .iter()
            .filter(|slot| match &slot.inert {
                Some(record) => triggered(record.flags.0, script_instance(record, 0).spatial),
                None => self
                    .scripts
                    .get(&slot.id)
                    .is_some_and(|script| triggered(script.flags, script.spatial)),
            })
            .map(|slot| slot.id)
            .collect();
        for id in ids {
            self.request(ScriptRequest {
                script: id,
                event: ScriptEvent::Spatial,
                source: mover,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            });
        }
        self.spatial_enabled = true;
    }
    pub fn ambient_percent(&mut self, percent: i32) {
        let requested = if percent == 50 {
            40960
        } else {
            (if percent > 50 { 40960 } else { 16384 }) + i64::from(percent) * 24576 / 100
        };
        self.set_ambient(requested);
    }
    pub fn set_ambient(&mut self, value: i64) {
        self.ambient =
            (value + 6553 * i64::from(self.perk(Perk::NightVision))).clamp(16384, 65536) as i32;
    }
    pub fn object_light(
        &mut self,
        reference: ObjectRef,
        percent: i32,
        radius: i32,
    ) -> Result<(), WorldError> {
        let request = if percent == 0 {
            0
        } else {
            i64::from(percent) * 65636 / 100
        };
        let requested = i32::try_from(request).map_err(|_| WorldError::Arithmetic)?;
        self.emit(WorldEffect::ObjectLight {
            object: reference,
            requested,
            radius,
        });
        if radius < 0 || requested < 0 {
            return Err(WorldError::Invalid("negative object light".into()));
        }
        let object = self
            .get_mut(reference)
            .ok_or_else(|| WorldError::Invalid("missing light owner".into()))?;
        object.raw.light = Light {
            radius: radius.min(8),
            intensity: requested.min(65536),
        };
        self.rebuild_lighting();
        Ok(())
    }
    /// SPEC-007 §3.9 setting an object's light: the radius is clamped to
    /// at most 8 at the given intensity, and an intensity of 0 or less sets
    /// both to 0.
    pub fn set_light(&mut self, reference: ObjectRef, radius: i32, intensity: i32) {
        if let Some(object) = self.get_mut(reference) {
            object.raw.light = if intensity > 0 {
                Light {
                    radius: radius.min(8),
                    intensity,
                }
            } else {
                Light {
                    radius: 0,
                    intensity: 0,
                }
            };
        }
        self.rebuild_lighting();
    }
    /// The lattice distance beyond a light's radius within which a line
    /// from the source can still pass: `hex_line` rounds each point to the
    /// nearest hex, so an interior tile can lie one hex beyond the distance
    /// of the line's end (`lighting_tests` checks this bound).
    pub(crate) const LIGHT_LINE_SLACK: i32 = 1;
    /// P-02: whether moving `reference` from `from` on `from_elevation` to
    /// `hex` on `elevation` can change the light map, so `move_object`
    /// rebuilds it only then. `rebuild_lighting` reads only the sources
    /// (active, not hidden, lit) and the blockers (not hidden, not
    /// light-through): a hidden mover, or a light-through one without
    /// light, changes neither; a source always does; a blocker changes the
    /// map only where it vacates a hex no other blocker holds, or enters a
    /// hex none held, within reach of a source on that elevation.
    pub(crate) fn move_changes_lighting(
        &self,
        reference: ObjectRef,
        from: Option<i32>,
        from_elevation: u8,
        hex: i32,
        elevation: u8,
    ) -> bool {
        if from == Some(hex) && from_elevation == elevation {
            return false;
        }
        let Some(object) = self.get(reference) else {
            return true;
        };
        if object.raw.flags.contains(BasicFlags::HIDDEN) {
            return false;
        }
        if object.active && object.raw.light.intensity > 0 {
            return true;
        }
        if object.raw.flags.contains(BasicFlags::LIGHT_THROUGH) {
            return false;
        }
        let mut changed = [
            from.map(|from| (from_elevation, from)),
            Some((elevation, hex)),
        ];
        for (other, object) in self.iter() {
            if other == reference
                || object.raw.flags.contains(BasicFlags::HIDDEN)
                || object.raw.flags.contains(BasicFlags::LIGHT_THROUGH)
            {
                continue;
            }
            let Some(held) = object.raw.hex else {
                continue;
            };
            for slot in &mut changed {
                if *slot == Some((object.raw.elevation, held)) {
                    *slot = None;
                }
            }
        }
        let changed: Vec<_> = changed.into_iter().flatten().collect();
        !changed.is_empty()
            && self.iter().any(|(_, object)| {
                object.active
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && object.raw.light.intensity > 0
                    && object.raw.elevation < 3
                    && object.raw.hex.is_some_and(|center| {
                        let reach = object.raw.light.radius.clamp(0, 8) + Self::LIGHT_LINE_SLACK;
                        changed.iter().any(|(layer, hex)| {
                            *layer == object.raw.elevation
                                && vault_formats::HexGrid::lattice_distance(center, *hex)
                                    .is_ok_and(|distance| distance <= reach)
                        })
                    })
            })
    }
    pub fn rebuild_lighting(&mut self) {
        for layer in &mut self.tile_light {
            layer.fill(655);
        }
        let blockers: BTreeSet<_> = self
            .iter()
            .filter_map(|(_, object)| {
                (!object.raw.flags.contains(BasicFlags::HIDDEN)
                    && !object.raw.flags.contains(BasicFlags::LIGHT_THROUGH))
                .then_some((object.raw.elevation, object.raw.hex?))
            })
            .collect();
        let sources: Vec<_> = self
            .iter()
            .filter_map(|(_, object)| {
                (object.active
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && object.raw.light.intensity > 0
                    && object.raw.elevation < 3)
                    .then_some((object.raw.hex?, object.raw.elevation, object.raw.light))
            })
            .collect();
        for (center, layer, light) in sources {
            let radius = light.radius.clamp(0, 8);
            let intensity = light.intensity.clamp(0, 65536);
            let mut visited = BTreeSet::new();
            let mut todo = VecDeque::from([(center, 0)]);
            while let Some((hex, distance)) = todo.pop_front() {
                if !visited.insert(hex) || !(0..40000).contains(&hex) {
                    continue;
                }
                let line = hex_line(center, hex);
                if line
                    .iter()
                    .skip(1)
                    .take(line.len().saturating_sub(2))
                    .all(|tile| !blockers.contains(&(layer, *tile)))
                {
                    let value =
                        i64::from(intensity - distance * ((intensity - 655) / (radius + 1)));
                    self.tile_light[layer as usize][hex as usize] += value;
                }
                if distance < radius {
                    for direction in 0..6 {
                        if let Ok(Some(next)) = vault_formats::HexGrid::neighbor(hex, direction) {
                            todo.push_back((next, distance + 1));
                        }
                    }
                }
            }
        }
    }
    pub fn visible_light(&self, reference: ObjectRef) -> i32 {
        let Some(object) = self.get(reference) else {
            return self.ambient;
        };
        let mut local = object
            .raw
            .hex
            .and_then(|hex| {
                self.tile_light
                    .get(object.raw.elevation as usize)?
                    .get(hex as usize)
            })
            .copied()
            .unwrap_or(0);
        if reference == self.player {
            local -= i64::from(object.raw.light.intensity);
        }
        local.max(i64::from(self.ambient)).clamp(0, 65536) as i32
    }
    pub fn distance(&self, a: ObjectRef, b: ObjectRef) -> i32 {
        match (self.get(a), self.get(b)) {
            (Some(a), Some(b)) if a.raw.elevation == b.raw.elevation => {
                vault_formats::HexGrid::angular_distance(
                    a.raw.hex.unwrap_or(-1),
                    b.raw.hex.unwrap_or(-1),
                )
                .unwrap_or(9999)
            }
            _ => 9999,
        }
    }
    pub fn inventory_count(&self, owner: ObjectRef, pid: u32) -> i32 {
        let Some(object) = self.get(owner) else {
            return 0;
        };
        object
            .inventory
            .iter()
            .fold(0_i32, |sum, (reference, quantity)| {
                let Some(item) = self.get(*reference) else {
                    return sum;
                };
                sum.saturating_add(if item.raw.pid == pid { *quantity } else { 0 })
                    .saturating_add(self.inventory_count(*reference, pid))
            })
    }
    pub fn carried(&self, owner: ObjectRef, pid: u32) -> Option<ObjectRef> {
        for (item, _) in &self.get(owner)?.inventory {
            if self.get(*item)?.raw.pid == pid {
                return Some(*item);
            }
            if let Some(found) = self.carried(*item, pid) {
                return Some(found);
            }
        }
        None
    }
    /// SPEC-011 §3.10 armor contribution, for the player only: AC, DR and
    /// DT added (or removed) unclamped, then the armor perk's effect. NPC
    /// protection never comes from worn armor.
    pub(crate) fn armour_effect(&mut self, owner: ObjectRef, item: ObjectRef, apply: bool) {
        if owner != self.player {
            return;
        }
        let Some(armour) = self.item_data(item).and_then(|data| match &data.data {
            ItemData::Armor(armour) => Some(armour.clone()),
            _ => None,
        }) else {
            return;
        };
        let sign = if apply { 1 } else { -1 };
        self.adjust_bonus(owner, 9, armour.armor_class.saturating_mul(sign));
        for index in 0..7 {
            self.adjust_bonus(
                owner,
                24 + index,
                armour.resistances[index].saturating_mul(sign),
            );
            self.adjust_bonus(
                owner,
                17 + index,
                armour.thresholds[index].saturating_mul(sign),
            );
        }
        self.perk_effect(owner, armour.perk, apply);
    }
    /// Add `delta` to a bonus-array stat. The player's array is its own;
    /// an NPC's lives on its prototype (SPEC-011 §3.2), so the change
    /// applies to every critter sharing the PID, including ones created
    /// later in this session.
    pub fn adjust_bonus(&mut self, critter: ObjectRef, index: usize, delta: i32) {
        let context = self.context(critter);
        let player = self.player;
        let Some(object) = self.get_mut(critter) else {
            return;
        };
        let pid = object.raw.pid;
        let Some(sheet) = object.character.as_mut() else {
            return;
        };
        sheet.adjust_bonus(index, delta, context);
        if critter == player {
            return;
        }
        let shared = (sheet.base, sheet.bonus);
        self.prototype_stats.insert(pid, shared);
        for (reference, key) in &self.references {
            if *reference == critter || *reference == player {
                continue;
            }
            if let Some(object) = self.entities.get_mut(*key)
                && object.raw.pid == pid
                && let Some(sheet) = object.character.as_mut()
            {
                (sheet.base, sheet.bonus) = shared;
            }
        }
    }
    /// SPEC-011 §3.12 perk effect (rank, armor, or withdrawal) on a
    /// critter's bonus array; an index outside 0-63 does nothing.
    pub fn perk_effect(&mut self, critter: ObjectRef, perk: i32, apply: bool) {
        let Some(perk) = Perk::from_index(perk) else {
            return;
        };
        for (stat, amount) in crate::progression::perk_changes(perk) {
            self.adjust_bonus(critter, stat, if apply { amount } else { -amount });
        }
    }
    pub fn equip(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        flag: u32,
    ) -> Result<(), WorldError> {
        if self.get(item).is_none_or(|o| o.owner != Some(owner)) {
            return Err(WorldError::Invalid(
                "equipment must be in the critter's inventory".into(),
            ));
        }
        let previous: Vec<_> = self
            .get(owner)
            .ok_or_else(|| WorldError::Invalid("equipment owner missing".into()))?
            .inventory
            .iter()
            .filter(|(reference, _)| {
                self.get(*reference)
                    .is_some_and(|o| o.raw.flags.0 & flag != 0)
            })
            .map(|(r, _)| *r)
            .collect();
        if previous.contains(&item) {
            return Ok(());
        }
        for old in previous {
            if flag == BasicFlags::WORN.0 {
                self.armour_effect(owner, old, false);
            }
            if let Some(object) = self.get_mut(old) {
                object.raw.flags.0 &= !flag;
            }
        }
        self.get_mut(item).expect("item").raw.flags.0 |= flag;
        if flag == BasicFlags::WORN.0 {
            self.armour_effect(owner, item, true);
        }
        Ok(())
    }
    pub fn enqueue_map_timers(&mut self) -> Result<(), WorldError> {
        self.queue.insert(
            self.clock.ticks,
            self.clock.midnight_delay(),
            0,
            EventKind::Midnight,
        )?;
        if self.map.header.name[0] != 0 {
            self.queue
                .insert(self.clock.ticks, 600, 0, EventKind::MapUpdate)?;
        }
        Ok(())
    }
    /// SPEC-006 queue clearing on map exit. Drug-stage and withdrawal
    /// events of party members (the player always is one) stay queued;
    /// those of other owners are removed and returned in queue order for
    /// the SPEC-011 §3.9 exit handlers.
    pub fn map_exit_cleanup(
        &mut self,
        mut approve: impl FnMut(&TimedEvent) -> bool,
    ) -> Vec<TimedEvent> {
        self.trace.push(WorldTrace::MapExitCleanup);
        let (player, party) = (self.player, self.party.clone());
        let mut leaving = Vec::new();
        self.queue.retain(|event| match event.kind {
            EventKind::Poison | EventKind::Radiation { .. } => true,
            EventKind::Script { .. } | EventKind::MapUpdate | EventKind::Midnight => false,
            EventKind::Drug { .. } | EventKind::Withdrawal(_) => {
                let kept = event.object == player || party.contains(&event.object);
                if !kept {
                    leaving.push(event.clone());
                }
                kept
            }
            EventKind::Knockout | EventKind::Other(_) => !approve(event),
        });
        leaving
    }
}

/// Deterministic integer cube interpolation used by the host's line/blocker policy.
pub fn hex_line(from: i32, to: i32) -> Vec<i32> {
    let (Ok([ac, ar]), Ok([bc, br])) = (
        vault_formats::HexGrid::coordinates(from),
        vault_formats::HexGrid::coordinates(to),
    ) else {
        return Vec::new();
    };
    let (aq, aqr) = (i64::from(ac), i64::from(ar - (ac + 1) / 2));
    let (bq, bqr) = (i64::from(bc), i64::from(br - (bc + 1) / 2));
    let n = i64::from(vault_formats::HexGrid::lattice_distance(from, to).unwrap_or(0));
    if n == 0 {
        return vec![from];
    }
    let mut result = Vec::new();
    for step in 0..=n {
        let numerators = [
            aq * (n - step) + bq * step,
            aqr * (n - step) + bqr * step,
            (-aq - aqr) * (n - step) + (-bq - bqr) * step,
        ];
        let mut rounded = numerators.map(|value| (value + n / 2).div_euclid(n));
        let errors = std::array::from_fn::<_, 3, _>(|i| (rounded[i] * n - numerators[i]).abs());
        if errors[0] >= errors[1] && errors[0] >= errors[2] {
            rounded[0] = -rounded[1] - rounded[2];
        } else if errors[1] >= errors[2] {
            rounded[1] = -rounded[0] - rounded[2];
        }
        let c = rounded[0];
        let r = rounded[1] + (c + 1).div_euclid(2);
        if (0..200).contains(&c) && (0..200).contains(&r) {
            result.push((c + 200 * r) as i32);
        }
    }
    result
}

#[cfg(test)]
mod draw_order_tests {
    use super::*;

    /// SPEC-014 Fixture Q: a walk step or a hex change moves the object to
    /// the end of its hex's group; an art change alone does not.
    #[test]
    fn spec014_fixture_q_reinsertion_order() {
        let mut world = WorldState::empty(1).unwrap();
        let mut add = |id: u32, pid: u32, hex: i32| {
            let mut raw = object_record(pid, id);
            raw.hex = Some(hex);
            world
                .insert(WorldObject {
                    name: Vec::new(),
                    description: Vec::new(),
                    raw,
                    prototype: None,
                    character: None,
                    owner: None,
                    inventory: Vec::new(),
                    money: None,
                    active: true,
                    opponent: 0,
                })
                .unwrap()
        };
        let a = add(1, 0x0000_0001, 20100);
        add(2, 0x0000_0001, 20100);
        let c = add(3, 0x0100_0001, 20100);
        add(4, 0x0200_0001, 20100);
        let ids = |world: &WorldState| -> Vec<u32> {
            world.render_objects().elevations[0]
                .iter()
                .map(|object| object.id)
                .collect()
        };
        assert_eq!(ids(&world), [1, 2, 3, 4]);
        // Art, frame, light, hiding and outline changes do not reinsert.
        world.get_mut(c).unwrap().raw.art_id = 7;
        world.get_mut(a).unwrap().raw.frame = 2;
        world.get_mut(a).unwrap().raw.outline = 1;
        world.set_light(c, 4, 65536);
        world.set_hidden(c, true);
        world.set_hidden(c, false);
        assert_eq!(ids(&world), [1, 2, 3, 4]);
        // A walk step's new pixel offset does.
        world.set_offset(c, [4, -2]);
        assert_eq!(ids(&world), [1, 2, 4, 3]);
        world.set_flat(a, true);
        assert_eq!(ids(&world), [2, 4, 3, 1]);
        // A move to another hex, or the same one, reinserts and resets the
        // pixel offset.
        world.move_object(c, 20101, 0, false).unwrap();
        assert_eq!(ids(&world), [2, 4, 1, 3]);
        assert_eq!(world.get(c).unwrap().raw.displacement, [0; 2]);
    }

    /// SPEC-014 Fixture T per-hex order: a zero-shift frame step, a step
    /// of the other object, then a move to the hex the object is on.
    #[test]
    fn spec014_fixture_t_every_step_and_move_reinserts() {
        let mut world = WorldState::empty(1).unwrap();
        let mut add = |id: u32| {
            let mut raw = object_record(0x0100_0001, id);
            raw.hex = Some(20100);
            world
                .insert(WorldObject {
                    name: Vec::new(),
                    description: Vec::new(),
                    raw,
                    prototype: None,
                    character: None,
                    owner: None,
                    inventory: Vec::new(),
                    money: None,
                    active: true,
                    opponent: 0,
                })
                .unwrap()
        };
        let a = add(1);
        let b = add(2);
        let ids = |world: &WorldState| -> Vec<u32> {
            world.render_objects().elevations[0]
                .iter()
                .map(|object| object.id)
                .collect()
        };
        world.shift_offset(b, [0, 0]);
        assert_eq!(ids(&world), [1, 2]);
        world.shift_offset(a, [0, 0]);
        assert_eq!(ids(&world), [2, 1]);
        world.set_offset(b, [3, 1]);
        world.move_object(b, 20100, 0, false).unwrap();
        assert_eq!(ids(&world), [1, 2]);
        assert_eq!(world.get(b).unwrap().raw.displacement, [0; 2]);
        // An offset change to (0, 0) from (0, 0) still reinserts.
        world.set_offset(a, [0, 0]);
        assert_eq!(ids(&world), [2, 1]);
    }
}
