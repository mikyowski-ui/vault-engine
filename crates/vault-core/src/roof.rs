//! SPEC-014 §3.36 roof hiding and restoring: the hex-to-roof-square
//! mapping, the empty test, the edge-connected hide and restore fills, the
//! rule run on every placement of the player, and the restore on leaving a
//! map (SPEC-014 §3.35 step 3). In R1 nothing else turns roofs off.
use crate::world::WorldState;
use vault_formats::map::TilePair;

/// SPEC-003 §2.8 roof flag bits of a tile word: the hide bit suppresses the
/// roof's rendering and picking, the barrier bit stops the fills.
pub const ROOF_HIDDEN: u32 = 0x1000_0000;
pub const ROOF_BARRIER: u32 = 0x2000_0000;
/// The roof art index of an empty square, the blank tile. Any other index
/// counts as a roof, including 0.
pub const BLANK_ROOF: u16 = 1;
/// The roof grid is 100 × 100 squares, each covering a 2 × 2 block of hexes.
pub const GRID: i32 = 100;

/// SPEC-014 §3.36 tracking state: the last roof square, the last elevation
/// and the last-empty flag. "None" is (-1, -1, -1) with last-empty true;
/// removing all objects (unloading or loading a map, a game reset) resets
/// to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoofTracker {
    pub column: i32,
    pub row: i32,
    pub elevation: i32,
    pub last_empty: bool,
}
impl RoofTracker {
    pub const NONE: Self = Self {
        column: -1,
        row: -1,
        elevation: -1,
        last_empty: true,
    };
    /// The last elevation, column and row, when a square was recorded
    /// since the last reset.
    pub fn last(self) -> Option<(u8, i32, i32)> {
        u8::try_from(self.elevation)
            .ok()
            .filter(|elevation| *elevation <= 2)
            .map(|elevation| (elevation, self.column, self.row))
    }
}
impl Default for RoofTracker {
    fn default() -> Self {
        Self::NONE
    }
}

/// SPEC-014 §3.36: the roof square of hex `h` is column `trunc((h mod 200)
/// / 2)`, row `trunc(trunc(h / 200) / 2)`.
pub fn roof_square(hex: i32) -> (i32, i32) {
    ((hex % 200) / 2, (hex / 200) / 2)
}
/// The grid position `column + 100 × row` of a square inside the grid.
pub fn square_position(column: i32, row: i32) -> Option<usize> {
    ((0..GRID).contains(&column) && (0..GRID).contains(&row))
        .then_some((column + GRID * row) as usize)
}
/// SPEC-014 §3.36: a square is empty when its roof art index is exactly 1.
pub fn roof_empty(tile: TilePair) -> bool {
    tile.roof() == BLANK_ROOF
}
/// SPEC-014 §3.36: the nibble of a square is its roof flag bits 28-31 (bit
/// 0 the hide bit, bit 1 the barrier bit).
pub fn roof_nibble(tile: TilePair) -> u32 {
    tile.word >> 28
}
/// SPEC-014 §3.36 hide fill from a square: a square inside the grid, not
/// empty, with neither the hide nor the barrier bit gets its hide bit set,
/// and the fill continues to its four edge neighbors. Returns the number of
/// squares hidden; a start that does not qualify hides nothing.
pub fn hide_fill(tiles: &mut [TilePair], column: i32, row: i32) -> usize {
    fill(
        tiles,
        column,
        row,
        |tile| !roof_empty(tile) && tile.word & (ROOF_HIDDEN | ROOF_BARRIER) == 0,
        |tile| tile.word |= ROOF_HIDDEN,
    )
}
/// SPEC-014 §3.36 restore fill from a square: a square inside the grid, not
/// empty, with its hide bit set gets the bit cleared, and the fill continues
/// to its four edge neighbors. Returns the number of squares restored.
pub fn restore_fill(tiles: &mut [TilePair], column: i32, row: i32) -> usize {
    fill(
        tiles,
        column,
        row,
        |tile| !roof_empty(tile) && tile.word & ROOF_HIDDEN != 0,
        |tile| tile.word &= !ROOF_HIDDEN,
    )
}
/// The edge-connected fill both rules share: the squares that qualify, left,
/// right, up and down from the start, never diagonally. The reference fills
/// recursively and any traversal order gives the same set; this walks an
/// explicit stack, so a large group cannot overflow the call stack.
fn fill(
    tiles: &mut [TilePair],
    column: i32,
    row: i32,
    qualifies: impl Fn(TilePair) -> bool,
    mark: impl Fn(&mut TilePair),
) -> usize {
    let mut count = 0;
    let mut todo = vec![(column, row)];
    while let Some((column, row)) = todo.pop() {
        let Some(tile) = square_position(column, row).and_then(|index| tiles.get_mut(index)) else {
            continue;
        };
        if !qualifies(*tile) {
            continue;
        }
        mark(tile);
        count += 1;
        todo.extend([
            (column - 1, row),
            (column + 1, row),
            (column, row - 1),
            (column, row + 1),
        ]);
    }
    count
}

