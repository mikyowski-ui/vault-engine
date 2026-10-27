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

/// The crate's inventory icon, 48 by 36.
fn crate_icon() -> Vec<u8> {
    let (plank, edge) = (rgb(5, 3, 1), rgb(3, 2, 0));
    picture(48, 36, |x, y| {
        let inside = (6..42).contains(&x) && (6..30).contains(&y);
        if !inside {
            0
        } else if x == 6 || y == 6 || x == 41 || y == 29 || x == y + 6 || y % 6 == 0 {
            edge
        } else {
            plank
        }
    })
}

/// A blade's inventory icon, 56 by 24: a steel blade `length` pixels long
/// and `width` wide on a brown haft.
fn blade_icon(length: i32, width: i32) -> Vec<u8> {
    let (steel, edge, haft) = (rgb(5, 5, 2), rgb(3, 3, 2), rgb(4, 2, 0));
    picture(56, 24, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let middle = 12;
        let blade_start = 54 - length;
        if x >= blade_start && x < 54 && (y - middle).abs() <= width / 2 + (54 - x) / 8 {
            if (y - middle).abs() == width / 2 {
                edge
            } else {
                steel
            }
        } else if (4..blade_start).contains(&x) && (y - middle).abs() <= 1 {
            haft
        } else {
            0
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
    // Every unarmed animation (SPEC-007's suffixes: stand, walk, the
    // actions, punch and kick, hits and dodges, falls and deaths) as the
    // standing figure, so attacks and hits find their art.
    let mut suffixes: Vec<String> = (b'a'..=b't').map(|c| format!("a{}", c as char)).collect();
    suffixes.extend((b'a'..=b'p').map(|c| format!("b{}", c as char)));
    suffixes.extend((b'a'..=b'p').map(|c| format!("r{}", c as char)));
    suffixes.extend(["ch", "cj", "na"].map(String::from));
    for animation in suffixes {
        art(&format!("art/critters/sample{animation}.frm"), figure());
    }
    art("art/items/sample.frm", crate_art());
    art("art/scenery/sample.frm", barrel());
    // Inventory icons (`art/inven/`), the box's prototype pointed at the
    // crate's, and two weapons the inventories can hold: a knife and a
    // spear (`proto/items` rows 2 and 3, animation codes 1 and 4).
    std::fs::create_dir_all(root.join("art/inven")).unwrap();
    art(
        "art/inven/inven.lst",
        b"crate.frm\nknife.frm\nspear.frm\n".to_vec(),
    );
    art("art/inven/crate.frm", crate_icon());
    art("art/inven/knife.frm", blade_icon(14, 4));
    art("art/inven/spear.frm", blade_icon(28, 2));
    let mut container = std::fs::read(root.join("proto/items/synthetic.pro")).unwrap();
    world::word(&mut container, 52, 0x0700_0000);
    art("proto/items/synthetic.pro", container);
    for (row, name, code, text) in [(2, "knife", 1, 200), (3, "spear", 4, 300)] {
        let mut weapon = world::prototype(0, 3);
        world::word(&mut weapon, 0, row);
        world::word(&mut weapon, 4, text);
        world::word(&mut weapon, 52, 0x0700_0000 + row - 1);
        world::word(&mut weapon, 57, code);
        art(&format!("proto/items/{name}.pro"), weapon);
    }
    art(
        "proto/items/items.lst",
        b"synthetic.pro\nknife.pro\nspear.pro\n".to_vec(),
    );
    let messages = root.join("text/english/game/pro_item.msg");
    let mut text = std::fs::read_to_string(&messages).unwrap();
    text.push_str("{200}{}{Knife}\n{201}{}{A plain knife.}\n");
    text.push_str("{300}{}{Spear}\n{301}{}{A long spear.}\n");
    std::fs::write(messages, text).unwrap();
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
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'E' => [7, 4, 6, 4, 7],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
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

/// The interface's art over a corpus [`state::write_corpus`] wrote, all
/// drawn here: the bar (SPEC-014 §3.19-3.25: `iface.frm`, its buttons, the
/// counters' `numbers.frm` with digits in place of the Pip-Boy fixture's
/// flat one, the AP pips and the combat panel), the cursors and the action
/// menu's icons (§3.26-3.28), the loot window and its arrows (SPEC-013
/// §3.16), and `font1.aaf` (the synthetic preview font) for the interface's
/// text. `cargo xtask fixtures` and `capture-gallery` add it; the tests'
/// corpus keeps its blank bar, which the recorded Vanilla replay digests
/// cover.
pub fn write_hud(root: &std::path::Path) {
    let dir = root.join("art/intrface");
    std::fs::create_dir_all(&dir).unwrap();
    let art = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).unwrap();
    art("iface.frm", iface().frm());
    // The interface's text font (101: the monitor, the to-hit text).
    std::fs::write(root.join("font1.aaf"), state::preview_aaf()).unwrap();
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
    // The pointers (SPEC-014 §3.26-3.27): the screen arrow, the in-world
    // arrow, crosshair, movement hex and use crosshair, and the action
    // pick art.
    // A cursor's hotspot is the frame's bottom middle less its shift
    // (SPEC-001's per-direction shift), so each art sets its own: the
    // arrows' tips, the crosshairs' centers, the hex outline on its cell.
    art("stdarrow.frm", shifted(arrow(18), [9, 17]));
    art("actarrow.frm", shifted(arrow(18), [9, 17]));
    art("actpick.frm", shifted(arrow(18), [9, 17]));
    art("actarrom.frm", shifted(arrow(18), [9, 17]));
    art(
        "acrshair.frm",
        shifted(crosshair(21, (6 << 5) | (1 << 2)), [0, 10]),
    );
    art(
        "crsshair.frm",
        shifted(crosshair(21, (6 << 5) | (1 << 2)), [0, 10]),
    );
    art("crossuse.frm", shifted(crosshair(21, AMBER), [0, 10]));
    art(
        "acttohit.frm",
        shifted(crosshair(21, (6 << 5) | (1 << 2)), [0, 10]),
    );
    art("msef003.frm", shifted(hex_outline(), [0, 7]));
    // The AP pips (SPEC-014 §3.22): 8 by 5, nine pixels apart.
    art("hlgrn.frm", pip((1 << 5) | (6 << 2)));
    art("hlred.frm", pip(6 << 5));
    // The combat panel (§3.25): `endanim.frm`'s five frames slide its
    // shutter open, the lights sit over it, and the End Turn and End
    // Combat buttons go in it.
    art("endanim.frm", panel_frames());
    art("endltgrn.frm", light((1 << 5) | (7 << 2)));
    art("endltred.frm", light(7 << 5));
    for ((up, down), label) in [
        (("endturnu.frm", "endturnd.frm"), "TURN"),
        (("endcmbtu.frm", "endcmbtd.frm"), "CMBT"),
    ] {
        let [up_art, down_art] = button(38, 22, label);
        art(up, up_art);
        art(down, down_art);
    }
    // The loot window (SPEC-013 §3.16, 537 by 376) and its scroll and
    // body-cycling arrows.
    art("loot.frm", loot_window().frm());
    for ((up, down), direction) in [
        (("invupout.frm", "invupin.frm"), Arrow::Up),
        (("invdnout.frm", "invdnin.frm"), Arrow::Down),
    ] {
        let [up_art, down_art] = arrow_button(22, 23, direction);
        art(up, up_art);
        art(down, down_art);
    }
    for ((up, down), direction) in [
        (("slu.frm", "sld.frm"), Arrow::Left),
        (("sru.frm", "srd.frm"), Arrow::Right),
    ] {
        let [up_art, down_art] = arrow_button(20, 18, direction);
        art(up, up_art);
        art(down, down_art);
    }
    // The watch the combat lock shows (one frame stands for each).
    for name in ["wait.frm", "wait2.frm", "watch.frm"] {
        art(name, shifted(watch(), [0, 7]));
    }
    // The action menu's icons (§3.28), 40 by 40, normal and highlighted.
    for (stem, label) in [
        ("useget", "USE"),
        ("look", "LOOK"),
        ("inven", "INV"),
        ("skill", "SKIL"),
        ("rotate", "ROT"),
        ("talk", "TALK"),
        ("cancel", "CANC"),
    ] {
        let [normal, lit] = icon(label);
        art(&format!("{stem}n.frm"), normal);
        art(&format!("{stem}h.frm"), lit);
    }
}

/// `frm` with its first direction's shift set to `[x, y]` (SPEC-001: the
/// six x shifts at 10, the six y shifts at 22, big-endian words).
fn shifted(mut frm: Vec<u8>, [x, y]: [i16; 2]) -> Vec<u8> {
    frm[10..12].copy_from_slice(&x.to_be_bytes());
    frm[22..24].copy_from_slice(&y.to_be_bytes());
    frm
}

/// An FRM of several frames, each `width` by `height` from `frames`
/// (SPEC-001: one direction, the frames one after another).
fn sequence(width: u16, height: u16, frames: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = world::frm(width, height, 0);
    bytes.truncate(62);
    bytes[8..10].copy_from_slice(&(frames.len() as u16).to_be_bytes());
    let each = 12 + u32::from(width) * u32::from(height);
    bytes[58..62].copy_from_slice(&(each * frames.len() as u32).to_be_bytes());
    for pixels in frames {
        let frame = world::frm(width, height, 0);
        let header = frame.len() - pixels.len();
        bytes.extend_from_slice(&frame[62..header]);
        bytes.extend_from_slice(pixels);
    }
    bytes
}

/// The loot window, 537 by 376: a riveted steel panel with the two
/// six-slot columns' wells (the looter's at x 44, the target's at x 422),
/// the two portraits' wells (176 and 297, y 37), and the Done button's
/// well under them, at SPEC-013's places.
fn loot_window() -> Canvas {
    let mut window = Canvas::new(537, 376, STEEL);
    for y in 0..376 {
        for x in 0..537 {
            if noise(x as u32 / 24, y as u32, 13).is_multiple_of(13) {
                window.set(x, y, STEEL_LIGHT);
            }
        }
    }
    window.bevel([0, 0, 537, 376], STEEL_SHINE, STEEL_SHADOW);
    window.bevel([1, 1, 535, 374], STEEL_LIGHT, STEEL_DARK);
    for (x, y) in [(8, 8), (528, 8), (8, 367), (528, 367)] {
        window.rivet(x, y);
    }
    for x in [40, 418] {
        window.well([x, 31, 72, 296], RECESS);
        for row in 1..6 {
            window.fill([x + 2, 31 + 48 * row, 68, 1], STEEL_DARK);
        }
    }
    for x in [172, 293] {
        window.well([x, 33, 68, 108], 0);
    }
    window.well([126, 37, 26, 50], RECESS);
    window.well([377, 37, 26, 50], RECESS);
    window.well([432, 158, 48, 26], RECESS);
    window.well([284, 324, 23, 24], RECESS);
    window
}

/// Which way an arrow button points.
#[derive(Clone, Copy)]
enum Arrow {
    Up,
    Down,
    Left,
    Right,
}

/// An arrow button's up and down art: a steel face with an amber triangle
/// pointing `direction`; pressed, darker with the bevel inverted.
fn arrow_button(width: u32, height: u32, direction: Arrow) -> [Vec<u8>; 2] {
    [false, true].map(|down| {
        let face = if down { STEEL_DARK } else { STEEL_LIGHT };
        let mut art = Canvas::new(width, height, face);
        let (w, h) = (width as i32, height as i32);
        if down {
            art.bevel([0, 0, w, h], STEEL_SHADOW, STEEL);
        } else {
            art.bevel([0, 0, w, h], STEEL_SHINE, STEEL_SHADOW);
        }
        let (cx, cy) = (w / 2, h / 2 + i32::from(down));
        for step in 0..5 {
            let (along, across) = (step - 2, step);
            for spread in -across..=across {
                let (x, y) = match direction {
                    Arrow::Up => (cx + spread, cy + along),
                    Arrow::Down => (cx + spread, cy - along),
                    Arrow::Left => (cx + along, cy + spread),
                    Arrow::Right => (cx - along, cy + spread),
                };
                art.set(x, y, AMBER);
            }
        }
        art.frm()
    })
}

/// An AP pip in `ink`, 8 by 5, its corners cut.
fn pip(ink: u8) -> Vec<u8> {
    picture(8, 5, |x, y| {
        let corner = (x == 0 || x == 7) && (y == 0 || y == 4);
        if corner { 0 } else { ink }
    })
}

/// The combat panel's five frames, 57 by 58: a steel shutter sliding up to
/// show the dark well the buttons sit in.
fn panel_frames() -> Vec<u8> {
    let frames: Vec<Vec<u8>> = (0..5)
        .map(|frame| {
            let mut art = Canvas::new(57, 58, RECESS);
            let shutter = 58 - 58 * frame / 4;
            art.fill([0, 0, 57, shutter], STEEL_LIGHT);
            for y in (3..shutter).step_by(6) {
                art.fill([2, y, 53, 1], STEEL_SHADOW);
            }
            art.bevel([0, 0, 57, 58], STEEL_SHADOW, STEEL_LIGHT);
            art.pixels
        })
        .collect();
    sequence(57, 58, &frames)
}

/// The combat panel's light, a lamp at its top in `ink` (57 by 58, the rest
/// transparent).
fn light(ink: u8) -> Vec<u8> {
    let mut art = Canvas::new(57, 58, 0);
    for y in 0..5_i32 {
        for x in 0..9_i32 {
            if (2 * x - 8).pow(2) + (2 * y - 4).pow(2) <= 20 {
                art.set(24 + x, 1 + y, ink);
            }
        }
    }
    art.frm()
}

/// A small pocket watch, 12 by 15.
fn watch() -> Vec<u8> {
    let (case, face, hand) = (AMBER, (6 << 5) | (6 << 2) | 3, STEEL_SHADOW);
    let mut art = Canvas::new(12, 15, 0);
    art.fill([5, 0, 2, 2], case);
    for y in 2..15_i32 {
        for x in 0..12_i32 {
            let d2 = (2 * x - 11).pow(2) + (2 * y - 17).pow(2);
            if d2 <= 121 {
                art.set(x, y, if d2 > 81 { case } else { face });
            }
        }
    }
    art.fill([5, 5, 1, 4], hand);
    art.fill([6, 8, 2, 1], hand);
    art.frm()
}

/// An arrow pointer `size` pixels high, its tip at the top-left (white
/// with a black edge; outside it transparent).
fn arrow(size: i32) -> Vec<u8> {
    let (fill, edge) = ((6 << 5) | (6 << 2) | 3, (1 << 5) | (1 << 2) | 1);
    let mut art = Canvas::new(size as u32, size as u32, 0);
    for y in 0..size {
        let width = (y * 2 / 3).min(size - 1) + 1;
        for x in 0..width {
            let inside = x > 0 && x < width - 1 && y < size - 1;
            art.set(x, y, if inside { fill } else { edge });
        }
    }
    art.frm()
}

/// A ring with cross hairs, `size` pixels across, in `ink`.
fn crosshair(size: i32, ink: u8) -> Vec<u8> {
    let mut art = Canvas::new(size as u32, size as u32, 0);
    let c = size / 2;
    for y in 0..size {
        for x in 0..size {
            let d2 = (x - c).pow(2) + (y - c).pow(2);
            let ring = ((c - 2).pow(2)..=c.pow(2)).contains(&d2);
            let hair = (x == c || y == c) && (d2 <= (c - 4).pow(2)) && d2 > 4;
            if ring || hair {
                art.set(x, y, ink);
            }
        }
    }
    art.frm()
}

/// The movement cursor: a hex cell's outline (32 by 16) in green.
fn hex_outline() -> Vec<u8> {
    let ink = (1 << 5) | (6 << 2);
    let mut art = Canvas::new(32, 16, 0);
    for y in 0..16 {
        let half = if y < 4 {
            4 + 4 * y
        } else if y > 11 {
            4 + 4 * (15 - y)
        } else {
            16
        };
        let half = half.min(16);
        let (left, right) = (16 - half, 15 + half);
        art.set(left, y, ink);
        art.set(right.min(31), y, ink);
        if y == 0 || y == 15 {
            art.fill([left, y, right - left + 1, 1], ink);
        }
    }
    art.frm()
}

/// An action-menu icon, 40 by 40: a bevelled steel tile with `label`, and
/// highlighted, amber with the label in black.
fn icon(label: &str) -> [Vec<u8>; 2] {
    [false, true].map(|lit| {
        let (face, ink) = if lit {
            (AMBER, STEEL_SHADOW)
        } else {
            (STEEL, AMBER)
        };
        let mut art = Canvas::new(40, 40, face);
        art.bevel([0, 0, 40, 40], STEEL_SHINE, STEEL_SHADOW);
        art.bevel([1, 1, 38, 38], STEEL_LIGHT, STEEL_DARK);
        art.text((40 - Canvas::width(label)) / 2, 17, label, ink);
        art.frm()
    })
}
