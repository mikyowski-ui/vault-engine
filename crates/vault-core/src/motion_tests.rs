//! SPEC-007 §3.11 non-combat animation: Fixture H's walking steps, the
//! walk and run rules, the interaction sequences of §3.11.3, and doors and
//! containers (§3.11.4).
use crate::{
    CharacterSheet, ObjectAction, ObjectRef, Perk, WorldEffect, WorldError, WorldHost, WorldState,
    assets::WorldAssets,
    choreography::{Begin, Stepped},
    critical::effect,
    fixture_world as fixture,
    host_choreography::Playback,
    motion::{Call, Goal, HEX_STEPS, Motion, MotionSequence, Walk, crosses},
    routing::step,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vault_formats::{
    Fid, FrmArchive, MsgCatalog, Prototype,
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, LockFlags},
};

#[derive(Default)]
struct Assets {
    prototypes: BTreeMap<u32, Arc<Prototype>>,
    art: BTreeMap<u32, Arc<FrmArchive>>,
    /// Critter animation codes without art.
    missing: BTreeSet<u8>,
    tokens: BTreeMap<(u8, u16), Vec<u8>>,
}
impl WorldAssets for Assets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        self.prototypes
            .get(&pid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("no prototype".into()))
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), WorldError> {
        Err(WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        self.tokens.get(&(category, index)).cloned()
    }
    /// Critter art exists but for the listed animations; other art exists
    /// when it loads.
    fn art_exists(&self, fid: u32) -> bool {
        match Fid(fid).category() {
            1 => !self.missing.contains(&Fid(fid).animation()),
            _ => self.art.contains_key(&fid),
        }
    }
    fn object_art(&self, fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        self.art
            .get(&fid)
            .cloned()
            .ok_or_else(|| WorldError::Invalid("object art unavailable".into()))
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        let text: &[u8] = match file {
            "misc.msg" => b"{2000}{}{You cannot get there.}\n",
            "proto.msg" => {
                b"{480}{}{You see: %s.}\n{485}{}{You search the %s.}\n{486}{}{You close the %s.}\n\
                  {487}{}{It is locked.}\n{700}{}{You don't have enough action points.}\n"
            }
            _ => return Err(WorldError::Invalid(format!("{file} is missing"))),
        };
        Ok(Arc::new(MsgCatalog::parse(text).unwrap()))
    }
}

/// An FRM of `frames` frames per direction at `fps` with action frame
/// `action`, every frame in a direction moving by `shifts[direction]`.
fn frm(frames: u16, fps: u16, action: u16, shifts: [[i16; 2]; 6]) -> Arc<FrmArchive> {
    let mut bytes = Vec::new();
    bytes.extend(4_u32.to_be_bytes());
    bytes.extend(fps.to_be_bytes());
    bytes.extend(action.to_be_bytes());
    bytes.extend(frames.to_be_bytes());
    bytes.extend([0; 24]);
    let direction = u32::from(frames) * 13;
    for index in 0..6 {
        bytes.extend((index * direction).to_be_bytes());
    }
    bytes.extend((6 * direction).to_be_bytes());
    for [x, y] in shifts {
        for _ in 0..frames {
            bytes.extend(1_u16.to_be_bytes());
            bytes.extend(1_u16.to_be_bytes());
            bytes.extend(1_u32.to_be_bytes());
            bytes.extend(x.to_be_bytes());
            bytes.extend(y.to_be_bytes());
            bytes.push(1);
        }
    }
    Arc::new(FrmArchive::parse(&bytes).unwrap())
}
const STILL: [[i16; 2]; 6] = [[0; 2]; 6];
/// A critter art identifier of the player's base.
fn critter(animation: u8, weapon: u8) -> u32 {
    Fid::new(1, 0, animation, weapon, 0).0
}

