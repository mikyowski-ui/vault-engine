//! SPEC-015 version 2 archives: the in-memory parser and the streamed
//! reader must both refuse malformed footers, trees and members without
//! panicking, and agree on what they accept.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use vault_formats::Dat2Archive;

fuzz_target!(|data: &[u8]| {
    let memory = Dat2Archive::parse(data.to_vec());
    let streamed = Dat2Archive::from_reader(Cursor::new(data.to_vec()));
    assert_eq!(memory.is_ok(), streamed.is_ok());
    if let (Ok(memory), Ok(streamed)) = (memory, streamed) {
        for entry in memory.entries() {
            let first = memory.extract(entry);
            let second = streamed.extract(entry);
            assert_eq!(first.is_ok(), second.is_ok());
            if let (Ok(first), Ok(second)) = (first, second) {
                assert_eq!(first, second);
                assert_eq!(first.len(), entry.real_size as usize);
            }
        }
    }
});
