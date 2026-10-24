//! SPEC-005 §3.2 self-stop: the execution-cancellation bit `0x0100` that
//! `kill_critter`, `destroy_object` and `critter_damage` set on the running
//! program when their object is its own owner.
use crate::fixture_state as fixture;
use crate::{assets::WorldAssets, world::ScriptRequest, *};
use std::sync::Arc;
use vault_formats::{Prototype, pro::BasicFlags};
use vault_vm::{IntProgram, VmStatus};

/// `gvar[index] += 1`.
fn count(index: i32) -> Vec<u8> {
    [
        fixture::push(index),
        fixture::push(index),
        fixture::instruction(0x80c5),
        fixture::push(1),
        fixture::instruction(0x8039),
        fixture::instruction(0x80c6),
    ]
    .concat()
}
fn program(procedures: &[(&[u8], Vec<u8>)]) -> IntProgram {
    let procedures: Vec<(&[u8], Vec<u8>)> = procedures
        .iter()
        .map(|(name, body)| (*name, fixture::returning(body.clone())))
        .collect();
    IntProgram::parse(&fixture::program(&[], &procedures)).unwrap()
}
const SCRIPT_OVERRIDES: u16 = 0x80b9;
const SELF_OBJ: u16 = 0x80bc;
const KILL_CRITTER: u16 = 0x80ed;
const KILL_CRITTER_TYPE: u16 = 0x80ee;
const CRITTER_DAMAGE: u16 = 0x80ef;
const DESTROY_OBJECT: u16 = 0x80f4;
/// The requests every program answers: `look_at_p_proc` counts gvar 2,
/// `damage_p_proc` gvar 4, `destroy_p_proc` gvar 3, `talk_p_proc` gvar 6.
fn handlers() -> Vec<(&'static [u8], Vec<u8>)> {
    vec![
        (b"look_at_p_proc", count(2)),
        (b"destroy_p_proc", count(3)),
        (b"damage_p_proc", count(4)),
        (b"talk_p_proc", count(6)),
    ]
}
/// A program whose `use_p_proc` overrides, runs `action`, then counts
/// gvar 1 (the rest of the procedure).
fn acting(action: Vec<u8>) -> IntProgram {
    let mut procedures: Vec<(&[u8], Vec<u8>)> = vec![
        (b"start", Vec::new()),
        (
            b"use_p_proc",
            [fixture::instruction(SCRIPT_OVERRIDES), action, count(1)].concat(),
        ),
    ];
    procedures.extend(handlers());
    program(&procedures)
}
/// `op` on the script's own object (`self_obj`), with `after` pushed
/// after the object.
fn on_self(op: u16, after: &[i32]) -> Vec<u8> {
    let mut code = fixture::instruction(SELF_OBJ);
    for value in after {
        code.extend(fixture::push(*value));
    }
    code.extend(fixture::instruction(op));
    code
}
fn on_object(op: u16, object: u32, after: &[i32]) -> Vec<u8> {
    let mut code = fixture::push(object as i32);
    for value in after {
        code.extend(fixture::push(*value));
    }
    code.extend(fixture::instruction(op));
    code
}

struct Assets {
    critter: Arc<Prototype>,
    item: Arc<Prototype>,
    programs: Vec<IntProgram>,
}
impl WorldAssets for Assets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        Ok(if pid >> 24 == 1 {
            self.critter.clone()
        } else {
            self.item.clone()
        })
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.programs.get(index))
            .map(|program| ("synthetic.int".into(), program.clone()))
            .ok_or_else(|| WorldError::Invalid(format!("script {index} does not load")))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn game_messages(&self, file: &str) -> Result<Arc<vault_formats::MsgCatalog>, WorldError> {
        if file != "combat.msg" {
            return Err(WorldError::Invalid(format!("{file} is missing")));
        }
        let text = b"{108}{}{ and }\n{307}{}{was killed}\n{613}{}{%s was hit for %d hit points}\n";
        Ok(Arc::new(vault_formats::MsgCatalog::parse(text).unwrap()))
    }
}

