//! The autoplay bot, over the synthetic campaign: `execute_bot`'s rules, the
//! agent's determinism, the tactician, the goal spine, and the chaos audit,
//! whose every check is also made to fail.
use crate::{
    CharacterSheet, EnginePreset, GameProfile, ObjectRef, WorldEffect, WorldHost, WorldState,
    assets::VfsWorldAssets,
    autoplay::{AutoplayAgent, BotAction, BotOutcome, BotScreen, BotStatus, BotVerb, BotView},
    critical::{hit_mode, location},
    dialogue::{DialogueOption, DialogueText},
    fallout2::Fallout2World,
    host_choreography::Playback,
    host_save::SaveInfo,
    queue::EventKind,
    skills::{self, skill},
    test_support::{dir_assets, hp, patched, set_hp},
};
use vault_fixtures::campaign::{self, Edition};
use vault_formats::{
    AutomapDb, HexGrid,
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, LockFlags},
};

type Host = WorldHost<VfsWorldAssets>;

fn view() -> BotView {
    BotView {
        input_open: true,
        ..BotView::default()
    }
}

/// The campaign's first map.
fn campaign_host(edition: Edition) -> (tempfile::TempDir, Host) {
    seeded_host(edition, 1)
}

fn seeded_host(edition: Edition, seed: i32) -> (tempfile::TempDir, Host) {
    let dir = tempfile::tempdir().unwrap();
    vault_fixtures::state::write_corpus(dir.path());
    campaign::write_campaign(dir.path(), edition);
    let world = WorldState::new(CharacterSheet::default(), seed).unwrap();
    let mut host = WorldHost::new(world, dir_assets(dir.path()));
    host.interactive_dialogue = true;
    host.load_campaign().unwrap();
    if edition == Edition::Fallout2 {
        host.set_game_profile(GameProfile::Fallout2);
        let tables = host.load_fallout2_tables().unwrap();
        let start = tables.maps.match_name(b"artemple.map");
        host.world.persistence.automap =
            AutomapDb::fallout2(|map| tables.maps.excluded(map as i32));
        host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
    }
    host.load_map(&campaign::start(edition)).unwrap();
    (dir, host)
}

fn object(host: &Host, id: u32) -> ObjectRef {
    host.world.reference_for_id(id).expect("object")
}

fn ap(host: &Host) -> i32 {
    host.world.action_points(host.world.player)
}

fn hex(host: &Host) -> i32 {
    host.world.get(host.world.player).unwrap().raw.hex.unwrap()
}

fn lock(host: &Host, id: u32) -> LockFlags {
    host.world.get(object(host, id)).unwrap().lock().unwrap()
}

fn refused(outcome: Result<BotOutcome, crate::WorldError>) -> String {
    match outcome.unwrap() {
        BotOutcome::Refused(reason) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn done(host: &mut Host, action: BotAction) {
    assert_eq!(
        host.execute_bot(&action),
        Ok(BotOutcome::Done),
        "{action:?}"
    );
}

/// Everything on the ground in `ids` goes into the pack.
fn carry(host: &mut Host, ids: &[u32]) {
    for id in ids {
        let item = object(host, *id);
        let player = host.world.player;
        host.world.add_item(player, item, 1, false).unwrap();
    }
}

/// The player holds the pistol.
fn arm_player(host: &mut Host) {
    carry(host, &[campaign::PISTOL]);
    done(
        host,
        BotAction::Equip {
            item: campaign::PISTOL,
        },
    );
}

fn set_rounds(host: &mut Host, rounds: i32) {
    let pistol = object(host, campaign::PISTOL);
    let object = host.world.get_mut(pistol).unwrap();
    object.raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Weapon {
            rounds,
            ammo_pid: 0x11,
        },
    };
}

fn set_ap(host: &mut Host, points: i32) {
    let player = host.world.player;
    host.world.critter_state_mut(player).unwrap().ap = points;
}

/// A fight with the foe, on another team, `distance` hexes north of the
/// player, on the player's turn.
fn fight(host: &mut Host, distance: i32) -> ObjectRef {
    let foe = object(host, campaign::FOE);
    host.world.critter_state_mut(foe).unwrap().team = 1;
    let at = hex(host) - 200 * distance;
    host.world.move_object(foe, at, 0, false).unwrap();
    let player = host.world.player;
    host.start_combat(player, foe).unwrap();
    let mut now = 1000;
    while !host.world.combat_state.player_turn || host.player_turn_over() {
        now += 16;
        host.tick(now).unwrap();
        assert!(now < 60_000, "the player's turn never came");
    }
    foe
}

/// A bot in a host, frame by frame.
struct Run {
    host: Host,
    agent: AutoplayAgent,
    now: u64,
    actions: Vec<BotAction>,
    view: BotView,
    _dir: tempfile::TempDir,
}

impl Run {
    fn new(edition: Edition, seed: i32, chaos: bool) -> Self {
        Self::in_world(edition, 1, seed, chaos)
    }
    fn in_world(edition: Edition, world: i32, seed: i32, chaos: bool) -> Self {
        let (dir, host) = seeded_host(edition, world);
        Self {
            host,
            agent: AutoplayAgent::new(seed, chaos).unwrap(),
            now: 100_000,
            actions: Vec::new(),
            view: view(),
            _dir: dir,
        }
    }
    fn status(&self) -> &BotStatus {
        self.agent.status()
    }
    fn decide(&mut self) -> Option<BotAction> {
        self.agent.decide(&self.host, &self.view)
    }
    /// One decision, its action and one tick.
    fn frame(&mut self) -> Option<BotAction> {
        let action = self.decide();
        if let Some(action) = &action {
            self.actions.push(action.clone());
            let outcome = self.host.execute_bot(action).unwrap();
            self.agent.note_outcome(&outcome);
        }
        self.now += 16;
        self.host.tick(self.now).unwrap();
        action
    }
    fn until(&mut self, limit: usize, done: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            if done(self) {
                return;
            }
            self.frame();
        }
        panic!(
            "not after {limit} frames: {:#?}, last actions {:?}",
            self.status(),
            self.actions.iter().rev().take(5).collect::<Vec<_>>()
        );
    }
    fn did(&self, what: impl Fn(&BotAction) -> bool) -> bool {
        self.actions.iter().any(what)
    }
    fn armed(&self) -> Vec<u32> {
        self.actions
            .iter()
            .filter_map(|action| match action {
                BotAction::ArmExplosive { item, .. } => Some(*item),
                _ => None,
            })
            .collect()
    }
    /// The audit's first bomb is out and burning.
    fn until_armed(&mut self) -> ObjectRef {
        self.until(3000, |run| !run.armed().is_empty());
        object(&self.host, self.armed()[0])
    }
    fn until_explosion(&mut self) {
        self.until(1500, |run| run.host.world.explosion.is_some());
    }
    /// The audit's first bomb, on the scripted door, has gone off.
    fn until_blown(&mut self) {
        self.until(3000, |run| {
            run.host
                .world
                .reference_for_id(campaign::SCRIPTED_DOOR)
                .is_none()
                && run.host.world.explosion.is_none()
        });
        self.frame();
    }
    /// The audit of the first map is over: the bot moves on.
    fn until_audited(&mut self) {
        self.until(6000, |run| {
            run.status().goal.starts_with("Leaving by the exit grid")
                || run.host.world.pending_transition.is_some()
        });
    }
    fn violation(&self) -> String {
        self.status().violation.clone().unwrap_or_default()
    }
    fn expect_violation(&mut self, text: &str) {
        self.until(3000, |run| run.status().violation.is_some());
        assert!(self.violation().contains(text), "{}", self.violation());
    }
    /// The bot's action on the plain door has gone out `count` times.
    fn until_swung(&mut self, count: usize) {
        self.until(400, |run| {
            run.actions
                .iter()
                .filter(|action| {
                    matches!(action, BotAction::Interact { target, verb: BotVerb::Use }
                        if *target == campaign::PLAIN_DOOR)
                })
                .count()
                == count
        });
    }
}

