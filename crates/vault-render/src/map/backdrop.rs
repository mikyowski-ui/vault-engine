//! Pre-rendered map backdrops: one painted image per elevation with an
//! optional depth plane (each pixel's place in SPEC-014 §3.44's painter's
//! order) and an optional fade plane (the foreground pixels that fade
//! around the player and the cursor). Owner brief "pre-rendered map
//! backdrops with depth and foreground fade", the drawing half; the files
//! are decoded elsewhere and passed in as a [`BackdropView`].
//!
//! A refresh draws the depth-0 pixels at the end of the Floor layer (one
//! draw item, key `(0, 10000, 0)`), and the pixels of each group hex B as
//! one draw item in the Upright layer after the objects of hex B (key
//! `(2, B, 0xFFFFFFFF)`). Both paths share the pixel selection here, so the
//! reference compositor and the instanced path draw the same bytes.
use super::{
    MapLight, MapScene, MapView,
    lighting::Pens,
    roof::{Cutout, RoofCutaway},
};
use crate::IndexedImage;
use std::collections::BTreeMap;
use vault_formats::{HexGrid, Palette, map::MapObject};

/// The largest depth value: group hex 39999. A tile holding a larger value
/// is not drawn (`[Safe rule]`).
pub const BACKDROP_DEPTH_MAX: u16 = 40000;

/// The decoded backdrop under one refresh, for one elevation.
#[derive(Debug, Clone, Copy)]
pub struct BackdropView<'a> {
    pub elevation: u8,
    /// The origin hex, 0-39999.
    pub origin_hex: i32,
    /// The image pixel of the origin hex's reference corner.
    pub origin_px: [i32; 2],
    /// The whole image, in pixels.
    pub size: [u32; 2],
    /// The decoded chunks; those that do not intersect the refresh may be
    /// absent (a missing chunk draws nothing).
    pub chunks: &'a [BackdropTile<'a>],
    /// Objects of art category 3 (walls) on this elevation are not drawn.
    pub suppress_walls: bool,
    /// Objects with these PIDs on this elevation are not drawn (sorted
    /// ascending).
    pub suppress_pids: &'a [u32],
}

/// One decoded chunk. Planes are row-major; depth is one u16 per pixel
/// (0 floor, 1..=40000 group hex + 1); fade is 1 bit per pixel, rows
/// padded to bytes, bit 7 first.
#[derive(Debug, Clone, Copy)]
pub struct BackdropTile<'a> {
    /// The image pixel of the tile's top-left.
    pub origin: [u32; 2],
    pub size: [u16; 2],
    pub colour: &'a [u8],
    pub depth: Option<&'a [u16]>,
    pub fade: Option<&'a [u8]>,
}

impl BackdropTile<'_> {
    /// The tile's image rectangle `[x0, y0, x1, y1]`.
    fn rect(&self) -> [i64; 4] {
        let [x, y] = self.origin.map(i64::from);
        [
            x,
            y,
            x + i64::from(self.size[0]),
            y + i64::from(self.size[1]),
        ]
    }
    /// The bytes of one fade row.
    fn fade_stride(&self) -> usize {
        usize::from(self.size[0]).div_ceil(8)
    }
    /// Whether every plane's length matches the size, and the tile lies
    /// inside an image of `size`.
    fn shaped(&self, size: [u32; 2]) -> bool {
        let (width, height) = (usize::from(self.size[0]), usize::from(self.size[1]));
        let pixels = width * height;
        pixels > 0
            && self.colour.len() == pixels
            && self.depth.is_none_or(|depth| depth.len() == pixels)
            && self
                .fade
                .is_none_or(|fade| fade.len() == height * self.fade_stride())
            && (0..2).all(|axis| {
                u64::from(self.origin[axis]) + u64::from(self.size[axis]) <= u64::from(size[axis])
            })
    }
    /// Whether every depth value is at most [`BACKDROP_DEPTH_MAX`].
    fn depths_valid(&self) -> bool {
        self.depth
            .is_none_or(|depth| depth.iter().all(|value| *value <= BACKDROP_DEPTH_MAX))
    }
    /// The colour, depth and fade bit of tile pixel (x, y), inside the
    /// tile of a shaped tile.
    fn pixel(&self, x: usize, y: usize) -> (u8, u16, bool) {
        let at = y * usize::from(self.size[0]) + x;
        let depth = self.depth.map_or(0, |depth| depth[at]);
        let fade = self
            .fade
            .is_some_and(|fade| fade[y * self.fade_stride() + x / 8] & (0x80 >> (x % 8)) != 0);
        (self.colour[at], depth, fade)
    }
}

