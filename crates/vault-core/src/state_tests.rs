use crate::fixture_state as fixture;
use crate::fixture_vm as vm_fixture;
use crate::{
    assets::{VfsWorldAssets, WorldAssets},
    dialogue::*,
    library::SoundState,
    queue::{EventKind, TimedQueue},
    rng::{skill_check, stat_check},
    world::WorldTrace,
    *,
};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};
use vault_formats::{
    Fid, FrmArchive, GcdFile, MapFile, Prototype,
    pro::{BasicFlags, Category},
};
use vault_vm::{
    IntProgram, RunOutcome, Value, Vm, VmError,
    extensions::{self, Rule},
};

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
    // Reversed bounds, and (safe rule) a width beyond 2147483647, consume
    // exactly one draw and return the first bound.
    for (first, last) in [(5, 2), (i32::MIN, i32::MAX), (-1, i32::MAX)] {
        let mut ranged = ParkMiller::from_state(before.clone()).unwrap();
        let mut drawn = ParkMiller::from_state(before.clone()).unwrap();
        assert_eq!(ranged.range(first, last).unwrap(), first);
        drawn.draw();
        assert_eq!(ranged.state(), drawn.state());
    }
    let mut widest = ParkMiller::from_state(before.clone()).unwrap();
    assert!((0..=i32::MAX - 1).contains(&widest.range(0, i32::MAX - 1).unwrap()));
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
    assert!(
        sheet.reconcile_levels(context),
        "level 4 grants the Skilled pick"
    );
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
    let mut animation = TalkingHead::new(
        head.clone(),
        Mood::Neutral,
        HeadIdle::default(),
        &mut Dice::new(&[77]),
    )
    .unwrap();
    // A speech frame is drawn from the mood's speech art; the fidget state
    // underneath is kept.
    animation.phoneme(41).unwrap();
    assert_eq!(
        animation.shown,
        Some(HeadFrame {
            animation: 10,
            variant: 0,
            frame: 8
        })
    );
    assert_eq!(animation.art_id().unwrap(), 0x08043007);
    // A reaction first finishes the current (two-frame) fidget.
    animation.react(49);
    for now in [100, 200, 300] {
        assert!(animation.tick(now, 2, 10, &mut Dice::default()).unwrap());
        assert_eq!(animation.art_id().unwrap(), 0x08043007);
    }
    assert_eq!(animation.phase, HeadPhase::Waiting);
    animation.tick(400, 2, 10, &mut Dice::default()).unwrap();
    assert_eq!(animation.art_id().unwrap(), 0x08030007);
    // After all transition frames play, the head becomes good and then
    // chooses a good fidget (selector 1).
    animation.tick(500, 2, 10, &mut Dice::default()).unwrap();
    animation.tick(600, 2, 10, &mut Dice::new(&[10])).unwrap();
    assert_eq!(
        (animation.mood, animation.phase, animation.animation),
        (Mood::Good, HeadPhase::Fidget, 1)
    );
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
            Some(expected)
        );
    }
    // Safe rule: a count of zero or less still takes its draw but selects
    // no fidget; the head then waits on its speech frame and is diagnosed.
    for count in [0, -2] {
        let mut dice = Dice::new(&[40]);
        assert_eq!(choose_fidget(count, &mut 0, &mut dice).unwrap(), None);
        assert_eq!(dice.draws, 1);
    }
    let mut empty = head.clone();
    empty.counts = [0; 3];
    let mut waiting =
        TalkingHead::new(empty, Mood::Bad, HeadIdle::default(), &mut Dice::new(&[40])).unwrap();
    assert_eq!(
        (waiting.phase, waiting.animation, waiting.frame),
        (HeadPhase::Waiting, 11, 0)
    );
    assert!(waiting.diagnostic.take().is_some());
    waiting.frame = 1;
    // The idle wait elapses: threshold draw first, then the fidget draw.
    assert!(
        !waiting
            .tick(20000, 3, 10, &mut Dice::new(&[1, 40]))
            .unwrap()
    );
    assert_eq!((waiting.phase, waiting.frame), (HeadPhase::Waiting, 1));
    assert_eq!((waiting.idle_seconds, waiting.wait_ms), (10, 5000));
    assert!(waiting.diagnostic.is_some());
    // SPEC-006 §3.11 session start: the fidget is drawn at once, and with the
    // retained timestamp at zero the first tick shows its frame 0.
    let mut opening = TalkingHead::new(
        head.clone(),
        Mood::Neutral,
        HeadIdle {
            idle_seconds: 50,
            wait_ms: 6000,
        },
        &mut Dice::new(&[30]),
    )
    .unwrap();
    // 30 + 50 / 2 = 55 selects variant 2, and a three-variant draw resets
    // the carried idle counter.
    assert_eq!(
        (opening.phase, opening.animation, opening.variant),
        (HeadPhase::Fidget, 4, 2)
    );
    assert_eq!(opening.idle_seconds, 0);
    assert!(opening.tick(5000, 2, 10, &mut Dice::default()).unwrap());
    assert_eq!((opening.frame, opening.last_fidget_ms), (0, 5000));
    assert!(!opening.tick(5050, 2, 10, &mut Dice::default()).unwrap());
    assert!(opening.tick(5100, 2, 10, &mut Dice::default()).unwrap());
    assert_eq!(opening.frame, 1);
    // The fidget finishes on frame 0 without moving the retained timestamp,
    // so the carried 6-second threshold runs from its last shown frame.
    assert!(opening.tick(5200, 2, 10, &mut Dice::default()).unwrap());
    assert_eq!(
        (opening.phase, opening.frame, opening.last_fidget_ms),
        (HeadPhase::Waiting, 0, 5100)
    );
    assert!(!opening.tick(11099, 2, 10, &mut Dice::default()).unwrap());
    let mut dice = Dice::new(&[3, 90]);
    assert!(opening.tick(11100, 2, 10, &mut dice).unwrap());
    assert_eq!(dice.draws, 2);
    assert_eq!(
        (opening.idle_seconds, opening.wait_ms, opening.variant),
        (0, 7000, 3)
    );
    assert_eq!(
        opening.idle(),
        HeadIdle {
            idle_seconds: 0,
            wait_ms: 7000
        }
    );
    // The idle counter changes only by the threshold's whole seconds, speech
    // completion (3) and user choice (0); waiting time alone adds nothing.
    let mut speaking = TalkingHead::new(
        HeadDefinition {
            counts: [2; 3],
            ..head.clone()
        },
        Mood::Neutral,
        HeadIdle::default(),
        &mut Dice::new(&[77]),
    )
    .unwrap();
    speaking.phoneme(3).unwrap();
    speaking.speech_end();
    assert_eq!(speaking.idle_seconds, 3);
    speaking.tick(8000, 2, 10, &mut Dice::default()).unwrap();
    assert_eq!(
        (speaking.idle_seconds, speaking.phase),
        (3, HeadPhase::Waiting)
    );
    // No fidget frame was shown yet (timestamp zero): the first wait is
    // measured from zero and adds the 10-second threshold.
    speaking
        .tick(10000, 2, 10, &mut Dice::new(&[0, 50]))
        .unwrap();
    assert_eq!(
        (speaking.idle_seconds, speaking.wait_ms, speaking.phase),
        (13, 4000, HeadPhase::Fidget)
    );
    speaking.react(0);
    assert_eq!(speaking.idle_seconds, 0);
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
    fn program_by_name(&self, name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        assert_eq!(name, b"Hello");
        Ok((
            "scripts/child.int".into(),
            IntProgram::parse(&vm_fixture::program()).unwrap(),
        ))
    }
    fn object_art(&self, _fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        Ok(Arc::new(
            FrmArchive::parse(&fixture::animated_frame(12, 20, &[1, 2])).unwrap(),
        ))
    }
    fn palette(&self, name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        if name == b"Hello" {
            Ok(vault_formats::Palette::parse(&crate::fixture_assets::palette()).unwrap())
        } else {
            Err(WorldError::Invalid("missing palette".into()))
        }
    }
    fn font_number(&self, number: i32) -> Option<Arc<vault_formats::Font>> {
        (number == 101).then(|| {
            Arc::new(vault_formats::Font::parse_aaf(&crate::fixture_assets::aaf()).unwrap())
        })
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
    assert!(combat_host.motions.is_empty() && combat_host.building_animation.is_none());
    assert!(
        !combat_host
            .world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::Animation { .. }))
    );
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    host.playback = crate::host_choreography::Playback::RealTime;
    let vm = run_fragment(&mut host, &base, &bytes).unwrap();
    assert_eq!(
        (vm.value_stack.byte_len(), vm.return_stack.byte_len()),
        (0, 0)
    );
    assert_eq!(vm.status.0 & 0x20, 0);
    assert!(!host.world.dialogue.open);
    assert_eq!(host.world.dialogue.target, 0);
    // The registered forward animation 1 of 0x10000 plays as one sequence.
    assert_eq!(host.motions.len(), 1);
    let entry = &host.motions[0].schedule.entries()[0];
    assert_eq!(entry.owner, 0x10000);
    assert!(matches!(
        entry.action,
        crate::motion::Motion::Play {
            animation: 1,
            play: crate::motion::Play::Forward,
            ..
        }
    ));
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
fn spec005_safe_rules_for_library_and_game_opcodes() {
    use fixture::{instruction, literal, push};
    let (mut host, base, _) = host(&[b"one,two,three", b"xyz"]);
    let top = |host: &mut WorldHost<Assets>, code: &[u8]| {
        let vm = run_fragment(host, &base, code).unwrap();
        let value = vm.value_stack.top().unwrap();
        (value, vm.string(value).ok().map(<[u8]>::to_vec))
    };
    // tokenize: an absent previous match returns I zero; delimiter 0 never
    // scans past the source terminator.
    let absent = [
        literal(6),
        literal(22),
        push(i32::from(b',')),
        instruction(0x80a0),
    ]
    .concat();
    assert_eq!(top(&mut host, &absent).0, Value::integer(0));
    let whole = [literal(6), push(0), push(0), instruction(0x80a0)].concat();
    assert_eq!(
        top(&mut host, &whole).1.as_deref(),
        Some(&b"one,two,three"[..])
    );
    // obj_name of null is always `<Critter>`; game_ticks wraps.
    let name = [push(0), instruction(0x80a4)].concat();
    assert_eq!(top(&mut host, &name).1.as_deref(), Some(&b"<Critter>"[..]));
    let ticks = [push(i32::MAX), instruction(0x80f2)].concat();
    assert_eq!(
        top(&mut host, &ticks).0,
        Value::integer(i32::MAX.wrapping_mul(10))
    );
    // turn_off/turn_on_objs_in_area: reversed hex bounds are swapped and the
    // elevation loop terminates (operands pushed first elevation first).
    let player = host.world.player;
    let area = |opcode| {
        [
            push(2),
            push(0),
            push(20110),
            push(20090),
            instruction(opcode),
        ]
        .concat()
    };
    run_fragment(&mut host, &base, &area(0x80e1)).unwrap();
    assert!(
        host.world
            .get(player)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::HIDDEN)
    );
    run_fragment(&mut host, &base, &area(0x80e2)).unwrap();
    assert!(
        !host
            .world
            .get(player)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::HIDDEN)
    );
    // A sound handle whose stripped slot is outside 0-31, or empty, is a
    // no-op rather than a VM error.
    for handle in [5, 0x40, 0xa000_0040_u32 as i32] {
        run_fragment(
            &mut host,
            &base,
            &[push(handle), instruction(0x8099)].concat(),
        )
        .unwrap();
    }
    // saystart at the initial depth -1 indexes the slot after incrementing;
    // sayreplyflags stores reply flags, not option flags.
    run_fragment(&mut host, &base, &instruction(0x804e)).unwrap();
    assert_eq!(host.library.dialogue_depth, 0);
    run_fragment(&mut host, &base, &[push(7), instruction(0x805f)].concat()).unwrap();
    assert_eq!(
        (host.library.reply_flags, host.library.option_flags),
        (7, 0)
    );
    // The any-key binding is cleared with its owning program.
    host.library.any_key = Some((base.id, 1));
    host.library.cleanup(base.id);
    assert_eq!(host.library.any_key, None);
    // A nonfinite color channel reads as 0 instead of failing.
    let mut vm = Vm::fragment(base.program.clone(), &[]).unwrap();
    for _ in 0..3 {
        vm.value_stack.push(Value::float(f32::NAN)).unwrap();
    }
    let call = extensions::decode(&mut vm, 0x8077).unwrap();
    assert!(call.args.iter().all(|arg| arg.channel == Some(0.0)));
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
    // SPEC-011 §3.13: re-adding the representative is a successful no-op.
    host.world
        .add_item(host.world.player, item, 2, true)
        .unwrap();
    assert_eq!(host.world.inventory_count(host.world.player, 1), 1);
    host.world.remove_item(host.world.player, item, 1).unwrap();
    assert_eq!(host.world.get(item).unwrap().raw.hex, Some(1));
    assert_eq!(host.world.inventory_count(host.world.player, 1), 0);
}

