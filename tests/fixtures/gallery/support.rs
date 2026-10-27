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

// --- The interface bar ------------------------------------------------

/// A painted canvas, row-major, written as a one-frame FRM.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl Canvas {
    fn new(width: u32, height: u32, fill: u8) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; (width * height) as usize],
        }
    }
    fn set(&mut self, x: i32, y: i32, color: u8) {
        if (0..self.width as i32).contains(&x) && (0..self.height as i32).contains(&y) {
            self.pixels[(y as u32 * self.width + x as u32) as usize] = color;
        }
    }
    fn fill(&mut self, [x, y, w, h]: [i32; 4], color: u8) {
        for row in y..y + h {
            for column in x..x + w {
                self.set(column, row, color);
            }
        }
    }
    /// A bevel around `[x, y, w, h]`: `light` on the top and left edges,
    /// `dark` on the bottom and right (swapped, it is a recess).
    fn bevel(&mut self, [x, y, w, h]: [i32; 4], light: u8, dark: u8) {
        self.fill([x, y, w, 1], light);
        self.fill([x, y, 1, h], light);
        self.fill([x, y + h - 1, w, 1], dark);
        self.fill([x + w - 1, y, 1, h], dark);
    }
    /// A recessed well: a shadowed frame and `inside` within it.
    fn well(&mut self, [x, y, w, h]: [i32; 4], inside: u8) {
        self.bevel([x, y, w, h], STEEL_SHADOW, STEEL_LIGHT);
        self.fill([x + 1, y + 1, w - 2, h - 2], inside);
    }
    /// A domed rivet centered at `[x, y]`.
    fn rivet(&mut self, x: i32, y: i32) {
        self.fill([x - 1, y - 1, 3, 3], STEEL_LIGHT);
        self.set(x - 1, y - 1, STEEL_SHINE);
        self.set(x + 1, y + 1, STEEL_SHADOW);
    }
    /// `text` in the 3-by-5 [`glyph`] font, top-left at `[x, y]`.
    fn text(&mut self, x: i32, y: i32, text: &str, color: u8) {
        for (index, c) in text.chars().enumerate() {
            for (row, bits) in glyph(c).iter().enumerate() {
                for column in 0..3 {
                    if bits & (4 >> column) != 0 {
                        self.set(x + 4 * index as i32 + column, y + row as i32, color);
                    }
                }
            }
        }
    }
    /// The width `text` takes in the 3-by-5 font.
    fn width(text: &str) -> i32 {
        4 * text.chars().count() as i32 - 1
    }
    fn frm(&self) -> Vec<u8> {
        picture(self.width as u16, self.height as u16, |x, y| {
            self.pixels[(y * self.width + x) as usize]
        })
    }
}

/// A 3-by-5 pixel font for the bar's labels, row by row (bit 2 is the
/// left column).
fn glyph(c: char) -> [u8; 5] {
    match c {
        'A' => [2, 5, 7, 5, 5],
        'C' => [3, 4, 4, 4, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'P' => [6, 5, 6, 4, 4],
        'V' => [5, 5, 5, 5, 2],
        _ => [0; 5],
    }
}

const STEEL: u8 = (2 << 5) | (2 << 2) | 1;
const STEEL_LIGHT: u8 = (3 << 5) | (3 << 2) | 1;
const STEEL_SHINE: u8 = (4 << 5) | (4 << 2) | 2;
const STEEL_DARK: u8 = (1 << 5) | (1 << 2) | 1;
const STEEL_SHADOW: u8 = (1 << 5) | (1 << 2);
const SCREEN: u8 = 1 << 2;
const SCREEN_EDGE: u8 = (1 << 5) | (6 << 2);
const AMBER: u8 = (6 << 5) | (5 << 2);
const RECESS: u8 = (1 << 5) | (1 << 2);

/// `iface.frm`, 640 by 100 (SPEC-014 §3.19's bar window): brushed steel
/// with seams and rivets, the message monitor's green-bordered screen
/// around SPEC-014's monitor rectangle (23, 24, 167, 60), the item slot's
/// and AP strip's recesses, the HP and AC counters' bezels with their
/// labels, and a well behind each button.
fn iface() -> Canvas {
    let mut bar = Canvas::new(640, 100, STEEL);
    for y in 0..100 {
        for x in 0..640 {
            let streak = noise(x as u32 / 24, y as u32, 11) % 13;
            let color = if streak == 0 { STEEL_LIGHT } else { STEEL };
            bar.set(x, y, color);
        }
    }
    bar.fill([0, 0, 640, 1], STEEL_SHINE);
    bar.fill([0, 1, 640, 1], STEEL_LIGHT);
    bar.fill([0, 99, 640, 1], STEEL_SHADOW);
    for seam in [200, 258, 462, 516, 576] {
        bar.fill([seam, 2, 1, 97], STEEL_SHADOW);
        bar.fill([seam + 1, 2, 1, 97], STEEL_LIGHT);
        for y in [7, 92] {
            bar.rivet(seam - 4, y);
            bar.rivet(seam + 5, y);
        }
    }
    for (x, y) in [(6, 7), (6, 92), (633, 7), (633, 92)] {
        bar.rivet(x, y);
    }
    // The message monitor.
    bar.well([17, 18, 179, 72], SCREEN);
    bar.bevel([19, 20, 175, 68], SCREEN_EDGE, SCREEN_EDGE);
    // The item slot, the AP strip, and the button wells.
    bar.well([264, 23, 194, 73], RECESS);
    bar.well([313, 11, 96, 11], RECESS);
    for well in [
        [208, 39, 36, 25],
        [207, 60, 40, 38],
        [216, 4, 26, 25],
        [521, 5, 26, 25],
        [524, 38, 45, 61],
        [578, 36, 58, 58],
    ] {
        bar.well(well, RECESS);
    }
    // The HP and AC counters' bezels, each labelled above it (the ammo
    // bar is drawn at x 463, left of them).
    for (label, y) in [("HP", 40), ("AC", 75)] {
        bar.well([470, y - 3, 39, 23], 0);
        bar.text(470 + (39 - Canvas::width(label)) / 2, y - 10, label, AMBER);
    }
    bar
}

/// A bar button's up and down art: a bevelled steel face with `label`
/// centered in amber; pressed, the face darkens, the bevel inverts and the
/// label moves down a pixel.
fn button(width: u32, height: u32, label: &str) -> [Vec<u8>; 2] {
    [false, true].map(|down| {
        let face = if down { STEEL_DARK } else { STEEL_LIGHT };
        let mut art = Canvas::new(width, height, face);
        let (w, h) = (width as i32, height as i32);
        if down {
            art.bevel([0, 0, w, h], STEEL_SHADOW, STEEL);
        } else {
            art.bevel([0, 0, w, h], STEEL_SHINE, STEEL_SHADOW);
        }
        let x = (w - Canvas::width(label)) / 2;
        art.text(x, (h - 5) / 2 + i32::from(down), label, AMBER);
        art.frm()
    })
}

/// A round button's up and down art, `color` with a highlight (pressed,
/// darker and without it); outside the disc is transparent.
fn round(width: u32, height: u32, color: u8, dark: u8, shine: u8) -> [Vec<u8>; 2] {
    [false, true].map(|down| {
        let mut art = Canvas::new(width, height, 0);
        let (cx, cy) = ((width as i32 - 1) / 2, (height as i32 - 1) / 2);
        let radius = cx.min(cy);
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let d2 = (x - cx).pow(2) + (y - cy).pow(2);
                if d2 > radius * radius {
                    continue;
                }
                let rim = d2 > (radius - 2).pow(2);
                let lit = !down && (x - cx + 3).pow(2) + (y - cy + 3).pow(2) <= 4;
                art.set(
                    x,
                    y,
                    match (rim, lit, down) {
                        (true, _, _) => dark,
                        (_, true, _) => shine,
                        (_, _, true) => dark,
                        _ => color,
                    },
                );
            }
        }
        art.frm()
    })
}

