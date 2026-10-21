//! SPEC-008 `SAVE.DAT`: the fixed header and the complete ordered payloads,
//! including the player object tree.
#![no_main]

#[path = "common.rs"]
mod common;

use libfuzzer_sys::fuzz_target;
use vault_formats::savegame::{SaveGame, SaveHeader};

/// The campaign initializer's variable count observed in the corpus.
const CAMPAIGN_COUNT: usize = 618;

fuzz_target!(|data: &[u8]| {
    let _ = SaveHeader::parse(data);
    let _ = SaveGame::parse(data, CAMPAIGN_COUNT, &common::lookup);
});