/// The player at 20100, outside combat, with instant playback.
fn host() -> WorldHost<Assets> {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, Assets::default());
    host.assets.tokens.insert((1, 0), b"hmjmps".to_vec());
    host.world.effects.clear();
    host
}
fn hex(host: &WorldHost<Assets>, object: ObjectRef) -> i32 {
    host.world.get(object).unwrap().raw.hex.unwrap()
}
/// `count` hexes from `from` in `direction`.
fn hexes(from: i32, direction: u8, count: usize) -> i32 {
    (0..count).fold(from, |hex, _| step(hex, direction))
}
/// A prototype from the fixture template with words and bytes patched.
fn prototype(
    category: u8,
    subtype: usize,
    words: &[(usize, i32)],
    bytes: &[(usize, u8)],
) -> Arc<Prototype> {
    let mut data = fixture::prototype(category, subtype);
    for (offset, value) in words {
        fixture::word(&mut data, *offset, *value);
    }
    for (offset, value) in bytes {
        data[*offset] = *value;
    }
    Arc::new(Prototype::parse(&data).unwrap())
}
fn place(host: &mut WorldHost<Assets>, prototype: Arc<Prototype>, hex: i32) -> ObjectRef {
    let object = host.world.spawn(prototype, hex, 0).unwrap();
    host.world.get_mut(object).unwrap().raw.flags = BasicFlags(0);
    object
}
/// An unlocked, closed door with sound byte `A` and four frames of art,
/// each moving one pixel right.
fn door(host: &mut WorldHost<Assets>, hex: i32) -> ObjectRef {
    host.assets
        .art
        .insert(0x0200_0000, frm(4, 10, 0, [[1, 0]; 6]));
    let door = place(
        host,
        prototype(2, 0, &[(0x18, 0x800)], &[(0x28, b'A')]),
        hex,
    );
    host.world.get_mut(door).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Door(LockFlags(0)),
    };
    door
}
/// A closed container with sound byte `B` and three frames of art.
fn chest(host: &mut WorldHost<Assets>, hex: i32, words: &[(usize, i32)]) -> ObjectRef {
    host.assets.art.insert(0, frm(3, 10, 0, STILL));
    let chest = place(host, prototype(0, 1, words, &[(0x38, b'B')]), hex);
    host.world.get_mut(chest).unwrap().name = b"chest".to_vec();
    chest
}
fn npc(host: &mut WorldHost<Assets>, hex: i32) -> ObjectRef {
    place(host, prototype(1, 0, &[], &[]), hex)
}
fn lock(host: &WorldHost<Assets>, object: ObjectRef) -> LockFlags {
    host.world.get(object).unwrap().lock().unwrap()
}
fn set_lock(host: &mut WorldHost<Assets>, object: ObjectRef, bits: u32) {
    let found = host.world.get_mut(object).unwrap();
    let mut lock = found.lock().unwrap();
    lock.0 |= bits;
    found.set_lock(lock);
}
fn sounds(host: &mut WorldHost<Assets>) -> Vec<String> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::SoundEffect { name, .. } => Some(String::from_utf8_lossy(name).into()),
            _ => None,
        })
        .collect()
}
fn messages(host: &mut WorldHost<Assets>) -> Vec<String> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Message(text) => Some(String::from_utf8_lossy(text).into()),
            _ => None,
        })
        .collect()
}
/// A built sequence's entries: delay and what each does.
fn labels(sequence: &MotionSequence) -> Vec<(i32, String)> {
    sequence
        .schedule
        .entries()
        .iter()
        .map(|entry| {
            let label = match &entry.action {
                Motion::Turn { .. } => "turn".to_string(),
                Motion::Sound { name } => format!("sound {}", String::from_utf8_lossy(name)),
                Motion::Animate {
                    animation, weapon, ..
                } => match weapon {
                    Some(code) => format!("animate {animation} code {code}"),
                    None => format!("animate {animation}"),
                },
                Motion::Swing { forward, .. } => format!("swing {forward}"),
                Motion::Play {
                    animation, play, ..
                } => format!("play {animation} {play:?}"),
                Motion::Stand => "stand".into(),
                Motion::SetArt { art } => format!("art {art:#x}"),
                Motion::Walk(walk) => {
                    format!("{} {:?}", ["walk", "run"][usize::from(walk.run)], walk.goal)
                }
                Motion::Call(call) => match call {
                    Call::Adjacent { .. } => "adjacent".into(),
                    Call::CombatPoints => "points".into(),
                    Call::Pickup { .. } => "pickup".into(),
                    Call::Use { .. } | Call::UseOn { .. } => "use".into(),
                    Call::UseContainer { .. } => "container".into(),
                    Call::Loot { .. } => "loot".into(),
                    Call::Arrive { .. } => "arrive".into(),
                    Call::OpenBit { open } => format!("open bit {open}"),
                    Call::Finish { open } => format!("finish {open}"),
                },
            };
            (entry.delay, label)
        })
        .collect()
}
fn row(delay: i32, label: &str) -> (i32, String) {
    (delay, label.to_string())
}

#[test]
fn spec007_fixture_h_crossing_rule() {
    // Every SE frame adds (5, 4) and every E frame (8, 0); the route is
    // [SE, E]. Step 3 crosses on y with residual (-1, 0); step 8 crosses
    // on x.
    let mut displacement = [0, 0];
    let mut rows = Vec::new();
    let mut facing = 2;
    for step in 1..=8 {
        let shift = if facing == 2 { [5, 4] } else { [8, 0] };
        displacement = [displacement[0] + shift[0], displacement[1] + shift[1]];
        let crossed = crosses(displacement, facing);
        rows.push((step, facing, displacement, crossed));
        if crossed {
            let pixel = HEX_STEPS[usize::from(facing)];
            displacement = [displacement[0] - pixel[0], displacement[1] - pixel[1]];
            facing = 1;
        }
    }
    assert_eq!(
        rows,
        [
            (1, 2, [5, 4], false),
            (2, 2, [10, 8], false),
            (3, 2, [15, 12], true),
            (4, 1, [7, 0], false),
            (5, 1, [15, 0], false),
            (6, 1, [23, 0], false),
            (7, 1, [31, 0], false),
            (8, 1, [39, 0], true),
        ]
    );
    // A zero pixel step is never reached; negative steps cross at or below.
    assert!(!crosses([0, 100], 1) && crosses([32, -40], 1));
    assert!(crosses([16, 0], 0) && crosses([0, -12], 0) && !crosses([15, -11], 0));
    assert!(crosses([-32, 0], 4) && !crosses([-31, 5], 4));
    assert!(crosses([-16, 0], 3) && crosses([0, 12], 3) && !crosses([-15, 11], 3));
}

