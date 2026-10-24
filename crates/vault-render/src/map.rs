use crate::{
    ImageError, IndexedImage,
    floating::FloatingSprite,
    outline::{BlendTable, OutlineInks, blend_table, draw_outline},
};
use std::sync::OnceLock;
use vault_formats::{
    HexGrid, MapFile, Palette,
    frm::FrmFrame,
    geometry::GeometryError,
    map::{MapObject, MapObjects},
    pro::{BasicFlags, Category},
};

pub trait MapArt {
    fn frame(
        &self,
        category: Category,
        art_id: u32,
        orientation: u8,
        frame: usize,
    ) -> Option<(&FrmFrame, [i16; 2])>;
}
#[derive(Debug, thiserror::Error)]
pub enum MapRenderError {
    #[error(transparent)]
    Image(#[from] ImageError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("elevation must be 0, 1, or 2")]
    Elevation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapHover {
    pub elevation: u8,
    pub hex: Option<i32>,
    pub floor: Option<i32>,
    pub roof: Option<i32>,
}
#[derive(Debug, Clone, Copy)]
pub struct MapView {
    pub grid: HexGrid,
    pub elevation: u8,
    pub roofs: bool,
    pub width: u32,
    pub height: u32,
}
impl MapView {
    pub fn new(map: &MapFile, width: u32, height: u32) -> Result<Self, MapRenderError> {
        Ok(Self {
            grid: HexGrid::centered(map.header.entry_hex, width, height)?,
            elevation: map.header.entry_elevation,
            roofs: true,
            width,
            height,
        })
    }
    pub fn pan(&mut self, x: i32, y: i32) {
        for (axis, delta) in [x, y].into_iter().enumerate() {
            self.grid.origin[axis] = self.grid.origin[axis]
                .saturating_add(i64::from(delta))
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
        }
    }
    pub fn hover(self, point: [i32; 2]) -> MapHover {
        MapHover {
            elevation: self.elevation,
            hex: self.grid.pick_hex(point),
            floor: self.grid.pick_square(point, false),
            roof: self.grid.pick_square(point, true),
        }
    }
}
/// The squares in drawing order, by corner row, then column, then index,
/// each with its floor corner's offset from the grid origin. A corner is the
/// grid origin plus that fixed offset, so the order and the offsets are the
/// same for every view and are computed once (P-02: a frame only adds its
/// origin instead of recomputing 10000 corners).
fn square_order() -> &'static [(i32, [i64; 2])] {
    static ORDER: OnceLock<Vec<(i32, [i64; 2])>> = OnceLock::new();
    ORDER.get_or_init(|| {
        let grid = HexGrid { origin: [0, 0] };
        let mut squares: Vec<(i32, [i64; 2])> = (0..10000)
            .map(|index| {
                let corner = grid.square_corner(index, false).expect("square in range");
                (index, corner)
            })
            .collect();
        squares.sort_by_key(|(index, [x, y])| (*y, *x, *index));
        squares
    })
}
/// The floor corners of every square by index, offset from the grid
/// origin (the roof corner is 96 pixels higher).
fn square_offsets() -> &'static [[i64; 2]] {
    static OFFSETS: OnceLock<Vec<[i64; 2]>> = OnceLock::new();
    OFFSETS.get_or_init(|| {
        let mut offsets = vec![[0; 2]; 10000];
        for (index, offset) in square_order() {
            offsets[*index as usize] = *offset;
        }
        offsets
    })
}
/// `origin + offset`; the origin is within the i32 range (checked by the
/// callers) and the offsets are small, so the sum cannot overflow.
fn corner(origin: [i64; 2], offset: [i64; 2]) -> [i64; 2] {
    [origin[0] + offset[0], origin[1] + offset[1]]
}

pub fn render_map(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
) -> Result<IndexedImage, MapRenderError> {
    render_map_with(map, &map.objects, view, art)
}

/// [`render_map`] drawing `objects` over `map`'s squares in place of the
/// map's own object table (P-02: a live world passes its current objects
/// without cloning the map file).
pub fn render_map_with(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
) -> Result<IndexedImage, MapRenderError> {
    render_map_cut(map, objects, view, art, None)
}

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
    fn level(light: i64) -> u8 {
        (light.clamp(0, 65536) / 512) as u8
    }
    /// The level a hex's objects are lit at: the larger of the ambient
    /// light and the hex's light. An object without a hex is at full light.
    fn object(&self, hex: Option<i32>) -> u8 {
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

/// SPEC-014 §3.44 what a refresh draws besides the floor: the cutout,
/// the light (none draws every pixel unlit) and the outlines (none draws
/// no outline): each drawn object's outline word selects its ink.
#[derive(Clone, Copy, Default)]
pub struct MapScene<'a> {
    pub cutout: Option<&'a Cutout<'a>>,
    pub light: Option<MapLight<'a>>,
    pub outlines: Option<&'a OutlineInks>,
    /// SPEC-014 §3.46 the floating texts, in creation order.
    pub floating: &'a [FloatingSprite],
    /// SPEC-014 §3.27 the in-world cursor's objects, when it shows.
    pub cursor: Option<SceneCursor<'a>>,
}

/// SPEC-014 §3.27 the in-world cursor: a flat object on the current
/// elevation (flat, no-block, shoot-through, light-through, temporary)
/// that shows every mode's art, drawn in the §3.44 flat pass at its hex
/// and lit as any object there, and in movement mode a second, upright
/// object for the AP text, drawn in the upright pass at the same hex. Both
/// exist only in the refresh: they are not world objects, so scripts,
/// saves, picking and blocking never see them.
#[derive(Clone, Copy)]
pub struct SceneCursor<'a> {
    /// The hex both objects are at.
    pub hex: i32,
    /// Their place in the elevation's object list (insertion order): they
    /// follow the objects listed before index `order` and precede the rest,
    /// so within their hex groups they come after the objects inserted
    /// before their last move (every move reinserts them at the end).
    pub order: usize,
    /// The cursor art and its top-left corner in the view (index 0 is
    /// transparent).
    pub art: (&'a IndexedImage, [i64; 2]),
    /// Movement mode's outline: the hex frame at its top-left corner and
    /// its outline word, added to the outline list when the cursor is
    /// drawn, so it is painted after the roofs like every outline.
    pub outline: Option<(&'a FrmFrame, [i64; 2], u32)>,
    /// The AP-text object's art and top-left corner.
    pub text: Option<(&'a IndexedImage, [i64; 2])>,
}

/// One entry of a refresh's drawing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drawn {
    /// An object, by its index in the elevation's list.
    Object(usize),
    /// The in-world cursor (flat pass).
    Cursor,
    /// Movement mode's AP text (upright pass).
    CursorText,
}

/// SPEC-014 §3.37 the player see-through cutout: the `egg.frm` mask placed
/// on the player, through which roofs and the walls and scenery that would
/// hide the player are drawn partly transparent.
#[derive(Clone, Copy)]
pub struct Cutout<'a> {
    /// The mask frame (129 by 98); each pixel is a weight 0-122.
    pub mask: &'a FrmFrame,
    /// The player's hex and current pixel displacement.
    pub hex: i32,
    pub displacement: [i32; 2],
    /// A hidden player cuts roofs only.
    pub hidden: bool,
    /// A prototype's extended flags by PID (0 when unknown).
    pub extended: &'a dyn Fn(u32) -> u32,
    pub palette: &'a Palette,
}

/// The basic translucency bits `0x4000`-`0x80000` (SPEC-003 §2.10).
const TRANSLUCENCY: u32 = 0x4000 | 0x8000 | 0x10000 | 0x20000 | 0x40000 | 0x80000;
/// "Explicitly no translucency".
const NO_TRANSLUCENCY: u32 = 0x8000;
/// The basic wall-transparency end marker.
const TRANSPARENCY_END: u32 = 0x1000_0000;
/// The orientation constant, `0x3FF5555555555556` (not the nearest double
/// to 4/3).
const K: f64 = f64::from_bits(0x3FF5_5555_5555_5556);