#[test]
fn an_agent_starts_idle() {
    let agent = AutoplayAgent::new(42, false).unwrap();
    assert_eq!(agent.status().actions, 0);
}

// `execute_bot`.

#[test]
fn screen_actions_are_the_shells() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    for action in [
        BotAction::TravelTo { place: 0 },
        BotAction::EnterHere,
        BotAction::TownEntrance { index: 0 },
        BotAction::CloseScreen,
        BotAction::DismissDialog,
    ] {
        assert_eq!(host.execute_bot(&action), Ok(BotOutcome::NeedsShell));
    }
}

#[test]
fn an_object_that_is_gone_is_refused_and_nothing_changes() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let before = host.world.state_hash();
    let gone = 999_999;
    for action in [
        BotAction::Attack {
            target: gone,
            hit_mode: hit_mode::PUNCH,
            location: None,
        },
        BotAction::Reload { weapon: gone },
        BotAction::UseItem { item: gone },
        BotAction::UseItemOn {
            item: gone,
            target: campaign::PLAIN_DOOR,
        },
        BotAction::UseSkill {
            skill: skill::LOCKPICK,
            target: gone,
        },
        BotAction::Interact {
            target: gone,
            verb: BotVerb::Use,
        },
        BotAction::ArmExplosive {
            item: gone,
            seconds: 10,
        },
        BotAction::Equip { item: gone },
    ] {
        let reason = refused(host.execute_bot(&action));
        assert!(reason.contains("999999"), "{reason}");
    }
    assert_eq!(host.world.state_hash(), before);
}

#[test]
fn what_lies_on_the_ground_is_not_in_the_pack() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    for action in [
        BotAction::Reload {
            weapon: campaign::PISTOL,
        },
        BotAction::UseItem {
            item: campaign::DRUG,
        },
        BotAction::ArmExplosive {
            item: campaign::DYNAMITE,
            seconds: 10,
        },
        BotAction::Equip {
            item: campaign::PISTOL,
        },
    ] {
        assert!(refused(host.execute_bot(&action)).contains("carries no object"));
    }
}

#[test]
fn a_walk_takes_the_clicks_path() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let goal = hex(&host) + 4;
    done(
        &mut host,
        BotAction::Walk {
            hex: goal,
            run: false,
        },
    );
    assert_eq!(hex(&host), goal);
    let off = refused(host.execute_bot(&BotAction::Walk {
        hex: 40000,
        run: false,
    }));
    assert!(off.contains("off the map"), "{off}");
}

#[test]
fn a_dialogue_or_the_combat_lock_stops_world_actions() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let walk = BotAction::Walk {
        hex: hex(&host) + 4,
        run: false,
    };
    host.world.dialogue.open = true;
    assert!(refused(host.execute_bot(&walk)).contains("dialogue"));
    host.world.dialogue.open = false;
    fight(&mut host, 3);
    host.world.combat_state.player_turn = false;
    host.combat_lock.end_turn();
    assert!(refused(host.execute_bot(&walk)).contains("locked"));
    assert!(refused(host.execute_bot(&BotAction::EndTurn)).contains("locked"));
}

#[test]
fn an_attack_out_of_combat_asks_for_combat() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    arm_player(&mut host);
    set_rounds(&mut host, 5);
    let foe = object(&host, campaign::FOE);
    let at = hex(&host) - 400;
    host.world.move_object(foe, at, 0, false).unwrap();
    done(
        &mut host,
        BotAction::Attack {
            target: campaign::FOE,
            hit_mode: hit_mode::RIGHT_PRIMARY,
            location: None,
        },
    );
    assert!(host.world.pending_combat.is_some());
    assert!(!host.world.combat);
}

#[test]
fn an_attack_spends_its_cost_and_an_aimed_one_a_point_more() {
    for (place, cost) in [(None, 4), (Some(location::HEAD), 5)] {
        let (_dir, mut host) = campaign_host(Edition::Fallout1);
        arm_player(&mut host);
        set_rounds(&mut host, 5);
        fight(&mut host, 2);
        let before = ap(&host);
        done(
            &mut host,
            BotAction::Attack {
                target: campaign::FOE,
                hit_mode: hit_mode::RIGHT_PRIMARY,
                location: place,
            },
        );
        assert_eq!(ap(&host), before - cost, "{place:?}");
        let pistol = object(&host, campaign::PISTOL);
        let rounds = host.world.weapon_profile(pistol, hit_mode::RIGHT_PRIMARY);
        assert_eq!(rounds.unwrap().rounds, 4, "{place:?}");
    }
}

#[test]
fn a_refused_attack_changes_nothing_and_says_why() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    arm_player(&mut host);
    set_rounds(&mut host, 0);
    fight(&mut host, 2);
    let (hash, points) = (host.world.state_hash(), ap(&host));
    let attack = |target, hit_mode| BotAction::Attack {
        target,
        hit_mode,
        location: None,
    };
    let shot = attack(campaign::FOE, hit_mode::RIGHT_PRIMARY);
    assert!(refused(host.execute_bot(&shot)).contains("NoAmmunition"));
    let wrong = attack(campaign::PLAIN_DOOR, hit_mode::RIGHT_PRIMARY);
    assert!(refused(host.execute_bot(&wrong)).contains("no critter"));
    let reload = attack(campaign::FOE, hit_mode::RIGHT_RELOAD);
    assert!(refused(host.execute_bot(&reload)).contains("no attack"));
    assert_eq!(host.world.state_hash(), hash);
    assert_eq!(ap(&host), points);
}

#[test]
fn a_reload_spends_two_points_and_needs_ammunition() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    arm_player(&mut host);
    set_rounds(&mut host, 0);
    fight(&mut host, 2);
    let reload = BotAction::Reload {
        weapon: campaign::PISTOL,
    };
    assert!(refused(host.execute_bot(&reload)).contains("no ammunition"));
    carry(&mut host, &[campaign::AMMUNITION]);
    let before = ap(&host);
    set_ap(&mut host, 1);
    assert!(refused(host.execute_bot(&reload)).contains("short of AP"));
    set_ap(&mut host, before);
    done(&mut host, reload.clone());
    assert_eq!(ap(&host), before - 2);
    let pistol = object(&host, campaign::PISTOL);
    assert!(
        host.world
            .weapon_profile(pistol, hit_mode::RIGHT_PRIMARY)
            .unwrap()
            .rounds
            > 0
    );
}

