//! SPEC-009 `ai.txt`: the INI grammar and the packet table built from it.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::ai::{Config, Packets};

fuzz_target!(|data: &[u8]| {
    let _ = Config::parse(data);
    if let Ok(packets) = Packets::parse(data) {
        for number in [-1, 0, 1, packets.len() as i32, i32::MAX] {
            let _ = packets.resolve(number);
        }
    }
});
