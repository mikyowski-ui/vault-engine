//! SPEC-021 the deterministic state hash and the combat sub-hash.
//!
//! The covered parts of a [`WorldState`] are copied into a plain-data
//! [`StateSnapshot`], which encodes itself as the canonical byte stream of
//! SPEC-021 §2 (little-endian fixed-width integers, four-byte tags, `00` /
//! `01` option bytes, `u32`-counted sequences, sorted keyed collections).
//! Both hashes are FNV-1a 64 ([`fnv1a64`]) of their stream.
//!
//! Implementation notes (SPEC-021 §3.1 and §3.3):
//!
//! - **Handles.** The entity store's key exposes only a packed 64-bit FFI
//!   value: its low 32 bits are the slot index and its high 32 bits the
//!   generation (the store's documented layout). [`Handle::of`] splits it.
//! - **Unresolved references.** The combat lists hold object references.
//!   One that no longer names an entity has no handle left; it is written
//!   as slot index = the reference, generation 0, live flag `00`. The store
//!   never issues generation 0 (live keys have odd generations), so such a
//!   handle can never equal a live one. An inventory entry whose item no
//!   longer resolves writes item PID `FF FF FF FF`.
//! - **Current turn.** The actor the combat state names as current is the
//!   world's combat actor; its position in the active partition is the
//!   current turn, absent when it is not there.
//! - **Map loaded.** No map is loaded while the outgoing map was cleared
//!   for the world map, or while the current map has no working name (the
//!   empty map of a fresh world); the save path uses the same test.
//!
//! This module never touches `std::hash`, a hash map or set, or a
//! platform-width integer in the stream (SPEC-021 test T12).
use crate::{EntityId, ObjectRef, WorldState};
use slotmap::Key;
use std::fmt;
use vault_formats::map::ObjectState;

/// The FNV-1a 64 offset basis.
pub const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
/// The FNV-1a 64 prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
/// The world stream's magic, revision 1.
pub const WORLD_MAGIC: [u8; 4] = *b"VSH1";
/// The combat stream's magic, revision 1.
pub const COMBAT_MAGIC: [u8; 4] = *b"VCH1";

/// Streaming FNV-1a 64 (SPEC-021 §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64(u64);
impl Default for Fnv1a64 {
    fn default() -> Self {
        Self(FNV_OFFSET_BASIS)
    }
}
impl Fnv1a64 {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
}
/// FNV-1a 64 of `bytes`.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = Fnv1a64::new();
    hash.write(bytes);
    hash.finish()
}

/// An entity handle: slot index and generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Handle {
    pub index: u32,
    pub generation: u32,
}
impl Handle {
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
    /// Split the store key's packed value: index low, generation high.
    pub fn of(id: EntityId) -> Self {
        let packed = id.data().as_ffi();
        Self {
            index: packed as u32,
            generation: (packed >> 32) as u32,
        }
    }
}
impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.index, self.generation)
    }
}
impl std::str::FromStr for Handle {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (index, generation) = text
            .split_once(':')
            .ok_or_else(|| format!("handle {text:?} is not index:generation"))?;
        let number = |part: &str| {
            part.parse::<u32>()
                .map_err(|_| format!("handle {text:?} is not index:generation"))
        };
        Ok(Self::new(number(index)?, number(generation)?))
    }
}

/// A map position: elevation and SPEC-003 linear hex number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub elevation: u32,
    pub hex: u32,
}
/// One direct inventory entry: item PID and quantity, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InventoryEntry {
    pub pid: u32,
    pub quantity: u32,
}
/// SPEC-021 §2.4 one entity. `hp` and `ap` are written only when the PID's
/// type byte is 1 (a critter); extraction leaves them 0 for other types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord {
    pub handle: Handle,
    pub pid: u32,
    pub position: Option<Position>,
    pub orientation: u32,
    pub hp: i32,
    pub ap: i32,
    pub inventory: Vec<InventoryEntry>,
}
impl EntityRecord {
    pub fn critter(&self) -> bool {
        self.pid >> 24 == 1
    }
}
/// SPEC-021 §2.5 the SPEC-006 generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RngRecord {
    pub state: i32,
    pub selector: i32,
    pub entries: [i32; 32],
}
impl From<&crate::RngState> for RngRecord {
    fn from(state: &crate::RngState) -> Self {
        Self {
            state: state.recurrence,
            selector: state.selector,
            entries: state.shuffle,
        }
    }
}
/// What a live participant record carries after its handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantState {
    pub hp: i32,
    pub ap: i32,
    pub position: Option<Position>,
    pub orientation: u32,
}
/// SPEC-021 §2.6 one combat participant; `live` is `None` for a handle
/// that does not resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Participant {
    pub handle: Handle,
    pub live: Option<ParticipantState>,
}
/// SPEC-021 §2.6 the combat record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CombatRecord {
    pub round: u32,
    pub current: Option<u32>,
    pub active: Vec<Participant>,
    pub waiting: Vec<Participant>,
}
/// SPEC-021 §2.8 one script instance's non-empty LVAR slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LvarSlice {
    pub script: u32,
    pub values: Vec<i32>,
}
/// The covered values of a world, as plain data (SPEC-021 §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    pub entities: Vec<EntityRecord>,
    pub rng: RngRecord,
    pub combat: Option<CombatRecord>,
    pub gvars: Option<Vec<i32>>,
    pub mvars: Option<Vec<i32>>,
    pub lvars: Vec<LvarSlice>,
}