#[test]
fn an_item_is_used_as_the_slot_uses_it() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    carry(&mut host, &[campaign::DRUG, campaign::DYNAMITE]);
    fight(&mut host, 2);
    let before = ap(&host);
    done(
        &mut host,
        BotAction::UseItem {
            item: campaign::DRUG,
        },
    );
    assert_eq!(ap(&host), before - 2);
    let explosive = BotAction::UseItem {
        item: campaign::DYNAMITE,
    };
    assert!(refused(host.execute_bot(&explosive)).contains("window"));
    assert_eq!(ap(&host), before - 2);
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    carry(&mut host, &[campaign::DRUG]);
    fight(&mut host, 2);
    set_ap(&mut host, 1);
    let used = BotAction::UseItem {
        item: campaign::DRUG,
    };
    assert!(refused(host.execute_bot(&used)).contains("short of AP"));
}

#[test]
fn a_skill_on_a_door_only_tries() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let before = lock(&host, campaign::LOCKED_DOOR);
    done(
        &mut host,
        BotAction::UseSkill {
            skill: skill::LOCKPICK,
            target: campaign::LOCKED_DOOR,
        },
    );
    assert_eq!(lock(&host, campaign::LOCKED_DOOR), before);
    let unknown = BotAction::UseSkill {
        skill: 99,
        target: campaign::LOCKED_DOOR,
    };
    assert!(refused(host.execute_bot(&unknown)).contains("skill 99"));
}

#[test]
fn a_use_on_a_door_needs_the_item_to_permit_it() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    carry(&mut host, &[campaign::PISTOL]);
    let action = BotAction::UseItemOn {
        item: campaign::PISTOL,
        target: campaign::PLAIN_DOOR,
    };
    let reason = refused(host.execute_bot(&action));
    assert!(reason.contains("permits no use"), "{reason}");
}

#[test]
fn arming_drops_the_armed_unit_and_a_stack_keeps_the_rest() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let player = host.world.player;
    let dynamite = object(&host, campaign::DYNAMITE);
    host.world.add_item(player, dynamite, 3, false).unwrap();
    for seconds in [0, 5, 190] {
        let action = BotAction::ArmExplosive {
            item: campaign::DYNAMITE,
            seconds,
        };
        assert!(refused(host.execute_bot(&action)).contains("timer"));
    }
    carry(&mut host, &[campaign::PISTOL]);
    let not_one = BotAction::ArmExplosive {
        item: campaign::PISTOL,
        seconds: 10,
    };
    assert!(refused(host.execute_bot(&not_one)).contains("no explosive"));
    let at = hex(&host);
    done(
        &mut host,
        BotAction::ArmExplosive {
            item: campaign::DYNAMITE,
            seconds: 10,
        },
    );
    let armed = host.world.get(dynamite).unwrap();
    assert_eq!(armed.raw.pid, skills::DYNAMITE_ARMED);
    assert!(armed.raw.flags.contains(BasicFlags::USED));
    assert_eq!((armed.owner, armed.raw.hex), (None, Some(at)));
    assert_eq!(host.world.inventory_count(player, skills::DYNAMITE), 2);
    assert!(
        host.world
            .queue
            .events()
            .any(|event| event.object == dynamite && matches!(event.kind, EventKind::Other(_)))
    );
    // Its id now names a thing on the ground: nothing arms it twice.
    let again = BotAction::ArmExplosive {
        item: campaign::DYNAMITE,
        seconds: 10,
    };
    assert!(refused(host.execute_bot(&again)).contains("carries no object"));
}

#[test]
fn arming_in_combat_costs_the_slots_use() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    carry(&mut host, &[campaign::PLASTIC]);
    fight(&mut host, 2);
    let before = ap(&host);
    done(
        &mut host,
        BotAction::ArmExplosive {
            item: campaign::PLASTIC,
            seconds: 180,
        },
    );
    assert_eq!(ap(&host), before - 2);
}

#[test]
fn dialogue_choices_are_checked_and_esc_ends_a_stalled_session() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    let choice = BotAction::Dialogue { choice: Some(0) };
    assert!(refused(host.execute_bot(&choice)).contains("no dialogue"));
    done(
        &mut host,
        BotAction::Interact {
            target: campaign::FRIEND,
            verb: BotVerb::Talk,
        },
    );
    assert!(host.world.dialogue.open);
    let options = host.world.dialogue.options.len();
    assert!(options > 0);
    let wrong = BotAction::Dialogue {
        choice: Some(options),
    };
    assert!(refused(host.execute_bot(&wrong)).contains("no option"));
    let walk = BotAction::Walk {
        hex: hex(&host) + 4,
        run: false,
    };
    assert!(refused(host.execute_bot(&walk)).contains("dialogue"));
    host.world.dialogue.waiting = false;
    assert!(refused(host.execute_bot(&choice)).contains("not waiting"));
    done(&mut host, BotAction::Dialogue { choice: None });
    assert!(!host.world.dialogue.open);
}

#[test]
fn ending_a_turn_and_a_combat_go_by_the_rules() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    assert!(refused(host.execute_bot(&BotAction::EndCombat)).contains("no combat"));
    assert!(refused(host.execute_bot(&BotAction::EndTurn)).contains("not the player's turn"));
    fight(&mut host, 2);
    done(&mut host, BotAction::EndTurn);
    assert!(host.world.combat_state.turn_ended);
}

#[test]
fn a_weapon_goes_in_the_active_hand_out_of_combat_only() {
    let (_dir, mut host) = campaign_host(Edition::Fallout1);
    carry(&mut host, &[campaign::PISTOL]);
    let equip = BotAction::Equip {
        item: campaign::PISTOL,
    };
    done(&mut host, equip.clone());
    let pistol = object(&host, campaign::PISTOL);
    let player = host.world.player;
    assert_eq!(
        host.world.wielded(player, hit_mode::RIGHT_PRIMARY),
        Some(pistol)
    );
    fight(&mut host, 2);
    assert!(refused(host.execute_bot(&equip)).contains("combat"));
}

// The agent.

#[test]
fn two_agents_with_one_seed_choose_the_same_actions() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let mut logs = Vec::new();
        for _ in 0..2 {
            let mut run = Run::new(edition, 7, true);
            for _ in 0..2500 {
                run.frame();
            }
            assert!(run.actions.len() > 20, "{:?}", run.status());
            logs.push((run.actions, run.host.world.state_hash()));
        }
        assert_eq!(logs[0], logs[1], "{edition:?}");
    }
}

#[test]
fn two_seeds_explore_differently() {
    let mut logs = Vec::new();
    for seed in [7, 8] {
        let mut run = Run::new(Edition::Fallout1, seed, false);
        for _ in 0..200 {
            run.frame();
        }
        logs.push(run.actions);
    }
    assert_ne!(logs[0], logs[1]);
}

#[test]
fn deciding_changes_nothing_and_draws_nothing_from_the_world() {
    let mut run = Run::new(Edition::Fallout1, 7, true);
    for _ in 0..2500 {
        let (hash, rng, clock) = (
            run.host.world.state_hash(),
            run.host.world.rng.state().clone(),
            run.host.world.clock,
        );
        let action = run.decide();
        assert_eq!(run.host.world.state_hash(), hash);
        assert_eq!(run.host.world.rng.state(), &rng);
        assert_eq!(run.host.world.clock, clock);
        if let Some(action) = action {
            run.host.execute_bot(&action).unwrap();
        }
        run.now += 16;
        run.host.tick(run.now).unwrap();
    }
}

#[test]
fn an_action_the_rules_refuse_is_counted_when_the_agent_is_told() {
    let mut agent = AutoplayAgent::new(1, false).unwrap();
    agent.note_outcome(&BotOutcome::Done);
    agent.note_outcome(&BotOutcome::Refused("no".into()));
    assert_eq!(agent.status().refusals, 1);
}

