use super::{MapView, roof::Cutout};
use crate::{
    IndexedImage,
    outline::{BlendTable, blend_row, blend_table},
};
use std::sync::OnceLock;
use vault_formats::{Palette, frm::FrmFrame};

/// SPEC-014 §3.44 the light a refresh draws objects and roofs with.
#[derive(Clone, Copy)]
pub struct MapLight<'a> {
    /// The palette whose intensity, blend and gray tables apply.
    pub palette: &'a Palette,
    /// The ambient light (SPEC-006 §3.6).
    pub ambient: i32,
    /// Each hex's light on the current elevation.
    pub tiles: &'a [i64],
}

impl MapLight<'_> {
    /// L, a light value divided by 512 (0-128).
    pub(super) fn level(light: i64) -> u8 {
        (light.clamp(0, 65536) / 512) as u8
    }
    /// The level a hex's objects are lit at: the larger of the ambient
    /// light and the hex's light. An object without a hex is at full light.
    pub(super) fn object(&self, hex: Option<i32>) -> u8 {
        Self::level(hex.map_or(65536, |hex| {
            let tile = usize::try_from(hex)
                .ok()
                .and_then(|hex| self.tiles.get(hex))
                .copied()
                .unwrap_or_default();
            // The hex light is capped at 65536.
            tile.min(65536).max(i64::from(self.ambient))
        }))
    }
}

/// The basic translucency bits `0x4000`-`0x80000` (SPEC-003 §2.10).
pub(super) const TRANSLUCENCY: u32 = 0x4000 | 0x8000 | 0x10000 | 0x20000 | 0x40000 | 0x80000;
/// "Explicitly no translucency".
pub(super) const NO_TRANSLUCENCY: u32 = 0x8000;

/// A blit of `frame`'s nonzero pixels, each replaced by `pixel(source,
/// destination, position)`, clipped to the image.
fn blit_with(
    image: &mut IndexedImage,
    frame: &FrmFrame,
    origin: [i64; 2],
    pixel: impl FnMut(u8, u8, [i64; 2]) -> u8,
) {
    let size = [i64::from(frame.width), i64::from(frame.height)];
    blit_pixels(image, size, frame.pixels(), origin, pixel);
}

/// [`blit_with`] for a `size` block of `source` pixels (an image layer).
fn blit_pixels(
    image: &mut IndexedImage,
    [frame_width, frame_height]: [i64; 2],
    source: &[u8],
    origin: [i64; 2],
    mut pixel: impl FnMut(u8, u8, [i64; 2]) -> u8,
) {
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    let pixels = image.pixels_mut();
    for y in origin[1].max(0)..(origin[1] + frame_height).min(height) {
        for x in origin[0].max(0)..(origin[0] + frame_width).min(width) {
            let color = source[((y - origin[1]) * frame_width + x - origin[0]) as usize];
            if color == 0 {
                continue;
            }
            let target = &mut pixels[(y * width + x) as usize];
            *target = pixel(color, *target, [x, y]);
        }
    }
}

/// SPEC-014 §3.44 the five translucent draws, by their object flag: the
/// foreground color key, the glass gray table (else the common one), and
/// whether the draw is at full light whatever the hex light.
pub(super) const TRANSLUCENT: [(u32, u16, bool, bool); 5] = [
    (0x10000, 25439, false, true),
    (0x20000, 10239, true, false),
    (0x40000, 32767, false, false),
    (0x80000, 30689, false, false),
    (0x4000, 31744, false, false),
];

/// The §3.44 translucent draw an object's flags select: exactly one
/// translucency bit, other than "explicitly no translucency".
pub(super) fn translucent_row(flags: u32) -> Option<usize> {
    let bits = flags & TRANSLUCENCY;
    if bits.count_ones() != 1 {
        return None;
    }
    TRANSLUCENT.iter().position(|(bit, ..)| *bit == bits)
}

/// SPEC-014 §3.44 how a refresh draws its pixels: unlit without a light,
/// otherwise through the light's tables.
pub(super) struct Pens<'a> {
    light: Option<MapLight<'a>>,
    /// The intensity table, indexed `color * 256 + level`.
    intensity: &'a [u8],
    /// The common and glass gray levels of every entry.
    gray: [[u8; 256]; 2],
    /// Each translucent row's blend table, `blend_F[g][D]`, once built.
    blends: [Option<Box<BlendTable>>; 5],
}

