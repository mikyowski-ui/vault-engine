//! The launcher's live preview of the Enhanced settings: a small scene, drawn
//! procedurally because a shipped launcher has no game data and no fixtures,
//! that answers as a setting changes.
//!
//! The picture is a function of [`PreviewSettings`], the clock [`tick`] set
//! and the rectangle, in whole-number arithmetic, so the same inputs give the
//! same pixels. Everything is drawn straight into the launcher's image inside
//! a clip, with no buffer of its own, and the launcher's palette is the only
//! palette: shades between its entries come from a 4x4 ordered dither,
//! anchored on the rectangle so the pattern stays put as the rectangle is cut.
//!
//! [`tick`]: SettingsPreviewCanvas::tick
use crate::enhanced::{RoofPeek, ViewportMode};
use crate::launcher::{color, font};
use vault_render::IndexedImage;

/// What the preview shows: the settings in play, with the one under the
/// pointer standing in for its saved value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewSettings {
    pub enhanced: bool,
    pub viewport: ViewportMode,
    /// 0.5 to 3.0.
    pub zoom: f32,
    pub roof_peek: RoofPeek,
    pub tactical_overlay: bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            enhanced: false,
            viewport: ViewportMode::Fixed,
            zoom: 1.0,
            roof_peek: RoofPeek::Vanilla,
            tactical_overlay: false,
        }
    }
}

/// What a picture depends on: the settings with those it ignores dropped and
/// the zoom in range, in 256ths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    wide: bool,
    zoom: i64,
    roof: RoofPeek,
    tactical: bool,
}

impl Shown {
    fn of(settings: PreviewSettings) -> Self {
        if !settings.enhanced {
            return Self {
                wide: false,
                zoom: 256,
                roof: RoofPeek::Vanilla,
                tactical: false,
            };
        }
        let zoom = if settings.zoom.is_finite() {
            settings.zoom.clamp(0.5, 3.0)
        } else {
            1.0
        };
        // The fixed viewport keeps the Vanilla frame, where zoom and roof peek
        // do nothing, so the preview does not pretend they do.
        let wide = settings.viewport == ViewportMode::Unlocked;
        Self {
            wide,
            zoom: if wide {
                (zoom * 256.0).round() as i64
            } else {
                256
            },
            roof: if wide {
                settings.roof_peek
            } else {
                RoofPeek::Vanilla
            },
            tactical: settings.tactical_overlay,
        }
    }
}

/// The cutaway's clock: a new position every `STEP_MS`, the sweep's loop in
/// `STEPS` of them.
const STEP_MS: u64 = 40;
const STEPS: u64 = 120;
/// The cutaway's loop over the roof, in scene units: the room's near corner,
/// the critter, the far end, the door.
const SWEEP: [[i64; 2]; 4] = [[-60, -36], [4, -36], [56, -14], [-34, -12]];
/// The cutaway's clear radius and its dissolving ring, in scene units.
const CUT: i64 = 40;
const CUT_RING: i64 = 10;

/// The cutaway's centre at `step` of its loop.
fn sweep(step: u64) -> [i64; 2] {
    let per_leg = STEPS / SWEEP.len() as u64;
    let leg = (step / per_leg) as usize % SWEEP.len();
    let along = (step % per_leg) as i64;
    let [from, to] = [SWEEP[leg], SWEEP[(leg + 1) % SWEEP.len()]];
    [0, 1].map(|axis| from[axis] + (to[axis] - from[axis]) * along / per_leg as i64)
}

/// The preview. It holds no world: only the settings and its clock.
#[derive(Debug, Clone, Default)]
pub struct SettingsPreviewCanvas {
    settings: PreviewSettings,
    now_ms: u64,
    /// A [`set`](Self::set) changed the picture and no tick has said so.
    stale: bool,
}

impl SettingsPreviewCanvas {
    pub fn new() -> Self {
        Self::default()
    }
    /// Show `settings`.
    pub fn set(&mut self, settings: PreviewSettings) {
        self.stale |= Shown::of(settings) != Shown::of(self.settings);
        self.settings = settings;
    }
    /// Advance its animations. Whether it needs drawing again: only the
    /// cutaway moves, so any other picture answers `false` until the next
    /// change of settings.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let before = self.step();
        self.now_ms = now_ms;
        let moved = self.step() != before;
        std::mem::take(&mut self.stale) || moved
    }
    /// The cutaway's step of its loop, 0 for a picture that holds still.
    fn step(&self) -> u64 {
        if Shown::of(self.settings).roof == RoofPeek::CursorXray {
            self.now_ms / STEP_MS % STEPS
        } else {
            0
        }
    }
    /// Draw into `rect` (`[x, y, width, height]`) of `image`, in the launcher's
    /// palette. Nothing outside `rect` changes; a rectangle that is empty or
    /// off the image draws nothing.
    pub fn draw(&self, image: &mut IndexedImage, rect: [i32; 4]) {
        let [x, y, width, height] = rect.map(i64::from);
        if width <= 0 || height <= 0 {
            return;
        }
        let size = [i64::from(image.width()), i64::from(image.height())];
        let bounds = [
            x.max(0),
            y.max(0),
            (x + width).min(size[0]),
            (y + height).min(size[1]),
        ];
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return;
        }
        let shown = Shown::of(self.settings);
        let view = View::new([x, y, width, height], shown.zoom);
        let mut painter = Painter {
            pixels: image.pixels_mut(),
            stride: size[0],
            clip: bounds,
            anchor: [x, y],
            view,
            hole: None,
        };
        painter.block(bounds, Ink::Flat(BEYOND));
        let frame = if shown.wide { view.wide } else { view.classic };
        if frame[1] >= frame[3] {
            return;
        }
        painter.clip = intersect(bounds, frame);
        scene(&mut painter, shown, self.step());
        painter.clip = bounds;
        painter.outline(frame, Ink::Flat(color::BORDER));
        if shown.wide {
            painter.classic_marker();
        }
    }
}

/// The ink outside the camera frame.
const BEYOND: u8 = color::FIELD;
/// The palette's one warm colour: the tin roof, the door and the marks that
/// have to stand out from the greens.
const AMBER: u8 = color::WARN;