#[test]
fn nothing_happens_while_the_session_takes_no_input() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.input_open = false;
    assert_eq!(run.frame(), None);
    assert_eq!(run.frame(), None);
    assert_eq!(run.status().stuck, 0);
}

#[test]
fn a_dialog_box_is_dismissed_first_and_other_screens_are_closed() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.dialog = true;
    run.view.input_open = false;
    assert_eq!(run.decide(), Some(BotAction::DismissDialog));
    run.view.dialog = false;
    run.view.input_open = true;
    for screen in [
        BotScreen::Movie,
        BotScreen::Inventory,
        BotScreen::Loot,
        BotScreen::Barter,
        BotScreen::Elevator,
        BotScreen::Other,
    ] {
        run.view.screen = screen;
        assert_eq!(run.decide(), Some(BotAction::CloseScreen), "{screen:?}");
    }
}

#[test]
fn a_screen_that_will_not_close_is_not_asked_every_frame() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.screen = BotScreen::Other;
    let mut asked = 0;
    for _ in 0..80 {
        asked += usize::from(run.decide().is_some());
    }
    assert!((2..=20).contains(&asked), "{asked}");
    assert!(run.status().stuck > 60);
}

#[test]
fn the_world_map_heads_for_a_known_town_then_enters_it() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.screen = BotScreen::WorldMap;
    run.host.world.world_map.saved.towns |= 0b110;
    let Some(BotAction::TravelTo { place }) = run.decide() else {
        panic!("no heading");
    };
    assert_ne!(place, 0, "the party stands in town 0");
    // Travel is under way: wait.
    let travel = crate::worldmap::Travel {
        destination: [9, 9],
        stepper: crate::worldmap::Stepper::new([0, 0], [9, 9]),
    };
    run.host.world.world_map.travel = Some(travel);
    assert_eq!(run.decide(), None);
    assert!(run.status().goal.contains("Crossing"));
    run.host.world.world_map.travel = None;
    // Stopped short: head on.
    assert_eq!(run.decide(), Some(BotAction::TravelTo { place }));
    // There: enter.
    let center = crate::worldmap::town_center(place).unwrap();
    run.host.world.world_map.set_position(center);
    assert_eq!(run.decide(), Some(BotAction::EnterHere));
}

#[test]
fn a_town_it_cannot_reach_is_given_up() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.screen = BotScreen::WorldMap;
    run.host.world.world_map.saved.towns |= 0b10;
    for _ in 0..3 {
        assert_eq!(run.decide(), Some(BotAction::TravelTo { place: 1 }));
    }
    // Only that town is known, and it heads there once more, on a fresh try.
    assert_eq!(run.decide(), Some(BotAction::TravelTo { place: 1 }));
}

#[test]
fn the_fallout2_world_map_lists_open_cities() {
    let mut run = Run::new(Edition::Fallout2, 7, false);
    run.view.screen = BotScreen::WorldMap;
    let cities = run.host.fallout2_known_cities();
    let here = run.host.fallout2_party_city();
    match run.decide() {
        Some(BotAction::TravelTo { place }) => {
            assert!(cities.contains(&place) && Some(place) != here);
        }
        Some(BotAction::EnterHere) => assert!(cities.iter().all(|city| Some(*city) == here)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn town_markers_are_tried_in_turn_then_the_screen_closes() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.view.screen = BotScreen::TownMap;
    for index in 0..8 {
        assert_eq!(run.decide(), Some(BotAction::TownEntrance { index }));
    }
    assert_eq!(run.decide(), Some(BotAction::CloseScreen));
}

#[test]
fn dialogue_takes_branches_it_has_not_taken() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let dialogue = &mut run.host.world.dialogue;
    dialogue.open = true;
    dialogue.waiting = true;
    dialogue.resolved_reply = b"Which way?".to_vec();
    dialogue.options = (0..3)
        .map(|_| DialogueOption {
            text: DialogueText::Literal(Vec::new()),
            procedure: 0,
            reaction: 50,
        })
        .collect();
    let mut chosen = Vec::new();
    for _ in 0..3 {
        match run.decide() {
            Some(BotAction::Dialogue {
                choice: Some(index),
            }) => chosen.push(index),
            other => panic!("{other:?}"),
        }
    }
    chosen.sort_unstable();
    assert_eq!(chosen, [0, 1, 2]);
    // No option left at all: out.
    run.host.world.dialogue.options.clear();
    assert_eq!(run.decide(), Some(BotAction::Dialogue { choice: None }));
}

#[test]
fn a_stalled_dialogue_is_left_once_it_is_stuck() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.host.world.dialogue.open = true;
    let mut left = None;
    for frame in 0..40 {
        if run.decide().is_some() {
            left = Some(frame);
            break;
        }
    }
    assert!(left.is_some_and(|frame| frame >= 12), "{left:?}");
}

#[test]
fn the_spine_takes_what_lies_around_arms_itself_and_talks() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.until(400, |run| {
        run.did(|action| {
            matches!(
                action,
                BotAction::Interact {
                    verb: BotVerb::Talk,
                    ..
                }
            )
        })
    });
    let player = run.host.world.player;
    for id in [
        campaign::PISTOL,
        campaign::AMMUNITION,
        campaign::DYNAMITE,
        campaign::PLASTIC,
        campaign::DRUG,
    ] {
        let item = object(&run.host, id);
        assert_eq!(
            run.host.world.get(item).unwrap().owner,
            Some(player),
            "{id}"
        );
    }
    let pistol = object(&run.host, campaign::PISTOL);
    assert_eq!(
        run.host.world.wielded(player, hit_mode::RIGHT_PRIMARY),
        Some(pistol)
    );
    // The container stays where it is: it is no item to carry.
    let container = object(&run.host, campaign::CONTAINER);
    assert_eq!(run.host.world.get(container).unwrap().owner, None);
}

#[test]
fn the_spine_shows_its_route() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let start = hex(&run.host);
    run.decide().unwrap();
    let status = run.status();
    assert_eq!(status.route.first(), Some(&start));
    assert!(status.route.len() > 1);
    assert!(status.target_hex.is_some());
    assert!(status.goal.contains("Picking up"), "{}", status.goal);
}

#[test]
fn the_spine_walks_the_campaign_to_the_world_map() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let mut run = Run::new(edition, 7, false);
        run.until(8000, |run| {
            run.host
                .world
                .effects
                .iter()
                .any(|effect| matches!(effect, WorldEffect::WorldMap))
        });
        assert_eq!(run.status().violation, None);
    }
}

/// The player's elevation.
fn elevation(host: &Host) -> u8 {
    host.world.get(host.world.player).unwrap().raw.elevation
}

/// A second host holding what `host` saves, loaded: `host` and it must agree
/// on where the player stands, and a load of that load on the hash.
fn reloaded(host: &mut Host, corpus: &std::path::Path, edition: Edition) {
    let mut second = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        dir_assets(corpus),
    );
    second.load_campaign().unwrap();
    if edition == Edition::Fallout2 {
        second.set_game_profile(GameProfile::Fallout2);
        let tables = second.load_fallout2_tables().unwrap();
        let start = tables.maps.match_name(b"artemple.map");
        second.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
    }
    let place = |host: &Host| {
        let player = host.world.get(host.world.player).unwrap();
        (player.raw.hex, player.raw.elevation, host.world.elevation)
    };
    let mut image = host.slot_image(&SaveInfo::default()).unwrap();
    second.world.rng = host.world.rng.clone();
    second.load_slot(&mut image).unwrap();
    assert_eq!(place(&second), place(host), "{edition:?}");
    let first = second.world.state_hash();
    let mut again = second.slot_image(&SaveInfo::default()).unwrap();
    second.world.rng = host.world.rng.clone();
    second.load_slot(&mut again).unwrap();
    assert_eq!(second.world.state_hash(), first, "{edition:?}");
    assert_eq!(place(&second), place(host), "{edition:?}");
}

