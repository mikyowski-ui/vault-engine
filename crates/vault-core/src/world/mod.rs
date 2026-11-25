use crate::{
    CharacterSheet, EntityId, EntityStore, ParkMiller, Perk, SequenceIds, WorldError,
    dialogue::{GDialog, Reputation},
    queue::{EventKind, TimedEvent, TimedQueue, WorldClock},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vault_formats::{
    MapFile,
    map::{CritterState, MapHeader, MapObject, MapObjects, ObjectState, ScriptCategory},
    pro::{MapFlags, Prototype},
};

mod objects;
mod queries;
mod scripts;
mod spatial;

pub(crate) use scripts::CarriedScript;

pub use objects::{ObjectAction, WorldObject, next_insertion, object_record};
pub use scripts::{
    PARTY_SCRIPT_BASE, PLAYER_SCRIPT_FILE, ScriptEvent, ScriptInstance, ScriptRequest, ScriptSlot,
    party_marks,
};
pub use spatial::hex_line;

pub type ObjectRef = u32;

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
    /// Each victim in damage order, with its damage and knockback: the
    /// critter blocking the center hex, or a timed explosive's whole blast
    /// radius.
    pub victims: Vec<(ObjectRef, i32, i32)>,
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
    /// How the player's placements move `camera_hex`; the shell sets it.
    pub view_follow: crate::camera::ViewFollow,
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
    /// SPEC-024b: sfall mode's world state; `None` with the mode off. It is
    /// outside `state_hash`, which stays `VSH1`.
    pub sfall: Option<Box<crate::sfall::SfallWorld>>,
    /// SPEC-024b §4.12: what a script set for this session. It is neither
    /// saved nor hashed.
    pub session: crate::sfall::SessionSettings,
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
    /// SPEC-017 §5.1 rule 49: the explosive being resolved, from its event
    /// firing until its explosion ends. It outlives the object itself.
    pub resolving: Option<ObjectRef>,
    /// SPEC-017 §5.1 rule 49: the object whose notice `damage_p_proc` runs.
    pub notified: Option<ObjectRef>,
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
    /// Bounded: every push goes through `host::push_bounded`.
    pub trace: Vec<WorldTrace>,
    pub elapsed_ms: u64,
    /// SPEC-014 §3.44 each object's latest insertion into its hex list
    /// (a [`next_insertion`] stamp): set when it is inserted and renewed by
    /// [`WorldState::reinsert`].
    insertions: BTreeMap<ObjectRef, u64>,
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
            view_follow: crate::camera::ViewFollow::Step,
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
            sfall: None,
            session: Default::default(),
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
            resolving: None,
            notified: None,
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
        // SPEC-007 §3.3: the combat system's normal initialization (at
        // program start, for a new game and at the reset before a load)
        // leaves the player's AP at the effective maximum. Out of combat
        // that AP is what the attack preflight checks (SPEC-014 §3.38 step
        // 2), so a player left at 0 could never open combat with an attack
        // until some combat had ended. A load then restores its saved AP.
        let maximum = world.maximum_ap(world.player);
        world.set_ap(world.player, maximum);
        world.create_player_script()?;
        Ok(())
    }
    pub fn diagnostic(&mut self, message: impl Into<String>) {
        self.emit(WorldEffect::Diagnostic(message.into()));
    }
    pub fn emit(&mut self, effect: WorldEffect) {
        if self.effects.len() < 100_000 {
            self.effects.push_back(effect);
        }
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
        crate::host::push_bounded(&mut self.trace, WorldTrace::MapExitCleanup);
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