fn intersect(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// One scene unit in fixed point.
const ONE: i64 = 1 << 16;
/// The 4:3 frame's height in scene units at zoom 1. A schematic scale, not
/// the game's: the frame stands for the game's 640x480 and has to read in a
/// few hundred pixels.
const FRAME_HEIGHT: i64 = 240;

/// Where the scene lands in a rectangle. The frames share a height, so the
/// scale and the centre do not depend on the viewport: widening adds picture
/// at the sides and moves nothing.
#[derive(Debug, Clone, Copy)]
struct View {
    /// The 16:9 frame, left, top, right and bottom, as large as fits.
    wide: [i64; 4],
    /// The 4:3 frame inside it.
    classic: [i64; 4],
    /// The scene's origin on the image.
    center: [i64; 2],
    /// Image pixels per scene unit, in fixed point.
    scale: i64,
}

impl View {
    fn new([x, y, width, height]: [i64; 4], zoom: i64) -> Self {
        let tall = height.min(width * 9 / 16);
        let center = [x + width / 2, y + height / 2];
        let frame = |wide: i64| {
            let [left, top] = [center[0] - wide / 2, center[1] - tall / 2];
            [left, top, left + wide, top + tall]
        };
        Self {
            wide: frame(tall * 16 / 9),
            classic: frame(tall * 4 / 3),
            center,
            scale: (tall * zoom * (ONE / 256) / FRAME_HEIGHT).max(1),
        }
    }
    /// A scene point on the image.
    fn at(&self, [x, y]: [i64; 2]) -> [i64; 2] {
        [
            self.center[0] + ((x * self.scale) >> 16),
            self.center[1] + ((y * self.scale) >> 16),
        ]
    }
    /// Scene units per image pixel, in fixed point.
    fn step(&self) -> i64 {
        (ONE * ONE) / self.scale
    }
}

/// On the line from `(from, out_from)` to `(to, out_to)`, the second
/// coordinate at `value` of the first.
fn cross(from: i64, to: i64, value: i64, out_from: i64, out_to: i64) -> i64 {
    if from == to {
        return out_from;
    }
    let rise = i128::from(out_to - out_from) * i128::from(value - from);
    out_from + (rise / i128::from(to - from)) as i64
}

/// A 4x4 ordered dither, thresholds 0 to 15.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

#[derive(Debug, Clone, Copy)]
enum Ink {
    Flat(u8),
    /// The first colour with the second through it, in sixteenths.
    Mix(u8, u8, u8),
}

/// A clear circle through what is painted while it is set, ringed by a band
/// that dissolves through the dither. Squared radii, in image pixels.
#[derive(Debug, Clone, Copy)]
struct Hole {
    center: [i64; 2],
    clear: i64,
    edge: i64,
}

impl Hole {
    fn cuts(&self, x: i64, y: i64, threshold: u8) -> bool {
        let [dx, dy] = [x - self.center[0], y - self.center[1]];
        let distance = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
        if distance <= self.clear {
            return true;
        }
        if distance >= self.edge {
            return false;
        }
        let through = (distance - self.clear).saturating_mul(16) / (self.edge - self.clear);
        through < i64::from(threshold)
    }
}

struct Painter<'a> {
    pixels: &'a mut [u8],
    stride: i64,
    /// Left, top, right and bottom, inside the image.
    clip: [i64; 4],
    /// Where the dither starts: the rectangle's corner.
    anchor: [i64; 2],
    view: View,
    hole: Option<Hole>,
}

impl Painter<'_> {
    fn threshold(&self, x: i64, y: i64) -> u8 {
        let [column, row] = [x - self.anchor[0], y - self.anchor[1]];
        BAYER[(row & 3) as usize][(column & 3) as usize]
    }
    /// One row, `from` up to `to`, clipped.
    fn span(&mut self, y: i64, from: i64, to: i64, ink: Ink) {
        let [left, top, right, bottom] = self.clip;
        if y < top || y >= bottom {
            return;
        }
        for x in from.max(left)..to.min(right) {
            let threshold = self.threshold(x, y);
            if self.hole.is_some_and(|hole| hole.cuts(x, y, threshold)) {
                continue;
            }
            self.pixels[(y * self.stride + x) as usize] = match ink {
                Ink::Flat(index) => index,
                Ink::Mix(_, through, amount) if threshold < amount => through,
                Ink::Mix(ground, ..) => ground,
            };
        }
    }
    fn plot(&mut self, x: i64, y: i64, index: u8) {
        self.span(y, x, x + 1, Ink::Flat(index));
    }
    /// A box in image pixels: left, top, right and bottom.
    fn block(&mut self, [left, top, right, bottom]: [i64; 4], ink: Ink) {
        for y in top.max(self.clip[1])..bottom.min(self.clip[3]) {
            self.span(y, left, right, ink);
        }
    }
    /// A box's one-pixel edge, inside it.
    fn outline(&mut self, [left, top, right, bottom]: [i64; 4], ink: Ink) {
        self.block([left, top, right, top + 1], ink);
        self.block([left, bottom - 1, right, bottom], ink);
        self.block([left, top, left + 1, bottom], ink);
        self.block([right - 1, top, right, bottom], ink);
    }
    /// A convex polygon of up to six scene points.
    fn fill(&mut self, points: &[[i64; 2]], ink: Ink) {
        let mut corners = [[0; 2]; 6];
        for (corner, point) in corners.iter_mut().zip(points) {
            *corner = self.view.at(*point);
        }
        let corners = &corners[..points.len().min(6)];
        let lows = corners.iter().map(|corner| corner[1]);
        let highs = corners.iter().map(|corner| corner[1] + 1);
        let top = lows.min().unwrap_or(0).max(self.clip[1]);
        let bottom = highs.max().unwrap_or(0).min(self.clip[3]);
        for y in top..bottom {
            let (mut left, mut right) = (i64::MAX, i64::MIN);
            for (index, a) in corners.iter().enumerate() {
                let b = &corners[(index + 1) % corners.len()];
                if y < a[1].min(b[1]) || y > a[1].max(b[1]) {
                    continue;
                }
                let x = cross(a[1], b[1], y, a[0], b[0]);
                let far = if a[1] == b[1] { b[0] } else { x };
                left = left.min(x).min(far);
                right = right.max(x).max(far);
            }
            if left <= right {
                self.span(y, left, right.saturating_add(1), ink);
            }
        }
    }
    /// A one-pixel line between scene points, walked inside the clip only.
    fn line(&mut self, from: [i64; 2], to: [i64; 2], index: u8) {
        let [a, b] = [self.view.at(from), self.view.at(to)];
        let [left, top, right, bottom] = self.clip;
        if (a[0] - b[0]).abs() >= (a[1] - b[1]).abs() {
            for x in a[0].min(b[0]).max(left)..a[0].max(b[0]).saturating_add(1).min(right) {
                self.plot(x, cross(a[0], b[0], x, a[1], b[1]), index);
            }
        } else {
            for y in a[1].min(b[1]).max(top)..a[1].max(b[1]).saturating_add(1).min(bottom) {
                self.plot(cross(a[1], b[1], y, a[0], b[0]), y, index);
            }
        }
    }
    /// One-bit rows in the launcher font's layout, one image pixel each.
    fn bits(&mut self, [x, y]: [i64; 2], rows: &[u8], index: u8) {
        for (row, bits) in (0..).zip(rows) {
            for column in 0..5 {
                if bits & (0x10 >> column) != 0 {
                    self.plot(x + column, y + row, index);
                }
            }
        }
    }
    /// `text` at image pixels, in the launcher's font.
    fn text(&mut self, [x, y]: [i64; 2], text: &str, index: u8) {
        for (at, letter) in (0..).zip(text.chars()) {
            self.bits(
                [x + at * i64::from(font::ADVANCE), y],
                font::glyph(letter),
                index,
            );
        }
    }
    /// `text` on a dark plate, centred on `x` with the plate's foot at `y`.
    fn tag(&mut self, [x, y]: [i64; 2], text: &str, index: u8) {
        let wide = i64::from(font::width(text));
        let left = x - wide / 2;
        self.block(
            [left - 2, y - 10, left + wide + 1, y],
            Ink::Flat(color::BACKGROUND),
        );
        self.text([left, y - 9], text, index);
    }
    /// The health bar over the head at `head`.
    fn health(&mut self, [x, y]: [i64; 2], percent: i64) {
        const WIDE: i64 = 24;
        self.block([x - 13, y - 7, x + 13, y - 1], Ink::Flat(color::BACKGROUND));
        let [from, level] = [x - WIDE / 2, y - 6];
        self.block(
            [from, level, from + WIDE, level + 4],
            Ink::Flat(color::BORDER),
        );
        let ink = if percent > 50 {
            color::OK
        } else {
            color::ERROR
        };
        let fill = WIDE * percent / 100;
        self.block([from, level, from + fill, level + 4], Ink::Flat(ink));
    }
    /// The 4:3 frame on the wide view: a dashed edge, solid corners, and its
    /// size. It belongs to the camera, not the scene, so zoom leaves it be.
    fn classic_marker(&mut self) {
        const INK: Ink = Ink::Flat(AMBER);
        const DASH: i64 = 10;
        const CORNER: i64 = 10;
        let [left, top, right, bottom] = self.view.classic;
        for x in left.max(self.clip[0])..right.min(self.clip[2]) {
            if (x - left) % DASH < DASH / 2 {
                self.plot(x, top, AMBER);
                self.plot(x, bottom - 1, AMBER);
            }
        }
        for y in top.max(self.clip[1])..bottom.min(self.clip[3]) {
            if (y - top) % DASH < DASH / 2 {
                self.plot(left, y, AMBER);
                self.plot(right - 1, y, AMBER);
            }
        }
        for [x, y, step_x, step_y] in [
            [left, top, 1, 1],
            [right - 1, top, -1, 1],
            [left, bottom - 1, 1, -1],
            [right - 1, bottom - 1, -1, -1],
        ] {
            let across = x + step_x * CORNER;
            let along = y + step_y * CORNER;
            self.block([x.min(across), y, x.max(across), y + 1], INK);
            self.block([x, y.min(along), x + 1, y.max(along)], INK);
        }
        self.tag([left + 26, top + 13], "640x480", AMBER);
    }
}