/// The bot takes the ladder up and comes back by the one down, the script of
/// each moving it (SPEC-013 §3.23), with the chaos audit on and a save of
/// either elevation loading back to where it stood.
#[test]
fn the_bot_climbs_to_the_second_elevation_and_back() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let mut run = Run::new(edition, 7, true);
        let climbed = |run: &Run, target: u32| {
            run.did(|action| {
                matches!(action, BotAction::Interact { target: id, verb: BotVerb::Use }
                    if *id == target)
            })
        };
        assert_eq!(elevation(&run.host), 0);
        run.until(20_000, |run| elevation(&run.host) == 1);
        assert!(climbed(&run, campaign::LADDER_UP), "{edition:?}");
        assert_eq!(hex(&run.host), campaign::UPPER_LANDING, "{edition:?}");
        assert_eq!(run.host.world.elevation, 1, "{edition:?}");
        let corpus = run._dir.path().to_path_buf();
        reloaded(&mut run.host, &corpus, edition);
        run.until(4000, |run| elevation(&run.host) == 0);
        assert!(climbed(&run, campaign::LADDER_DOWN), "{edition:?}");
        assert_eq!(hex(&run.host), campaign::LOWER_LANDING, "{edition:?}");
        reloaded(&mut run.host, &corpus, edition);
        let status = run.status();
        assert_eq!(status.violation, None, "{edition:?}");
        assert_eq!(status.abandoned, 0, "{edition:?}: {status:#?}");
    }
}

/// A ladder up is the bot's before the exit grid, and each ladder is tried once.
#[test]
fn a_ladder_comes_before_the_way_out_and_only_once() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    // A walk to the foe crosses the exit grid, and the map is left early.
    let foe = object(&run.host, campaign::FOE);
    run.host.world.erase(foe);
    let first = run.host.world.current_map_name();
    run.until(20_000, |run| run.host.world.current_map_name() != first);
    let uses: Vec<u32> = run
        .actions
        .iter()
        .filter_map(|action| match action {
            BotAction::Interact {
                target,
                verb: BotVerb::Use,
            } if [campaign::LADDER_UP, campaign::LADDER_DOWN].contains(target) => Some(*target),
            _ => None,
        })
        .collect();
    assert_eq!(uses, [campaign::LADDER_UP, campaign::LADDER_DOWN]);
}

#[test]
fn the_spine_hops_when_a_goal_is_too_far_for_one_route() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let player = run.host.world.player;
    // Far past the route search's reach over open ground, the only item.
    for id in [
        campaign::PISTOL,
        campaign::AMMUNITION,
        campaign::DYNAMITE,
        campaign::PLASTIC,
    ] {
        let item = object(&run.host, id);
        run.host.world.erase(item);
    }
    let drug = object(&run.host, campaign::DRUG);
    let far = 20100 + 45 * 200;
    run.host.world.move_object(drug, far, 0, false).unwrap();
    assert!(run.host.world.route(player, 20100, far, false).is_empty());
    run.until(200, |run| {
        run.host.world.get(drug).unwrap().owner == Some(player)
    });
}

#[test]
fn a_goal_that_goes_nowhere_is_given_up() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    // The world never carries the goal's action out, however often it asks.
    let mut frames = 0;
    while run.status().abandoned == 0 {
        run.decide();
        frames += 1;
        assert!(frames < 200, "{:#?}", run.status());
    }
    assert!(frames >= 12, "{frames}");
}

#[test]
fn the_spine_ends_on_a_map_it_has_finished_and_says_so() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let ids: Vec<ObjectRef> = run
        .host
        .world
        .iter()
        .filter(|(reference, _)| *reference != run.host.world.player)
        .map(|(reference, _)| reference)
        .collect();
    for reference in ids {
        run.host.world.erase(reference);
    }
    run.until(1000, |run| {
        run.status().goal == "Nothing left to do on this map"
    });
    for _ in 0..30 {
        assert_eq!(run.frame(), None);
    }
    assert!(run.status().stuck >= 30);
}

// The tactician.

#[test]
fn the_tactician_shoots_what_is_in_reach() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    arm_player(&mut run.host);
    set_rounds(&mut run.host, 5);
    fight(&mut run.host, 2);
    let action = run.decide().unwrap();
    assert!(
        matches!(
            action,
            BotAction::Attack {
                target: campaign::FOE,
                hit_mode: hit_mode::RIGHT_PRIMARY,
                ..
            }
        ),
        "{action:?}"
    );
    assert!(
        run.status().tactic.contains("4 AP"),
        "{}",
        run.status().tactic
    );
}

#[test]
fn the_tactician_closes_the_range_and_keeps_the_attacks_points() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let foe = fight(&mut run.host, 4);
    let player = run.host.world.player;
    let start = run.host.world.distance(player, foe);
    let action = run.decide().unwrap();
    let BotAction::Walk {
        hex: stop,
        run: false,
    } = action
    else {
        panic!("{action:?}");
    };
    done(&mut run.host, action);
    assert_eq!(hex(&run.host), stop);
    assert_eq!(run.host.world.distance(player, foe), 1);
    assert!(start > 1);
    assert!(ap(&run.host) >= 3, "the punch still has its points");
}

#[test]
fn the_tactician_reloads_an_empty_weapon_then_shoots() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    arm_player(&mut run.host);
    carry(&mut run.host, &[campaign::AMMUNITION]);
    set_rounds(&mut run.host, 0);
    fight(&mut run.host, 2);
    let action = run.decide().unwrap();
    assert_eq!(
        action,
        BotAction::Reload {
            weapon: campaign::PISTOL
        }
    );
    done(&mut run.host, action);
    let next = run.decide().unwrap();
    assert!(matches!(next, BotAction::Attack { .. }), "{next:?}");
}

#[test]
fn a_weapon_whose_attack_cannot_register_is_given_up() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    // No animation code, so its attack has no art (SPEC-007 §3.10.1).
    let words = [
        (0x18, 0x76),
        (0x34, -1),
        (0x39 + 16, 10),
        (0x39 + 32, 4),
        (0x39 + 52, 1),
        (0x39 + 60, 10),
    ];
    let pistol = object(&run.host, campaign::PISTOL);
    run.host.world.get_mut(pistol).unwrap().prototype = Some(patched(0, 3, &words, &[]));
    arm_player(&mut run.host);
    set_rounds(&mut run.host, 5);
    fight(&mut run.host, 1);
    for _ in 0..2 {
        let action = run.frame();
        assert!(
            matches!(
                action,
                Some(BotAction::Attack {
                    hit_mode: hit_mode::RIGHT_PRIMARY,
                    ..
                })
            ),
            "{action:?}"
        );
    }
    let action = run.frame();
    assert!(
        matches!(
            action,
            Some(BotAction::Attack {
                hit_mode: hit_mode::PUNCH,
                ..
            })
        ),
        "{action:?}"
    );
}

