use super::*;
use crate::{
    decode::{decode, decode_next, disassemble},
    objects::{render_critters, render_object_detail, render_objects, snapshot_objects},
    stepper::{status_names, value_text},
    trace::render_trace,
    watch::{ChangeKind, render_vars},
};
use vault_core::{
    CharacterSheet, ObjectRef,
    world::{ScriptInstance, WorldObject, object_record},
};
use vault_fixtures::vm::{self as fixture, op, push};
use vault_formats::{
    map::{CritterState, ObjectState},
    pro::{Category, CombatFlags},
};
use vault_vm::{BasicHost, IntProgram, RunOutcome, Value, VmError, VmStatus};

fn fragment(parts: &[Vec<u8>]) -> Vm {
    let program = IntProgram::parse(&fixture::program()).unwrap();
    Vm::fragment(program, &parts.concat()).unwrap()
}
/// `push 2; push 3; add; nop` at 0x0, 0x6, 0xc, 0xe; the end is 0x10.
fn arithmetic() -> Vm {
    fragment(&[push(2), push(3), op(0x8039), op(0x8000)])
}

// --- trace ring ---------------------------------------------------------

#[test]
fn ring_evicts_oldest_and_iterates_oldest_first() {
    let mut ring = TraceRing::new(3);
    assert!(ring.is_empty());
    for value in 0..3 {
        assert_eq!(ring.push(value), None);
    }
    assert_eq!(ring.push(3), Some(0));
    assert_eq!(ring.push(4), Some(1));
    assert_eq!(ring.iter().copied().collect::<Vec<_>>(), [2, 3, 4]);
    assert_eq!(ring.iter().rev().copied().collect::<Vec<_>>(), [4, 3, 2]);
    assert_eq!(ring.last(2).copied().collect::<Vec<_>>(), [3, 4]);
    assert_eq!(ring.last(10).copied().collect::<Vec<_>>(), [2, 3, 4]);
    assert_eq!(ring.last(0).count(), 0);
    assert_eq!(ring.newest(), Some(&4));
    assert_eq!((ring.len(), ring.capacity()), (3, 3));
    assert_eq!((ring.pushed(), ring.evicted()), (5, 2));
    ring.clear();
    assert!(ring.is_empty());
    assert_eq!((ring.pushed(), ring.evicted()), (5, 2));
    ring.push(9);
    assert_eq!(ring.iter().copied().collect::<Vec<_>>(), [9]);
}

#[test]
fn zero_capacity_ring_counts_but_keeps_nothing() {
    let mut ring = TraceRing::new(0);
    assert_eq!(ring.push('a'), Some('a'));
    assert!(ring.is_empty());
    assert_eq!((ring.pushed(), ring.evicted()), (1, 1));
}

// --- decoding -----------------------------------------------------------

#[test]
fn decode_lengths_immediates_and_classes() {
    let code = [push(-7), op(0x8039), vec![0x80, 0x01, 0, 0, 0, 9]].concat();
    let first = decode(&code, 0).unwrap();
    assert_eq!(
        (
            first.word,
            first.immediate,
            first.length(),
            first.next_address()
        ),
        (0xc001, Some(-7_i32 as u32), 6, 6)
    );
    assert_eq!(
        (first.class(), first.text()),
        (OpcodeClass::Push, "push.int -7".into())
    );
    let add = decode(&code, 6).unwrap();
    assert_eq!(
        (add.immediate, add.length(), add.text()),
        (None, 2, "add".into())
    );
    assert_eq!(add.class(), OpcodeClass::Core);
    // The generic push keeps its own word and is not normalized.
    let raw = decode(&code, 8).unwrap();
    assert_eq!((raw.word, raw.text()), (0x8001, "push.raw 0x9".into()));
    assert_eq!(decode(&code, 14), Err(DecodeError::End(14)));
    assert_eq!(decode(&code, 15), Err(DecodeError::Truncated(15)));

    let float = decode(&[0xa0, 0x01, 0x3f, 0xc0, 0, 0], 0).unwrap();
    assert_eq!(float.text(), "push.float 1.50000");
    let string = decode(&[0x98, 0x01, 0, 0, 0, 0x10], 0).unwrap();
    assert_eq!(string.text(), "push.dstr 0x10");
}

#[test]
fn decode_registration_ranges_and_invalid_words() {
    for (word, class) in [
        (0x8000, Some(OpcodeClass::Core)),
        (0x804b, Some(OpcodeClass::Core)),
        (0x804c, Some(OpcodeClass::Library)),
        (0x80a0, Some(OpcodeClass::Library)),
        (0x80a1, Some(OpcodeClass::Game)),
        (0x8155, Some(OpcodeClass::Game)),
        (0x8156, None),
        (0x7fff, None),
        (0xc801, Some(OpcodeClass::Push)),
        // SPEC-004 §2.5: all 32 slot-1 words with bit 15 are pushes, the
        // non-standard tags included; the VM agrees.
        (0x8401, Some(OpcodeClass::Push)),
        (0xfc01, Some(OpcodeClass::Push)),
        (0x0001, None),
        // Other words with bits 10-14 set stay rejected.
        (0xc039, None),
    ] {
        assert_eq!(decode::classify(word), class, "{word:#06x}");
    }
    let tagged = decode(&[0x84, 0x01, 0, 0, 0, 5], 0).unwrap();
    assert_eq!(
        (tagged.class(), tagged.text()),
        (OpcodeClass::Push, "push.tag 0x8401 0x5".to_string())
    );
    let mut vm = fragment(&[vec![0x84, 0x01, 0, 0, 0, 5]]);
    assert_eq!(decode_next(&vm).unwrap().text(), tagged.text());
    let mut host = BasicHost::default();
    assert_eq!(vm.update(&mut host).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(vm.value_stack.top().unwrap().tag, 0x8401);
    assert_eq!(
        decode(&op(0x8156), 0),
        Err(DecodeError::Invalid {
            address: 0,
            word: 0x8156
        })
    );
    // A truncated immediate.
    assert_eq!(
        decode(&[0xc0, 0x01, 0, 0], 0),
        Err(DecodeError::Truncated(0))
    );
    let ext = decode(&op(0x80a5), 0).unwrap();
    assert_eq!((ext.mnemonic(), ext.text()), ("ext", "ext 0x80a5".into()));
    let lib = decode(&op(0x8050), 0).unwrap();
    assert_eq!(lib.text(), "lib 0x8050");
}

#[test]
fn instruction_display_and_disassembly() {
    let vm = arithmetic();
    let listing = disassemble(vm.code(), 0, 10);
    let lines: Vec<String> = listing.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "0x0000  c001 00000002  push.int 2".to_string(),
            "0x0006  c001 00000003  push.int 3".into(),
            format!("0x000c  8039{}add", " ".repeat(11)),
            format!("0x000e  8000{}nop", " ".repeat(11)),
        ]
    );
    assert_eq!(disassemble(vm.code(), 0, 2).len(), 2);
}