/// Half a hex's height and the width of its slanted ends, in scene units:
/// hexes 32 wide and 16 tall, their columns 24 apart.
const HEX_HALF: i64 = 8;
const HEX_COLUMN: i64 = 24;

/// A hex's centre, in scene units. Odd columns sit half a hex lower.
const fn hex_center([column, row]: [i64; 2]) -> [i64; 2] {
    let shift = if column.rem_euclid(2) == 1 {
        HEX_HALF
    } else {
        0
    };
    [
        HEX_COLUMN * column + 2 * HEX_HALF,
        2 * HEX_HALF * row + HEX_HALF + shift,
    ]
}

/// The hex under a fixed-point scene point, and whether the point is within
/// `band` of its edge.
fn hex_at(x: i64, y: i64, band: i64) -> ([i64; 2], bool) {
    let [half, wide] = [HEX_HALF * ONE, HEX_COLUMN * ONE];
    let column = x.div_euclid(wide);
    let along = x.rem_euclid(wide);
    let odd = column.rem_euclid(2) == 1;
    let shifted = if odd { y - half } else { y };
    let row = shifted.div_euclid(2 * half);
    let off = shifted.rem_euclid(2 * half) - half;
    if along >= half {
        return ([column, row], half - off.abs() < band);
    }
    if off.abs() <= along {
        return ([column, row], along - off.abs() < band);
    }
    // Past the slant: the column to the left, half a row up or down.
    let neighbour = match (odd, off < 0) {
        (true, true) | (false, false) => row,
        (true, false) => row + 1,
        (false, true) => row - 1,
    };
    ([column - 1, neighbour], off.abs() - along < band)
}

/// The floor's shades, chosen hex by hex.
fn ground_ink([column, row]: [i64; 2]) -> Ink {
    match (column * 7 + row * 13 + (column ^ row)).rem_euclid(7) {
        0 | 3 => Ink::Mix(color::BUTTON, color::HOVER, 8),
        1 => Ink::Mix(color::BUTTON, color::BORDER, 3),
        2 | 5 => Ink::Mix(color::BUTTON, color::PANEL, 5),
        _ => Ink::Flat(color::BUTTON),
    }
}

/// The building's sides along the two ground axes and its heights.
const LONG: i64 = 56;
const SHORT: i64 = 36;
const WALL: i64 = 32;
const RIDGE: i64 = 58;
/// The building's far corner on the ground.
const FAR: [i64; 2] = [-20, -60];

/// A point of the building: `long` down-right, `short` down-left, `up` high.
const fn iso(long: i64, short: i64, up: i64) -> [i64; 2] {
    [FAR[0] + 2 * long - 2 * short, FAR[1] + long + short - up]
}

/// A figure, two scene units a cell, drawn for this preview. `h` hair, `s`
/// skin, `a` and `d` armour, `l` legs, `b` boots.
const FIGURE: [&str; 15] = [
    "..hhhh..", ".hhhhhh.", ".hssssh.", "..ssss..", "...ss...", ".aaaaaa.", "saaddaas", "saaddaas",
    "s.aaaa.s", "s.dddd.s", "..llll..", "..l..l..", "..l..l..", "..l..l..", ".bb..bb.",
];
/// A figure's height in scene units.
const FIGURE_TALL: i64 = 30;
const HOSTILE: [(u8, u8); 6] = [
    (b'h', color::BACKGROUND),
    (b's', color::TEXT),
    (b'a', color::ERROR),
    (b'd', color::BACKGROUND),
    (b'l', color::DIM),
    (b'b', color::BACKGROUND),
];
const ALLY: [(u8, u8); 6] = [
    (b'h', AMBER),
    (b's', color::TEXT),
    (b'a', color::ACCENT),
    (b'd', color::SELECTED),
    (b'l', color::DIM),
    (b'b', color::BACKGROUND),
];

/// A metal toolbox on the floor.
const TOOLBOX: [&str; 6] = [
    "..dddd..", "..d..d..", "tttttttt", "mmmmmmmm", "mmmddmmm", "mmmmmmmm",
];
const TOOLBOX_INKS: [(u8, u8); 3] = [
    (b'd', color::BACKGROUND),
    (b't', color::TEXT),
    (b'm', color::DIM),
];

/// A rusted drum, outside the 4:3 frame.
const DRUM: [&str; 9] = [
    ".ttttt.", "mmmmmmm", "mdmmmdm", "ddddddd", "mdmmmdm", "mdmmmdm", "ddddddd", "mdmmmdm",
    ".mmmmm.",
];
const DRUM_INKS: [(u8, u8); 3] = [
    (b't', color::TEXT),
    (b'm', color::DIM),
    (b'd', color::BORDER),
];

/// A crate in the room.
const CRATE: [&str; 7] = [
    "tttttttt", "mddddddm", "mdmmmmdm", "mdmmmmdm", "mdmmmmdm", "mddddddm", "mmmmmmmm",
];
const CRATE_INKS: [(u8, u8); 3] = [
    (b't', color::DIM),
    (b'm', color::BORDER),
    (b'd', color::HOVER),
];