#[test]
fn the_tactician_falls_back_on_fists() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    arm_player(&mut run.host);
    set_rounds(&mut run.host, 0);
    fight(&mut run.host, 1);
    let action = run.decide().unwrap();
    assert!(
        matches!(
            action,
            BotAction::Attack {
                hit_mode: hit_mode::PUNCH,
                ..
            }
        ),
        "{action:?}"
    );
}

/// The campaign's drug, made one that heals at once.
fn healing_drug(host: &mut Host) -> ObjectRef {
    let mut bytes = vault_fixtures::world::prototype(0, 2);
    vault_fixtures::world::word(&mut bytes, 0, 40);
    vault_fixtures::world::word(&mut bytes, 0x39, crate::drugs::CURRENT_HP);
    vault_fixtures::world::word(&mut bytes, 0x39 + 4, -1);
    vault_fixtures::world::word(&mut bytes, 0x39 + 8, -1);
    vault_fixtures::world::word(&mut bytes, 0x39 + 12, 10);
    let prototype = std::sync::Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let drug = object(host, campaign::DRUG);
    host.world.get_mut(drug).unwrap().prototype = Some(prototype);
    drug
}

fn hurt_in_a_fight(hit_points: impl Fn(i32) -> i32) -> Run {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let drug = healing_drug(&mut run.host);
    let player = run.host.world.player;
    run.host.world.add_item(player, drug, 2, false).unwrap();
    fight(&mut run.host, 1);
    let max = run.host.world.effective(player, 7);
    set_hp(&mut run.host.world, player, hit_points(max));
    run
}

#[test]
fn the_tactician_heals_below_two_fifths_once_a_turn() {
    let mut run = hurt_in_a_fight(|max| (max * 2 + 4) / 5);
    let action = run.decide();
    assert!(
        matches!(action, Some(BotAction::Attack { .. })),
        "at the line it fights: {action:?}"
    );
    let mut run = hurt_in_a_fight(|max| (max * 2 + 4) / 5 - 1);
    let heal = BotAction::UseItem {
        item: campaign::DRUG,
    };
    assert_eq!(run.decide(), Some(heal));
    let action = run.decide();
    assert!(
        matches!(action, Some(BotAction::Attack { .. })),
        "one heal a turn: {action:?}"
    );
}

#[test]
fn upkeep_heals_out_of_combat_too() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let drug = healing_drug(&mut run.host);
    let player = run.host.world.player;
    run.host.world.add_item(player, drug, 1, false).unwrap();
    set_hp(&mut run.host.world, player, 1);
    assert_eq!(
        run.decide(),
        Some(BotAction::UseItem {
            item: campaign::DRUG
        })
    );
}

#[test]
fn the_tactician_ends_a_turn_it_cannot_use() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    fight(&mut run.host, 1);
    // Not enough for a punch.
    set_ap(&mut run.host, 2);
    assert_eq!(run.decide(), Some(BotAction::EndTurn));
}

#[test]
fn the_tactician_asks_to_end_a_fight_with_no_one_left() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let foe = fight(&mut run.host, 2);
    run.host.world.critter_state_mut(foe).unwrap().team = 0;
    assert_eq!(run.decide(), Some(BotAction::EndCombat));
    assert_eq!(run.decide(), Some(BotAction::EndTurn));
}

#[test]
fn the_tactician_waits_for_its_turn() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    fight(&mut run.host, 2);
    run.host.world.combat_state.player_turn = false;
    assert_eq!(run.decide(), None);
    assert_eq!(run.status().stuck, 0);
}

#[test]
fn a_foe_that_cannot_talk_gets_a_fight_and_the_bot_finishes_it() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    let foe = object(&run.host, campaign::FOE);
    run.host.world.critter_state_mut(foe).unwrap().team = 1;
    run.until(6000, |run| {
        let world = &run.host.world;
        world.get(foe).is_none_or(|_| world.is_dead(foe)) || world.is_dead(world.player)
    });
    assert_eq!(run.status().violation, None);
    assert!(run.did(|action| matches!(action, BotAction::Attack { .. })));
}

// The chaos audit.

#[test]
fn the_audit_runs_clean_over_the_campaign() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        for seed in [1, 2, 3] {
            let mut run = Run::new(edition, seed, true);
            run.until_audited();
            let status = run.status();
            assert_eq!(status.violation, None, "{edition:?} seed {seed}");
            assert_eq!(status.violations, 0);
            assert!(status.checks >= 30, "{edition:?} seed {seed}: {status:#?}");
            // Both explosives went, and the scripted door with one of them.
            assert_eq!(run.armed(), [campaign::DYNAMITE, campaign::PLASTIC]);
            let world = &run.host.world;
            assert_eq!(world.reference_for_id(campaign::DYNAMITE), None);
            assert_eq!(world.reference_for_id(campaign::PLASTIC), None);
            assert_eq!(world.reference_for_id(campaign::SCRIPTED_DOOR), None);
        }
    }
}

/// The world's own rolls decide how each fuse goes: on time or early.
#[test]
fn the_audit_holds_whatever_the_fuses_roll() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        for world in 1..=30 {
            let mut run = Run::in_world(edition, world, 5, true);
            let foe = object(&run.host, campaign::FOE);
            run.host.world.critter_state_mut(foe).unwrap().team = 1;
            run.until(9000, |run| {
                run.status().goal.starts_with("Leaving by the exit grid")
                    || run.host.world.pending_transition.is_some()
                    || run.host.world.is_dead(run.host.world.player)
            });
            assert_eq!(run.status().violation, None, "{edition:?} world {world}");
        }
    }
}

#[test]
fn a_fuse_that_fires_at_once_is_checked_all_the_same() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let player = run.host.world.player;
    set_hp(&mut run.host.world, player, 500);
    let at = |run: &Run| {
        run.status().goal.starts_with("Testing")
            && run
                .host
                .world
                .reference_for_id(campaign::DYNAMITE)
                .is_some()
    };
    run.until(3000, at);
    let bomb = loop {
        let Some(action) = run.decide() else {
            panic!("the bot stopped short of arming");
        };
        let outcome = run.host.execute_bot(&action).unwrap();
        run.actions.push(action.clone());
        assert_eq!(outcome, BotOutcome::Done);
        if let BotAction::ArmExplosive { item, .. } = action {
            break object(&run.host, item);
        }
        run.now += 16;
        run.host.tick(run.now).unwrap();
    };
    // SPEC-013 §3.10 result 0: type 11 after no ticks, at the bot's feet.
    run.host.world.queue.remove_object(bomb);
    let (ticks, kind) = (
        run.host.world.clock.ticks,
        EventKind::Other(skills::PREMATURE_EVENT),
    );
    run.host.world.queue.insert(ticks, 0, bomb, kind).unwrap();
    run.now += 16;
    run.host.tick(run.now).unwrap();
    run.until_blown();
    assert!(hp(&run.host, player) < 500);
    assert_eq!(run.status().violation, None);
    assert!(run.status().checks >= 30, "{:#?}", run.status());
}

/// The desktop plays everything over time: the bot waits for each sequence
/// where a headless run sees it finished at once.
#[test]
fn the_audit_runs_clean_when_everything_plays_over_time() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let mut run = Run::new(edition, 1, true);
        run.host.playback = Playback::RealTime;
        let foe = object(&run.host, campaign::FOE);
        run.host.world.critter_state_mut(foe).unwrap().team = 1;
        run.until(120_000, |run| {
            run.status().goal.starts_with("Leaving by the exit grid")
                || run.host.world.pending_transition.is_some()
        });
        let status = run.status();
        assert_eq!(status.violation, None, "{edition:?}");
        assert!(status.checks >= 30, "{edition:?}: {status:#?}");
        assert_eq!(run.armed(), [campaign::DYNAMITE, campaign::PLASTIC]);
    }
}

