//! SPEC-012 MVE: header, chunk walk, video/audio opcodes, and frame display.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::mve::{Event, Movie};

/// Every event consumes at least one opcode record, so a well-behaved
/// decoder finishes a fuzz-sized input far below this bound; reaching it
/// means the event loop stopped advancing.
const MAX_EVENTS: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    let Ok(mut movie) = Movie::open(data.to_vec()) else {
        return;
    };
    for _ in 0..MAX_EVENTS {
        match movie.next_event() {
            Ok(Event::Finished) | Err(_) => return,
            Ok(_) => {}
        }
    }
    panic!("MVE event loop did not finish within {MAX_EVENTS} events");
});