/// Where encoded bytes go: a buffer, or straight into the hash.
trait Sink {
    fn put(&mut self, bytes: &[u8]);
}
impl Sink for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}
impl Sink for Fnv1a64 {
    fn put(&mut self, bytes: &[u8]) {
        self.write(bytes);
    }
}
/// The SPEC-021 §2.2 primitive encodings over a sink.
struct Encoder<'a, S: Sink>(&'a mut S);
impl<S: Sink> Encoder<'_, S> {
    fn u8(&mut self, value: u8) {
        self.0.put(&[value]);
    }
    fn u32(&mut self, value: u32) {
        self.0.put(&value.to_le_bytes());
    }
    fn i32(&mut self, value: i32) {
        self.0.put(&value.to_le_bytes());
    }
    fn tag(&mut self, tag: &[u8; 4]) {
        self.0.put(tag);
    }
    /// A sequence count; a collection too long for u32 writes `FF FF FF FF`
    /// and still writes every element (SPEC-021 §3.4).
    fn count(&mut self, len: usize) {
        self.u32(u32::try_from(len).unwrap_or(u32::MAX));
    }
    fn present(&mut self, present: bool) {
        self.u8(u8::from(present));
    }
    fn handle(&mut self, handle: Handle) {
        self.u32(handle.index);
        self.u32(handle.generation);
    }
    fn position(&mut self, position: Option<Position>) {
        self.present(position.is_some());
        if let Some(position) = position {
            self.u32(position.elevation);
            self.u32(position.hex);
        }
    }
    fn opt_i32(&mut self, present: bool, value: i32) {
        self.present(present);
        if present {
            self.i32(value);
        }
    }
    fn i32s(&mut self, values: &[i32]) {
        self.count(values.len());
        for value in values {
            self.i32(*value);
        }
    }
    fn entity(&mut self, entity: &EntityRecord) {
        self.handle(entity.handle);
        self.u32(entity.pid);
        self.position(entity.position);
        self.u32(entity.orientation);
        let critter = entity.critter();
        self.opt_i32(critter, entity.hp);
        self.opt_i32(critter, entity.ap);
        let mut inventory: Vec<InventoryEntry> = entity.inventory.clone();
        inventory.sort_unstable();
        self.count(inventory.len());
        for entry in inventory {
            self.u32(entry.pid);
            self.u32(entry.quantity);
        }
    }
    fn rng(&mut self, rng: &RngRecord) {
        self.i32(rng.state);
        self.i32(rng.selector);
        for entry in rng.entries {
            self.i32(entry);
        }
    }
    fn participant(&mut self, participant: &Participant) {
        self.handle(participant.handle);
        self.present(participant.live.is_some());
        if let Some(live) = participant.live {
            self.i32(live.hp);
            self.i32(live.ap);
            self.position(live.position);
            self.u32(live.orientation);
        }
    }
    /// World stream parts 6 and 7, which the combat stream repeats.
    fn combat(&mut self, combat: Option<&CombatRecord>) {
        self.tag(b"CMBT");
        self.present(combat.is_some());
        let Some(combat) = combat else {
            return;
        };
        self.u32(combat.round);
        self.present(combat.current.is_some());
        if let Some(current) = combat.current {
            self.u32(current);
        }
        for list in [&combat.active, &combat.waiting] {
            self.count(list.len());
            for participant in list {
                self.participant(participant);
            }
        }
    }
    fn world(&mut self, snapshot: &StateSnapshot) {
        self.tag(&WORLD_MAGIC);
        self.tag(b"ENTS");
        let mut entities: Vec<&EntityRecord> = snapshot.entities.iter().collect();
        entities.sort_by_key(|entity| entity.handle);
        self.count(entities.len());
        for entity in entities {
            self.entity(entity);
        }
        self.tag(b"RNGS");
        self.rng(&snapshot.rng);
        self.combat(snapshot.combat.as_ref());
        for (tag, values) in [(b"GVAR", &snapshot.gvars), (b"MVAR", &snapshot.mvars)] {
            self.tag(tag);
            self.present(values.is_some());
            if let Some(values) = values {
                self.i32s(values);
            }
        }
        self.tag(b"LVAR");
        let mut slices: Vec<&LvarSlice> = snapshot.lvars.iter().collect();
        slices.sort_by(|a, b| (a.script, &a.values).cmp(&(b.script, &b.values)));
        self.count(slices.len());
        for slice in slices {
            self.u32(slice.script);
            self.i32s(&slice.values);
        }
    }
    fn combat_stream(&mut self, combat: Option<&CombatRecord>) {
        self.tag(&COMBAT_MAGIC);
        self.combat(combat);
    }
}

