//! Hostile critters under the Fallout 2 profile: a critter whose
//! `critter_p_proc` sees the player requests combat (SPEC-006 §3.7, SPEC-005
//! `obj_can_see_obj` and `attack`), and a critter the player attacks out of
//! combat fights back (SPEC-014 §3.38 step 3, SPEC-007 §3.3, SPEC-009 §3.5).
//! The new game's player must start with full AP for the second: the
//! out-of-combat attack preflight checks it.
use super::*;
use vault_core::{assets::VfsWorldAssets, host_choreography::Playback, movie::MoviePolicy};
use vault_fixtures::state as fixture;
use vault_formats::{Vfs, fallout2::Fallout2Install};

/// A new Fallout 2 game over `vfs` with the premade `sheet`: SPEC-018 §6.6
/// step 3's hand-off to the start map `maps/artemple.map`, as the Play
/// shell makes it, with movies and attacks completing at once.
fn new_game(vfs: Arc<dyn Vfs>, sheet: &str) -> MapPreview {
    let open = || {
        let assets = VfsWorldAssets::new(vfs.clone(), "english").unwrap();
        assets.set_game_profile(vault_core::GameProfile::Fallout2);
        assets
    };
    let tables = WorldHost::new(
        vault_core::WorldState::new(CharacterSheet::default(), 1).unwrap(),
        open(),
    )
    .load_fallout2_tables()
    .unwrap();
    let assets = open();
    let sheet = CharacterSheet::from_gcd(&assets.gcd(sheet).unwrap(), Default::default());
    let mut world = MapPreview::with_setup(
        assets,
        vault_core::fallout2::START_MAP,
        sheet,
        1,
        EnginePreset::Vanilla,
        |host| host.begin_fallout2_game(tables),
    )
    .unwrap();
    world.host.movie_policy = MoviePolicy::CompleteImmediately;
    world.host.playback = Playback::Instant;
    world
}

fn ap(world: &MapPreview, critter: ObjectRef) -> i32 {
    world.host.world.critter_state(critter).unwrap().ap
}
/// The effective maximum AP (derived stat 8).
fn maximum_ap(world: &MapPreview, critter: ObjectRef) -> i32 {
    let context = world.host.world.context(critter);
    let object = world.host.world.get(critter).unwrap();
    object.character.as_ref().unwrap().stat(8, context)
}

/// Step frames until the player's second combat turn: the first one (after
/// any replayed attack click) is ended at once, so the critters take their
/// turns in between. Returns whether that second turn came.
fn fight_one_round(world: &mut MapPreview, now: &mut u64) -> bool {
    let mut ended = false;
    for _ in 0..2000 {
        *now += 16;
        world.tick(*now).unwrap();
        let combat = &world.host.world.combat_state;
        if !world.host.world.combat
            || !combat.player_turn
            || combat.turn_ended
            || world.hud.replay.is_some()
        {
            continue;
        }
        if ended {
            return true;
        }
        world.host.end_player_turn();
        ended = true;
    }
    false
}

/// A Fallout 2 packet (SPEC-018 §4.7.1: no `last_msg`, `hurt_too_much`
/// optional, the twelve range ends) that always attacks and never taunts.
fn ai_txt() -> String {
    let mut text = String::from("[Hostile]\r\n");
    for (key, value) in [
        ("packet_num", "0"),
        ("max_dist", "20"),
        ("min_to_hit", "0"),
        ("min_hp", "0"),
        ("aggression", "100"),
        ("secondary_freq", "0"),
        ("called_freq", "0"),
        ("font", "101"),
        ("color", "145"),
        ("outline_color", "-1"),
        ("chance", "0"),
    ] {
        text.push_str(&format!("{key}={value}\r\n"));
    }
    for (index, key) in vault_formats::ai::KEYS[12..24].iter().enumerate() {
        text.push_str(&format!("{key}={}\r\n", 1000 + 10 * index));
    }
    for (index, key) in vault_formats::ai::END_KEYS.iter().enumerate() {
        text.push_str(&format!("{key}={}\r\n", 1000 + 10 * index));
    }
    text
}