impl<'a> Pens<'a> {
    pub(super) fn new(light: Option<MapLight<'a>>) -> Self {
        let gray = light.map_or([[0; 256]; 2], |light| {
            std::array::from_fn(|glass| {
                std::array::from_fn(|index| light.palette.gray_level(index as u8, glass == 1))
            })
        });
        Self {
            light,
            intensity: light.map_or(&[][..], |light| light.palette.intensity_table()),
            gray,
            blends: Default::default(),
        }
    }
    /// The lit draw: indexes 229 and up unchanged, the rest relit at L.
    fn lit(&self, source: u8, level: u8) -> u8 {
        if self.light.is_none() || source >= 229 {
            source
        } else {
            self.intensity[usize::from(source) * 256 + usize::from(level)]
        }
    }
    /// `intensity[source][level]` with no exemption (the masked draw).
    fn relit(&self, source: u8, level: u8) -> u8 {
        if self.light.is_none() {
            source
        } else {
            self.intensity[usize::from(source) * 256 + usize::from(level)]
        }
    }
    /// Draw an object at `level` in its lit or translucent draw.
    pub(super) fn draw(
        &mut self,
        image: &mut IndexedImage,
        frame: &FrmFrame,
        origin: [i64; 2],
        flags: u32,
        level: u8,
    ) {
        let row = translucent_row(flags);
        let (Some(light), Some(row)) = (self.light, row) else {
            blit_with(image, frame, origin, |source, _, _| self.lit(source, level));
            return;
        };
        let (_, key, glass, full) = TRANSLUCENT[row];
        let blend = self.blends[row].get_or_insert_with(|| blend_table(light.palette, key));
        let gray = &self.gray[usize::from(glass)];
        let level = usize::from(if full { 128 } else { level });
        let intensity = self.intensity;
        blit_with(image, frame, origin, |source, destination, _| {
            let mixed = blend[blend_row(gray[usize::from(source)])][usize::from(destination)];
            intensity[usize::from(mixed) * 256 + level]
        });
    }
    /// An image layer drawn like an object without translucency: the lit
    /// draw at `level` of its nonzero pixels.
    pub(super) fn draw_layer(
        &self,
        image: &mut IndexedImage,
        layer: &IndexedImage,
        origin: [i64; 2],
        level: u8,
    ) {
        let size = [i64::from(layer.width()), i64::from(layer.height())];
        blit_pixels(image, size, layer.pixels(), origin, |source, _, _| {
            self.lit(source, level)
        });
    }
    /// A roof square through the `enhanced` cutaway: the pixels `keeps`
    /// accepts are drawn as [`Self::draw_cut`] (with a cutout) or the lit
    /// draw (without) would draw them; the rest leave what lies beneath.
    pub(super) fn draw_roof_peek(
        &self,
        image: &mut IndexedImage,
        frame: &FrmFrame,
        origin: [i64; 2],
        level: u8,
        cutout: Option<(&Cutout, [i64; 2])>,
        keeps: impl Fn([i64; 2]) -> bool,
    ) {
        blit_with(image, frame, origin, |source, destination, at| {
            if !keeps(at) {
                return destination;
            }
            match cutout.and_then(|(cut, mask)| Some((cut, cut.weight(mask, at)?))) {
                Some((cut, weight)) => {
                    cut.palette
                        .mask_blend(self.relit(source, level), destination, weight)
                }
                None => self.lit(source, level),
            }
        });
    }
    /// SPEC-014 §3.37 a draw through the mask at `mask`: inside it each
    /// pixel is relit at `level` and blended by its weight, outside it the
    /// lit draw applies. Indexes 229 and up are relit inside the mask
    /// (the reference rule).
    pub(super) fn draw_cut(
        &self,
        image: &mut IndexedImage,
        frame: &FrmFrame,
        origin: [i64; 2],
        level: u8,
        cut: &Cutout,
        mask: [i64; 2],
    ) {
        blit_with(image, frame, origin, |source, destination, at| {
            match cut.weight(mask, at) {
                Some(weight) => {
                    cut.palette
                        .mask_blend(self.relit(source, level), destination, weight)
                }
                None => self.lit(source, level),
            }
        });
    }
}