#[test]
fn spec007_fixture_h_walking_steps_through_the_world() {
    let mut host = host();
    let mut shifts = STILL;
    shifts[2] = [5, 4];
    shifts[1] = [8, 0];
    host.assets.art.insert(critter(1, 0), frm(8, 10, 0, shifts));
    let player = host.world.player;
    let start = hex(&host, player);
    let se = step(start, 2);
    let goal = step(se, 1);
    let begin = |host: &mut WorldHost<Assets>| {
        let mut walk = Walk::new(Goal::Hex(goal), false, usize::MAX);
        assert_eq!(host.start_walk(player, &mut walk), Begin::Timed);
        walk.route = vec![2, 1];
        walk
    };
    let mut walk = begin(&mut host);
    let mut rows = Vec::new();
    for number in 1..=8 {
        let stepped = host.walk_step(player, &mut walk, number);
        let object = &host.world.get(player).unwrap().raw;
        rows.push((
            object.frame,
            object.orientation,
            object.displacement,
            object.hex.unwrap(),
            stepped,
        ));
    }
    let next = Stepped::After(100);
    assert_eq!(
        rows,
        [
            (0, 2, [5, 4], start, next),
            (1, 2, [10, 8], start, next),
            (2, 1, [-1, 0], se, next),
            (3, 1, [7, 0], se, next),
            (4, 1, [15, 0], se, next),
            (5, 1, [23, 0], se, next),
            (6, 1, [31, 0], se, next),
            // The route is complete: the residual (7, 0) is dropped.
            (7, 1, [0, 0], goal, Stepped::Done),
        ]
    );
    // A blocker on the E neighbor at step 8: the walker stays on the SE
    // neighbor, and with no route to its destination it stops there.
    host.world.move_object(player, start, 0, false).unwrap();
    let mut walk = begin(&mut host);
    for number in 1..=7 {
        host.walk_step(player, &mut walk, number);
    }
    npc(&mut host, goal);
    assert_eq!(host.walk_step(player, &mut walk, 8), Stepped::Done);
    let object = &host.world.get(player).unwrap().raw;
    assert_eq!((object.hex, object.displacement), (Some(se), [0, 0]));
}

#[test]
fn spec007_walks_step_at_the_art_rate_and_end_standing() {
    let mut host = host();
    host.playback = Playback::RealTime;
    // Every direction moves a quarter hex a frame: four frames a hex.
    let shifts = HEX_STEPS.map(|[x, y]| [(x / 4) as i16, (y / 4) as i16]);
    host.assets.art.insert(critter(1, 0), frm(8, 10, 0, shifts));
    let player = host.world.player;
    let goal = hexes(20100, 1, 3);
    assert!(host.walk_toward(player, goal, false, 0).unwrap());
    let mut now = 0;
    let mut path = vec![hex(&host, player)];
    while host.moving(player) {
        host.step_motions(now);
        path.push(hex(&host, player));
        now += 100;
        assert!(now < 5000, "the walk ends");
    }
    // Twelve frame steps at 100 ms, the hexes changing on steps 4, 8, 12.
    assert_eq!(now, 1200);
    path.dedup();
    assert_eq!(path, [20100, hexes(20100, 1, 1), hexes(20100, 1, 2), goal]);
    let object = &host.world.get(player).unwrap().raw;
    assert_eq!(Fid(object.art_id).animation(), 0, "standing again");
    assert_eq!(object.displacement, [0, 0]);
    // §3.11.1: no footstep or movement sound.
    assert!(sounds(&mut host).is_empty());
}

#[test]
fn spec007_run_requests_walk_when_crippled_sneaking_or_without_run_art() {
    let mut host = host();
    let player = host.world.player;
    let goal = hexes(20100, 1, 2);
    let animation = |host: &mut WorldHost<Assets>, run: bool| {
        let mut walk = Walk::new(Goal::Hex(goal), run, usize::MAX);
        assert_eq!(host.start_walk(player, &mut walk), Begin::Timed);
        walk.animation
    };
    assert_eq!(animation(&mut host, false), 1);
    assert_eq!(animation(&mut host, true), 19);
    host.assets.missing.insert(19);
    assert_eq!(animation(&mut host, true), 1, "no run art");
    host.assets.missing.clear();
    host.world.critter_state_mut(player).unwrap().results.0 |= effect::CRIPPLED_RIGHT_LEG;
    assert_eq!(animation(&mut host, true), 1, "a crippled leg");
    host.world.critter_state_mut(player).unwrap().results.0 = 0;
    host.set_sneak(true).unwrap();
    assert_eq!(animation(&mut host, true), 1, "sneaking");
    host.world
        .player_sheet_mut()
        .unwrap()
        .perks
        .insert(Perk::SilentRunning, 1);
    assert_eq!(animation(&mut host, true), 19, "Silent Running");
    // A player's run click clears sneaking, unless with Silent Running.
    assert!(host.walk_toward(player, goal, true, 0).unwrap());
    assert!(host.sneaking());
    host.world.move_object(player, 20100, 0, false).unwrap();
    host.world
        .player_sheet_mut()
        .unwrap()
        .perks
        .remove(&Perk::SilentRunning);
    assert!(host.walk_toward(player, goal, true, 0).unwrap());
    assert!(!host.sneaking());
    assert_eq!(hex(&host, player), goal);
}

