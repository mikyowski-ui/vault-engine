//! SPEC-011 §3.17-3.19 barter: the session setup (hidden merchant armor
//! and right-hand item or first weapon, the script and reaction
//! modifiers), the merchant price formula, transaction acceptance, and the
//! returns on leaving.
use crate::{
    ObjectRef, Perk, WorldError,
    assets::WorldAssets,
    host::WorldHost,
    world::{WorldObject, object_record},
};
use vault_formats::pro::{BasicFlags, ItemData};

/// Master Trader's modifier reduction.
pub const MASTER_TRADER: i32 = 25;
/// The floor of a negative markup factor: single-precision 0.01 widened to
/// binary64, `0.0099999998` to the specification's printed precision.
pub const MINIMUM_FACTOR: f64 = 0.01_f32 as f64;

/// §3.17 step 3: reaction above 75 gives -15, above 25 gives 0, otherwise
/// +25.
pub fn reaction_modifier(reaction: i32) -> i32 {
    if reaction > 75 {
        -15
    } else if reaction > 25 {
        0
    } else {
        25
    }
}

/// §3.18 merchant price in IEEE-754 binary64: `g = V_T - C_T`;
/// `m = (M + 100 - MT) × 0.01`, floored at [`MINIMUM_FACTOR`] when
/// negative; `r = (160 + B_m) / (160 + B_p) × 2g`; price `trunc(m × r + C_T)`.
/// An empty table therefore costs 0. There is no random draw.
pub fn price(
    table_value: i64,
    table_caps: i64,
    modifier: i32,
    master_trader: bool,
    player_barter: i32,
    merchant_barter: i32,
) -> i32 {
    let goods = (table_value - table_caps) as f64;
    let discount = if master_trader { MASTER_TRADER } else { 0 };
    let mut factor = (i64::from(modifier) + 100 - i64::from(discount)) as f64 * 0.01;
    if factor < 0.0 {
        factor = MINIMUM_FACTOR;
    }
    let ratio =
        (160.0 + f64::from(merchant_barter)) / (160.0 + f64::from(player_barter)) * (2.0 * goods);
    (factor * ratio + table_caps as f64).trunc() as i32
}

/// §3.19 outcome of the offer key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trade {
    /// Both tables were empty; nothing happens.
    Nothing,
    Accepted,
    Refused,
}