/// A script whose `critter_p_proc` body is `body(at)`, `at` being the
/// body's own address (for its branch target).
fn critter_script(body: impl Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let build = |at: i32| {
        fixture::program(
            &[],
            &[
                (b"start", fixture::returning(Vec::new())),
                (b"critter_p_proc", fixture::returning(body(at))),
            ],
        )
    };
    // Procedure 2 (after `main` and `start`) begins with its frame word.
    let first = build(0);
    let entry = 46 + 24 * 2 + 16;
    build(i32::from_be_bytes(first[entry..entry + 4].try_into().unwrap()) + 2)
}
/// `if obj_can_see_obj(self_obj, dude_obj) then attack(dude_obj)`, in the
/// eight-operand `attack` form (accuracy 0, damage 0 to 30000).
fn hostile_script() -> Vec<u8> {
    critter_script(|at| {
        let test = [
            fixture::instruction(0x80bc),
            fixture::instruction(0x80bf),
            fixture::instruction(0x80dc),
            fixture::instruction(0x802f),
        ]
        .concat();
        let mut attack = fixture::instruction(0x80bf);
        for value in [0, 1, 0, 0, 30000, 0, 0] {
            attack.extend(fixture::push(value));
        }
        attack.extend(fixture::instruction(0x80d0));
        // The skip address, then the condition, as `branch.zero` pops them.
        let skip = at + 6 + (test.len() + attack.len()) as i32;
        [fixture::push(skip), test, attack].concat()
    })
}
/// A heartbeat that does nothing.
fn passive_script() -> Vec<u8> {
    critter_script(|_| Vec::new())
}

/// A synthetic Fallout 2 install (SPEC-018 §8) over the synthetic corpus:
/// its critter runs `script`, the start map has no map script, `ai.txt`
/// holds [`ai_txt`]. The directory must outlive the returned databases.
fn synthetic_install(script: Vec<u8>) -> (tempfile::TempDir, Arc<dyn Vfs>) {
    let corpus = super::test_support::synthetic_corpus();
    let root = corpus.path();
    vault_fixtures::fallout2::write_tables(root);
    let map = std::fs::read(root.join("maps/synthetic.map")).unwrap();
    std::fs::write(
        root.join("maps/artemple.map"),
        vault_fixtures::fallout2::named_map(map, "artemple.map", 0, 20100, 2),
    )
    .unwrap();
    // SPEC-007 §3.10.1: an attack whose sequence art is missing does not
    // register, so the punch and every reaction get art.
    for suffix in ["aq", "an", "ao", "ap", "ba", "bb", "bo", "bp", "ch", "cj"] {
        std::fs::copy(
            root.join("art/critters/sampleaa.frm"),
            root.join(format!("art/critters/sample{suffix}.frm")),
        )
        .unwrap();
    }
    std::fs::write(root.join("scripts/synthetic.int"), script).unwrap();
    std::fs::write(root.join("data/ai.txt"), ai_txt()).unwrap();
    std::fs::create_dir_all(root.join("text/english/game")).unwrap();
    std::fs::write(
        root.join("text/english/game/combatai.msg"),
        "{1000}{}{Die!}\n",
    )
    .unwrap();
    let install = tempfile::tempdir().unwrap();
    vault_fixtures::dat2::fallout2_install(root, install.path(), 2);
    let provider = Fallout2Install::discover(install.path())
        .unwrap()
        .open()
        .unwrap()
        .provider(None);
    (install, Arc::new(provider))
}
/// The synthetic map's critter, on team 1 (the player is on team 0) with
/// the [`ai_txt`] packet. It stands next to the player's entrance hex.
fn synthetic_critter(world: &mut MapPreview) -> ObjectRef {
    let critter = super::test_support::npc(world);
    let script = world.host.world.script_for(critter).unwrap();
    assert_eq!(world.host.world.scripts[&script].file, 0, "synthetic.int");
    let state = world.host.world.critter_state_mut(critter).unwrap();
    state.team = 1;
    state.ai = 0;
    critter
}