/// The pointer, in the font's rows.
const ARROW: [u8; 7] = [0x10, 0x18, 0x1C, 0x1E, 0x1C, 0x14, 0x02];

/// Who stands where, feet on the ground in scene units.
const INSIDE: [i64; 2] = [4, -20];
const CRATE_AT: [i64; 2] = iso(10, 8, 0);
const ALLY_AT: [i64; 2] = hex_center([-3, 1]);
const HOSTILE_AT: [i64; 2] = hex_center([3, 2]);
const ITEM_AT: [i64; 2] = hex_center([-1, 4]);
/// Beyond the 4:3 frame, either side.
const DRUMS_AT: [[i64; 2]; 2] = [hex_center([-8, 1]), hex_center([-9, 2])];
const RUBBLE_AT: [i64; 2] = hex_center([7, 0]);
/// The hexes between the ally and the hostile, whose movement costs show.
const STEP_HEXES: [[i64; 2]; 5] = [[-2, 2], [-1, 2], [0, 3], [1, 3], [2, 3]];
const STEP_COSTS: [&str; 5] = ["1", "2", "3", "4", "5"];

/// A sprite's box on the image, feet at `foot`, two scene units a cell.
struct Placed<'a> {
    rows: &'a [&'a str],
    area: [i64; 4],
}

impl<'a> Placed<'a> {
    fn new(view: &View, rows: &'a [&'a str], foot: [i64; 2]) -> Self {
        let [cells, tall] = [
            rows.first().map_or(0, |row| row.len()) as i64,
            rows.len() as i64,
        ];
        let [left, top] = view.at([foot[0] - cells, foot[1] - 2 * tall]);
        let [right, bottom] = view.at([foot[0] + cells, foot[1]]);
        Self {
            rows,
            area: [left, top, right, bottom],
        }
    }
    /// The cell over an image pixel, `.` off the sprite.
    fn cell(&self, x: i64, y: i64) -> u8 {
        let [left, top, right, bottom] = self.area;
        if x < left || y < top || x >= right || y >= bottom {
            return b'.';
        }
        let row = (y - top) * self.rows.len() as i64 / (bottom - top);
        let line = self.rows[row as usize].as_bytes();
        line[((x - left) * line.len() as i64 / (right - left)) as usize]
    }
}

fn sprite(painter: &mut Painter, rows: &[&str], foot: [i64; 2], inks: &[(u8, u8)]) {
    let placed = Placed::new(&painter.view, rows, foot);
    let [left, top, right, bottom] = intersect(placed.area, painter.clip);
    for y in top..bottom {
        for x in left..right {
            let cell = placed.cell(x, y);
            if let Some((_, index)) = inks.iter().find(|(key, _)| *key == cell) {
                painter.plot(x, y, *index);
            }
        }
    }
}

/// A one-pixel ring around a sprite.
fn sprite_outline(painter: &mut Painter, rows: &[&str], foot: [i64; 2], index: u8) {
    let placed = Placed::new(&painter.view, rows, foot);
    let [left, top, right, bottom] = placed.area;
    let [left, top, right, bottom] =
        intersect([left - 1, top - 1, right + 1, bottom + 1], painter.clip);
    let solid = |x, y| placed.cell(x, y) != b'.';
    for y in top..bottom {
        for x in left..right {
            let beside = solid(x - 1, y) || solid(x + 1, y) || solid(x, y - 1) || solid(x, y + 1);
            if beside && !solid(x, y) {
                painter.plot(x, y, index);
            }
        }
    }
}

/// A soft dark patch under something standing at `foot`.
fn shadow(painter: &mut Painter, [x, y]: [i64; 2], reach: i64) {
    painter.fill(
        &[
            [x - reach, y - 1],
            [x - reach / 2, y - 4],
            [x + reach / 2, y - 4],
            [x + reach, y - 1],
            [x + reach / 2, y + 2],
            [x - reach / 2, y + 2],
        ],
        Ink::Mix(color::HOVER, color::BACKGROUND, 10),
    );
}

fn ground(painter: &mut Painter) {
    let step = painter.view.step();
    let band = step / 2;
    // Zoomed far out the hexes crowd together, and solid edges turn to noise.
    let edge_ink = if step > 3 * ONE / 2 {
        Ink::Mix(color::BUTTON, color::PANEL, 8)
    } else {
        Ink::Flat(color::PANEL)
    };
    let [center_x, center_y] = painter.view.center;
    let [left, top, right, bottom] = painter.clip;
    for y in top..bottom {
        let scene_y = (y - center_y) * step + step / 2;
        for x in left..right {
            let scene_x = (x - center_x) * step + step / 2;
            let (hex, edge) = hex_at(scene_x, scene_y, band);
            let ink = if edge { edge_ink } else { ground_ink(hex) };
            painter.span(y, x, x + 1, ink);
        }
    }
}

/// What only the wide view reaches: drums on the left, rubble on the right.
fn props(painter: &mut Painter) {
    for at in DRUMS_AT {
        shadow(painter, at, 12);
        sprite(painter, &DRUM, at, &DRUM_INKS);
    }
    let [x, y] = RUBBLE_AT;
    shadow(painter, [x, y + 2], 26);
    let slab = Ink::Mix(color::BORDER, color::DIM, 6);
    painter.fill(
        &[[x - 22, y], [x - 8, y - 14], [x + 6, y], [x - 6, y + 4]],
        slab,
    );
    painter.fill(
        &[
            [x - 2, y + 2],
            [x + 10, y - 22],
            [x + 24, y],
            [x + 12, y + 6],
        ],
        Ink::Mix(color::HOVER, color::DIM, 5),
    );
    painter.line([x + 10, y - 22], [x + 12, y + 6], color::BORDER);
}

/// The room, seen when the roof is off: its floor, back walls and contents.
fn room(painter: &mut Painter) {
    let boards = Ink::Mix(color::HOVER, color::DIM, 3);
    painter.fill(
        &[
            iso(0, 0, 0),
            iso(LONG, 0, 0),
            iso(LONG, SHORT, 0),
            iso(0, SHORT, 0),
        ],
        boards,
    );
    for long in (8..LONG).step_by(8) {
        painter.line(iso(long, 0, 0), iso(long, SHORT, 0), color::BORDER);
    }
    painter.fill(
        &[
            iso(0, 0, 0),
            iso(LONG, 0, 0),
            iso(LONG, 0, WALL),
            iso(0, 0, WALL),
        ],
        Ink::Mix(color::BORDER, color::DIM, 9),
    );
    painter.fill(
        &[
            iso(0, 0, 0),
            iso(0, SHORT, 0),
            iso(0, SHORT, WALL),
            iso(0, 0, WALL),
        ],
        Ink::Mix(color::SELECTED, color::BORDER, 8),
    );
    painter.line(iso(0, 0, 0), iso(0, 0, WALL), color::DIM);
    sprite(painter, &CRATE, CRATE_AT, &CRATE_INKS);
    shadow(painter, INSIDE, 10);
    sprite(painter, &FIGURE, INSIDE, &HOSTILE);
}