impl Cutout<'_> {
    /// The mask's top-left corner in the view: the player's hex center plus
    /// its displacement, less (64, 72).
    fn origin(&self, view: MapView) -> Option<[i64; 2]> {
        let [x, y] = view.grid.hex_center(self.hex).ok()?;
        Some([
            x + i64::from(self.displacement[0]) - 64,
            y + i64::from(self.displacement[1]) - 72,
        ])
    }
    /// The mask weight at a view pixel, or `None` outside the mask.
    fn weight(&self, origin: [i64; 2], [x, y]: [i64; 2]) -> Option<u8> {
        let (u, v) = (x - origin[0], y - origin[1]);
        let (width, height) = (i64::from(self.mask.width), i64::from(self.mask.height));
        ((0..width).contains(&u) && (0..height).contains(&v))
            .then(|| self.mask.pixels()[(v * width + u) as usize])
    }
    /// The §3.37 orientation table for a wall or scenery object, with or
    /// without the wall-transparency-end exceptions.
    fn oriented(&self, view: MapView, object: &MapObject, ends: bool) -> bool {
        let Some(hex) = object.hex else {
            return false;
        };
        let (Ok(at), Ok(player)) = (
            view.grid.hex_reference(hex),
            view.grid.hex_reference(self.hex),
        ) else {
            return false;
        };
        let (dx, dy) = ((at[0] - player[0]) as f64, (at[1] - player[1]) as f64);
        let r = dx <= dy * K;
        let r_prime = dx >= dy * K;
        let f = dx >= -4.0 * dy;
        let f_prime = dx <= -4.0 * dy;
        let end = ends && object.flags.0 & TRANSPARENCY_END != 0;
        let extended = (self.extended)(object.pid);
        if extended & (0x0800_0000 | 0x8000_0000) != 0 {
            f && !(r_prime && end)
        } else if extended & 0x1000_0000 != 0 {
            f || r
        } else if extended & 0x2000_0000 != 0 {
            f && r
        } else {
            r && !(f_prime && end)
        }
    }
    /// Whether a drawn object is cut: a scenery or wall art type, a visible
    /// player, no translucency bit, and the orientation test.
    fn cuts(&self, view: MapView, object: &MapObject) -> bool {
        matches!((object.art_id >> 24) & 0x0f, 2 | 3)
            && !self.hidden
            && object.flags.0 & TRANSLUCENCY == 0
            && self.oriented(view, object, true)
    }
}

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
const TRANSLUCENT: [(u32, u16, bool, bool); 5] = [
    (0x10000, 25439, false, true),
    (0x20000, 10239, true, false),
    (0x40000, 32767, false, false),
    (0x80000, 30689, false, false),
    (0x4000, 31744, false, false),
];

/// The §3.44 translucent draw an object's flags select: exactly one
/// translucency bit, other than "explicitly no translucency".
fn translucent_row(flags: u32) -> Option<usize> {
    let bits = flags & TRANSLUCENCY;
    if bits.count_ones() != 1 {
        return None;
    }
    TRANSLUCENT.iter().position(|(bit, ..)| *bit == bits)
}

/// SPEC-014 §3.44 how a refresh draws its pixels: unlit without a light,
/// otherwise through the light's tables.
struct Pens<'a> {
    light: Option<MapLight<'a>>,
    /// The intensity table, indexed `color * 256 + level`.
    intensity: &'a [u8],
    /// The common and glass gray levels of every entry.
    gray: [[u8; 256]; 2],
    /// Each translucent row's blend table, `blend_F[g][D]`, once built.
    blends: [Option<Box<BlendTable>>; 5],
}

impl<'a> Pens<'a> {
    fn new(light: Option<MapLight<'a>>) -> Self {
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
    fn draw(
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
            let mixed = blend[usize::from(gray[usize::from(source)])][usize::from(destination)];
            intensity[usize::from(mixed) * 256 + level]
        });
    }
    /// An image layer drawn like an object without translucency: the lit
    /// draw at `level` of its nonzero pixels.
    fn draw_layer(
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
    /// SPEC-014 §3.37 a draw through the mask at `mask`: inside it each
    /// pixel is relit at `level` and blended by its weight, outside it the
    /// lit draw applies. Indexes 229 and up are relit inside the mask
    /// (the reference rule).
    fn draw_cut(
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

/// The hex under a view point by the SPEC-003 picking rule, without the
/// grid bounds: `column + 200 × row` even off the grid, so hex differences
/// are the same wherever the points lie.
fn hex_under(grid: HexGrid, [x, y]: [i64; 2]) -> i64 {
    let y = y - grid.origin[1];
    let k = y.div_euclid(12);
    let v = x - grid.origin[0] - 16 * k;
    let (b, r) = (v.div_euclid(64), v.rem_euclid(64));
    let mut column = 2 * b + i64::from(r >= 32);
    let mut row = b + k;
    let (lx, ly) = (r % 32, y - 12 * k);
    if ly < 4 {
        if lx < 16 - 4 * ly {
            row -= 1;
        } else if lx > 16 + 4 * ly {
            column += 1;
            if column.rem_euclid(2) == 1 {
                row -= 1;
            }
        }
    }
    199 - column + 200 * row
}

/// SPEC-014 §3.44 the visiting lattice, one table per parity of the
/// view-center hex: 41 columns by 72 rows of points over the 640-by-380
/// window widened by 320 pixels each side and 240 above and below, each
/// point as (hex offset from the corner hex, row, column), sorted by offset.
fn lattice() -> &'static [Vec<(i64, u8, u8)>; 2] {
    static LATTICE: OnceLock<[Vec<(i64, u8, u8)>; 2]> = OnceLock::new();
    LATTICE.get_or_init(|| {
        std::array::from_fn(|parity| {
            let grid = HexGrid::centered(20100 + parity as i32, 640, 380).expect("hex on the grid");
            let corner = hex_under(grid, [-320, -240]);
            let [x0, y0] = grid
                .hex_center(corner as i32)
                .expect("corner hex on the grid");
            let step = if x0 > -320 { -16 } else { 16 };
            let mut points = Vec::with_capacity(41 * 72);
            for row in 0..72_i64 {
                let first = match row {
                    0 => x0,
                    _ if row % 2 == 1 => x0 + step,
                    _ => x0 - step,
                };
                for column in 0..41_i64 {
                    let hex = hex_under(grid, [first + 32 * column, y0 + 12 * row]);
                    points.push((hex - corner, row as u8, column as u8));
                }
            }
            points.sort_by_key(|(offset, ..)| *offset);
            points
        })
    })
}

/// SPEC-014 §3.44 the hexes a refresh of the whole view visits, in
/// ascending hex number: the lattice for the view-center hex's parity from
/// the hex under the widened top-left corner, limited to the widened
/// view's rows and columns.
fn visited_hexes(view: MapView) -> impl Iterator<Item = i32> {
    let (width, height) = (i64::from(view.width), i64::from(view.height));
    let center = hex_under(
        view.grid,
        [
            (width - 32).div_euclid(2) + 16,
            (height - 16).div_euclid(2) + 8,
        ],
    );
    let base = hex_under(view.grid, [-320, -240]);
    let (rows, columns) = ((height + 480) / 12, (width + 640) / 32);
    lattice()[center.rem_euclid(2) as usize]
        .iter()
        .filter(move |(_, row, column)| i64::from(*row) < rows && i64::from(*column) < columns)
        .filter_map(move |(offset, ..)| i32::try_from(base + offset).ok())
        .filter(|hex| (0..40000).contains(hex))
}

/// SPEC-014 §3.44 the order a refresh draws `objects` (one elevation,
/// given in insertion order) in: the flat pass, then the upright pass, by
/// visited hex and within a hex in list order (flat objects first, each
/// group oldest insertion first). Hidden objects and objects on unvisited
/// hexes are not drawn. The second list holds the objects without a hex,
/// drawn last.
fn draw_sequence(view: MapView, objects: &[MapObject]) -> (Vec<usize>, Vec<usize>) {
    let (placed, hexless) = draw_sequence_with(view, objects, None);
    let placed = placed
        .into_iter()
        .filter_map(|drawn| match drawn {
            Drawn::Object(index) => Some(index),
            Drawn::Cursor | Drawn::CursorText => None,
        })
        .collect();
    (placed, hexless)
}

/// [`draw_sequence`] with the §3.27 cursor objects at `cursor` (their hex
/// and list place, [`SceneCursor`]): the cursor in its hex's flat group and
/// the AP text in its upright group, each before the objects listed from
/// `order` on. On an unvisited hex neither is drawn.
fn draw_sequence_with(
    view: MapView,
    objects: &[MapObject],
    cursor: Option<(i32, usize)>,
) -> (Vec<Drawn>, Vec<usize>) {
    let mut placed: Vec<(i32, bool, usize)> = Vec::new();
    let mut hexless = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        if object.flags.contains(BasicFlags::HIDDEN) {
            continue;
        }
        match object.hex {
            Some(hex) => placed.push((hex, !object.flags.contains(BasicFlags::FLAT), index)),
            None => hexless.push(index),
        }
    }
    placed.sort_unstable();
    let mut flat = Vec::new();
    let mut upright = Vec::new();
    for hex in visited_hexes(view) {
        let here = cursor.filter(|(at, _)| *at == hex).map(|(_, order)| order);
        let (mut cursor_due, mut text_due) = (here.is_some(), here.is_some());
        let start = placed.partition_point(|(at, ..)| *at < hex);
        for (_, is_upright, index) in placed[start..].iter().take_while(|(at, ..)| *at == hex) {
            let later = here.is_some_and(|order| *index >= order);
            if *is_upright {
                if text_due && later {
                    upright.push(Drawn::CursorText);
                    text_due = false;
                }
                upright.push(Drawn::Object(*index));
            } else {
                if cursor_due && later {
                    flat.push(Drawn::Cursor);
                    cursor_due = false;
                }
                flat.push(Drawn::Object(*index));
            }
        }
        if cursor_due {
            flat.push(Drawn::Cursor);
        }
        if text_due {
            upright.push(Drawn::CursorText);
        }
    }
    flat.extend(upright);
    (flat, hexless)
}

