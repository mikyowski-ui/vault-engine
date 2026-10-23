use crate::{ImageError, IndexedImage};
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

/// A blit whose pixels inside the mask are blended by their weight.
fn blit_cut(
    image: &mut IndexedImage,
    frame: &FrmFrame,
    origin: [i64; 2],
    cut: &Cutout,
    mask: [i64; 2],
) {
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    let frame_width = i64::from(frame.width);
    let source = frame.pixels();
    let pixels = image.pixels_mut();
    for y in origin[1].max(0)..(origin[1] + i64::from(frame.height)).min(height) {
        for x in origin[0].max(0)..(origin[0] + frame_width).min(width) {
            let color = source[((y - origin[1]) * frame_width + x - origin[0]) as usize];
            if color == 0 {
                continue;
            }
            let target = &mut pixels[(y * width + x) as usize];
            *target = match cut.weight(mask, [x, y]) {
                Some(weight) => cut.palette.mask_blend(color, *target, weight),
                None => color,
            };
        }
    }
}

/// The draw order: flat objects first, then the rest; within each pass in
/// object order, ascending hex and then the order on the hex (an object
/// without a hex, placed by screen position, after the placed ones).
fn draw_key(object: &MapObject) -> (bool, i64) {
    (
        !object.flags.contains(BasicFlags::FLAT),
        object.hex.map_or(i64::MAX, i64::from),
    )
}

/// [`render_map_with`] with the player's see-through cutout. Without a
/// cutout (no mask art, or no player hex) everything draws normally (the
/// SPEC-014 §3.37 safe rule).
pub fn render_map_cut(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    cutout: Option<&Cutout>,
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
    let cutout = cutout.and_then(|cut| Some((cut, cut.origin(view)?)));
    // A square whose corner lies past the right or bottom edge draws nothing.
    let beyond = |[x, y]: [i64; 2]| x >= i64::from(view.width) || y >= i64::from(view.height);
    for (index, offset) in square_order() {
        let corner = corner(origin, *offset);
        if beyond(corner) {
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
            image.blit(frame, corner);
        }
    }
    let table = objects;
    let mut objects = Vec::new();
    for object in &table.elevations[view.elevation as usize] {
        if object.flags.contains(BasicFlags::HIDDEN) {
            continue;
        }
        let Ok(category) = Category::from_pid(object.pid) else {
            continue;
        };
        let Some((frame, shift)) =
            art.frame(category, object.art_id, object.orientation, object.frame)
        else {
            continue;
        };
        let (origin, _) = placement(view, frame, shift, object)?;
        objects.push((draw_key(object), object, frame, origin));
    }
    // A stable sort keeps the order on each hex.
    objects.sort_by_key(|(key, ..)| *key);
    for (_, object, frame, origin) in objects {
        match cutout {
            Some((cut, mask)) if cut.cuts(view, object) => {
                blit_cut(&mut image, frame, origin, cut, mask);
            }
            _ => image.blit(frame, origin),
        }
    }
    if view.roofs {
        for (index, offset) in square_order() {
            let corner = corner(origin, *offset);
            if beyond([corner[0], corner[1] - 96]) {
                continue;
            }
            let tile = map
                .tile(view.elevation as usize, *index as usize)
                .expect("validated map grid");
            if tile.roof() != 1
                && !tile.roof_hidden()
                && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.roof()), 0, 0)
            {
                let corner = [corner[0], corner[1] - 96];
                // Every drawn roof square goes through the mask.
                match cutout {
                    Some((cut, mask)) => blit_cut(&mut image, frame, corner, cut, mask),
                    None => image.blit(frame, corner),
                }
            }
        }
    }
    Ok(image)
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
    for (order, object) in objects.elevations[view.elevation as usize]
        .iter()
        .enumerate()
    {
        if object.flags.contains(BasicFlags::HIDDEN) {
            continue;
        }
        let category = Category::from_pid(object.pid).ok()?;
        let Some((frame, shift)) =
            art.frame(category, object.art_id, object.orientation, object.frame)
        else {
            continue;
        };
        let (origin, _) = placement(view, frame, shift, object).ok()?;
        if sample(frame, origin) {
            let bits = object.flags.0 & TRANSLUCENCY;
            let translucent = bits != 0 && bits != NO_TRANSLUCENCY;
            let inside = in_mask
                && bits == 0
                && matches!((object.art_id >> 24) & 0x0f, 2 | 3)
                && cutout.is_some_and(|cut| cut.oriented(view, object, false));
            hits.push((
                (draw_key(object), order),
                object.id,
                !translucent && !inside,
            ));
        }
    }
    hits.sort_by_key(|(key, ..)| std::cmp::Reverse(*key));
    Some(hits.into_iter().map(|(_, id, solid)| (id, solid)).collect())
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
}
