//! The documentation gallery's showcase scene (`cargo xtask
//! capture-gallery`): procedural art drawn here over the synthetic corpus,
//! and the camera demo's map without its script. Every pixel comes from the
//! functions below and the synthetic palette; nothing is taken from a
//! retail file.
use crate::{state, world};

/// The synthetic palette (`state::preview_palette`) is a 3-3-2 colour
/// cube: red and green 0-7, blue 0-3. Indexes 229 and up cycle in the
/// game, so the colours here stay below them.
fn rgb(r: u8, g: u8, b: u8) -> u8 {
    let index = (r << 5) | (g << 2) | b;
    debug_assert!(index < 229 && index != 0);
    index
}

/// A fixed pseudo-random value for a pixel of one picture.
fn noise(x: u32, y: u32, seed: u32) -> u32 {
    let mut value = u64::from(x) | (u64::from(y) << 20) | (u64::from(seed) << 40);
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) as u32
}

/// A one-frame FRM of `width` by `height` painted by `paint` (index 0 is
/// transparent).
fn picture(width: u16, height: u16, paint: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut bytes = world::frm(width, height, 0);
    let start = bytes.len() - usize::from(width) * usize::from(height);
    for y in 0..u32::from(height) {
        for x in 0..u32::from(width) {
            bytes[start + (y * u32::from(width) + x) as usize] = paint(x, y);
        }
    }
    bytes
}

/// A floor or roof tile (80 by 36): `base` with sparse two-pixel flecks
/// of `accents`, and with `stripe` every fourth row when given.
fn tile(seed: u32, base: u8, accents: &[u8], stripe: Option<u8>) -> Vec<u8> {
    picture(80, 36, |x, y| {
        let roll = noise(x / 2, y, seed) as usize;
        match stripe {
            Some(stripe) if y % 4 == 0 => stripe,
            _ if roll % 100 < 93 => base,
            _ => accents[roll / 100 % accents.len()],
        }
    })
}

/// A brick wall segment, 32 by 56.
fn wall() -> Vec<u8> {
    let bricks = [rgb(4, 1, 0), rgb(5, 2, 0), rgb(4, 2, 0)];
    let mortar = rgb(3, 3, 1);
    picture(32, 56, |x, y| {
        if y < 4 {
            return rgb(2, 2, 1);
        }
        let row = (y - 4) / 7;
        let shifted = x + if row % 2 == 0 { 0 } else { 7 };
        if (y - 4) % 7 == 0 || shifted % 14 == 0 {
            mortar
        } else {
            bricks[noise(shifted / 14, row, 7) as usize % bricks.len()]
        }
    })
}

/// A standing figure in a blue suit with a yellow stripe, 20 by 40.
fn figure() -> Vec<u8> {
    let (skin, hair) = (rgb(6, 4, 2), rgb(2, 1, 0));
    let (suit, dark, stripe) = (rgb(0, 2, 3), rgb(0, 1, 2), rgb(6, 6, 0));
    picture(20, 40, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let head = (x - 10).pow(2) + (y - 6).pow(2);
        match y {
            0..=11 if head <= 16 => {
                if y <= 3 {
                    hair
                } else {
                    skin
                }
            }
            12..=27 if (5..15).contains(&x) => {
                if x == 9 || x == 10 {
                    stripe
                } else {
                    suit
                }
            }
            13..=25 if (3..5).contains(&x) || (15..17).contains(&x) => suit,
            26..=27 if (3..5).contains(&x) || (15..17).contains(&x) => skin,
            28..=39 if (6..9).contains(&x) || (11..14).contains(&x) => dark,
            _ => 0,
        }
    })
}

/// A wooden crate, 20 by 16.
fn crate_art() -> Vec<u8> {
    let (plank, edge) = (rgb(5, 3, 1), rgb(3, 2, 0));
    picture(20, 16, |x, y| {
        if x == 0 || y == 0 || x == 19 || y == 15 || x == y + 2 || y % 5 == 0 {
            edge
        } else {
            plank
        }
    })
}

/// A green barrel with metal bands, 18 by 26.
fn barrel() -> Vec<u8> {
    let (body, band, shade) = (rgb(1, 4, 0), rgb(3, 4, 1), rgb(0, 3, 0));
    picture(18, 26, |x, y| {
        if y % 9 == 2 {
            band
        } else if !(3..=14).contains(&x) {
            shade
        } else {
            body
        }
    })
}

/// The showcase map's file name in the corpus.
pub const SHOWCASE_MAP: &str = "maps/showcase.map";
/// The sand tile's index in the showcase's `tiles.lst` (1 stays the
/// conventional blank tile).
const SAND: u32 = 3;

/// Restyle a corpus [`state::write_corpus`] wrote, for the gallery, and add
/// [`SHOWCASE_MAP`]: the camera demo's map (a walled, roofed building with
/// a critter inside and an item, in a ring of scroll blockers) with no map
/// script, its open ground sand. The floor tiles become paving (index 0),
/// the blank tile (1), corrugated roofing (2) and sand (3); walls become
/// brick, critters figures, the item a crate and scenery a barrel. Only
/// art, `tiles.lst` and the new map change, so the corpus's scripts and
/// prototypes are untouched.
pub fn write_showcase(root: &std::path::Path) {
    let art = |path: &str, bytes: Vec<u8>| std::fs::write(root.join(path), bytes).unwrap();
    art(
        "art/tiles/plaza.frm",
        tile(1, rgb(4, 3, 1), &[rgb(3, 3, 1), rgb(5, 4, 1)], None),
    );
    art(
        "art/tiles/roof.frm",
        tile(
            2,
            rgb(3, 2, 0),
            &[rgb(4, 2, 0), rgb(4, 3, 1)],
            Some(rgb(2, 1, 0)),
        ),
    );
    art(
        "art/tiles/sand.frm",
        tile(3, rgb(5, 4, 1), &[rgb(6, 5, 1), rgb(4, 3, 1)], None),
    );
    art(
        "art/tiles/tiles.lst",
        b"plaza.frm\nblank.frm\nroof.frm\nsand.frm\n".to_vec(),
    );
    art("art/walls/sample.frm", wall());
    for animation in ["aa", "ak", "al"] {
        art(&format!("art/critters/sample{animation}.frm"), figure());
    }
    art("art/items/sample.frm", crate_art());
    art("art/scenery/sample.frm", barrel());
    // The camera demo's map without a script, renamed, its open ground
    // (floor 1) sand. Its tile words follow the 236-byte header (no
    // variables, elevation 0 only).
    let mut map = state::cam_demo_map(-1);
    map[4..20].fill(0);
    map[4..16].copy_from_slice(b"SHOWCASE.MAP");
    for square in 0..10000 {
        let at = 236 + 4 * square;
        let word = u32::from_be_bytes(map[at..at + 4].try_into().unwrap());
        if word & 0xffff == 1 {
            map[at..at + 4].copy_from_slice(&((word & 0xffff_0000) | SAND).to_be_bytes());
        }
    }
    std::fs::write(root.join(SHOWCASE_MAP), map).unwrap();
}
