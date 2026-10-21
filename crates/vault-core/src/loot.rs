//! SPEC-013 §3.9 and §3.16: the container/corpse loot window and steal
//! mode as an interaction state machine. The presentation layer draws the
//! panes and drives presses, drops and keys; the session holds the rules.
//! Hidden equipment is a session filter, so inventories are never modified
//! to hide it (and no temporary holder object is created, the §3.16 safe
//! rule).
use crate::{
    LootRequest, ObjectRef, WorldError,
    assets::WorldAssets,
    critical::effect,
    host::WorldHost,
    host_drugs::Arg,
    skills::{self, StealExperience, StealInputs},
    stats::Perk,
    world::{ScriptEvent, ScriptRequest},
};
use vault_formats::{
    Fid,
    pro::{BasicFlags, Category, ItemData, PrototypeData},
};

const EQUIPPED: u32 = BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0 | BasicFlags::WORN.0;

/// One open loot window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootSession {
    pub looter: ObjectRef,
    /// The root target (the victim in steal mode).
    pub root: ObjectRef,
    pub steal: bool,
    /// N: presses on item slots in this session.
    pub attempts: i32,
    pub experience: StealExperience,
    pub caught: bool,
    hidden: Vec<ObjectRef>,
    /// Containers opened in each pane, outermost first (empty: the root).
    pub player_view: Vec<ObjectRef>,
    pub target_view: Vec<ObjectRef>,
    /// Stacked bodies to cycle through, and the current position.
    pub bodies: Vec<ObjectRef>,
    pub body: usize,
}
/// The outcome of one drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootMove {
    Moved,
    /// The capacity rules refused it (`inventry.msg` 25 or 26 shown).
    Refused,
    /// Caught stealing: the window must close at once.
    Caught,
}