#[test]
fn spec002_setfont_selects_only_loaded_font_numbers() {
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    let set = |host: &mut WorldHost<Assets>, number: i32| {
        let mut vm = Vm::fragment(base.program.clone(), &0x8075_u16.to_be_bytes()).unwrap();
        vm.value_stack.push(Value::integer(number)).unwrap();
        vm.run(host, 10).unwrap();
    };
    assert_eq!(host.library.font, 0);
    set(&mut host, 101);
    assert_eq!(host.library.font, 101);
    // A missing slot keeps the previous selection and raises no VM error.
    set(&mut host, 104);
    assert_eq!(host.library.font, 101);
    assert!(host.world.effects.iter().any(
        |effect| matches!(effect, WorldEffect::Diagnostic(text) if text.contains("font 104"))
    ));
}

#[test]
fn spec003_saved_maps_retain_embedded_mvars_and_report_out_of_range_access() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let load = |flags: i32| {
        let mut map = fixture::interactive_map();
        fixture::world::word(&mut map, 0x24, 1);
        fixture::world::word(&mut map, 0x28, flags);
        std::fs::write(dir.path().join("maps/synthetic.map"), map).unwrap();
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
        host
    };
    // Unsaved: the map-specific .GAM replaces the binary MVAR array.
    assert_eq!(load(0).world.mvars, [4, -1]);
    // Saved-state bit 0: the embedded values are retained, even for a .MAP name.
    let mut host = load(1);
    assert_eq!(host.world.mvars, [7, -2]);
    // Out-of-range MVAR reads give zero with a diagnostic (SPEC-003/SPEC-005).
    host.world.effects.clear();
    let base = Vm::new(host.programs.values().next().unwrap().program.clone());
    let mut vm = Vm::fragment(base.program.clone(), &0x80c3_u16.to_be_bytes()).unwrap();
    vm.value_stack.push(Value::integer(5)).unwrap();
    vm.run(&mut host, 10).unwrap();
    assert_eq!(vm.value_stack.pop().unwrap(), Value::integer(0));
    assert!(host.world.effects.iter().any(
        |effect| matches!(effect, WorldEffect::Diagnostic(text) if text.contains("MVAR read"))
    ));
}