#[test]
fn decode_next_checks_boundaries() {
    let mut vm = arithmetic();
    assert_eq!(decode_next(&vm).unwrap().text(), "push.int 2");
    vm.pc = 2;
    assert_eq!(decode_next(&vm), Err(DecodeError::Address(2)));
    vm.pc = 16;
    assert_eq!(decode_next(&vm), Err(DecodeError::End(16)));
    assert_eq!(
        DecodeError::Address(2).to_string(),
        "0x0002 is not an instruction boundary"
    );
}

// --- stepping -----------------------------------------------------------

#[test]
fn step_executes_one_instruction_and_records_it() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(16);
    let first = stepper.step(&mut vm, &mut host);
    assert_eq!(
        (first.seq, first.pc, first.next_pc, first.value_depth),
        (0, 0, 6, 1)
    );
    assert!(first.executed);
    assert_eq!(first.outcome, StepOutcome::Ran(RunOutcome::Budget));
    assert_eq!(first.instruction.unwrap().text(), "push.int 2");
    assert_eq!(
        first.to_string(),
        format!(
            "#0      vm=1 0x0000  push.int 2{} -> 0x0006 vs=1 rs=0 st=0x0000>0x0000 ok",
            " ".repeat(14)
        )
    );
    stepper.step(&mut vm, &mut host);
    let add = stepper.step(&mut vm, &mut host);
    assert_eq!((add.pc, add.value_depth), (12, 1));
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(5));
    // The VM reports the fragment end as soon as the final instruction runs.
    let nop = stepper.step(&mut vm, &mut host);
    assert_eq!(nop.outcome, StepOutcome::Ran(RunOutcome::FragmentEnd));
    assert!(nop.executed);
    let end = stepper.step(&mut vm, &mut host);
    assert_eq!(end.outcome, StepOutcome::Ran(RunOutcome::FragmentEnd));
    assert_eq!((end.instruction, end.executed), (None, false));
    assert!(end.to_string().ends_with("fragmentend (not executed)"));
    assert_eq!(stepper.trace().len(), 5);
    assert_eq!(
        stepper.trace().iter().map(|e| e.seq).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn breakpoints_stop_before_execution_and_resume() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(16);
    assert!(stepper.add_breakpoint(12));
    assert!(!stepper.add_breakpoint(12));
    let stop = stepper.run_until_break(&mut vm, &mut host, 100);
    assert_eq!(
        stop,
        RunStop {
            steps: 2,
            reason: StopReason::Breakpoint(12)
        }
    );
    assert_eq!(
        stop.to_string(),
        "stopped after 2 steps: breakpoint at 0x000c"
    );
    assert_eq!(vm.pc, 12);
    // Resuming from the breakpoint executes it; the VM reports the fragment
    // end as soon as the final instruction runs.
    let stop = stepper.run_until_break(&mut vm, &mut host, 100);
    assert_eq!(
        stop,
        RunStop {
            steps: 2,
            reason: StopReason::Outcome(RunOutcome::FragmentEnd)
        }
    );
    assert_eq!(stepper.breakpoints().collect::<Vec<_>>(), [12]);
    assert!(stepper.remove_breakpoint(12));
    stepper.clear_breakpoints();
    assert_eq!(stepper.breakpoints().count(), 0);
}

#[test]
fn run_until_break_honours_the_step_limit() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(16);
    let stop = stepper.run_until_break(&mut vm, &mut host, 1);
    assert_eq!(stop.reason, StopReason::StepLimit);
    assert_eq!(stop.steps, 1);
    assert_eq!(
        stepper.run_until_break(&mut vm, &mut host, 0).reason,
        StopReason::StepLimit
    );
    // Reaching a breakpoint exactly at the limit reports the breakpoint.
    stepper.add_breakpoint(12);
    let stop = stepper.run_until_break(&mut vm, &mut host, 1);
    assert_eq!(stop.reason, StopReason::Breakpoint(12));
}

#[test]
fn faults_are_recorded_in_the_trace() {
    let mut vm = fragment(&[op(0x801a)]);
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(4);
    let stop = stepper.run_until_break(&mut vm, &mut host, 10);
    assert_eq!(stop.reason, StopReason::Fault(VmError::Underflow));
    let entry = stepper.trace().newest().unwrap();
    assert_eq!(entry.outcome, StepOutcome::Fault(VmError::Underflow));
    assert_eq!(entry.instruction.unwrap().mnemonic(), "drop");
    assert!(entry.status_after & VmStatus::ERROR != 0);
    assert!(entry.to_string().ends_with("fault: stack underflow"));
    assert_eq!(status_names(entry.status_after), "ended|error");
}

/// `critical.enter; push 1; push 2; critical.leave; nop` at 0x0, 0x2, 0x8,
/// 0xe, 0x10.
fn critical_section() -> Vm {
    fragment(&[op(0x8002), push(1), push(2), op(0x8003), op(0x8000)])
}

#[test]
fn a_critical_section_is_one_step() {
    let mut vm = critical_section();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(4);
    assert!(!stepper.step_into_critical, "off unless asked for");
    let entry = stepper.step(&mut vm, &mut host);
    assert_eq!(entry.instruction.unwrap().mnemonic(), "critical.enter");
    assert_eq!((entry.next_pc, entry.value_depth), (16, 2));
    assert_eq!(entry.status_after, 0);
    assert_eq!(stepper.critical_hold(), None);
}

#[test]
fn stepping_into_a_critical_section_runs_one_instruction_at_a_time() {
    let mut vm = critical_section();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(8);
    stepper.step_into_critical = true;
    let hold = Some(CriticalHold { vm: 1, now_ms: 0 });
    for (next_pc, critical, held) in [
        (2, true, hold),
        (8, true, hold),
        (14, true, hold),
        (16, false, None),
    ] {
        let entry = stepper.step(&mut vm, &mut host);
        assert_eq!(entry.next_pc, next_pc);
        assert_eq!(entry.outcome, StepOutcome::Ran(RunOutcome::Budget));
        assert_eq!(entry.status_after & VmStatus::CRITICAL != 0, critical);
        assert_eq!(stepper.critical_hold(), held, "after {next_pc:#x}");
    }
    // It ends where the atomic step does, on the same stacks.
    let mut atomic = critical_section();
    Stepper::new(1).step(&mut atomic, &mut host);
    assert_eq!(
        (vm.pc, vm.status, vm.value_stack.bytes()),
        (atomic.pc, atomic.status, atomic.value_stack.bytes())
    );
    assert_eq!(
        stepper
            .trace()
            .iter()
            .map(|entry| entry.pc)
            .collect::<Vec<_>>(),
        [0, 2, 8, 14]
    );
}

#[test]
fn a_breakpoint_inside_a_critical_section_stops_only_when_stepping_into_it() {
    let mut stepper = Stepper::new(8);
    stepper.add_breakpoint(8);
    let mut host = BasicHost::default();
    let mut vm = critical_section();
    let stop = stepper.run_until_break(&mut vm, &mut host, 100);
    assert_eq!(stop.reason, StopReason::Outcome(RunOutcome::FragmentEnd));

    stepper.step_into_critical = true;
    let mut vm = critical_section();
    let stop = stepper.run_until_break(&mut vm, &mut host, 100);
    assert_eq!(stop.reason, StopReason::Breakpoint(8));
    assert_eq!(stop.steps, 2);
    assert!(stepper.critical_hold().is_some());
}

