//! SPEC-004 `.INT` programs: the safe loader, then a bounded run of the
//! program under the basic host (every update is budgeted, and critical
//! sections stop at the VM's hard instruction limit).
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_vm::{BasicHost, IntProgram, RunOutcome, Vm};

fuzz_target!(|data: &[u8]| {
    let Ok(program) = IntProgram::parse(data) else {
        return;
    };
    let mut vm = Vm::new(program);
    let mut host = BasicHost::default();
    for _ in 0..64 {
        match vm.run(&mut host, 100) {
            Ok(RunOutcome::Budget | RunOutcome::Yielded) => {}
            _ => break,
        }
        host.milliseconds = host.milliseconds.wrapping_add(1000);
        if vm.poll_scheduled(&mut host, 100).is_err() {
            break;
        }
    }
});