/// A program image whose procedure 1 record carries scheduling metadata.
fn scheduled_image(flags: u32, deadline: u32) -> (IntProgram, u32) {
    let (mut bytes, condition) = vm_fixture::scheduled_program(1);
    let record = 46 + 24;
    for (offset, value) in [(4, flags), (8, deadline), (12, condition)] {
        bytes[record + offset..record + offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    (IntProgram::parse(&bytes).unwrap(), condition)
}

#[test]
fn spec004_host_tick_fires_scheduled_procedures_and_launches_programs() {
    use vault_vm::{VmHost, VmStatus, procedure_flags};
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    // The player's critter script is bound to the fixture's own VM, which
    // the host does not hold; with map background processing off the
    // critter-script cycle step (SPEC-006 §3.7) leaves it alone.
    host.world.background_enabled = false;
    let install = |host: &mut WorldHost<Assets>, flags: u32, deadline: u32| {
        let mut vm = Vm::new(scheduled_image(flags, deadline).0);
        vm.id = 77;
        // A loaded program whose own flow has already ended.
        vm.status.0 |= VmStatus::ENDED;
        host.programs.insert(77, vm);
    };
    // Timed: the deadline comparison is strict, and firing clears the whole
    // flag word; the deferred trampoline then restores the saved context.
    install(
        &mut host,
        procedure_flags::TIMED | procedure_flags::EXPORT,
        2100,
    );
    host.tick(2100).unwrap();
    assert_ne!(host.programs[&77].program.procedures()[1].flags, 0);
    host.tick(2101).unwrap();
    let vm = &host.programs[&77];
    assert_eq!(vm.program.procedures()[1].flags, 0);
    assert!(vm.status.contains(VmStatus::ENDED));
    assert_eq!(
        (vm.return_stack.byte_len(), vm.value_stack.byte_len()),
        (0, 0)
    );
    // Conditional: the stream runs against the real host (it exports a
    // variable) and its nonzero predicate fires the procedure.
    assert!(host.registry.fetch_variable(b"main").is_err());
    install(&mut host, procedure_flags::CONDITIONAL, 0);
    host.tick(2102).unwrap();
    assert_eq!(host.programs[&77].program.procedures()[1].flags, 0);
    assert!(host.registry.fetch_variable(b"main").is_ok());
    // A program blocked on a child neither polls nor runs.
    install(&mut host, procedure_flags::TIMED, 0);
    host.programs.get_mut(&77).unwrap().status.0 |= VmStatus::ATTACHED;
    host.tick(5000).unwrap();
    assert_eq!(
        host.programs[&77].program.procedures()[1].flags,
        procedure_flags::TIMED
    );
    host.programs.remove(&77);

    let launch = |host: &mut WorldHost<Assets>, opcode: u16, parent: Option<u32>| {
        let mut vm = Vm::fragment(
            base.program.clone(),
            &[fixture::literal(6), fixture::instruction(opcode)].concat(),
        )
        .unwrap();
        vm.id = 900;
        vm.parent = parent;
        let before: BTreeSet<_> = host.programs.keys().copied().collect();
        let outcome = vm.run(host, 100).unwrap();
        let created: Vec<_> = host
            .programs
            .keys()
            .copied()
            .filter(|id| !before.contains(id))
            .collect();
        assert_eq!(created.len(), 1);
        (vm, outcome, created[0])
    };
    // 0x800B starts a fresh independent program; the interpreter pass runs its
    // bootstrap one ordinary ten-instruction budget at a time until it ends.
    let (_, outcome, child) = launch(&mut host, 0x800b, None);
    assert_eq!(outcome, RunOutcome::FragmentEnd);
    assert_eq!(host.programs[&child].parent, None);
    let mut ticks = 0;
    while !host.programs[&child].status.contains(VmStatus::ENDED) {
        host.tick(6000 + ticks).unwrap();
        ticks += 1;
        assert!(ticks < 1000, "launched bootstrap must finish");
    }
    assert!(ticks > 1);
    // 0x8008 blocks the parent while its fresh child is attached.
    let (parent, outcome, child) = launch(&mut host, 0x8008, None);
    assert_eq!(outcome, RunOutcome::Blocked);
    assert_eq!(
        (parent.child, host.programs[&child].parent),
        (Some(child), Some(900))
    );
    // 0x8009 ends the old program and transfers its parent association,
    // including the grandparent's child link.
    let mut grandparent = Vm::new(base.program.clone());
    grandparent.id = 500;
    grandparent.child = Some(900);
    host.programs.insert(500, grandparent);
    let (old, outcome, replacement) = launch(&mut host, 0x8009, Some(500));
    assert_eq!(outcome, RunOutcome::Ended);
    assert!(old.status.contains(VmStatus::ENDED));
    assert_eq!(host.programs[&replacement].parent, Some(500));
    assert_eq!(host.programs[&500].child, Some(replacement));
}

#[test]
fn spec005_library_resources_named_events_and_request_corrections() {
    use fixture::{instruction, literal, push};
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    // 0x8089: one valid image operand admits all three; all are resolved and
    // retained (pressed, normal, hover), with canonical I zero as a null image.
    let code = [
        literal(6),
        push(0),
        push(0),
        push(100),
        push(50),
        instruction(0x8062),
        literal(6),
        instruction(0x8064),
        literal(6),
        push(1),
        push(2),
        push(30),
        push(10),
        instruction(0x8086),
        literal(6),
        literal(14),
        push(0),
        literal(14),
        instruction(0x8089),
    ]
    .concat();
    run_fragment(&mut host, &base, &code).unwrap();
    let button = host
        .library
        .windows
        .values()
        .next()
        .unwrap()
        .buttons
        .values()
        .next()
        .unwrap();
    assert_eq!(
        button.images,
        [Some(b"Continue".to_vec()), None, Some(b"Continue".to_vec())]
    );
    // 0x806E loads and installs the palette tables; a failed load is a VM error.
    run_fragment(
        &mut host,
        &base,
        &[literal(6), instruction(0x806e)].concat(),
    )
    .unwrap();
    assert!(host.library.palette.is_some());
    assert_eq!(host.library.palette_generation, 1);
    assert!(
        run_fragment(
            &mut host,
            &base,
            &[literal(14), instruction(0x806e)].concat()
        )
        .is_err()
    );
    assert_eq!(host.library.palette_generation, 1);

    // 0x810C selector 1000 sets orientation only for control below 6.
    let me = host.world.player as i32;
    for (control, expected) in [(3, 3), (6, 3), (9, 3), (0, 0)] {
        let code = [push(me), push(1000), push(control), instruction(0x810c)].concat();
        run_fragment(&mut host, &base, &code).unwrap();
        assert_eq!(
            host.world.get(host.world.player).unwrap().raw.orientation,
            expected
        );
    }

    // 0x80D0 requires a nonfleeing target; attack_setup has no such test.
    let defender = host
        .world
        .spawn(host.assets.prototype.clone(), 20102, 1)
        .unwrap();
    let set_fleeing = |host: &mut WorldHost<Assets>, fleeing: bool| {
        if let Some(vault_formats::map::ObjectState::Critter(state)) = host
            .world
            .get_mut(defender)
            .map(|object| &mut object.raw.state)
        {
            state.maneuvers = if fleeing { 4 } else { 0 };
        }
    };
    let attack = [
        push(defender as i32),
        push(0),
        push(0),
        push(50),
        push(1),
        push(10),
        push(0),
        push(1),
        instruction(0x80d0),
    ]
    .concat();
    let requests = |host: &WorldHost<Assets>| {
        host.world
            .effects
            .iter()
            .filter(|effect| matches!(effect, WorldEffect::Combat { .. }))
            .count()
    };
    set_fleeing(&mut host, true);
    run_fragment(&mut host, &base, &attack).unwrap();
    assert_eq!(requests(&host), 0);
    let setup = [push(me), push(defender as i32), instruction(0x8143)].concat();
    run_fragment(&mut host, &base, &setup).unwrap();
    assert_eq!(requests(&host), 1);
    set_fleeing(&mut host, false);
    run_fragment(&mut host, &base, &attack).unwrap();
    assert_eq!(requests(&host), 2);
}

#[test]
fn spec005_named_handlers_are_busy_until_their_hit_runs() {
    let mut library = crate::library::LibraryState::default();
    library.named[0] = Some(crate::library::NamedBinding {
        name: b"tick".to_vec(),
        program: 7,
        procedure: 2,
        persistent: true,
        hits: 2,
        busy: false,
    });
    // At most one hit per binding per pass; the handler is then busy.
    assert_eq!(library.callbacks(), [(7, 2)]);
    assert!(library.named[0].as_ref().unwrap().busy);
    assert!(library.callbacks().is_empty());
    library.release_named(7, 2);
    assert_eq!(library.callbacks(), [(7, 2)]);
    // One-shot events are freed after dispatch.
    library.named[1] = Some(crate::library::NamedBinding {
        name: b"once".to_vec(),
        program: 7,
        procedure: 3,
        persistent: false,
        hits: 1,
        busy: false,
    });
    library.release_named(7, 2);
    assert_eq!(library.callbacks(), [(7, 3)]);
    assert!(library.named[1].is_none());
}

#[test]
fn spec006_live_queue_stop_elevation_updates_speech_reputation_and_campaign_failure() {
    use fixture::{instruction, push, returning};
    let (mut host, _, _) = host(&[b"Hello", b"Continue"]);
    // A timed procedure that requests a map transition.
    host.assets.program = IntProgram::parse(&fixture::program(
        &[b"Hello"],
        &[
            (b"start", returning(Vec::new())),
            (
                b"timed_event_p_proc",
                returning([push(0), push(1), instruction(0x80e4)].concat()),
            ),
        ],
    ))
    .unwrap();
    let owner = host.world.player;
    let sid = host.world.add_script(owner, 0, 4, 4).unwrap();
    host.world.queue = TimedQueue::default();
    host.world.clock.set(1000);
    for _ in 0..3 {
        host.world
            .queue
            .insert(
                1000,
                10,
                owner,
                EventKind::Script {
                    script: sid,
                    parameter: 0,
                },
            )
            .unwrap();
    }
    host.world.clock.set(1009);
    assert_eq!(host.process_queue().unwrap(), 0);
    assert_eq!(host.world.queue.events().count(), 3);
    // At 1010: A's first request only runs the bootstrap (result 0); B's
    // timed procedure requests a transition (nonzero), so C stays pending.
    host.world.clock.set(1010);
    assert_eq!(host.process_queue().unwrap(), 1);
    assert_eq!(host.world.queue.events().count(), 1);
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::MapTransition {
            index: 0,
            parameter: 1
        }
    )));
    // SPEC-005: a transition to the map's default entrance.
    assert_eq!(
        host.world.pending_transition,
        Some(crate::Transition {
            map: 0,
            elevation: -1,
            hex: -1,
            facing: -1
        })
    );
    host.world.pending_transition = None;

    // Elevation changes request a map update only with a current map script.
    host.world.requests.clear();
    host.world.map_script = None;
    host.world.move_object(owner, 20100, 2, false).unwrap();
    assert!(host.world.requests.is_empty());
    host.world.map_script = Some(sid);
    host.world.move_object(owner, 20100, 0, false).unwrap();
    assert!(
        host.world
            .requests
            .iter()
            .any(|request| request.script == sid && request.event == ScriptEvent::MapUpdate)
    );
    let requested = host.world.requests.len();
    host.world.move_object(owner, 20101, 0, false).unwrap();
    assert_eq!(
        host.world.requests.len(),
        requested,
        "same elevation: no update"
    );
    host.world.requests.clear();

    // Speech completion draws speech frame zero and sets the idle counter to 3.
    let definition = HeadDefinition {
        index: 7,
        stem: "synth".into(),
        counts: [3; 3],
    };
    let mut head = TalkingHead::new(
        definition,
        Mood::Neutral,
        HeadIdle::default(),
        &mut Dice::new(&[77]),
    )
    .unwrap();
    head.phoneme(9).unwrap();
    host.world.dialogue.head = Some(head);
    host.world.dialogue.speech = Some(b"voice".to_vec());
    host.speech_finished();
    let head = host.world.dialogue.head.as_ref().unwrap();
    assert_eq!(
        (head.frame, head.idle_seconds, head.phase),
        (0, 3, HeadPhase::Waiting)
    );
    assert!(host.world.dialogue.speech.is_none());

    // Reaction inputs: the player's sheet reputation and configured globals.
    host.world.player_sheet_mut().unwrap().reputation = 7;
    host.world.gvars = Some((0..10).collect());
    host.world.rules.reputation_globals = Some(crate::world::ReputationGlobals {
        child_killer: 3,
        bad_monsters: 4,
        good_monsters: 5,
        champion: 6,
        berserker: 7,
    });
    let inputs = host.reputation_inputs();
    assert_eq!(
        (
            inputs.value,
            inputs.child_killer,
            inputs.bad_monsters,
            inputs.good_monsters,
            inputs.champion,
            inputs.berserker
        ),
        (7, 3, 4, 5, 6, 7)
    );

    // A campaign initializer that fails to open reports failure and keeps
    // the prior array.
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    std::fs::remove_file(dir.path().join("data/vault13.gam")).unwrap();
    let assets = VfsWorldAssets::new(
        Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    let mut campaign = WorldHost::new(WorldState::empty(1).unwrap(), assets);
    campaign.world.gvars = Some(vec![5, 6]);
    assert!(campaign.load_campaign().is_err());
    assert_eq!(campaign.world.gvars, Some(vec![5, 6]));
}