/// `numbers.frm`, 360 by 17 (SPEC-014 §3.21): three 120-pixel bands
/// (white, yellow, red) of seven-segment digits 0-9 in 9-pixel cells, then
/// the two roll frames (90, 99), the minus (108) and the blank (114).
fn numbers() -> Vec<u8> {
    // Segments a-g, as [x, y, w, h] in a 9 by 17 cell.
    const SEGMENTS: [[i32; 4]; 7] = [
        [2, 1, 5, 2],
        [6, 2, 2, 6],
        [6, 9, 2, 6],
        [2, 14, 5, 2],
        [1, 9, 2, 6],
        [1, 2, 2, 6],
        [2, 7, 5, 2],
    ];
    // Which segments each digit lights (bit 0 is a).
    const DIGITS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let mut art = Canvas::new(360, 17, 0);
    for (band, ink) in [
        (0, (6 << 5) | (6 << 2) | 3),
        (120, (6 << 5) | (6 << 2)),
        (240, (6 << 5) | (1 << 2)),
    ] {
        for (digit, lit) in DIGITS.iter().enumerate() {
            let x = band + 9 * digit as i32;
            for (segment, [sx, sy, w, h]) in SEGMENTS.into_iter().enumerate() {
                if lit & (1 << segment) != 0 {
                    art.fill([x + sx, sy, w, h], ink);
                }
            }
        }
        // The roll frames: the middle bar, high and low.
        art.fill([band + 90 + 2, 4, 5, 2], ink);
        art.fill([band + 99 + 2, 11, 5, 2], ink);
        // The minus.
        art.fill([band + 108 + 1, 7, 4, 2], ink);
    }
    art.frm()
}

/// The interface bar's art (SPEC-014 §3.19-3.22) over a corpus
/// [`state::write_corpus`] wrote: `iface.frm`, the counters'
/// `numbers.frm` (replacing the Pip-Boy fixture's flat one with digits),
/// and the up and down art of the inventory, options, Skilldex,
/// swap-hands, map, character and Pip-Boy buttons and the item slot, all
/// drawn here. `cargo xtask fixtures` and `capture-gallery` add it; the
/// tests' corpus keeps its blank bar, which the recorded Vanilla replay
/// digests cover.
pub fn write_hud(root: &std::path::Path) {
    let dir = root.join("art/intrface");
    std::fs::create_dir_all(&dir).unwrap();
    let art = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).unwrap();
    art("iface.frm", iface().frm());
    art("numbers.frm", numbers());
    for ((up, down), [up_art, down_art]) in [
        (("invbutup.frm", "invbutdn.frm"), button(32, 21, "INV")),
        (("mapup.frm", "mapdn.frm"), button(41, 19, "MAP")),
        (("chaup.frm", "chadn.frm"), button(41, 19, "CHA")),
        (("pipup.frm", "pipdn.frm"), button(41, 19, "PIP")),
        (("sattkbup.frm", "sattkbdn.frm"), button(188, 67, "")),
        (
            ("optiup.frm", "optidn.frm"),
            round(34, 34, STEEL_LIGHT, STEEL_SHADOW, STEEL_SHINE),
        ),
        (
            ("bigredup.frm", "bigreddn.frm"),
            round(22, 21, 5 << 5, 3 << 5, (6 << 5) | (3 << 2) | 1),
        ),
    ] {
        art(up, up_art);
        art(down, down_art);
    }
    art("saturbup.frm", button(188, 67, "")[1].clone());
}
