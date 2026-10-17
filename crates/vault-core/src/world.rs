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
#[derive(Debug, Clone, Default)]
pub struct WorldRules {
    pub weapon_perks: BTreeMap<i32, WeaponPerk>,
    pub reputation_globals: Option<ReputationGlobals>,
    pub perk_ids: BTreeMap<i32, Perk>,
    pub event_codes: BTreeMap<ScriptEvent, i32>,
    pub xp_thresholds: Vec<i32>,
    pub xp_bonus_percent: i32,
    pub currency_pid: Option<u32>,
    pub doctor_bag_pid: Option<u32>,
    pub first_aid_kit_pid: Option<u32>,
    pub equipment_effects: BTreeMap<u32, crate::stats::Effect>,
    /// Game-calendar year for day zero; the approved specs define the
    /// non-leap month/day calendar but no base year.
    pub calendar_year: u16,
}
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEffect {
    Diagnostic(String),
    Message(Vec<u8>),
    Sound(Vec<u8>),
    Speech(Vec<u8>),
    StopSpeech,
    Movie {
        index: i32,
        flags: u32,
    },
    Fade {
        out: bool,
        duration: i32,
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
    MapTransition {
        index: i32,
        parameter: i32,
    },
    WorldMap,
    TownMap,
    Elevator(i32),
    EndGame,
    Slideshow,
    EndMovie,
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
    Medical {
        actor: ObjectRef,
        target: ObjectRef,
        skill: i32,
        code: i32,
        margin: i32,
        kit_bonus: i32,
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
    /// Pip-Boy credited rest minutes (reset when the Pip-Boy opens).
    pub rest_credit: i32,
    /// SPEC-008 working saved-map set, automap database, and carried
    /// save components.
    pub persistence: crate::persistence::Persistence,
    /// A scripted combat request awaiting the host's next update.
    pub pending_combat: Option<crate::combat_state::CombatRequest>,
    pub combat_quit: bool,
    pub combat_actor: ObjectRef,
    pub ui_disabled: bool,
    pub mode: i32,
    pub difficulty: i32,
    pub combat_difficulty: i32,
    pub running_burning: i32,
    pub violence: Option<i32>,
    pub recording: bool,
    pub debug_messages: bool,
    pub cursor_objects: [ObjectRef; 2],
    pub active_hand: i32,
    pub dialogue: GDialog,
    pub reputation: Reputation,
    pub rules: WorldRules,
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
            rest_credit: 0,
            persistence: Default::default(),
            pending_combat: None,
            combat_quit: false,
            combat_actor: 0,
            ui_disabled: false,
            mode: 0,
            difficulty: 1,
            combat_difficulty: 0,
            running_burning: 1,
            violence: None,
            recording: false,
            debug_messages: false,
            cursor_objects: [0; 2],
            active_hand: 1,
            dialogue: GDialog::default(),
            reputation: Reputation::default(),
            rules: WorldRules::default(),
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
    /// Add player experience with the configured bonus/clamps, advancing
    /// levels through the configured thresholds.
    pub fn give_experience(&mut self, amount: i32) {
        let context = self.context(self.player);
        let bonus = self.rules.xp_bonus_percent;
        let thresholds = self.rules.xp_thresholds.clone();
        if let Some(sheet) = self.player_sheet_mut() {
            let gain = i64::from(amount) * (100 + i64::from(bonus)) / 100;
            sheet.experience =
                (i64::from(sheet.experience) + gain).clamp(0, i64::from(i32::MAX)) as i32;
            while sheet.level < 21
                && thresholds
                    .get(sheet.level as usize)
                    .is_some_and(|threshold| sheet.experience >= *threshold)
            {
                sheet.advance_level(context);
            }
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
            Some(PrototypeData::Critter(critter)) => Some(CharacterSheet::from_critter(critter)),
            _ => None,
        };
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
        let money = (self.rules.currency_pid == Some(raw.pid)).then_some(match raw.state {
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
        let mut raw = object_record(prototype.pid, 0);
        raw.hex = Some(hex);
        raw.elevation = elevation;
        raw.art_id = prototype.art_id;
        // Only persistent presentation/blocking bits propagate from a prototype.
        raw.flags = BasicFlags(prototype.flags.0 & 0xb00fd818);
        raw.light = prototype.light.unwrap_or(Light {
            radius: 0,
            intensity: 0,
        });
        let character = if let PrototypeData::Critter(critter) = &prototype.data {
            let sheet = CharacterSheet::from_critter(critter);
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
        let money = (self.rules.currency_pid == Some(prototype.pid)).then_some(0);
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
        self.rebuild_lighting();
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
            script.local_base = self.lvars.len() as i32;
            self.lvars.resize(end, 0);
        }
        Ok(script.local_base as usize)
    }
    pub fn local(&mut self, sid: Option<u32>, index: i32) -> i32 {
        let Some(sid) = sid.filter(|sid| sid >> 24 != 0) else {
            return -1;
        };
        let Ok(base) = self.ensure_locals(sid) else {
            return -1;
        };
        let at = base as i64 + i64::from(index);
        let value = usize::try_from(at)
            .ok()
            .and_then(|at| self.lvars.get(at))
            .copied();
        if value.is_none() {
            // SPEC-003: out-of-pool reads report an error and return zero.
            self.diagnostic(format!("LVAR read {index} outside the local pool"));
        }
        value.unwrap_or(0)
    }
    pub fn set_local(&mut self, sid: Option<u32>, index: i32, value: i32) -> bool {
        let Some(sid) = sid else {
            return false;
        };
        let Ok(base) = self.ensure_locals(sid) else {
            return false;
        };
        let at = base as i64 + i64::from(index);
        if let Some(slot) = usize::try_from(at)
            .ok()
            .and_then(|at| self.lvars.get_mut(at))
        {
            *slot = value;
            true
        } else {
            false
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
        }
        self.rebuild_lighting();
        if animated && previous != Some(hex) {
            self.check_spatials(reference);
        }
        Ok(())
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
    pub fn inventory_weight(&self, owner: ObjectRef) -> i64 {
        self.get(owner).map_or(0, |object| {
            object
                .inventory
                .iter()
                .map(|(item, quantity)| {
                    self.get(*item).map_or(0, |object| {
                        (i64::from(object.weight()) + self.inventory_weight(*item))
                            * i64::from(*quantity)
                    })
                })
                .sum()
        })
    }
    fn armour_effect(&mut self, owner: ObjectRef, item: ObjectRef, apply: bool) {
        let Some(prototype) = self.get(item).and_then(|o| o.prototype.clone()) else {
            return;
        };
        let context = self.context(owner);
        let extra = self.rules.equipment_effects.get(&prototype.pid).copied();
        if let Some(sheet) = self.get_mut(owner).and_then(|o| o.character.as_mut()) {
            let sign = if apply { 1 } else { -1 };
            if let PrototypeData::Item(vault_formats::pro::Item {
                data: ItemData::Armor(armour),
                ..
            }) = &prototype.data
            {
                sheet.adjust_bonus(9, armour.armor_class.saturating_mul(sign), context);
                for index in 0..7 {
                    sheet.adjust_bonus(
                        17 + index,
                        armour.thresholds[index].saturating_mul(sign),
                        context,
                    );
                    sheet.adjust_bonus(
                        24 + index,
                        armour.resistances[index].saturating_mul(sign),
                        context,
                    );
                }
            }
            if let Some(effect) = extra {
                sheet.effect(effect, apply, context);
            }
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
    pub fn add_item(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        quantity: i32,
        force: bool,
    ) -> Result<(), WorldError> {
        if quantity <= 0 || owner == item || self.get(owner).is_none() || self.get(item).is_none() {
            return Err(WorldError::Invalid("invalid inventory addition".into()));
        }
        let mut ancestor = Some(owner);
        for depth in 0..=64 {
            if ancestor == Some(item) || depth == 64 {
                return Err(WorldError::Invalid("inventory cycle/depth".into()));
            }
            ancestor = ancestor.and_then(|reference| self.get(reference)?.owner);
            if ancestor.is_none() {
                break;
            }
        }
        let item_owner = self.get(item).and_then(|o| o.owner);
        if item_owner.is_some() && (!force || item_owner != Some(owner)) {
            return Err(WorldError::Invalid("item already owned".into()));
        }
        if !force {
            let extra =
                i64::from(self.get(item).expect("checked item").weight()) * i64::from(quantity);
            if let Some(sheet) = self.get(owner).and_then(|o| o.character.as_ref())
                && self.inventory_weight(owner) + extra
                    > i64::from(sheet.stat(12, self.context(owner)))
            {
                return Err(WorldError::Invalid("carry capacity exceeded".into()));
            }
            if let Some(PrototypeData::Item(vault_formats::pro::Item {
                data: ItemData::Container { capacity, .. },
                ..
            })) = self
                .get(owner)
                .and_then(|o| o.prototype.as_deref())
                .map(|p| &p.data)
            {
                let used: i64 = self
                    .get(owner)
                    .expect("owner")
                    .inventory
                    .iter()
                    .map(|(reference, count)| {
                        self.get(*reference)
                            .and_then(|o| o.prototype.as_deref())
                            .map_or(0, |p| match &p.data {
                                PrototypeData::Item(item) => {
                                    i64::from(item.size) * i64::from(*count)
                                }
                                _ => 0,
                            })
                    })
                    .sum();
                let size = self
                    .get(item)
                    .and_then(|o| o.prototype.as_deref())
                    .map_or(0, |p| match &p.data {
                        PrototypeData::Item(item) => item.size,
                        _ => 0,
                    });
                if used + i64::from(size) * i64::from(quantity) > i64::from(*capacity) {
                    return Err(WorldError::Invalid("container size exceeded".into()));
                }
            }
        }
        let inventory = &mut self.get_mut(owner).expect("owner").inventory;
        if let Some((_, count)) = inventory
            .iter_mut()
            .find(|(reference, _)| *reference == item)
        {
            *count = count.checked_add(quantity).ok_or(WorldError::Arithmetic)?;
        } else {
            inventory.push((item, quantity));
        }
        let object = self.get_mut(item).expect("item");
        object.owner = Some(owner);
        object.raw.hex = None;
        Ok(())
    }
    pub fn remove_item(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        quantity: i32,
    ) -> Result<(), WorldError> {
        if quantity <= 0 {
            return Err(WorldError::Invalid("nonpositive removal".into()));
        }
        let object = self
            .get(owner)
            .ok_or_else(|| WorldError::Invalid("missing inventory owner".into()))?;
        let slot = object
            .inventory
            .iter()
            .position(|(r, _)| *r == item)
            .ok_or_else(|| WorldError::Invalid("item is not carried".into()))?;
        if object.inventory[slot].1 < quantity {
            return Err(WorldError::Invalid("not enough inventory quantity".into()));
        }
        let remaining = object.inventory[slot].1 - quantity;
        let replacement = if remaining > 0 {
            let mut created = Vec::new();
            match self.clone_inventory_tree(item, Some(owner), &mut created) {
                Ok(reference) => Some(reference),
                Err(error) => {
                    for reference in created.into_iter().rev() {
                        self.erase(reference);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        if self
            .get(item)
            .is_some_and(|o| o.raw.flags.contains(BasicFlags::WORN))
        {
            self.armour_effect(owner, item, false);
        }
        let inventory = &mut self.get_mut(owner).expect("owner").inventory;
        if let Some(replacement) = replacement {
            inventory[slot] = (replacement, remaining);
        } else {
            inventory.remove(slot);
        }
        if let Some(item) = self.get_mut(item) {
            item.owner = None;
            item.raw.hex = Some(1);
            item.raw.elevation = 0;
            item.raw.flags.0 &= !0x07000000;
            if item.money.is_some() {
                item.money = Some(quantity);
            }
        }
        Ok(())
    }
    fn clone_inventory_tree(
        &mut self,
        original: ObjectRef,
        owner: Option<ObjectRef>,
        created: &mut Vec<ObjectRef>,
    ) -> Result<ObjectRef, WorldError> {
        let mut object = self
            .get(original)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("missing inventory tree".into()))?;
        let children = std::mem::take(&mut object.inventory);
        object.raw.id = 0;
        object.raw.script_id = -1;
        object.raw.flags.0 &= !0x07000000;
        object.owner = owner;
        let reference = self.insert(object)?;
        created.push(reference);
        for (child, quantity) in children {
            let cloned = self.clone_inventory_tree(child, Some(reference), created)?;
            self.get_mut(reference)
                .expect("clone")
                .inventory
                .push((cloned, quantity));
        }
        Ok(reference)
    }
    pub fn caps(&self, owner: ObjectRef) -> i32 {
        self.get(owner).map_or(0, |o| {
            o.inventory.iter().fold(0_i32, |sum, (item, count)| {
                sum.saturating_add(if self.get(*item).is_some_and(|o| o.money.is_some()) {
                    *count
                } else {
                    0
                })
                .saturating_add(self.caps(*item))
            })
        })
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
    pub fn map_exit_cleanup(&mut self, mut approve: impl FnMut(&TimedEvent) -> bool) {
        self.trace.push(WorldTrace::MapExitCleanup);
        self.queue.retain(|event| match event.kind {
            EventKind::Poison | EventKind::Radiation { .. } => true,
            EventKind::Script { .. } | EventKind::MapUpdate | EventKind::Midnight => false,
            EventKind::Knockout
            | EventKind::Drug { .. }
            | EventKind::Withdrawal(_)
            | EventKind::Other(_) => !approve(event),
        });
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
