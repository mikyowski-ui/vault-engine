#[cfg(doc)]
use super::MapScene;
use super::{
    MapArt, MapView,
    backdrop::{BackdropView, Suppress},
    enhanced,
    lighting::{NO_TRANSLUCENCY, TRANSLUCENCY},
    roof::{Cutout, RoofCutaway},
};
use std::sync::OnceLock;
use vault_formats::{
    HexGrid, MapFile,
    frm::FrmFrame,
    geometry::GeometryError,
    map::{MapObject, MapObjects},
    pro::{BasicFlags, Category},
};

/// The hex under a view point by the SPEC-003 picking rule, without the
/// grid bounds: `column + 200 × row` even off the grid, so hex differences
/// are the same wherever the points lie.
pub(super) fn hex_under(grid: HexGrid, [x, y]: [i64; 2]) -> i64 {
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
pub(super) fn lattice() -> &'static [Vec<(i64, u8, u8)>; 2] {
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
pub(super) fn visited_hexes(view: MapView) -> impl Iterator<Item = i32> {
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
/// drawn last. The `extra` hexes ([`overscan_hexes`], empty in Vanilla)
/// are visited too, merged into the ascending order.
#[cfg(test)]
pub(super) fn draw_sequence(
    view: MapView,
    objects: &[MapObject],
    extra: &[i32],
) -> (Vec<usize>, Vec<usize>) {
    draw_sequence_with(view, objects, extra, None)
}

/// [`draw_sequence`] skipping, as it skips hidden objects, the objects a
/// backdrop suppresses: they are not drawn, outlined or picked, and keep
/// their list positions ([`ListIndexes`] counts them).
pub(super) fn draw_sequence_with(
    view: MapView,
    objects: &[MapObject],
    extra: &[i32],
    suppress: Option<&Suppress>,
) -> (Vec<usize>, Vec<usize>) {
    let mut placed: Vec<(i32, bool, usize)> = Vec::new();
    let mut hexless = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        if object.flags.contains(BasicFlags::HIDDEN)
            || suppress.is_some_and(|suppress| suppress.hides(object))
        {
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
    let mut hexes: Vec<i32> = visited_hexes(view).collect();
    if !extra.is_empty() {
        hexes.extend_from_slice(extra);
        hexes.sort_unstable();
        hexes.dedup();
    }
    for hex in hexes {
        let start = placed.partition_point(|(at, ..)| *at < hex);
        for (_, is_upright, index) in placed[start..].iter().take_while(|(at, ..)| *at == hex) {
            if *is_upright {
                upright.push(*index);
            } else {
                flat.push(*index);
            }
        }
    }
    flat.extend(upright);
    (flat, hexless)
}

/// Whether a refresh of `view` draws a cursor at `hex`: on a hex it visits.
pub(super) fn cursor_visited(view: MapView, hex: i32, extra: &[i32]) -> bool {
    extra.contains(&hex) || visited_hexes(view).any(|visited| visited == hex)
}

/// The `enhanced` overscan: the hexes of the visible objects (not hidden,
/// on a hex, with art) whose sprite rectangle reaches into `view`. Merged
/// into the visited hexes, they let a canvas tile draw and pick a sprite
/// of any size whose hex lies in a neighbouring tile beyond the visited
/// window's margins (320 pixels each side, 240 above and below): the
/// margin is each sprite's own extent, so no sprite is cut at a seam. The
/// global ascending hex order keeps the tiles' overlaps consistent.
#[cfg(test)]
pub(super) fn overscan_hexes(view: MapView, objects: &[MapObject], art: &impl MapArt) -> Vec<i32> {
    overscan_hexes_with(view, objects, art, None)
}

/// [`overscan_hexes`] without the objects a backdrop suppresses (they are
/// not drawn, as hidden objects are not).
pub(super) fn overscan_hexes_with(
    view: MapView,
    objects: &[MapObject],
    art: &impl MapArt,
    suppress: Option<&Suppress>,
) -> Vec<i32> {
    let (width, height) = (i64::from(view.width), i64::from(view.height));
    objects
        .iter()
        .filter(|object| !object.flags.contains(BasicFlags::HIDDEN))
        .filter(|object| suppress.is_none_or(|suppress| !suppress.hides(object)))
        .filter_map(|object| {
            let hex = object.hex?;
            let category = Category::from_pid(object.pid).ok()?;
            let (frame, shift) =
                art.frame(category, object.art_id, object.orientation, object.frame)?;
            let ([x, y], _) = placement(view, frame, shift, object).ok()?;
            let reaches = x < width
                && y < height
                && x + i64::from(frame.width) > 0
                && y + i64::from(frame.height) > 0;
            reaches.then_some(hex)
        })
        .collect()
}

/// SPEC-022 §2.1 the list index of each drawn entry: its position in its
/// hex's object list. The list holds the elevation's objects on that hex
/// (hidden ones included) in insertion order, and the cursor takes no place
/// in it (revision 4). Objects of other elevations are in other lists, so
/// they never share a hex list here.
pub(crate) struct ListIndexes {
    /// By object index; objects without a hex have 0.
    pub objects: Vec<u32>,
}

impl ListIndexes {
    pub fn new(objects: &[MapObject]) -> Self {
        let mut counts: std::collections::BTreeMap<i32, u32> = Default::default();
        let mut indexes = vec![0; objects.len()];
        for (index, object) in objects.iter().enumerate() {
            let Some(hex) = object.hex else {
                continue;
            };
            let count = counts.entry(hex).or_default();
            indexes[index] = *count;
            *count += 1;
        }
        Self { objects: indexes }
    }
}

/// Where an object's frame is drawn: its top-left corner and its anchor
/// (the sort point). A hex-placed object anchors at the hex center plus the
/// art shift and its displacement, any other at its screen position.
pub(super) fn placement(
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
    pick_with(map, objects, view, art, point, cutout, None, false, None)
}

/// [`pick_candidates`] over a scene with a pre-rendered `backdrop` (brief
/// §3.8.5-6): an object the backdrop suppresses is never a candidate, and
/// a sprite pixel is not a hit where the backdrop draws a group pixel
/// (colour not 0, depth at least 1) that is drawn after the object: a
/// group of hex B covers an upright object on hex h when B ≥ h (ties
/// covering, as a group draws after its hex's objects) and every flat
/// object (the flat pass draws before every group). Fading never
/// uncovers a pixel. Without a backdrop this is [`pick_candidates`].
pub fn pick_candidates_backdrop(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutout: Option<&Cutout>,
    backdrop: Option<&BackdropView>,
) -> Option<Vec<(u32, bool)>> {
    pick_with(
        map, objects, view, art, point, cutout, None, false, backdrop,
    )
}

/// [`pick_candidates_cut`] over a scene with a pre-rendered `backdrop`, as
/// [`pick_candidates_backdrop`] picks.
#[allow(clippy::too_many_arguments)]
pub fn pick_candidates_cut_backdrop(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutout: Option<&Cutout>,
    cutaway: Option<RoofCutaway>,
    backdrop: Option<&BackdropView>,
) -> Option<Vec<(u32, bool)>> {
    pick_with(
        map, objects, view, art, point, cutout, cutaway, true, backdrop,
    )
}

/// [`pick_candidates`] with the `enhanced` roof cutaway: a roof pixel the
/// cutaway removes at `point` (inside its inner radius, or dithered out in
/// its ring at that exact pixel; [`RoofCutaway::keeps`]) does not stop the
/// pick, so the pick matches what the scene shows. It picks as an enhanced
/// canvas tile draws, with the overscan ([`MapScene::overscan`]).
pub fn pick_candidates_cut(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutout: Option<&Cutout>,
    cutaway: Option<RoofCutaway>,
) -> Option<Vec<(u32, bool)>> {
    pick_with(map, objects, view, art, point, cutout, cutaway, true, None)
}

#[allow(clippy::too_many_arguments)]
fn pick_with(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    point: [i32; 2],
    cutout: Option<&Cutout>,
    cutaway: Option<RoofCutaway>,
    overscan: bool,
    backdrop: Option<&BackdropView>,
) -> Option<Vec<(u32, bool)>> {
    if view.elevation > 2 {
        return None;
    }
    let sample = |frame: &FrmFrame, origin: [i64; 2]| enhanced::sample(frame, origin, point);
    let in_mask = cutout.is_some_and(|cut| {
        cut.origin(view)
            .and_then(|mask| cut.weight(mask, point.map(i64::from)))
            .is_some_and(|weight| weight != 0)
    });
    let cut_away = cutaway.is_some_and(|cutaway| !cutaway.keeps(view.grid, point.map(i64::from)));
    if view.roofs && !in_mask && !cut_away && enhanced::roof_pixel_at(map, view, art, point)? {
        return None;
    }
    let mut hits = Vec::new();
    let objects = &objects.elevations[view.elevation as usize];
    let suppress = Suppress::of(backdrop, view);
    let extra = if overscan {
        overscan_hexes_with(view, objects, art, suppress.as_ref())
    } else {
        Vec::new()
    };
    let (placed, hexless) = draw_sequence_with(view, objects, &extra, suppress.as_ref());
    // The backdrop group drawn at the point, if any.
    let covering = backdrop.and_then(|backdrop| backdrop.group_at(view, point));
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
            if let (Some(group), Some(hex)) = (covering, object.hex)
                && (object.flags.contains(BasicFlags::FLAT) || i32::from(group) >= hex)
            {
                continue;
            }
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
