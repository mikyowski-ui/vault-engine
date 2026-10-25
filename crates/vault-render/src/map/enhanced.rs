//! The `enhanced` preset's map layers: the roof cutaway around the cursor
//! ([`RoofCutaway`]) and the shroud over hexes the camera may not reach
//! ([`Shroud`]). Engine extension, not part of any specification; with
//! both [`MapScene`](super::MapScene) fields `None` (Vanilla) nothing here
//! runs.
//!
//! Both dissolve through the standard 4x4 ordered (Bayer) dither anchored
//! to world pixels (view pixels less the grid origin), so the patterns stay
//! put on the map while the camera pans. Everything is integer arithmetic:
//! no rounding differs between platforms.
use super::{MapArt, MapView, RoofCutaway, square_offsets};
use crate::IndexedImage;
use std::{cell::RefCell, rc::Rc};
use vault_formats::{HexGrid, MapFile, frm::FrmFrame, pro::Category};

/// The standard 4x4 ordered-dither (Bayer) matrix, by row then column.
pub const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// The Bayer threshold (0-15) at world pixel `[x, y]`:
/// `BAYER[y & 3][x & 3]`, negative coordinates wrapping as `rem_euclid`.
pub fn bayer([x, y]: [i64; 2]) -> u8 {
    BAYER[(y & 3) as usize][(x & 3) as usize]
}

/// The world pixel under view pixel `point`: the point less the grid
/// origin.
fn world(grid: HexGrid, point: [i64; 2]) -> [i64; 2] {
    [
        point[0].saturating_sub(grid.origin[0]),
        point[1].saturating_sub(grid.origin[1]),
    ]
}

/// `dx² + dy²` of two points, the differences bounded so the sum cannot
/// overflow (a view is far smaller than the bound).
fn distance_squared(a: [i64; 2], b: [i64; 2]) -> i128 {
    const BOUND: i128 = 1 << 40;
    let axis = |axis: usize| (i128::from(a[axis]) - i128::from(b[axis])).clamp(-BOUND, BOUND);
    axis(0) * axis(0) + axis(1) * axis(1)
}

impl RoofCutaway {
    /// Whether a roof pixel at view pixel `point` is drawn. Within `inner`
    /// pixels of the center (Euclidean distance d ≤ inner) it is cut away;
    /// beyond `inner + ring` it is drawn; in the ring, with
    /// t = (d − inner) / ring, it is drawn iff `bayer(world) < floor(16 t)`,
    /// evaluated exactly as `256 d² ≥ (16 inner + (bayer + 1) ring)²`.
    pub fn keeps(self, grid: HexGrid, point: [i64; 2]) -> bool {
        let d2 = distance_squared(point, self.center);
        let inner = i128::from(self.inner);
        if d2 <= inner * inner {
            return false;
        }
        let ring = i128::from(self.ring);
        let outer = inner + ring;
        if d2 > outer * outer {
            return true;
        }
        let threshold = 16 * inner + (i128::from(bayer(world(grid, point))) + 1) * ring;
        256 * d2 >= threshold * threshold
    }

    /// Whether the cutaway can change any pixel of the `size` block at
    /// `origin` (its square of reach overlaps the block).
    pub(super) fn touches(self, origin: [i64; 2], size: [i64; 2]) -> bool {
        let reach = i64::from(self.inner) + i64::from(self.ring);
        (0..2).all(|axis| {
            origin[axis] <= self.center[axis].saturating_add(reach)
                && origin[axis].saturating_add(size[axis]) > self.center[axis].saturating_sub(reach)
        })
    }
}

/// Whether some roof square draws a nonzero pixel at `point`: `None` when a
/// corner leaves the coordinate range (the pick then finds nothing).
pub(super) fn roof_pixel_at(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
) -> Option<bool> {
    let [ox, oy] = view.grid.origin;
    for (square, offset) in square_offsets().iter().enumerate() {
        let tile = map.tile(view.elevation as usize, square)?;
        if tile.roof() != 1
            && !tile.roof_hidden()
            && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.roof()), 0, 0)
            && sample(
                frame,
                [ox.checked_add(offset[0])?, oy.checked_add(offset[1] - 96)?],
                point,
            )
        {
            return Some(true);
        }
    }
    Some(false)
}

/// Whether `frame` drawn at `origin` has a nonzero pixel at `point`.
pub(super) fn sample(frame: &FrmFrame, origin: [i64; 2], point: [i32; 2]) -> bool {
    let x = i64::from(point[0]) - origin[0];
    let y = i64::from(point[1]) - origin[1];
    x >= 0
        && y >= 0
        && x < i64::from(frame.width)
        && y < i64::from(frame.height)
        && frame.pixels()[(y * i64::from(frame.width) + x) as usize] != 0
}