/// The floor squares of the view (SPEC-003 §2.8), unlit.
pub fn render_floor(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
) -> Result<IndexedImage, MapRenderError> {
    floor(map, view, art, |image, frame, corner| {
        image.blit(frame, corner)
    })
}

/// The floor squares of the view (SPEC-003 §2.8) under the SPEC-014 §3.44
/// floor light: each tile lit per pixel from the light of ten hexes around
/// it. A tile with no hex under its base point is not drawn.
pub fn render_floor_lit(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    light: MapLight,
) -> Result<IndexedImage, MapRenderError> {
    let mut floor_light = FloorLight::new(light);
    floor(map, view, art, |image, frame, corner| {
        floor_light.draw(image, view, frame, corner);
    })
}

/// Each drawn floor square's art at its corner, by `draw`.
fn floor(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    mut draw: impl FnMut(&mut IndexedImage, &FrmFrame, [i64; 2]),
) -> Result<IndexedImage, MapRenderError> {
    if view.elevation > 2 {
        return Err(MapRenderError::Elevation);
    }
    if view
        .grid
        .origin
        .iter()
        .any(|value| i32::try_from(*value).is_err())
    {
        return Err(GeometryError::ProjectionOverflow.into());
    }
    let mut image = IndexedImage::new(view.width, view.height)?;
    let origin = view.grid.origin;
    for (index, offset) in square_order() {
        let corner = corner(origin, *offset);
        if beyond(view, corner) {
            continue;
        }
        let tile = map
            .tile(view.elevation as usize, *index as usize)
            .expect("validated map grid");
        // SPEC-003 §2.8 exempts only roof index 1 from drawing; floor index 1
        // (the ordinary blank tile) draws its art like any other floor.
        if !tile.floor_hidden()
            && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.floor()), 0, 0)
        {
            draw(&mut image, frame, corner);
        }
    }
    Ok(image)
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
const FLOOR_MAP: usize = 41 * 80;
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
fn fill_floor_map(lights: &[i32; 10], map: &mut [i32; FLOOR_MAP], written: &mut [bool; FLOOR_MAP]) {
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
fn floor_sources() -> &'static [u16; FLOOR_MAP] {
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
struct FloorLight<'a> {
    light: MapLight<'a>,
    intensity: &'a [u8],
    map: Box<[i32; FLOOR_MAP]>,
    written: Box<[bool; FLOOR_MAP]>,
}

impl<'a> FloorLight<'a> {
    fn new(light: MapLight<'a>) -> Self {
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
    fn vertices(&self, base: i32) -> [i32; 10] {
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
    /// Draw one floor tile whose art's top-left is at `corner`.
    fn draw(
        &mut self,
        image: &mut IndexedImage,
        view: MapView,
        frame: &FrmFrame,
        corner: [i64; 2],
    ) {
        let (Ok(x), Ok(y)) = (i32::try_from(corner[0]), i32::try_from(corner[1] + 13)) else {
            return;
        };
        let Some(base) = view.grid.pick_hex([x, y]) else {
            return;
        };
        let lights = self.vertices(base);
        let intensity = self.intensity;
        if lights.iter().all(|light| *light == lights[0]) {
            // Uniform: the lit draw, so indexes 229 and up are not lit.
            let level = MapLight::level(i64::from(lights[0]));
            blit_with(image, frame, corner, |source, _, _| {
                if source >= 229 {
                    source
                } else {
                    intensity[usize::from(source) * 256 + usize::from(level)]
                }
            });
            return;
        }
        // The map is filled afresh: with the safe rule below no entry is
        // read that this tile's triangles did not write (unless its whole
        // column is unwritten, when it reads 0).
        self.map.fill(0);
        fill_floor_map(&lights, &mut self.map, &mut self.written);
        let (map, sources) = (&self.map, floor_sources());
        blit_with(image, frame, corner, |source, _, [px, py]| {
            // `[May-improve]`: indexes 229-255 stay unlit, as in a uniform
            // tile (R1 draws them through their unmapped intensity rows,
            // as index 0).
            if source >= 229 {
                return source;
            }
            let entry = 160 + 80 * (py - corner[1]) + (px - corner[0]);
            let light = usize::try_from(entry)
                .ok()
                .filter(|entry| *entry < FLOOR_MAP)
                .map_or(0, |entry| map[usize::from(sources[entry])]);
            let level = (light / 512).clamp(0, 128) as usize;
            intensity[usize::from(source) * 256 + level]
        });
    }
}

/// A square whose corner lies past the right or bottom edge draws nothing.
fn beyond(view: MapView, [x, y]: [i64; 2]) -> bool {
    x >= i64::from(view.width) || y >= i64::from(view.height)
}

/// [`render_map_with`] with the player's see-through cutout, unlit.
/// Without a cutout (no mask art, or no player hex) everything draws
/// normally (the SPEC-014 §3.37 safe rule).
pub fn render_map_cut(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    cutout: Option<&Cutout>,
) -> Result<IndexedImage, MapRenderError> {
    let mut image = render_floor(map, view, art)?;
    draw_map_scene(
        &mut image,
        map,
        objects,
        view,
        art,
        MapScene {
            cutout,
            ..MapScene::default()
        },
    )?;
    Ok(image)
}

/// SPEC-014 §3.44 the layers of a refresh over its floor `image`: the
/// objects of the flat and upright passes, the roofs (section 3.36) at the
/// ambient light, the outlines collected in the passes (at most 100, in
/// drawing order), then the objects without a hex at full light. The
/// cutout (§3.37) applies to the roofs and the walls and scenery it cuts.
pub fn draw_map_scene(
    image: &mut IndexedImage,
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    scene: MapScene,
) -> Result<(), MapRenderError> {
    if view.elevation > 2 {
        return Err(MapRenderError::Elevation);
    }
    let objects = &objects.elevations[view.elevation as usize];
    let cutout = scene.cutout.and_then(|cut| Some((cut, cut.origin(view)?)));
    let mut pens = Pens::new(scene.light);
    let level = |hex: Option<i32>| scene.light.map_or(128, |light| light.object(hex));
    let sprite = |object: &MapObject| -> Result<Option<(&FrmFrame, [i64; 2])>, GeometryError> {
        let Ok(category) = Category::from_pid(object.pid) else {
            return Ok(None);
        };
        let Some((frame, shift)) =
            art.frame(category, object.art_id, object.orientation, object.frame)
        else {
            return Ok(None);
        };
        Ok(Some((frame, placement(view, frame, shift, object)?.0)))
    };
    let cursor = scene.cursor;
    let (placed, hexless) = draw_sequence_with(
        view,
        objects,
        cursor.map(|cursor| (cursor.hex, cursor.order)),
    );
    let mut outlines = Vec::new();
    for drawn in placed {
        let index = match (drawn, cursor) {
            (Drawn::Object(index), _) => index,
            // SPEC-014 §3.27: the cursor objects are lit as any object on
            // their hex; the cursor's outline joins the list as it draws.
            (Drawn::Cursor, Some(cursor)) => {
                let (art, origin) = cursor.art;
                pens.draw_layer(image, art, origin, level(Some(cursor.hex)));
                if outlines.len() < 100
                    && let Some((frame, origin, word)) = cursor.outline
                    && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(word))
                {
                    outlines.push((frame, origin, ink));
                }
                continue;
            }
            (Drawn::CursorText, Some(cursor)) => {
                if let Some((text, origin)) = cursor.text {
                    pens.draw_layer(image, text, origin, level(Some(cursor.hex)));
                }
                continue;
            }
            (Drawn::Cursor | Drawn::CursorText, None) => continue,
        };
        let object = &objects[index];
        let Some((frame, origin)) = sprite(object)? else {
            continue;
        };
        let level = level(object.hex);
        match cutout {
            Some((cut, mask)) if cut.cuts(view, object) => {
                pens.draw_cut(image, frame, origin, level, cut, mask);
            }
            _ => pens.draw(image, frame, origin, object.flags.0, level),
        }
        // At most 100 outlined objects per refresh, in drawing order; an
        // object without a hex is never outlined.
        if outlines.len() < 100
            && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(object.outline))
        {
            outlines.push((frame, origin, ink));
        }
    }
    if view.roofs {
        let ambient = scene
            .light
            .map_or(128, |light| MapLight::level(i64::from(light.ambient)));
        for (index, offset) in square_order() {
            let corner = corner(view.grid.origin, *offset);
            let corner = [corner[0], corner[1] - 96];
            if beyond(view, corner) {
                continue;
            }
            let tile = map
                .tile(view.elevation as usize, *index as usize)
                .expect("validated map grid");
            if tile.roof() != 1
                && !tile.roof_hidden()
                && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.roof()), 0, 0)
            {
                // Every drawn roof square goes through the mask.
                match cutout {
                    Some((cut, mask)) => pens.draw_cut(image, frame, corner, ambient, cut, mask),
                    None => pens.draw(image, frame, corner, 0, ambient),
                }
            }
        }
    }
    // The refresh covers the whole view, which is the map buffer.
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    for (frame, origin, ink) in outlines {
        draw_outline(
            image,
            frame,
            origin,
            ink,
            [0, 0, width, height],
            [width, height],
        );
    }
    // SPEC-014 §3.46: floating text, in creation order, transparently, at
    // its creation hex's current window position plus its offset; not
    // hidden by roofs and whether or not its owner shows.
    for text in scene.floating {
        let Ok([x, y]) = view.grid.hex_reference(text.hex) else {
            continue;
        };
        image.blit_image(
            &text.image,
            [x + i64::from(text.offset[0]), y + i64::from(text.offset[1])],
        );
    }
    for index in hexless {
        let object = &objects[index];
        if let Some((frame, origin)) = sprite(object)? {
            pens.draw(image, frame, origin, object.flags.0, 128);
        }
    }
    Ok(())
}