#[test]
fn spec007_combat_walks_pay_for_each_hex_and_stop_without_points() {
    let mut host = host();
    let player = host.world.player;
    host.world.combat = true;
    let goal = hexes(20100, 1, 5);
    // Free movement pays the first hex and 2 AP the next two.
    host.world.free_movement = 1;
    host.world.critter_state_mut(player).unwrap().ap = 2;
    assert!(host.walk_toward(player, goal, false, 0).unwrap());
    assert_eq!(hex(&host, player), hexes(20100, 1, 3));
    assert_eq!(host.world.free_movement, 0);
    assert_eq!(host.world.critter_state(player).unwrap().ap, 0);
    // A crippled leg costs 4 AP a hex: 5 AP walk two hexes.
    host.world.move_object(player, 20100, 0, false).unwrap();
    let state = host.world.critter_state_mut(player).unwrap();
    state.ap = 5;
    state.results.0 |= effect::CRIPPLED_LEFT_LEG;
    assert!(host.walk_toward(player, goal, false, 0).unwrap());
    assert_eq!(hex(&host, player), hexes(20100, 1, 2));
    // Keeping 2 of 2 AP leaves nothing to walk with.
    host.world.critter_state_mut(player).unwrap().ap = 2;
    assert!(!host.walk_toward(player, goal, false, 2).unwrap());
}

#[test]
fn spec007_pickups_reach_down_and_take_the_item_on_the_action_frame() {
    let mut host = host();
    host.playback = Playback::RealTime;
    // The ground reach: six frames at 10 fps, action frame A = 2.
    host.assets.art.insert(critter(10, 0), frm(6, 10, 2, STILL));
    host.assets.tokens.insert((0, 0), b"knife.frm".to_vec());
    let player = host.world.player;
    let item = place(&mut host, prototype(0, 5, &[], &[]), step(20100, 1));
    host.action(player, item, ObjectAction::Pickup).unwrap();
    // Registered: the first delay forced to 0, the turn and checks done,
    // the reach running, and the sound's delay A counted down once.
    assert_eq!(
        labels(&host.motions[0]),
        [
            row(0, "turn"),
            row(-1, "adjacent"),
            row(-1, "points"),
            row(0, "animate 10"),
            row(1, "sound KNIFE"),
            row(2, "pickup"),
        ]
    );
    host.step_motions(0);
    assert!(sounds(&mut host).is_empty());
    // The sound starts on the reach's step that shows frame A - 1.
    host.step_motions(100);
    assert_eq!(host.world.get(player).unwrap().raw.frame, 1);
    assert_eq!(sounds(&mut host), ["KNIFE"]);
    host.step_motions(200);
    assert_eq!(host.world.get(item).unwrap().owner, None);
    // The pickup's own delay A counts from the sound: frame 3.
    host.step_motions(300);
    assert_eq!(host.world.get(item).unwrap().owner, Some(player));
    host.step_motions(1000);
    assert!(!host.moving(player));
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        0
    );
}