impl BackdropView<'_> {
    /// The canvas position of image pixel (0, 0) in `view`: the origin
    /// hex's reference corner less `origin_px`. `None` when the backdrop is
    /// for another elevation or its origin hex is off the grid; it then
    /// draws nothing and suppresses nothing.
    pub fn canvas_offset(&self, view: &MapView) -> Option<[i64; 2]> {
        if self.elevation != view.elevation {
            return None;
        }
        canvas_offset(view, self.origin_hex, self.origin_px)
    }
    /// The tiles drawn, in order: a tile whose planes do not match its size
    /// or which reaches past the image is skipped, and so is a tile that
    /// overlaps an earlier kept one (chunks of one image never overlap).
    /// `[Safe rule]`
    fn tiles(&self) -> Vec<&BackdropTile<'_>> {
        let mut kept: Vec<&BackdropTile> = Vec::new();
        for tile in self.chunks {
            let rect = tile.rect();
            let overlaps = kept.iter().any(|other| {
                let other = other.rect();
                rect[0] < other[2] && other[0] < rect[2] && rect[1] < other[3] && other[1] < rect[3]
            });
            if tile.shaped(self.size) && !overlaps {
                kept.push(tile);
            }
        }
        kept
    }
    /// The group hex of the backdrop pixel at canvas `point` of `view`,
    /// when one is drawn there (colour not 0, depth at least 1); fading
    /// never uncovers it.
    pub(super) fn group_at(&self, view: MapView, point: [i32; 2]) -> Option<u16> {
        let offset = self.canvas_offset(&view)?;
        let x = i64::from(point[0]).checked_sub(offset[0])?;
        let y = i64::from(point[1]).checked_sub(offset[1])?;
        let tile = self.tiles().into_iter().find(|tile| {
            let rect = tile.rect();
            (rect[0]..rect[2]).contains(&x) && (rect[1]..rect[3]).contains(&y)
        })?;
        if !tile.depths_valid() {
            return None;
        }
        let rect = tile.rect();
        let (colour, depth, _) = tile.pixel((x - rect[0]) as usize, (y - rect[1]) as usize);
        (colour != 0 && depth >= 1).then(|| depth - 1)
    }
}

/// The canvas position of image pixel (0, 0): `R(origin_hex) - origin_px`.
fn canvas_offset(view: &MapView, origin_hex: i32, origin_px: [i32; 2]) -> Option<[i64; 2]> {
    if !(0..40000).contains(&origin_hex) {
        return None;
    }
    let reference = view.grid.hex_reference(origin_hex).ok()?;
    Some([
        reference[0].checked_sub(i64::from(origin_px[0]))?,
        reference[1].checked_sub(i64::from(origin_px[1]))?,
    ])
}

/// The image pixels `[x0, y0, x1, y1]` (half-open) a refresh of `view`
/// draws of a backdrop of `size` pixels placed by `origin_hex` and
/// `origin_px`: the view's canvas (the whole refresh rectangle; the
/// overscan visits more objects but draws no pixel past the canvas) mapped
/// into the image and clipped to it. `None` when the two do not meet or
/// the origin hex is off the grid. The chunk cache decodes the chunks under
/// it ([`backdrop_chunks`]).
pub fn backdrop_rect(
    view: &MapView,
    origin_hex: i32,
    origin_px: [i32; 2],
    size: [u32; 2],
) -> Option<[u32; 4]> {
    let offset = canvas_offset(view, origin_hex, origin_px)?;
    let canvas = [i64::from(view.width), i64::from(view.height)];
    let mut rect = [0_u32; 4];
    for axis in 0..2 {
        let start = 0_i64.saturating_sub(offset[axis]).max(0);
        let end = canvas[axis]
            .saturating_sub(offset[axis])
            .min(i64::from(size[axis]));
        if start >= end {
            return None;
        }
        rect[axis] = start as u32;
        rect[axis + 2] = end as u32;
    }
    Some(rect)
}

/// The chunks (column, row), row by row, that hold the image pixels of
/// `rect` ([`backdrop_rect`]) at chunk size `chunk_size`; none for a chunk
/// size of 0.
pub fn backdrop_chunks(rect: [u32; 4], chunk_size: u32) -> impl Iterator<Item = [u32; 2]> {
    let span = |start: u32, end: u32| {
        if chunk_size == 0 || start >= end {
            0..0
        } else {
            start / chunk_size..(end - 1) / chunk_size + 1
        }
    };
    let (columns, rows) = (span(rect[0], rect[2]), span(rect[1], rect[3]));
    rows.flat_map(move |row| columns.clone().map(move |column| [column, row]))
}

/// The objects a backdrop stops drawing on its elevation (brief §3.8): they
/// keep every world role and their list positions.
#[derive(Clone, Copy)]
pub(super) struct Suppress<'a> {
    walls: bool,
    pids: &'a [u32],
    sorted: bool,
}