/// Where an object's frame is drawn: its top-left corner and its anchor
/// (the sort point). A hex-placed object anchors at the hex center plus the
/// art shift and its displacement, any other at its screen position.
fn placement(
    view: MapView,
    frame: &FrmFrame,
    shift: [i16; 2],
    object: &MapObject,
) -> Result<([i64; 2], [i64; 2]), GeometryError> {
    Ok(if let Some(hex) = object.hex {
        let center = view.grid.hex_center(hex)?;
        // i64 arithmetic preserves geometry even when a panned camera is far
        // outside the grid; the clipped blit never iterates offscreen pixels.
        let anchor = [
            center[0] + i64::from(shift[0]) + i64::from(object.displacement[0]),
            center[1] + i64::from(shift[1]) + i64::from(object.displacement[1]),
        ];
        (
            [
                anchor[0] - i64::from(frame.width / 2),
                anchor[1] - i64::from(frame.height) + 1,
            ],
            anchor,
        )
    } else {
        let origin = object.screen.map(i64::from);
        (
            origin,
            [
                origin[0] + i64::from(frame.width / 2),
                origin[1] + i64::from(frame.height) - 1,
            ],
        )
    })
}

/// The frame an object shows and its top-left corner in the view, for
/// overlays drawn over the composed map (an item's hover outline).
pub fn object_sprite<'a>(
    view: MapView,
    art: &'a impl MapArt,
    object: &MapObject,
) -> Option<(&'a FrmFrame, [i64; 2])> {
    let category = Category::from_pid(object.pid).ok()?;
    let (frame, shift) = art.frame(category, object.art_id, object.orientation, object.frame)?;
    let (origin, _) = placement(view, frame, shift, object).ok()?;
    Some((frame, origin))
}

/// Pick the frontmost nontransparent object pixel. Roof pixels occlude picking.
pub fn pick_object(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
) -> Option<u32> {
    pick_objects(map, view, art, point)?.first().copied()
}

/// Every object with a nontransparent pixel at `point`, frontmost first;
/// `None` when a roof pixel occludes the point.
pub fn pick_objects(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
) -> Option<Vec<u32>> {
    pick_objects_with(map, &map.objects, view, art, point)
}

/// [`pick_objects`] over `objects` in place of the map's own object table
/// (P-02: no map clone per pick).
pub fn pick_objects_with(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
) -> Option<Vec<u32>> {
    Some(
        pick_candidates(map, objects, view, art, point, None)?
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
    )
}