#[test]
fn spec006_spatials_are_elevation_specific() {
    let (mut host, _, _) = host(&[b"Hello", b"Continue"]);
    let owner = host.world.player;
    let sid = host.world.add_script(owner, 0, 1, 0).unwrap();
    host.world.scripts.get_mut(&sid).unwrap().spatial = Some((2010, 0, 1));
    host.world.requests.clear();
    // The same tile numbers at another elevation do not match.
    host.world.move_object(owner, 2009, 1, true).unwrap();
    host.world.move_object(owner, 2011, 1, true).unwrap();
    assert!(
        host.world
            .requests
            .iter()
            .all(|r| r.event != ScriptEvent::Spatial)
    );
    // Tile 2412 (distance 3) does not match; 2011 on elevation 0 does. (A move
    // that changes only elevation keeps the tile, so it performs no check.)
    host.world.move_object(owner, 2412, 0, true).unwrap();
    assert!(
        host.world
            .requests
            .iter()
            .all(|r| r.event != ScriptEvent::Spatial)
    );
    host.world.move_object(owner, 2011, 0, true).unwrap();
    assert!(
        host.world
            .requests
            .iter()
            .any(|r| r.script == sid && r.event == ScriptEvent::Spatial && r.source == owner)
    );
}

#[test]
fn spec005_critter_damage_uses_armor_unless_bypassed() {
    use fixture::{instruction, push};
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    let me = host.world.player;
    let sheet = host.world.get_mut(me).unwrap().character.as_mut().unwrap();
    sheet.base[17] = 4;
    sheet.base[24] = 25;
    let hp = |host: &WorldHost<Assets>| match &host.world.get(me).unwrap().raw.state {
        vault_formats::map::ObjectState::Critter(state) => state.hp,
        _ => unreachable!(),
    };
    let set_hp = |host: &mut WorldHost<Assets>, value: i32| {
        if let vault_formats::map::ObjectState::Critter(state) =
            &mut host.world.get_mut(me).unwrap().raw.state
        {
            state.hp = value;
        }
    };
    let damage = |host: &mut WorldHost<Assets>, amount: i32, flags: i32| {
        host.world.effects.clear();
        let code = [
            push(me as i32),
            push(amount),
            push(flags),
            instruction(0x80ef),
        ]
        .concat();
        run_fragment(host, &base, &code).unwrap();
    };
    set_hp(&mut host, 100);
    // Normal type: 20 - DT 4 = 16, then 16 - trunc(16 * 25 / 100) = 12.
    damage(&mut host, 20, 0);
    assert_eq!(hp(&host), 88);
    assert!(
        host.world
            .effects
            .iter()
            .any(|e| matches!(e, WorldEffect::Animation { .. }))
    );
    // 0x100 bypasses armor; 0x200 suppresses the animation request.
    damage(&mut host, 20, 0x300);
    assert_eq!(hp(&host), 68);
    assert!(
        !host
            .world
            .effects
            .iter()
            .any(|e| matches!(e, WorldEffect::Animation { .. }))
    );
    // Armor cannot turn small damage into healing.
    damage(&mut host, 3, 0);
    assert_eq!(hp(&host), 68);
}