#[test]
fn a_hold_refuses_a_step_after_the_clock_moves_and_changes_nothing() {
    let mut vm = critical_section();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(8);
    stepper.step_into_critical = true;
    host.milliseconds = 40;
    stepper.step(&mut vm, &mut host);
    assert_eq!(
        stepper.critical_hold(),
        Some(CriticalHold { vm: 1, now_ms: 40 })
    );
    // The world ticked, which SPEC-004 §3.3 does not allow here.
    host.milliseconds = 56;
    let breach = HoldBreach::ClockMoved {
        vm: 1,
        from_ms: 40,
        to_ms: 56,
    };
    let (pc, values) = (vm.pc, vm.value_stack.bytes().to_vec());
    let entry = stepper.step(&mut vm, &mut host);
    assert_eq!(entry.outcome, StepOutcome::Refused(breach));
    assert!(!entry.executed);
    assert!(
        entry.to_string().ends_with(
            "refused: the clock moved from 40 to 56 inside vm 1's critical section (not executed)"
        ),
        "{entry}"
    );
    assert_eq!((vm.pc, vm.value_stack.bytes()), (pc, values.as_slice()));
    let stop = stepper.run_until_break(&mut vm, &mut host, 10);
    assert_eq!(stop.reason, StopReason::Refused(breach));
    assert_eq!(stop.steps, 1);
    assert_eq!(
        stop.to_string(),
        "stopped after 1 steps: refused: the clock moved from 40 to 56 inside vm 1's critical section"
    );
    // Accepting the breach lets stepping go on.
    assert_eq!(
        stepper.release_critical(),
        Some(CriticalHold { vm: 1, now_ms: 40 })
    );
    assert!(stepper.step(&mut vm, &mut host).executed);
}

#[test]
fn a_hold_refuses_another_vm() {
    let mut held = critical_section();
    let mut other = arithmetic();
    other.id = 2;
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(8);
    stepper.step_into_critical = true;
    stepper.step(&mut held, &mut host);
    let entry = stepper.step(&mut other, &mut host);
    assert_eq!(
        entry.outcome,
        StepOutcome::Refused(HoldBreach::OtherVm {
            held: 1,
            stepped: 2
        })
    );
    assert_eq!(other.pc, 0);
    assert!(stepper.step(&mut held, &mut host).executed);
}

#[test]
fn a_section_that_stops_itself_is_not_held() {
    // Normal execution ends its run here too, so others may run.
    let mut vm = fragment(&[op(0x8002), op(0x8011), op(0x8000)]);
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(8);
    stepper.step_into_critical = true;
    stepper.step(&mut vm, &mut host);
    assert!(stepper.critical_hold().is_some());
    let entry = stepper.step(&mut vm, &mut host);
    assert_eq!(entry.outcome, StepOutcome::Ran(RunOutcome::Stopped));
    assert!(entry.status_after & VmStatus::CRITICAL != 0);
    assert_eq!(stepper.critical_hold(), None);
}

#[test]
fn turning_stepping_off_inside_a_section_runs_it_to_its_end() {
    let mut vm = critical_section();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(8);
    stepper.step_into_critical = true;
    stepper.step(&mut vm, &mut host);
    stepper.step_into_critical = false;
    let entry = stepper.step(&mut vm, &mut host);
    // `Vm::run(host, 1)` does not count critical instructions, so the one
    // instruction it runs is the nop after the section.
    assert_eq!(entry.next_pc, 18);
    assert_eq!(entry.status_after, 0);
    assert_eq!(stepper.critical_hold(), None);
}

#[test]
fn a_non_runnable_vm_steps_without_executing() {
    let mut vm = arithmetic();
    vm.status = VmStatus(VmStatus::STOPPED);
    let mut stepper = Stepper::new(4);
    let entry = stepper.step(&mut vm, &mut BasicHost::default());
    assert_eq!(entry.outcome, StepOutcome::Ran(RunOutcome::Stopped));
    assert!(!entry.executed);
    assert_eq!(vm.pc, 0);
}

#[test]
fn trace_ring_bounds_long_runs() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(2);
    stepper.run_until_break(&mut vm, &mut host, 100);
    let trace = stepper.trace();
    assert_eq!((trace.len(), trace.pushed(), trace.evicted()), (2, 4, 2));
    assert_eq!(trace.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3]);
    let text = render_trace(trace, 1);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "trace: 2 of 4 retained (2 evicted, capacity 2)");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].starts_with("#3      vm=1 0x000e  nop"));
    stepper.clear_trace();
    assert!(stepper.trace().is_empty());
}

#[test]
fn vm_snapshot_renders_registers_and_stacks() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut stepper = Stepper::new(4);
    stepper.step(&mut vm, &mut host);
    stepper.step(&mut vm, &mut host);
    let snapshot = VmSnapshot::of(&vm);
    assert_eq!(snapshot.values, [Value::integer(2), Value::integer(3)]);
    assert_eq!(
        snapshot.to_string(),
        format!(
            "vm 1 pc=0x000c status=0x0000 (running) F=-1 G=-1\n\
             next: 0x000c  8039{}add\n\
             values [2]: int:2 int:3\n\
             returns [0]: \n",
            " ".repeat(11)
        )
    );
    assert_eq!(value_text(Value::float(0.5)), "float:0.50000");
    assert_eq!(
        value_text(Value {
            payload: 4,
            tag: Value::STATIC_STRING
        }),
        "str:0x4"
    );
    assert_eq!(
        value_text(Value {
            payload: 1,
            tag: 0x8001
        }),
        "0x8001:0x1"
    );
    assert_eq!(status_names(0), "running");
    assert_eq!(
        status_names(VmStatus::WAITING | VmStatus::CRITICAL),
        "waiting|critical"
    );
}

// --- variable watches ---------------------------------------------------

fn script(id: u32, locals: usize) -> ScriptInstance {
    ScriptInstance {
        id,
        owner: 0,
        file: 0,
        flags: 0,
        local_base: -1,
        local_count: locals,
        spatial: None,
        initialized: false,
        program: 0,
        source: 0,
        target: 0,
        event: 0,
        fixed: 0,
        action: 0,
        result: 0,
        overridden: false,
        margin: 0,
    }
}