impl<'a> Suppress<'a> {
    /// The suppression of `backdrop` in `view`: none without a backdrop that
    /// applies to the view ([`BackdropView::canvas_offset`]).
    pub(super) fn of(backdrop: Option<&BackdropView<'a>>, view: MapView) -> Option<Self> {
        let backdrop = backdrop?;
        backdrop.canvas_offset(&view)?;
        (backdrop.suppress_walls || !backdrop.suppress_pids.is_empty()).then(|| Self {
            walls: backdrop.suppress_walls,
            pids: backdrop.suppress_pids,
            sorted: backdrop.suppress_pids.is_sorted(),
        })
    }
    /// Whether `object` is not drawn: a wall-category art id, or a listed
    /// PID.
    pub(super) fn hides(&self, object: &MapObject) -> bool {
        (self.walls && (object.art_id >> 24) & 0x0f == 3)
            || if self.sorted {
                self.pids.binary_search(&object.pid).is_ok()
            } else {
                self.pids.contains(&object.pid)
            }
    }
}

/// One drawn backdrop pixel on the canvas.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pixel {
    pub x: u32,
    pub y: u32,
    pub colour: u8,
    pub fade: bool,
}

/// One draw item's pixels (the floor pass or one group) and their
/// bounding rectangle `[x0, y0, x1, y1]` on the canvas.
#[derive(Debug, Clone)]
pub(super) struct Group {
    pub rect: [u32; 4],
    pub pixels: Vec<Pixel>,
}

impl Group {
    fn new(pixel: Pixel) -> Self {
        Self {
            rect: [pixel.x, pixel.y, pixel.x + 1, pixel.y + 1],
            pixels: vec![pixel],
        }
    }
    fn push(&mut self, pixel: Pixel) {
        self.rect = [
            self.rect[0].min(pixel.x),
            self.rect[1].min(pixel.y),
            self.rect[2].max(pixel.x + 1),
            self.rect[3].max(pixel.y + 1),
        ];
        self.pixels.push(pixel);
    }
    /// The bounding rectangle's size.
    pub(super) fn size(&self) -> [u32; 2] {
        [self.rect[2] - self.rect[0], self.rect[3] - self.rect[1]]
    }
    /// An image over the bounding rectangle: `value` at each pixel, 0
    /// elsewhere.
    pub(super) fn image(&self, mut value: impl FnMut(&Pixel) -> u8) -> Vec<u8> {
        let [width, height] = self.size();
        let mut image = vec![0; width as usize * height as usize];
        for pixel in &self.pixels {
            let at = (pixel.y - self.rect[1]) as usize * width as usize
                + (pixel.x - self.rect[0]) as usize;
            image[at] = value(pixel);
        }
        image
    }
}

/// The backdrop under one refresh: the floor pass's pixels and each group
/// hex's, every pixel on the canvas with a colour other than 0.
#[derive(Debug, Clone, Default)]
pub(super) struct Layout {
    pub floor: Option<Group>,
    pub groups: BTreeMap<u16, Group>,
}

impl Layout {
    /// The backdrop's pixels on `view`'s canvas; `None` when it does not
    /// apply to the view. A tile with a depth value above 40000 draws
    /// nothing.
    pub(super) fn of(backdrop: &BackdropView, view: MapView) -> Option<Self> {
        let offset = backdrop.canvas_offset(&view)?;
        let canvas = [i64::from(view.width), i64::from(view.height)];
        let mut layout = Self::default();
        for tile in backdrop.tiles() {
            let rect = tile.rect();
            let start = [0, 1].map(|axis| rect[axis].saturating_add(offset[axis]).max(0));
            let end = [0, 1].map(|axis| {
                rect[axis + 2]
                    .saturating_add(offset[axis])
                    .min(canvas[axis])
            });
            if start[0] >= end[0] || start[1] >= end[1] || !tile.depths_valid() {
                continue;
            }
            for y in start[1]..end[1] {
                let ty = (y - offset[1] - rect[1]) as usize;
                for x in start[0]..end[0] {
                    let tx = (x - offset[0] - rect[0]) as usize;
                    let (colour, depth, fade) = tile.pixel(tx, ty);
                    if colour == 0 {
                        continue;
                    }
                    let pixel = Pixel {
                        x: x as u32,
                        y: y as u32,
                        colour,
                        fade,
                    };
                    if depth == 0 {
                        match &mut layout.floor {
                            Some(floor) => floor.push(pixel),
                            None => layout.floor = Some(Group::new(pixel)),
                        }
                    } else {
                        layout
                            .groups
                            .entry(depth - 1)
                            .and_modify(|group| group.push(pixel))
                            .or_insert_with(|| Group::new(pixel));
                    }
                }
            }
        }
        Some(layout)
    }
}

