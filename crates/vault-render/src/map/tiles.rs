use super::{DrawRecord, MapArt, MapLight, MapRenderError, MapView, lighting::FloorLight};
use crate::{IndexedImage, layers::FrameLayer};
use std::sync::OnceLock;
use vault_formats::{HexGrid, MapFile, frm::FrmFrame, geometry::GeometryError, pro::Category};

/// The squares in drawing order, by corner row, then column, then index,
/// each with its floor corner's offset from the grid origin. A corner is the
/// grid origin plus that fixed offset, so the order and the offsets are the
/// same for every view and are computed once (P-02: a frame only adds its
/// origin instead of recomputing 10000 corners).
pub(super) fn square_order() -> &'static [(i32, [i64; 2])] {
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
pub(super) fn square_offsets() -> &'static [[i64; 2]] {
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
pub(super) fn corner(origin: [i64; 2], offset: [i64; 2]) -> [i64; 2] {
    [origin[0] + offset[0], origin[1] + offset[1]]
}

/// The floor squares of the view (SPEC-003 §2.8), unlit.
pub fn render_floor(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
) -> Result<IndexedImage, MapRenderError> {
    render_floor_recorded(map, view, art, None, None)
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
    render_floor_recorded(map, view, art, Some(light), None)
}

/// [`render_floor_lit`] (with a light) or [`render_floor`] (without),
/// logging each drawn square to `record` (SPEC-022 §3.4 order parity).
pub(super) fn render_floor_recorded(
    map: &MapFile,
    view: MapView,
    art: &impl MapArt,
    light: Option<MapLight>,
    mut record: Option<&mut dyn FnMut(DrawRecord)>,
) -> Result<IndexedImage, MapRenderError> {
    check_floor_view(view)?;
    let mut image = IndexedImage::new(view.width, view.height)?;
    let mut floor_light = light.map(FloorLight::new);
    for (rank, frame, corner) in floor_squares(map, view, art) {
        let drawn = match &mut floor_light {
            Some(floor_light) => floor_light.draw(&mut image, view, frame, corner),
            None => {
                image.blit(frame, corner);
                true
            }
        };
        if drawn && let Some(record) = record.as_mut() {
            record(DrawRecord::new(FrameLayer::Floor, rank, 0, 0));
        }
    }
    Ok(image)
}

/// The checks a floor refresh makes before drawing: the elevation, and a
/// grid origin within the i32 range.
pub(super) fn check_floor_view(view: MapView) -> Result<(), MapRenderError> {
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
    Ok(())
}

/// The floor squares a refresh draws, in drawing order: each one's rank
/// (its position in [`square_order`]), art and corner. A square whose
/// corner lies past the view, hidden, or without art is skipped.
pub(super) fn floor_squares<'a, A: MapArt>(
    map: &'a MapFile,
    view: MapView,
    art: &'a A,
) -> impl Iterator<Item = (u32, &'a FrmFrame, [i64; 2])> + use<'a, A> {
    let origin = view.grid.origin;
    square_order()
        .iter()
        .enumerate()
        .filter_map(move |(rank, (index, offset))| {
            let corner = corner(origin, *offset);
            if beyond(view, corner) {
                return None;
            }
            let tile = map
                .tile(view.elevation as usize, *index as usize)
                .expect("validated map grid");
            // SPEC-003 §2.8 exempts only roof index 1 from drawing; floor
            // index 1 (the ordinary blank tile) draws its art like any
            // other floor.
            if tile.floor_hidden() {
                return None;
            }
            let (frame, _) = art.frame(Category::Tile, u32::from(tile.floor()), 0, 0)?;
            Some((rank as u32, frame, corner))
        })
}

/// A square whose corner lies past the right or bottom edge draws nothing.
pub(super) fn beyond(view: MapView, [x, y]: [i64; 2]) -> bool {
    x >= i64::from(view.width) || y >= i64::from(view.height)
}