#[test]
fn script_locals_resolve_through_their_own_slice() {
    let mut world = WorldState::empty(1).unwrap();
    world.scripts.insert(0x100, script(0x100, 2));
    world.scripts.insert(0x200, script(0x200, 3));
    let local = |script, index| VarAddress::ScriptLocal { script, index };
    // Unallocated locals are absent.
    assert_eq!(local(0x100, 0).read(&world), None);
    world.ensure_locals(0x100).unwrap();
    world.ensure_locals(0x200).unwrap();
    world.lvars.copy_from_slice(&[1, 2, 3, 4, 5]);
    assert_eq!(local(0x100, 1).read(&world), Some(2));
    assert_eq!(local(0x200, 0).pool_index(&world), Some(2));
    assert_eq!(local(0x200, 2).read(&world), Some(5));
    // Outside the script's own slice, even though the pool has the slot.
    assert_eq!(local(0x100, 2).read(&world), None);
    assert_eq!(local(0x300, 0).read(&world), None);
    assert_eq!(VarAddress::Local(4).read(&world), Some(5));
    assert_eq!(VarAddress::Local(5).read(&world), None);
    // No campaign globals before the initializer loads.
    assert_eq!(VarAddress::Global(0).read(&world), None);
    assert_eq!(local(0x200, 1).scope(), VarScope::Local);
    assert_eq!(local(0x200, 1).to_string(), "LVAR[0x200:1]");
    assert_eq!(VarAddress::Global(3).to_string(), "GVAR[3]");
}

#[test]
fn watcher_reports_changes_between_ticks_for_all_scopes() {
    let mut world = WorldState::empty(1).unwrap();
    world.gvars = Some(vec![0; 4]);
    world.mvars = vec![10, 20];
    world.scripts.insert(0x100, script(0x100, 2));
    let mut list = WatchList::new();
    list.watch_labeled(VarAddress::Global(1), "quest")
        .watch(VarAddress::Map(5))
        .watch(VarAddress::ScriptLocal {
            script: 0x100,
            index: 1,
        })
        .watch_all(VarScope::Map);
    let mut watcher = Watcher::new(list);
    assert!(
        watcher.tick(&world).is_empty(),
        "first tick is the baseline"
    );
    assert!(watcher.tick(&world).is_empty(), "nothing changed");

    world.gvars.as_mut().unwrap()[1] = 7;
    world.gvars.as_mut().unwrap()[2] = 9; // unwatched
    world.mvars[0] = 11;
    world.mvars.push(30);
    world.ensure_locals(0x100).unwrap();
    let changes = watcher.tick(&world);
    let lines: Vec<String> = changes.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "GVAR[1] (quest): 0 -> 7",
            "MVAR[0]: 10 -> 11",
            "MVAR[2]: - -> 30",
            "LVAR[0x100:1]: - -> 0",
        ]
    );
    assert_eq!(
        changes.iter().map(VarChange::kind).collect::<Vec<_>>(),
        [
            ChangeKind::Changed,
            ChangeKind::Changed,
            ChangeKind::Appeared,
            ChangeKind::Appeared
        ]
    );

    world.lvars[1] = 4;
    world.mvars.truncate(1);
    let lines: Vec<String> = watcher
        .tick(&world)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        lines,
        [
            "MVAR[1]: 20 -> -",
            "MVAR[2]: 30 -> -",
            "LVAR[0x100:1]: 0 -> 4"
        ]
    );
    world.scripts.remove(&0x100);
    let changes = watcher.tick(&world);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind(), ChangeKind::Disappeared);

    assert_eq!(
        render_vars(watcher.current().unwrap(), &watcher.list),
        "GVAR[1] = 7 (quest)\nMVAR[0] = 11\n"
    );
    watcher.reset();
    world.mvars[0] = 0;
    assert!(watcher.tick(&world).is_empty(), "reset re-baselines");
}

#[test]
fn watch_list_editing() {
    let mut list = WatchList::new();
    assert!(list.is_empty());
    list.watch(VarAddress::Global(0));
    list.watch_all(VarScope::Local);
    assert_eq!(list.label(VarAddress::Global(0)), None);
    list.watch_labeled(VarAddress::Global(0), "first");
    // Re-watching keeps the label.
    list.watch(VarAddress::Global(0));
    assert_eq!(list.label(VarAddress::Global(0)), Some("first"));
    assert!(list.unwatch(VarAddress::Global(0)));
    assert!(!list.unwatch(VarAddress::Global(0)));
    assert!(list.unwatch_all(VarScope::Local));
    assert!(list.is_empty());
    let mut world = WorldState::empty(1).unwrap();
    world.lvars = vec![3, 4];
    list.watch_all(VarScope::Local);
    let snapshot = list.snapshot(&world);
    assert_eq!(snapshot.get(VarAddress::Local(1)), Some(4));
    assert_eq!(snapshot.values.len(), 2);
}

// --- objects and critters -------------------------------------------------

fn critter_state(hp: i32, team: i32) -> ObjectState {
    ObjectState::Critter(CritterState {
        reaction: 0,
        damage: 0,
        maneuvers: 0,
        ap: 8,
        results: CombatFlags(0),
        ai: 4,
        team,
        attacker: -1,
        hp,
        radiation: 0,
        poison: 2,
    })
}
fn object(pid: u32, hex: i32, name: &str, state: Option<ObjectState>) -> WorldObject {
    let mut raw = object_record(pid, 0);
    raw.hex = Some(hex);
    if let Some(state) = state {
        raw.state = state;
    }
    WorldObject {
        raw,
        prototype: None,
        character: None,
        name: name.as_bytes().to_vec(),
        description: Vec::new(),
        owner: None,
        inventory: Vec::new(),
        money: None,
        active: true,
        opponent: 0,
    }
}
/// The player (0x10000), a door (0x10001), and a raider (0x10002).
fn populated() -> (WorldState, [ObjectRef; 3]) {
    let mut sheet = CharacterSheet {
        current_hp: 25,
        ..CharacterSheet::default()
    };
    sheet.radiation = 6;
    let mut world = WorldState::new(sheet, 1).unwrap();
    let door = world
        .insert(object(0x0200_0005, 100, "door", None))
        .unwrap();
    let raider = world
        .insert(object(
            0x0100_0007,
            200,
            "raider",
            Some(critter_state(12, 3)),
        ))
        .unwrap();
    let player = world.player;
    (world, [player, door, raider])
}

#[test]
fn object_and_critter_snapshots() {
    let (world, [player, door, raider]) = populated();
    assert_eq!((player, door, raider), (0x10000, 0x10001, 0x10002));
    let all = snapshot_objects(&world, &ObjectFilter::default());
    assert_eq!(all.len(), 3);
    let door_view = &all[1];
    assert_eq!(
        (door_view.category, door_view.hex, door_view.name.as_str()),
        (Category::Scenery, Some(100), "door")
    );
    assert!(door_view.critter.is_none());
    let player_view = all[0].critter.as_ref().unwrap();
    assert_eq!(
        (player_view.hp, player_view.radiation, player_view.level),
        (25, 6, Some(1))
    );
    assert!(player_view.player && player_view.alive);
    let raider_view = all[2].critter.as_ref().unwrap();
    assert_eq!(
        (
            raider_view.hp,
            raider_view.ap,
            raider_view.team,
            raider_view.ai
        ),
        (12, 8, 3, 4)
    );
    assert_eq!((raider_view.poison, raider_view.level), (2, None));
    assert!(!raider_view.player && !raider_view.party);

    let count = |filter: ObjectFilter| snapshot_objects(&world, &filter).len();
    assert_eq!(count(ObjectFilter::default().critters()), 2);
    assert_eq!(
        count(ObjectFilter::default().category(Category::Scenery)),
        1
    );
    assert_eq!(count(ObjectFilter::default().elevation(1)), 0);
    assert_eq!(count(ObjectFilter::default().elevation(0)), 3);
    assert_eq!(count(ObjectFilter::default().reference(raider)), 1);
    assert_eq!(count(ObjectFilter::default().reference(0x1)), 0);
}

