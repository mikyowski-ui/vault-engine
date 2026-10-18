//! SPEC-011 §3.13-3.16 inventory rules: stacking and ammunition stacks,
//! item weight with the carry, body-type, container, and power-cell limits,
//! item value and caps, reloading (direct, search, and drag), and
//! unloading.
use crate::{ObjectRef, WorldError, WorldState, world::WorldObject};
use std::sync::Arc;
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, Category, Item, ItemData, Prototype, PrototypeData},
};

/// Money's PID. Its base cost of 1 makes caps count at face value.
pub const MONEY_PID: u32 = 41;
/// The worn and in-hand flags.
pub const EQUIPPED: u32 = BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0 | BasicFlags::WORN.0;

/// Why §3.14 refuses an addition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The owner is a critter whose carry limit would be exceeded, or a
    /// container whose outermost critter owner's limit would be.
    TooHeavy,
    /// The owner is a critter with a body type other than biped.
    WrongBody,
    /// The container's capacity would be reached.
    TooBig,
    /// A misc item accepts only its power cell; other items accept nothing.
    NotAccepted,
}
impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooHeavy => "carry capacity exceeded",
            Self::WrongBody => "only bipeds carry inventory",
            Self::TooBig => "container size exceeded",
            Self::NotAccepted => "this item cannot hold that item",
        }
    }
}

fn item_of(object: &WorldObject) -> Option<&Item> {
    match &object.prototype.as_deref()?.data {
        PrototypeData::Item(item) => Some(item),
        _ => None,
    }
}