#[test]
fn spec007_interaction_sequences_follow_the_section_3_11_3_table() {
    let mut host = host();
    let player = host.world.player;
    host.assets.tokens.insert((0, 0), b"knife".to_vec());
    for weapon in [0, 5] {
        host.assets
            .art
            .insert(critter(10, weapon), frm(6, 10, 3, STILL));
        host.assets
            .art
            .insert(critter(11, weapon), frm(6, 10, 2, STILL));
    }
    // A pickup at object distance 5 runs to the item.
    let far = place(&mut host, prototype(0, 5, &[], &[]), hexes(20100, 1, 5));
    let item = far;
    assert_eq!(
        labels(&host.pickup_motion(player, far).unwrap()),
        [
            (0, format!("run {:?}", Goal::Object(item))),
            row(-1, "turn"),
            row(-1, "adjacent"),
            row(-1, "points"),
            row(0, "animate 10"),
            row(3, "sound KNIFE"),
            row(3, "pickup"),
        ]
    );
    let sequence = host.pickup_motion(player, far).unwrap();
    assert!(sequence.schedule.entries()[2].forced, "the adjacency check");
    // At distance 4 it walks; in combat it walks.
    let near = place(&mut host, prototype(0, 5, &[], &[]), hexes(20100, 1, 4));
    let walk = |host: &WorldHost<Assets>, item| {
        host.pickup_motion(player, item).unwrap().schedule.entries()[0]
            .action
            .clone()
    };
    assert!(matches!(walk(&host, near), Motion::Walk(walk) if !walk.run));
    host.world.combat = true;
    host.world.critter_state_mut(player).unwrap().ap = 3;
    assert!(matches!(walk(&host, far), Motion::Walk(walk) if !walk.run && walk.limit == 3));
    host.world.combat = false;

    // Armed (a pistol, code 5): a closed container with interaction bit 0
    // (the ground reach) puts the weapon away, reaches, is used on the
    // reach's action frame (safe rule), takes the weapon out and is looted.
    host.world.get_mut(player).unwrap().raw.art_id = critter(0, 5);
    let ground = chest(&mut host, step(20100, 1), &[(0x3D, 1)]);
    assert_eq!(
        labels(&host.container_motion(player, ground).unwrap()),
        [
            row(-1, "turn"),
            row(-1, "adjacent"),
            row(-1, "points"),
            row(-1, "sound HMJMPSHD"),
            row(-1, "animate 39"),
            row(0, "animate 10"),
            row(3, "container"),
            row(-1, "animate 38 code 5"),
            row(-1, "loot"),
        ]
    );
    // An open middle-reach container is only used; the Remains of Gizmo
    // (PID 213) is never used and always looted.
    let middle = chest(&mut host, step(20100, 4), &[]);
    host.world.get_mut(middle).unwrap().raw.frame = 2;
    let entries = labels(&host.container_motion(player, middle).unwrap());
    assert_eq!(
        entries[5..],
        [
            row(0, "animate 11"),
            row(2, "container"),
            row(-1, "animate 38 code 5")
        ]
    );
    host.world.get_mut(middle).unwrap().raw.pid = 213;
    let entries = labels(&host.container_motion(player, middle).unwrap());
    assert_eq!(
        entries[5..],
        [
            row(0, "animate 11"),
            row(-1, "animate 38 code 5"),
            row(-1, "loot")
        ]
    );

    // Scenery with extended flag 0x01 takes the ground reach; armed, the
    // put-away animation has delay 0 and the reach -1.
    let lever = place(
        &mut host,
        prototype(2, 5, &[(0x18, 0x01)], &[]),
        step(20100, 2),
    );
    assert_eq!(
        labels(
            &host
                .use_motion(player, lever, Call::Use { target: lever })
                .unwrap()
        )[3..],
        [
            row(-1, "sound HMJMPSHD"),
            row(0, "animate 39"),
            row(-1, "animate 10"),
            row(-1, "use"),
            row(-1, "animate 38 code 5"),
        ]
    );
    // SPEC-013 §3.23 a ladder up: a walk to its southeast hex, the
    // adjacency check, the turn, the AP, the climb, the use; the climber
    // keeps its pose.
    host.world.get_mut(player).unwrap().raw.art_id = critter(0, 0);
    let ladder = place(&mut host, prototype(2, 3, &[], &[]), hexes(20100, 0, 2));
    let sequence = host.ladder_motion(player, ladder).unwrap();
    assert!(!sequence.reset);
    let spot = step(hexes(20100, 0, 2), 2);
    assert_eq!(
        labels(&sequence),
        [
            (0, format!("walk {:?}", Goal::Hex(spot))),
            row(-1, "adjacent"),
            row(-1, "turn"),
            row(-1, "points"),
            row(-1, "sound HMJMPSAE"),
            row(0, "animate 4"),
            row(-1, "use"),
        ]
    );
    // Without the reach art the whole sequence cannot register.
    host.assets.missing.insert(11);
    assert!(
        host.use_motion(player, middle, Call::Use { target: middle })
            .is_err()
    );
}

#[test]
fn spec007_doors_block_until_open_and_stay_passable_until_shut() {
    let mut host = host();
    host.playback = Playback::RealTime;
    let player = host.world.player;
    let at = step(20100, 1);
    let door = door(&mut host, at);
    let blocked = |host: &WorldHost<Assets>| host.world.movement_blocker(at, 0, player).is_some();
    assert!(blocked(&host));
    host.use_door(player, door).unwrap();
    // The open bit flips as the sequence starts; under the safe rule an
    // opening door plays the open sound; the flags wait for the end.
    assert!(lock(&host, door).contains(LockFlags::OPEN));
    assert_eq!(sounds(&mut host), ["SODOORSA"]);
    host.world.effects.clear();
    for (now, frame) in [(0, 0), (100, 1), (200, 2), (300, 3)] {
        host.step_motions(now);
        assert_eq!(host.world.get(door).unwrap().raw.frame, frame);
        assert!(blocked(&host), "blocks until fully open");
    }
    host.step_motions(400);
    let raw = &host.world.get(door).unwrap().raw;
    assert_eq!(raw.flags.0 & 0xA000_0010, 0xA000_0010);
    assert_eq!(raw.displacement, [3, 0]);
    assert!(!blocked(&host));
    // Closing: the bit clears at once and the close sound plays; the door
    // stays passable until it is shut.
    host.use_door(player, door).unwrap();
    assert!(!lock(&host, door).contains(LockFlags::OPEN));
    assert_eq!(sounds(&mut host), ["SCDOORSA"]);
    host.world.effects.clear();
    for (now, frame) in [(400, 3), (500, 2), (600, 1), (700, 0)] {
        host.step_motions(now);
        assert_eq!(host.world.get(door).unwrap().raw.frame, frame);
        assert!(!blocked(&host), "passable until shut");
    }
    host.step_motions(800);
    assert!(blocked(&host));
    assert_eq!(host.world.get(door).unwrap().raw.displacement, [0, 0]);
    // A locked door plays its locked sound and, with no script to stop
    // it, still opens.
    set_lock(&mut host, door, LockFlags::LOCKED.0);
    host.use_door(player, door).unwrap();
    assert_eq!(sounds(&mut host), ["SLDOORSA", "SODOORSA"]);
}