/// Script file 0 is the player's (it only has `start`); the test adds
/// its own programs from file 1.
fn host() -> WorldHost<Assets> {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 8]);
    WorldHost::new(
        world,
        Assets {
            critter: Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap()),
            item: Arc::new(Prototype::parse(&fixture::world::prototype(0, 5)).unwrap()),
            programs: vec![program(&[(b"start", Vec::new())])],
        },
    )
}
fn request(script: u32, event: ScriptEvent) -> ScriptRequest {
    ScriptRequest {
        script,
        event,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    }
}
/// A critter at `hex` with a script running `program`; its first request
/// (the program load) is made.
fn scripted_critter(host: &mut WorldHost<Assets>, hex: i32, program: IntProgram) -> (u32, u32) {
    let critter = host
        .world
        .spawn(host.assets.critter.clone(), hex, 0)
        .unwrap();
    let sid = scripted(host, critter, program);
    (critter, sid)
}
fn scripted(host: &mut WorldHost<Assets>, object: u32, program: IntProgram) -> u32 {
    host.assets.programs.push(program);
    let file = host.assets.programs.len() as i32 - 1;
    let sid = host.world.add_script(object, file, 4, 0).unwrap();
    host.dispatch(request(sid, ScriptEvent::Start)).unwrap();
    sid
}
fn gvar(host: &WorldHost<Assets>, index: usize) -> i32 {
    host.world.gvars.as_ref().unwrap()[index]
}
fn program_of(host: &WorldHost<Assets>, sid: u32) -> u32 {
    host.world.scripts[&sid].program
}
fn stopped(host: &WorldHost<Assets>, program: u32) -> bool {
    host.programs[&program].self_stopped()
}

#[test]
fn spec005_kill_critter_on_self_stops_the_run_and_later_requests_run_nothing() {
    let mut host = host();
    let (critter, sid) = scripted_critter(&mut host, 20102, acting(on_self(KILL_CRITTER, &[48])));
    host.world.scripts.get_mut(&sid).unwrap().result = 7;
    // The current run: the kill completes, the rest of `use_p_proc` does
    // not run, and the request returns as if the procedure had ended, with
    // the override set before the opcode.
    let result = host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert!(host.world.is_dead(critter));
    assert_eq!(gvar(&host, 1), 0, "the rest of the procedure does not run");
    assert!(result.overridden && !result.pending);
    assert_eq!(result.result, 7);
    let program = program_of(&host, sid);
    assert!(stopped(&host, program));
    assert!(
        host.programs[&program]
            .status
            .contains(VmStatus::SELF_STOPPED)
    );
    // Later requests run nothing, store their action number, clear the
    // override and leave the result: default actions proceed.
    for (event, code) in [(ScriptEvent::LookAt, 21), (ScriptEvent::Talk, 11)] {
        let result = host.dispatch(request(sid, event)).unwrap();
        assert!(!result.overridden && !result.pending);
        assert_eq!(result.result, 7);
        let instance = &host.world.scripts[&sid];
        assert_eq!((instance.event, instance.overridden), (code, false));
    }
    assert_eq!((gvar(&host, 2), gvar(&host, 6)), (0, 0));
    // The VM time slice skips it too, and the bit stays.
    host.update_programs().unwrap();
    assert!(stopped(&host, program));
    // Lifetime: a map load frees every program, the bit with it.
    let _ = host.load_map("maps/none.map");
    assert!(!host.programs.contains_key(&program));
}

#[test]
fn spec005_critter_damage_on_self_skips_its_own_damage_request() {
    let mut host = host();
    // Bypass armor (0x100) and no animation (0x200): exactly 5 damage.
    let (critter, sid) = scripted_critter(
        &mut host,
        20102,
        acting(on_self(CRITTER_DAMAGE, &[5, 0x300])),
    );
    let before = host.world.hit_points(critter).unwrap();
    let result = host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert_eq!(host.world.hit_points(critter), Some(before - 5));
    // The program is host-busy during the opcode: its own `damage_p_proc`
    // request is skipped.
    assert_eq!(gvar(&host, 4), 0);
    assert_eq!(gvar(&host, 1), 0);
    assert!(result.overridden && !result.pending);
    assert!(stopped(&host, program_of(&host, sid)));
    // Damage aimed at another critter: its `damage_p_proc` runs, and the
    // damaging script goes on.
    let (_, victim_sid) = scripted_critter(&mut host, 20104, acting(Vec::new()));
    let victim = host.world.scripts[&victim_sid].owner;
    let (_, sid) = scripted_critter(
        &mut host,
        20106,
        acting(on_object(CRITTER_DAMAGE, victim, &[5, 0x300])),
    );
    host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert_eq!((gvar(&host, 4), gvar(&host, 1)), (1, 1));
    assert!(!stopped(&host, program_of(&host, sid)));
}