impl StateSnapshot {
    /// The SPEC-021 §2.3 world stream.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        Encoder(&mut bytes).world(self);
        bytes
    }
    /// The SPEC-021 §2.7 combat stream.
    pub fn canonical_combat_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        Encoder(&mut bytes).combat_stream(self.combat.as_ref());
        bytes
    }
    pub fn state_hash(&self) -> u64 {
        let mut hash = Fnv1a64::new();
        Encoder(&mut hash).world(self);
        hash.finish()
    }
    pub fn combat_hash(&self) -> u64 {
        let mut hash = Fnv1a64::new();
        Encoder(&mut hash).combat_stream(self.combat.as_ref());
        hash.finish()
    }
}

impl WorldState {
    /// Whether a map is loaded (the MVAR part's presence).
    pub fn map_loaded(&self) -> bool {
        !self.unloaded && self.current_map_name().is_some()
    }
    /// The handle of a reference, and whether it names a live entity.
    fn participant_handle(&self, reference: ObjectRef) -> (Handle, bool) {
        match self.entity_key(reference) {
            Some(key) => (Handle::of(key), self.entities.contains_key(key)),
            None => (Handle::new(reference, 0), false),
        }
    }
    /// The handle a combat trace names an actor by (see the module notes
    /// for a reference that no longer resolves).
    pub fn state_handle(&self, reference: ObjectRef) -> Handle {
        self.participant_handle(reference).0
    }
    fn critter_values(&self, object: &crate::world::WorldObject) -> (i32, i32) {
        match &object.raw.state {
            ObjectState::Critter(state) if object.raw.pid >> 24 == 1 => (state.hp, state.ap),
            _ => (0, 0),
        }
    }
    fn object_position(object: &crate::world::WorldObject) -> Option<Position> {
        if object.owner.is_some() {
            return None;
        }
        object.raw.hex.map(|hex| Position {
            elevation: u32::from(object.raw.elevation),
            hex: hex as u32,
        })
    }
    fn participant(&self, reference: ObjectRef) -> Participant {
        let (handle, live) = self.participant_handle(reference);
        let live = live.then(|| self.get(reference)).flatten().map(|object| {
            let (hp, ap) = self.critter_values(object);
            ParticipantState {
                hp,
                ap,
                position: Self::object_position(object),
                orientation: u32::from(object.raw.orientation),
            }
        });
        Participant { handle, live }
    }
    fn combat_record(&self) -> Option<CombatRecord> {
        let combat = &self.combat_state;
        if combat.flags & crate::critical::state::ACTIVE == 0 {
            return None;
        }
        let split = combat.active.min(combat.list.len());
        let (active, waiting) = combat.list.split_at(split);
        let current = active
            .iter()
            .position(|critter| *critter == self.combat_actor && self.combat_actor != 0)
            .map(|position| u32::try_from(position).unwrap_or(u32::MAX));
        Some(CombatRecord {
            round: combat.round,
            current,
            active: active.iter().map(|c| self.participant(*c)).collect(),
            waiting: waiting.iter().map(|c| self.participant(*c)).collect(),
        })
    }
    /// The covered values as plain data (SPEC-021 §3.3).
    pub fn state_snapshot(&self) -> StateSnapshot {
        let entities = self
            .entities
            .iter()
            .map(|(key, object)| {
                let (hp, ap) = self.critter_values(object);
                EntityRecord {
                    handle: Handle::of(key),
                    pid: object.raw.pid,
                    position: Self::object_position(object),
                    orientation: u32::from(object.raw.orientation),
                    hp,
                    ap,
                    inventory: object
                        .inventory
                        .iter()
                        .map(|(item, quantity)| InventoryEntry {
                            pid: self.get(*item).map_or(u32::MAX, |item| item.raw.pid),
                            quantity: *quantity as u32,
                        })
                        .collect(),
                }
            })
            .collect();
        let lvars = self
            .scripts
            .values()
            .filter_map(|script| {
                let base = usize::try_from(script.local_base).ok()?;
                let end = base
                    .saturating_add(script.local_count)
                    .min(self.lvars.len());
                let values = self.lvars.get(base..end)?;
                (!values.is_empty()).then(|| LvarSlice {
                    script: script.id,
                    values: values.to_vec(),
                })
            })
            .collect();
        StateSnapshot {
            entities,
            rng: RngRecord::from(self.rng.state()),
            combat: self.combat_record(),
            gvars: self.gvars.clone(),
            mvars: self.map_loaded().then(|| self.mvars.clone()),
            lvars,
        }
    }
    /// SPEC-021 the world stream's FNV-1a 64 hash.
    pub fn state_hash(&self) -> u64 {
        self.state_snapshot().state_hash()
    }
    /// SPEC-021 §2.7 the combat stream's FNV-1a 64 hash.
    pub fn combat_hash(&self) -> u64 {
        let mut hash = Fnv1a64::new();
        Encoder(&mut hash).combat_stream(self.combat_record().as_ref());
        hash.finish()
    }
}