#[test]
fn spec007_using_a_door_walks_reaches_and_opens_it() {
    let mut host = host();
    let player = host.world.player;
    let at = hexes(20100, 1, 3);
    let door = door(&mut host, at);
    host.action(player, door, ObjectAction::Use).unwrap();
    // Instant playback: the walk, the middle reach and the door's own
    // sequence have all played.
    assert_eq!(host.world.object_distance(player, door), 1);
    let raw = &host.world.get(door).unwrap().raw;
    assert_eq!((raw.frame, raw.flags.0 & 0x10), (3, 0x10));
    assert_eq!(sounds(&mut host), ["SODOORSA"]);
    // Plain scenery without a script shows `proto.msg` 480.
    let beside = step(hex(&host, player), 5);
    let rock = place(&mut host, prototype(2, 5, &[(0x18, 0x800)], &[]), beside);
    host.world.get_mut(rock).unwrap().name = b"rock".to_vec();
    host.action(player, rock, ObjectAction::Use).unwrap();
    assert!(messages(&mut host).contains(&"You see: rock.".to_string()));
}

#[test]
fn spec007_containers_open_and_close_with_their_messages_and_no_flags() {
    let mut host = host();
    let player = host.world.player;
    let chest = chest(&mut host, step(20100, 1), &[]);
    host.action(player, chest, ObjectAction::Use).unwrap();
    assert_eq!(host.world.get(chest).unwrap().raw.frame, 2);
    assert_eq!(host.world.get(chest).unwrap().raw.flags.0 & 0xA000_0010, 0);
    assert_eq!(sounds(&mut host), ["IOCNTNRB"]);
    assert!(messages(&mut host).contains(&"You search the chest.".to_string()));
    assert_eq!(
        host.take_loot_request().map(|request| request.target),
        Some(chest)
    );
    host.world.effects.clear();
    // An open container only closes.
    host.action(player, chest, ObjectAction::Use).unwrap();
    assert_eq!(host.world.get(chest).unwrap().raw.frame, 0);
    assert_eq!(sounds(&mut host), ["ICCNTNRB"]);
    assert!(messages(&mut host).contains(&"You close the chest.".to_string()));
    assert_eq!(host.take_loot_request(), None);
    host.world.effects.clear();
    // A locked one plays its locked sound and says so, and its loot
    // request still stands.
    set_lock(&mut host, chest, LockFlags::LOCKED.0);
    host.action(player, chest, ObjectAction::Use).unwrap();
    assert_eq!(sounds(&mut host), ["ILCNTNRB"]);
    assert!(messages(&mut host).contains(&"It is locked.".to_string()));
    assert_eq!(host.world.get(chest).unwrap().raw.frame, 0);
    assert!(host.take_loot_request().is_some());
}

#[test]
fn spec007_script_open_and_close_skip_use_scripts_and_refuse_locks() {
    let mut host = host();
    let door = door(&mut host, step(20100, 1));
    host.script_open(door, true);
    let raw = &host.world.get(door).unwrap().raw;
    assert_eq!((raw.frame, raw.flags.0 & 0xA000_0010), (3, 0xA000_0010));
    assert!(lock(&host, door).contains(LockFlags::OPEN));
    assert_eq!(sounds(&mut host), ["SODOORSA"]);
    host.world.effects.clear();
    // Already open: nothing.
    host.script_open(door, true);
    assert!(sounds(&mut host).is_empty());
    // A locked door is refused, with no sound.
    set_lock(&mut host, door, LockFlags::LOCKED.0);
    host.script_open(door, false);
    assert!(sounds(&mut host).is_empty());
    assert!(lock(&host, door).contains(LockFlags::OPEN));
    // A jammed one closes, and its jam clears.
    let found = host.world.get_mut(door).unwrap();
    found.set_lock(LockFlags(LockFlags::OPEN.0 | LockFlags::JAMMED.0));
    host.script_open(door, false);
    assert_eq!(lock(&host, door), LockFlags(0));
    assert_eq!(host.world.get(door).unwrap().raw.frame, 0);
    assert_eq!(sounds(&mut host), ["SCDOORSA"]);
    // A container's state is its frame.
    let chest = chest(&mut host, step(20100, 4), &[]);
    host.script_open(chest, true);
    assert_eq!(host.world.get(chest).unwrap().raw.frame, 2);
}

#[test]
fn spec007_walkers_open_doors_they_may_use_and_walk_on() {
    let mut host = host();
    let walker = npc(&mut host, 20100 + 2);
    let at = hexes(20100 + 2, 1, 2);
    let door = door(&mut host, at);
    let goal = hexes(at, 1, 2);
    assert!(host.walk_toward(walker, goal, false, 0).unwrap());
    assert_eq!(hex(&host, walker), goal);
    assert!(lock(&host, door).contains(LockFlags::OPEN));
    assert_eq!(host.world.get(door).unwrap().raw.flags.0 & 0x10, 0x10);
    assert_eq!(sounds(&mut host), ["SODOORSA"]);
}