impl LootSession {
    /// §3.9.2 a left press on a filled slot in the hand cursor: the attempt
    /// counter rises first, whatever the drag then does.
    pub fn press(&mut self) {
        if self.steal {
            self.attempts += 1;
        }
    }
    /// The container currently shown in a pane.
    pub fn shown(&self, target_side: bool) -> ObjectRef {
        let (view, root) = if target_side {
            (&self.target_view, self.root)
        } else {
            (&self.player_view, self.looter)
        };
        view.last().copied().unwrap_or(root)
    }
    /// Open a container in a pane, up to nine levels deep.
    pub fn open_container(&mut self, target_side: bool, container: ObjectRef) -> bool {
        let view = if target_side {
            &mut self.target_view
        } else {
            &mut self.player_view
        };
        if view.len() >= skills::MAX_NESTING {
            return false;
        }
        view.push(container);
        true
    }
    /// The portrait hot zone: up one container level.
    pub fn up(&mut self, target_side: bool) {
        if target_side {
            self.target_view.pop();
        } else {
            self.player_view.pop();
        }
    }
    /// Stacked-body cycling (PgUp/PgDn), when there is more than one body.
    pub fn cycle(&mut self, forward: bool) -> Option<ObjectRef> {
        if self.bodies.len() < 2 {
            return None;
        }
        let len = self.bodies.len();
        // A target missing from the list starts at the length, which wraps
        // to the first body on the first forward press.
        self.body = if forward {
            if self.body + 1 >= len {
                0
            } else {
                self.body + 1
            }
        } else {
            (self.body + len - 1) % len
        };
        self.root = self.bodies[self.body];
        self.target_view.clear();
        Some(self.root)
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Take the deferred loot request (for the presentation layer).
    pub fn take_loot_request(&mut self) -> Option<LootRequest> {
        self.world.pending_loot.take()
    }
    /// An active critter: a critter prototype without knocked out, dead or
    /// lose-turn results.
    fn active_critter(&self, critter: ObjectRef) -> bool {
        self.world
            .get(critter)
            .is_some_and(|object| object.category() == Category::Critter)
            && self.world.critter_state(critter).is_some_and(|state| {
                state.results.0 & (effect::KNOCKED_OUT | effect::DEAD | effect::LOSE_TURN) == 0
            })
    }
    /// §3.9.1 and §3.16 opening. Steal mode needs a critter thief and an
    /// active critter victim; otherwise the ordinary window opens. Refused
    /// for a non-player looter, a closed multi-frame container, a thief
    /// stealing from itself, or a `pickup_p_proc` override (ordinary only).
    pub fn open_loot(&mut self, request: LootRequest) -> Result<Option<LootSession>, WorldError> {
        let LootRequest {
            looter,
            target,
            steal,
        } = request;
        if looter != self.world.player || looter == target {
            return Ok(None);
        }
        let Some(object) = self.world.get(target) else {
            return Ok(None);
        };
        let closed_container = object.category() == Category::Item
            && object.raw.frame == 0
            && self
                .assets
                .object_art(object.raw.art_id)
                .is_ok_and(|art| art.frame_count() > 1);
        if closed_container {
            return Ok(None);
        }
        let steal = steal
            && self
                .world
                .get(looter)
                .is_some_and(|o| o.category() == Category::Critter)
            && self.active_critter(target);
        if !steal && let Some(script) = self.world.script_for(target) {
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::Pickup,
                source: looter,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
            if result.overridden {
                return Ok(None);
            }
        }
        let equipped = |host: &Self, owner: ObjectRef| -> Vec<ObjectRef> {
            host.world.get(owner).map_or(Vec::new(), |object| {
                object
                    .inventory
                    .iter()
                    .map(|(item, _)| *item)
                    .filter(|item| {
                        host.world
                            .get(*item)
                            .is_some_and(|o| o.raw.flags.0 & EQUIPPED != 0)
                    })
                    .collect()
            })
        };
        let mut hidden = equipped(self, looter);
        if steal {
            hidden.extend(equipped(self, target));
        }
        let mut bodies = Vec::new();
        let mut body = 0;
        if !steal
            && self
                .world
                .get(target)
                .is_some_and(|o| o.category() == Category::Critter)
        {
            let place = self
                .world
                .get(target)
                .and_then(|o| Some((o.raw.hex?, o.raw.elevation)));
            let mut list: Vec<_> = self
                .world
                .iter()
                .filter(|(_, o)| {
                    o.category() == Category::Critter
                        && o.raw.hex.zip(Some(o.raw.elevation)) == place
                })
                .map(|(reference, _)| reference)
                .collect();
            // Discarded entries are replaced by the current last entry.
            let mut index = 0;
            while index < list.len() {
                let fallen = self.world.critter_state(list[index]).is_some_and(|state| {
                    state.results.0 & (effect::DEAD | effect::KNOCKED_OUT) != 0
                });
                if fallen {
                    index += 1;
                } else {
                    list.swap_remove(index);
                }
            }
            if list.len() > 1 {
                body = list.iter().position(|b| *b == target).unwrap_or(list.len());
                bodies = list;
            }
        }
        Ok(Some(LootSession {
            looter,
            root: target,
            steal,
            attempts: 0,
            experience: StealExperience::default(),
            caught: false,
            hidden,
            player_view: Vec::new(),
            target_view: Vec::new(),
            bodies,
            body,
        }))
    }
    /// The entries a pane lists: the shown container's inventory without
    /// the session's hidden equipment.
    pub fn loot_items(&self, session: &LootSession, target_side: bool) -> Vec<(ObjectRef, i32)> {
        self.world
            .get(session.shown(target_side))
            .map_or(Vec::new(), |object| {
                object
                    .inventory
                    .iter()
                    .copied()
                    .filter(|(item, _)| !session.hidden.contains(item))
                    .collect()
            })
    }
    /// §3.9.3 the steal check for moving `item` against the root victim
    /// (the safe rule for every check of the session).
    fn loot_steal_check(
        &mut self,
        session: &LootSession,
        item: ObjectRef,
    ) -> Result<bool, WorldError> {
        let thief = session.looter;
        let victim = session.root;
        let context = self.world.context(thief);
        let sheet = self.world.get(thief).and_then(|o| o.character.as_ref());
        let thief_steal = sheet.map_or(0, |sheet| {
            sheet.skill(skills::skill::STEAL as usize, context)
        });
        let facing = |object: ObjectRef| self.world.get(object).map_or(0, |o| o.raw.orientation);
        let victim_object = self.world.get(victim);
        let victim_critter = victim_object.is_some_and(|o| Fid(o.raw.art_id).category() == 1);
        let victim_steal = victim_object
            .filter(|o| o.category() == Category::Critter)
            .and_then(|o| o.character.as_ref())
            .map(|sheet| sheet.skill(skills::skill::STEAL as usize, self.world.context(victim)));
        let inputs = StealInputs {
            attempts: session.attempts,
            item_size: self.world.item_data(item).map_or(0, |data| data.size),
            thief_steal,
            thief_critical: self.world.effective(thief, 15),
            pickpocket: thief == self.world.player && self.world.perk(Perk::Pickpocket) > 0,
            victim_critter,
            in_front: skills::in_front(facing(thief), facing(victim)),
            victim_prone: self.world.critter_state(victim).is_some_and(|state| {
                state.results.0 & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0
            }),
            party_member: thief == self.world.player && self.world.party.contains(&victim),
            victim_steal,
        };
        let result = skills::steal_check(&inputs, self.world.clock.ticks, &mut self.world.rng)?;
        Ok(result.caught)
    }
    /// §3.9.2/§3.16 a drop of `quantity` of `item` from one pane into the
    /// other's shown container: the steal check in steal mode, then the
    /// move under the capacity rules (refusals show `inventry.msg` 26 into
    /// the target, 25 into the player).
    pub fn loot_move(
        &mut self,
        session: &mut LootSession,
        from_target: bool,
        item: ObjectRef,
        quantity: i32,
    ) -> Result<LootMove, WorldError> {
        let source = session.shown(from_target);
        let destination = session.shown(!from_target);
        let party = self.world.party.contains(&session.root);
        if session.steal {
            // SPEC-013 §3.9.3 steps 5-6, §3.9.5: a party member's move is a
            // critical success with no draws and never a catch; step 8's
            // 571/573 message is still displayed.
            let caught = !party && self.loot_steal_check(session, item)?;
            let name = self
                .world
                .get(item)
                .map(|o| o.name.clone())
                .unwrap_or_default();
            let id = match (from_target, caught) {
                (true, true) => 570,
                (true, false) => 571,
                (false, true) => 572,
                (false, false) => 573,
            };
            self.print_game("skill.msg", id, &[Arg::Text(&name)]);
            if caught {
                session.caught = true;
                return Ok(LootMove::Caught);
            }
        }
        self.world.remove_item(source, item, quantity)?;
        if self
            .world
            .add_item(destination, item, quantity, false)
            .is_err()
        {
            self.world.add_item(source, item, quantity, true)?;
            self.print_game("inventry.msg", if from_target { 25 } else { 26 }, &[]);
            return Ok(LootMove::Refused);
        }
        if session.steal && !party {
            session.experience.success();
        }
        Ok(LootMove::Moved)
    }
    /// §3.16 Take All (Shift+A, non-steal): everything of the root target
    /// moves to the player when its total weight fits the remaining carry
    /// capacity; a refusal is silent.
    pub fn loot_take_all(&mut self, session: &LootSession) -> Result<bool, WorldError> {
        if session.steal {
            return Ok(false);
        }
        let looter = session.looter;
        let limit = i64::from(self.world.effective(looter, 12));
        let fits = self.world.inventory_weight(session.root)
            <= limit - self.world.inventory_weight(looter);
        if !fits {
            return Ok(false);
        }
        let entries: Vec<_> = self
            .world
            .get(session.root)
            .map_or(Vec::new(), |o| o.inventory.clone());
        for (item, quantity) in entries {
            self.world.remove_item(session.root, item, quantity)?;
            self.world.add_item(looter, item, quantity, true)?;
        }
        Ok(true)
    }
    /// §3.16 closing: steal experience when not caught (`inventry.msg` 29
    /// with the award before Swift Learner; safe rule: only a positive
    /// award), then, when caught with N positive, the victim's
    /// `pickup_p_proc` with the player as source.
    pub fn close_loot(&mut self, session: LootSession) -> Result<(), WorldError> {
        let victim = session.root;
        if session.steal && !session.caught && !self.world.party.contains(&victim) {
            let context = self.world.context(session.looter);
            let steal = self
                .world
                .get(session.looter)
                .and_then(|o| o.character.as_ref())
                .map_or(0, |sheet| {
                    sheet.skill(skills::skill::STEAL as usize, context)
                });
            if let Some(award) = session.experience.award(steal) {
                self.give_experience(award);
                self.print_game("inventry.msg", 29, &[Arg::Int(i64::from(award))]);
            }
        }
        if session.steal
            && session.caught
            && session.attempts > 0
            && let Some(script) = self.world.script_for(victim)
        {
            self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::Pickup,
                source: session.looter,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            })?;
        }
        self.drain_requests()
    }
    /// §3.16 container use from the map: a closed container (frame 0) posts
    /// a loot request after its use step; an open one only closes.
    pub fn loot_container(&mut self, looter: ObjectRef, container: ObjectRef, was_closed: bool) {
        let item_213 = self.world.get(container).is_some_and(|o| o.raw.pid == 213);
        let is_container = self.world.get(container).is_some_and(|o| {
            o.prototype.as_ref().is_some_and(|p| {
                matches!(&p.data, PrototypeData::Item(item) if matches!(item.data, ItemData::Container { .. }))
            })
        });
        if is_container && (was_closed || item_213) {
            self.world.pending_loot = Some(LootRequest {
                looter,
                target: container,
                steal: false,
            });
        }
    }
}
