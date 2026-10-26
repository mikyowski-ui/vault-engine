use super::*;
use std::collections::BTreeMap;
use vault_fixtures::vm::{self as fixture, op, push};

fn program() -> IntProgram {
    IntProgram::parse(&fixture::program()).unwrap()
}
fn fragment(code: &[u8]) -> Vm {
    Vm::fragment(program(), code).unwrap()
}
fn finish(vm: &mut Vm) -> Result<RunOutcome, VmError> {
    let mut host = BasicHost::default();
    for _ in 0..1000 {
        let result = vm.update(&mut host)?;
        if result != RunOutcome::Budget {
            return Ok(result);
        }
    }
    panic!("fixture did not terminate")
}
#[test]
fn the_fallout1_word_set_is_spec_004_and_005_exactly() {
    let set = WordSet::FALLOUT1;
    for word in 0x8000..=0x8155_u16 {
        assert!(set.accepts(word), "{word:#06x}");
    }
    // Past SPEC-005's last host word (where another game's words would
    // start) and below bit 15: refused.
    for word in [0x8156, 0x8183, 0x81ff, 0x7fff, 0x0000] {
        assert!(!set.accepts(word), "{word:#06x}");
    }
    // Every slot-1 push, whatever bits 10-14 hold.
    for slot in 0..32_u16 {
        assert!(set.accepts(0x8001 + slot * 0x0400));
    }
    assert!(!set.accepts(0x0001));
}

#[test]
fn spec004_complete_program_metadata_execution_and_mutations() {
    let bytes = fixture::program();
    assert_eq!(bytes.len(), 282);
    let parsed = program();
    assert_eq!(
        (
            parsed.identifier_origin,
            parsed.literal_origin,
            parsed.code_start
        ),
        (0x5e, 0x76, 0x84)
    );
    assert_eq!(parsed.procedures().len(), 2);
    assert_eq!(
        (
            parsed.procedures()[0].body,
            parsed.procedures()[1].body,
            parsed.procedures()[1].arguments
        ),
        (0x9c, 0xfc, 2)
    );
    assert_eq!(parsed.identifier(6), Some(b"main".as_slice()));
    assert_eq!(parsed.identifier(14), Some(b"sumxy".as_slice()));
    assert_eq!(parsed.literal(6), Some(b"hi".as_slice()));
    assert_eq!(parsed.find_procedure(b"MAIN", false), None);
    assert_eq!(parsed.find_procedure(b"MAIN", true), Some(0));
    assert_eq!(parsed.find_procedure(b"SuMxY", false), Some(1));
    for (offset, value, result) in [
        (0xae, 7, 99),
        (0xae, 1, -1),
        (0x5a, 3, 99),
        (0x4a, 0x20, 99),
    ] {
        let mut bytes = bytes.clone();
        bytes[offset..offset + 4].copy_from_slice(&i32::to_be_bytes(value));
        let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
        assert_eq!(finish(&mut vm).unwrap(), RunOutcome::Ended);
        assert_eq!(
            (vm.pc, vm.f, vm.g, vm.return_stack.byte_len()),
            (20, -1, 0, 0)
        );
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(result));
        assert_eq!(vm.value_stack.byte_len(), 6);
        assert!(!vm.status.contains(VmStatus::ERROR));
    }
}
#[test]
fn spec004_every_isolated_fragment() {
    for line in include_str!("../../../tests/fixtures/vm/fragments.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let parts: Vec<_> = line.split('|').map(str::trim).collect();
        let mut vm = fragment(&fixture::assets::hex(parts[1]));
        assert_eq!(
            finish(&mut vm).unwrap(),
            RunOutcome::FragmentEnd,
            "{}",
            parts[0]
        );
        assert_eq!(
            vm.value_stack.bytes(),
            fixture::assets::hex(parts[2]),
            "{}",
            parts[0]
        );
        if parts[0] == "static-string" {
            assert_eq!(vm.string(vm.value_stack.top().unwrap()).unwrap(), b"hi");
        }
        // SPEC-004 §4.2: `0xC801` with bit `0x0800` cleared is `0xC001`, so
        // the jump to 8 (the fragment's end) is accepted.
        if parts[0] == "masked-c801-jump" {
            assert_eq!(vm.pc, 8);
        }
    }
}
#[test]
fn spec004_errors_structural_boundaries_and_stack_capacity() {
    for line in include_str!("../../../tests/fixtures/vm/errors.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let parts: Vec<_> = line.split('|').map(str::trim).collect();
        let mut vm = fragment(&fixture::assets::hex(parts[1]));
        let error = finish(&mut vm).unwrap_err();
        assert!(match parts[2] {
            "underflow" => error == VmError::Underflow,
            "division" => error == VmError::DivisionByZero,
            "opcode" => matches!(error, VmError::Opcode(_)),
            // SPEC-004 §4.3: the `0x8401` push executes; the jump's masked
            // integer check then rejects it.
            "type" => error == VmError::Type(0x8401),
            _ => false,
        });
        if parts[2] == "division" {
            // Both operands are consumed before the zero-divisor error.
            assert_eq!(vm.value_stack.byte_len(), 0);
        }
        assert!(vm.status.contains(VmStatus::ENDED) && vm.status.contains(VmStatus::ERROR));
    }
    for (offset, value) in [
        (0x56, 0x200_u32),
        (0x76, 7),
        (42, u32::MAX),
        (0x2e, 7),
        (0x72, 0),
        (0x5a, u32::MAX),
    ] {
        let mut bytes = fixture::program();
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        assert!(IntProgram::parse(&bytes).is_err(), "mutation {offset:#x}");
    }
    let bytes = fixture::program();
    for end in 0..0x84 {
        assert!(IntProgram::parse(&bytes[..end]).is_err());
    }
    let mut vm = fragment(&push(0).repeat(341));
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(vm.value_stack.byte_len(), 2046);
    let mut vm = fragment(&push(0).repeat(342));
    assert_eq!(finish(&mut vm), Err(VmError::Overflow));
    assert_eq!(vm.value_stack.byte_len(), 2046);
    let mut vm = fragment(&[0xc0, 1, 0]);
    assert!(matches!(finish(&mut vm), Err(VmError::Truncated(_))));
}