#[test]
fn fallout2_a_hostile_critter_that_sees_the_player_starts_combat() {
    let (_install, vfs) = synthetic_install(hostile_script());
    let mut world = new_game(vfs, "premade/synthetic.gcd");
    assert!(world.host.world.fallout2.is_some());
    let player = world.host.world.player;
    let critter = synthetic_critter(&mut world);
    assert!(world.host.world.can_see(critter, player));
    // The critter-script cycle step runs its `critter_p_proc`, which sees
    // the player and requests combat: the critter is the attacker.
    let mut now = 0;
    for _ in 0..200 {
        now += 16;
        world.tick(now).unwrap();
        if world.host.world.combat {
            break;
        }
    }
    assert!(world.host.world.combat, "the heartbeat started no combat");
    let combat = &world.host.world.combat_state;
    assert_eq!(&combat.list[..combat.active], &[critter, player]);
    assert_eq!(world.host.world.get(critter).unwrap().opponent, player);
    // Its first turn attacked the player (a punch spends AP), and the
    // player's turn is next.
    assert!(combat.player_turn);
    assert!(ap(&world, critter) < maximum_ap(&world, critter));
    // It keeps fighting in the next round.
    assert!(fight_one_round(&mut world, &mut now));
    assert!(ap(&world, critter) < maximum_ap(&world, critter));
    assert_eq!(world.host.world.get(critter).unwrap().opponent, player);
}

#[test]
fn fallout2_a_critter_the_player_attacks_fights_back() {
    let (_install, vfs) = synthetic_install(passive_script());
    let mut world = new_game(vfs, "premade/synthetic.gcd");
    let player = world.host.world.player;
    let critter = synthetic_critter(&mut world);
    // SPEC-007 §3.3: the new game's player starts with its full AP.
    assert!(maximum_ap(&world, player) > 0);
    assert_eq!(ap(&world, player), maximum_ap(&world, player));
    // SPEC-014 §3.38 step 3: the attack click passes the preflight and
    // requests combat with the player as attacker.
    world.attack_request(critter);
    assert_eq!(
        world
            .host
            .world
            .pending_combat
            .map(|request| (request.attacker, request.defender)),
        Some((player, critter)),
        "refused: {}",
        String::from_utf8_lossy(&world.status)
    );
    assert_eq!(world.hud.replay, Some(critter));
    // The click is replayed on the player's first turn; the critter then
    // takes its turn against its last attacker.
    let mut now = 0;
    assert!(fight_one_round(&mut world, &mut now));
    assert!(world.host.world.combat);
    let combat = &world.host.world.combat_state;
    assert!(combat.list[..combat.active].contains(&critter));
    assert_eq!(world.host.world.get(critter).unwrap().opponent, player);
    assert!(
        ap(&world, critter) < maximum_ap(&world, critter),
        "the critter did not fight back"
    );
}