#[test]
fn spec005_registered_sequences_execute_retire_and_clear_busy() {
    use fixture::{instruction, literal, push};
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    // Registered sequences play over time under the SPEC-007 §3.10.1 start
    // rule; each registers at the time of the next tick.
    host.playback = crate::host_choreography::Playback::RealTime;
    let me = host.world.player as i32;
    let player = host.world.player;
    let busy = |host: &mut WorldHost<Assets>| {
        let code = [push(me), instruction(0x80e7)].concat();
        let mut vm = run_fragment(host, &base, &code).unwrap();
        vm.value_stack.pop().unwrap()
    };
    let begin = |flags: i32| [push(1), push(flags), instruction(0x810e)].concat();
    let end = [push(3), push(0), instruction(0x810e)].concat();
    let mut now = 1000;
    let register = |host: &mut WorldHost<Assets>, now: u64, code: &[u8]| {
        host.world.elapsed_ms = now;
        run_fragment(host, &base, code).unwrap();
    };
    let sounds = |host: &WorldHost<Assets>| -> Vec<Vec<u8>> {
        host.world
            .effects
            .iter()
            .filter_map(|e| match e {
                WorldEffect::SoundEffect { name, owner } if *owner == Some(player) => {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect()
    };
    // A registered walk to hex 20103 on the object's elevation.
    let code = [
        begin(1),
        push(me),
        push(20103),
        push(-1),
        instruction(0x8113),
        end.clone(),
    ]
    .concat();
    register(&mut host, now, &code);
    assert_eq!(busy(&mut host), Value::integer(-1));
    let mut visited = Vec::new();
    while host.moving(player) {
        host.tick(now).unwrap();
        visited.push(host.world.get(player).unwrap().raw.hex.unwrap());
        now += 100;
        assert!(now < 10_000, "sequence must retire");
    }
    assert_eq!(host.world.get(player).unwrap().raw.hex, Some(20103));
    // One adjacent hex per frame step along the planned route.
    let mut path = vec![20100];
    path.extend(visited.iter().copied());
    path.dedup();
    assert!(path.len() >= 4);
    for pair in path.windows(2) {
        assert_eq!(
            vault_formats::HexGrid::lattice_distance(pair[0], pair[1]).unwrap(),
            1
        );
    }
    // Retired: no longer busy, and the critter returned to standing.
    assert_eq!(busy(&mut host), Value::integer(0));
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        0
    );

    // Forward animation 20 with a two-frame sprite ends on static art 48;
    // flag 4 keeps that art. The first entry's delay (2) is forced to 0, so
    // it takes three frame steps: first frame, second frame, end.
    let code = [
        begin(1 | 4),
        push(me),
        push(20),
        push(2),
        instruction(0x810f),
        end.clone(),
    ]
    .concat();
    register(&mut host, now, &code);
    let mut steps = 0;
    while host.moving(player) {
        host.tick(now).unwrap();
        now += 100;
        steps += 1;
        assert!(steps < 20);
    }
    assert_eq!(steps, 3, "first frame, second frame, end");
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        48
    );

    // Reverse playback shows the frames in descending order.
    let code = [
        begin(1 | 4),
        push(me),
        push(1),
        push(0),
        instruction(0x8110),
        end.clone(),
    ]
    .concat();
    register(&mut host, now, &code);
    host.tick(now).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.frame, 1);
    host.tick(now + 100).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.frame, 0);
    host.tick(now + 200).unwrap();
    now += 300;
    assert!(!host.moving(player));

    // The start rule: a delay-0 entry starts with the entry before it, and
    // a -1 entry once every earlier entry has finished.
    let code = [
        begin(1),
        push(me),
        push(1),
        push(0),
        instruction(0x810f),
        push(me),
        literal(6),
        push(0),
        instruction(0x813b),
        push(me),
        literal(14),
        push(-1),
        instruction(0x813b),
        end.clone(),
    ]
    .concat();
    host.world.effects.clear();
    register(&mut host, now, &code);
    assert_eq!(sounds(&host), [b"Hello".to_vec()], "with the animation");
    host.tick(now).unwrap();
    host.tick(now + 100).unwrap();
    assert_eq!(sounds(&host).len(), 1, "the animation still runs");
    host.tick(now + 200).unwrap();
    assert_eq!(
        sounds(&host),
        [b"Hello".to_vec(), b"Continue".to_vec()],
        "once the animation has finished"
    );
    now += 300;

    // `anim` registers its animation: forward 20 plays both frames, then
    // ends on static art 48.
    let code = [push(me), push(20), push(0), instruction(0x810c)].concat();
    register(&mut host, now, &code);
    assert_eq!(busy(&mut host), Value::integer(-1));
    host.tick(now).unwrap();
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        20
    );
    host.tick(now + 100).unwrap();
    host.tick(now + 200).unwrap();
    now += 300;
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        48
    );
    host.world.get_mut(player).unwrap().raw.art_id &= !0x00ff_0000;

    // Sound cues emit their name and never count as busy; neither does a
    // lone standing action.
    let code = [
        begin(1),
        push(me),
        literal(6),
        push(0),
        instruction(0x813b),
        end.clone(),
    ]
    .concat();
    host.world.effects.clear();
    register(&mut host, now, &code);
    assert_eq!(busy(&mut host), Value::integer(0));
    assert_eq!(sounds(&host), [b"Hello".to_vec()]);
    let code = [push(me), instruction(0x80cc)].concat();
    register(&mut host, now, &code);
    assert_eq!(busy(&mut host), Value::integer(0));
    assert_eq!(
        Fid(host.world.get(player).unwrap().raw.art_id).animation(),
        0,
        "standing at once"
    );
}