impl WorldState {
    /// A square's tile pair on an elevation: the blank pair outside the
    /// grid or on an elevation without tile data.
    pub fn roof_tile(&self, elevation: u8, column: i32, row: i32) -> TilePair {
        square_position(column, row)
            .and_then(|index| self.map.tile(usize::from(elevation), index))
            .unwrap_or(TilePair::BLANK)
    }
    fn roof_tiles_mut(&mut self, elevation: u8) -> Option<&mut [TilePair]> {
        self.map
            .tiles
            .get_mut(usize::from(elevation))?
            .as_deref_mut()
    }
    /// The hide fill from a square of an elevation; the number hidden.
    pub fn hide_roofs_from(&mut self, elevation: u8, column: i32, row: i32) -> usize {
        self.roof_tiles_mut(elevation)
            .map_or(0, |tiles| hide_fill(tiles, column, row))
    }
    /// The restore fill from a square of an elevation; the number restored.
    pub fn restore_roofs_from(&mut self, elevation: u8, column: i32, row: i32) -> usize {
        self.roof_tiles_mut(elevation)
            .map_or(0, |tiles| restore_fill(tiles, column, row))
    }
    /// SPEC-014 §3.36 the rule, run every time the player is placed on a
    /// hex (a walking step, a placement, a script move, the entrance
    /// placement of a map load), after that hex's exit grids are checked.
    /// Only the player triggers it.
    ///
    /// 1. The new square S on the new elevation; the same square on the
    ///    same elevation stops.
    /// 2. E is "S is empty", c the nibble of S, p the nibble at the last
    ///    square's position.
    /// 3. If E differs from last-empty, or c from p: when last-empty is
    ///    false, the restore fill runs from the last square; when E is
    ///    false, the hide fill runs from S.
    /// 4. S, the elevation and E become the last ones either way.
    ///
    /// Two `[May-improve]` choices from the section's defects: with no last
    /// square (the first placement after a reset) the reference reads p 101
    /// words before the grid; here only E is compared against last-empty,
    /// which gives the same result. On an elevation change the reference
    /// reads p and restores on the **new** elevation at the old position,
    /// leaving the old elevation's group hidden; here p is read from, and
    /// the restore fill runs on, the elevation where the group was hidden.
    pub fn place_player_roof(&mut self, hex: i32, elevation: u8) {
        let (column, row) = roof_square(hex);
        let last = self.roof;
        if (last.column, last.row, last.elevation) == (column, row, i32::from(elevation)) {
            return;
        }
        let empty = roof_empty(self.roof_tile(elevation, column, row));
        let changed = match last.last() {
            Some((last_elevation, last_column, last_row)) => {
                let current = roof_nibble(self.roof_tile(elevation, column, row));
                let previous = roof_nibble(self.roof_tile(last_elevation, last_column, last_row));
                empty != last.last_empty || current != previous
            }
            None => empty != last.last_empty,
        };
        if changed {
            if !last.last_empty
                && let Some((last_elevation, last_column, last_row)) = last.last()
            {
                self.restore_roofs_from(last_elevation, last_column, last_row);
            }
            if !empty {
                self.hide_roofs_from(elevation, column, row);
            }
        }
        self.roof = RoofTracker {
            column,
            row,
            elevation: i32::from(elevation),
            last_empty: empty,
        };
    }
    /// SPEC-014 §3.35 step 3 and §3.36 leaving a map: if the last square,
    /// read on the player's current elevation, is not empty, the restore
    /// fill runs from it before the map is written. Every player placement
    /// records its elevation, so that is the last recorded elevation and
    /// the group hidden at the last square is restored under either
    /// elevation-change rule; a group the reference's defect leaves hidden
    /// on another elevation is written hidden and drawn again on the next
    /// load. With no last square the reference would read outside the
    /// grid; nothing was hidden, so nothing is restored (`[May-improve]`
    /// safe bound).
    pub fn restore_roofs(&mut self) {
        let Some((_, column, row)) = self.roof.last() else {
            return;
        };
        let elevation = self
            .get(self.player)
            .map_or(self.elevation, |player| player.raw.elevation);
        if !roof_empty(self.roof_tile(elevation, column, row)) {
            self.restore_roofs_from(elevation, column, row);
        }
    }
    /// The grid positions of an elevation's hidden roof squares, in grid
    /// order (diagnostics and tests).
    pub fn hidden_roof_squares(&self, elevation: u8) -> Vec<usize> {
        self.map
            .tiles
            .get(usize::from(elevation))
            .and_then(Option::as_ref)
            .map_or_else(Vec::new, |tiles| {
                tiles
                    .iter()
                    .enumerate()
                    .filter(|(_, tile)| tile.roof_hidden())
                    .map(|(index, _)| index)
                    .collect()
            })
    }
}
