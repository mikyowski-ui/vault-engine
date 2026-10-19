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
    /// SPEC-007 combat invocations (fixed parameter 4 turn, 5 join check).
    Combat,
    Damage,
    Destroy,
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
            Self::Combat => b"combat_p_proc",
            Self::Damage => b"damage_p_proc",
            Self::Destroy => b"destroy_p_proc",
        }
    }
    pub fn documented_code(self) -> Option<i32> {
        match self {
            Self::Start => Some(1),
            Self::MapEnter => Some(15),
            Self::MapUpdate => Some(23),
            Self::LookAt => Some(21),
            Self::Description => Some(3),
            Self::Use => Some(6),
            Self::UseSkill => Some(8),
            Self::UseObject => Some(7),
            Self::Pickup => Some(4),
            Self::Talk => Some(11),
            _ => None,
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
#[derive(Debug, Clone, Default)]
pub struct WorldRules {
    /// Weapon-perk overrides; SPEC-011 §3.22 numbers 58-61 apply otherwise.
    pub weapon_perks: BTreeMap<i32, WeaponPerk>,
    pub reputation_globals: Option<ReputationGlobals>,
    pub event_codes: BTreeMap<ScriptEvent, i32>,
    /// Money PID override; SPEC-011 §3.16 PID 41 applies otherwise.
    pub currency_pid: Option<u32>,
    pub doctor_bag_pid: Option<u32>,
    pub first_aid_kit_pid: Option<u32>,
    /// Game-calendar year for day zero; the approved specs define the
    /// non-leap month/day calendar but no base year.
    pub calendar_year: u16,
    /// The campaign global holding the remaining water for the SPEC-012
    /// midnight countdown; no approved specification names it.
    pub water_global: Option<usize>,
}
/// SPEC-013 §3.22 a map transition request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// Map number; 0 is the world map and -1 the town map.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveExplosion {
    pub hex: i32,
    pub elevation: u8,
    /// The critter blocking the center hex, its damage and knockback.
    pub victim: Option<(ObjectRef, i32, i32)>,
    /// The credited attacker: the player for a timed explosive, none for a
    /// scripted request.
    pub source: Option<ObjectRef>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEffect {
    Diagnostic(String),
    Message(Vec<u8>),
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
    next_reference: u32,
    pub(crate) next_uid: u32,
    pub player: ObjectRef,
    pub party: BTreeSet<ObjectRef>,
    pub map: MapFile,
    pub gvars: Option<Vec<i32>>,
    pub mvars: Vec<i32>,
    pub lvars: Vec<i32>,
    pub scripts: BTreeMap<u32, ScriptInstance>,
    pub script_order: Vec<u32>,
    pub map_script: Option<u32>,
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
    /// SPEC-009 packets, snapshot, and taunt state.
    pub ai: crate::ai::AiState,
    /// Live floating-text objects: owner and expiry on the host clock.
    pub floating: Vec<(ObjectRef, u64)>,
    /// Pip-Boy credited rest minutes (reset when the Pip-Boy opens).
    pub rest_credit: i32,
    /// SPEC-008 working saved-map set, automap database, and carried
    /// save components.
    pub persistence: crate::persistence::Persistence,
    /// A scripted combat request awaiting the host's next update.
    pub pending_combat: Option<crate::combat_state::CombatRequest>,
    /// SPEC-013 §3.22 the pending map transition (the last request wins).
    pub pending_transition: Option<Transition>,
    /// SPEC-013 §3.9/§3.16 a deferred loot or steal request for the
    /// presentation layer's loot window.
    pub pending_loot: Option<LootRequest>,
    /// SPEC-013 §3.10 the scripted explosion awaiting the request pass.
    pub pending_explosion: Option<ExplosionRequest>,
    /// SPEC-013 §3.10 the animated explosion in progress, if any.
    pub explosion: Option<ActiveExplosion>,
    /// SPEC-014 §3.33 the hexes the player entered since the seen state was
    /// last processed: 40000 bits, 8 hexes per byte in hex order.
    pub visited: Vec<u8>,
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
}
pub fn empty_map() -> MapFile {
    MapFile {
        header: MapHeader {
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
            ai: Default::default(),
            floating: Vec::new(),
            rest_credit: 0,
            persistence: Default::default(),
            pending_combat: None,
            pending_transition: None,
            pending_loot: None,
            pending_explosion: None,
            explosion: None,
            visited: vec![0; crate::persistence::VISITED_BYTES],
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
        })
    }
    pub fn new(player: CharacterSheet, seed: i32) -> Result<Self, WorldError> {
        let mut world = Self::empty(seed)?;
        world.rng.startup_diagnostic();
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
        Ok(world)
    }
    pub fn insert(&mut self, mut object: WorldObject) -> Result<ObjectRef, WorldError> {
        if self.references.len() >= 200_000 {
            return Err(WorldError::Invalid("world object limit exceeded".into()));
        }
        if object.raw.id == 0 || self.iter().any(|(_, other)| other.raw.id == object.raw.id) {
            while self.iter().any(|(_, other)| other.raw.id == self.next_uid) {
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
        let key = self.entities.insert(object);
        self.references.insert(reference, key);
        Ok(reference)
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
        self.scripts.clear();
        self.script_order.clear();
        self.map_script = None;
        self.mvars = map.mvars.clone();
        self.lvars = map.lvars.clone();
        for category in &map.scripts {
            for script in category.active() {
                let spatial = if let ScriptPrefix::Spatial { location, radius } = script.prefix {
                    Some((location.hex() as i32, location.elevation(), radius))
                } else {
                    None
                };
                self.script_order.push(script.id);
                self.scripts.insert(
                    script.id,
                    ScriptInstance {
                        id: script.id,
                        owner: 0,
                        file: script.file_index,
                        flags: script.flags.0,
                        local_base: script.local_base,
                        local_count: if script.local_count > 0 {
                            script.local_count as usize
                        } else {
                            assets.local_count(script.file_index)
                        },
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
                    },
                );
            }
        }
        let roots: Vec<_> = map.objects.elevations.iter().flatten().cloned().collect();
        self.map = map;
        for root in roots {
            self.import_object(root, None, assets)?;
        }
        self.rebuild_lighting();
        Ok(())
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
        self.rebuild_lighting();
        Ok(reference)
    }
    /// A critter's sheet from its prototype, with any stat arrays this
    /// session changed on that prototype.
    fn critter_sheet(&self, pid: u32, critter: &vault_formats::pro::Critter) -> CharacterSheet {
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
                    | SceneryData::LadderDown { destination } => {
                        ObjectDetail::Ladder(vault_formats::pro::PackedLocation(destination))
                    }
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
        if let Some(object) = self.entities.remove(key) {
            if let Some(owner) = object.owner
                && let Some(owner) = self.get_mut(owner)
            {
                owner.inventory.retain(|(r, _)| *r != reference);
            }
            for (child, _) in object.inventory {
                self.erase(child);
            }
            if object.raw.script_id != -1 {
                self.scripts.remove(&(object.raw.script_id as u32));
                self.script_order
                    .retain(|sid| *sid != object.raw.script_id as u32);
            }
        }
        self.queue.remove_object(reference);
        self.party.remove(&reference);
        // SPEC-009: a removed critter is swap-removed from the snapshot, and
        // no last-attacker reference to it is retained. Combat cleanup clears
        // every reference, so only an active combat can hold one.
        if let Some(index) = self.ai.snapshot.iter().position(|e| *e == reference) {
            self.ai.snapshot.swap_remove(index);
        }
        if self.combat {
            for object in self.entities.values_mut() {
                if object.opponent == reference {
                    object.opponent = 0;
                }
            }
        }
    }
    pub fn render_snapshot(&self) -> MapFile {
        let mut map = self.map.clone();
        map.objects.elevations = std::array::from_fn(|_| Vec::new());
        map.objects.total = 0;
        for (_, object) in self.iter() {
            if object.owner.is_none() && object.active && object.raw.elevation < 3 {
                map.objects.elevations[object.raw.elevation as usize].push(object.raw.clone());
                map.objects.total += 1;
            }
        }
        map
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
        let position = self
            .script_order
            .iter()
            .position(|sid| sid >> 24 > u32::from(category))
            .unwrap_or(self.script_order.len());
        self.script_order.insert(position, id);
        if let Some(object) = self.get_mut(owner) {
            object.raw.script_id = id as i32;
            object.raw.cached_script_file = file;
        }
        Ok(id)
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
        for script in self.script_order.clone() {
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
        if object.raw.elevation != elevation {
            object.opponent = 0;
            if let ObjectState::Critter(state) = &mut object.raw.state {
                state.maneuvers = 0;
            }
        }
        object.raw.hex = Some(hex);
        object.raw.elevation = elevation;
        if reference == self.player {
            self.camera_hex = hex;
            self.set_current_elevation(elevation);
            self.mark_visited(hex);
            self.check_exit_grids(hex, elevation);
        }
        self.rebuild_lighting();
        if animated && previous != Some(hex) {
            self.check_spatials(reference);
        }
        Ok(())
    }
    /// SPEC-013 §3.22: the player moved onto `hex`: every exit grid there
    /// requests a transition, the last one winning; a request in combat
    /// ends combat.
    fn check_exit_grids(&mut self, hex: i32, elevation: u8) {
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
        for id in self.script_order.clone() {
            if let Some(script) = self.scripts.get(&id)
                && script.flags & 2 == 0
                && let Some((center, layer, radius)) = script.spatial
                && layer == elevation
                && (hex == center
                    || (radius != 0
                        && vault_formats::HexGrid::lattice_distance(hex, center)
                            .is_ok_and(|distance| distance <= radius)))
            {
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