#[test]
fn trailing_procedure_stubs_load_but_cannot_execute() {
    for field in [0x56, 0x52] {
        let mut bytes = fixture::program();
        let end = bytes.len() as u32;
        bytes[field..field + 4].copy_from_slice(&end.to_be_bytes());
        if field == 0x52 {
            bytes[0x4a..0x4e].copy_from_slice(&procedure_flags::CONDITIONAL.to_be_bytes());
        }
        let program = IntProgram::parse(&bytes).unwrap();
        let mut vm = Vm::new(program);
        let result = if field == 0x56 {
            finish(&mut vm).map(|_| ())
        } else {
            vm.poll_scheduled(&mut BasicHost::default(), 100)
                .map(|_| ())
        };
        assert_eq!(result, Err(VmError::Address(end)));
        assert!(vm.status.contains(VmStatus::ERROR));
        bytes[field..field + 4].copy_from_slice(&(end + 1).to_be_bytes());
        assert!(IntProgram::parse(&bytes).is_err());
    }
}
#[test]
fn arithmetic_types_strings_truthiness_and_undefined_overflow() {
    for (opcode, expected) in [
        (0x8033, 0),
        (0x8034, 1),
        (0x8035, 0),
        (0x8036, 1),
        (0x8037, 0),
        (0x8038, 1),
        (0x803a, 4),
        (0x803b, 21),
        (0x803c, 2),
        (0x803d, 1),
        (0x803e, 1),
        (0x803f, 1),
        (0x8040, 3),
        (0x8041, 7),
        (0x8042, 4),
    ] {
        let mut vm = fragment(&op(opcode));
        vm.value_stack.push(Value::integer(7)).unwrap();
        vm.value_stack.push(Value::integer(3)).unwrap();
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(expected));
    }
    let mut vm = fragment(&op(0x8039));
    let a = vm.intern(b"A").unwrap();
    vm.value_stack.push(a).unwrap();
    vm.value_stack.push(Value::float(1.5)).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(
        vm.string(vm.value_stack.top().unwrap()).unwrap(),
        b"A1.50000"
    );
    let mut vm = fragment(&op(0x8037));
    let two = vm.intern(b"2").unwrap();
    vm.value_stack.push(Value::integer(10)).unwrap();
    vm.value_stack.push(two).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(1));
    for (opcode, left_string, expected) in
        [(0x803e, false, 1), (0x803e, true, 0), (0x803f, false, 1)]
    {
        let mut vm = fragment(&op(opcode));
        let text = vm.intern(b"").unwrap();
        let zero = Value::float(-0.0);
        for value in if left_string {
            [text, zero]
        } else {
            [zero, text]
        } {
            vm.value_stack.push(value).unwrap();
        }
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(expected));
    }
    // SPEC-004 §3.5 safe rules: an operation that would consume its
    // operands and push nothing raises a type error, and the float-to-integer
    // conversion rejects both string tags.
    for opcode in [0x803a, 0x803b, 0x803c, 0x803d, 0x8040, 0x8041, 0x8042] {
        let mut vm = fragment(&op(opcode));
        let string = vm.intern(b"left").unwrap();
        vm.value_stack.push(string).unwrap();
        vm.value_stack.push(Value::integer(2)).unwrap();
        assert_eq!(finish(&mut vm), Err(VmError::Type(string.tag)));
    }
    let mut vm = fragment(&op(0x8044));
    let string = vm.intern(b"dynamic").unwrap();
    vm.value_stack.push(string).unwrap();
    assert_eq!(finish(&mut vm), Err(VmError::Type(string.tag)));
    // Integer results wrap modulo 2^32.
    for (opcode, a, b, expected) in [
        (0x803a, i32::MIN, 1, i32::MAX),
        (0x803b, i32::MAX, 2, -2),
        (0x803c, i32::MIN, -1, i32::MIN),
    ] {
        let mut vm = fragment(&op(opcode));
        for value in [a, b] {
            vm.value_stack.push(Value::integer(value)).unwrap();
        }
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(expected));
    }
    let mut vm = fragment(&op(0x8046));
    vm.value_stack.push(Value::integer(i32::MIN)).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(i32::MIN));
    // Out-of-range and NaN conversions give 80 00 00 00.
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2147483648.0] {
        let mut vm = fragment(&op(0x8044));
        vm.value_stack.push(Value::float(value)).unwrap();
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(i32::MIN));
    }
    let mut vm = fragment(&op(0x8040));
    vm.value_stack
        .push(Value {
            tag: Value::FLOAT,
            payload: 0x7fffffff,
        })
        .unwrap();
    vm.value_stack.push(Value::integer(-1)).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(i32::MIN));
    let mut vm = fragment(&op(0x803d));
    let value = vm.intern(b"nonnumeric").unwrap();
    vm.value_stack.push(value).unwrap();
    vm.value_stack.push(Value::float(0.0)).unwrap();
    assert_eq!(finish(&mut vm), Err(VmError::Type(value.tag)));
    // Nonfinite floats format as nan, inf, and -inf.
    for (float, expected) in [
        (f32::NAN, &b"Anan"[..]),
        (f32::INFINITY, b"Ainf"),
        (f32::NEG_INFINITY, b"A-inf"),
    ] {
        let mut vm = fragment(&op(0x8039));
        let left = vm.intern(b"A").unwrap();
        vm.value_stack.push(left).unwrap();
        vm.value_stack.push(Value::float(float)).unwrap();
        finish(&mut vm).unwrap();
        let top = vm.value_stack.top().unwrap();
        assert_eq!(vm.string(top).unwrap(), expected);
    }
}
#[test]
fn slots_transfers_and_frame_validation_preserve_tags() {
    let mut code = op(0x802c);
    code.extend(push(42));
    code.extend(push(0));
    code.extend(op(0x8012));
    code.extend(op(0x801b));
    code.extend(op(0x800d));
    code.extend(push(9));
    code.extend(op(0x800d));
    code.extend(op(0x8019));
    code.extend(op(0x800c));
    code.extend(op(0x801e));
    code.extend(op(0x8018));
    let mut vm = fragment(&code);
    finish(&mut vm).unwrap();
    assert_eq!(vm.g, 0);
    assert_eq!(vm.value_stack.byte_len(), 18);
    assert_eq!(vm.return_stack.byte_len(), 0);
    // Stores keep the stored value's tag; every slot index must be an
    // integer (SPEC-004 §3.1 safe rule).
    for opcode in [0x8013, 0x8031] {
        let mut vm = fragment(&op(opcode));
        vm.g = 0;
        vm.f = 0;
        for value in [Value::integer(0), Value::float(2.5), Value::integer(0)] {
            vm.value_stack.push(value).unwrap();
        }
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::float(2.5));
    }
    for opcode in [0x8012, 0x8013, 0x8031, 0x802b] {
        let mut vm = fragment(&op(opcode));
        vm.g = 0;
        vm.f = 0;
        for value in [
            Value::integer(0),
            Value::float(2.5),
            Value {
                tag: 0x8001,
                payload: 0,
            },
        ] {
            vm.value_stack.push(value).unwrap();
        }
        assert_eq!(finish(&mut vm), Err(VmError::Type(0x8001)));
    }
    let mut vm = fragment(&op(0x8032));
    vm.f = 0;
    vm.value_stack.push(Value::integer(1)).unwrap();
    vm.value_stack
        .push(Value {
            tag: 0x8001,
            payload: 0,
        })
        .unwrap();
    assert!(matches!(finish(&mut vm), Err(VmError::Type(0x8001))));
    let mut vm = fragment(&[push(0), op(0x8012)].concat());
    assert_eq!(finish(&mut vm), Err(VmError::Slot));
    let mut vm = fragment(&[push(-1), op(0x802b)].concat());
    assert_eq!(finish(&mut vm), Err(VmError::Slot));
    let mut vm = fragment(&[push(1), op(0x802d)].concat());
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(0xfc));
    let mut vm = fragment(&[push(1), push(3), op(0x8027)].concat());
    assert_eq!(finish(&mut vm), Err(VmError::Arguments));
}
#[test]
fn registries_copy_strings_and_procedure_names_are_case_insensitive() {
    let mut host = BasicHost::default();
    let mut vm = fragment(&[push(6), op(0x8016), op(0x8015)].concat());
    let value = vm.intern(b"exported").unwrap();
    vm.value_stack.push(value).unwrap();
    vm.value_stack.push(Value::integer(6)).unwrap();
    vm.run(&mut host, 100).unwrap();
    let mut other = fragment(&[push(6), op(0x8014)].concat());
    other.id = 2;
    other.intern(b"different").unwrap();
    other.run(&mut host, 100).unwrap();
    assert_eq!(
        other.string(other.value_stack.top().unwrap()).unwrap(),
        b"exported"
    );
    assert_eq!(other.value_stack.top().unwrap().tag, Value::DYNAMIC_STRING);
    assert!(host.export_variable(2, b"MAIN").is_err());
    let mut vm = fragment(&[push(2), push(1), op(0x8017)].concat());
    vm.run(&mut host, 100).unwrap();
    assert_eq!(
        host.import_procedure(b"SUMXY").unwrap(),
        ExportedProcedure {
            owner: 1,
            address: 0xfc,
            arguments: 2
        }
    );
    let mut vm = fragment(&op(0x8028));
    let name = vm.intern(b"SUMXY").unwrap();
    vm.value_stack.push(name).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(1));
    for event in [
        Lifecycle::Start,
        Lifecycle::Talk,
        Lifecycle::LookAt,
        Lifecycle::Description,
    ] {
        assert_eq!(program().lifecycle_entry(event).unwrap(), Some(1));
    }
    assert_eq!(
        program().lifecycle_entry(Lifecycle::MapEnter).unwrap(),
        None
    );
    assert_eq!(
        program().lifecycle_entry(Lifecycle::MapUpdate).unwrap(),
        None
    );
}
#[derive(Default)]
struct Host {
    basic: BasicHost,
    programs: BTreeMap<u32, Vm>,
    launches: Vec<(LaunchMode, Option<u32>, bool)>,
    extensions: Vec<u16>,
    purged: Vec<u32>,
}
impl VmHost for Host {
    fn now_ms(&self) -> u32 {
        self.basic.milliseconds
    }
    fn program_mut(&mut self, id: u32) -> Result<&mut Vm, VmError> {
        self.programs
            .get_mut(&id)
            .ok_or_else(|| VmError::Host("missing program".into()))
    }
    fn launch(
        &mut self,
        name: &[u8],
        mode: LaunchMode,
        parent: Option<u32>,
        critical: bool,
    ) -> Result<u32, VmError> {
        assert_eq!(name, b"hi");
        self.launches.push((mode, parent, critical));
        Ok(2)
    }
    fn extension(&mut self, opcode: u16, vm: &mut Vm) -> Result<(), VmError> {
        self.extensions.push(opcode);
        vm.value_stack.push(Value::integer(73))
    }
    fn purge(&mut self, id: u32) -> Result<(), VmError> {
        self.purged.push(id);
        Ok(())
    }
}
#[test]
fn all_context_return_variants_and_external_string_transfer() {
    for opcode in [
        0x801f, 0x8020, 0x8021, 0x8022, 0x8023, 0x8024, 0x8025, 0x8026,
    ] {
        let mut vm = fragment(&op(opcode));
        let external = matches!(opcode, 0x8022 | 0x8023 | 0x8024 | 0x8026);
        let preserve = matches!(opcode, 0x8024..=0x8026);
        let mut host = Host::default();
        let mut caller = Vm::new(program());
        caller.id = 2;
        caller.return_stack.push(Value::integer(18)).unwrap();
        host.programs.insert(2, caller);
        for value in [0x10000, 9, 17] {
            vm.value_stack.push(Value::integer(value)).unwrap();
        }
        if preserve {
            vm.value_stack
                .push(Value {
                    tag: Value::STATIC_STRING,
                    payload: 6,
                })
                .unwrap();
        }
        if opcode != 0x801f {
            vm.return_stack.push(Value::integer(2)).unwrap();
        }
        if external {
            for value in [VmStatus::CRITICAL as i32, 7, 2] {
                vm.return_stack.push(Value::integer(value)).unwrap();
            }
        }
        let result = vm.run(&mut host, 100).unwrap();
        assert_eq!((vm.window, vm.wait_predicate), (17, 9));
        assert!(!vm.status.contains(VmStatus::YIELD));
        assert_eq!(
            result,
            if matches!(opcode, 0x8021 | 0x8023 | 0x8025 | 0x8026) {
                RunOutcome::Yielded
            } else {
                RunOutcome::FragmentEnd
            }
        );
        if opcode == 0x8024 {
            let caller = &host.programs[&2];
            assert_eq!(caller.pc, 18);
            assert_eq!(
                caller.string(caller.value_stack.top().unwrap()).unwrap(),
                b"hi"
            );
            assert_eq!(caller.value_stack.top().unwrap().tag, Value::DYNAMIC_STRING);
        } else if preserve {
            assert_eq!(vm.value_stack.top().unwrap().tag, Value::STATIC_STRING);
        }
        if external {
            assert_eq!(host.programs[&2].wait_predicate, 7);
        }
    }
    let mut vm = fragment(&op(0x801d));
    vm.return_stack
        .push(Value::float(f32::from_bits(2)))
        .unwrap();
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::Yielded);
    assert_eq!(vm.pc, 2); // Return addresses ignore their tags.
}
#[test]
fn program_control_host_extensions_wait_quirk_and_instruction_limits() {
    for (opcode, outcome, mode) in [
        (0x8008, RunOutcome::Blocked, LaunchMode::Attached),
        (0x8009, RunOutcome::Ended, LaunchMode::Replace),
        (0x800a, RunOutcome::Blocked, LaunchMode::Spawn),
        (0x800b, RunOutcome::FragmentEnd, LaunchMode::Independent),
    ] {
        let mut vm = fragment(&[fixture::assets::hex("90 01 00 00 00 06"), op(opcode)].concat());
        vm.status.0 |= VmStatus::CRITICAL;
        let mut host = Host::default();
        assert_eq!(vm.run(&mut host, 100).unwrap(), outcome);
        assert_eq!(host.launches[0].0, mode);
        assert_eq!(host.launches[0].2, mode == LaunchMode::Spawn);
    }
    for opcode in [0x800e, 0x800f] {
        let mut vm = fragment(&op(opcode));
        vm.parent = Some(2);
        let mut host = Host::default();
        let mut parent = Vm::new(program());
        parent.id = 2;
        parent.status.0 = VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED;
        parent.child = Some(1);
        host.programs.insert(2, parent);
        vm.run(&mut host, 100).unwrap();
        assert_eq!(host.programs[&2].status.0, 0);
        assert!(host.programs[&2].child.is_none());
        assert_eq!(host.purged.len(), usize::from(opcode == 0x800e));
    }
    let mut vm = fragment(&[op(0x804c), op(0x8155)].concat());
    let mut host = Host::default();
    vm.run(&mut host, 100).unwrap();
    assert_eq!(host.extensions, [0x804c, 0x8155]);
    let mut vm = fragment(&op(0x804c));
    assert!(matches!(finish(&mut vm), Err(VmError::Opcode(0x804c))));
    let mut vm = fragment(&op(0x8011));
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::Stopped);
    let mut vm = fragment(&[push(10), op(0x8047), op(0x8000)].concat());
    let mut host = BasicHost::default();
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::FragmentEnd); // Positive wait completes immediately.
    let mut vm = fragment(&[push(10), op(0x8047), op(0x8000)].concat());
    vm.run(&mut host, 2).unwrap();
    vm.status.0 |= VmStatus::WAITING;
    host.milliseconds = 11;
    // A wait first checked after its deadline is released (§3.3 safe rule).
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::FragmentEnd);
    let mut vm = fragment(&[op(0x804a), op(0x8000), op(0x804b)].concat());
    assert_eq!(vm.run(&mut host, 1).unwrap(), RunOutcome::FragmentEnd);
    let mut vm = fragment(&[op(0x8002), push(2), op(0x8004)].concat());
    vm.hard_instruction_limit = 20;
    assert_eq!(vm.run(&mut host, 1), Err(VmError::InstructionLimit));
}
#[test]
fn scheduling_mutates_image_uses_strict_deadlines_and_conditional_precedence() {
    let mut host = BasicHost::default();
    host.milliseconds = 100;
    let mut vm = fragment(&[push(2), push(1), op(0x8006)].concat());
    vm.run(&mut host, 100).unwrap();
    assert_eq!(
        (
            vm.program.procedures()[1].flags,
            vm.program.procedures()[1].deadline
        ),
        (1, 2100)
    );
    assert_eq!(&vm.program.image()[0x4e..0x52], &2100_u32.to_be_bytes());
    let (bytes, condition) = fixture::scheduled_program(1);
    let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
    vm.program.schedule(1, procedure_flags::TIMED, 2100, 0);
    host.milliseconds = 2100;
    assert_eq!(vm.poll_scheduled(&mut host, 100).unwrap(), None);
    host.milliseconds = 2101;
    assert_eq!(vm.poll_scheduled(&mut host, 100).unwrap(), Some(1));
    assert_eq!(vm.program.procedures()[1].flags, 0);
    assert_eq!(vm.run(&mut host, 9).unwrap(), RunOutcome::Budget);
    assert_eq!(
        (vm.pc, vm.return_stack.byte_len(), vm.value_stack.byte_len()),
        (0, 0, 0)
    );
    for predicate in [0, 1] {
        let (bytes, condition2) = fixture::scheduled_program(predicate);
        assert_eq!(condition, condition2);
        let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
        vm.program.schedule(
            1,
            procedure_flags::TIMED | procedure_flags::CONDITIONAL | procedure_flags::CRITICAL,
            u32::MAX,
            condition,
        );
        let result = vm.poll_scheduled(&mut host, 100).unwrap();
        assert_eq!(result, (predicate != 0).then_some(1));
        assert!(host.fetch_variable(b"main").is_ok()); // Conditional execution had a host side effect.
        if predicate != 0 {
            assert!(vm.status.contains(VmStatus::CRITICAL));
        }
    }
    let mut vm = fragment(&[push(1), op(0x8048), op(0x8049)].concat());
    vm.program.schedule(1, u32::MAX, 123, condition);
    vm.run(&mut host, 100).unwrap();
    assert!(
        vm.program
            .procedures()
            .iter()
            .all(|p| p.flags == 0 && p.deadline == 0 && p.condition == 0)
    );
}
#[test]
fn host_calls_and_undefined_imported_argument_abi() {
    let (bytes, _) = fixture::scheduled_program(1);
    let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
    vm.program.schedule(1, procedure_flags::CRITICAL, 0, 0);
    vm.prepare_host_call(1, true).unwrap();
    assert!(!vm.status.contains(VmStatus::CRITICAL)); // Synchronous path does not apply procedure critical.
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::Yielded);
    assert_eq!(
        (
            vm.pc,
            vm.f,
            vm.value_stack.byte_len(),
            vm.return_stack.byte_len()
        ),
        (0, -1, 0, 0)
    );
    let mut host = BasicHost::default();
    host.export_procedure(
        b"sumxy",
        ExportedProcedure {
            owner: 2,
            address: 0xfc,
            arguments: 2,
        },
    )
    .unwrap();
    let mut vm = fragment(&[push(1), op(0x8005)].concat());
    vm.program.schedule(1, procedure_flags::IMPORT, 0, 0);
    assert_eq!(vm.run(&mut host, 100), Err(VmError::ImportedAbi));
    let mut vm = Vm::new(program());
    vm.program.schedule(1, procedure_flags::IMPORT, 0, 0);
    assert!(matches!(
        vm.prepare_host_call(1, true),
        Err(VmError::Host(_))
    ));
    assert_eq!(
        (vm.value_stack.byte_len(), vm.return_stack.byte_len()),
        (0, 0)
    );
}