#[test]
fn the_tactician_waits_for_an_attack_to_finish_playing() {
    let mut run = Run::new(Edition::Fallout1, 7, false);
    run.host.playback = Playback::RealTime;
    arm_player(&mut run.host);
    set_rounds(&mut run.host, 5);
    fight(&mut run.host, 2);
    assert!(matches!(run.frame(), Some(BotAction::Attack { .. })));
    assert!(run.host.presenting());
    assert_eq!(run.frame(), None);
    assert_eq!(run.status().stuck, 0);
}

#[test]
fn the_audit_leaves_locks_and_doors_as_it_found_them() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let sealed = |run: &Run| {
        [campaign::LOCKED_DOOR, campaign::JAMMED_DOOR]
            .map(|id| lock(&run.host, id).0 & (LockFlags::LOCKED.0 | LockFlags::JAMMED.0))
    };
    let before = sealed(&run);
    run.until(3000, |run| run.status().goal.starts_with("Testing"));
    assert_eq!(sealed(&run), before);
    for id in [campaign::PLAIN_DOOR, campaign::LOCKED_DOOR] {
        assert!(!lock(&run.host, id).contains(LockFlags::OPEN), "{id}");
    }
}

#[test]
fn the_audit_says_what_it_is_testing() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until(3000, |run| run.status().goal.starts_with("Testing"));
    let goal = format!("Testing Door #{} with Dynamite", campaign::SCRIPTED_DOOR);
    assert_eq!(run.status().goal, goal);
    run.until_blown();
    run.until(1000, |run| run.status().goal.ends_with("with C4"));
    let goal = format!("Testing Door #{} with C4", campaign::PLAIN_DOOR);
    assert_eq!(run.status().goal, goal);
}

#[test]
fn the_bot_stands_clear_of_its_bomb_and_waits_for_it() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    let at = run.host.world.get(bomb).unwrap().raw.hex.unwrap();
    run.until(10, |run| {
        HexGrid::angular_distance(hex(&run.host), at).unwrap() >= 5
    });
    let armed = run.actions.len();
    run.until(1500, |run| run.host.world.get(bomb).is_none());
    // Nothing else was done while the fuse burned.
    assert_eq!(run.actions.len(), armed);
    assert_eq!(run.status().stuck, 0);
}

/// Frames of the agent watching, with its actions thrown away.
fn watch(run: &mut Run, frames: usize) {
    for _ in 0..frames {
        run.decide();
        run.now += 16;
        run.host.tick(run.now).unwrap();
    }
}

#[test]
fn a_fuse_stops_for_a_dialogue_and_is_not_overdue() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    let ticks = run.host.world.clock.ticks;
    // Longer than the fuse, in frames.
    run.host.world.dialogue.open = true;
    watch(&mut run, 1000);
    run.host.world.dialogue.open = false;
    assert_eq!(run.host.world.clock.ticks, ticks);
    assert!(run.host.world.get(bomb).is_some());
    assert_eq!(run.status().violation, None);
    run.until_blown();
    assert_eq!(run.status().violation, None);
}

#[test]
fn a_fuse_stops_while_a_combat_round_lasts_and_goes_on_with_the_next() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    run.frame();
    let foe = object(&run.host, campaign::FOE);
    run.host.world.critter_state_mut(foe).unwrap().team = 1;
    let player = run.host.world.player;
    run.host.start_combat(player, foe).unwrap();
    run.now += 16;
    run.host.tick(run.now).unwrap();
    // The player's turn lasts, and only the clock of rounds moves time.
    let ticks = run.host.world.clock.ticks;
    watch(&mut run, 1000);
    assert_eq!(run.host.world.clock.ticks, ticks);
    assert!(run.host.world.get(bomb).is_some());
    // SPEC-006 §3.7: each round adds five game seconds, and the fuse fires
    // in the middle of the fight.
    run.until(4000, |run| run.host.world.get(bomb).is_none());
    run.until_blown();
    assert_eq!(run.status().violation, None);
}

#[test]
fn an_enhanced_player_may_route_through_a_closed_door() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.host.set_preset(EnginePreset::Enhanced);
    run.until_audited();
    assert_eq!(run.status().violation, None);
}

// Each check fails when the world breaks its rule: the tests run to a point,
// break it by hand, and look for the report.

#[test]
fn a_door_that_blocks_when_open_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_swung(1);
    let door = object(&run.host, campaign::PLAIN_DOOR);
    let flags = &mut run.host.world.get_mut(door).unwrap().raw.flags.0;
    *flags &= !BasicFlags::NONBLOCKING.0;
    run.expect_violation("is open but blocks");
}

#[test]
fn a_door_that_stops_blocking_when_closed_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let door = object(&run.host, campaign::PLAIN_DOOR);
    // Hidden, the door is no blocker, whatever else it says.
    run.host.world.get_mut(door).unwrap().raw.flags.0 |= BasicFlags::HIDDEN.0;
    run.expect_violation("nothing blocks");
}

#[test]
fn a_door_that_will_not_swing_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_swung(1);
    // Put it back before the audit looks.
    let door = object(&run.host, campaign::PLAIN_DOOR);
    let mut flags = lock(&run.host, campaign::PLAIN_DOOR);
    flags.0 &= !LockFlags::OPEN.0;
    let object = run.host.world.get_mut(door).unwrap();
    object.set_lock(flags);
    object.raw.flags.0 &= !BasicFlags::NONBLOCKING.0;
    object.raw.frame = 0;
    run.expect_violation("did not swing");
}

#[test]
fn a_route_through_a_closed_door_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_swung(2);
    // Closed by its bit, yet the search walks through it.
    let door = object(&run.host, campaign::PLAIN_DOOR);
    let flags = &mut run.host.world.get_mut(door).unwrap().raw.flags.0;
    *flags |= BasicFlags::NONBLOCKING.0;
    run.expect_violation("passes closed");
}

#[test]
fn a_rock_in_an_open_doorway_is_no_fault_of_the_door() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_swung(1);
    // A rock on one side leaves the door open and the probe's way shut.
    let door = object(&run.host, campaign::PLAIN_DOOR);
    let at = run.host.world.get(door).unwrap().raw.hex.unwrap();
    let rock = run
        .host
        .world
        .spawn(patched(2, 5, &[], &[]), at, 0)
        .unwrap();
    run.host.world.get_mut(rock).unwrap().raw.flags = BasicFlags(0);
    let flags = &mut run.host.world.get_mut(door).unwrap().raw.flags.0;
    *flags |= BasicFlags::NONBLOCKING.0;
    run.frame();
    // The rock is the blocker, not the door: nothing to report of the door.
    assert_eq!(run.status().violation, None);
}

#[test]
fn a_lockpick_that_changes_a_door_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until(400, |run| {
        run.did(|action| {
            matches!(action, BotAction::UseSkill { target, .. }
                if *target == campaign::LOCKED_DOOR)
        })
    });
    let door = object(&run.host, campaign::LOCKED_DOOR);
    let mut flags = lock(&run.host, campaign::LOCKED_DOOR);
    flags.0 &= !LockFlags::LOCKED.0;
    run.host.world.get_mut(door).unwrap().set_lock(flags);
    run.expect_violation("Lockpick changed");
}

