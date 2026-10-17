use super::*;
use std::collections::BTreeMap;
#[allow(dead_code)]
#[path = "../../../tests/fixtures/vm/support.rs"]
mod fixture;
use fixture::{op, push};

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
            _ => false,
        });
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
    for opcode in [0x803a, 0x803b, 0x803c, 0x803d, 0x8040, 0x8041, 0x8042] {
        let mut vm = fragment(&op(opcode));
        let string = vm.intern(b"left").unwrap();
        vm.value_stack.push(string).unwrap();
        vm.value_stack.push(Value::integer(2)).unwrap();
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.byte_len(), 0);
    }
    let mut vm = fragment(&op(0x8044));
    let string = vm.intern(b"dynamic").unwrap();
    vm.value_stack.push(string).unwrap();
    finish(&mut vm).unwrap();
    assert_eq!(vm.value_stack.top().unwrap(), string);
    for (opcode, a, b) in [
        (0x803a, i32::MIN, 1),
        (0x803b, i32::MAX, 2),
        (0x803c, i32::MIN, -1),
    ] {
        let mut vm = fragment(&op(opcode));
        for value in [a, b] {
            vm.value_stack.push(Value::integer(value)).unwrap();
        }
        assert_eq!(finish(&mut vm), Err(VmError::Arithmetic));
    }
    for value in [f32::NAN, f32::INFINITY, 2147483648.0] {
        let mut vm = fragment(&op(0x8044));
        vm.value_stack.push(Value::float(value)).unwrap();
        assert_eq!(finish(&mut vm), Err(VmError::Arithmetic));
    }
    let mut vm = fragment(&op(0x803d));
    let value = vm.intern(b"nonnumeric").unwrap();
    vm.value_stack.push(value).unwrap();
    vm.value_stack.push(Value::float(0.0)).unwrap();
    assert_eq!(finish(&mut vm).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(vm.value_stack.byte_len(), 0);
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
    for opcode in [0x8013, 0x8031] {
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
        finish(&mut vm).unwrap();
        assert_eq!(vm.value_stack.top().unwrap(), Value::float(2.5));
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
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::Waiting);
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
