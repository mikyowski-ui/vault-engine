//! Umbrella over `validate_asset`: the first input byte selects the asset
//! path (and so the parser and any path-dependent rules), the rest is the
//! asset.
#![no_main]

use libfuzzer_sys::fuzz_target;
use vault_formats::{VfsPath, validate_asset};

const PATHS: [&str; 16] = [
    "color.pal",
    "art/items/item.frm",
    "art/critters/critter.fr2",
    "font0.fon",
    "font0.aaf",
    "art/items/items.lst",
    "scripts/scripts.lst",
    "proto/items/items.lst",
    "data/other.lst",
    "sound/sfx/effect.acm",
    "text/english/game/game.msg",
    "proto/items/item.pro",
    "premade/player.gcd",
    "art/splash/splash.rix",
    "data/vault13.gam",
    "maps/map.gam",
];

fuzz_target!(|data: &[u8]| {
    let Some((selector, asset)) = data.split_first() else {
        return;
    };
    let path = VfsPath::new(PATHS[usize::from(*selector) % PATHS.len()]).unwrap();
    let _ = validate_asset(&path, asset);
});