#[test]
fn inactive_objects_are_filtered_unless_requested() {
    let (mut world, [_, door, _]) = populated();
    world.get_mut(door).unwrap().active = false;
    assert_eq!(snapshot_objects(&world, &ObjectFilter::default()).len(), 2);
    assert_eq!(snapshot_objects(&world, &ObjectFilter::all()).len(), 3);
}

#[test]
fn object_tracker_diffs_ticks() {
    let (mut world, [player, door, raider]) = populated();
    let mut tracker = ObjectTracker::new(ObjectFilter::default());
    assert!(
        tracker.tick(&world).is_empty(),
        "first tick is the baseline"
    );
    assert!(tracker.tick(&world).is_empty());

    world.get_mut(door).unwrap().raw.hex = Some(101);
    world.get_mut(raider).unwrap().raw.state = critter_state(0, 3);
    world
        .get_mut(player)
        .unwrap()
        .character
        .as_mut()
        .unwrap()
        .current_hp = 20;
    let lines: Vec<String> = tracker
        .tick(&world)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        lines,
        [
            "~ 0x10000 Player hp: 25 -> 20",
            "~ 0x10001 door hex: 100 -> 101",
            "~ 0x10002 raider hp: 12 -> 0",
        ]
    );

    let item = world.insert(object(0x0000_0029, 300, "", None)).unwrap();
    world.get_mut(door).unwrap().active = false;
    let changes = tracker.tick(&world);
    let lines: Vec<String> = changes.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "- 0x10001 door",
            "+ 0x10003 <unnamed> pid=0x00000029 hex=300"
        ]
    );
    assert_eq!(changes[1].reference(), item);
    assert_eq!(tracker.current().unwrap().len(), 3);
}

#[test]
fn losing_critter_state_reports_vanished_fields() {
    let (mut world, [_, _, raider]) = populated();
    let mut tracker = ObjectTracker::new(ObjectFilter::default().reference(raider));
    tracker.tick(&world);
    world.get_mut(raider).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: vault_formats::map::ObjectDetail::Plain,
    };
    let changes = tracker.tick(&world);
    let ObjectChange::Changed { fields, .. } = &changes[0] else {
        panic!("expected a change")
    };
    // Every critter field but the already-absent level.
    assert_eq!(fields.len(), 12);
    assert!(fields.iter().all(|change| change.new == "-"));
    assert_eq!((fields[0].field, fields[0].old.as_str()), ("hp", "12"));
}

#[test]
fn object_tables_are_fixed_column() {
    let (world, [.., raider]) = populated();
    let objects = snapshot_objects(&world, &ObjectFilter::default());
    let table = render_objects(&objects[1..2]);
    assert_eq!(
        table,
        "REF          ID PID        CAT        HEX EL OR FRAME ART        FLAGS         SID ACT NAME\n\
         0x10001       2 0x02000005 scenery    100  0  0     0 0x02000000 0x00000000     -1 yes door\n"
    );
    let critters = render_critters(&objects);
    assert_eq!(
        critters,
        "REF        HP  AP TEAM  AI  CID POISON  RAD ALIVE ROLE   NAME\n\
         0x10000    25   7    0   0   -1      0    6 yes   player Player\n\
         0x10002    12   8    3   4   -1      2    0 yes   -      raider\n"
    );
    let raider_view = snapshot_objects(&world, &ObjectFilter::default().reference(raider));
    let detail = render_object_detail(&raider_view[0]);
    assert!(detail.starts_with("object 0x10002\n  id: 3\n  pid: 0x01000007\n"));
    assert!(detail.contains("  team: 3\n"));
    assert!(detail.ends_with("  party: false\n"));
}

// --- inspector facade ---------------------------------------------------

#[test]
fn inspector_ticks_vars_and_objects_together() {
    let (mut world, [_, door, _]) = populated();
    world.gvars = Some(vec![0, 0]);
    let mut watches = WatchList::new();
    watches.watch_all(VarScope::Global);
    // Object tracking is opt-in (it snapshots the world every tick).
    let mut inspector = Inspector::new(InspectorConfig {
        watches,
        track_objects: true,
        ..InspectorConfig::default()
    });
    let frame = inspector.tick(&world);
    assert_eq!(frame.tick, 0);
    assert!(frame.is_empty());

    world.gvars.as_mut().unwrap()[1] = 3;
    world.get_mut(door).unwrap().raw.orientation = 2;
    let frame = inspector.tick(&world);
    assert_eq!(
        frame.to_string(),
        "tick 1: 1 var changes, 1 object changes\n  GVAR[1]: 0 -> 3\n  ~ 0x10001 door orientation: 0 -> 2\n"
    );
    assert!(inspector.tick(&world).is_empty());

    inspector.reset_baselines();
    world.gvars.as_mut().unwrap()[0] = 1;
    assert!(inspector.tick(&world).is_empty());

    let dump = inspector.render_world(&world);
    assert!(dump.starts_with("== vars ==\nGVAR[0] = 1\nGVAR[1] = 3\n== objects ==\nREF"));
    assert!(dump.contains("== critters ==\nREF"));
}

#[test]
fn inspector_can_skip_object_tracking() {
    let (mut world, [_, door, _]) = populated();
    let mut inspector = Inspector::new(InspectorConfig {
        track_objects: false,
        ..InspectorConfig::default()
    });
    inspector.tick(&world);
    world.get_mut(door).unwrap().raw.hex = Some(5);
    assert!(inspector.tick(&world).is_empty());
}

#[test]
fn inspector_steps_and_renders_a_vm() {
    let mut vm = arithmetic();
    let mut host = BasicHost::default();
    let mut inspector = Inspector::new(InspectorConfig {
        trace_capacity: 8,
        ..InspectorConfig::default()
    });
    inspector.stepper.add_breakpoint(14);
    let entry = inspector.step(&mut vm, &mut host);
    assert_eq!(entry.pc, 0);
    let stop = inspector.run_until_break(&mut vm, &mut host, 10);
    assert_eq!(stop.reason, StopReason::Breakpoint(14));
    let text = inspector.render_vm(&vm, 2);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "vm 1 pc=0x000e status=0x0000 (running) F=-1 G=-1");
    assert_eq!(lines[2], "values [1]: int:5");
    assert_eq!(lines[4], "trace: 3 of 3 retained (0 evicted, capacity 8)");
    assert_eq!(lines.len(), 7);
    assert!(lines[6].contains("add"));
}