#[test]
fn generic_push_conditional_registration_complement_and_external_preparation() {
    let mut vm = fragment(&fixture::assets::hex("80 01 12 34 56 78"));
    finish(&mut vm).unwrap();
    assert_eq!(
        vm.value_stack.top().unwrap(),
        Value {
            tag: 0x8001,
            payload: 0x12345678
        }
    );
    let mut vm = fragment(&[push(6), push(1), op(0x8007)].concat());
    vm.program.schedule(1, procedure_flags::EXPORT, 123, 0);
    finish(&mut vm).unwrap();
    assert_eq!(
        (
            vm.program.procedures()[1].flags,
            vm.program.procedures()[1].deadline,
            vm.program.procedures()[1].condition
        ),
        (10, 123, 6)
    );
    let mut vm = fragment(&op(0x8043));
    vm.value_stack.push(Value::float(1.0)).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(
        vm.value_stack.top().unwrap(),
        Value::integer(!(1.0_f32.to_bits() as i32))
    );
    let (bytes, _) = fixture::scheduled_program(1);
    let mut callee = Vm::new(IntProgram::parse(&bytes).unwrap());
    callee.id = 3;
    let mut caller = Vm::new(program());
    caller.id = 2;
    callee.prepare_external_call(&mut caller, 1, true).unwrap();
    assert!(caller.status.contains(VmStatus::ATTACHED));
    let mut host = Host::default();
    host.programs.insert(2, caller);
    assert_eq!(callee.run(&mut host, 100).unwrap(), RunOutcome::Yielded);
    assert_eq!(host.programs[&2].status.0, 0);
    assert_eq!(
        (
            callee.pc,
            callee.f,
            callee.value_stack.byte_len(),
            callee.return_stack.byte_len()
        ),
        (0, -1, 0, 0)
    );
}