/// The retail install: `$VAULT_FALLOUT2`, else `~/games/fallout2`.
fn retail_install() -> Option<Arc<dyn Vfs>> {
    let folder = std::env::var_os("VAULT_FALLOUT2")
        .map(std::path::PathBuf::from)
        .or_else(|| crate::config::home_dir().map(|home| home.join("games/fallout2")));
    let Some(install) = folder.and_then(|folder| Fallout2Install::discover(&folder).ok()) else {
        eprintln!("skipping: no Fallout 2 install (set VAULT_FALLOUT2)");
        return None;
    };
    Some(Arc::new(install.open().unwrap().provider(None)))
}
/// A new retail game moved on to the Temple of Trials' critters.
/// `maps/artemple.map`, the new game's start map, holds no hostile critter
/// (only the temple's villager); its exit grids lead to `maps/arcaves.map`,
/// whose giant ants and radscorpions are the trial's.
fn retail_temple(vfs: &Arc<dyn Vfs>) -> MapPreview {
    let mut world = new_game(vfs.clone(), "premade/combat.gcd");
    assert!(world.host.world.map.header.name.starts_with(b"ARTEMPLE"));
    let player = world.host.world.player;
    assert_eq!(ap(&world, player), maximum_ap(&world, player));
    world.host.load_map("maps/arcaves.map").unwrap();
    world
}
/// The giant ants on the player's elevation, in world order.
fn retail_ants(world: &MapPreview) -> Vec<ObjectRef> {
    let player = world.host.world.player;
    let elevation = world.host.world.get(player).unwrap().raw.elevation;
    world
        .host
        .world
        .iter()
        .filter(|(reference, object)| {
            *reference != player
                && object.name == b"Giant Ant"
                && object.raw.elevation == elevation
                && object.alive()
        })
        .map(|(reference, _)| reference)
        .collect()
}
/// Put the player `distance` hexes from `critter` on a hex from which the
/// critter can see it, trying its facing first.
fn place_player(world: &mut MapPreview, critter: ObjectRef, distance: i32) -> i32 {
    let player = world.host.world.player;
    let object = world.host.world.get(critter).unwrap();
    let (hex, elevation, facing) = (
        object.raw.hex.unwrap(),
        object.raw.elevation,
        object.raw.orientation,
    );
    for turn in 0..6 {
        let direction = (facing + turn) % 6;
        let Ok(spot) = vault_formats::HexGrid::step(hex, direction, distance) else {
            continue;
        };
        world
            .host
            .world
            .move_object(player, spot, elevation, false)
            .unwrap();
        if world.host.world.can_see(critter, player) {
            return spot;
        }
    }
    panic!("no hex {distance} from {critter:?} that it sees");
}

#[test]
#[ignore = "needs a Fallout 2 install"]
fn fallout2_retail_temple_critters_start_combat_and_fight_back() {
    let Some(vfs) = retail_install() else {
        return;
    };
    // The heartbeat: an ant that sees the player requests combat.
    let mut world = retail_temple(&vfs);
    let player = world.host.world.player;
    let ant = retail_ants(&world)[0];
    let spot = place_player(&mut world, ant, 3);
    let mut now = 0;
    let mut started = None;
    for frame in 0..600 {
        now += 16;
        world.tick(now).unwrap();
        if world.host.world.combat {
            started = Some(frame);
            break;
        }
    }
    eprintln!(
        "ant {ant:?} at {:?}, player at {spot}: combat after {started:?} frames",
        world.host.world.get(ant).unwrap().raw.hex
    );
    assert!(started.is_some(), "the ant's heartbeat started no combat");
    let combat = &world.host.world.combat_state;
    assert_eq!(combat.list[0], ant, "the ant attacks first");
    assert_eq!(world.host.world.get(ant).unwrap().opponent, player);
    let hp = world.host.world.hit_points(player);
    assert!(fight_one_round(&mut world, &mut now));
    eprintln!(
        "after a round: player hp {hp:?} -> {:?}, ant AP {}",
        world.host.world.hit_points(player),
        ap(&world, ant)
    );

    // Retaliation: on a fresh game, the player strikes an ant before its
    // heartbeat has run.
    let mut world = retail_temple(&vfs);
    let player = world.host.world.player;
    let ant = retail_ants(&world)[1];
    place_player(&mut world, ant, 1);
    world.attack_request(ant);
    assert_eq!(
        world
            .host
            .world
            .pending_combat
            .map(|request| (request.attacker, request.defender)),
        Some((player, ant)),
        "refused: {}",
        String::from_utf8_lossy(&world.status)
    );
    let mut now = 0;
    let hp = world.host.world.hit_points(ant);
    assert!(fight_one_round(&mut world, &mut now));
    eprintln!(
        "player struck ant {ant:?}: ant hp {hp:?} -> {:?}, ant AP {} of {}, combat {}",
        world.host.world.hit_points(ant),
        ap(&world, ant),
        maximum_ap(&world, ant),
        world.host.world.combat
    );
    assert!(world.host.world.combat);
    assert_eq!(world.host.world.get(ant).unwrap().opponent, player);
    assert!(
        ap(&world, ant) < maximum_ap(&world, ant),
        "the ant did not fight back"
    );
}