/// Whether a drawn roof pixel is at view pixel `point`: roofs are shown
/// (`view.roofs`), the cutaway (if any) keeps that pixel, and some roof
/// square has a nonzero pixel there. The roof test of
/// [`pick_candidates_cut`](super::pick_candidates_cut) without the player's
/// cutout mask, for pointer rules that ask whether the pointer is over a
/// visible roof. An elevation outside 0-2, or a view past the coordinate
/// range, counts as hidden.
pub fn roof_hides(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutaway: Option<RoofCutaway>,
) -> bool {
    if view.elevation > 2 {
        return true;
    }
    view.roofs
        && cutaway.is_none_or(|cutaway| cutaway.keeps(view.grid, point.map(i64::from)))
        && roof_pixel_at(map, view, art, point).unwrap_or(true)
}

/// The shroud's dither, by Bayer threshold b: a pixel over a hex outside
/// the region darkens iff its distance d from the nearest allowed hex
/// center has `d ≥ 20 + 4 b`, that is `b < floor((d − 16) / 4)`: the band
/// from d = 16 (about a hex's half width, the allowed hex's edge) to
/// d = 80 (two hexes, 64 pixels, beyond it) darkens with t = (d − 16) / 64
/// as the cutaway's ring dissolves, and from 80 on every pixel is dark.
const DARK_FROM: [i64; 16] = {
    let mut squares = [0; 16];
    let mut level = 0;
    while level < 16 {
        let distance = 20 + 4 * level as i64;
        squares[level] = distance * distance;
        level += 1;
    }
    squares
};
/// The distance from which every pixel is dark.
pub(super) const FULLY_DARK: i64 = 80;
const _: () = assert!(DARK_FROM[15] == FULLY_DARK * FULLY_DARK);
/// The farthest any pixel's hex center is from the pixel (the hex cell is
/// 32 by 16 around its center; tested).
pub(super) const CELL_REACH: i64 = 24;
/// Hex centers farther than this from every allowed center have no pixel
/// within [`FULLY_DARK`]: the field records no nearest center for them.
pub(super) const FIELD_REACH: i64 = FULLY_DARK + CELL_REACH;

/// One hex of a [`ShroudField`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShroudCell {
    /// Inside the region: never darkened.
    Allowed,
    /// Outside, with the nearest allowed hex's center (in world pixels,
    /// the grid origin at zero) within reach.
    Near([i32; 2]),
    /// Outside and too far from every allowed hex: fully dark.
    Far,
}

/// The shroud's per-hex distance field: for each hex outside the region,
/// the center of the nearest allowed hex (by center distance, the lowest
/// hex number on a tie) when it is within reach. It depends only on the
/// allowed set, so it is built once per region; `draw_map_scene` keeps
/// the last one it built and rebuilds it only when the allowed set
/// changes (callers drawing with [`draw_shroud`] keep their own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShroudField {
    cells: Vec<ShroudCell>,
}

/// A hex's center in world pixels (the grid origin at zero).
fn hex_center(hex: i32) -> [i64; 2] {
    HexGrid::default().hex_center(hex).expect("hex on the grid")
}

impl ShroudField {
    /// The field for `allowed` (one flag per hex 0-39999; a missing entry
    /// is not allowed).
    pub fn new(allowed: &[bool]) -> Self {
        let inside = |hex: i32| {
            usize::try_from(hex)
                .ok()
                .and_then(|hex| allowed.get(hex))
                .copied()
                .unwrap_or(false)
        };
        // The nearest allowed hex of an outside hex is on the region's
        // edge (one of its neighbors is nearer otherwise), so only the edge
        // hexes spread their distance, over every hex within reach: centers
        // within 104 pixels are at most 8 columns and 9 rows away.
        let centers: Vec<[i64; 2]> = (0..40000).map(hex_center).collect();
        let mut nearest = vec![(i64::MAX, -1_i32); 40000];
        for hex in 0..40000 {
            if !inside(hex) {
                continue;
            }
            let edge = (0..6).any(|orientation| {
                HexGrid::neighbor(hex, orientation)
                    .ok()
                    .flatten()
                    .is_none_or(|next| !inside(next))
            });
            if !edge {
                continue;
            }
            let center = centers[hex as usize];
            let [column, row] = [hex % 200, hex / 200];
            for other_row in (row - 10).max(0)..(row + 11).min(200) {
                for other_column in (column - 10).max(0)..(column + 11).min(200) {
                    let other = other_column + 200 * other_row;
                    if inside(other) {
                        continue;
                    }
                    let [dx, dy] = [0, 1].map(|axis| centers[other as usize][axis] - center[axis]);
                    let d2 = dx * dx + dy * dy;
                    let best = &mut nearest[other as usize];
                    if d2 < FIELD_REACH * FIELD_REACH && d2 < best.0 {
                        *best = (d2, hex);
                    }
                }
            }
        }
        let cells = nearest
            .iter()
            .enumerate()
            .map(|(hex, (_, near))| {
                if inside(hex as i32) {
                    ShroudCell::Allowed
                } else if *near >= 0 {
                    ShroudCell::Near(centers[*near as usize].map(|value| value as i32))
                } else {
                    ShroudCell::Far
                }
            })
            .collect();
        Self { cells }
    }