/// SPEC-014 §3.44 the floor light's ten vertices: each one's position in
/// the 41-row, 80-column light map (row × 80 + column) and its hex offset
/// from the base hex for an even and an odd base hex.
const FLOOR_VERTICES: [(usize, i32, i32); 10] = [
    (16, -1, -201),
    (48, -2, -2),
    (960, 0, 0),
    (992, 199, -1),
    (1024, 198, 198),
    (1936, 200, 200),
    (1968, 399, 199),
    (2000, 398, 398),
    (2912, 400, 400),
    (2944, 599, 399),
];
/// The light map's size: 41 rows of 80 entries.
pub(super) const FLOOR_MAP: usize = 41 * 80;
/// The five upright triangles as (B, C, A): base left, base right, apex.
const UPRIGHT: [[usize; 3]; 5] = [[2, 3, 0], [3, 4, 1], [5, 6, 3], [6, 7, 4], [8, 9, 6]];
/// The upright triangles' (step, n) row pairs.
const UPRIGHT_ROWS: [(isize, usize); 13] = [
    (-1, 2),
    (78, 2),
    (76, 6),
    (73, 8),
    (71, 10),
    (68, 14),
    (65, 16),
    (63, 18),
    (61, 20),
    (58, 24),
    (55, 26),
    (53, 28),
    (50, 32),
];
/// The five inverted triangles as (T, A, R): top left, bottom apex, top
/// right.
const INVERTED: [[usize; 3]; 5] = [[0, 3, 1], [2, 5, 3], [3, 6, 4], [5, 8, 6], [6, 9, 7]];
/// The inverted triangles' (step, n) row pairs.
const INVERTED_ROWS: [(isize, usize); 13] = [
    (0, 32),
    (48, 32),
    (49, 30),
    (52, 26),
    (55, 24),
    (57, 22),
    (60, 18),
    (63, 16),
    (65, 14),
    (67, 12),
    (70, 8),
    (73, 6),
    (75, 4),
];

/// Fill 13 rows of a triangle into `map`: from cursor `start` and row
/// value `value`, each row advances the cursor by its step, then gives its
/// n entries `value`, `value + dx`, ... (the cursor moving past them), and
/// then `value` grows by `dy`. Every written entry is marked in `written`.
fn fill_rows(
    map: &mut [i32; FLOOR_MAP],
    written: &mut [bool; FLOOR_MAP],
    rows: &[(isize, usize); 13],
    start: usize,
    [mut value, dx, dy]: [i32; 3],
) {
    let mut cursor = start as isize;
    for (step, count) in rows {
        cursor += step;
        for entry in 0..*count as i32 {
            if let Ok(at) = usize::try_from(cursor)
                && at < FLOOR_MAP
            {
                map[at] = value.wrapping_add(entry.wrapping_mul(dx));
                written[at] = true;
            }
            cursor += 1;
        }
        value = value.wrapping_add(dy);
    }
}

/// Fill the light map from the ten vertex lights: the five upright
/// triangles, then the five inverted ones, a later triangle overwriting
/// shared entries. Divisions truncate toward zero.
pub(super) fn fill_floor_map(
    lights: &[i32; 10],
    map: &mut [i32; FLOOR_MAP],
    written: &mut [bool; FLOOR_MAP],
) {
    for [b, c, a] in UPRIGHT {
        let dx = (lights[c] - lights[b]) / 32;
        let dy = (lights[b] - lights[a]) / 13;
        let start = FLOOR_VERTICES[a].0;
        fill_rows(map, written, &UPRIGHT_ROWS, start, [lights[a], dx, dy]);
    }
    for [t, a, r] in INVERTED {
        let dx = (lights[r] - lights[t]) / 32;
        let dy = (lights[a] - lights[t]) / 13;
        let start = FLOOR_VERTICES[t].0;
        fill_rows(map, written, &INVERTED_ROWS, start, [lights[t], dx, dy]);
    }
}

/// The entry each light-map entry is read from. R1 reads the entry itself,
/// and an entry no triangle writes keeps whatever an earlier tile left
/// there (zero at first). `[May-improve]`: an entry no triangle writes
/// reads the nearest written entry above it in the same column (and,
/// with none above, the nearest one below). The written set does not
/// depend on the lights, so this is computed once.
pub(super) fn floor_sources() -> &'static [u16; FLOOR_MAP] {
    static SOURCES: OnceLock<[u16; FLOOR_MAP]> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let mut map = [0; FLOOR_MAP];
        let mut written = [false; FLOOR_MAP];
        fill_floor_map(&[0; 10], &mut map, &mut written);
        std::array::from_fn(|entry| {
            let column = entry % 80;
            let above = (0..entry / 80)
                .rev()
                .map(|row| row * 80 + column)
                .find(|at| written[*at]);
            let below = (entry / 80 + 1..41)
                .map(|row| row * 80 + column)
                .find(|at| written[*at]);
            let source = if written[entry] {
                entry
            } else {
                above.or(below).unwrap_or(entry)
            };
            source as u16
        })
    })
}

