use crate::{ImageError, IndexedImage};
use std::sync::OnceLock;
use vault_formats::{
    HexGrid, MapFile,
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
        let (origin, anchor) = placement(view, frame, shift, object)?;
        objects.push((
            (
                !object.flags.contains(BasicFlags::FLAT),
                anchor[1],
                anchor[0],
                object.id,
            ),
            frame,
            origin,
        ));
    }
    objects.sort_by_key(|(key, _, _)| *key);
    for (_, frame, origin) in objects {
        image.blit(frame, origin);
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
                image.blit(frame, [corner[0], corner[1] - 96]);
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
    if view.roofs {
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
    for object in &objects.elevations[view.elevation as usize] {
        if object.flags.contains(BasicFlags::HIDDEN) {
            continue;
        }
        let category = Category::from_pid(object.pid).ok()?;
        let Some((frame, shift)) =
            art.frame(category, object.art_id, object.orientation, object.frame)
        else {
            continue;
        };
        let (origin, anchor) = placement(view, frame, shift, object).ok()?;
        if sample(frame, origin) {
            let key = (
                !object.flags.contains(BasicFlags::FLAT),
                anchor[1],
                anchor[0],
                object.id,
            );
            hits.push((key, object.id));
        }
    }
    hits.sort_by_key(|(key, _)| std::cmp::Reverse(*key));
    Some(hits.into_iter().map(|(_, id)| id).collect())
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