/// SPEC-014 §3.27 the pointer's candidates, frontmost first, each with
/// whether it is solid: `None` over a visible roof unless the pointer is on
/// a nonzero pixel of the cutout mask. A candidate with a translucency bit
/// other than "explicitly no translucency" is never solid, nor is a wall or
/// scenery candidate inside the cutout: without translucency bits, passing
/// the orientation table without the transparency-end exceptions (whether
/// or not the player is hidden), with a nonzero mask pixel under the
/// pointer.
pub fn pick_candidates(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutout: Option<&Cutout>,
) -> Option<Vec<(u32, bool)>> {
    if view.elevation > 2 {
        return None;
    }
    let sample = |frame: &FrmFrame, origin: [i64; 2]| {
        let x = i64::from(point[0]) - origin[0];
        let y = i64::from(point[1]) - origin[1];
        x >= 0
            && y >= 0
            && x < i64::from(frame.width)
            && y < i64::from(frame.height)
            && frame.pixels()[(y * i64::from(frame.width) + x) as usize] != 0
    };
    let in_mask = cutout.is_some_and(|cut| {
        cut.origin(view)
            .and_then(|mask| cut.weight(mask, point.map(i64::from)))
            .is_some_and(|weight| weight != 0)
    });
    if view.roofs && !in_mask {
        let [ox, oy] = view.grid.origin;
        for (square, offset) in square_offsets().iter().enumerate() {
            let tile = map.tile(view.elevation as usize, square)?;
            if tile.roof() != 1
                && !tile.roof_hidden()
                && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.roof()), 0, 0)
                && sample(
                    frame,
                    [ox.checked_add(offset[0])?, oy.checked_add(offset[1] - 96)?],
                )
            {
                return None;
            }
        }
    }
    let mut hits = Vec::new();
    let objects = &objects.elevations[view.elevation as usize];
    let (placed, hexless) = draw_sequence(view, objects);
    // Frontmost first: the reverse of the drawing order.
    for index in placed.into_iter().chain(hexless).rev() {
        let object = &objects[index];
        // M-R2: an object with an unknown PID category or no placement is
        // skipped, as the draw path skips it; it does not hide the others.
        let Ok(category) = Category::from_pid(object.pid) else {
            continue;
        };
        let Some((frame, shift)) =
            art.frame(category, object.art_id, object.orientation, object.frame)
        else {
            continue;
        };
        let Ok((origin, _)) = placement(view, frame, shift, object) else {
            continue;
        };
        if sample(frame, origin) {
            let bits = object.flags.0 & TRANSLUCENCY;
            let translucent = bits != 0 && bits != NO_TRANSLUCENCY;
            let inside = in_mask
                && bits == 0
                && matches!((object.art_id >> 24) & 0x0f, 2 | 3)
                && cutout.is_some_and(|cut| cut.oriented(view, object, false));
            hits.push((object.id, !translucent && !inside));
        }
    }
    Some(hits)
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/world/support.rs"]
mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use vault_formats::{FrmArchive, Prototype};
    struct Art {
        tile: FrmArchive,
        /// Tile index 1, the ordinary blank tile.
        blank: FrmArchive,
        object: FrmArchive,
    }
    impl MapArt for Art {
        fn frame(
            &self,
            category: Category,
            id: u32,
            direction: u8,
            frame: usize,
        ) -> Option<(&FrmFrame, [i16; 2])> {
            let archive = match (category, id) {
                (Category::Tile, 1) => &self.blank,
                (Category::Tile, _) => &self.tile,
                _ => &self.object,
            };
            Some((archive.frame(direction as usize, frame)?, [0, 0]))
        }
    }
    /// A grey palette: index i is grey `i / 4` (six-bit), and five-bit grey
    /// g looks up index `8 g`.
    fn grey_palette() -> Palette {
        let mut bytes = Vec::with_capacity(0x8300);
        for index in 0..256_u32 {
            let grey = (index / 4) as u8;
            bytes.extend([grey; 3]);
        }
        for red in 0..32_u32 {
            bytes.extend(std::iter::repeat_n((red * 8) as u8, 1024));
        }
        Palette::parse(&bytes).unwrap()
    }
    fn preview() -> MapFile {
        MapFile::parse(&fixture::preview_map(), &|pid| {
            let category = (pid >> 24) as u8;
            Ok(Prototype::parse(&fixture::prototype(
                category,
                if category == 0 { 1 } else { 5 },
            ))?
            .info())
        })
        .unwrap()
    }
    /// Art by identifier: tiles and objects keyed by their art id.
    struct Keyed(std::collections::BTreeMap<(bool, u32), FrmArchive>);
    impl MapArt for Keyed {
        fn frame(
            &self,
            category: Category,
            id: u32,
            direction: u8,
            frame: usize,
        ) -> Option<(&FrmFrame, [i16; 2])> {
            let archive = self.0.get(&(category == Category::Tile, id))?;
            Some((archive.frame(direction as usize, frame)?, [0, 0]))
        }
    }
    /// An object of `art` on `hex`, from the fixture map's first object.
    fn object(map: &MapFile, id: u32, art: u32, hex: i32) -> MapObject {
        let mut object = map.objects.elevations[0][0].clone();
        object.id = id;
        object.pid = art & 0x0f00_0000 | 1;
        object.art_id = art;
        object.hex = Some(hex);
        object.flags = BasicFlags(0);
        object.displacement = [0; 2];
        object.frame = 0;
        object.orientation = 0;
        object
    }
    fn scene(objects: Vec<MapObject>) -> MapObjects {
        MapObjects {
            total: objects.len(),
            elevations: [objects, Vec::new(), Vec::new()],
            consumed_len: 0,
        }
    }
    fn view() -> MapView {
        MapView {
            grid: HexGrid::centered(20100, 640, 380).unwrap(),
            elevation: 0,
            roofs: false,
            width: 640,
            height: 380,
        }
    }

    #[test]
    fn spec014_cutout_orientation_table_uses_the_exact_constant() {
        let map = preview();
        let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
        let palette = grey_palette();
        let cut = |extended: &'static dyn Fn(u32) -> u32| Cutout {
            mask: mask.frame(0, 0).unwrap(),
            hex: 20100,
            displacement: [0; 2],
            hidden: false,
            extended,
            palette: &palette,
        };
        let view = view();
        let wall = |hex: i32, end: bool| {
            let mut wall = object(&map, 1, 0x0300_0001, hex);
            if end {
                wall.flags.0 |= TRANSPARENCY_END;
            }
            wall
        };
        // Rows on and back (+-16, +-12) are exact equalities (12 K rounds
        // to 16), so R and R' both hold. Three rows back (-48, -36) is where
        // K decides: 36 K exceeds 48, so R fails there (it would hold with
        // the nearest double to 4/3).
        let plain = cut(&|_| 0);
        assert_eq!(12.0 * K, 16.0);
        const { assert!(36.0 * K > 48.0 && 36.0 * (4.0_f64 / 3.0) == 48.0) };
        assert!(plain.oriented(view, &wall(20300, false), true));
        assert!(plain.oriented(view, &wall(19900, false), true));
        assert!(!plain.oriented(view, &wall(19500, false), true));
        // Two columns on (-48, +12): R, F and F' all hold, so the end marker
        // decides the default row.
        assert!(plain.oriented(view, &wall(20102, false), true));
        assert!(!plain.oriented(view, &wall(20102, true), true));
        assert!(plain.oriented(view, &wall(20102, true), false));
        // The other rows of the table.
        let east = cut(&|_| 0x0800_0000);
        assert!(east.oriented(view, &wall(20102, true), true));
        assert!(!east.oriented(view, &wall(19500, false), true));
        let either = cut(&|_| 0x1000_0000);
        assert!(either.oriented(view, &wall(20300, false), true));
        assert!(!either.oriented(view, &wall(19500, false), true));
        let both = cut(&|_| 0x2000_0000);
        assert!(both.oriented(view, &wall(20300, false), true));
        // Three rows back (-48, -36): F' only.
        assert!(!both.oriented(view, &wall(19500, false), true));
    }

    #[test]
    fn spec014_cutout_blends_walls_and_roofs_through_the_mask() {
        let map = preview();
        let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
        let palette = grey_palette();
        let none = |_| 0;
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (true, 1),
            FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
        );
        art.insert(
            (true, 2),
            FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
        );
        art.insert(
            (false, 0x0300_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        );
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        );
        let art = Keyed(art);
        let mut view = view();
        let at = |hex: i32| view.grid.hex_center(hex).unwrap();
        let pixel =
            |image: &IndexedImage, [x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
        let cutout = Cutout {
            mask: mask.frame(0, 0).unwrap(),
            hex: 20100,
            displacement: [0; 2],
            hidden: false,
            extended: &none,
            palette: &palette,
        };
        // A wall one row in front of the player is cut; scenery three rows
        // behind is not. The floor under the wall is index 0 (black).
        let wall = object(&map, 1, 0x0300_0001, 20300);
        let behind = object(&map, 2, 0x0200_0001, 19500);
        let objects = scene(vec![wall.clone(), behind]);
        let image = render_map_cut(&map, &objects, view, &art, Some(&cutout)).unwrap();
        // 20 is five-bit grey 2: at weight 64 it halves to 1 (index 8), and
        // black adds nothing.
        assert_eq!(pixel(&image, at(20300)), 8);
        assert_eq!(pixel(&image, at(19500)), 20);
        assert_eq!(palette.mask_blend(20, 0, 64), 8);
        assert_eq!(palette.mask_blend(20, 0, 0), 20);
        // Without the cutout, for a hidden player, or with a translucency
        // bit the wall draws normally.
        let image = render_map_cut(&map, &objects, view, &art, None).unwrap();
        assert_eq!(pixel(&image, at(20300)), 20);
        let hidden = Cutout {
            hidden: true,
            ..cutout
        };
        let image = render_map_cut(&map, &objects, view, &art, Some(&hidden)).unwrap();
        assert_eq!(pixel(&image, at(20300)), 20);
        let mut glass = wall.clone();
        glass.flags.0 |= 0x8000;
        let image = render_map_cut(&map, &scene(vec![glass]), view, &art, Some(&cutout)).unwrap();
        assert_eq!(pixel(&image, at(20300)), 20);
        // The mask moves with the player's displacement: shifted well away,
        // the wall is outside it.
        let away = Cutout {
            displacement: [200, 0],
            ..cutout
        };
        let image = render_map_cut(&map, &objects, view, &art, Some(&away)).unwrap();
        assert_eq!(pixel(&image, at(20300)), 20);
        // Every roof square goes through the mask, even for a hidden player:
        // grey 25 at weight 64 over the blank floor gives grey 12.
        view.roofs = true;
        let roof = [256, 84];
        let player = view.grid.pick_hex([256, 124]).unwrap();
        let over = Cutout {
            hex: player,
            hidden: true,
            ..cutout
        };
        let image = render_map_cut(&map, &scene(Vec::new()), view, &art, Some(&over)).unwrap();
        assert_eq!(pixel(&image, roof), 96);
        let image = render_map_cut(&map, &scene(Vec::new()), view, &art, None).unwrap();
        assert_eq!(pixel(&image, roof), 200);
        // Picking: over the roof only inside the mask; there the wall in
        // front is "inside the cutout" and not solid, a translucent object
        // never is, and a plain one is.
        view.roofs = false;
        let point = at(20300).map(|value| value as i32);
        let candidates = pick_candidates(&map, &objects, view, &art, point, Some(&cutout)).unwrap();
        assert_eq!(candidates, [(1, false)]);
        assert_eq!(
            pick_candidates(&map, &objects, view, &art, point, None).unwrap(),
            [(1, true)]
        );
        let mut tinted = object(&map, 3, 0x0200_0001, 20300);
        tinted.flags.0 |= 0x4000;
        assert_eq!(
            pick_candidates(&map, &scene(vec![tinted]), view, &art, point, None).unwrap(),
            [(3, false)]
        );
    }

    #[test]
    fn objects_draw_flat_first_then_in_hex_order() {
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(200, 40, 30)).unwrap(),
        );
        art.insert(
            (false, 0x0200_0002),
            FrmArchive::parse(&fixture::frm(200, 40, 40)).unwrap(),
        );
        let art = Keyed(art);
        let view = view();
        // Hex 20104 sits 96 pixels left of and 24 below 20100; hex 20300
        // sits 16 right of and 12 below it. By screen height 20104 would
        // draw last; in object order 20300 does.
        let left = object(&map, 1, 0x0200_0001, 20104);
        let right = object(&map, 2, 0x0200_0002, 20300);
        let [x, y] = view.grid.hex_center(20300).unwrap();
        let image = render_map_cut(
            &map,
            &scene(vec![right.clone(), left.clone()]),
            view,
            &art,
            None,
        )
        .unwrap();
        let pixel = image.pixels()[(y - 5) as usize * 640 + (x - 40) as usize];
        assert_eq!(pixel, 40);
        // A flat object draws before every non-flat one.
        let mut flat = right;
        flat.flags.0 |= BasicFlags::FLAT.0;
        let image = render_map_cut(&map, &scene(vec![flat, left]), view, &art, None).unwrap();
        let pixel = image.pixels()[(y - 5) as usize * 640 + (x - 40) as usize];
        assert_eq!(pixel, 30);
    }

    #[test]
    fn spec014_lattice_visits_every_shown_hex_in_ascending_order() {
        // 41 columns by 72 rows, each point a distinct hex.
        for table in lattice() {
            assert_eq!(table.len(), 41 * 72);
            assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0));
        }
        // Off-grid numbering agrees with picking on the grid.
        let grid = HexGrid::centered(20100, 640, 380).unwrap();
        for point in [[0, 0], [320, 190], [17, 333], [639, 379], [-5, 100]] {
            let picked = grid.pick_hex(point).map(i64::from);
            assert_eq!(Some(hex_under(grid, point.map(i64::from))), picked);
        }
        // Every hex whose center is in the view is visited, for views
        // centered on hexes of both parities.
        for center in [20100, 20101, 15333, 24688] {
            let mut view = view();
            view.grid = HexGrid::centered(center, 640, 380).unwrap();
            let visited: Vec<i32> = visited_hexes(view).collect();
            assert!(visited.windows(2).all(|pair| pair[0] < pair[1]));
            for hex in 0..40000 {
                let [x, y] = view.grid.hex_center(hex).unwrap();
                if (0..640).contains(&x) && (0..380).contains(&y) {
                    assert!(visited.binary_search(&hex).is_ok(), "{center}: {hex}");
                }
            }
        }
    }

    #[test]
    fn spec014_fixture_q_draw_order() {
        let map = preview();
        let flat = |id: u32, hex: i32| {
            let mut item = object(&map, id, 0x0000_0001, hex);
            item.flags.0 |= BasicFlags::FLAT.0;
            item
        };
        let [a, b] = [flat(1, 20100), flat(2, 20100)];
        let c = object(&map, 3, 0x0100_0001, 20100);
        let d = object(&map, 4, 0x0200_0001, 20100);
        let e = flat(5, 20101);
        let f = object(&map, 6, 0x0300_0001, 20101);
        let order = |objects: Vec<MapObject>| {
            let (placed, hexless) = draw_sequence(view(), &objects);
            assert!(hexless.is_empty());
            placed
                .iter()
                .map(|index| objects[*index].id)
                .collect::<Vec<_>>()
        };
        // After load: A, B, E, then C, D, F.
        let loaded = vec![
            a.clone(),
            b.clone(),
            c.clone(),
            d.clone(),
            e.clone(),
            f.clone(),
        ];
        assert_eq!(order(loaded), [1, 2, 5, 3, 4, 6]);
        // C takes a walk step on 20100: it moves to the end of its group.
        let mut stepped = c.clone();
        stepped.displacement = [4, -2];
        let walked = vec![
            a.clone(),
            b.clone(),
            d.clone(),
            e.clone(),
            f.clone(),
            stepped,
        ];
        assert_eq!(order(walked), [1, 2, 5, 4, 3, 6]);
        // C steps onto 20101, after E and F there.
        let mut moved = c;
        moved.hex = Some(20101);
        let crossed = vec![a, b, d, e, f, moved];
        assert_eq!(order(crossed.clone()), [1, 2, 5, 4, 6, 3]);
        // Hidden objects are skipped, and an object without a hex is last.
        let mut hidden = crossed;
        hidden[2].flags.0 |= BasicFlags::HIDDEN.0;
        hidden[0].hex = None;
        let (placed, hexless) = draw_sequence(view(), &hidden);
        assert_eq!(placed, [1, 3, 4, 5]);
        assert_eq!(hexless, [0]);
    }

    /// One object of `color` on hex 20100 with `flags`, drawn over a floor
    /// of `floor` with `light`; the pixel at the hex center.
    fn lit_pixel(color: u8, floor: u8, flags: u32, light: MapLight) -> u8 {
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(8, 8, color)).unwrap(),
        );
        let art = Keyed(art);
        let view = view();
        let mut item = object(&map, 1, 0x0200_0001, 20100);
        item.flags.0 = flags;
        let mut image = IndexedImage::new(640, 380).unwrap();
        image.pixels_mut().fill(floor);
        let lit = MapScene {
            light: Some(light),
            ..MapScene::default()
        };
        draw_map_scene(&mut image, &map, &scene(vec![item]), view, &art, lit).unwrap();
        let [x, y] = view.grid.hex_center(20100).unwrap();
        image.pixels()[(y - 3) as usize * 640 + x as usize]
    }

    #[test]
    fn spec014_lit_and_translucent_draws() {
        // In the grey palette index i has five-bit grey i / 8, and five-bit
        // grey g looks up index 8 g.
        let palette = grey_palette();
        let mut tiles = vec![0; 40000];
        tiles[20100] = 32768;
        let dim = MapLight {
            palette: &palette,
            ambient: 16384,
            tiles: &tiles,
        };
        // Lit draw: the hex light (above the ambient) halves grey 25 to 12;
        // 229 and up are unchanged.
        assert_eq!(lit_pixel(200, 80, 0, dim), 96);
        assert_eq!(lit_pixel(230, 80, 0, dim), 230);
        // Full ambient light leaves grey 25 at index 200.
        let bright = MapLight {
            ambient: 65536,
            ..dim
        };
        assert_eq!(lit_pixel(200, 80, 0, bright), 200);
        // Wall translucency: key 25439 has red 24 (index 192); grey 25 has
        // common gray level 6, so the mix is (6 × 24 + 10) / 7 = 22 over
        // grey 10, at full light whatever the hex light (index 176).
        assert_eq!(lit_pixel(200, 80, 0x10000, dim), 176);
        // Steam (key 32767, grey 31) uses the hex light: (6 × 31 + 10) / 7
        // = 28, halved to 14.
        assert_eq!(lit_pixel(200, 80, 0x40000, dim), 112);
        // Gray level 0 keeps the destination but relights it.
        assert_eq!(lit_pixel(8, 80, 0x40000, dim), 40);
        // Indexes 229 and up get no exemption: 240 (grey 30, gray level 7)
        // draws the foreground itself.
        assert_eq!(lit_pixel(240, 80, 0x40000, bright), 248);
        // Explicitly no translucency, or two bits, is the lit draw.
        assert_eq!(lit_pixel(200, 80, 0x8000, dim), 96);
        assert_eq!(lit_pixel(200, 80, 0x30000, dim), 96);
        // Unlit, everything is drawn as stored.
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(8, 8, 200)).unwrap(),
        );
        let mut glass = object(&map, 1, 0x0200_0001, 20100);
        glass.flags.0 = 0x20000;
        let image = render_map_with(&map, &scene(vec![glass]), view(), &Keyed(art)).unwrap();
        let [x, y] = view().grid.hex_center(20100).unwrap();
        assert_eq!(image.pixels()[(y - 3) as usize * 640 + x as usize], 200);
    }

    #[test]
    fn spec014_floating_text_draws_over_roofs_at_its_hex_and_m_r2_picks_past_bad_objects() {
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (true, 2),
            FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
        );
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        );
        let art = Keyed(art);
        let mut view = view();
        view.roofs = true;
        let roof = [256_i64, 84];
        let [hx, hy] = view.grid.hex_reference(20100).unwrap();
        // A 3-by-3 box with one glyph pixel at (1, 1), placed over the roof.
        let mut image = IndexedImage::new(3, 3).unwrap();
        image.pixels_mut()[4] = 77;
        let text = FloatingSprite {
            hex: 20100,
            offset: [(roof[0] - hx - 1) as i32, (roof[1] - hy - 1) as i32],
            image,
        };
        let draw = |texts: &[FloatingSprite]| {
            let mut image = render_floor(&map, view, &art).unwrap();
            let layers = MapScene {
                floating: texts,
                ..MapScene::default()
            };
            draw_map_scene(&mut image, &map, &scene(Vec::new()), view, &art, layers).unwrap();
            image
        };
        let pixel =
            |image: &IndexedImage, [x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
        let bare = draw(&[]);
        assert_eq!(pixel(&bare, roof), 200);
        let image = draw(std::slice::from_ref(&text));
        assert_eq!(pixel(&image, roof), 77, "not hidden by the roof");
        for x in [roof[0] - 1, roof[0] + 1] {
            let beside = [x, roof[1]];
            assert_eq!(
                pixel(&image, beside),
                pixel(&bare, beside),
                "index 0 skipped"
            );
        }
        // M-R2: an object whose PID has no category is skipped by picking
        // as by drawing; it does not hide the objects under the pointer.
        view.roofs = false;
        let good = object(&map, 1, 0x0200_0001, 20300);
        let mut bad = object(&map, 2, 0x0200_0001, 20500);
        bad.pid = 0x0f00_0001;
        let objects = scene(vec![bad, good]);
        let point = view
            .grid
            .hex_center(20300)
            .unwrap()
            .map(|value| value as i32);
        let hits = pick_candidates(&map, &objects, view, &art, point, None).unwrap();
        assert_eq!(hits, [(1, true)]);
    }

    #[test]
    fn spec014_outlines_follow_the_roofs_at_most_100() {
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        );
        let art = Keyed(art);
        let view = view();
        // 101 objects two hexes apart, all outlined in the other key (type
        // 3), and a disabled hostile one that is never collected.
        let hexes: Vec<i32> = (95..106)
            .flat_map(|row| (90..110).step_by(2).map(move |column| row * 200 + column))
            .take(101)
            .collect();
        let mut objects: Vec<_> = hexes
            .iter()
            .enumerate()
            .map(|(id, hex)| {
                let mut object = object(&map, id as u32 + 1, 0x0200_0001, *hex);
                object.outline = 3;
                object
            })
            .collect();
        let mut disabled = object(&map, 0, 0x0200_0001, 20111);
        disabled.outline = 0x8000_0001;
        objects.insert(0, disabled);
        let palette = grey_palette();
        let inks = OutlineInks::new(&palette);
        let other = crate::outline::key_index(&palette, crate::outline::OTHER_KEY);
        assert_ne!(other, 0);
        let mut image = render_floor(&map, view, &art).unwrap();
        let mut sorted = objects.clone();
        sorted.sort_by_key(|object| object.hex);
        draw_map_scene(
            &mut image,
            &map,
            &scene(objects),
            view,
            &art,
            MapScene {
                outlines: Some(&inks),
                ..MapScene::default()
            },
        )
        .unwrap();
        // The ring left of each sprite: the 100 first in drawing order are
        // outlined, the last is not; the disabled one never is.
        let ring = |object: &MapObject| {
            let (_, origin) = object_sprite(view, &art, object).unwrap();
            image.pixels()[origin[1] as usize * 640 + origin[0] as usize - 1]
        };
        let (disabled, sorted): (Vec<_>, Vec<_>) =
            sorted.into_iter().partition(|object| object.id == 0);
        assert_ne!(ring(&disabled[0]), other);
        assert!(sorted[..100].iter().all(|object| ring(object) == other));
        assert_ne!(ring(&sorted[100]), other);
    }

    #[test]
    fn spec014_cursor_object_draws_in_the_flat_pass_and_its_outline_after_the_roofs() {
        let map = preview();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (true, 2),
            FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
        );
        // Flat items in 20, upright scenery in 30.
        art.insert(
            (false, 0x0000_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        );
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 30)).unwrap(),
        );
        let art = Keyed(art);
        let mut view = view();
        let flat = |id: u32, hex: i32| {
            let mut item = object(&map, id, 0x0000_0001, hex);
            item.flags.0 |= BasicFlags::FLAT.0;
            item
        };
        let upright = |id: u32, hex: i32| object(&map, id, 0x0200_0001, hex);
        // The cursor sits on 20100; its art covers the whole view in 77 and
        // the AP text, when present, in 88.
        let layer =
            |color: u8| IndexedImage::from_pixels(640, 380, vec![color; 640 * 380]).unwrap();
        let (cursor_art, text_art) = (layer(77), layer(88));
        let ring = FrmArchive::parse(&fixture::frm(4, 4, 99)).unwrap();
        let ring = ring.frame(0, 0).unwrap();
        let palette = grey_palette();
        let inks = OutlineInks::new(&palette);
        let other = crate::outline::key_index(&palette, crate::outline::OTHER_KEY);
        let draw = |view: MapView, objects: Vec<MapObject>, cursor: SceneCursor| {
            let mut image = render_floor(&map, view, &art).unwrap();
            let layers = MapScene {
                outlines: Some(&inks),
                cursor: Some(cursor),
                ..MapScene::default()
            };
            draw_map_scene(&mut image, &map, &scene(objects), view, &art, layers).unwrap();
            image
        };
        let at = |image: &IndexedImage, hex: i32| {
            let [x, y] = view.grid.hex_center(hex).unwrap();
            image.pixels()[y as usize * 640 + x as usize]
        };
        let objects = vec![
            flat(1, 20098),
            flat(2, 20102),
            upright(3, 20096),
            upright(4, 20104),
        ];
        let cursor = SceneCursor {
            hex: 20100,
            order: objects.len(),
            art: (&cursor_art, [0, 0]),
            outline: None,
            text: None,
        };
        let image = draw(view, objects.clone(), cursor);
        // Over a flat object of a lower hex, under one of a higher hex, and
        // under every upright object, whatever its hex.
        assert_eq!(at(&image, 20098), 77, "over the lower flat object");
        assert_eq!(at(&image, 20102), 20, "under the higher flat object");
        assert_eq!(at(&image, 20096), 30, "under a lower upright object");
        assert_eq!(at(&image, 20104), 30, "under a higher upright object");
        // The AP text is upright: over the flat objects and the uprights of
        // lower hexes, under those of higher hexes.
        let with_text = SceneCursor {
            text: Some((&text_art, [0, 0])),
            ..cursor
        };
        let image = draw(view, objects.clone(), with_text);
        assert_eq!(at(&image, 20098), 88);
        assert_eq!(at(&image, 20102), 88);
        assert_eq!(at(&image, 20096), 88, "over the lower upright object");
        assert_eq!(at(&image, 20104), 30, "under the higher upright object");
        // Within its hex the cursor follows the flat objects inserted
        // before its last move and precedes those inserted after.
        let own = vec![flat(5, 20100)];
        let before = SceneCursor { order: 1, ..cursor };
        assert_eq!(at(&draw(view, own.clone(), before), 20100), 77);
        let after = SceneCursor { order: 0, ..cursor };
        assert_eq!(at(&draw(view, own, after), 20100), 20);
        // The cursor's outline joins the outline list when it is drawn, so
        // it is painted after the roofs: the roof hides the cursor's art,
        // not its ring.
        view.roofs = true;
        let roof = [256_i64, 84];
        let small = IndexedImage::from_pixels(4, 4, vec![77; 16]).unwrap();
        let origin = [roof[0] + 1, roof[1]];
        let outlined = SceneCursor {
            order: 0,
            art: (&small, origin),
            outline: Some((ring, origin, 3)),
            ..cursor
        };
        let image = draw(view, Vec::new(), outlined);
        let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
        assert_eq!(pixel(origin), 200, "the roof covers the cursor");
        assert_eq!(pixel(roof), other, "the ring is drawn over the roof");
        // Without the outline the roof shows there.
        let plain = SceneCursor {
            outline: None,
            ..outlined
        };
        let image = draw(view, Vec::new(), plain);
        assert_eq!(
            image.pixels()[roof[1] as usize * 640 + roof[0] as usize],
            200
        );
    }

    #[test]
    fn spec014_objects_take_their_hex_light_and_roofs_the_ambient() {
        let map = preview();
        let palette = grey_palette();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (true, 2),
            FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
        );
        art.insert(
            (false, 0x0200_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 200)).unwrap(),
        );
        let art = Keyed(art);
        let mut view = view();
        view.roofs = true;
        let mut tiles = vec![0; 40000];
        tiles[20300] = 65536;
        let light = MapLight {
            palette: &palette,
            ambient: 32768,
            tiles: &tiles,
        };
        let lit = object(&map, 1, 0x0200_0001, 20300);
        let dark = object(&map, 2, 0x0200_0001, 19500);
        let mut free = object(&map, 3, 0x0200_0001, 0);
        free.hex = None;
        free.screen = [600, 10];
        let mut image = render_floor(&map, view, &art).unwrap();
        let scene_light = MapScene {
            light: Some(light),
            ..MapScene::default()
        };
        draw_map_scene(
            &mut image,
            &map,
            &scene(vec![lit, dark, free]),
            view,
            &art,
            scene_light,
        )
        .unwrap();
        let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
        let at = |hex: i32| {
            let [x, y] = view.grid.hex_center(hex).unwrap();
            [x, y - 1]
        };
        assert_eq!(pixel(at(20300)), 200);
        assert_eq!(pixel(at(19500)), 96);
        // Without a hex: full light.
        assert_eq!(pixel([600, 10]), 200);
        // The roof square of the fixture map at the ambient light.
        assert_eq!(pixel([256, 84]), 96);
    }

    #[test]
    fn spec014_cutout_relights_inside_the_mask() {
        let map = preview();
        let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
        let palette = grey_palette();
        let none = |_| 0;
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (false, 0x0300_0001),
            FrmArchive::parse(&fixture::frm(2, 2, 230)).unwrap(),
        );
        let art = Keyed(art);
        let view = view();
        let cutout = Cutout {
            mask: mask.frame(0, 0).unwrap(),
            hex: 20100,
            displacement: [0; 2],
            hidden: false,
            extended: &none,
            palette: &palette,
        };
        let tiles = vec![0; 40000];
        let light = MapLight {
            palette: &palette,
            ambient: 32768,
            tiles: &tiles,
        };
        let draw = |cut: Option<&Cutout>| {
            let mut image = IndexedImage::new(640, 380).unwrap();
            let wall = object(&map, 1, 0x0300_0001, 20300);
            let scene_cut = MapScene {
                cutout: cut,
                light: Some(light),
                outlines: None,
                floating: &[],
                cursor: None,
            };
            draw_map_scene(&mut image, &map, &scene(vec![wall]), view, &art, scene_cut).unwrap();
            let [x, y] = view.grid.hex_center(20300).unwrap();
            image.pixels()[y as usize * 640 + x as usize]
        };
        // Outside a cutout 230 is exempt from lighting; through the mask it
        // is relit (grey 28 to 14) and then blended at weight 64 over black
        // (grey 7, index 56).
        assert_eq!(draw(None), 230);
        assert_eq!(draw(Some(&cutout)), palette.mask_blend(112, 0, 64));
        assert_eq!(draw(Some(&cutout)), 56);
    }

    #[test]
    #[ignore = "needs the local corpus"]
    fn spec014_fixture_q_corpus_translucency() {
        let dir = std::env::var_os("VAULT_CORPUS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
            })
            .join("fallout1-unpacked/color.pal");
        let Ok(bytes) = std::fs::read(&dir) else {
            eprintln!("skipping: no local corpus at {}", dir.display());
            return;
        };
        let palette = Palette::parse(&bytes).unwrap();
        // SPEC-001 §3.7: entries 0 and 229-255 unmapped; additive mixes.
        assert!((0..=255).all(|index| palette.is_mapped(index) == (1..229).contains(&index)));
        assert_eq!(palette.additive(215, 215), 196);
        assert_eq!(palette.additive(220, 220), 220);
        // SPEC-014 Fixture I seen cells: intensity column 64.
        assert_eq!(
            [50, 150, 100].map(|index| palette.intensity(index, 64)),
            [52, 153, 228]
        );
        // The five foreground colors.
        let foregrounds = TRANSLUCENT.map(|(_, key, ..)| {
            let key = u32::from(key);
            palette.lookup_rgb5([
                (key >> 10) as u8 & 31,
                (key >> 5) as u8 & 31,
                key as u8 & 31,
            ])
        });
        assert_eq!(foregrounds, [33, 35, 220, 58, 133].map(Some));
        // Source 215 over destination 100: 6 as wall translucency, 214 as
        // glass at full light.
        let tiles = vec![65536; 40000];
        let light = MapLight {
            palette: &palette,
            ambient: 65536,
            tiles: &tiles,
        };
        assert_eq!(lit_pixel(215, 100, 0x10000, light), 6);
        assert_eq!(lit_pixel(215, 100, 0x20000, light), 214);
        // SPEC-014 §3.44 the outline keys: red 133 (gray level 2), grey 9,
        // item 58 and other 49.
        use crate::outline::{GREY_KEY, ITEM_KEY, OTHER_KEY, RED_KEY, key_index};
        assert_eq!(
            [RED_KEY, GREY_KEY, ITEM_KEY, OTHER_KEY].map(|key| key_index(&palette, key)),
            [133, 9, 58, 49]
        );
        assert_eq!(palette.gray_level(133, false), 2);
    }

    #[test]
    fn map_composition_uses_exact_floor_roof_and_object_coordinates() {
        let map = MapFile::parse(&fixture::preview_map(), &|pid| {
            let category = (pid >> 24) as u8;
            Ok(Prototype::parse(&fixture::prototype(
                category,
                if category == 0 { 1 } else { 5 },
            ))?
            .info())
        })
        .unwrap();
        // As in shipped data, the blank tile's pixels are all transparent index 0.
        let art = Art {
            tile: FrmArchive::parse(&fixture::frm(80, 36, 7)).unwrap(),
            blank: FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
            object: FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
        };
        let mut view = MapView::new(&map, 640, 380).unwrap();
        let image = render_map(&map, view, &art).unwrap();
        let pixel = |image: &IndexedImage, x: usize, y: usize| image.pixels()[y * 640 + x];
        assert_eq!(pixel(&image, 256, 180), 7);
        assert_eq!(pixel(&image, 256, 84), 7);
        assert_eq!(pixel(&image, 319, 189), 20); // Hex 20100 bottom-center, no frame increment.
        view.roofs = false;
        assert_eq!(pixel(&render_map(&map, view, &art).unwrap(), 256, 84), 0);
        view.pan(32, -12);
        assert_eq!(view.hover([352, 178]).hex, Some(20100));
        assert_eq!(pixel(&render_map(&map, view, &art).unwrap(), 351, 177), 20);
        view.elevation = 1;
        let image = render_map(&map, view, &art).unwrap();
        assert!(!image.pixels().contains(&20)); // No elevation-0 objects leak into other layers.
        // Floor index 1 is drawn like any other floor art (only roof index 1 is
        // exempt), so an opaque blank-tile image is visible under the view.
        let opaque = Art {
            blank: FrmArchive::parse(&fixture::frm(80, 36, 9)).unwrap(),
            ..art
        };
        let mut view = MapView::new(&map, 640, 380).unwrap();
        view.roofs = false;
        assert_eq!(pixel(&render_map(&map, view, &opaque).unwrap(), 256, 84), 9);
    }

    /// SPEC-014 Fixture T floor light: an even base hex, every vertex
    /// light 65536 except V3 = 0.
    #[test]
    fn spec014_fixture_t_floor_light_map() {
        let mut lights = [65536; 10];
        lights[3] = 0;
        let mut map = [0; FLOOR_MAP];
        let mut written = [false; FLOOR_MAP];
        fill_floor_map(&lights, &mut map, &mut written);
        let entry = |[px, py]: [usize; 2]| 160 + 80 * py + px;
        for (pixel, expected_entry, value, column) in [
            ([32, 10], 992, 0, 0),
            ([40, 10], 1000, 16384, 32),
            ([48, 10], 1008, 32768, 64),
            ([16, 4], 496, 49152, 96),
            ([32, 0], 192, 55454, 108),
            ([20, 20], 1780, 50410, 98),
            ([0, 10], 960, 65536, 128),
            ([64, 10], 1024, 65536, 128),
            ([48, 22], 1968, 65536, 128),
        ] {
            assert_eq!(entry(pixel), expected_entry, "{pixel:?}");
            assert_eq!(map[expected_entry], value, "{pixel:?}");
            assert_eq!(map[expected_entry] / 512, column, "{pixel:?}");
        }
        // (33, 35) reads entry 2993, which no triangle writes: light 0 in
        // R1. Under the safe rule it reads the nearest written entry above
        // in its column.
        assert_eq!(entry([33, 35]), 2993);
        assert!(!written[2993]);
        assert_eq!(map[2993], 0);
        let source = usize::from(floor_sources()[2993]);
        assert_eq!(source % 80, 33);
        assert!(source < 2993 && written[source]);
        assert!((source + 80..2993).step_by(80).all(|at| !written[at]));
        // Written entries read themselves.
        assert_eq!(floor_sources()[1000], 1000);
    }

    /// The vertex lights: the larger of the ambient and the hex light
    /// (capped at 65536), with the even or odd offsets, off the grid 0.
    #[test]
    fn spec014_floor_vertices_take_the_base_hex_parity() {
        let palette = grey_palette();
        let mut tiles = vec![0_i64; 40000];
        for (hex, light) in tiles.iter_mut().enumerate() {
            *light = hex as i64 % 5000 * 10;
        }
        tiles[20100 + 599] = 100_000;
        let light = MapLight {
            palette: &palette,
            ambient: 500,
            tiles: &tiles,
        };
        let floor = FloorLight::new(light);
        let even = floor.vertices(20100);
        assert_eq!(even[0], 99 * 10);
        assert_eq!(even[3], 299 * 10);
        assert_eq!(even[9], 65536);
        let odd = floor.vertices(20101);
        assert_eq!(odd[0], (101 - 201 + 5000) * 10);
        assert_eq!(odd[3], 100 * 10);
        assert_eq!(odd[9], (101 + 399) * 10);
        // Off the grid: hex light 0, so the ambient.
        let edge = floor.vertices(1);
        assert_eq!(edge[0], 500);
    }

    /// A tile with all ten lights equal uses the lit draw (229 and up
    /// unlit); an interpolated tile leaves 229-255 unlit too (the safe
    /// rule) and lights the rest per pixel.
    #[test]
    fn spec014_floor_tiles_light_uniform_and_interpolated() {
        let map = preview();
        let palette = grey_palette();
        let mut art = std::collections::BTreeMap::new();
        art.insert(
            (true, 1),
            FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
        );
        art.insert(
            (true, 2),
            FrmArchive::parse(&fixture::frm(80, 36, 240)).unwrap(),
        );
        let art = Keyed(art);
        let view = view();
        let tiles = vec![0_i64; 40000];
        let draw = |ambient: i32, tiles: &[i64]| {
            render_floor_lit(
                &map,
                view,
                &art,
                MapLight {
                    palette: &palette,
                    ambient,
                    tiles,
                },
            )
            .unwrap()
        };
        let image = draw(40000, &tiles);
        // Fixture T: the lit draw at column trunc(40000 / 512) = 78.
        let lit = palette.intensity(200, 78);
        assert_ne!(lit, 200);
        assert!(image.pixels().contains(&lit));
        assert!(!image.pixels().contains(&200));
        // Square 5050 (tile 2, index 240) stays unlit.
        assert!(image.pixels().contains(&240));
        // A bright hex makes nearby tiles interpolated: some pixels of index
        // 200 get a level between ambient and full light.
        let mut bright = tiles.clone();
        bright[20100] = 65536;
        let image = draw(0, &bright);
        let levels: std::collections::BTreeSet<u8> = image
            .pixels()
            .iter()
            .copied()
            .filter(|pixel| *pixel != 0 && *pixel != 240)
            .collect();
        assert!(levels.len() > 2, "{levels:?}");
        assert!(image.pixels().contains(&240));
    }
}