/// SPEC-014 §3.44 the floor light of one refresh.
pub(super) struct FloorLight<'a> {
    light: MapLight<'a>,
    intensity: &'a [u8],
    map: Box<[i32; FLOOR_MAP]>,
    written: Box<[bool; FLOOR_MAP]>,
}

impl<'a> FloorLight<'a> {
    pub(super) fn new(light: MapLight<'a>) -> Self {
        Self {
            light,
            intensity: light.palette.intensity_table(),
            map: Box::new([0; FLOOR_MAP]),
            written: Box::new([false; FLOOR_MAP]),
        }
    }
    /// The ten vertex lights of a tile on `base`: each the larger of the
    /// ambient light and its hex's light (capped at 65536), a hex off the
    /// grid giving hex light 0. `[May-improve]`: R1 always uses the
    /// even-hex offsets and, for an odd base hex, skips vertex 0 (keeping
    /// the previous tile's); here all ten are computed, with the odd-hex
    /// offsets for an odd base hex.
    pub(super) fn vertices(&self, base: i32) -> [i32; 10] {
        let odd = base.rem_euclid(2) == 1;
        let ambient = self.light.ambient;
        FLOOR_VERTICES.map(|(_, even_offset, odd_offset)| {
            let hex = base + if odd { odd_offset } else { even_offset };
            let tile = usize::try_from(hex)
                .ok()
                .filter(|hex| *hex < 40000)
                .and_then(|hex| self.light.tiles.get(hex))
                .copied()
                .unwrap_or_default()
                .clamp(0, 65536) as i32;
            tile.max(ambient)
        })
    }
    /// The light of the floor tile whose art's top-left is at `corner`:
    /// `None` when no hex is under its base point (the tile is not drawn),
    /// its common vertex light when all ten agree, else the interpolated
    /// light map, filled into `self.map` for [`Self::level_at`].
    pub(super) fn tile(&mut self, view: MapView, corner: [i64; 2]) -> Option<TileLight> {
        let (Ok(x), Ok(y)) = (i32::try_from(corner[0]), i32::try_from(corner[1] + 13)) else {
            return None;
        };
        let base = view.grid.pick_hex([x, y])?;
        let lights = self.vertices(base);
        if lights.iter().all(|light| *light == lights[0]) {
            return Some(TileLight::Uniform(lights[0]));
        }
        // The map is filled afresh: with the safe rule below no entry is
        // read that this tile's triangles did not write (unless its whole
        // column is unwritten, when it reads 0).
        self.map.fill(0);
        fill_floor_map(&lights, &mut self.map, &mut self.written);
        Some(TileLight::Interpolated)
    }
    /// The level (0-128) of tile pixel `[px, py]` (relative to the tile's
    /// corner) of the interpolated tile [`Self::tile`] last filled.
    pub(super) fn level_at(&self, [px, py]: [i64; 2]) -> u8 {
        let entry = 160 + 80 * py + px;
        let light = usize::try_from(entry)
            .ok()
            .filter(|entry| *entry < FLOOR_MAP)
            .map_or(0, |entry| self.map[usize::from(floor_sources()[entry])]);
        (light / 512).clamp(0, 128) as u8
    }
    /// Draw one floor tile whose art's top-left is at `corner`; whether
    /// it was drawn.
    pub(super) fn draw(
        &mut self,
        image: &mut IndexedImage,
        view: MapView,
        frame: &FrmFrame,
        corner: [i64; 2],
    ) -> bool {
        let intensity = self.intensity;
        match self.tile(view, corner) {
            None => false,
            Some(TileLight::Uniform(light)) => {
                // Uniform: the lit draw, so indexes 229 and up are not lit.
                let level = MapLight::level(i64::from(light));
                blit_with(image, frame, corner, |source, _, _| {
                    if source >= 229 {
                        source
                    } else {
                        intensity[usize::from(source) * 256 + usize::from(level)]
                    }
                });
                true
            }
            Some(TileLight::Interpolated) => {
                blit_with(image, frame, corner, |source, _, [px, py]| {
                    // `[May-improve]`: indexes 229-255 stay unlit, as in a
                    // uniform tile (R1 draws them through their unmapped
                    // intensity rows, as index 0).
                    if source >= 229 {
                        return source;
                    }
                    let level = self.level_at([px - corner[0], py - corner[1]]);
                    intensity[usize::from(source) * 256 + usize::from(level)]
                });
                true
            }
        }
    }
}

/// SPEC-014 §3.44 a floor tile's light: uniform (the common vertex light)
/// or interpolated per pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TileLight {
    Uniform(i32),
    Interpolated,
}