#[test]
fn spec007_static_death_correction_corpses_and_inventory_drop() {
    use fixture::{instruction, push};
    let (mut host, base, _) = host(&[b"Hello", b"Continue"]);
    let kill = |host: &mut WorldHost<Assets>, victim: u32, animation: i32| {
        let code = [push(victim as i32), push(animation), instruction(0x80ed)].concat();
        run_fragment(host, &base, &code).unwrap();
        host.world.get(victim).unwrap().clone()
    };
    let spawn = |host: &mut WorldHost<Assets>| {
        let prototype = host.assets.prototype.clone();
        host.world.spawn(prototype, 20120, 1).unwrap()
    };
    // Static code 61 without maximum violence reverts to forced fall-back 20.
    host.world.violence = Some(2);
    let first = spawn(&mut host);
    let corpse = kill(&mut host, first, 61);
    assert_eq!(Fid(corpse.raw.art_id).animation(), 20);
    assert!(
        corpse
            .raw
            .flags
            .contains(vault_formats::pro::BasicFlags::NONBLOCKING)
    );
    assert!(
        corpse
            .raw
            .flags
            .contains(vault_formats::pro::BasicFlags::FLAT)
    );
    // With maximum violence and available art the static code is kept.
    host.world.violence = Some(3);
    let second = spawn(&mut host);
    assert_eq!(Fid(kill(&mut host, second, 61).raw.art_id).animation(), 61);
    // Final animation 31 drops the inventory onto the corpse's hex.
    let third = spawn(&mut host);
    let item = host
        .world
        .spawn(host.assets.prototype.clone(), 1, 0)
        .unwrap();
    host.world.add_item(third, item, 1, true).unwrap();
    kill(&mut host, third, 31);
    let dropped = host.world.get(item).unwrap();
    assert_eq!(
        (dropped.owner, dropped.raw.hex, dropped.raw.elevation),
        (None, Some(20120), 1)
    );
}