#[test]
fn spec007_a_new_request_stops_the_walking_player_first() {
    let mut host = host();
    host.playback = Playback::RealTime;
    let shifts = HEX_STEPS.map(|[x, y]| [(x / 4) as i16, (y / 4) as i16]);
    host.assets.art.insert(critter(1, 0), frm(8, 10, 0, shifts));
    let player = host.world.player;
    assert!(
        host.walk_toward(player, hexes(20100, 1, 6), false, 0)
            .unwrap()
    );
    host.step_motions(0);
    host.step_motions(100);
    assert_ne!(host.world.get(player).unwrap().raw.displacement, [0, 0]);
    // The new walk replaces the old one; the walker stands on its hex.
    assert!(
        host.walk_toward(player, hexes(20100, 4, 2), false, 0)
            .unwrap()
    );
    assert_eq!(host.motions.len(), 1);
    assert_eq!(host.world.get(player).unwrap().raw.displacement, [0, 0]);
    // Busy with anything else (a reach), the player ignores a request.
    host.motions.clear();
    host.assets.art.insert(critter(10, 0), frm(6, 10, 2, STILL));
    let item = place(&mut host, prototype(0, 5, &[], &[]), step(20100, 1));
    host.action(player, item, ObjectAction::Pickup).unwrap();
    assert!(
        host.walk_toward(player, hexes(20100, 4, 2), false, 0)
            .unwrap()
    );
    assert_eq!(host.motions.len(), 1);
    assert!(!host.motions[0].walking(player));
}

/// The attack shows queued, as (delay, what each entry does).
fn shown(host: &WorldHost<Assets>) -> Vec<(i32, String)> {
    use crate::choreography::Action;
    host.world
        .shows
        .iter()
        .flat_map(|show| show.main.entries())
        .map(|entry| {
            let label = match &entry.action {
                Action::Sound { name, .. } => format!("sound {}", String::from_utf8_lossy(name)),
                Action::Animate { art, .. } => format!("animate {}", Fid(*art).animation()),
                Action::Move { .. } => "move".into(),
                Action::Walk { route, .. } => format!("walk {route:?}"),
                Action::Corpse { .. } => "corpse".into(),
                _ => "other".into(),
            };
            (entry.delay, label)
        })
        .collect()
}
/// A living NPC with `hp` hit points.
fn living(host: &mut WorldHost<Assets>, hex: i32, hp: i32) -> ObjectRef {
    let critter = npc(host, hex);
    host.world.set_hit_points(critter, hp);
    critter
}

#[test]
fn spec007_idle_fidgets_play_a_standing_critter_on_the_screen() {
    use crate::RandomSource;
    let mut host = host();
    host.playback = Playback::RealTime;
    host.assets.art.insert(critter(0, 0), frm(4, 10, 0, STILL));
    // A male stem ("m...") near the player, and a critter off the screen.
    host.assets.tokens.insert((1, 1), b"maboss".to_vec());
    host.assets
        .art
        .insert(Fid::new(1, 1, 0, 0, 0).0, frm(4, 10, 0, STILL));
    let player = host.world.player;
    host.world.set_hit_points(player, 10);
    let near = living(&mut host, hexes(20100, 1, 3), 10);
    host.world.get_mut(near).unwrap().raw.art_id = Fid::new(1, 1, 0, 0, 0).0;
    let far = living(&mut host, 39000, 10);
    host.world.get_mut(far).unwrap().raw.art_id = Fid::new(1, 1, 0, 0, 0).0;
    // The task starts with the first pass and runs once time has passed.
    let initial = host.world.rng.clone();
    host.tick(16).unwrap();
    assert_eq!(host.world.rng, initial);
    // Two candidates: one pick, then a wait of random(0, 3000) + 7000
    // (k = trunc(20 / 2) clamped to 7).
    let mut expected = host.world.rng.clone();
    let pick = expected.range(0, 1).unwrap();
    let wait = expected.range(0, 3000).unwrap() as u64 + 7000;
    host.tick(32).unwrap();
    assert_eq!(host.world.rng, expected);
    let picked = [player, near][pick as usize];
    let sequence = host.motions.last().unwrap();
    assert!(sequence.insignificant && sequence.names(picked));
    let name = if picked == player {
        "HMJMPSAA"
    } else {
        "MABOSSAA"
    };
    assert_eq!(
        labels(sequence),
        [(0, format!("sound {name}")), row(0, "play 0 Forward")]
    );
    // Nothing more until the wait has passed.
    let state = host.world.rng.clone();
    host.tick(32 + wait).unwrap();
    assert_eq!(host.world.rng, state);
    host.tick(33 + wait).unwrap();
    assert_ne!(host.world.rng, state);
    // A later sequence naming the fidgeting critter ends the fidget.
    host.motions.clear();
    host.fidget = None;
    host.tick(20_000).unwrap();
    host.tick(20_001).unwrap();
    let fidgeting = host.motions.last().unwrap().owners()[0];
    assert!(
        host.walk_toward(fidgeting, hexes(20100, 4, 3), false, 0)
            .unwrap()
    );
    assert!(host.motions.iter().all(|sequence| !sequence.insignificant));
    // None in combat, nor while the player is hidden.
    host.motions.clear();
    host.world.combat = true;
    let state = host.world.rng.clone();
    host.tick(40_000).unwrap();
    assert!(host.fidget.is_none());
    host.world.combat = false;
    host.world.get_mut(player).unwrap().raw.flags.0 |= BasicFlags::HIDDEN.0;
    host.tick(40_016).unwrap();
    host.tick(50_000).unwrap();
    assert_eq!(host.world.rng, state);
}