/// The two walls that face the camera, with the door and windows, and the
/// tops of all four.
fn walls(painter: &mut Painter) {
    painter.fill(
        &[
            iso(0, SHORT, 0),
            iso(LONG, SHORT, 0),
            iso(LONG, SHORT, WALL),
            iso(0, SHORT, WALL),
        ],
        Ink::Mix(color::BORDER, color::DIM, 3),
    );
    painter.fill(
        &[
            iso(LONG, SHORT, 0),
            iso(LONG, 0, 0),
            iso(LONG, 0, WALL),
            iso(LONG, SHORT, WALL),
        ],
        Ink::Mix(color::DIM, color::BORDER, 5),
    );
    for long in [6, 42] {
        pane(painter, [long, SHORT], [long + 8, SHORT]);
    }
    pane(painter, [LONG, 22], [LONG, 12]);
    // The door, in the long wall.
    let [from, to] = [22, 34];
    let top = 25;
    painter.fill(
        &[
            iso(from, SHORT, 0),
            iso(to, SHORT, 0),
            iso(to, SHORT, top),
            iso(from, SHORT, top),
        ],
        Ink::Mix(color::PANEL, color::BUTTON, 6),
    );
    for (a, b) in [
        ([from, 0], [from, top]),
        ([from, top], [to, top]),
        ([to, top], [to, 0]),
    ] {
        painter.line(iso(a[0], SHORT, a[1]), iso(b[0], SHORT, b[1]), AMBER);
    }
    let [x, y] = iso(31, SHORT, 11);
    painter.fill(
        &[
            [x - 1, y - 1],
            [x + 1, y - 1],
            [x + 1, y + 1],
            [x - 1, y + 1],
        ],
        Ink::Flat(AMBER),
    );
    // The long wall's foot, and the corner the two faces share.
    painter.line(iso(0, SHORT, 0), iso(LONG, SHORT, 0), color::BACKGROUND);
    painter.line(iso(LONG, SHORT, 0), iso(LONG, 0, 0), color::BACKGROUND);
    painter.line(iso(LONG, SHORT, 0), iso(LONG, SHORT, WALL), color::TEXT);
    let corners = [
        iso(0, 0, WALL),
        iso(LONG, 0, WALL),
        iso(LONG, SHORT, WALL),
        iso(0, SHORT, WALL),
    ];
    for (index, corner) in corners.iter().enumerate() {
        painter.line(*corner, corners[(index + 1) % corners.len()], color::TEXT);
    }
}

/// A window in a wall, `from` and `to` on the ground as `[long, short]`.
fn pane(painter: &mut Painter, from: [i64; 2], to: [i64; 2]) {
    let [low, high] = [12, 23];
    painter.fill(
        &[
            iso(from[0], from[1], low),
            iso(to[0], to[1], low),
            iso(to[0], to[1], high),
            iso(from[0], from[1], high),
        ],
        Ink::Mix(color::BACKGROUND, color::SELECTED, 5),
    );
    painter.line(
        iso(from[0], from[1], high),
        iso(to[0], to[1], high),
        color::BACKGROUND,
    );
    painter.line(
        iso(from[0], from[1], low),
        iso(to[0], to[1], low),
        color::TEXT,
    );
}

/// The hip roof.
fn roof(painter: &mut Painter) {
    let [far, right, near, left] = [
        iso(0, 0, WALL),
        iso(LONG, 0, WALL),
        iso(LONG, SHORT, WALL),
        iso(0, SHORT, WALL),
    ];
    let back = iso(SHORT / 2, SHORT / 2, RIDGE);
    let front = iso(LONG - SHORT / 2, SHORT / 2, RIDGE);
    painter.fill(
        &[far, right, front, back],
        Ink::Mix(color::HOVER, color::DIM, 2),
    );
    painter.fill(&[left, far, back], Ink::Mix(color::BORDER, color::DIM, 2));
    painter.fill(
        &[left, near, front, back],
        Ink::Mix(color::BORDER, AMBER, 3),
    );
    painter.fill(&[near, right, front], Ink::Mix(color::DIM, AMBER, 4));
    // Courses of tin down the long slope.
    for course in 1..4 {
        let between = |eave: [i64; 2], ridge: [i64; 2]| {
            [0, 1].map(|axis| eave[axis] + (ridge[axis] - eave[axis]) * course / 4)
        };
        painter.line(between(left, back), between(near, front), color::SELECTED);
    }
    painter.line(back, front, color::TEXT);
    painter.line(front, near, color::DIM);
    painter.line(near, left, color::BACKGROUND);
    painter.line(near, right, color::BACKGROUND);
}

/// The cutaway at a scene point.
fn hole(view: &View, at: [i64; 2]) -> Hole {
    let radius = |units: i64| (units * view.scale) >> 16;
    let [clear, edge] = [radius(CUT), radius(CUT + CUT_RING)];
    Hole {
        center: view.at(at),
        clear: clear.saturating_mul(clear),
        edge: edge.saturating_mul(edge),
    }
}

/// The top of a figure's head on the image.
fn head(view: &View, [x, y]: [i64; 2]) -> [i64; 2] {
    view.at([x, y - FIGURE_TALL])
}

fn scene(painter: &mut Painter, shown: Shown, step: u64) {
    ground(painter);
    props(painter);
    if shown.tactical {
        for hex in STEP_HEXES {
            let [x, y] = hex_center(hex);
            painter.fill(
                &[
                    [x - 16, y],
                    [x - 8, y - 8],
                    [x + 8, y - 8],
                    [x + 16, y],
                    [x + 8, y + 8],
                    [x - 8, y + 8],
                ],
                Ink::Mix(color::BUTTON, color::ACCENT, 3),
            );
        }
    }
    room(painter);
    walls(painter);
    let cutaway = (shown.roof == RoofPeek::CursorXray).then(|| hole(&painter.view, sweep(step)));
    if shown.roof != RoofPeek::Hidden {
        painter.hole = cutaway;
        roof(painter);
        painter.hole = None;
    }
    shadow(painter, ITEM_AT, 11);
    sprite(painter, &TOOLBOX, ITEM_AT, &TOOLBOX_INKS);
    if shown.tactical {
        sprite_outline(painter, &TOOLBOX, ITEM_AT, AMBER);
    }
    for (at, skin) in [(ALLY_AT, &ALLY), (HOSTILE_AT, &HOSTILE)] {
        shadow(painter, at, 10);
        sprite(painter, &FIGURE, at, skin);
    }
    if shown.tactical {
        floating_marks(painter, shown, cutaway);
    }
    if cutaway.is_some() {
        let [x, y] = painter.view.at(sweep(step));
        painter.bits([x, y], &ARROW, color::TEXT);
    }
}