#[test]
fn spec009_ai_opcodes_alert_gate_hearing_team_and_party() {
    use fixture::{instruction, literal, push};
    let (mut host, base, _) = host(&[b"Hello"]);
    let me = host.world.player;
    let spawn = |host: &mut WorldHost<Assets>, hex| {
        host.world
            .spawn(host.assets.prototype.clone(), hex, 1)
            .unwrap()
    };
    let target = spawn(&mut host, 20102);
    let other = spawn(&mut host, 20104);
    let attack = |defender: ObjectRef| {
        [
            push(defender as i32),
            push(0),
            push(0),
            push(50),
            push(1),
            push(10),
            push(0),
            push(1),
            instruction(0x80d0),
        ]
        .concat()
    };
    let maneuvers =
        |host: &WorldHost<Assets>, critter| host.world.critter_state(critter).unwrap().maneuvers;
    // In combat with alert clear: set it and reference the target directly
    // (no combat request); with alert already set nothing changes.
    host.world.combat = true;
    run_fragment(&mut host, &base, &attack(target)).unwrap();
    assert_eq!(host.world.get(me).unwrap().opponent, target);
    assert_eq!(maneuvers(&host, me) & 1, 1);
    host.world.get_mut(me).unwrap().opponent = 0;
    run_fragment(&mut host, &base, &attack(other)).unwrap();
    assert_eq!(host.world.get(me).unwrap().opponent, 0);
    assert!(host.world.pending_combat.is_none());

    // obj_can_hear_obj (safe rule): 1 for two placed objects on one
    // elevation that pass the perception test; a null operand returns 0
    // with a diagnostic instead of failing.
    let hear = |host: &mut WorldHost<Assets>, a: ObjectRef, b: ObjectRef| {
        let code = [push(a as i32), push(b as i32), instruction(0x80f5)].concat();
        run_fragment(host, &base, &code)
            .unwrap()
            .value_stack
            .pop()
            .unwrap()
    };
    assert_eq!(hear(&mut host, target, me), Value::integer(1));
    let level = host.world.get(target).unwrap().raw.elevation;
    host.world.get_mut(target).unwrap().raw.elevation = (level + 1) % 3;
    assert_eq!(hear(&mut host, target, me), Value::integer(0));
    host.world.get_mut(target).unwrap().raw.elevation = level;
    host.world.effects.clear();
    assert_eq!(hear(&mut host, 0, me), Value::integer(0));
    assert!(matches!(
        host.world.effects.front(),
        Some(WorldEffect::Diagnostic(_))
    ));

    // critter_add_trait kind 1 selector 6 switches teams, clearing a last
    // attacker on the new team; party_add switches the member to team 0.
    let team = |host: &WorldHost<Assets>, critter| host.world.critter_state(critter).unwrap().team;
    host.world.critter_state_mut(other).unwrap().team = 4;
    host.world.get_mut(target).unwrap().opponent = other;
    let add_trait = |critter: ObjectRef, value: i32| {
        [
            push(critter as i32),
            push(1),
            push(6),
            push(value),
            instruction(0x8102),
        ]
        .concat()
    };
    run_fragment(&mut host, &base, &add_trait(target, 4)).unwrap();
    assert_eq!(team(&host, target), 4);
    assert_eq!(host.world.get(target).unwrap().opponent, 0);
    host.world.get_mut(target).unwrap().opponent = other;
    run_fragment(&mut host, &base, &add_trait(target, 4)).unwrap();
    assert_eq!(
        host.world.get(target).unwrap().opponent,
        other,
        "an unchanged team is skipped"
    );
    run_fragment(
        &mut host,
        &base,
        &[push(target as i32), instruction(0x8124)].concat(),
    )
    .unwrap();
    assert!(host.world.party.contains(&target));
    assert_eq!(team(&host, target), 0);

    // float_msg creates a live floating text (which suppresses taunts) for
    // the default 3500 ms plus 1399 ms per line.
    host.world.elevation = 1;
    let float = [
        push(target as i32),
        literal(6),
        push(1),
        instruction(0x810a),
    ]
    .concat();
    run_fragment(&mut host, &base, &float).unwrap();
    assert_eq!(host.world.floating, [(target, 3500 + 1399)]);
}

#[test]
fn spec010_sound_name_opcodes_use_prototype_sound_bytes() {
    use fixture::{instruction, push};
    let (mut host, base, _) = host(&[b"Hello"]);
    // A container with item sound byte `B` (0x38) and a weapon with sound
    // byte `E` (0x79); both stand on the player's elevation.
    let spawn = |host: &mut WorldHost<Assets>,
                 category: u8,
                 subtype: usize,
                 byte: usize,
                 value: u8,
                 hex| {
        let mut bytes = fixture::world::prototype(category, subtype);
        bytes[byte] = value;
        let prototype = Arc::new(Prototype::parse(&bytes).unwrap());
        host.world.spawn(prototype, hex, 1).unwrap() as i32
    };
    let container = spawn(&mut host, 0, 1, 0x38, b'B', 20102);
    let weapon = spawn(&mut host, 0, 3, 0x79, b'E', 20104);
    let door = spawn(&mut host, 2, 0, 0x28, b'K', 20106);
    let name = |host: &mut WorldHost<Assets>, code: Vec<u8>| {
        let mut vm = run_fragment(host, &base, &code).unwrap();
        let value = vm.value_stack.pop().unwrap();
        // D offset zero is the null result unless a string occupies it.
        vm.string(value).ok().map(<[u8]>::to_vec)
    };
    // sfx_build_open_name(action, object).
    assert_eq!(
        name(
            &mut host,
            [push(container), push(1), instruction(0x80a5)].concat()
        ),
        Some(b"ICCNTNRB".to_vec())
    );
    assert_eq!(
        name(
            &mut host,
            [push(door), push(2), instruction(0x80a5)].concat()
        ),
        Some(b"SLDOORSK".to_vec())
    );
    // A null object pushes D offset zero.
    assert_eq!(
        name(&mut host, [push(0), push(1), instruction(0x80a5)].concat()),
        None
    );
    // sfx_build_weapon_name(target, hit mode, weapon, effect type): a hit on
    // the container (material 0: `M`) with the secondary mode.
    let weapon_name = |target: i32, mode: i32, effect: i32| {
        [
            push(effect),
            push(weapon),
            push(mode),
            push(target),
            instruction(0x8141),
        ]
        .concat()
    };
    assert_eq!(
        name(&mut host, weapon_name(container, 3, 4)),
        Some(b"WHE2MXX1".to_vec())
    );
    assert_eq!(
        name(&mut host, weapon_name(0, 2, 2)),
        Some(b"WOE1XXX1".to_vec())
    );
    // Safe rule: an out-of-range effect type or hit mode is a VM error.
    for (mode, effect) in [(2, 7), (8, 1)] {
        assert!(run_fragment(&mut host, &base, &weapon_name(0, mode, effect)).is_err());
    }
    // sfx_build_char_name(extra, animation, object): without an art-list
    // token there is no name, which pushes empty D text (safe rule).
    let me = host.world.player as i32;
    assert_eq!(
        name(
            &mut host,
            [push(me), push(16), push(4), instruction(0x813d)].concat()
        ),
        Some(Vec::new())
    );
}