    /// The cell of `hex` (0-39999).
    pub fn cell(&self, hex: i32) -> Option<ShroudCell> {
        self.cells.get(usize::try_from(hex).ok()?).copied()
    }

    /// Whether the shroud darkens view pixel `point`: its hex
    /// (`grid.pick_hex`) is off the grid or outside the region, and it is
    /// at least `20 + 4 bayer(world)` pixels from the hex's nearest allowed
    /// center ([`ShroudCell::Far`] always).
    pub fn darkens(&self, grid: HexGrid, point: [i64; 2]) -> bool {
        let (Ok(x), Ok(y)) = (i32::try_from(point[0]), i32::try_from(point[1])) else {
            return true;
        };
        let Some(hex) = grid.pick_hex([x, y]) else {
            return true;
        };
        match self.cells[hex as usize] {
            ShroudCell::Allowed => false,
            ShroudCell::Far => true,
            ShroudCell::Near(center) => {
                let at = world(grid, point);
                let d2 = distance_squared(at, center.map(i64::from));
                d2 >= i128::from(DARK_FROM[usize::from(bayer(at))])
            }
        }
    }
}

/// Paint the shroud of `field` over `image` (a view whose grid is `grid`):
/// every pixel [`ShroudField::darkens`] becomes `dark`. Rows are walked in
/// spans of one hex each (at most three per 32-pixel half cell), so a hex
/// inside the region or far outside it costs one lookup and a fill; only
/// the dithered band is decided per pixel.
pub fn draw_shroud(image: &mut IndexedImage, grid: HexGrid, field: &ShroudField, dark: u8) {
    let width = i64::from(image.width());
    let pixels = image.pixels_mut();
    let [ox, oy] = grid.origin;
    if [ox, oy].iter().any(|value| i32::try_from(*value).is_err()) {
        // No hex of the grid is anywhere near the view.
        pixels.fill(dark);
        return;
    }
    for (y, row) in pixels.chunks_exact_mut(width as usize).enumerate() {
        let wy = y as i64 - oy;
        let k = wy.div_euclid(12);
        let ly = wy - 12 * k;
        let bayer_row = &BAYER[(wy & 3) as usize];
        let mut x = 0;
        while x < width {
            let v = x - ox - 16 * k;
            let (b, r) = (v.div_euclid(64), v.rem_euclid(64));
            let lx = r % 32;
            let mut column = 2 * b + i64::from(r >= 32);
            let mut hex_row = b + k;
            // The hex picking rule's top band (the cell's slanted edges).
            let end = if ly < 4 {
                if lx < 16 - 4 * ly {
                    hex_row -= 1;
                    16 - 4 * ly
                } else if lx > 16 + 4 * ly {
                    column += 1;
                    if column.rem_euclid(2) == 1 {
                        hex_row -= 1;
                    }
                    32
                } else {
                    16 + 4 * ly + 1
                }
            } else {
                32
            };
            let next = (x + end - lx).min(width);
            let column = 199 - column;
            let cell = ((0..200).contains(&column) && (0..200).contains(&hex_row))
                .then(|| field.cells[(column + 200 * hex_row) as usize]);
            let span = &mut row[x as usize..next as usize];
            match cell {
                Some(ShroudCell::Allowed) => {}
                None | Some(ShroudCell::Far) => span.fill(dark),
                Some(ShroudCell::Near([cx, cy])) => {
                    let dy = wy - i64::from(cy);
                    for (offset, pixel) in span.iter_mut().enumerate() {
                        let wx = x + offset as i64 - ox;
                        let dx = wx - i64::from(cx);
                        let level = usize::from(bayer_row[(wx & 3) as usize]);
                        if dx * dx + dy * dy >= DARK_FROM[level] {
                            *pixel = dark;
                        }
                    }
                }
            }
            x = next;
        }
    }
}

thread_local! {
    /// The last shroud field built, with the allowed set it was built for.
    static FIELD: RefCell<Option<(Vec<bool>, Rc<ShroudField>)>> = const { RefCell::new(None) };
}

/// The field for `allowed`, reused while the allowed set is unchanged (a
/// comparison of the flags, so the result never depends on the cache).
pub(super) fn cached_field(allowed: &[bool]) -> Rc<ShroudField> {
    FIELD.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((flags, field)) = cache.as_ref()
            && flags.as_slice() == allowed
        {
            return Rc::clone(field);
        }
        let field = Rc::new(ShroudField::new(allowed));
        *cache = Some((allowed.to_vec(), Rc::clone(&field)));
        field
    })
}