#[test]
fn spec005_destroy_object_on_self_returns_the_override_set_before_it() {
    let mut host = host();
    let (critter, sid) = scripted_critter(&mut host, 20102, acting(on_self(DESTROY_OBJECT, &[])));
    let program = program_of(&host, sid);
    let result = host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    // The object and its script instance are gone; the request still
    // returns the override the script set before the opcode.
    assert!(host.world.get(critter).is_none());
    assert!(!host.world.scripts.contains_key(&sid));
    assert!(result.overridden && !result.pending);
    assert_eq!(gvar(&host, 1), 0);
    assert!(stopped(&host, program));
    // The null-object early return does not stop the program.
    let (_, sid) = scripted_critter(&mut host, 20104, acting(on_object(DESTROY_OBJECT, 0, &[])));
    host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert_eq!(gvar(&host, 1), 1);
    assert!(!stopped(&host, program_of(&host, sid)));
}

#[test]
fn spec005_a_held_item_that_destroys_itself_keeps_its_instance() {
    let mut host = host();
    let player = host.world.player;
    let item = host
        .world
        .spawn(host.assets.item.clone(), 20110, 0)
        .unwrap();
    host.world.add_item(player, item, 1, true).unwrap();
    let sid = scripted(&mut host, item, acting(on_self(DESTROY_OBJECT, &[])));
    let result = host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert!(result.overridden && !result.pending);
    // Out of the inventory but not freed: detached from its script, hidden
    // and temporary; the instance stays until the map is left.
    let held = host.world.get(player).unwrap();
    assert!(held.inventory.iter().all(|(object, _)| *object != item));
    let object = host.world.get(item).unwrap();
    assert_eq!((object.owner, object.raw.script_id), (None, -1));
    assert!(object.raw.flags.contains(BasicFlags::HIDDEN));
    assert!(object.raw.flags.contains(BasicFlags::TEMPORARY));
    assert!(host.world.scripts.contains_key(&sid));
    assert!(stopped(&host, program_of(&host, sid)));
}

#[test]
fn spec005_nothing_else_sets_the_self_stop_bit() {
    let mut host = host();
    // `kill_critter_type` on the script's own PID kills its own object but
    // does not stop it.
    let pid = host.assets.critter.pid;
    let code = [fixture::push(pid as i32), fixture::push(2)].concat();
    let (critter, sid) = scripted_critter(
        &mut host,
        20102,
        acting([code, fixture::instruction(KILL_CRITTER_TYPE)].concat()),
    );
    host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert!(host.world.is_dead(critter));
    assert_eq!(gvar(&host, 1), 1);
    assert!(!stopped(&host, program_of(&host, sid)));
    // `kill_critter` aimed at another critter leaves the killer running.
    let other = host
        .world
        .spawn(host.assets.critter.clone(), 20108, 0)
        .unwrap();
    let (_, sid) = scripted_critter(
        &mut host,
        20104,
        acting(on_object(KILL_CRITTER, other, &[48])),
    );
    host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
    assert!(host.world.is_dead(other));
    assert_eq!(gvar(&host, 1), 2);
    assert!(!stopped(&host, program_of(&host, sid)));
}

#[test]
fn spec007_scripted_damage_prints_its_combat_line_before_it_applies() {
    let mut host = host();
    let (_, victim_sid) = scripted_critter(&mut host, 20104, acting(Vec::new()));
    let victim = host.world.scripts[&victim_sid].owner;
    host.world.get_mut(victim).unwrap().name = b"Raider".to_vec();
    let hp = host.world.hit_points(victim).unwrap();
    let damage = |host: &mut WorldHost<Assets>, amount: i32| {
        let (_, sid) = scripted_critter(
            host,
            20106,
            acting(on_object(CRITTER_DAMAGE, victim, &[amount, 0x300])),
        );
        host.world.effects.clear();
        host.dispatch(request(sid, ScriptEvent::Use)).unwrap();
        host.world
            .effects
            .iter()
            .filter_map(|effect| match effect {
                WorldEffect::Message(text) => Some(String::from_utf8(text.clone()).unwrap()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(damage(&mut host, 5), ["Raider was hit for 5 hit points."]);
    // Lethal damage: the critter was alive before it, so it gets its line.
    assert_eq!(
        damage(&mut host, hp - 5),
        ["Raider was hit for 25 hit points and was killed."]
    );
    // Dead before the damage: no line.
    assert!(damage(&mut host, 5).is_empty());
}