#[test]
fn spec011_progression_and_barter_opcodes() {
    use fixture::{instruction, push};
    let (mut host, base, _) = host(&[b"Hello"]);
    let me = host.world.player;
    // give_exp_points applies Swift Learner and the level loop, playing
    // the level-up sound once per level.
    host.world
        .player_sheet_mut()
        .unwrap()
        .perks
        .insert(Perk::SwiftLearner, 1);
    run_fragment(
        &mut host,
        &base,
        &[push(2900), instruction(0x80a1)].concat(),
    )
    .unwrap();
    let sheet = host.world.player_sheet().unwrap();
    assert_eq!((sheet.experience, sheet.level), (3045, 3));
    let sounds = host
        .world
        .effects
        .iter()
        .filter(
            |e| matches!(e, WorldEffect::SoundEffect { name, owner: None } if name == b"levelup"),
        )
        .count();
    assert_eq!(sounds, 2);
    // has_trait kind 0 reads ranks by SPEC-011 perk number.
    let has = |host: &mut WorldHost<Assets>, perk: i32| {
        let code = [push(0), push(me as i32), push(perk), instruction(0x80f3)].concat();
        run_fragment(host, &base, &code)
            .unwrap()
            .value_stack
            .pop()
            .unwrap()
    };
    assert_eq!(has(&mut host, 50), Value::integer(1), "Swift Learner");
    assert_eq!(has(&mut host, 0), Value::integer(0));
    // critter_add_trait kind 0 uses the chooser's eligibility test and
    // fails silently; either way it pushes -1.
    let add = |host: &mut WorldHost<Assets>, perk: i32| {
        let code = [
            push(me as i32),
            push(0),
            push(perk),
            push(1),
            instruction(0x8102),
        ]
        .concat();
        run_fragment(host, &base, &code)
            .unwrap()
            .value_stack
            .pop()
            .unwrap()
    };
    assert_eq!(add(&mut host, 0), Value::integer(-1));
    assert_eq!(add(&mut host, 23), Value::integer(-1));
    let sheet = host.world.player_sheet().unwrap();
    assert_eq!(
        (sheet.rank(Perk::Awareness), sheet.rank(Perk::Slayer)),
        (1, 0),
        "Awareness (level 3, PE 5) passes; Slayer (level 18) does not"
    );
    assert_eq!(add(&mut host, 0), Value::integer(-1));
    assert_eq!(
        host.world.player_sheet().unwrap().rank(Perk::Awareness),
        1,
        "maximum rank 1"
    );
    // gdialog_barter replaces the script modifier and requests barter;
    // gdialog_set_barter_mod replaces it alone. SPEC-013 §3.18: only a
    // merchant whose prototype has flag 0x02 switches to barter.
    host.world.dialogue.open = true;
    let mut merchant = fixture::npc_prototype();
    fixture::world::word(&mut merchant, 0x2c, 0x02);
    let merchant = host
        .world
        .spawn(Arc::new(Prototype::parse(&merchant).unwrap()), 20300, 0)
        .unwrap();
    host.world.dialogue.target = merchant;
    run_fragment(&mut host, &base, &[push(-20), instruction(0x8129)].concat()).unwrap();
    assert_eq!(host.world.dialogue.barter_modifier, -20);
    assert!(host.world.dialogue.pending_barter);
    host.world.dialogue.pending_barter = false;
    let plain = host
        .world
        .spawn(
            Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap()),
            20500,
            0,
        )
        .unwrap();
    host.world.dialogue.target = plain;
    run_fragment(&mut host, &base, &[push(-5), instruction(0x8129)].concat()).unwrap();
    assert_eq!(host.world.dialogue.barter_modifier, -5);
    assert!(!host.world.dialogue.pending_barter, "no barter flag");
    run_fragment(&mut host, &base, &[push(7), instruction(0x814e)].concat()).unwrap();
    assert_eq!(host.world.dialogue.barter_modifier, 7);
}
#[test]
fn spec006_head_counts_use_own_columns_only_when_fidget_art_exists() {
    let dir = tempfile::tempdir().unwrap();
    let heads = dir.path().join("art/heads");
    std::fs::create_dir_all(&heads).unwrap();
    std::fs::write(
        heads.join("heads.lst"),
        b"full,1,2,3\npart,1,2,3\nfour,1,4,0\n",
    )
    .unwrap();
    for name in [
        "fullgf1", "fullnf1", "fullnf2", "fullbf1", "fullbf2", "fullbf3", "partgf1", "partnf1",
        "partnf2", "partbf1", "partbf2", "fourgf1", "fournf4", "fourbf1",
    ] {
        std::fs::write(heads.join(format!("{name}.frm")), b"").unwrap();
    }
    let assets = VfsWorldAssets::new(
        Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    assert_eq!(assets.head(0).unwrap().counts, [1, 2, 3]);
    // `partbf3.frm` is missing, so the bad count keeps the first column.
    assert_eq!(assets.head(1).unwrap().counts, [1, 2, 1]);
    // A count above 3 selects only its own variant; a zero count is never
    // selectable and keeps the first column.
    assert_eq!(assets.head(2).unwrap().counts, [1, 4, 1]);
}

#[test]
fn vfs_assets_read_each_resource_once_failures_included() {
    use std::{collections::BTreeMap, sync::Mutex};
    use vault_formats::{LocalDirProvider, Vfs, VfsError, VfsPath};
    struct Counting {
        inner: LocalDirProvider,
        reads: Mutex<BTreeMap<String, usize>>,
    }
    impl Vfs for Counting {
        fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
            *self.reads.lock().unwrap().entry(path.into()).or_default() += 1;
            self.inner.read(path)
        }
        fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
            *self.reads.lock().unwrap().entry(path.into()).or_default() += 1;
            self.inner.read_bounded(path, max_bytes)
        }
        fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
            self.inner.files()
        }
    }
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let counting = Arc::new(Counting {
        inner: LocalDirProvider::mount(dir.path()).unwrap(),
        reads: Default::default(),
    });
    let assets = VfsWorldAssets::new(counting.clone(), "english").unwrap();
    for _ in 0..3 {
        // Lists, message catalogs, prototypes, frames and palettes, found
        // or not.
        let _ = assets.message(1, 100);
        let _ = assets.local_count(0);
        let _ = assets.prototype(0x0100_0001);
        assert!(assets.prototype(0x0100_0009).is_err());
        assert!(assets.frame("art/scenery/sample.frm").is_ok());
        assert!(assets.frame("art/scenery/ghost.frm").is_err());
        assert!(assets.game_messages("pro_item.msg").is_ok());
        assert!(assets.game_messages("absent.msg").is_err());
        assert!(assets.palette(b"color.pal").is_ok());
        assert!(assets.palette(b"absent.pal").is_err());
        let _ = assets.head(0);
        let _ = assets.interface_path(0);
        assert!(assets.available(b"color.pal"));
        assert!(!assets.available(b"absent.pal"));
    }
    let reads = counting.reads.lock().unwrap();
    assert!(reads.len() > 8, "{reads:?}");
    assert!(reads.values().all(|count| *count == 1), "{reads:?}");
}

#[test]
fn spec014_an_entry_hex_outside_the_map_border_fails_the_load() {
    // SPEC-014 §3.29: map entry is a view-center change under the border.
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let mut map = fixture::interactive_map();
    fixture::world::word(&mut map, 0x14, 30 * 200 + 100);
    std::fs::write(dir.path().join("maps/outside.map"), map).unwrap();
    let assets = VfsWorldAssets::new(
        Arc::new(vault_formats::LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    let error = host.load_map("maps/outside.map").unwrap_err();
    assert!(
        error.to_string().contains("outside the map border"),
        "{error}"
    );
    host.load_map("maps/synthetic.map").unwrap();
    assert!(crate::ui::cursor::inside_border(host.world.camera_hex));
}

#[test]
fn spec005_world_map_opens_the_town_map_first_and_town_map_the_world_map() {
    // SPEC-005 0x8108 `world_map` requests map -1 (the current town's town
    // map, then the world map); 0x8109 `town_map` requests map -2 (the
    // world map directly), SPEC-014 §3.39.
    for (op, map) in [(0x8108, -1), (0x8109, -2)] {
        let (mut host, base, _) = host(&[b"Hello"]);
        run_fragment(&mut host, &base, &fixture::instruction(op)).unwrap();
        assert_eq!(
            host.world.pending_transition,
            Some(crate::Transition {
                map,
                elevation: -1,
                hex: -1,
                facing: -1
            }),
            "{op:#x}"
        );
    }
}