#[test]
fn spec007_script_damage_shows_a_hit_that_slides_falls_and_gets_up() {
    let mut host = host();
    host.playback = Playback::RealTime;
    let victim = living(&mut host, 20300, 100);
    // Facing SE: hit from the front. 25 damage slides it 2 hexes SE; it
    // falls back (20) and gets up at once (37), with no knockdown.
    host.world.get_mut(victim).unwrap().raw.orientation = 2;
    host.present_script_damage(victim, 25, 0, false, false)
        .unwrap();
    assert_eq!(hex(&host, victim), hexes(20300, 2, 2));
    assert_eq!(
        shown(&host),
        [
            row(0, "sound whc1xxx1"),
            row(0, "move"),
            row(-1, "animate 37"),
        ]
    );
    assert_eq!(
        host.world.critter_state(victim).unwrap().results.0 & effect::KNOCKED_DOWN,
        0
    );
    // Electrical damage, bypassed armor and a multihex victim slide no
    // hexes: a plain flinch.
    for (kind, bypassed) in [(4, false), (0, true)] {
        host.world.shows.clear();
        let at = hex(&host, victim);
        host.present_script_damage(victim, 25, kind, bypassed, false)
            .unwrap();
        assert_eq!(hex(&host, victim), at);
        assert_eq!(
            shown(&host)[1..],
            [row(0, "sound HMJMPSAO"), row(0, "animate 14")]
        );
    }
    // A lethal hit takes the maximum-violence column for its damage type
    // (normal: 25, chunks), under maximum-violence validation.
    host.world.shows.clear();
    host.world.violence = Some(3);
    host.world.set_hit_points(victim, 0);
    host.world.critter_state_mut(victim).unwrap().results.0 |= effect::DEAD;
    host.present_script_damage(victim, 9, 0, false, true)
        .unwrap();
    let rows = shown(&host);
    assert!(rows.contains(&row(0, "animate 25")), "{rows:?}");
    assert_eq!(rows.last().unwrap().1, "corpse");
}

#[test]
fn spec007_knockouts_wake_into_a_knockdown_and_get_up_outside_combat() {
    use crate::queue::EventKind;
    let mut host = host();
    host.playback = Playback::RealTime;
    let critter = living(&mut host, 20300, 10);
    let fallen = |host: &mut WorldHost<Assets>, animation: u8| {
        host.world.get_mut(critter).unwrap().raw.art_id = Fid::new(1, 0, animation, 0, 0).0;
        host.world.critter_state_mut(critter).unwrap().results.0 = effect::KNOCKED_OUT;
        let now = host.world.clock.ticks;
        host.world
            .queue
            .insert(now, 0, critter, EventKind::Knockout)
            .unwrap();
        host.process_queue().unwrap();
        host.world.critter_state(critter).unwrap().results.0
    };
    // In combat the knockout only turns into a knockdown.
    host.world.combat = true;
    assert_eq!(fallen(&mut host, 21), effect::KNOCKED_DOWN);
    assert!(host.motions.is_empty());
    // Outside combat the critter gets up at once: 37 after a fall back.
    host.world.combat = false;
    assert_eq!(fallen(&mut host, 20), 0);
    assert_eq!(labels(&host.motions[0]), [row(0, "play 37 Forward")]);
    host.motions.clear();
    assert_eq!(fallen(&mut host, 21), 0);
    assert_eq!(labels(&host.motions[0]), [row(0, "play 36 Forward")]);
}

#[test]
fn spec007_combat_cleanup_gets_a_prone_player_up_unless_a_script_ended_it() {
    use crate::queue::EventKind;
    let mut host = host();
    host.playback = Playback::RealTime;
    let player = host.world.player;
    let down = |host: &mut WorldHost<Assets>, flags: u32| {
        host.world.get_mut(player).unwrap().raw.art_id = Fid::new(1, 0, 20, 0, 0).0;
        host.world.critter_state_mut(player).unwrap().results.0 = flags;
        let now = host.world.clock.ticks;
        host.world
            .queue
            .insert(now, 600, player, EventKind::Knockout)
            .unwrap();
        host.world.combat = true;
    };
    let waking = |host: &WorldHost<Assets>| {
        host.world
            .queue
            .events()
            .any(|event| event.object == player && event.kind == EventKind::Knockout)
    };
    // Knocked down: the wake event goes and the player gets up (37).
    down(&mut host, effect::KNOCKED_DOWN);
    host.end_combat().unwrap();
    assert_eq!(host.world.critter_state(player).unwrap().results.0, 0);
    assert!(!waking(&host));
    assert_eq!(labels(&host.motions[0]), [row(0, "play 37 Forward")]);
    // A script ended combat while the player was out: the knockout stays.
    host.motions.clear();
    down(&mut host, effect::KNOCKED_OUT);
    host.world.combat_quit = true;
    host.end_combat().unwrap();
    assert_eq!(
        host.world.critter_state(player).unwrap().results.0,
        effect::KNOCKED_OUT
    );
    assert!(waking(&host) && host.motions.is_empty());
}
