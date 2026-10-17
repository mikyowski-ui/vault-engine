use crate::{ImageError, IndexedImage};
use vault_formats::{
    HexGrid, MapFile,
    frm::FrmFrame,
    geometry::GeometryError,
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
pub fn render_map(
    map: &MapFile,
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
    let mut squares: Vec<_> = (0..10000)
        .map(|index| {
            (
                view.grid
                    .square_corner(index, false)
                    .expect("square in range"),
                index,
            )
        })
        .collect();
    squares.sort_by_key(|([x, y], index)| (*y, *x, *index));
    for (corner, index) in &squares {
        let tile = map
            .tile(view.elevation as usize, *index as usize)
            .expect("validated map grid");
        if tile.floor() != 1
            && !tile.floor_hidden()
            && let Some((frame, _)) = art.frame(Category::Tile, u32::from(tile.floor()), 0, 0)
        {
            image.blit(frame, *corner);
        }
    }
    let mut objects = Vec::new();
    for object in &map.objects.elevations[view.elevation as usize] {
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
        let (origin, anchor) = if let Some(hex) = object.hex {
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
        };
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
        for (corner, index) in squares {
            let tile = map
                .tile(view.elevation as usize, index as usize)
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
        object: FrmArchive,
    }
    impl MapArt for Art {
        fn frame(
            &self,
            category: Category,
            _id: u32,
            direction: u8,
            frame: usize,
        ) -> Option<(&FrmFrame, [i16; 2])> {
            let archive = if category == Category::Tile {
                &self.tile
            } else {
                &self.object
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
        let art = Art {
            tile: FrmArchive::parse(&fixture::frm(80, 36, 7)).unwrap(),
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
    }
}