#[test]
fn string_tagged_identifier_offsets_are_bounded_by_their_consumer() {
    // Offset 14 names sumxy in I, but is not a literal in T.
    let reference = fixture::assets::hex("90 01 00 00 00 0E");
    let mut vm = fragment(
        &[
            reference.clone(),
            op(0x8016),
            push(42),
            reference.clone(),
            op(0x8015),
            reference.clone(),
            op(0x8014),
        ]
        .concat(),
    );
    vm.run(&mut BasicHost::default(), 100).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(42));
    let mut vm = fragment(&[reference, push(1), op(0x8039)].concat());
    assert_eq!(finish(&mut vm), Err(VmError::StringReference(14)));
    let mut bytes = fixture::program();
    bytes[0x86..0x8c].copy_from_slice(&[0x90, 0x01, 0, 0, 0, 14]);
    let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::Ended);
    assert_eq!(vm.value_stack.top().unwrap(), Value::integer(99));
}

#[test]
fn bounded_loader_and_instruction_mutation_smoke() {
    let original = fixture::program();
    let mut state = 0x81002_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for _ in 0..256 {
        let mut bytes = original.clone();
        let at = next() as usize % bytes.len();
        bytes[at] = next() as u8;
        if let Ok(program) = IntProgram::parse(&bytes) {
            let mut vm = Vm::new(program);
            vm.hard_instruction_limit = 64;
            let _ = vm.run(&mut BasicHost::default(), 16);
        }
    }
    for length in 0..128 {
        let bytes: Vec<_> = (0..length).map(|_| next() as u8).collect();
        let _ = IntProgram::parse(&bytes);
        let mut vm = fragment(&bytes);
        vm.hard_instruction_limit = 32;
        let _ = vm.run(&mut BasicHost::default(), 8);
    }
}