/// One barter session: the merchant, its offer table T, the player's offer
/// table P, and the merchant equipment hidden for the session with the
/// equipment flag each item gets back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarterSession {
    pub merchant: ObjectRef,
    pub table: ObjectRef,
    pub offer: ObjectRef,
    hidden: Vec<(ObjectRef, u32)>,
    /// The inventory place the hidden weapon left, where the `enhanced`
    /// WIELD button puts it back ([`WorldHost::barter_wield`]).
    weapon_slot: usize,
}
impl BarterSession {
    /// Merchant items hidden (unbuyable) during the session.
    pub fn hidden(&self) -> impl Iterator<Item = ObjectRef> + '_ {
        self.hidden.iter().map(|(item, _)| *item)
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// An unplaced holder for one offer table; it accepts anything.
    fn barter_table(&mut self) -> Result<ObjectRef, WorldError> {
        self.world.insert(WorldObject {
            raw: object_record(0x0500_0000, 0),
            prototype: None,
            character: None,
            name: b"barter table".to_vec(),
            description: Vec::new(),
            owner: None,
            inventory: Vec::new(),
            money: None,
            active: false,
            opponent: 0,
        })
    }
    /// Take one unit out of the merchant's inventory for the session;
    /// returns the inventory place its entry had.
    fn hide(&mut self, merchant: ObjectRef, item: ObjectRef) -> Result<usize, WorldError> {
        let slot = self
            .world
            .get(merchant)
            .and_then(|object| {
                object
                    .inventory
                    .iter()
                    .position(|(entry, _)| *entry == item)
            })
            .unwrap_or(0);
        self.world.remove_item(merchant, item, 1)?;
        if let Some(object) = self.world.get_mut(item) {
            object.raw.hex = None;
        }
        Ok(slot)
    }
    /// §3.17 barter entry: the merchant's worn armor is hidden, then its
    /// right-hand item or, without one, its first weapon in inventory
    /// order.
    pub fn begin_barter(&mut self, merchant: ObjectRef) -> Result<BarterSession, WorldError> {
        let inventory = self
            .world
            .get(merchant)
            .ok_or_else(|| WorldError::Invalid("barter requires a merchant".into()))?
            .inventory
            .clone();
        let flagged = |host: &Self, flag: BasicFlags| {
            inventory.iter().map(|(item, _)| *item).find(|item| {
                host.world
                    .get(*item)
                    .is_some_and(|object| object.raw.flags.contains(flag))
            })
        };
        let mut hidden = Vec::new();
        if let Some(armour) = flagged(self, BasicFlags::WORN) {
            hidden.push((armour, BasicFlags::WORN.0));
        }
        let held = flagged(self, BasicFlags::RIGHT_HAND)
            .map(|item| (item, BasicFlags::RIGHT_HAND.0))
            .or_else(|| {
                inventory
                    .iter()
                    .map(|(item, _)| *item)
                    .find(|item| {
                        self.world
                            .item_data(*item)
                            .is_some_and(|data| matches!(data.data, ItemData::Weapon(_)))
                    })
                    .map(|item| (item, 0))
            });
        hidden.extend(held);
        let mut weapon_slot = 0;
        for (item, flag) in &hidden {
            let slot = self.hide(merchant, *item)?;
            if *flag != BasicFlags::WORN.0 {
                weapon_slot = slot;
            }
        }
        Ok(BarterSession {
            merchant,
            table: self.barter_table()?,
            offer: self.barter_table()?,
            hidden,
            weapon_slot,
        })
    }
    /// The `enhanced` WIELD button in a Fallout 2 party Trade window
    /// ([`WorldHost::can_wield_for`]). The member's hidden weapon first goes
    /// back to its inventory place, so the cycle
    /// ([`WorldHost::cycle_companion_weapon`]) runs over every weapon it
    /// carries; then §3.17's rule hides the weapon now held, or without one
    /// the first weapon, as on entry. Returns the weapon now held, or
    /// `None` when refused (under Fallout 1, and in a merchant's barter).
    pub fn barter_wield(
        &mut self,
        session: &mut BarterSession,
    ) -> Result<Option<Option<ObjectRef>>, WorldError> {
        let merchant = session.merchant;
        if self.world.fallout2.is_none() || !self.can_wield_for(merchant) {
            return Ok(None);
        }
        let mut held = None;
        let mut kept = Vec::new();
        for (item, flag) in std::mem::take(&mut session.hidden) {
            if flag == BasicFlags::WORN.0 {
                kept.push((item, flag));
                continue;
            }
            let count = self.world.get(merchant).map_or(0, |o| o.inventory.len());
            self.world.add_item(merchant, item, 1, true)?;
            // A unit that did not join a stack returns to its own place.
            if let Some(object) = self.world.get_mut(merchant)
                && object.inventory.len() > count
                && let Some(entry) = object.inventory.pop()
            {
                let slot = session.weapon_slot.min(object.inventory.len());
                object.inventory.insert(slot, entry);
            }
            if flag == BasicFlags::RIGHT_HAND.0 {
                held = Some(item);
            }
        }
        session.hidden = kept;
        let next = crate::companion::next_weapon(&self.companion_weapons(merchant), held);
        self.wield_companion(merchant, next)?;
        let hide = match next {
            Some(weapon) => Some((weapon, BasicFlags::RIGHT_HAND.0)),
            None => self
                .companion_weapons(merchant)
                .first()
                .map(|item| (*item, 0)),
        };
        if let Some((item, flag)) = hide {
            session.weapon_slot = self.hide(merchant, item)?;
            session.hidden.push((item, flag));
        }
        Ok(Some(next))
    }
    /// The held weapon a party Trade window's WIELD label names: the hidden
    /// right-hand item (§3.17 takes it out of the inventory).
    pub fn barter_held_weapon(&self, session: &BarterSession) -> Option<ObjectRef> {
        session
            .hidden
            .iter()
            .find(|(_, flag)| *flag == BasicFlags::RIGHT_HAND.0)
            .map(|(item, _)| *item)
    }
    /// Whether [`Self::open_party_trade`] may open on `member`, before its
    /// reach test.
    pub fn can_open_party_trade(&self, member: ObjectRef) -> bool {
        self.preset() == crate::EnginePreset::Enhanced
            && self.control_of(member).is_some()
            && self.fallout2_can_push(member)
    }
    /// The enhanced HUD pill's party Trade window under Fallout 2: SPEC-019
    /// §3.7's trade session opened on `member` without a conversation,
    /// under the conditions a conversation needs. Out of combat, a party
    /// member with a control record that is neither dead nor knocked out
    /// ([`WorldHost::fallout2_can_push`]'s test); at object distance 9 or
    /// more, or behind a shot-blocked line, the talk exception's
    /// `misc.msg` 2000 is printed instead (SPEC-007 §3.11.3, with no
    /// approach). `None` when refused.
    pub fn open_party_trade(
        &mut self,
        member: ObjectRef,
    ) -> Result<Option<BarterSession>, WorldError> {
        if !self.can_open_party_trade(member) {
            return Ok(None);
        }
        let player = self.world.player;
        let far =
            self.world.object_distance(player, member) >= crate::host_motion::TALK_RUN_DISTANCE;
        if far || self.world.shot(player, member).hard_obstruction {
            self.print_game("misc.msg", 2000, &[]);
            return Ok(None);
        }
        self.begin_barter(member).map(Some)
    }
    /// §3.17 effective modifier `M = M_s + M_r`. Safe rule: applied from
    /// the first frame (the reference's very first price uses M_s alone).
    pub fn barter_modifier(&mut self, session: &BarterSession) -> i32 {
        let reaction = self.reaction(session.merchant);
        self.world
            .dialogue
            .barter_modifier
            .saturating_add(reaction_modifier(reaction))
    }
    /// §3.18 price of the merchant's offer table.
    pub fn barter_price(&mut self, session: &BarterSession) -> i32 {
        let modifier = self.barter_modifier(session);
        let player = self.world.player;
        let skill = |host: &Self, critter: ObjectRef| {
            host.world
                .get(critter)
                .and_then(|object| object.character.as_ref())
                .map_or(0, |sheet| sheet.skill(15, host.world.context(critter)))
        };
        price(
            self.world.inventory_value(session.table),
            i64::from(self.world.caps(session.table)),
            modifier,
            self.world.perk(Perk::MasterTrader) > 0,
            skill(self, player),
            skill(self, session.merchant),
        )
    }
    /// Move `quantity` of an entry between an inventory and an offer table
    /// (tables never refuse; a move back into an inventory is forced).
    pub fn barter_move(
        &mut self,
        from: ObjectRef,
        to: ObjectRef,
        item: ObjectRef,
        quantity: i32,
    ) -> Result<(), WorldError> {
        self.world.remove_item(from, item, quantity)?;
        self.world.add_item(to, item, quantity, true)
    }
    /// Forced transfer of an owner's whole inventory.
    fn transfer_all(&mut self, from: ObjectRef, to: ObjectRef) -> Result<(), WorldError> {
        let entries = self
            .world
            .get(from)
            .map(|object| object.inventory.clone())
            .unwrap_or_default();
        for (item, quantity) in entries {
            self.barter_move(from, to, item, quantity)?;
        }
        Ok(())
    }
    /// §3.19 offer key: with either table non-empty, refuse an empty or
    /// armed player offer or a price above its plain value; otherwise the
    /// tables change hands, bypassing carry limits. Prints `inventry.msg`
    /// 27 on acceptance and 28 on refusal.
    pub fn attempt_trade(&mut self, session: &BarterSession) -> Result<Trade, WorldError> {
        let empty = |host: &Self, table: ObjectRef| {
            host.world
                .get(table)
                .is_none_or(|object| object.inventory.is_empty())
        };
        let offer_empty = empty(self, session.offer);
        if offer_empty && empty(self, session.table) {
            return Ok(Trade::Nothing);
        }
        // SPEC-019 §3.7: trading with a Fallout 2 party member that has a
        // control record always succeeds, without the price comparison.
        let trade = self.control_of(session.merchant).is_some();
        let accepted = trade
            || !offer_empty
                && !self.world.holds_armed(session.offer)
                && i64::from(self.barter_price(session))
                    <= self.world.inventory_value(session.offer);
        if !accepted {
            self.print_game("inventry.msg", 28, &[]);
            return Ok(Trade::Refused);
        }
        self.transfer_all(session.table, self.world.player)?;
        self.transfer_all(session.offer, session.merchant)?;
        self.print_game("inventry.msg", 27, &[]);
        Ok(Trade::Accepted)
    }
    /// Leaving barter returns T to the merchant and P to the player, then
    /// restores the hidden equipment (armor worn, the right-hand item in
    /// the right hand, a fallback weapon unequipped).
    pub fn end_barter(&mut self, session: BarterSession) -> Result<(), WorldError> {
        self.transfer_all(session.table, session.merchant)?;
        self.transfer_all(session.offer, self.world.player)?;
        for (item, flag) in session.hidden {
            self.world.add_item(session.merchant, item, 1, true)?;
            if flag != 0 {
                // The returned item may have joined a stack: only its own
                // unit goes back into the slot (SPEC-011 §3.13).
                let item = self.world.split_unit(item)?;
                self.world.equip(session.merchant, item, flag)?;
            }
        }
        self.world.erase(session.table);
        self.world.erase(session.offer);
        Ok(())
    }
}
