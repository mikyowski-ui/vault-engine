//! SPEC-018 §7 Fallout 2 `SAVE.DAT`: the header and its install checks, then
//! the complete save against an installed shape, and a save that parses back
//! through the writer.
//!
//! The first input byte picks the shape, so a save can name tables that are
//! installed or absent; the rest is the file. Every count in the layout is
//! capped by the parser or checked against the shape.
#![no_main]

#[path = "common.rs"]
mod common;

use libfuzzer_sys::fuzz_target;
use vault_formats::{
    GameProfile,
    fallout2::save::Fallout2Shape,
    savegame::{SaveGame, SaveHeader},
};

/// The campaign count `fuzz/seed.py` writes in its seed's header.
const CAMPAIGN_COUNT: usize = 5;
/// The header is 30051 bytes, so this leaves ample room for the payloads.
const MAX_INPUT: usize = 1 << 20;

/// SPEC-018 §8.9's shape. With `gaps`, a second record of each table is absent.
fn shape(gaps: bool) -> Fallout2Shape {
    let mut shape = Fallout2Shape {
        grid: [7, 6],
        entrances: vec![3],
        areas_present: vec![true],
        maps: 3,
        maps_present: vec![true; 3],
        party_levels: vec![Some(2)],
    };
    if gaps {
        shape.entrances.push(1);
        shape.areas_present.push(false);
        shape.maps_present[2] = false;
        shape.party_levels.push(None);
    }
    shape
}

fuzz_target!(|data: &[u8]| {
    let Some((selector, save)) = data.split_first() else {
        return;
    };
    if save.len() > MAX_INPUT {
        return;
    }
    if let Ok(header) = SaveHeader::parse_for(save, GameProfile::Fallout2) {
        let _ = header.check_fallout2(CAMPAIGN_COUNT, false);
        let _ = header.check_fallout2(CAMPAIGN_COUNT, true);
    }
    let installed = shape(selector & 1 != 0);
    let parsed = SaveGame::parse_fallout2(save, CAMPAIGN_COUNT, &common::lookup, &installed);
    if let Ok(game) = parsed {
        let _ = game.write();
    }
});