#[test]
fn spec005_export_ownership_reset_capacity_and_cleanup() {
    let mut host = BasicHost::default();
    host.register_program(1, b"scripts/one.int");
    host.register_program(2, b"SCRIPTS/ONE.INT");
    host.register_program(3, b"scripts/other.int");
    host.export_variable(1, b"Name").unwrap();
    host.store_variable(b"name", ExternalValue::Value(Value::integer(9)))
        .unwrap();
    host.export_variable(2, b"NAME").unwrap();
    assert_eq!(
        host.fetch_variable(b"name").unwrap(),
        ExternalValue::Value(Value::integer(0))
    );
    assert!(host.export_variable(3, b"name").is_err());
    host.export_procedure(
        b"name",
        ExportedProcedure {
            owner: 1,
            address: 0,
            arguments: 0,
        },
    )
    .unwrap();
    host.purge(1).unwrap();
    assert!(host.fetch_variable(b"name").is_ok());
    assert!(host.import_procedure(b"name").is_err());
    for index in 1..1013 {
        host.export_variable(2, format!("var{index}").as_bytes())
            .unwrap();
    }
    assert!(host.export_variable(2, b"overflow").is_err());
    host.export_variable(2, b"name").unwrap(); // Reset still works when the registry is full.
}

#[test]
fn spec005_long_export_names_use_the_retained_31_byte_key() {
    let mut host = BasicHost::default();
    host.register_program(1, b"owner.int");
    let long = b"a_rather_long_exported_variable_name";
    assert!(long.len() > 31);
    host.export_variable(1, long).unwrap();
    // Fetching/storing with the same long name reaches the stored entry.
    assert!(host.fetch_variable(long).is_ok());
    host.store_variable(long, ExternalValue::Value(Value::integer(9)))
        .unwrap();
    assert_eq!(
        host.fetch_variable(b"A_RATHER_LONG_EXPORTED_VARIABLE_NAME")
            .unwrap(),
        ExternalValue::Value(Value::integer(9))
    );
    // Names sharing the retained prefix collide, the documented hazard.
    assert!(
        host.fetch_variable(b"a_rather_long_exported_variable_other")
            .is_ok()
    );
    assert!(
        host.fetch_variable(b"a_rather_long_exported_variabl")
            .is_err()
    );
}
#[test]
fn scheduling_faults_wrapping_remainder_and_detach_without_a_parent() {
    // SPEC-004 §3.8 safe rule: a fired procedure whose call cannot be
    // prepared faults the VM (ended/error, `last_error`) instead of
    // vanishing; its schedule was consumed by the firing.
    // (An isolated fragment cannot take a host call.)
    let mut vm = fragment(&op(0x8000));
    vm.program.schedule(1, procedure_flags::TIMED, 0, 0);
    let mut host = BasicHost::default();
    host.milliseconds = 1;
    let error = vm.poll_scheduled(&mut host, 100).unwrap_err();
    assert!(matches!(error, VmError::Host(_)));
    assert!(vm.status.contains(VmStatus::ERROR) && vm.status.contains(VmStatus::ENDED));
    assert_eq!(vm.last_error, Some(error));
    assert_eq!(vm.program.procedures()[1].flags, 0);
    // §3.5 [May-improve]: `m / -1` wraps to `m`, so `m % -1` is 0.
    for (opcode, expected) in [(0x803c, i32::MIN), (0x803d, 0)] {
        let mut vm = fragment(&op(opcode));
        vm.value_stack.push(Value::integer(i32::MIN)).unwrap();
        vm.value_stack.push(Value::integer(-1)).unwrap();
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::integer(expected));
    }
    // 0x8007 rejects a condition address at the program's code end (not an
    // instruction) when scheduled, not when the condition first runs.
    let mut bytes = fixture::program();
    bytes.truncate(0xfc);
    let end = 0xfc + 14;
    for instruction in [push(end as i32), push(1), op(0x8007)] {
        bytes.extend(instruction);
    }
    assert_eq!(bytes.len(), end);
    let mut vm = Vm::new(IntProgram::parse(&bytes).unwrap());
    vm.program.schedule(1, procedure_flags::TIMED, 0, 0);
    assert_eq!(vm.poll_scheduled(&mut host, 100).unwrap(), Some(1));
    assert_eq!(vm.run(&mut host, 100), Err(VmError::Address(end as u32)));
    // 0x800F with a parent the host no longer has drops the link; no fault.
    let mut vm = fragment(&op(0x800f));
    vm.parent = Some(9);
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(vm.parent, None);
    assert!(!vm.status.contains(VmStatus::ERROR));
}
#[test]
fn spec005_registry_lifetime_full_clear_and_per_program_procedures() {
    let mut host = BasicHost::default();
    host.register_program(1, b"scripts/v13cave.int");
    host.register_program(2, b"scripts/vaultent.int");
    host.export_variable(1, b"Ian_ptr").unwrap();
    host.store_variable(b"ian_ptr", ExternalValue::Value(Value::integer(5)))
        .unwrap();
    host.export_procedure(
        b"helper",
        ExportedProcedure {
            owner: 1,
            address: 0,
            arguments: 0,
        },
    )
    .unwrap();
    // Ending the program removes its procedure entries only: the variable
    // and its value survive, and another file still conflicts on this map.
    host.purge(1).unwrap();
    assert!(host.import_procedure(b"helper").is_err());
    assert_eq!(
        host.fetch_variable(b"IAN_PTR").unwrap(),
        ExternalValue::Value(Value::integer(5))
    );
    assert!(matches!(
        host.export_variable(2, b"Ian_ptr"),
        Err(VmError::Symbol(_))
    ));
    // The map change's full clear empties both tables, so the next map's
    // script becomes the new owner with value I zero.
    host.clear();
    assert!(host.fetch_variable(b"Ian_ptr").is_err());
    host.register_program(2, b"scripts/vaultent.int");
    host.export_variable(2, b"Ian_ptr").unwrap();
    assert_eq!(
        host.fetch_variable(b"Ian_ptr").unwrap(),
        ExternalValue::Value(Value::integer(0))
    );
}
#[test]
fn spec005_replace_removes_old_procedures_before_the_replacement_starts() {
    #[derive(Default)]
    struct Replacing {
        basic: BasicHost,
        removed_before_launch: Option<bool>,
    }
    impl VmHost for Replacing {
        fn launch(
            &mut self,
            _name: &[u8],
            _mode: LaunchMode,
            _parent: Option<u32>,
            _critical: bool,
        ) -> Result<u32, VmError> {
            self.removed_before_launch = Some(self.basic.import_procedure(b"shared").is_err());
            Ok(2)
        }
        fn remove_procedures(&mut self, owner: u32) -> Result<(), VmError> {
            self.basic.remove_procedures(owner)
        }
    }
    let mut host = Replacing::default();
    host.basic.register_program(1, b"scripts/old.int");
    host.basic.export_variable(1, b"kept").unwrap();
    host.basic
        .export_procedure(
            b"shared",
            ExportedProcedure {
                owner: 1,
                address: 0,
                arguments: 0,
            },
        )
        .unwrap();
    let mut vm = fragment(&[fixture::assets::hex("90 01 00 00 00 06"), op(0x8009)].concat());
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::Ended);
    assert_eq!(host.removed_before_launch, Some(true));
    assert!(host.basic.fetch_variable(b"kept").is_ok());
}
#[test]
fn critical_instructions_do_not_count_against_the_budget_and_step_runs_one() {
    // Enter critical, three no-ops, leave critical (uncounted), then two
    // counted no-ops: a budget of 3 stops before the last one.
    let code = [
        op(0x8002),
        op(0x8000),
        op(0x8000),
        op(0x8000),
        op(0x8003),
        op(0x8000),
        op(0x8000),
        op(0x8000),
    ]
    .concat();
    let mut vm = fragment(&code);
    let mut host = BasicHost::default();
    assert_eq!(vm.run(&mut host, 3).unwrap(), RunOutcome::Budget);
    assert_eq!(vm.pc, 14);
    let mut vm = fragment(&code);
    assert_eq!(vm.step(&mut host).unwrap(), RunOutcome::Budget);
    assert_eq!(vm.pc, 2);
    assert!(vm.status.contains(VmStatus::CRITICAL));
    assert_eq!(vm.step(&mut host).unwrap(), RunOutcome::Budget);
    assert_eq!(vm.pc, 4);
}

