//! The `enhanced` widescreen bezel: the procedural border drawn around a
//! full-screen 640x480 window (the main menu, character selection and
//! editor, the Pip-Boy, the world and town maps, save and load) when the
//! window is wider or taller than the frame. Engine extension, not part of
//! any specification; the Vanilla presentation and the fixed 640x480 view
//! never draw it.
//!
//! The design is a dark, brushed, riveted steel plate in the manner of an
//! industrial vault console:
//! - **Lip.** Around the frame runs a thin amber line, a dark gap, a
//!   four-pixel bevelled steel lip lit from the top left, and a groove.
//! - **Brackets.** An amber L-bracket marks each of the frame's corners
//!   just outside the lip.
//! - **Plates.** Beyond it the plate is brushed horizontally, lighter
//!   toward the top, and divided into panels by bevelled seams on a lattice
//!   anchored to the frame's corners: horizontal seams every 120 frame
//!   pixels (a quarter of the frame's height), vertical ones every 128
//!   beside the frame and every 160 above or below it. A rivet sits on
//!   each side of each crossing of a seam with the lip's groove or another
//!   seam.
//!
//! Everything is drawn in bezel pixels, which are frame pixels (or a
//! whole number of them, see [`bezel_layer`]), so the border scales with
//! the frame at every window size and interface scale. The brushing is
//! integer hash noise of the pixel's place relative to the frame's corner
//! and fixed seeds: the same layout draws the same bezel, and the plates
//! stay put around the frame when the window is resized. Colors are
//! palette indices, the nearest entries to fixed RGB targets outside the
//! palette-cycling range ([`BezelInks::nearest`]).
use crate::IndexedImage;
use crate::map::BAYER;

/// The frame the bezel surrounds, in frame pixels.
pub const FRAME: [u32; 2] = [640, 480];
/// The largest bezel image side (the largest [`IndexedImage`] a layer
/// takes, as the world canvas's).
const MAX_SIDE: u32 = 4096;
/// The divisors of 160 (the greatest common divisor of 640 and 480): the
/// bezel pixel sizes, in frame pixels, that keep the frame's edges on
/// whole bezel pixels.
const UNITS: [u32; 12] = [1, 2, 4, 5, 8, 10, 16, 20, 32, 40, 80, 160];

/// The plate seams' lattice, in frame pixels.
const SEAM_ROWS: i64 = 120;
const SEAM_SIDE_COLUMNS: i64 = 128;
const SEAM_BAND_COLUMNS: i64 = 160;
/// The rings around the frame, by Chebyshev distance from it in bezel
/// pixels: the amber line, the gap, the lip, its groove, then the plate.
const RING_AMBER: i64 = 1;
const RING_GAP: i64 = 2;
const RING_LIP: [i64; 2] = [3, 6];
const RING_GROOVE: i64 = 7;
/// The corner brackets' rings and arm length.
const BRACKET: [i64; 2] = [9, 10];
const BRACKET_ARM: i64 = 28;
/// Seams and rivets keep this far from the frame.
const PLATE_FROM: i64 = 13;
/// A rivet's offset from its seam crossing.
const RIVET_OFFSET: i64 = 7;
/// The brushing's fixed seeds.
const SEED_ROW: u64 = 0x5641_554c_5445_4331;
const SEED_STREAK: u64 = 0x4245_5a45_4c32_3032;
const SEED_GRAIN: u64 = 0x5249_5645_5453_3133;

/// The bezel's colors, as palette indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BezelInks {
    /// The steel, darkest to lightest.
    pub metal: [u8; 6],
    /// Grooves, seams' shadows and the gap around the frame.
    pub groove: u8,
    /// Bevel and rivet highlights.
    pub light: u8,
    /// The inner edge and the brackets' lit ring.
    pub amber: u8,
    /// The brackets' outer ring.
    pub amber_dark: u8,
}

impl BezelInks {
    /// The steel ramp's RGB targets, darkest first.
    pub const METAL: [[u8; 3]; 6] = [
        [17, 18, 20],
        [26, 27, 30],
        [36, 38, 41],
        [48, 50, 54],
        [63, 65, 69],
        [82, 84, 88],
    ];
    pub const GROOVE: [u8; 3] = [7, 7, 8];
    pub const LIGHT: [u8; 3] = [112, 114, 116];
    pub const AMBER: [u8; 3] = [212, 148, 40];
    pub const AMBER_DARK: [u8; 3] = [132, 88, 24];