#[test]
fn a_crowbar_is_used_when_carried_and_a_change_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    // A use-on tool the campaign does not hold, under the name that counts.
    let player = run.host.world.player;
    let bar = run
        .host
        .world
        .spawn(patched(0, 5, &[(0x18, 0x1000), (0, 0x12)], &[]), 20100, 0)
        .unwrap();
    run.host.world.get_mut(bar).unwrap().name = b"Crowbar".to_vec();
    run.host.world.add_item(player, bar, 1, false).unwrap();
    let id = run.host.world.get(bar).unwrap().raw.id;
    run.until(400, |run| {
        run.did(|action| {
            matches!(action, BotAction::UseItemOn { item, target }
                if *item == id && *target == campaign::LOCKED_DOOR)
        })
    });
    assert_eq!(run.status().violation, None);
    let door = object(&run.host, campaign::LOCKED_DOOR);
    let mut flags = lock(&run.host, campaign::LOCKED_DOOR);
    flags.0 &= !LockFlags::LOCKED.0;
    run.host.world.get_mut(door).unwrap().set_lock(flags);
    run.expect_violation("Crowbar changed");
}

#[test]
fn a_locked_container_stays_shut_and_one_that_swings_is_reported() {
    let used = |run: &Run| {
        run.did(|action| {
            matches!(action, BotAction::Interact { target, verb: BotVerb::Use }
                if *target == campaign::CONTAINER)
        })
    };
    let locked = LockFlags(LockFlags::LOCKED.0);
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let chest = object(&run.host, campaign::CONTAINER);
    run.host.world.get_mut(chest).unwrap().set_lock(locked);
    run.until(400, used);
    run.frame();
    assert_eq!(run.status().violation, None);
    assert_eq!(run.host.world.get(chest).unwrap().raw.frame, 0);
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let chest = object(&run.host, campaign::CONTAINER);
    run.host.world.get_mut(chest).unwrap().set_lock(locked);
    run.until(400, used);
    run.host.world.get_mut(chest).unwrap().raw.frame = 1;
    run.expect_violation("swung when used");
}

/// The friend, tough enough to live, `from` the bomb, and a run burning it.
fn bystander_run(offset: i32) -> (Run, ObjectRef) {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let friend = object(&run.host, campaign::FRIEND);
    set_hp(&mut run.host.world, friend, 500);
    let bomb = run.until_armed();
    let at = run.host.world.get(bomb).unwrap().raw.hex.unwrap();
    run.host
        .world
        .move_object(friend, at + offset, 0, false)
        .unwrap();
    (run, friend)
}

#[test]
fn a_critter_in_reach_is_hurt() {
    let (mut run, friend) = bystander_run(-2);
    run.until_blown();
    assert!(hp(&run.host, friend) < 500);
    assert_eq!(run.status().violation, None);
}

#[test]
fn a_blast_that_hurts_no_one_is_reported() {
    let (mut run, friend) = bystander_run(-2);
    run.until_explosion();
    run.frame();
    run.until(1000, |run| run.host.world.explosion.is_none());
    set_hp(&mut run.host.world, friend, 500);
    run.expect_violation("took no blast damage");
}

#[test]
fn a_critter_left_out_of_the_blast_is_reported() {
    let (mut run, _) = bystander_run(-2);
    run.until_explosion();
    run.host.world.explosion.as_mut().unwrap().victims.clear();
    run.expect_violation("the blast at");
}

#[test]
fn a_critter_hurt_outside_the_blast_is_reported() {
    // Five columns off: within the watch, past the reach.
    let (mut run, friend) = bystander_run(-205);
    run.until_explosion();
    run.frame();
    set_hp(&mut run.host.world, friend, 1);
    run.expect_violation("outside the blast");
}

#[test]
fn a_damage_procedure_that_never_ran_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_armed();
    run.until(1500, |run| {
        run.host
            .world
            .reference_for_id(campaign::SCRIPTED_DOOR)
            .is_none()
    });
    // The door went by its procedure: the trace forgets it.
    run.host.world.trace.clear();
    run.expect_violation("did not run");
}

#[test]
fn a_door_changed_by_a_blast_with_no_script_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until(4000, |run| run.armed().len() == 2);
    let door = object(&run.host, campaign::PLAIN_DOOR);
    run.host.world.erase(door);
    run.expect_violation("the blast changed");
}

#[test]
fn tiles_changed_by_a_blast_are_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_armed();
    run.host.world.map.tiles[0].as_mut().unwrap()[0].word ^= 0x1;
    run.expect_violation("changed tiles");
}

#[test]
fn hidden_roofs_are_no_change_of_tiles() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_armed();
    run.host.world.map.tiles[0].as_mut().unwrap()[0].word ^= 0x1000_0000;
    run.until_blown();
    assert_eq!(run.status().violation, None);
}

#[test]
fn an_armed_event_left_behind_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_armed();
    let ticks = run.host.world.clock.ticks;
    run.until_explosion();
    // Named for an object that does not exist, due far off.
    let kind = EventKind::Other(skills::EXPLOSION_EVENT);
    run.host
        .world
        .queue
        .insert(ticks, 100_000, 999_999, kind)
        .unwrap();
    run.expect_violation("outlived its explosive");
}

#[test]
fn an_armed_explosive_with_no_event_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    run.frame();
    run.host.world.queue.remove_object(bomb);
    run.expect_violation("no event queued");
}

#[test]
fn a_fuse_that_never_fires_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    let kind = run
        .host
        .world
        .queue
        .events()
        .find(|event| event.object == bomb)
        .map(|event| event.kind)
        .unwrap();
    run.host.world.queue.remove_object(bomb);
    let ticks = run.host.world.clock.ticks;
    run.host
        .world
        .queue
        .insert(ticks, 1_000_000, bomb, kind)
        .unwrap();
    run.expect_violation("overdue");
}

#[test]
fn an_explosive_left_after_its_blast_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let bomb = run.until_armed();
    run.until_explosion();
    assert!(run.host.world.get(bomb).is_none());
    // A new object answers to the explosive's id, as a ghost would.
    let copy = run
        .host
        .world
        .spawn(patched(0, 5, &[(0, 51)], &[]), 20100, 0)
        .unwrap();
    run.host.world.set_object_id(copy, campaign::DYNAMITE);
    run.expect_violation("outlived its explosion");
}

#[test]
fn an_explosion_that_never_ends_is_reported() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    run.until_armed();
    run.until_explosion();
    // Decisions with no tick: its art never finishes.
    for _ in 0..2100 {
        run.decide();
    }
    assert!(
        run.violation().contains("never ended"),
        "{}",
        run.violation()
    );
}

#[test]
fn a_destroyed_doors_id_is_free_for_another_object() {
    let mut run = Run::new(Edition::Fallout1, 1, true);
    let door = object(&run.host, campaign::SCRIPTED_DOOR);
    let copy = run.host.world.get(door).unwrap().clone();
    run.until_blown();
    assert!(run.host.world.get(door).is_none());
    // `insert` renumbers an object whose id the index still holds.
    let back = run.host.world.insert(copy).unwrap();
    let id = run.host.world.get(back).unwrap().raw.id;
    assert_eq!(id, campaign::SCRIPTED_DOOR);
}