impl WorldState {
    pub(crate) fn item_data(&self, item: ObjectRef) -> Option<&Item> {
        item_of(self.get(item)?)
    }
    /// Record a prototype for later lookups by PID (the ammunition terms of
    /// loaded weapons). Every inserted object's prototype is recorded.
    pub fn remember_prototype(&mut self, prototype: Arc<Prototype>) {
        self.prototypes.entry(prototype.pid).or_insert(prototype);
    }
    pub fn known_prototype(&self, pid: u32) -> Option<&Arc<Prototype>> {
        self.prototypes.get(&pid)
    }
    /// Weight, base cost, and pack size of an ammunition PID, when the PID
    /// names a known ammunition prototype (a "valid ammunition PID").
    pub(crate) fn ammo_terms(&self, pid: i32) -> Option<(i32, i32, i32)> {
        let prototype = self.prototypes.get(&u32::try_from(pid).ok()?)?;
        match &prototype.data {
            PrototypeData::Item(Item {
                weight,
                cost,
                data: ItemData::Ammunition { quantity, .. },
                ..
            }) => Some((*weight, *cost, *quantity)),
            _ => None,
        }
    }
    /// The pack size of an ammunition object.
    pub(crate) fn pack_size(&self, item: ObjectRef) -> Option<i32> {
        match self.item_data(item)?.data {
            ItemData::Ammunition { quantity, .. } => Some(quantity),
            _ => None,
        }
    }
    /// §3.14 item weight: a container adds its contents, a loaded weapon
    /// with a valid ammunition PID adds its whole packs rounded up, and
    /// anything else (ammunition included) is its own weight.
    pub fn item_weight(&self, item: ObjectRef) -> i64 {
        let Some(data) = self.item_data(item) else {
            return 0;
        };
        let own = i64::from(data.weight.max(0));
        match data.data {
            ItemData::Container { .. } => own + self.inventory_weight(item),
            ItemData::Weapon(_) => {
                let (rounds, pid) = self.loaded(item);
                match self.ammo_terms(pid) {
                    Some((weight, _, pack)) if rounds > 0 && pack > 0 => {
                        own + i64::from(weight.max(0))
                            * (i64::from(rounds - 1) / i64::from(pack) + 1)
                    }
                    _ => own,
                }
            }
            _ => own,
        }
    }
    /// The total weight of an owner's entries: item weight × quantity, so a
    /// partial ammunition front pack weighs as a full one.
    pub fn inventory_weight(&self, owner: ObjectRef) -> i64 {
        self.get(owner).map_or(0, |object| {
            object
                .inventory
                .iter()
                .map(|(item, quantity)| self.item_weight(*item) * i64::from(*quantity))
                .sum()
        })
    }
    fn carry_limit(&self, critter: ObjectRef) -> i64 {
        self.get(critter)
            .and_then(|object| object.character.as_ref())
            .map_or(0, |sheet| i64::from(sheet.stat(12, self.context(critter))))
    }
    /// The §3.14 limit test for adding `quantity` of `item` to `owner`.
    pub fn add_refusal(&self, owner: ObjectRef, item: ObjectRef, quantity: i32) -> Option<Refusal> {
        let target = self.get(owner)?;
        let added = self.item_weight(item) * i64::from(quantity);
        let heavy =
            |critter: ObjectRef| self.inventory_weight(critter) + added > self.carry_limit(critter);
        match target.category() {
            Category::Critter => {
                let sheet = target.character.as_ref()?;
                if sheet.body != 0 {
                    Some(Refusal::WrongBody)
                } else if heavy(owner) {
                    Some(Refusal::TooHeavy)
                } else {
                    None
                }
            }
            Category::Item => match item_of(target)?.data {
                ItemData::Container { capacity, .. } => {
                    let size = |item: ObjectRef| {
                        i64::from(self.item_data(item).map_or(0, |data| data.size))
                    };
                    let used: i64 = target
                        .inventory
                        .iter()
                        .map(|(content, count)| size(*content) * i64::from(*count))
                        .sum();
                    // Strict capacity: reaching the maximum is refused.
                    if used + size(item) * i64::from(quantity) >= i64::from(capacity) {
                        return Some(Refusal::TooBig);
                    }
                    let root = self.outermost_owner(owner);
                    (root != owner
                        && self
                            .get(root)
                            .is_some_and(|o| o.category() == Category::Critter)
                        && heavy(root))
                    .then_some(Refusal::TooHeavy)
                }
                ItemData::Misc { power_pid, .. } => (self.get(item).map(|o| o.raw.pid as i32)
                    != Some(power_pid))
                .then_some(Refusal::NotAccepted),
                _ => Some(Refusal::NotAccepted),
            },
            _ => None,
        }
    }
    /// Follow the owner chain to the object that is not itself owned.
    pub(crate) fn outermost_owner(&self, mut reference: ObjectRef) -> ObjectRef {
        for _ in 0..64 {
            match self.get(reference).and_then(|object| object.owner) {
                Some(parent) => reference = parent,
                None => break,
            }
        }
        reference
    }
    /// Whether `owner` holds `reference` directly or inside its containers.
    fn holds(&self, owner: ObjectRef, mut reference: ObjectRef) -> bool {
        for _ in 0..64 {
            match self.get(reference).and_then(|object| object.owner) {
                Some(parent) if parent == owner => return true,
                Some(parent) => reference = parent,
                None => return false,
            }
        }
        false
    }
    /// The stacking identity of §3.13, or none for an item that never
    /// stacks (equipped, used/armed, a container, or holding contents).
    /// Ammunition rounds and the money amount are left out.
    fn stack_key(&self, item: ObjectRef) -> Option<(u32, i32, ObjectState)> {
        let object = self.get(item)?;
        let data = item_of(object)?;
        if object.raw.flags.0 & (EQUIPPED | BasicFlags::USED.0) != 0
            || matches!(data.data, ItemData::Container { .. })
            || !object.inventory.is_empty()
        {
            return None;
        }
        let state = match &object.raw.state {
            ObjectState::Other {
                word,
                detail: ObjectDetail::Ammunition(_),
            } => ObjectState::Other {
                word: *word,
                detail: ObjectDetail::Ammunition(0),
            },
            ObjectState::Other { detail, .. } if object.money.is_some() => ObjectState::Other {
                word: 0,
                detail: match detail {
                    ObjectDetail::Charges(_) => ObjectDetail::Charges(0),
                    other => other.clone(),
                },
            },
            state => state.clone(),
        };
        Some((object.raw.pid, object.raw.script_id, state))
    }
    /// Whether two distinct items share one inventory entry.
    pub fn stackable(&self, a: ObjectRef, b: ObjectRef) -> bool {
        a != b
            && self
                .stack_key(a)
                .is_some_and(|key| Some(key) == self.stack_key(b))
    }
    /// SPEC-011 §3.13-3.14 inventory addition. A forced transfer (barter
    /// settlement, table returns, rollback) skips the limits. The item
    /// merges into a stackable entry and becomes its representative.
    pub fn add_item(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        quantity: i32,
        force: bool,
    ) -> Result<(), WorldError> {
        if quantity < 1 || owner == item || self.get(owner).is_none() || self.get(item).is_none() {
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
        if let Some(current) = self.get(item).and_then(|o| o.owner) {
            let representative = current == owner
                && self
                    .get(owner)
                    .is_some_and(|o| o.inventory.iter().any(|(entry, _)| *entry == item));
            // Re-adding the representative itself is a successful no-op.
            if force && representative {
                return Ok(());
            }
            return Err(WorldError::Invalid("item already owned".into()));
        }
        if !force && let Some(refusal) = self.add_refusal(owner, item, quantity) {
            return Err(WorldError::Invalid(refusal.message().into()));
        }
        let slot = self.get(owner).and_then(|object| {
            object
                .inventory
                .iter()
                .position(|(entry, _)| self.stackable(*entry, item))
        });
        match slot {
            Some(slot) => {
                let (old, count) = self.get(owner).expect("owner").inventory[slot];
                let count = match self.pack_size(item) {
                    Some(pack) => {
                        let rounds = self.ammo_rounds(old) + self.ammo_rounds(item);
                        if rounds > pack {
                            self.set_ammo_rounds(item, rounds - pack);
                            count.checked_add(quantity)
                        } else {
                            self.set_ammo_rounds(item, rounds);
                            count.checked_add(quantity - 1)
                        }
                    }
                    None => count.checked_add(quantity),
                }
                .ok_or(WorldError::Arithmetic)?;
                self.get_mut(owner).expect("owner").inventory[slot] = (item, count);
                // The old representative is destroyed; it must not take a
                // shared script with it.
                let script = self.get(item).map_or(-1, |o| o.raw.script_id);
                if let Some(object) = self.get_mut(old) {
                    object.owner = None;
                    if object.raw.script_id == script {
                        object.raw.script_id = -1;
                    }
                }
                self.erase(old);
            }
            None => self
                .get_mut(owner)
                .expect("owner")
                .inventory
                .push((item, quantity)),
        }
        let object = self.get_mut(item).expect("item");
        object.owner = Some(owner);
        object.raw.hex = None;
        Ok(())
    }
    /// SPEC-011 §3.13 removal of `quantity` units from an entry that `owner`
    /// holds directly or inside a container. The removed object is detached
    /// (connected at hex 1, elevation 0) with its equipped flags cleared; a
    /// copy represents any remaining units, an ammunition copy with full
    /// packs, so a partial pack always leaves first.
    pub fn remove_item(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        quantity: i32,
    ) -> Result<(), WorldError> {
        if quantity <= 0 {
            return Err(WorldError::Invalid("nonpositive removal".into()));
        }
        if self.get(owner).is_none() {
            return Err(WorldError::Invalid("missing inventory owner".into()));
        }
        let not_carried = || WorldError::Invalid("item is not carried".into());
        let parent = self
            .get(item)
            .and_then(|object| object.owner)
            .filter(|parent| *parent == owner || self.holds(owner, *parent))
            .ok_or_else(not_carried)?;
        let slot = self
            .get(parent)
            .and_then(|object| object.inventory.iter().position(|(r, _)| *r == item))
            .ok_or_else(not_carried)?;
        let count = self.get(parent).expect("parent").inventory[slot].1;
        if count < quantity {
            return Err(WorldError::Invalid("not enough inventory quantity".into()));
        }
        let remaining = count - quantity;
        let replacement = if remaining > 0 {
            let mut created = Vec::new();
            match self.clone_inventory_tree(item, Some(parent), &mut created) {
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
        if let (Some(replacement), Some(pack)) = (replacement, self.pack_size(item)) {
            self.set_ammo_rounds(replacement, pack);
        }
        if self
            .get(item)
            .is_some_and(|o| o.raw.flags.contains(BasicFlags::WORN))
        {
            self.armour_effect(parent, item, false);
        }
        let inventory = &mut self.get_mut(parent).expect("parent").inventory;
        if let Some(replacement) = replacement {
            inventory[slot] = (replacement, remaining);
        } else {
            inventory.remove(slot);
        }
        if let Some(item) = self.get_mut(item) {
            item.owner = None;
            item.raw.hex = Some(1);
            item.raw.elevation = 0;
            item.raw.flags.0 &= !EQUIPPED;
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
        object.raw.flags.0 &= !EQUIPPED;
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
    /// Destroy one unit of a carried stack (the rest keeps full packs).
    pub fn destroy_unit(&mut self, item: ObjectRef) -> Result<(), WorldError> {
        if let Some(owner) = self.get(item).and_then(|object| object.owner) {
            self.remove_item(owner, item, 1)?;
        }
        self.erase(item);
        Ok(())
    }
    /// §3.16 item value: a container adds its contents, a loaded weapon
    /// with a valid ammunition PID adds `L × ammo cost / K`, an ammunition
    /// object is worth `cost × R / K`, anything else its base cost.
    pub fn item_value(&self, item: ObjectRef) -> i64 {
        let Some(data) = self.item_data(item) else {
            return 0;
        };
        let cost = i64::from(data.cost);
        match data.data {
            ItemData::Container { .. } => cost + self.inventory_value(item),
            ItemData::Weapon(_) => {
                let (rounds, pid) = self.loaded(item);
                match self.ammo_terms(pid) {
                    Some((_, ammo_cost, pack)) if rounds > 0 && pack != 0 => {
                        cost + i64::from(rounds) * i64::from(ammo_cost) / i64::from(pack)
                    }
                    _ => cost,
                }
            }
            ItemData::Ammunition { quantity, .. } if quantity != 0 => {
                cost * i64::from(self.ammo_rounds(item)) / i64::from(quantity)
            }
            _ => cost,
        }
    }
    /// Entry value: value × Q, except that an ammunition entry counts its
    /// full packs plus its front pack: `cost × (Q - 1) + cost × R / K`.
    pub fn entry_value(&self, item: ObjectRef, quantity: i32) -> i64 {
        match self.item_data(item).map(|data| (data.cost, &data.data)) {
            Some((cost, ItemData::Ammunition { .. })) => {
                i64::from(cost) * i64::from(quantity - 1) + self.item_value(item)
            }
            _ => self.item_value(item) * i64::from(quantity),
        }
    }
    /// The total value of an owner's entries (caps included at face value).
    pub fn inventory_value(&self, owner: ObjectRef) -> i64 {
        self.get(owner).map_or(0, |object| {
            object
                .inventory
                .iter()
                .map(|(item, quantity)| self.entry_value(*item, *quantity))
                .sum()
        })
    }
    /// Total caps: the quantities of money entries, plus money inside
    /// containers, recursively.
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
    /// Whether any item in an owner's inventory, recursively, is flagged as
    /// used/armed (a primed explosive).
    pub fn holds_armed(&self, owner: ObjectRef) -> bool {
        self.get(owner).is_some_and(|object| {
            object.inventory.iter().any(|(item, _)| {
                self.get(*item)
                    .is_some_and(|o| o.raw.flags.contains(BasicFlags::USED))
                    || self.holds_armed(*item)
            })
        })
    }
    fn set_loaded(&mut self, weapon: ObjectRef, rounds: i32, ammo_pid: i32) {
        if let Some(object) = self.get_mut(weapon) {
            let word = match object.raw.state {
                ObjectState::Other { word, .. } => word,
                ObjectState::Critter(_) => return,
            };
            object.raw.state = ObjectState::Other {
                word,
                detail: ObjectDetail::Weapon { rounds, ammo_pid },
            };
        }
    }
    /// §3.15 reload of `weapon` from `ammo`: transfer up to the free
    /// magazine space, set the weapon's ammunition PID, and return the
    /// rounds left in the ammunition (unchanged for a full magazine), or -1
    /// for an incompatible pair.
    pub fn reload_weapon(&mut self, weapon: ObjectRef, ammo: ObjectRef) -> i32 {
        if !self.reload_permitted(weapon, ammo) {
            return -1;
        }
        let capacity = self.weapon_data(weapon).map_or(0, |gun| gun.capacity);
        let (loaded, _) = self.loaded(weapon);
        let mut left = self.ammo_rounds(ammo);
        if loaded < capacity {
            let transfer = (capacity - loaded).min(left).max(0);
            let pid = self.get(ammo).map_or(-1, |object| object.raw.pid as i32);
            self.set_loaded(weapon, loaded + transfer, pid);
            left -= transfer;
            self.set_ammo_rounds(ammo, left);
        }
        left
    }
    /// The first top-level ammunition entry the weapon may reload from.
    pub fn first_compatible_ammo(&self, owner: ObjectRef, weapon: ObjectRef) -> Option<ObjectRef> {
        self.get(owner)?
            .inventory
            .iter()
            .map(|(item, _)| *item)
            .find(|item| self.pack_size(*item).is_some() && self.reload_permitted(weapon, *item))
    }
    /// §3.15 search reload: one reload from the first compatible entry in
    /// inventory order, destroying one emptied unit. A full weapon is not
    /// reloaded. Returns whether a reload happened.
    pub fn search_reload(
        &mut self,
        owner: ObjectRef,
        weapon: ObjectRef,
    ) -> Result<bool, WorldError> {
        let capacity = self.weapon_data(weapon).map_or(0, |gun| gun.capacity);
        if self.loaded(weapon).0 >= capacity {
            return Ok(false);
        }
        let Some(ammo) = self.first_compatible_ammo(owner, weapon) else {
            return Ok(false);
        };
        if self.reload_weapon(weapon, ammo) == 0 {
            self.destroy_unit(ammo)?;
        }
        Ok(true)
    }
    /// §3.15 inventory drag reload of a carried weapon from up to `count`
    /// units of an ammunition entry: the weapon leaves the inventory first,
    /// emptied units are destroyed while the entry lasts, the loop stops at
    /// a pack left non-empty, and the weapon is re-added (it may re-stack,
    /// becoming the entry's representative). Returns whether anything was
    /// loaded (the caller plays the ready sound).
    pub fn drag_reload(
        &mut self,
        owner: ObjectRef,
        weapon: ObjectRef,
        ammo: ObjectRef,
        count: i32,
    ) -> Result<bool, WorldError> {
        let before = self.loaded(weapon).0;
        self.remove_item(owner, weapon, 1)?;
        let mut ammo = ammo;
        for _ in 0..count.max(0) {
            let left = self.reload_weapon(weapon, ammo);
            if left != 0 {
                break;
            }
            let parent = self.get(ammo).and_then(|object| object.owner);
            let slot = parent.and_then(|parent| {
                self.get(parent)?
                    .inventory
                    .iter()
                    .position(|(entry, units)| *entry == ammo && *units > 1)
            });
            self.destroy_unit(ammo)?;
            match parent.zip(slot) {
                Some((parent, slot)) => ammo = self.get(parent).expect("parent").inventory[slot].0,
                None => break,
            }
        }
        let loaded = self.loaded(weapon).0 > before;
        self.add_item(owner, weapon, 1, true)?;
        Ok(loaded)
    }
    /// §3.15 unload: a new ammunition object of the weapon's ammunition PID
    /// holding `min(L, K)` rounds (the caller places it); the weapon keeps
    /// the rest. Requires capacity > 0, L > 0, and a valid ammunition PID.
    pub fn unload_weapon(&mut self, weapon: ObjectRef) -> Result<Option<ObjectRef>, WorldError> {
        let capacity = self.weapon_data(weapon).map_or(0, |gun| gun.capacity);
        let (rounds, pid) = self.loaded(weapon);
        let prototype = u32::try_from(pid)
            .ok()
            .and_then(|pid| self.prototypes.get(&pid).cloned());
        let (Some(prototype), Some((_, _, pack))) = (prototype, self.ammo_terms(pid)) else {
            return Ok(None);
        };
        if capacity <= 0 || rounds <= 0 {
            return Ok(None);
        }
        let taken = rounds.min(pack);
        let ammo = self.instantiate(prototype)?;
        self.set_ammo_rounds(ammo, taken);
        self.set_loaded(weapon, rounds - taken, pid);
        Ok(Some(ammo))
    }
}