    /// The entries of `palette` (display RGBA) nearest the targets, by
    /// squared RGB distance, among indices 1-228: index 0 and the
    /// palette-cycling range (229 and up) are never used, so the bezel
    /// never animates. The lowest index wins a tie.
    pub fn nearest(palette: &[[u8; 4]; 256]) -> Self {
        let pick = |rgb: [u8; 3]| {
            (1..229u8)
                .min_by_key(|index| {
                    let entry = palette[usize::from(*index)];
                    (0..3)
                        .map(|axis| (i32::from(entry[axis]) - i32::from(rgb[axis])).pow(2))
                        .sum::<i32>()
                })
                .unwrap_or(1)
        };
        Self {
            metal: Self::METAL.map(pick),
            groove: pick(Self::GROOVE),
            light: pick(Self::LIGHT),
            amber: pick(Self::AMBER),
            amber_dark: pick(Self::AMBER_DARK),
        }
    }
}

/// SplitMix64's finalizer: the brushing's noise.
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// A noise value in `-amplitude..=amplitude` for a place and a seed.
fn noise(seed: u64, a: i64, b: i64, amplitude: i64) -> i64 {
    let hash = mix(seed ^ mix((a as u64) ^ (b as u64).rotate_left(32)));
    (hash % (2 * amplitude as u64 + 1)) as i64 - amplitude
}

/// Where a bezel pixel lies relative to the frame.
#[derive(Debug, Clone, Copy)]
struct Place {
    /// The pixel relative to the frame's top-left corner.
    u: i64,
    v: i64,
    /// The Chebyshev distance from the frame (0 inside it).
    ring: i64,
    /// The pixel is beside the frame's columns (left or right of it), and
    /// its horizontal distance from the frame.
    side: bool,
    across: i64,
    /// The ring's side faces the light (the top and left sides).
    lit: bool,
}

impl Place {
    fn new(u: i64, v: i64, frame: [i64; 2]) -> Self {
        let outside = |value: i64, size: i64| {
            if value < 0 {
                -value
            } else if value >= size {
                value - size + 1
            } else {
                0
            }
        };
        let (ox, oy) = (outside(u, frame[0]), outside(v, frame[1]));
        let ring = ox.max(oy);
        // A corner belongs to the vertical side when it is farther across.
        let vertical = ox > oy;
        let lit = if vertical { u < 0 } else { v < 0 };
        Self {
            u,
            v,
            ring,
            side: ox > 0,
            across: ox,
            lit,
        }
    }
    /// The distance along the ring from its nearest corner.
    fn corner_distance(self, frame: [i64; 2]) -> i64 {
        let along = |value: i64, size: i64| (value + self.ring).min(size - 1 + self.ring - value);
        if self.across >= self.ring && self.across > 0 {
            along(self.v, frame[1])
        } else {
            along(self.u, frame[0])
        }
    }
}

