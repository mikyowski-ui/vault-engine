use crate::{
    assets::{VfsWorldAssets, WorldAssets},
    dialogue::*,
    library::SoundState,
    queue::{EventKind, TimedQueue},
    rng::{skill_check, stat_check},
    world::WorldTrace,
    *,
};
use std::{collections::VecDeque, sync::Arc};
use vault_formats::{FrmArchive, GcdFile, MapFile, Prototype, pro::Category};
use vault_vm::{
    IntProgram, RunOutcome, Value, Vm, VmError,
    extensions::{self, Rule},
};
#[allow(dead_code)]
#[path = "../../../tests/fixtures/state/support.rs"]
mod fixture;

#[derive(Default)]
struct Dice {
    values: VecDeque<i32>,
    draws: usize,
}
impl Dice {
    fn new(values: &[i32]) -> Self {
        Self {
            values: values.iter().copied().collect(),
            draws: 0,
        }
    }
}
impl RandomSource for Dice {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        self.draws += 1;
        let value = self.values.pop_front().expect("specified draw");
        assert!((first..=last).contains(&value));
        Ok(value)
    }
}
#[test]
fn spec006_rng_snapshot_seed_sign_rules_and_checks() {
    let mut rng = ParkMiller::seeded(1).unwrap();
    assert_eq!(
        (0..6).map(|_| rng.draw()).collect::<Vec<_>>(),
        [0, 812987216, 1817129560, 1441282327, 16531729, 1998097157]
    );
    let mut rng = ParkMiller::seeded(1).unwrap();
    assert_eq!(
        (0..6)
            .map(|_| rng.range(1, 100).unwrap())
            .collect::<Vec<_>>(),
        [1, 17, 61, 28, 30, 58]
    );
    let snapshot = rng.state().clone();
    let mut restored = ParkMiller::from_state(snapshot.clone()).unwrap();
    for _ in 0..100 {
        assert_eq!(rng.draw(), restored.draw());
    }
    let retained = rng.state().shuffle[0];
    rng.seed(1, None).unwrap();
    assert_eq!(rng.state().shuffle[0], retained);
    assert_ne!(rng.state(), ParkMiller::seeded(1).unwrap().state());
    assert!(ParkMiller::seeded(-1).is_err());
    rng.seed(-1, Some([0, 1])).unwrap();
    assert_eq!(
        ParkMiller::seeded(0).unwrap(),
        ParkMiller::seeded(-2).unwrap()
    );
    let mut equal = ParkMiller::from_state(snapshot).unwrap();
    let before = equal.state().clone();
    assert_eq!(equal.range(9, 9).unwrap(), 9);
    assert_ne!(&before, equal.state());
    for (draws, ticks, code, margin) in [
        (&[42, 7][..], 864000, 3, 26),
        (&[42, 8][..], 864000, 2, 26),
        (&[93, 2][..], 864000, 0, -25),
        (&[42][..], 863999, 2, 26),
    ] {
        let mut dice = Dice::new(draws);
        let check = skill_check(68, 0, 5, ticks, &mut dice).unwrap();
        assert_eq!((check.code, check.margin), (code, margin));
        assert_eq!(dice.draws, draws.len());
    }
    let mut dice = Dice::new(&[6]);
    let check = stat_check(5, 1, &mut dice).unwrap();
    assert_eq!((check.code, check.margin, dice.draws), (2, 0, 1));
}
fn derived(sheet: &CharacterSheet) -> Vec<i32> {
    [7, 8, 9, 11, 12, 13, 14, 15]
        .into_iter()
        .map(|i| sheet.stat(i, StatContext::default()))
        .collect()
}
#[test]
fn spec006_player_stats_all_skills_traits_perks_and_hp_history() {
    let bytes = fixture::gcd();
    let gcd = GcdFile::parse(&bytes).unwrap();
    let context = StatContext::default();
    let sheet = CharacterSheet::from_gcd(&gcd, context);
    assert_eq!(
        (0..7).map(|i| sheet.stat(i, context)).collect::<Vec<_>>(),
        [7, 5, 5, 5, 5, 5, 5]
    );
    assert_eq!(derived(&sheet), [30, 7, 5, 2, 200, 10, 1, 5]);
    assert_eq!(sheet.stat(29, context), 100);
    assert_eq!(
        (0..18).map(|i| sheet.skill(i, context)).collect::<Vec<_>>(),
        [
            68, 15, 15, 71, 61, 45, 35, 20, 30, 25, 25, 25, 35, 25, 59, 30, 35, 10
        ]
    );
    let mut mutated = bytes;
    mutated[0x20..0x24].copy_from_slice(&777_i32.to_be_bytes());
    assert_eq!(
        CharacterSheet::from_gcd(&GcdFile::parse(&mutated).unwrap(), context).base[7],
        30
    );
    let mut sheet = CharacterSheet::default();
    assert_eq!(derived(&sheet), [30, 7, 5, 1, 150, 10, 1, 5]);
    sheet.traits = [Trait::Gifted as i32, Trait::SmallFrame as i32];
    sheet.recompute(context);
    sheet.current_hp = 33;
    assert_eq!(derived(&sheet), [33, 8, 7, 1, 125, 12, 2, 6]);
    sheet.tags[0] = 0;
    assert_eq!(
        (
            sheet.skill(0, context),
            sheet.skill(12, context),
            sheet.skill(14, context)
        ),
        (52, 27, 27)
    );
    for (perk, rank) in [
        (Perk::ActionBoy, 2),
        (Perk::Dodger, 1),
        (Perk::BonusHthDamage, 1),
        (Perk::StrongBack, 1),
        (Perk::EarlierSequence, 1),
        (Perk::FasterHealing, 1),
        (Perk::MoreCriticals, 1),
    ] {
        sheet.set_perk(perk, rank, context);
    }
    assert_eq!(derived(&sheet), [33, 10, 12, 3, 175, 14, 3, 11]);
    sheet.set_perk(Perk::ActionBoy, 2, context);
    assert_eq!(sheet.stat(8, context), 10);
    sheet.set_perk(Perk::Lifegiver, 1, context);
    sheet.advance_level(context);
    assert_eq!((sheet.stat(7, context), sheet.current_hp), (42, 33));
    sheet.set_perk(Perk::Lifegiver, 2, context);
    assert_eq!(sheet.stat(7, context), 42);
    sheet.advance_level(context);
    assert_eq!(sheet.stat(7, context), 55);
    for trait_id in 0..16 {
        let mut sheet = CharacterSheet {
            traits: [trait_id, trait_id],
            ..Default::default()
        };
        sheet.recompute(context);
        let first = derived(&sheet);
        sheet.traits[1] = -1;
        sheet.recompute(context);
        assert_eq!(derived(&sheet), first);
    }
    let mut sheet = CharacterSheet {
        traits: [Trait::FastMetabolism as i32, -1],
        ..Default::default()
    };
    sheet.recompute(context);
    assert_eq!(
        (
            sheet.stat(31, context),
            sheet.stat(32, context),
            sheet.stat(14, context)
        ),
        (20, 50, 3)
    );
    sheet.traits = [Trait::NightPerson as i32, -1];
    assert_eq!(sheet.stat(4, context), 4);
    assert_eq!(
        sheet.stat(
            1,
            StatContext {
                ticks: 18 * 36000,
                ..context
            }
        ),
        6
    );
    sheet.traits = [Trait::Skilled as i32, Trait::Gifted as i32];
    sheet.level = 4;
    assert_eq!(sheet.reconcile_levels(context), 1);
    assert_eq!(sheet.unspent_skills, 36); // 5 + 2*6 - 5, three levels; no invented Skilled +5.
    sheet.invested[0] = -7;
    assert_eq!(sheet.skill(0, context), -7);
    sheet.invested[0] = 500;
    assert_eq!(sheet.skill(0, context), 200);
}
#[test]
fn spec006_queue_iq_reaction_and_art_vectors() {
    let mut queue = TimedQueue::default();
    for object in 1..=3 {
        queue
            .insert(1000, 10, object, EventKind::Other(object))
            .unwrap();
    }
    assert_eq!(queue.process(1009, |_, _| panic!()).unwrap(), 0);
    let mut seen = Vec::new();
    queue
        .process(1010, |_, event| {
            seen.push(event.object);
            event.object == 2
        })
        .unwrap();
    assert_eq!(seen, [1, 2]);
    assert_eq!(queue.events().count(), 1);
    queue
        .process(1010, |_, event| {
            seen.push(event.object);
            false
        })
        .unwrap();
    assert_eq!(seen, [1, 2, 3]);
    assert!(!iq_allows(3, 0, 4));
    assert!(iq_allows(3, 0, -4));
    assert!(iq_allows(3, 0, -3));
    assert!(iq_allows(3, 1, 4));
    assert!(iq_allows(3, 1, -4));
    assert!(!iq_allows(3, 1, -3));
    assert!(iq_allows(10, 3, 12));
    let head = HeadDefinition {
        index: 7,
        stem: "synth".into(),
        counts: [3; 3],
    };
    assert_eq!(head.art_id(4, 2).unwrap(), 0x08042007);
    assert_eq!(head.path(4, 2).unwrap(), "art/heads/synthnf2.frm");
    assert_eq!(head.art_id(10, 0).unwrap(), 0x080a0007);
    assert_eq!(head.art_id(3, 0).unwrap(), 0x08030007);
    let mut animation =
        TalkingHead::new(head.clone(), Mood::Neutral, 0, &mut Dice::new(&[77])).unwrap();
    animation.phoneme(41).unwrap();
    assert_eq!(
        (animation.art_id().unwrap(), animation.frame),
        (0x080a0007, 8)
    );
    animation.react(49);
    animation.tick(100, 2, 10, &mut Dice::default()).unwrap();
    assert_eq!(animation.art_id().unwrap(), 0x08030007);
    assert_eq!(
        PHONEMES,
        [
            0, 3, 1, 1, 3, 1, 1, 1, 7, 8, 7, 3, 1, 8, 1, 7, 7, 6, 6, 2, 2, 2, 2, 4, 4, 5, 5, 2, 2,
            2, 2, 2, 6, 2, 2, 5, 8, 2, 2, 2, 2, 8
        ]
    );
    for (count, draw, expected) in [
        (2, 67, 1),
        (2, 68, 2),
        (3, 51, 1),
        (3, 52, 2),
        (3, 76, 2),
        (3, 77, 3),
        (5, 1, 5),
    ] {
        assert_eq!(
            choose_fidget(count, &mut 0, &mut Dice::new(&[draw])).unwrap(),
            expected
        );
    }
    let mut session = GDialog::default();
    session.start(1, 1, 5, None).unwrap();
    assert_eq!(session.background_art(), 0x09000005);
    session.start(1, 1, -1, None).unwrap();
    assert_eq!(session.background, 5);
    let mut locals = [0; 4];
    assert_eq!(
        npc_reaction(&mut locals, 5, 0, false, Reputation::default()),
        50
    );
    assert_eq!(&locals[..3], &[50, 2, 1]);
    assert_eq!(
        npc_reaction(
            &mut locals,
            10,
            10,
            true,
            Reputation {
                value: 999,
                ..Default::default()
            }
        ),
        50
    );
}
struct Assets {
    program: IntProgram,
    prototype: Arc<Prototype>,
    map: MapFile,
}
impl Assets {
    fn new(strings: &[&[u8]]) -> Self {
        let prototype = Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap());
        let map = MapFile::parse(&fixture::world::preview_map(), &|pid: u32| {
            Ok(Prototype::parse(&fixture::world::prototype(
                (pid >> 24) as u8,
                if pid >> 24 == 0 { 1 } else { 5 },
            ))
            .unwrap()
            .info())
        })
        .unwrap();
        Self {
            program: IntProgram::parse(&fixture::context_program(strings)).unwrap(),
            prototype,
            map,
        }
    }
}
impl WorldAssets for Assets {
    fn prototype(&self, _pid: u32) -> Result<Arc<Prototype>, WorldError> {
        Ok(self.prototype.clone())
    }
    fn program(&self, _index: i32) -> Result<(String, IntProgram), WorldError> {
        Ok(("synthetic.int".into(), self.program.clone()))
    }
    fn local_count(&self, _index: i32) -> usize {
        4
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn map(&self, _path: &str) -> Result<MapFile, WorldError> {
        Ok(self.map.clone())
    }
    fn head(&self, index: i32) -> Result<HeadDefinition, WorldError> {
        Ok(HeadDefinition {
            index: index as u16,
            stem: "synth".into(),
            counts: [3; 3],
        })
    }
    fn frame(&self, _path: &str) -> Result<Arc<FrmArchive>, WorldError> {
        Ok(Arc::new(
            FrmArchive::parse(&fixture::animated_frame(12, 20, &[1, 2])).unwrap(),
        ))
    }
}
fn host(strings: &[&[u8]]) -> (WorldHost<Assets>, Vm, u32) {
    let assets = Assets::new(strings);
    let mut world = WorldState::empty(1).unwrap();
    let object = world.spawn(assets.prototype.clone(), 20100, 1).unwrap();
    assert_eq!(object, 0x10000);
    world.player = object;
    if let Some(sheet) = world.get_mut(object).and_then(|o| o.character.as_mut()) {
        sheet.player = true;
    }
    let script = world.add_script(object, 0, 4, 4).unwrap();
    let vm = Vm::new(assets.program.clone());
    let mut host = WorldHost::new(world, assets);
    host.bind_program(&vm, script);
    (host, vm, script)
}
fn run_fragment(host: &mut WorldHost<Assets>, base: &Vm, bytes: &[u8]) -> Result<Vm, VmError> {
    let mut vm = Vm::fragment(base.program.clone(), bytes).unwrap();
    vm.id = base.id;
    for _ in 0..100 {
        match vm.run(host, 1000)? {
            RunOutcome::Budget => {}
            _ => return Ok(vm),
        }
    }
    Err(VmError::InstructionLimit)
}
#[test]
fn spec005_fixture_a_context_setter_pop_order_and_clock() {
    let (mut host, base, _) = host(&[b"beep"]);
    host.world.gvars = Some(vec![0, 0, 0, 0, 7]);
    host.world.clock.set(12340);
    let bytes = fixture_assets::sparse(include_str!("../../../tests/fixtures/host/setters.hex"));
    let vm = run_fragment(&mut host, &base, &bytes).unwrap();
    assert_eq!(host.world.gvars.as_ref().unwrap()[4], 99);
    assert_eq!(
        vm.value_stack.bytes(),
        fixture_assets::sparse(include_str!(
            "../../../tests/fixtures/host/setters-stack.hex"
        ))
    );
    assert_eq!(vm.return_stack.byte_len(), 0);
}
#[test]
fn spec005_fixture_b_sound_handles_and_type_error_consumption() {
    let (mut host, base, _) = host(&[b"beep"]);
    let occupied = SoundState {
        filename: b"occupied".to_vec(),
        mode: 1,
        streamed: false,
        stereo: false,
        playing: true,
        paused: false,
    };
    host.library.sounds[0] = Some(occupied.clone());
    host.library.sounds[1] = Some(occupied);
    let bytes = fixture_assets::sparse(include_str!("../../../tests/fixtures/host/sound.hex"));
    let vm = run_fragment(&mut host, &base, &bytes).unwrap();
    assert_eq!(vm.value_stack.byte_len(), 0);
    assert_eq!(vm.return_stack.byte_len(), 0);
    assert!(host.library.sounds[2].is_none());
    assert_eq!(
        host.library
            .sound_trace
            .iter()
            .map(|(op, _)| *op)
            .collect::<Vec<_>>(),
        [0x8098, 0x8099, 0x809a, 0x809b, 0x809d]
    );
    assert!(
        host.library
            .sound_trace
            .iter()
            .all(|(_, handle)| *handle == 0xa0000002)
    );
    let (mut host, base, _) = self::host(&[b"beep"]);
    let mut bad = bytes;
    bad[6..8].copy_from_slice(&0xa001_u16.to_be_bytes());
    let mut vm = Vm::fragment(base.program, bad.as_slice()).unwrap();
    assert!(matches!(vm.run(&mut host, 100), Err(VmError::Type(0xa001))));
    assert_eq!(vm.value_stack.byte_len(), 0);
    assert_eq!(vm.status.0 & 5, 5);
    assert!(host.library.sound_trace.is_empty());
}
#[test]
fn spec005_fixture_c_registration_dialogue_and_callback_type_mutation() {
    let bytes = fixture_assets::sparse(include_str!("../../../tests/fixtures/host/dialogue.hex"));
    let (mut combat_host, combat_base, _) = self::host(&[b"Hello", b"Continue"]);
    combat_host.world.combat = true;
    let combat_vm = run_fragment(&mut combat_host, &combat_base, &bytes[..0x30]).unwrap();
    assert_eq!(combat_vm.value_stack.byte_len(), 0);
    assert!(combat_host.animations.is_empty() && combat_host.building_animation.is_none());
    assert!(
        !combat_host
            .world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::Animation { .. }))
    );
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    let vm = run_fragment(&mut host, &base, &bytes).unwrap();
    assert_eq!(
        (vm.value_stack.byte_len(), vm.return_stack.byte_len()),
        (0, 0)
    );
    assert_eq!(vm.status.0 & 0x20, 0);
    assert!(!host.world.dialogue.open);
    assert_eq!(host.world.dialogue.target, 0);
    assert_eq!(host.animations.len(), 1);
    assert_eq!(host.animations[0].commands[0], (0x810f, vec![-1, 1, 65536]));
    let calls: Vec<_> = host.calls.iter().map(|call| call.opcode).collect();
    assert_eq!(
        calls,
        [
            0x810e, 0x810f, 0x810e, 0x80de, 0x811c, 0x811e, 0x811f, 0x811d, 0x80df
        ]
    );
    let mut missing_parameter = bytes.clone();
    missing_parameter.drain(0x28..0x2e);
    let (mut end_host, end_base, _) = self::host(&[b"Hello", b"Continue"]);
    assert!(matches!(
        run_fragment(&mut end_host, &end_base, &missing_parameter),
        Err(VmError::Underflow)
    ));
    let mut bad = bytes;
    bad[0x6c..0x6e].copy_from_slice(&0x9001_u16.to_be_bytes());
    let (mut host, base, _) = self::host(&[b"Hello", b"Continue"]);
    assert!(matches!(
        run_fragment(&mut host, &base, &bad),
        Err(VmError::Type(0x9001))
    ));
    assert!(host.world.dialogue.options.is_empty());
}
#[test]
fn every_extension_slot_has_checked_abi_and_registered_dispatch() {
    assert_eq!(extensions::EXTENSIONS.len(), 266);
    assert_eq!(
        extensions::EXTENSIONS
            .iter()
            .filter(|spec| spec.registered)
            .count(),
        264
    );
    for spec in extensions::EXTENSIONS {
        let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
        let mut vm = Vm::fragment(base.program, &spec.word.to_be_bytes()).unwrap();
        let operands = if spec.variable {
            vec![
                Value::integer(3),
                Value::integer(0),
                Value::integer(0),
                Value {
                    tag: 0x9001,
                    payload: 6,
                },
            ]
        } else {
            spec.operands
                .iter()
                .enumerate()
                .map(|(index, rule)| match rule {
                    Rule::String => Value {
                        tag: 0x9001,
                        payload: 6,
                    },
                    Rule::FillColor | Rule::TextColor | Rule::Float => Value::float(0.0),
                    Rule::Unchecked => Value {
                        tag: 0x8001,
                        payload: index as u32,
                    },
                    _ => Value::integer(0),
                })
                .collect::<Vec<_>>()
        };
        for value in operands.iter().rev() {
            vm.value_stack.push(*value).unwrap();
        }
        let result = vm.run(&mut host, 10);
        if matches!(spec.word, 0x807d | 0x807e | 0x8091) {
            assert!(matches!(result, Err(VmError::Opcode(_))));
        } else {
            assert!(
                !matches!(result, Err(VmError::Opcode(_))),
                "unwired opcode {:#x} {}",
                spec.word,
                spec.name
            );
        }
    }
}
#[test]
fn host_abi_color_qualifiers_holes_and_tokenizer_quirk() {
    let (mut host, base, _) = host(&[b"one,two,three", b"one"]);
    let code = [
        fixture::literal(6),
        fixture::literal(22),
        fixture::push(i32::from(b',')),
        fixture::instruction(0x80a0),
    ]
    .concat();
    // First literal is 14 bytes including terminator/padding, so second text starts at 22.
    let vm = run_fragment(&mut host, &base, &code).unwrap();
    assert_eq!(vm.string(vm.value_stack.top().unwrap()).unwrap(), b",tw");
    for (opcode, value, expected) in [
        (0x8077, Value::integer(1), false),
        (0x8077, Value::float(1.0), true),
        (0x8068, Value::integer(1), true),
        (
            0x8077,
            Value {
                tag: 0xa801,
                payload: 1.0_f32.to_bits(),
            },
            true,
        ),
    ] {
        let mut vm = Vm::fragment(base.program.clone(), &[]).unwrap();
        for _ in 0..3 {
            vm.value_stack.push(value).unwrap();
        }
        assert_eq!(extensions::decode(&mut vm, opcode).is_ok(), expected);
    }
    for opcode in [0x807d_u16, 0x807e, 0x8091] {
        let mut vm = Vm::fragment(base.program.clone(), &opcode.to_be_bytes()).unwrap();
        vm.value_stack.push(Value::integer(77)).unwrap();
        assert!(vm.run(&mut host, 100).is_err());
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(77));
    }
}
#[test]
fn world_lifecycle_lighting_spatials_and_queue_order() {
    let (mut host, _, sid) = host(&[b"Hello"]);
    host.world.scripts.get_mut(&sid).unwrap().spatial = Some((2010, 0, 1));
    host.world.scripts.get_mut(&sid).unwrap().id = 0x01000001;
    // The spatial predicate uses the explicit spatial tuple, independent of owner.
    host.world
        .move_object(host.world.player, 2011, 0, true)
        .unwrap();
    assert_eq!(host.world.requests.len(), 1);
    host.world.requests.clear();
    host.world
        .move_object(host.world.player, 2211, 0, true)
        .unwrap();
    assert_eq!(host.world.requests.len(), 1);
    host.world.requests.clear();
    host.world
        .move_object(host.world.player, 2412, 0, true)
        .unwrap();
    assert!(host.world.requests.is_empty());
    host.world.spatial_enabled = false;
    host.world
        .move_object(host.world.player, 2011, 0, true)
        .unwrap();
    assert!(host.world.requests.is_empty());
    for (percent, expected) in [(49, 28426), (50, 40960), (51, 53493)] {
        host.world.ambient_percent(percent);
        assert_eq!(host.world.ambient, expected);
    }
    host.world.object_light(host.world.player, 100, 4).unwrap();
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::ObjectLight {
            requested: 65636,
            ..
        }
    )));
    host.world.set_ambient(16384);
    assert_eq!(host.world.visible_light(host.world.player), 16384);
    host.world.queue = TimedQueue::default();
    host.world.clock.set(2633);
    host.world
        .queue
        .insert(2000, 600, 0, EventKind::MapUpdate)
        .unwrap();
    host.process_queue().unwrap();
    assert_eq!(host.world.queue.events().next().unwrap().due, 3233);
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let mut fresh = fixture::interactive_map();
    fixture::world::word(&mut fresh, 0x24, 1);
    fixture::world::word(&mut fresh, 0x28, 0);
    std::fs::write(dir.path().join("maps/synthetic.map"), fresh).unwrap();
    let assets = VfsWorldAssets::new(
        Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    host.load_campaign().unwrap();
    host.load_map("maps/synthetic.map").unwrap();
    assert_eq!(host.world.mvars, [4, -1]);
    let map_script = host.world.map_script.unwrap();
    let requests: Vec<_> = host
        .world
        .trace
        .iter()
        .filter_map(|event| {
            if let WorldTrace::Script {
                id,
                event,
                fixed,
                player_light,
                ..
            } = event
            {
                Some((*id, *event, *fixed, *player_light))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(requests[0], (map_script, ScriptEvent::MapEnter, 1, 0));
    assert!(
        requests
            .iter()
            .filter(|(_, event, _, _)| *event == ScriptEvent::Start)
            .all(|(_, _, _, light)| *light == 65536)
    );
    assert_eq!(
        requests
            .iter()
            .find(|(_, event, _, _)| *event == ScriptEvent::MapUpdate)
            .unwrap()
            .0,
        map_script
    );
    assert_eq!(
        host.world
            .trace
            .iter()
            .filter_map(|event| if let WorldTrace::LoadStep(step) = event {
                Some(*step)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    assert_eq!(
        host.world
            .get(host.world.player)
            .unwrap()
            .raw
            .light
            .intensity,
        65536
    );
}
#[test]
fn interactive_dialogue_runs_real_callbacks_and_restores_map() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let assets = VfsWorldAssets::new(
        Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    host.interactive_dialogue = true;
    host.load_campaign().unwrap();
    host.load_map("maps/synthetic.map").unwrap();
    let npc = host
        .world
        .iter()
        .find(|(r, o)| *r != host.world.player && o.category() == Category::Critter)
        .unwrap()
        .0;
    host.action(host.world.player, npc, ObjectAction::Talk)
        .unwrap();
    assert!(host.world.dialogue.open && host.world.dialogue.waiting);
    assert_eq!(host.world.dialogue.resolved_reply, b"Hello");
    assert_eq!(
        host.world.dialogue.resolved_options,
        [b"\x95 Continue".to_vec()]
    );
    assert!(host.world.dialogue.head.is_some());
    let time = host.world.clock.ticks;
    host.tick(1000).unwrap();
    assert_eq!(host.world.clock.ticks, time);
    host.choose_dialogue(Some(0)).unwrap();
    assert!(host.world.dialogue.waiting);
    assert_eq!(host.world.dialogue.options[0].procedure, 0);
    host.choose_dialogue(Some(0)).unwrap();
    assert!(!host.world.dialogue.open);
    assert!(host.world.background_enabled);
    let item = host
        .world
        .iter()
        .find(|(_, o)| o.category() == Category::Item && o.raw.hex == Some(20100))
        .unwrap()
        .0;
    host.action(host.world.player, item, ObjectAction::Pickup)
        .unwrap();
    assert_eq!(host.world.get(item).unwrap().owner, Some(host.world.player));
    host.world
        .add_item(host.world.player, item, 2, true)
        .unwrap();
    host.world.remove_item(host.world.player, item, 1).unwrap();
    assert_eq!(host.world.get(item).unwrap().raw.hex, Some(1));
    assert_eq!(host.world.inventory_count(host.world.player, 1), 2);
}