/// The level a floor-pass pixel at canvas `[x, y]` is lit at: as an object
/// on the hex under it, or at the ambient light off the grid (128, unused,
/// without a light).
pub(super) fn floor_level(light: Option<MapLight>, grid: HexGrid, [x, y]: [u32; 2]) -> u8 {
    light.map_or(128, |light| match grid.pick_hex([x as i32, y as i32]) {
        Some(hex) => light.object(Some(hex)),
        None => MapLight::level(i64::from(light.ambient)),
    })
}

/// The level a group's pixels are lit at: as an object on its hex.
pub(super) fn group_level(light: Option<MapLight>, hex: u16) -> u8 {
    light.map_or(128, |light| light.object(Some(i32::from(hex))))
}

/// How one foreground-eligible group pixel draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shade {
    /// Dissolved by the cursor fade: not drawn.
    Removed,
    /// The lit draw.
    Opaque,
    /// Through the player's mask with this weight.
    Masked(u8),
}

/// The fade rules of one refresh (brief §3.6).
pub(super) struct Fade<'a> {
    grid: HexGrid,
    /// The player's hex, the cutout and the mask's corner, when the cutout
    /// is placed and the player is not hidden.
    player: Option<(i32, &'a Cutout<'a>, [i64; 2])>,
    /// The hex under the cutaway's centre and the cutaway.
    cursor: Option<(i32, RoofCutaway)>,
}

impl<'a> Fade<'a> {
    pub(super) fn new(view: MapView, scene: &MapScene<'a>) -> Self {
        let player = scene
            .cutout
            .filter(|cut| !cut.hidden)
            .and_then(|cut| Some((cut.hex, cut, cut.origin(view)?)));
        let cursor = scene.roof_cutaway.and_then(|cutaway| {
            let center = [
                i32::try_from(cutaway.center[0]).ok()?,
                i32::try_from(cutaway.center[1]).ok()?,
            ];
            Some((view.grid.pick_hex(center)?, cutaway))
        });
        Self {
            grid: view.grid,
            player,
            cursor,
        }
    }
    /// How a pixel of group `hex` at canvas `at` draws: a foreground pixel
    /// (fade bit set) of a group past the cursor's hex that the cutaway
    /// removes is not drawn; one of a group past the player's hex inside
    /// the mask's rectangle is drawn through it; everything else is opaque.
    pub(super) fn shade(&self, hex: u16, fade: bool, at: [i64; 2]) -> Shade {
        if !fade {
            return Shade::Opaque;
        }
        let hex = i32::from(hex);
        if let Some((cursor, cutaway)) = self.cursor
            && hex > cursor
            && !cutaway.keeps(self.grid, at)
        {
            return Shade::Removed;
        }
        if let Some((player, cut, origin)) = self.player
            && hex > player
            && let Some(weight) = cut.weight(origin, at)
        {
            return Shade::Masked(weight);
        }
        Shade::Opaque
    }
    /// The palette the masked draw blends with.
    fn palette(&self) -> Option<&'a Palette> {
        self.player.map(|(_, cut, _)| cut.palette)
    }
}

/// Write `value(destination)` at canvas `[x, y]`, inside the image.
fn put(image: &mut IndexedImage, [x, y]: [u32; 2], value: impl FnOnce(u8) -> u8) {
    let (width, height) = (image.width(), image.height());
    if x < width && y < height {
        let pixel = &mut image.pixels_mut()[y as usize * width as usize + x as usize];
        *pixel = value(*pixel);
    }
}

/// The reference path's floor pass: each pixel lit as an object on the hex
/// under it (indexes 229-255 unlit).
pub(super) fn draw_floor(
    image: &mut IndexedImage,
    floor: &Group,
    pens: &Pens,
    light: Option<MapLight>,
    grid: HexGrid,
) {
    for pixel in &floor.pixels {
        let at = [pixel.x, pixel.y];
        let level = floor_level(light, grid, at);
        put(image, at, |_| pens.lit(pixel.colour, level));
    }
}

/// The reference path's draw of the group of hex `hex`: each pixel lit as
/// an object on that hex, opaque, or through the player's mask with
/// SPEC-014 §3.37's formula (relit with no exemption), or not at all where
/// the cursor fade removes it.
pub(super) fn draw_group(
    image: &mut IndexedImage,
    hex: u16,
    group: &Group,
    pens: &Pens,
    light: Option<MapLight>,
    fade: &Fade,
) {
    let level = group_level(light, hex);
    for pixel in &group.pixels {
        let at = [pixel.x, pixel.y];
        match fade.shade(hex, pixel.fade, at.map(i64::from)) {
            Shade::Removed => {}
            Shade::Opaque => put(image, at, |_| pens.lit(pixel.colour, level)),
            Shade::Masked(weight) => {
                let palette = fade.palette().expect("a masked pixel has a cutout");
                put(image, at, |destination| {
                    palette.mask_blend(pens.relit(pixel.colour, level), destination, weight)
                });
            }
        }
    }
}