/// The bezel for a frame whose top-left corner is bezel pixel `hole` in a
/// `size` image, `unit` frame pixels to a bezel pixel (a divisor of 160).
/// The frame's own area is filled with the darkest steel.
pub fn draw_bezel(size: [u32; 2], hole: [i64; 2], unit: u32, inks: BezelInks) -> IndexedImage {
    let unit = i64::from(unit.max(1));
    let scaled = |value: i64| (value / unit).max(1);
    let frame = FRAME.map(|side| i64::from(side) / unit);
    let [width, height] = size.map(i64::from);
    let mut pixels = vec![inks.metal[0]; (width * height) as usize];
    let ramp = |position: i64, x: i64, y: i64| {
        let position = position.clamp(0, 16 * 5);
        let level = position / 16
            + i64::from(i64::from(BAYER[(y & 3) as usize][(x & 3) as usize]) < position % 16);
        inks.metal[level.min(5) as usize]
    };
    let [lip0, lip1] = RING_LIP.map(scaled);
    let [bracket0, bracket1] = BRACKET.map(scaled);
    for y in 0..height {
        // The brushing: each row has its own tone and its own streaks.
        let v = y - hole[1];
        let row_tone = noise(SEED_ROW, v, 0, 5);
        let streak = 17 + noise(SEED_ROW, v, 1, 15).abs();
        let shift = noise(SEED_ROW, v, 2, 64);
        // Lighter toward the top of the window.
        let lighting = 10 * (height - y) / height.max(1);
        for x in 0..width {
            let u = x - hole[0];
            let place = Place::new(u, v, frame);
            let ring = place.ring;
            let color = if ring == 0 {
                inks.metal[0]
            } else if ring == scaled(RING_AMBER) {
                inks.amber
            } else if ring <= scaled(RING_GAP) {
                inks.groove
            } else if ring <= lip1 && ring >= lip0 {
                let step = ring - lip0;
                let last = lip1 - lip0;
                match (place.lit, step) {
                    (true, step) if step == last => inks.light,
                    (true, 0) => inks.metal[1],
                    (true, _) => inks.metal[if step * 2 < last { 3 } else { 4 }],
                    (false, step) if step == last => inks.metal[0],
                    (false, _) => inks.metal[if step * 2 < last { 3 } else { 2 }],
                }
            } else if ring <= scaled(RING_GROOVE) {
                inks.groove
            } else if ring >= bracket0
                && ring <= bracket1
                && place.corner_distance(frame) < scaled(BRACKET_ARM)
            {
                if ring == bracket0 {
                    inks.amber
                } else {
                    inks.amber_dark
                }
            } else {
                // The brushed plate, darker close to the lip.
                let segment = (u + shift).div_euclid(streak);
                let tone = 2 * 16
                    + lighting
                    + row_tone
                    + noise(SEED_STREAK, segment, v, 4)
                    + noise(SEED_GRAIN, u, v, 2)
                    - if ring < scaled(PLATE_FROM) { 6 } else { 0 };
                ramp(tone, u, v)
            };
            pixels[(y * width + x) as usize] = color;
        }
    }
    let mut image = IndexedImage::from_pixels(size[0], size[1], pixels).expect("sized pixels");
    draw_seams(&mut image, hole, frame, unit, inks);
    image
}

/// The plate seams and their rivets.
fn draw_seams(
    image: &mut IndexedImage,
    hole: [i64; 2],
    frame: [i64; 2],
    unit: i64,
    inks: BezelInks,
) {
    let [width, height] = [image.width(), image.height()].map(i64::from);
    let plate_from = (PLATE_FROM / unit).max(1);
    let rows = (SEAM_ROWS / unit).max(2);
    let side_columns = (SEAM_SIDE_COLUMNS / unit).max(2);
    let band_columns = (SEAM_BAND_COLUMNS / unit).max(2);
    let groove = (RING_GROOVE / unit).max(1);
    let offset = (RIVET_OFFSET / unit).max(1);
    let place = |x: i64, y: i64| Place::new(x - hole[0], y - hole[1], frame);
    let on_plate = |x: i64, y: i64| {
        (0..width).contains(&x) && (0..height).contains(&y) && place(x, y).ring >= plate_from
    };
    // Seam lines: a groove, with a highlight right of or below it.
    let vertical = |x: i64, y: i64| {
        let place = place(x, y);
        if place.side {
            // Beside the frame: every `side_columns` from the lip's groove.
            place.across > groove && (place.across - groove) % side_columns == 0
        } else {
            // Above or below the frame: between its corners' brackets.
            place.u > 0 && place.u.rem_euclid(band_columns) == 0
        }
    };
    let horizontal = |_: i64, y: i64| (y - hole[1]).rem_euclid(rows) == 0;
    let pixels = image.pixels_mut();
    for y in 0..height {
        for x in 0..width {
            if !on_plate(x, y) {
                continue;
            }
            let index = (y * width + x) as usize;
            if vertical(x, y) || horizontal(x, y) {
                pixels[index] = inks.groove;
            } else if (x > 0 && on_plate(x - 1, y) && vertical(x - 1, y))
                || (y > 0 && on_plate(x, y - 1) && horizontal(x, y - 1))
            {
                pixels[index] = inks.metal[4];
            }
        }
    }
    // Rivets beside each crossing of a horizontal seam with a vertical
    // seam or the lip's groove.
    let mut crossings = Vec::new();
    for y in 0..height {
        if !horizontal(0, y) {
            continue;
        }
        for x in 0..width {
            let place = place(x, y);
            let lip = place.side && place.across == groove && place.v >= 0 && place.v < frame[1];
            if lip || (on_plate(x, y) && vertical(x, y)) {
                crossings.push([x, y]);
            }
        }
    }
    for [x, y] in crossings {
        for [dx, dy] in [[-1, -1], [1, -1], [-1, 1], [1, 1]] {
            let center = [x + dx * offset, y + dy * offset];
            if !on_plate(center[0], center[1]) || place(center[0], center[1]).ring < plate_from {
                continue;
            }
            draw_rivet(pixels, [width, height], center, unit, inks);
        }
    }
}

