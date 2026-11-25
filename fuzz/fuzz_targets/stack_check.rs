//! SPEC-017 §8.2 the script stack model, as a session runs it on each `.INT` a
//! mod layer supplies: decode under every word, check, and only a clean
//! program is parsed under its real word set.
//!
//! The first input byte picks the table: bits 0-1 the word set, bit 2 the
//! enhanced range and bit 3 the mod words. The rest is the program. The
//! model's own step budget bounds the walk, and this target calls only what
//! shares it: `analyse_unit` gives each unit a whole budget of its own.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_vm::{IntProgram, WordSet, stack_check};

/// The input a real script might reach, which also keeps the step budget near
/// its floor: a longer program buys more steps, up to 1,000,000.
const MAX_INPUT: usize = 1 << 16;
/// The name of the one recorded exception, so that its matching runs too.
const NAME: &str = "gatedemo.int";

fuzz_target!(|data: &[u8]| {
    let Some((selector, bytes)) = data.split_first() else {
        return;
    };
    if bytes.len() > MAX_INPUT {
        return;
    }
    let words = match selector & 3 {
        0 => WordSet::FALLOUT1,
        1 => WordSet::FALLOUT2,
        _ => WordSet::FALLOUT2_SFALL,
    }
    .with_mods(selector & 8 != 0);
    let enhanced = selector & 4 != 0;
    // As the session does: the check, and not the decoder, names a bad word.
    let every = WordSet {
        words: 0x8000..=0xffff,
        mods: false,
    };
    let Ok(decoded) = IntProgram::parse_in(bytes, &every, false) else {
        return;
    };
    let table = stack_check::Table::with_enhanced(words.clone(), enhanced);
    let findings = stack_check::check(NAME, &decoded, &table, stack_check::exceptions(&words));
    let _ = stack_check::summary(&findings);
    if findings.is_empty() {
        let _ = IntProgram::parse_in(bytes, &words, enhanced);
    }
});