// --- live trace -----------------------------------------------------------

/// Every script index loads the VM fixture program.
struct TraceAssets(IntProgram);
impl vault_core::assets::WorldAssets for TraceAssets {
    fn prototype(
        &self,
        pid: u32,
    ) -> Result<std::sync::Arc<vault_formats::Prototype>, vault_core::WorldError> {
        Err(vault_core::WorldError::Invalid(format!(
            "no prototype {pid:#x}"
        )))
    }
    fn program(&self, _index: i32) -> Result<(String, IntProgram), vault_core::WorldError> {
        Ok(("trace.int".to_owned(), self.0.clone()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
}

#[test]
fn live_trace_records_the_steps_a_host_runs_and_detaches() {
    let program = IntProgram::parse(&fixture::program()).unwrap();
    let mut world = WorldState::empty(1).unwrap();
    let script = world.add_script(0, 0, 0, 0).unwrap();
    let mut host = vault_core::WorldHost::new(world, TraceAssets(program));
    let mut inspector = Inspector::new(InspectorConfig::default());
    assert!(!inspector.attached());
    inspector.attach(&mut host);
    assert!(inspector.attached() && host.vm_observer.is_some());
    let request = vault_core::world::ScriptRequest {
        script,
        event: vault_core::ScriptEvent::Start,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    };
    host.dispatch(request.clone()).unwrap();
    let entries = inspector.live_entries(1000);
    assert!(!entries.is_empty(), "the start run was traced");
    // One entry per step, numbered in order, each from a decoded address.
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.seq, index as u64);
    }
    assert!(entries.iter().any(|entry| entry.executed));
    // Detached, the host runs unobserved and the trace stops.
    inspector.detach(&mut host);
    assert!(host.vm_observer.is_none() && inspector.live_entries(10).is_empty());
    host.dispatch(request).unwrap();
}

#[test]
fn observed_script_runs_end_where_unobserved_ones_do() {
    let run = |observed: bool| {
        let program = IntProgram::parse(&fixture::program()).unwrap();
        let mut world = WorldState::empty(1).unwrap();
        let script = world.add_script(0, 0, 0, 0).unwrap();
        let mut host = vault_core::WorldHost::new(world, TraceAssets(program));
        let mut inspector = Inspector::new(InspectorConfig::default());
        if observed {
            inspector.attach(&mut host);
        }
        let result = host.dispatch(vault_core::world::ScriptRequest {
            script,
            event: vault_core::ScriptEvent::Start,
            source: 0,
            target: 0,
            fixed: 0,
            action: -1,
            only_present: false,
        });
        let vm = host.programs.values().next().unwrap();
        (
            result.map(|r| (r.overridden, r.pending)).ok(),
            vm.pc,
            vm.status.0,
            vm.value_stack.bytes().to_vec(),
            host.calls.len(),
        )
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn spec017_words_decode_in_a_fallout2_program() {
    use crate::decode::{classify_in, decode_in};
    use vault_vm::WordSet;
    // SPEC-017 §2.1: 0x8156-0x8183 are host words of a Fallout 2 program
    // (the 42 reserved ones fail closed when run, not when decoded).
    for word in [0x8156, 0x8159, 0x815a, 0x8183] {
        assert_eq!(
            classify_in(&WordSet::FALLOUT2, word),
            Some(OpcodeClass::Game),
            "{word:#06x}"
        );
        assert_eq!(classify_in(&WordSet::FALLOUT1, word), None);
    }
    assert_eq!(classify_in(&WordSet::FALLOUT2, 0x8184), None);
    let inven_cmds = decode_in(&op(0x8156), 0, &WordSet::FALLOUT2).unwrap();
    assert_eq!(inven_cmds.class(), OpcodeClass::Game);
    assert_eq!(inven_cmds.text(), "inven_cmds 0x8156");
    assert!(
        decode(&op(0x8156), 0).is_err(),
        "Fallout 1 keeps refusing it"
    );
    // The next instruction of a Fallout 2 program decodes with its set.
    let program = IntProgram::parse_in(&fixture::program(), &WordSet::FALLOUT2, false).unwrap();
    let vm = Vm::fragment(program, &op(0x8157)).unwrap();
    assert_eq!(decode_next(&vm).unwrap().word, 0x8157);
}

/// SPEC-024a §7.3: under manifest scripting the debugger names the mod
/// words, and without it they stay refused. The next word of a running
/// program is what the F9 page shows.
#[test]
fn spec024a_mod_words_decode_with_their_names() {
    use crate::decode::{classify_in, decode_in};
    use vault_vm::WordSet;
    let words = WordSet::FALLOUT2.with_mods(true);
    for (word, name) in [
        (0x8300, "mod_api_version"),
        (0x8301, "mod_get_setting"),
        (0x8302, "mod_set_setting"),
        (0x8303, "mod_log"),
        (0x8304, "mod_register_hook"),
        (0x8305, "mod_hook_id"),
        (0x8306, "mod_get_hook_arg"),
        (0x8307, "mod_set_hook_arg"),
        (0x8308, "mod_set_hook_return"),
        (0x8309, "mod_game_mode"),
    ] {
        assert_eq!(classify_in(&words, word), Some(OpcodeClass::Game));
        assert_eq!(classify_in(&WordSet::FALLOUT2, word), None, "{word:#06x}");
        assert_eq!(decode::classify(word), None, "{word:#06x}");
        let instruction = decode_in(&op(word), 0, &words).unwrap();
        assert_eq!(instruction.class(), OpcodeClass::Game);
        assert_eq!(instruction.mnemonic(), name);
        assert_eq!(instruction.text(), format!("{name} {word:#06x}"));
    }
    for word in [0x82ff, 0x830a] {
        assert_eq!(classify_in(&words, word), None, "{word:#06x}");
        assert_eq!(
            decode_in(&op(word), 0, &words),
            Err(DecodeError::Invalid { address: 0, word })
        );
    }
    let program = IntProgram::parse_in(&fixture::program(), &words, false).unwrap();
    let vm = Vm::fragment(program, &op(0x8309)).unwrap();
    assert_eq!(decode_next(&vm).unwrap().text(), "mod_game_mode 0x8309");
    // A word in no catalog keeps its old rendering.
    let bogus = Instruction {
        address: 0,
        word: 0x830a,
        immediate: None,
        mode: None,
    };
    assert_eq!(
        (bogus.class(), bogus.mnemonic()),
        (OpcodeClass::Invalid, "invalid")
    );
}

/// SPEC-024b: sfall mode's words past Fallout 2's set show their mnemonics,
/// with their class. Fallout 2's set still refuses them, and with no session
/// the words below them keep `ext`.
#[test]
fn spec024b_sfall_words_decode_with_their_names() {
    use crate::decode::{classify_in, decode_in};
    use vault_vm::{WordSet, sfall::word_row};
    let words = WordSet::FALLOUT2_SFALL;
    for word in 0x8184..=0x8281_u16 {
        let name = word_row(word).unwrap_or_else(|| panic!("{word:#06x}")).name;
        assert_ne!(name, "invalid", "{word:#06x}");
        assert_eq!(classify_in(&words, word), Some(OpcodeClass::Game));
        assert_eq!(classify_in(&WordSet::FALLOUT2, word), None, "{word:#06x}");
        let instruction = decode_in(&op(word), 0, &words).unwrap();
        assert_eq!(instruction.class(), OpcodeClass::Game);
        assert_eq!(instruction.mnemonic(), name);
        assert_eq!(instruction.text(), format!("{name} {word:#06x}"));
    }
    // SPEC-024b's own table, at both ends of a block.
    for (word, name) in [
        (0x8200, "fs_size"),
        (0x8207, "register_hook"),
        (0x8262, "register_hook_proc"),
    ] {
        assert_eq!(decode::mnemonic(word), name);
    }
    assert_eq!(decode::mnemonic(0x8183), "ext", "Fallout 2's last");
    assert_eq!(decode::mnemonic(0x8282), "invalid", "past the set");
    assert_eq!(classify_in(&words, 0x8282), None);
    let program = IntProgram::parse_in(&fixture::program(), &words, false).unwrap();
    let vm = Vm::fragment(program, &op(0x8200)).unwrap();
    assert_eq!(decode_next(&vm).unwrap().text(), "fs_size 0x8200");
}

#[test]
fn hand_built_instructions_with_unregistered_words_do_not_panic() {
    // `decode` never yields such a word; a hand-built instruction is
    // classified `Invalid` and still renders.
    let bogus = Instruction {
        address: 4,
        word: 0x1234,
        immediate: None,
        mode: None,
    };
    assert_eq!(bogus.class(), OpcodeClass::Invalid);
    assert_eq!(bogus.text(), "invalid 0x1234");
    assert_eq!(bogus.to_string(), "0x0004  1234           invalid 0x1234");
}

/// SPEC-024b §3.2: the session, not the number, decides what a word is.
#[test]
fn the_session_decides_what_a_word_means() {
    use crate::decode::{classify_with, decode_with, mnemonic_with};
    use vault_vm::{WordSet, extensions::opcode};
    let fallout1 = DecodeMode::new(&WordSet::FALLOUT1, false);
    let fallout1_enhanced = DecodeMode::new(&WordSet::FALLOUT1, true);
    let fallout2_enhanced = DecodeMode::new(&WordSet::FALLOUT2, true);
    let sfall = DecodeMode::new(&WordSet::FALLOUT2_SFALL, false);
    let sfall_enhanced = DecodeMode::new(&WordSet::FALLOUT2_SFALL, true);
    use OpcodeClass::{Enhanced, Game};
    for (mode, word, class, name) in [
        // The table of §3.2.
        (fallout1_enhanced, 0x8200, Enhanced, "engine_api_version"),
        (fallout2_enhanced, 0x8200, Enhanced, "engine_api_version"),
        (sfall, 0x8200, Game, "fs_size"),
        (sfall_enhanced, 0x8200, Game, "fs_size"),
        (fallout1_enhanced, 0x8210, Enhanced, "op_cam_pan_to"),
        (fallout2_enhanced, 0x8213, Enhanced, "op_cam_reset"),
        (sfall, 0x8210, Game, "sfall_ver_major"),
        (sfall_enhanced, 0x8213, Game, "hero_select_win"),
        (sfall, 0x8156, Game, "read_byte"),
        (fallout2_enhanced, 0x8156, Game, "inven_cmds"),
        // Sfall mode's set ends at 0x8281; the enhanced range goes on. An
        // unassigned word loads, and fails only when run.
        (sfall_enhanced, 0x8282, Enhanced, "enhanced"),
        (sfall_enhanced, 0x82ff, Enhanced, "enhanced"),
        (fallout1_enhanced, 0x8201, Enhanced, "enhanced"),
    ] {
        let instruction = decode_with(&op(word), 0, mode).unwrap();
        assert_eq!(instruction.class(), class, "{word:#06x} {mode:?}");
        assert_eq!(instruction.mnemonic(), name, "{word:#06x} {mode:?}");
        assert_eq!(instruction.text(), format!("{name} {word:#06x}"));
    }
    // A word a session does not have is refused, named or not elsewhere.
    for (mode, word) in [
        (fallout1, 0x8200),
        (fallout1, 0x8156),
        (fallout1_enhanced, 0x8156),
        (fallout1_enhanced, 0x8184),
        (sfall, 0x8282),
        (fallout1_enhanced, 0x8300),
        (sfall_enhanced, 0x8300),
    ] {
        assert_eq!(classify_with(mode, word), None, "{word:#06x}");
        assert_eq!(mnemonic_with(mode, word), "invalid", "{word:#06x}");
        assert_eq!(
            decode_with(&op(word), 0, mode),
            Err(DecodeError::Invalid { address: 0, word })
        );
    }
    // A Fallout 1 script shows Fallout 1's words, not Fallout 2's.
    let fallout1_name = opcode(0x80b2).unwrap().name;
    assert_ne!(fallout1_name, "mark_area_known");
    assert_eq!(mnemonic_with(fallout1, 0x80b2), fallout1_name);
    assert_eq!(mnemonic_with(fallout2_enhanced, 0x80b2), "mark_area_known");
    assert_eq!(
        mnemonic_with(fallout1, 0x8050),
        opcode(0x8050).unwrap().name
    );
    // A reserved word has no name to show, and pushes and core words have
    // the one name.
    assert_eq!(mnemonic_with(fallout2_enhanced, 0x815a), "ext");
    assert_eq!(mnemonic_with(sfall, 0x8039), "add");
    assert_eq!(mnemonic_with(sfall, 0xc001), "push.int");
}

/// SPEC-024a §7.1: `0x8300`-`0x8309` under manifest scripting, in either
/// profile and with or without sfall mode, and the rest of the range never.
#[test]
fn mod_words_follow_manifest_scripting_in_every_mode() {
    use crate::decode::{classify_with, decode_with};
    use vault_vm::WordSet;
    for words in [
        WordSet::FALLOUT1,
        WordSet::FALLOUT2,
        WordSet::FALLOUT2_SFALL,
    ] {
        for enhanced in [false, true] {
            let mode = DecodeMode::new(&words.clone().with_mods(true), enhanced);
            let first = decode_with(&op(0x8300), 0, mode).unwrap();
            assert_eq!(first.text(), "mod_api_version 0x8300");
            assert_eq!(first.class(), OpcodeClass::Game);
            for word in [0x830a, 0x83ff] {
                assert_eq!(classify_with(mode, word), None, "{word:#06x}");
            }
        }
    }
}

/// The next instruction, and so the trace, reads as the program's session.
#[test]
fn a_running_program_selects_its_own_mode() {
    use vault_vm::WordSet;
    let vm_in = |words: &WordSet, enhanced: bool, word: u16| {
        let program = IntProgram::parse_in(&fixture::program(), words, enhanced).unwrap();
        Vm::fragment(program, &op(word)).unwrap()
    };
    let enhanced = vm_in(&WordSet::FALLOUT1, true, 0x8200);
    assert_eq!(
        decode_next(&enhanced).unwrap().text(),
        "engine_api_version 0x8200"
    );
    let sfall = vm_in(&WordSet::FALLOUT2_SFALL, true, 0x8200);
    assert_eq!(decode_next(&sfall).unwrap().text(), "fs_size 0x8200");
    let vanilla = vm_in(&WordSet::FALLOUT1, false, 0x8200);
    assert_eq!(
        decode_next(&vanilla),
        Err(DecodeError::Invalid {
            address: 0,
            word: 0x8200
        })
    );

    let mut vm = vm_in(&WordSet::FALLOUT1, true, 0x8200);
    struct Enhanced;
    impl VmHost for Enhanced {
        fn enhanced_opcodes(&self) -> bool {
            true
        }
    }
    let entry = Stepper::new(1).step(&mut vm, &mut Enhanced);
    assert!(entry.to_string().contains("engine_api_version 0x8200"));
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(2));
}

#[test]
fn a_host_gives_its_session_mode() {
    use vault_vm::WordSet;
    let program = IntProgram::parse(&fixture::program()).unwrap();
    let world = WorldState::empty(1).unwrap();
    let host = vault_core::WorldHost::new(world, TraceAssets(program));
    let mode = DecodeMode::of_host(&host);
    assert_eq!(mode, DecodeMode::new(&WordSet::FALLOUT1, false));
    assert_eq!(mode.words(), WordSet::FALLOUT1);
    assert!(!mode.enhanced());
}

/// Entry points with no session stay as they were.
#[test]
fn callers_without_a_session_keep_their_rendering() {
    use crate::decode::disassemble_with;
    use vault_vm::WordSet;
    let code = [op(0x80a5), op(0x8050)].concat();
    let plain = disassemble(&code, 0, 2);
    assert!(plain.iter().all(|instruction| instruction.mode.is_none()));
    assert_eq!(plain[0].text(), "ext 0x80a5");
    assert_eq!(plain[1].text(), "lib 0x8050");
    assert_eq!(decode::mnemonic(0x8200), "fs_size");
    assert_eq!(decode::classify(0x8200), None);

    let mode = DecodeMode::new(&WordSet::FALLOUT1, false);
    let named = disassemble_with(&code, 0, 5, mode);
    assert_eq!(named.len(), 2);
    assert!(
        named
            .iter()
            .all(|instruction| instruction.mode == Some(mode))
    );
    assert_ne!(named[0].mnemonic(), "ext");
}

// --- timed queue and object mutator --------------------------------------

#[test]
fn queue_entries_label_events_and_name_owners_by_map_id() {
    use vault_core::{queue::EventKind, skills};
    let mut world = WorldState::empty(1).unwrap();
    let mut item = |pid, id| {
        let mut record = object(pid, 100, "", None);
        record.raw.id = id;
        world.insert(record).unwrap()
    };
    let dynamite = item(skills::DYNAMITE_ARMED, 0x51);
    let plastic = item(skills::PLASTIC_ARMED, 0x52);
    let unknown = item(0x29, 0x53);
    assert_ne!(dynamite, 0x51, "a handle is not the map id");
    let mut queue = |delay, object, kind| world.queue.insert(100, delay, object, kind).unwrap();
    queue(50, dynamite, EventKind::Other(8));
    queue(40, plastic, EventKind::Other(11));
    queue(30, unknown, EventKind::Other(8));
    queue(30, 0, EventKind::Poison);
    queue(
        60,
        0xdead,
        EventKind::Radiation {
            level: 2,
            healing: 0,
        },
    );
    queue(
        61,
        0,
        EventKind::Radiation {
            level: 2,
            healing: 1,
        },
    );
    queue(
        70,
        0,
        EventKind::Script {
            script: 0x1234,
            parameter: 7,
        },
    );
    queue(
        80,
        0,
        EventKind::Drug {
            stats: [1, 2, 3],
            modifiers: [-1, 0, 2],
        },
    );
    queue(81, 0, EventKind::Withdrawal([1, 0x6b, 4]));
    queue(82, 0, EventKind::Withdrawal([0, 0x6b, 4]));
    queue(90, 0, EventKind::Midnight);
    queue(91, 0, EventKind::MapUpdate);
    queue(92, 0, EventKind::Knockout);
    queue(93, 0, EventKind::Other(7));
    queue(94, 0, EventKind::Other(9));
    queue(95, 0, EventKind::Other(10));
    queue(96, 0, EventKind::Other(42));

    let entries = queue_entries(&world);
    let lines: Vec<(u64, Option<u32>, &str)> = entries
        .iter()
        .map(|entry| (entry.due, entry.owner, entry.detail.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (130, Some(0x53), "explosive countdown"),
            (130, None, "poison"),
            (140, Some(0x52), "C4 countdown (premature)"),
            (150, Some(0x51), "dynamite countdown"),
            (160, None, "radiation onset: level 2"),
            (161, None, "radiation recovery: level 2"),
            (170, None, "script timer: 0x1234, parameter 7"),
            (180, None, "drug: stats [1, 2, 3] by [-1, 0, 2]"),
            (181, None, "withdrawal onset: drug 0x6b, perk 4"),
            (182, None, "withdrawal end: drug 0x6b, perk 4"),
            (190, None, "midnight"),
            (191, None, "map update"),
            (192, None, "knockout wake-up"),
            (193, None, "flare"),
            (194, None, "item drain"),
            (195, None, "sneak recheck"),
            (196, None, "event type 42"),
        ]
    );
    assert_eq!(entries[0].kind, EventKind::Other(8));
    assert_eq!(entries[3].id, 1);
    // Reading leaves the queue as it was.
    assert_eq!(world.queue.events().count(), entries.len());
}

#[test]
fn object_mutator_builds_commands_for_its_object() {
    use vault_core::debug::DebugCommand as Command;
    let target = ObjectMutator(0x51);
    assert_eq!(
        target.hp(12),
        Command::SetHitPoints {
            object: 0x51,
            value: 12
        }
    );
    assert_eq!(
        target.ap(3),
        Command::SetActionPoints {
            object: 0x51,
            value: 3
        }
    );
    assert_eq!(
        target.door(Some(true), None, Some(false)),
        Command::SetDoor {
            object: 0x51,
            open: Some(true),
            locked: None,
            jammed: Some(false)
        }
    );
    assert_eq!(
        target.item_count(0x29, 4),
        Command::SetItemCount {
            owner: 0x51,
            pid: 0x29,
            count: 4
        }
    );
    assert_eq!(
        target.fire(6, 1),
        Command::FireProc {
            object: 0x51,
            event: 6,
            fixed: 1
        }
    );
    assert_eq!(
        target.lvar(2, -1),
        Command::SetObjectLocalVar {
            object: 0x51,
            index: 2,
            value: -1
        }
    );
}
