//! SPEC-003 integer projection and picking. Translation never encodes elevation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GeometryError {
    #[error("hex must be in 0..40000")]
    Hex,
    #[error("orientation must be in 0..6")]
    Orientation,
    #[error("angular distance walk left the grid or failed to converge")]
    AngularWalk,
    #[error("screen translation exceeds coordinate range")]
    ProjectionOverflow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HexGrid {
    pub origin: [i64; 2],
}
impl HexGrid {
    pub fn coordinates(hex: i32) -> Result<[i32; 2], GeometryError> {
        if !(0..40000).contains(&hex) {
            return Err(GeometryError::Hex);
        }
        Ok([hex % 200, hex / 200])
    }
    pub fn neighbor(hex: i32, orientation: u8) -> Result<Option<i32>, GeometryError> {
        let [c, r] = Self::coordinates(hex)?;
        let [dc, dr] = Self::delta(c, orientation)?;
        let [c, r] = [c + dc, r + dr];
        Ok(((0..200).contains(&c) && (0..200).contains(&r)).then_some(c + 200 * r))
    }
    fn delta(column: i32, orientation: u8) -> Result<[i32; 2], GeometryError> {
        let changes = if column % 2 == 0 {
            [[-1, 0], [-1, 1], [0, 1], [1, 1], [1, 0], [0, -1]]
        } else {
            [[-1, -1], [-1, 0], [0, 1], [1, 0], [1, -1], [0, -1]]
        };
        changes
            .get(usize::from(orientation))
            .copied()
            .ok_or(GeometryError::Orientation)
    }
    /// Multi-step movement with the border stop. Under the SPEC-003 §3.4
    /// safe rule an orientation outside 0-5 returns the starting hex.
    pub fn step(mut hex: i32, orientation: u8, count: i32) -> Result<i32, GeometryError> {
        Self::coordinates(hex)?;
        if orientation > 5 {
            return Ok(hex);
        }
        for _ in 0..count.max(0) {
            let [c, r] = Self::coordinates(hex)?;
            if c == 0 || r == 0 || c == 199 || r == 199 {
                break;
            }
            hex = Self::neighbor(hex, orientation)?.ok_or(GeometryError::Hex)?;
        }
        Ok(hex)
    }
    pub fn lattice_distance(a: i32, b: i32) -> Result<i32, GeometryError> {
        if a == -1 || b == -1 {
            return Ok(9999);
        }
        let [ac, ar] = Self::coordinates(a)?;
        let [bc, br] = Self::coordinates(b)?;
        let dq = ac - bc;
        let dr = ar - (ac + 1) / 2 - (br - (bc + 1) / 2);
        Ok(dq.abs().max(dr.abs()).max((dq + dr).abs()))
    }
    /// Hex distance. The -1 sentinel gives 9999; under the SPEC-003 §3.5
    /// safe rule every pair of valid hexes gives the axial lattice distance,
    /// which equals the reference walk wherever that walk stays on the grid.
    pub fn angular_distance(a: i32, b: i32) -> Result<i32, GeometryError> {
        Self::lattice_distance(a, b)
    }
    /// The reference's screen-angle walk, which can leave the grid (for
    /// example from 0 to 199, or from 5225 to 4558) and then has no defined
    /// result; kept to verify that it agrees with the lattice distance.
    pub fn angular_walk(mut a: i32, b: i32) -> Result<i32, GeometryError> {
        if a == -1 || b == -1 {
            return Ok(9999);
        }
        let target = Self::default().hex_reference(b)?;
        for distance in 0..40000 {
            if a == b {
                return Ok(distance);
            }
            let current = Self::default()
                .hex_reference(a)
                .map_err(|_| GeometryError::AngularWalk)?;
            let dx = target[0] - current[0];
            let dy = target[1] - current[1];
            let orientation = if dx == 0 {
                if dy < 0 { 0 } else { 2 }
            } else {
                // The specified constant and truncation are part of this operation.
                let degrees =
                    ((-dy as f64).atan2(dx as f64) * 180.0 * 0.318_309_886_285_112_2) as i32;
                let mut angle = 90 - degrees;
                if angle < 0 {
                    angle += 360;
                }
                (angle / 60).min(5) as u8
            };
            let [c, _] = Self::coordinates(a).map_err(|_| GeometryError::AngularWalk)?;
            let [dc, dr] = Self::delta(c, orientation)?;
            // The recovered walk uses linear offsets, unlike neighbor().
            a += dc + 200 * dr;
        }
        Err(GeometryError::AngularWalk)
    }
    pub fn centered(camera: i32, width: u32, height: u32) -> Result<Self, GeometryError> {
        let reference = Self::default().hex_reference(camera)?;
        Ok(Self {
            origin: [
                (i64::from(width) - 32).div_euclid(2) - reference[0],
                (i64::from(height) - 16).div_euclid(2) - reference[1],
            ],
        })
    }
    pub fn hex_reference(self, hex: i32) -> Result<[i64; 2], GeometryError> {
        let [column, row] = Self::coordinates(hex)?;
        let h = i64::from(199 - column);
        let r = i64::from(row);
        Ok([
            self.origin[0]
                .checked_add(48 * (h / 2) + 32 * (h % 2) + 16 * r)
                .ok_or(GeometryError::ProjectionOverflow)?,
            self.origin[1]
                .checked_add(-12 * (h / 2) + 12 * r)
                .ok_or(GeometryError::ProjectionOverflow)?,
        ])
    }
    pub fn hex_center(self, hex: i32) -> Result<[i64; 2], GeometryError> {
        let [x, y] = self.hex_reference(hex)?;
        Ok([
            x.checked_add(16).ok_or(GeometryError::ProjectionOverflow)?,
            y.checked_add(8).ok_or(GeometryError::ProjectionOverflow)?,
        ])
    }
    pub fn square_corner(self, square: i32, roof: bool) -> Option<[i64; 2]> {
        if !(0..10000).contains(&square) {
            return None;
        }
        let s = i64::from(99 - square % 100);
        let t = i64::from(square / 100);
        Some([
            self.origin[0].checked_add(-16 + 48 * s + 32 * t)?,
            self.origin[1].checked_add(-2 - 12 * s + 24 * t - if roof { 96 } else { 0 })?,
        ])
    }
    pub fn pick_square(self, point: [i32; 2], roof: bool) -> Option<i32> {
        let dx = i128::from(point[0]) - i128::from(self.origin[0]) + 16;
        let dy =
            i128::from(point[1]) - i128::from(self.origin[1]) + 2 - 12 + if roof { 96 } else { 0 };
        let c = 99 - (3 * dx - 4 * dy).div_euclid(192);
        let r = (dx + 4 * dy).div_euclid(128);
        ((0..100).contains(&c) && (0..100).contains(&r)).then_some((c + 100 * r) as i32)
    }
    pub fn pick_hex(self, point: [i32; 2]) -> Option<i32> {
        let y = i128::from(point[1]) - i128::from(self.origin[1]);
        let k = y.div_euclid(12);
        let v = i128::from(point[0]) - i128::from(self.origin[0]) - 16 * k;
        let b = v.div_euclid(64);
        let r = v.rem_euclid(64);
        let mut c = 2 * b + i128::from(r >= 32);
        let mut row = b + k;
        let lx = r % 32;
        let ly = y - 12 * k;
        if ly < 4 {
            if lx < 16 - 4 * ly {
                row -= 1;
            } else if lx > 16 + 4 * ly {
                c += 1;
                if c.rem_euclid(2) == 1 {
                    row -= 1;
                }
            }
        }
        c = 199 - c;
        ((0..200).contains(&c) && (0..200).contains(&row)).then_some((c + 200 * row) as i32)
    }
}