/// A host with the enhanced opcode range switched on.
struct EnhancedHost;
impl VmHost for EnhancedHost {
    fn enhanced_opcodes(&self) -> bool {
        true
    }
}
#[test]
fn enhanced_range_is_rejected_like_any_unknown_word_unless_the_host_enables_it() {
    use crate::extensions::{ENGINE_API_VERSION, ENHANCED_RANGE, enhanced_opcode};
    // Vanilla: 0x8200 fails exactly as an undefined word outside every
    // range does, with the same error and the same faulted state.
    let outcome = |word: u16| {
        let mut vm = fragment(&op(word));
        let mut host = BasicHost::default();
        let result = vm.run(&mut host, 10);
        (result, vm.status, vm.pc, vm.value_stack.byte_len())
    };
    for word in [0x8200, 0x8201, 0x8210, 0x8211, 0x8212, 0x8213, 0x82ff] {
        let (result, status, pc, depth) = outcome(word);
        let (other, other_status, other_pc, other_depth) = outcome(0x8156);
        assert_eq!(result, Err(VmError::Opcode(word)));
        assert_eq!(other, Err(VmError::Opcode(0x8156)));
        assert_eq!((status, pc, depth), (other_status, other_pc, other_depth));
    }
    // The default `VmHost` gate is off.
    assert!(!BasicHost::default().enhanced_opcodes());
    // A vanilla parse refuses the word at load time, like any other
    // undefined word.
    let mut image = fixture::program();
    let code = IntProgram::parse(&image).unwrap().code_start;
    for (word, name) in [(0x8200_u16, "0x8200"), (0x8156, "0x8156")] {
        image[code..code + 2].copy_from_slice(&word.to_be_bytes());
        let error = IntProgram::parse(&image).unwrap_err().to_string();
        assert!(
            error.contains(&format!("invalid instruction {name}")),
            "{error}"
        );
    }
    image[code..code + 2].copy_from_slice(&0x8200_u16.to_be_bytes());
    assert!(IntProgram::parse_enhanced(&image).unwrap().enhanced());
    assert!(IntProgram::parse_enhanced(&image[..]).is_ok());
    // An enhanced program on a vanilla host still fails as unknown.
    let enhanced = || IntProgram::parse_enhanced(&fixture::program()).unwrap();
    let mut vm = Vm::fragment(enhanced(), &op(0x8200)).unwrap();
    assert_eq!(
        vm.run(&mut BasicHost::default(), 10),
        Err(VmError::Opcode(0x8200))
    );
    // Enhanced: engine_api_version pushes the integer version.
    let mut vm = Vm::fragment(enhanced(), &op(0x8200)).unwrap();
    let mut host = EnhancedHost;
    assert_eq!(vm.run(&mut host, 10).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(
        vm.value_stack.top().unwrap(),
        Value::integer(ENGINE_API_VERSION)
    );
    // An unassigned word in the range is still unknown when enabled, and
    // words outside the range never reach the enhanced table.
    let mut vm = Vm::fragment(enhanced(), &op(0x8201)).unwrap();
    assert_eq!(vm.run(&mut host, 10), Err(VmError::Opcode(0x8201)));
    let mut vm = Vm::fragment(enhanced(), &op(0x8156)).unwrap();
    assert_eq!(vm.run(&mut host, 10), Err(VmError::Opcode(0x8156)));
    assert_eq!(*ENHANCED_RANGE.start(), 0x8200);
    assert_eq!(enhanced_opcode(0x8200).unwrap().name, "engine_api_version");
    assert!(enhanced_opcode(0x8155).is_none());
}
#[test]
fn enhanced_camera_words_pop_their_operands_and_push_nothing() {
    use crate::extensions::{
        ENGINE_API_VERSION, ENHANCED, PushKind, decode_enhanced, enhanced_opcode,
    };
    assert_eq!(ENGINE_API_VERSION, 2);
    // The table is in ascending word order, as the lookup requires.
    assert!(ENHANCED.windows(2).all(|pair| pair[0].word < pair[1].word));
    for (word, name, operands) in [
        (0x8210, "op_cam_pan_to", 2),
        (0x8211, "op_cam_set_zoom", 2),
        (0x8212, "op_cam_shake", 2),
        (0x8213, "op_cam_reset", 1),
    ] {
        let spec = enhanced_opcode(word).unwrap();
        assert_eq!((spec.name, spec.operands.len()), (name, operands));
        assert_eq!(spec.push, PushKind::None);
    }
    for word in [0x820f, 0x8214] {
        assert!(enhanced_opcode(word).is_none());
    }
    let enhanced = || IntProgram::parse_enhanced(&fixture::program()).unwrap();
    // A host without a view drops each request: the operands are consumed
    // and the sentinel below them stays on top.
    let code = [
        push(77),
        push(20100),
        push(1500),
        op(0x8210),
        push(200),
        push(1000),
        op(0x8211),
        push(6),
        push(800),
        op(0x8212),
        push(1500),
        op(0x8213),
    ]
    .concat();
    let mut vm = Vm::fragment(enhanced(), &code).unwrap();
    assert_eq!(
        vm.run(&mut EnhancedHost, 100).unwrap(),
        RunOutcome::FragmentEnd
    );
    assert_eq!(vm.value_stack.pop().unwrap(), Value::integer(77));
    assert!(vm.value_stack.pop().is_err());
    // Pop order: the last operand is `args[0]`.
    let mut vm = Vm::fragment(enhanced(), &[push(20100), push(1500)].concat()).unwrap();
    vm.run(&mut EnhancedHost, 100).unwrap();
    let call = decode_enhanced(&mut vm, 0x8210).unwrap();
    assert_eq!((call.int(0), call.int(1)), (1500, 20100));
    // The operands are integers; anything else is the usual type error, and
    // a missing operand the usual underflow.
    let mut vm = Vm::fragment(enhanced(), &op(0x8213)).unwrap();
    vm.value_stack.push(Value::float(1.5)).unwrap();
    assert_eq!(
        vm.run(&mut EnhancedHost, 100),
        Err(VmError::Type(Value::FLOAT))
    );
    let mut vm = Vm::fragment(enhanced(), &[push(1), op(0x8213), op(0x8213)].concat()).unwrap();
    assert_eq!(vm.run(&mut EnhancedHost, 100), Err(VmError::Underflow));
    // Unassigned neighbours stay unknown when enabled.
    let mut vm = Vm::fragment(enhanced(), &op(0x8214)).unwrap();
    assert_eq!(vm.run(&mut EnhancedHost, 10), Err(VmError::Opcode(0x8214)));
}

/// A host whose words are the Fallout 2 profile's rows: each defined word
/// pops and checks its operands and pushes I 0 when its row pushes; a
/// reserved word is the undefined-instruction error (SPEC-017 §2.1).
struct Fallout2Host;
impl VmHost for Fallout2Host {
    fn enhanced_opcodes(&self) -> bool {
        true
    }
    fn extension(&mut self, opcode: u16, vm: &mut Vm) -> Result<(), VmError> {
        let call = crate::extensions::decode_in(vm, &WordSet::FALLOUT2, opcode)?;
        if call.spec.push != crate::extensions::PushKind::None {
            vm.value_stack.push(Value::integer(0))?;
        }
        Ok(())
    }
}
/// The SPEC-004 fixture with its first initial-stream word replaced.
fn with_word(word: u16) -> Vec<u8> {
    let mut image = fixture::program();
    let code = IntProgram::parse(&image).unwrap().code_start;
    image[code..code + 2].copy_from_slice(&word.to_be_bytes());
    image
}
#[test]
fn spec017_vector_a_word_sets_at_load() {
    // (word, fallout1 loads, fallout2 loads, enhanced loads under both)
    for (word, fallout1, fallout2, enhanced) in [
        (0x8155_u16, true, true, true),
        (0x8156, false, true, true),
        (0x8183, false, true, true),
        (0x8184, false, false, false),
        (0x81ff, false, false, false),
        (0x8200, false, false, true),
    ] {
        let image = with_word(word);
        let loads =
            |words: &WordSet, enhanced: bool| IntProgram::parse_in(&image, words, enhanced).is_ok();
        assert_eq!(loads(&WordSet::FALLOUT1, false), fallout1, "{word:#06x}");
        assert_eq!(loads(&WordSet::FALLOUT2, false), fallout2, "{word:#06x}");
        // The enhanced preset adds only its own range, under either set.
        assert_eq!(
            loads(&WordSet::FALLOUT1, true),
            fallout1 || enhanced && word >= 0x8200,
            "{word:#06x}"
        );
        assert_eq!(loads(&WordSet::FALLOUT2, true), enhanced, "{word:#06x}");
    }
    // The Fallout 1 entry points are unchanged.
    assert_eq!(
        IntProgram::parse(&with_word(0x8156)).is_ok(),
        IntProgram::parse_in(&with_word(0x8156), &WordSet::FALLOUT1, false).is_ok()
    );
    assert_eq!(WordSet::FALLOUT2.words.len(), 388);
    assert!(WordSet::FALLOUT2.fallout2() && !WordSet::FALLOUT1.fallout2());
    for slot in 0..32_u16 {
        assert!(WordSet::FALLOUT2.accepts(0x8001 + slot * 0x0400));
    }
    // Bits 10-14 set on a non-push word stay refused.
    assert!(!WordSet::FALLOUT2.accepts(0xc039));
    let program = IntProgram::parse_in(&with_word(0x8156), &WordSet::FALLOUT2, false).unwrap();
    assert!(program.words().fallout2());
}
#[test]
fn spec017_vector_b_a_reserved_word_fails_closed() {
    for word in [0x815a_u16, 0x8183] {
        let code = [push(5), op(word)].concat();
        let program = IntProgram::parse_in(&fixture::program(), &WordSet::FALLOUT2, false).unwrap();
        let mut vm = Vm::fragment(program, &code).unwrap();
        assert_eq!(vm.run(&mut Fallout2Host, 10), Err(VmError::Opcode(word)));
        // Nothing was popped: the value stack still holds I 5.
        assert_eq!(vm.value_stack.bytes(), [0, 0, 0, 5, 0xc0, 0x01]);
        assert_eq!(vm.status.0 & 0x000f, VmStatus::ENDED | VmStatus::ERROR);
        // Under Fallout 1 the word is refused at load (and at fetch).
        assert!(IntProgram::parse(&with_word(word)).is_err());
        let mut vm = fragment(&code);
        assert_eq!(vm.run(&mut Fallout2Host, 10), Err(VmError::Opcode(word)));
    }
    // A defined Fallout 2 word runs; the same word on a Fallout 1 program
    // is refused before it reaches the host.
    let code = [push(1), push(13), push(0), op(0x8156)].concat();
    let program = IntProgram::parse_in(&fixture::program(), &WordSet::FALLOUT2, false).unwrap();
    let mut vm = Vm::fragment(program, &code).unwrap();
    assert_eq!(vm.run(&mut Fallout2Host, 10), Ok(RunOutcome::FragmentEnd));
    assert_eq!(vm.value_stack.bytes(), [0, 0, 0, 0, 0xc0, 0x01]);
    let mut vm = fragment(&code);
    assert_eq!(vm.run(&mut Fallout2Host, 10), Err(VmError::Opcode(0x8156)));
}
#[test]
fn spec017_fallout2_rows_amend_spec005_only_where_section_3_2_says() {
    use crate::extensions::{FALLOUT2, PushKind, Rule, opcode, opcode_in};
    let reassigned = [0x80b2_u16, 0x80cf, 0x80e1, 0x80e2, 0x80e5, 0x80e6, 0x8109];
    for word in 0x804c..=0x8155_u16 {
        let fallout1 = opcode_in(&WordSet::FALLOUT1, word).unwrap();
        assert_eq!(fallout1.name, opcode(word).unwrap().name);
        let fallout2 = opcode_in(&WordSet::FALLOUT2, word).unwrap();
        assert_eq!(
            fallout1.name == fallout2.name,
            !reassigned.contains(&word),
            "{word:#06x}"
        );
    }
    let row = |word| opcode_in(&WordSet::FALLOUT2, word).unwrap();
    let shape = |word| {
        let spec = row(word);
        (spec.name, spec.operands.len(), spec.push != PushKind::None)
    };
    assert_eq!(shape(0x80b2), ("mark_area_known", 3, false));
    assert_eq!(shape(0x80cf), ("tile_in_tile_rect", 5, true));
    assert_eq!(shape(0x80e1), ("metarule3", 4, true));
    assert_eq!(shape(0x80e2), ("set_map_music", 2, false));
    assert_eq!(shape(0x80e5), ("wm_area_set_pos", 3, false));
    assert_eq!(shape(0x80e6), ("set_exit_grids", 5, false));
    assert_eq!(shape(0x8109), ("unnamed", 3, true));
    assert_eq!(
        opcode(0x8109).unwrap().operands.len(),
        0,
        "Fallout 1's town_map"
    );
    assert_eq!(
        row(0x8109).operands,
        [Rule::Unchecked, Rule::Integer, Rule::Unchecked]
    );
    assert_eq!(shape(0x8156), ("inven_cmds", 3, true));
    assert_eq!(shape(0x8157), ("reg_anim_change_fid", 3, false));
    assert_eq!(shape(0x8158), ("reg_anim_take_out", 3, false));
    assert_eq!(shape(0x8159), ("reg_anim_turn_towards", 3, false));
    // metarule3 checks only its sub-function; set_map_music's track is S.
    assert_eq!(
        row(0x80e1).operands,
        [
            Rule::Unchecked,
            Rule::Unchecked,
            Rule::Unchecked,
            Rule::Integer
        ]
    );
    assert_eq!(row(0x80e2).operands, [Rule::String, Rule::Integer]);
    for word in 0x815a..=0x8183_u16 {
        assert!(opcode_in(&WordSet::FALLOUT2, word).is_none());
    }
    for word in [0x8156_u16, 0x8184, 0x8200] {
        assert!(opcode_in(&WordSet::FALLOUT1, word).is_none());
    }
    assert!(FALLOUT2.windows(2).all(|pair| pair[0].word < pair[1].word));
}