/// The tactical marks that float over the world.
fn floating_marks(painter: &mut Painter, shown: Shown, cutaway: Option<Hole>) {
    let view = painter.view;
    for (hex, cost) in STEP_HEXES.into_iter().zip(STEP_COSTS) {
        let [x, y] = view.at(hex_center(hex));
        painter.tag([x, y + 5], cost, color::ACCENT);
    }
    let [x, y] = head(&view, HOSTILE_AT);
    painter.health([x, y], 40);
    painter.tag([x, y - 8], "72%", AMBER);
    let [x, y] = head(&view, ALLY_AT);
    painter.health([x, y], 85);
    painter.tag([x, y - 8], "CTRL", color::ACCENT);
    // The critter inside, once the roof lets it be seen.
    let [x, y] = head(&view, INSIDE);
    let seen = match shown.roof {
        RoofPeek::Hidden => true,
        RoofPeek::CursorXray => cutaway.is_some_and(|hole| hole.cuts(x, y, 15)),
        RoofPeek::Vanilla => false,
    };
    if seen {
        painter.health([x, y], 100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [u32; 2] = [480, 270];
    const WHOLE: [i32; 4] = [0, 0, 480, 270];
    /// The step and clock with the cutaway over the critter inside.
    const OVER_CRITTER: u64 = 30 * STEP_MS;

    fn plain() -> PreviewSettings {
        PreviewSettings {
            enhanced: true,
            ..Default::default()
        }
    }
    fn wide() -> PreviewSettings {
        PreviewSettings {
            viewport: ViewportMode::Unlocked,
            ..plain()
        }
    }
    fn view() -> View {
        View::new([0, 0, 480, 270], 256)
    }
    fn canvas(settings: PreviewSettings, now_ms: u64) -> SettingsPreviewCanvas {
        let mut canvas = SettingsPreviewCanvas::new();
        canvas.set(settings);
        canvas.tick(now_ms);
        canvas
    }
    fn picture_at(settings: PreviewSettings, now_ms: u64) -> IndexedImage {
        let mut image = IndexedImage::new(SIZE[0], SIZE[1]).unwrap();
        canvas(settings, now_ms).draw(&mut image, WHOLE);
        image
    }
    fn picture(settings: PreviewSettings) -> IndexedImage {
        picture_at(settings, 0)
    }
    /// Every setting's cases, the cutaway among them.
    fn all_settings() -> Vec<PreviewSettings> {
        let mut all = Vec::new();
        for viewport in [ViewportMode::Fixed, ViewportMode::Unlocked] {
            for zoom in [0.5, 1.0, 3.0] {
                for roof_peek in [RoofPeek::Vanilla, RoofPeek::CursorXray, RoofPeek::Hidden] {
                    for tactical_overlay in [false, true] {
                        all.push(PreviewSettings {
                            enhanced: true,
                            viewport,
                            zoom,
                            roof_peek,
                            tactical_overlay,
                        });
                    }
                }
            }
        }
        all
    }
    fn at(image: &IndexedImage, [x, y]: [i64; 2]) -> u8 {
        image.pixels()[(y * i64::from(image.width()) + x) as usize]
    }
    /// The pixels that differ between two pictures of one size.
    fn changes(a: &IndexedImage, b: &IndexedImage) -> Vec<[i64; 2]> {
        let stride = a.width() as usize;
        let pairs = a.pixels().iter().zip(b.pixels()).enumerate();
        pairs
            .filter(|(_, (a, b))| a != b)
            .map(|(index, _)| [(index % stride) as i64, (index / stride) as i64])
            .collect()
    }
    fn holds([left, top, right, bottom]: [i64; 4], [x, y]: [i64; 2]) -> bool {
        (left..right).contains(&x) && (top..bottom).contains(&y)
    }
    /// How many pixels in `area` are `index`.
    fn count(image: &IndexedImage, area: [i64; 4], index: u8) -> usize {
        let [left, top, right, bottom] = area;
        (top..bottom)
            .flat_map(|y| (left..right).map(move |x| [x, y]))
            .filter(|point| at(image, *point) == index)
            .count()
    }
    /// The middle of the `index` pixels, from the scene's centre.
    fn middle(image: &IndexedImage, index: u8, center: [i64; 2]) -> [f64; 2] {
        let found: Vec<[i64; 2]> = (0..i64::from(SIZE[1]))
            .flat_map(|y| (0..i64::from(SIZE[0])).map(move |x| [x, y]))
            .filter(|point| at(image, *point) == index)
            .collect();
        assert!(!found.is_empty());
        [0, 1].map(|axis| {
            let sum: i64 = found.iter().map(|point| point[axis] - center[axis]).sum();
            sum as f64 / found.len() as f64
        })
    }
    /// A box around scene points on the image, `margin` pixels larger.
    fn around(view: &View, points: &[[i64; 2]], margin: i64) -> [i64; 4] {
        let shown = points.iter().map(|point| view.at(*point));
        let [mut left, mut top] = [i64::MAX; 2];
        let [mut right, mut bottom] = [i64::MIN; 2];
        for [x, y] in shown {
            (left, top) = (left.min(x), top.min(y));
            (right, bottom) = (right.max(x), bottom.max(y));
        }
        [
            left - margin,
            top - margin,
            right + margin + 1,
            bottom + margin + 1,
        ]
    }
    /// The roof's box on the image.
    fn roof_box(view: &View) -> [i64; 4] {
        let corners = [
            iso(0, 0, WALL),
            iso(LONG, 0, WALL),
            iso(LONG, SHORT, WALL),
            iso(0, SHORT, WALL),
            iso(SHORT / 2, SHORT / 2, RIDGE),
            iso(LONG - SHORT / 2, SHORT / 2, RIDGE),
        ];
        around(view, &corners, 1)
    }
    /// The box the `640x480` label takes.
    fn label_box(view: &View) -> [i64; 4] {
        let [left, top, ..] = view.classic;
        [left, top, left + 52, top + 14]
    }

    #[test]
    fn the_same_inputs_give_the_same_pixels() {
        for settings in all_settings() {
            assert!(
                picture_at(settings, OVER_CRITTER).pixels()
                    == picture_at(settings, OVER_CRITTER).pixels(),
                "{settings:?}"
            );
        }
        // What was under the rectangle before does not show.
        let shown = wide();
        let mut dirty = IndexedImage::new(SIZE[0], SIZE[1]).unwrap();
        dirty.pixels_mut().fill(0xEE);
        canvas(shown, 0).draw(&mut dirty, WHOLE);
        assert!(dirty.pixels() == picture(shown).pixels());
    }

    #[test]
    fn a_still_picture_does_not_depend_on_the_clock() {
        let still = PreviewSettings {
            tactical_overlay: true,
            ..wide()
        };
        assert!(picture_at(still, 0).pixels() == picture_at(still, 123_456).pixels());
    }

    #[test]
    fn enhanced_off_ignores_every_other_setting() {
        let vanilla = picture(PreviewSettings::default());
        for settings in all_settings() {
            let off = PreviewSettings {
                enhanced: false,
                ..settings
            };
            assert!(
                picture_at(off, OVER_CRITTER).pixels() == vanilla.pixels(),
                "{off:?}"
            );
        }
        // Enhanced with nothing turned up is the same picture.
        assert!(picture(plain()).pixels() == vanilla.pixels());
    }

    /// The fixed viewport keeps the Vanilla frame in play, so the preview shows
    /// zoom and roof peek doing nothing there.
    #[test]
    fn the_fixed_viewport_shows_neither_zoom_nor_roof_peek() {
        let fixed = picture(plain());
        for settings in [
            PreviewSettings {
                zoom: 2.0,
                ..plain()
            },
            PreviewSettings {
                roof_peek: RoofPeek::Hidden,
                ..plain()
            },
            PreviewSettings {
                roof_peek: RoofPeek::CursorXray,
                ..plain()
            },
        ] {
            assert!(
                changes(&fixed, &picture(settings)).is_empty(),
                "{settings:?}"
            );
        }
    }

    #[test]
    fn widening_adds_picture_at_the_sides_and_a_marker() {
        let view = view();
        let fixed = picture(plain());
        let unlocked = picture(wide());
        let [left, top, right, bottom] = view.classic;
        let inner = [left + 1, top + 1, right - 1, bottom - 1];
        let changed = changes(&fixed, &unlocked);
        let label = label_box(&view);
        for point in &changed {
            assert!(
                !holds(inner, *point) || holds(label, *point),
                "{point:?} moved inside the 4:3 frame"
            );
        }
        // Both sides gained picture: nothing there but the frame's ground before.
        let [wide_left, wide_top, wide_right, wide_bottom] = view.wide;
        let sides = [
            [wide_left + 1, wide_top + 1, left, wide_bottom - 1],
            [right, wide_top + 1, wide_right - 1, wide_bottom - 1],
        ];
        for side in sides {
            let area = ((side[2] - side[0]) * (side[3] - side[1])) as usize;
            assert_eq!(count(&fixed, side, BEYOND), area);
            assert_eq!(count(&unlocked, side, BEYOND), 0);
        }
        // The marker: its four edges, and its size.
        assert!(count(&fixed, label, AMBER) == 0);
        assert!(count(&unlocked, label, AMBER) > 40);
        for edge in [
            [left, top, right, top + 1],
            [left, bottom - 1, right, bottom],
            [left, top, left + 1, bottom],
            [right - 1, top, right, bottom],
        ] {
            assert!(count(&unlocked, edge, AMBER) > 60, "{edge:?}");
            assert_eq!(count(&fixed, edge, AMBER), 0, "{edge:?}");
        }
    }

    #[test]
    fn zoom_scales_the_scene_about_the_centre() {
        let view = view();
        let figure = |zoom| {
            middle(
                &picture(PreviewSettings { zoom, ..wide() }),
                color::ERROR,
                view.center,
            )
        };
        let one = figure(1.0);
        for zoom in [0.5_f32, 2.0] {
            let shown = figure(zoom);
            for (shown, one) in shown.iter().zip(&one) {
                let slip = (shown - one * f64::from(zoom)).abs();
                assert!(slip < 2.0, "zoom {zoom}: {shown} from {one}");
            }
        }
        // The marker stays where it is.
        let closer = picture(PreviewSettings {
            zoom: 2.0,
            ..wide()
        });
        assert!(!changes(&picture(wide()), &closer).is_empty());
        let far = picture(PreviewSettings {
            zoom: 0.5,
            ..wide()
        });
        let near = picture(PreviewSettings {
            zoom: 3.0,
            ..wide()
        });
        let [left, top, right, bottom] = view.classic;
        for y in (top..bottom).filter(|y| (y - top) % 10 < 5) {
            for x in [left, right - 1] {
                assert_eq!(at(&far, [x, y]), AMBER, "{x}, {y}");
                assert_eq!(at(&near, [x, y]), AMBER, "{x}, {y}");
            }
        }
    }

    #[test]
    fn zoom_stays_in_range() {
        let at_zoom = |zoom| picture(PreviewSettings { zoom, ..wide() });
        assert!(at_zoom(9.0).pixels() == at_zoom(3.0).pixels());
        assert!(at_zoom(0.1).pixels() == at_zoom(0.5).pixels());
        assert!(at_zoom(f32::NAN).pixels() == at_zoom(1.0).pixels());
        assert!(at_zoom(f32::INFINITY).pixels() == at_zoom(1.0).pixels());
    }

    #[test]
    fn hidden_takes_the_roof_off_and_touches_nothing_else() {
        let view = view();
        let roofed = picture(wide());
        let open = picture(PreviewSettings {
            roof_peek: RoofPeek::Hidden,
            ..wide()
        });
        let area = roof_box(&view);
        let changed = changes(&roofed, &open);
        assert!(changed.len() > 1000, "{}", changed.len());
        assert!(changed.iter().all(|point| holds(area, *point)));
        // The rust of the roof goes, and the critter inside shows.
        assert!(count(&roofed, area, AMBER) > count(&open, area, AMBER) * 10);
        let inside = Placed::new(&view, &FIGURE, INSIDE).area;
        assert_eq!(count(&roofed, inside, color::ERROR), 0);
        assert!(count(&open, inside, color::ERROR) > 50);
    }

    #[test]
    fn the_cutaway_moves_over_the_roof_and_shows_the_room() {
        let view = view();
        let xray = PreviewSettings {
            roof_peek: RoofPeek::CursorXray,
            ..wide()
        };
        let roofed = picture(wide());
        let inside = Placed::new(&view, &FIGURE, INSIDE).area;
        let reach = ((CUT + CUT_RING) * view.scale) >> 16;
        let mut pictures = Vec::new();
        for step in 0..STEPS {
            let now_ms = step * STEP_MS;
            let image = picture_at(xray, now_ms);
            let [x, y] = view.at(sweep(step));
            let circle = [x - reach - 1, y - reach - 1, x + reach + 2, y + reach + 2];
            let changed = changes(&roofed, &image);
            assert!(!changed.is_empty(), "step {step}");
            assert!(
                changed.iter().all(|point| holds(circle, *point)),
                "step {step}"
            );
            pictures.push(image);
        }
        // Over the critter it shows it; at the far end it does not.
        assert!(count(&pictures[30], inside, color::ERROR) > 50);
        assert_eq!(count(&pictures[60], inside, color::ERROR), 0);
        assert_ne!(pictures[30].pixels(), pictures[31].pixels());
        // The sweep loops.
        let again = picture_at(xray, (STEPS + 30) * STEP_MS);
        assert!(again.pixels() == pictures[30].pixels());
    }

    #[test]
    fn the_tactical_marks_show_where_they_belong_and_only_there() {
        let view = view();
        let bare = picture(wide());
        let marked = picture(PreviewSettings {
            tactical_overlay: true,
            ..wide()
        });
        let marks = |head: [i64; 2]| [head[0] - 15, head[1] - 19, head[0] + 15, head[1]];
        let mut areas = vec![
            marks(super::head(&view, HOSTILE_AT)),
            marks(super::head(&view, ALLY_AT)),
        ];
        let [left, top, right, bottom] = Placed::new(&view, &TOOLBOX, ITEM_AT).area;
        areas.push([left - 1, top - 1, right + 1, bottom + 1]);
        for hex in STEP_HEXES {
            let [x, y] = hex_center(hex);
            areas.push(around(&view, &[[x - 16, y - 8], [x + 16, y + 8]], 1));
        }
        let changed = changes(&bare, &marked);
        for point in &changed {
            assert!(
                areas.iter().any(|area| holds(*area, *point)),
                "{point:?} outside the marks"
            );
        }
        for area in &areas {
            assert!(
                changed.iter().any(|point| holds(*area, *point)),
                "nothing drawn at {area:?}"
            );
        }
        // The bars and tags carry their colours: red and amber over the
        // hostile, green over the ally, amber around the item.
        let hostile = marks(super::head(&view, HOSTILE_AT));
        assert!(count(&marked, hostile, color::ERROR) > count(&bare, hostile, color::ERROR));
        assert!(count(&marked, hostile, AMBER) > 10);
        let ally = marks(super::head(&view, ALLY_AT));
        assert!(count(&marked, ally, color::ACCENT) > 20);
        let item = areas[2];
        assert!(count(&marked, item, AMBER) > 20);
        assert_eq!(count(&bare, item, AMBER), 0);
    }

    #[test]
    fn the_critter_inside_takes_a_health_bar_only_when_it_can_be_seen() {
        let view = view();
        let [x, y] = super::head(&view, INSIDE);
        let bar = [x - 13, y - 7, x + 13, y - 1];
        let tactical = |roof_peek| {
            picture(PreviewSettings {
                tactical_overlay: true,
                roof_peek,
                ..wide()
            })
        };
        assert_eq!(count(&tactical(RoofPeek::Vanilla), bar, color::OK), 0);
        assert!(count(&tactical(RoofPeek::Hidden), bar, color::OK) > 20);
    }

    #[test]
    fn it_stays_inside_the_rectangle() {
        for rect in [
            [40, 30, 200, 120],
            [-60, -40, 200, 120],
            [400, 200, 200, 120],
            [-60, 100, 700, 40],
            [100, -30, 3, 500],
            [10, 10, 1, 1],
            [10, 10, 2, 1],
        ] {
            for settings in all_settings() {
                let mut image = IndexedImage::new(SIZE[0], SIZE[1]).unwrap();
                image.pixels_mut().fill(0xEE);
                canvas(settings, OVER_CRITTER).draw(&mut image, rect);
                let [x, y, width, height] = rect.map(i64::from);
                let area = [x, y, x + width, y + height];
                let stride = SIZE[0] as usize;
                for (index, pixel) in image.pixels().iter().enumerate() {
                    let point = [(index % stride) as i64, (index / stride) as i64];
                    assert_eq!(*pixel != 0xEE, holds(area, point), "{rect:?} {point:?}");
                }
            }
        }
    }

    #[test]
    fn empty_rectangles_and_rectangles_off_the_image_draw_nothing() {
        for rect in [
            [0, 0, 0, 0],
            [10, 10, 0, 100],
            [10, 10, 100, 0],
            [10, 10, -5, 40],
            [10, 10, 40, -5],
            [480, 0, 100, 100],
            [0, 270, 100, 100],
            [-100, 0, 100, 100],
            [0, -100, 100, 100],
            [i32::MAX, i32::MAX, i32::MAX, i32::MAX],
            [i32::MIN, i32::MIN, 1, 1],
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
        ] {
            let mut image = IndexedImage::new(SIZE[0], SIZE[1]).unwrap();
            image.pixels_mut().fill(0xEE);
            canvas(wide(), 0).draw(&mut image, rect);
            assert!(
                image.pixels().iter().all(|pixel| *pixel == 0xEE),
                "{rect:?}"
            );
        }
    }

    #[test]
    fn enormous_rectangles_and_tiny_images_do_not_panic() {
        for settings in all_settings() {
            for size in [[1, 1], [3, 2], [64, 48]] {
                for rect in [
                    [0, 0, i32::MAX, i32::MAX],
                    [-5, -5, i32::MAX, i32::MAX],
                    [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
                    [-1_000_000, -1_000_000, 2_000_000, 2_000_000],
                    [0, 0, 1, 1],
                ] {
                    let mut image = IndexedImage::new(size[0], size[1]).unwrap();
                    canvas(settings, OVER_CRITTER).draw(&mut image, rect);
                }
            }
        }
    }

    #[test]
    fn a_rectangle_cut_by_the_image_draws_the_same_pixels_there() {
        const SHIFT: [i64; 2] = [130, 70];
        for settings in all_settings() {
            let whole = picture_at(settings, OVER_CRITTER);
            let mut cut = IndexedImage::new(200, 100).unwrap();
            canvas(settings, OVER_CRITTER).draw(&mut cut, [-130, -70, 480, 270]);
            for y in 0..100 {
                for x in 0..200 {
                    assert_eq!(
                        at(&cut, [x, y]),
                        at(&whole, [x + SHIFT[0], y + SHIFT[1]]),
                        "{settings:?} at {x}, {y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_tick_is_quiet_when_nothing_moves() {
        for settings in all_settings() {
            if settings.roof_peek == RoofPeek::CursorXray {
                continue;
            }
            let mut canvas = canvas(settings, 0);
            for now_ms in [1, 16, 40, 1000, 90_000] {
                assert!(!canvas.tick(now_ms), "{settings:?}");
            }
        }
        // Enhanced off, the cutaway does not run either.
        let mut off = canvas(
            PreviewSettings {
                enhanced: false,
                roof_peek: RoofPeek::CursorXray,
                ..wide()
            },
            0,
        );
        assert!(!off.tick(STEP_MS * 3));
    }

    #[test]
    fn a_tick_speaks_when_the_cutaway_takes_a_step() {
        let mut canvas = canvas(
            PreviewSettings {
                roof_peek: RoofPeek::CursorXray,
                ..wide()
            },
            0,
        );
        assert!(!canvas.tick(STEP_MS - 1), "still the first step");
        assert!(canvas.tick(STEP_MS));
        assert!(!canvas.tick(STEP_MS), "the same clock");
        assert!(!canvas.tick(2 * STEP_MS - 1));
        assert!(canvas.tick(2 * STEP_MS));
        // A whole loop on, the picture is the one it was.
        assert!(!canvas.tick(2 * STEP_MS + STEPS * STEP_MS));
    }

    #[test]
    fn a_tick_speaks_once_for_a_change_of_settings() {
        let mut canvas = canvas(wide(), 0);
        assert!(!canvas.tick(1));
        canvas.set(wide());
        assert!(!canvas.tick(2), "the same settings");
        canvas.set(PreviewSettings {
            tactical_overlay: true,
            ..wide()
        });
        assert!(canvas.tick(3));
        assert!(!canvas.tick(4));
        // A field the picture ignores is no change.
        let mut off = SettingsPreviewCanvas::new();
        off.set(PreviewSettings {
            zoom: 2.0,
            ..PreviewSettings::default()
        });
        assert!(!off.tick(1));
    }

    #[test]
    fn hexes_tile_the_plane() {
        // A hex's own centre is in it, and off every edge.
        for column in -6..6 {
            for row in -6..6 {
                let [x, y] = hex_center([column, row]);
                let (found, edge) = hex_at(x * ONE, y * ONE, ONE / 2);
                assert_eq!(found, [column, row]);
                assert!(!edge);
                // So is a point just inside each slanted end.
                for side in [-1, 1] {
                    let (found, _) = hex_at((x + side * 15) * ONE, y * ONE, ONE / 2);
                    assert_eq!(found, [column, row], "side {side}");
                }
            }
        }
    }
}