/// A rivet centered on `center`: a domed head lit from the top left, with
/// a shadow to the bottom right.
fn draw_rivet(pixels: &mut [u8], size: [i64; 2], center: [i64; 2], unit: i64, inks: BezelInks) {
    let radius = (2 / unit).max(1);
    for dy in -radius..=radius + 1 {
        for dx in -radius..=radius + 1 {
            let [x, y] = [center[0] + dx, center[1] + dy];
            if !(0..size[0]).contains(&x) || !(0..size[1]).contains(&y) {
                continue;
            }
            let (head, shadow) = (dx * dx + dy * dy, (dx - 1) * (dx - 1) + (dy - 1) * (dy - 1));
            let color = if head <= radius * radius {
                // Lit on its upper left rim, shaded on its lower right.
                if dx + dy <= -radius {
                    inks.light
                } else if dx + dy >= radius {
                    inks.metal[3]
                } else {
                    inks.metal[5]
                }
            } else if shadow <= radius * radius {
                inks.groove
            } else {
                continue;
            };
            pixels[(y * size[0] + x) as usize] = color;
        }
    }
}

/// The bezel layer for a 640x480 frame drawn into `frame_rect` (surface
/// pixels) on a `surface`-sized window: its image and the surface
/// rectangle it covers (the whole surface, overhanging it by less than a
/// bezel pixel). Bezel pixels are frame pixels, or the fewest whole frame
/// pixels (a divisor of 160) that keep the image within 4096 pixels a
/// side. `None` for an empty surface or frame.
pub fn bezel_layer(
    surface: [u32; 2],
    frame_rect: [f32; 4],
    inks: BezelInks,
) -> Option<(IndexedImage, [f32; 4])> {
    let [x, y, w, _] = frame_rect.map(f64::from);
    let per_frame = w / f64::from(FRAME[0]);
    if per_frame.is_nan() || per_frame <= 0.0 || surface.contains(&0) {
        return None;
    }
    let [sw, sh] = surface.map(f64::from);
    for unit in UNITS {
        let pixel = per_frame * f64::from(unit);
        let frame = FRAME.map(|side| side / unit);
        // Whole bezel pixels left of and above the frame, and right of and
        // below it, covering the surface.
        let before = [(x / pixel).ceil().max(0.0), (y / pixel).ceil().max(0.0)];
        let after = [
            ((sw - x - w) / pixel).ceil().max(0.0),
            ((sh - y - f64::from(frame[1]) * pixel) / pixel)
                .ceil()
                .max(0.0),
        ];
        let size = [
            before[0] as u32 + frame[0] + after[0] as u32,
            before[1] as u32 + frame[1] + after[1] as u32,
        ];
        if size.iter().any(|side| *side > MAX_SIDE) {
            continue;
        }
        let hole = before.map(|value| value as i64);
        let image = draw_bezel(size, hole, unit, inks);
        let rect = [
            (x - before[0] * pixel) as f32,
            (y - before[1] * pixel) as f32,
            (f64::from(size[0]) * pixel) as f32,
            (f64::from(size[1]) * pixel) as f32,
        ];
        return Some((image, rect));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inks() -> BezelInks {
        BezelInks {
            metal: [10, 11, 12, 13, 14, 15],
            groove: 20,
            light: 21,
            amber: 30,
            amber_dark: 31,
        }
    }

    #[test]
    fn the_bezel_is_deterministic_and_frames_the_hole() {
        let first = draw_bezel([960, 540], [160, 60], 1, inks());
        let second = draw_bezel([960, 540], [160, 60], 1, inks());
        assert_eq!(first.pixels(), second.pixels());
        let at = |x: u32, y: u32| first.pixels()[(y * 960 + x) as usize];
        // The frame's own area is the darkest steel; the ring just outside
        // it is the amber inner edge, then the gap.
        assert_eq!(at(160, 60), 10);
        assert_eq!(at(799, 539), 10);
        assert_eq!(at(159, 300), 30);
        assert_eq!(at(800, 300), 30);
        assert_eq!(at(400, 59), 30);
        assert_eq!(at(158, 300), 20);
        // The lip is lit on the left and shadowed on the right.
        assert_eq!(at(160 - 6, 300), 21);
        assert_eq!(at(799 + 6, 300), 10);
        // Corner brackets in amber, none mid-side.
        assert_eq!(at(160 - 9, 70), 30);
        assert_eq!(at(160 - 10, 70), 31);
        assert_ne!(at(160 - 9, 300), 30);
        // The plate uses the steel ramp, seams and rivets only.
        let allowed = [10, 11, 12, 13, 14, 15, 20, 21];
        for y in 0..540 {
            for x in 0..140 {
                assert!(allowed.contains(&at(x, y)), "{x},{y}");
            }
        }
        // Horizontal seams every 120 rows from the frame's top.
        assert_eq!(at(60, 60 + 120), 20);
        assert_eq!(at(60, 60 + 121), 14);
    }

    #[test]
    fn the_brushing_stays_with_the_frame_when_the_window_grows() {
        let small = draw_bezel([900, 500], [130, 20], 1, inks());
        let large = draw_bezel([1100, 600], [230, 70], 1, inks());
        // The same place relative to the frame draws the same.
        for (u, v) in [(-100, 10), (-60, 200), (700, 300), (-20, 470)] {
            let a = small.pixels()[((20 + v) * 900 + 130 + u) as usize];
            let b = large.pixels()[((70 + v) * 1100 + 230 + u) as usize];
            // Only the top-lit gradient differs with the height.
            assert!(
                a == b || (inks().metal.contains(&a) && inks().metal.contains(&b)),
                "{u},{v}"
            );
        }
    }

    #[test]
    fn the_layer_covers_the_surface_on_whole_frame_pixels() {
        // 1920x1080 at the auto interface scale (2.25): the frame spans
        // the height, 240 surface pixels each side.
        let (image, rect) =
            bezel_layer([1920, 1080], [240.0, 0.0, 1440.0, 1080.0], inks()).unwrap();
        assert_eq!([image.width(), image.height()], [640 + 2 * 107, 480]);
        assert!(rect[0] <= 0.0 && rect[0] > -2.25);
        assert_eq!(rect[1], 0.0);
        assert!(rect[0] + rect[2] >= 1920.0);
        // The frame's corner is on a bezel pixel's corner.
        let per = rect[2] / image.width() as f32;
        assert!((per - 2.25).abs() < 1e-4);
        assert!(((240.0 - rect[0]) / per - 107.0).abs() < 1e-3);
        // A tiny interface scale on a huge window uses larger bezel pixels.
        let (image, rect) =
            bezel_layer([7680, 4320], [3520.0, 4080.0, 640.0 * 0.375, 180.0], inks()).unwrap();
        assert!(image.width() <= 4096 && image.height() <= 4096);
        assert!(rect[2] >= 7680.0 - rect[0].abs());
        assert!(bezel_layer([0, 480], [0.0, 0.0, 640.0, 480.0], inks()).is_none());
    }

    #[test]
    fn inks_skip_index_zero_and_the_cycling_range() {
        let mut palette = [[255, 0, 255, 255]; 256];
        palette[0] = [0, 0, 0, 255];
        palette[230] = [212, 148, 40, 255];
        palette[7] = [200, 140, 40, 255];
        palette[9] = [10, 10, 10, 255];
        let inks = BezelInks::nearest(&palette);
        assert_eq!(inks.amber, 7);
        assert_eq!(inks.groove, 9);
        assert!(inks.metal.iter().all(|index| (1..229).contains(index)));
    }
}
