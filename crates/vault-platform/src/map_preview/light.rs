//! The map view's lighting: each pixel takes its hex's light (at least the
//! ambient light, at most full light 65536), or the ambient light off the
//! grid, as the intensity level `light × 128 / 65536`.
//!
//! The pixel-to-hex mapping only changes with the view, and the levels only
//! with the light, so both are kept: a frame computes one level per visible
//! hex and shades in bulk through the palette's intensity table. A pick
//! depends only on the point minus the grid origin, so a scrolled view
//! moves the previous mapping and picks only the pixels it newly shows.
use vault_formats::{HexGrid, Palette};

#[derive(Debug, Default)]
pub(crate) struct LightCache {
    /// The view the slots were built for: grid, width and height.
    view: Option<(HexGrid, u32, u32)>,
    /// Per pixel, an index into `hexes`, or `hexes.len()` off the grid.
    slots: Vec<u32>,
    /// The hexes the view shows.
    hexes: Vec<i32>,
    /// The level per slot when `levels` was built (the last is off-grid).
    slot_levels: Vec<u8>,
    /// The level per pixel.
    levels: Vec<u8>,
    /// The hex each pixel picks (-1 off the grid), for the next scroll.
    pixel_hexes: Vec<i32>,
    /// Per hex, its slot plus one while a rebuild runs (0: not seen yet).
    slot_of: Vec<u32>,
}

/// The intensity level of a light value.
fn level(light: i32) -> u8 {
    ((light as u32).wrapping_mul(128) / 65536) as u8
}

impl LightCache {
    /// Shade `pixels`, a `width` by `height` view over `grid`, by the light
    /// of each pixel's hex (`tile_light` on the view's elevation).
    pub fn shade(
        &mut self,
        pixels: &mut [u8],
        [width, height]: [u32; 2],
        grid: HexGrid,
        palette: &Palette,
        tile_light: &[i64],
        ambient: i32,
    ) {
        if self.view != Some((grid, width, height)) {
            self.rebuild(grid, width, height);
        }
        let mut slot_levels: Vec<u8> = self
            .hexes
            .iter()
            .map(|hex| {
                level(
                    tile_light
                        .get(*hex as usize)
                        .copied()
                        .unwrap_or_default()
                        .max(i64::from(ambient))
                        .min(65536) as i32,
                )
            })
            .collect();
        slot_levels.push(level(ambient));
        if slot_levels != self.slot_levels {
            self.levels = self
                .slots
                .iter()
                .map(|slot| slot_levels[*slot as usize])
                .collect();
            self.slot_levels = slot_levels;
        }
        palette.shade(pixels, &self.levels);
    }
    fn rebuild(&mut self, grid: HexGrid, width: u32, height: u32) {
        let (w, h) = (width as usize, height as usize);
        // P-02: a view of the same size moved by `shift` takes each pixel
        // it shares with the previous view from there (the pixel at p now
        // picks what p - shift picked), so a scroll step picks only the
        // newly shown strip instead of all 243200 pixels.
        let shift = match self.view {
            Some((old, old_width, old_height))
                if (old_width, old_height) == (width, height)
                    && self.pixel_hexes.len() == w * h =>
            {
                Some([
                    grid.origin[0].wrapping_sub(old.origin[0]),
                    grid.origin[1].wrapping_sub(old.origin[1]),
                ])
            }
            _ => None,
        };
        let previous = std::mem::take(&mut self.pixel_hexes);
        let mut pixel_hexes = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let moved = shift.and_then(|[dx, dy]| {
                    let source = [(x as i64).checked_sub(dx)?, (y as i64).checked_sub(dy)?];
                    ((0..w as i64).contains(&source[0]) && (0..h as i64).contains(&source[1]))
                        .then(|| previous[source[1] as usize * w + source[0] as usize])
                });
                pixel_hexes.push(
                    moved.unwrap_or_else(|| grid.pick_hex([x as i32, y as i32]).unwrap_or(-1)),
                );
            }
        }
        // Slots in first-seen order, through a per-hex table.
        if self.slot_of.len() < 40000 {
            self.slot_of = vec![0; 40000];
        }
        self.hexes.clear();
        self.slots.clear();
        let mut off_grid = Vec::new();
        for (pixel, hex) in pixel_hexes.iter().enumerate() {
            let Ok(hex) = usize::try_from(*hex) else {
                off_grid.push(pixel);
                self.slots.push(0);
                continue;
            };
            if self.slot_of[hex] == 0 {
                self.hexes.push(hex as i32);
                self.slot_of[hex] = self.hexes.len() as u32;
            }
            self.slots.push(self.slot_of[hex] - 1);
        }
        for hex in &self.hexes {
            self.slot_of[*hex as usize] = 0;
        }
        let outside = self.hexes.len() as u32;
        for pixel in off_grid {
            self.slots[pixel] = outside;
        }
        self.pixel_hexes = pixel_hexes;
        self.slot_levels.clear();
        self.view = Some((grid, width, height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Varied colors and a nonzero RGB555 table, with generated intensity.
    fn palette() -> Palette {
        let mut bytes = Vec::with_capacity(0x8300);
        bytes.extend((0..768).map(|index| (index * 5 % 64) as u8));
        bytes.extend((0..32768).map(|index| (index % 255 + 1) as u8));
        Palette::parse(&bytes).unwrap()
    }

    #[test]
    fn cached_levels_shade_like_the_per_pixel_rule() {
        let palette = palette();
        let mut tile_light: Vec<i64> = (0..40000).map(|hex| hex * 7919 % 70000).collect();
        let mut cache = LightCache::default();
        let source: Vec<u8> = (0..640 * 380).map(|index| (index % 256) as u8).collect();
        // Scrolls by a hex or a few (the moved mapping) and a jump with
        // no overlap.
        let cases = [
            (HexGrid::centered(20100, 640, 380).unwrap(), 30000, false),
            (HexGrid::centered(20100, 640, 380).unwrap(), 30000, true),
            (HexGrid::centered(20101, 640, 380).unwrap(), 30000, false),
            (HexGrid::centered(20302, 640, 380).unwrap(), 30000, false),
            (HexGrid::centered(19897, 640, 380).unwrap(), 30000, false),
            (HexGrid::centered(0, 640, 380).unwrap(), 70000, false),
            (HexGrid::centered(205, 640, 380).unwrap(), 70000, false),
        ];
        for (grid, ambient, relight) in cases {
            if relight {
                // The same view under changed light is shaded anew.
                tile_light
                    .iter_mut()
                    .for_each(|light| *light = 65536 - *light);
            }
            let mut expected = source.clone();
            for y in 0..380 {
                for x in 0..640 {
                    let light = grid.pick_hex([x, y]).map_or(ambient, |hex| {
                        tile_light[hex as usize].max(i64::from(ambient)).min(65536) as i32
                    });
                    let index = (y * 640 + x) as usize;
                    if expected[index] != 0 {
                        expected[index] = palette.intensity(expected[index], level(light));
                    }
                }
            }
            let mut shaded = source.clone();
            cache.shade(
                &mut shaded,
                [640, 380],
                grid,
                &palette,
                &tile_light,
                ambient,
            );
            assert_eq!(shaded, expected);
        }
    }
}
