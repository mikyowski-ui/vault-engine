use crate::{
    ImageError, IndexedImage,
    floating::FloatingSprite,
    outline::{OutlineInks, draw_outline},
};
use vault_formats::{
    HexGrid, MapFile,
    frm::FrmFrame,
    geometry::GeometryError,
    map::{MapObject, MapObjects},
    pro::{BasicFlags, Category},
};

mod backdrop;
mod enhanced;
mod instanced;
mod lighting;
mod objects;
mod roof;
mod tiles;
use crate::layers::{BACKDROP_FLOOR_PRIMARY, FrameLayer, SHROUD_PRIMARY, SortKey};
pub use backdrop::{
    BACKDROP_DEPTH_MAX, BackdropTile, BackdropView, backdrop_chunks, backdrop_rect,
};
use backdrop::{Fade, Layout, Suppress, draw_floor, draw_group};
pub use enhanced::{
    BAYER, RoofPeekParams, ShroudCell, ShroudField, ShroudRecording, Vignette, bayer, draw_shroud,
    draw_vignette, record_shroud, roof_hides, shroud_dark,
};
pub use instanced::{DrawRecord, MapFrame, RefreshReport, SceneRenderer, gather_map_frame};
pub use lighting::MapLight;
use lighting::{FloorLight, Pens, TRANSLUCENT, TileLight, translucent_row};
pub(crate) use objects::ListIndexes;
use objects::{cursor_visited, draw_sequence_with, overscan_hexes_with, placement};
#[cfg(test)]
use objects::{draw_sequence, overscan_hexes};
pub use objects::{
    object_sprite, pick_candidates, pick_candidates_backdrop, pick_candidates_cut,
    pick_candidates_cut_backdrop, pick_object, pick_objects, pick_objects_with,
};
pub(crate) use roof::cutout_parts;
use roof::roof_squares;
pub use roof::{Cutout, RoofCutaway};
use tiles::{check_floor_view, floor_squares, render_floor_recorded, square_offsets};
pub use tiles::{render_floor, render_floor_lit};

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
    /// The `enhanced` preset's roof cutaway around the cursor (`None` in
    /// Vanilla).
    pub roof_cutaway: Option<RoofCutaway>,
    /// The `enhanced` preset's shroud over hexes outside its allowed set
    /// (`None` in Vanilla).
    pub shroud: Option<Shroud<'a>>,
    /// The `enhanced` canvas tiles' overscan ([`overscan_hexes`]): an
    /// object whose sprite reaches into the view is drawn though its hex
    /// lies beyond the visited window, so a sprite larger than the
    /// window's margins is not cut at a tile seam. `false` in Vanilla.
    pub overscan: bool,
    /// The `enhanced` ground-item highlight (Shift held): every drawn item
    /// on a hex whose own outline word draws nothing is outlined with this
    /// word, after the outlines and outside their list of 100, so the
    /// Vanilla outlines never change. `None` in Vanilla.
    pub highlight: Option<u32>,
    /// The pre-rendered backdrop under this refresh, for the view's
    /// elevation (`None`: none, and the refresh is drawn as without the
    /// field). Its depth-0 pixels draw at the end of the floor, each group
    /// in the upright pass after its hex's objects; it suppresses the
    /// objects it lists.
    pub backdrop: Option<&'a BackdropView<'a>>,
}

/// The `enhanced` preset's shroud: view pixels over hexes outside the
/// allowed set (or off the grid) are darkened toward `dark` through an
/// ordered dither, fully dark from two hexes (64 pixels) beyond the
/// set's edge ([`ShroudField::darkens`]). The unlocked view allows the
/// whole grid, so only what lies off it is darkened. It is painted after the roofs
/// and before the outlines, floating text and hexless objects; the
/// in-world cursor's art, drawn in the flat pass (§3.27), is shrouded with
/// the scene, its outline is not. The per-hex distance field is built from
/// `allowed` once and reused while the flags are unchanged. Engine
/// extension.
#[derive(Debug, Clone, Copy)]
pub struct Shroud<'a> {
    /// One flag per hex 0-39999: whether the hex is inside.
    pub allowed: &'a [bool],
    /// The palette index the shroud darkens to.
    pub dark: u8,
}

/// SPEC-014 §3.27 (R1-rev13) the in-world cursor: a presentation object
/// that shows every mode's art, always the top layer of a refresh, lit as
/// any object on its hex. Movement mode adds its outline and AP text, drawn
/// with it. It exists only in the refresh and takes no place in a hex's
/// object list, so scripts, saves, picking and blocking never see it.
#[derive(Clone, Copy)]
pub struct SceneCursor<'a> {
    /// The hex it is at.
    pub hex: i32,
    /// The cursor art and its top-left corner in the view (index 0 is
    /// transparent).
    pub art: (&'a IndexedImage, [i64; 2]),
    /// Movement mode's outline: the hex frame at its top-left corner and
    /// its outline word, drawn right after the art and outside the list of
    /// 100.
    pub outline: Option<(&'a FrmFrame, [i64; 2], u32)>,
    /// The AP-text object's art and top-left corner.
    pub text: Option<(&'a IndexedImage, [i64; 2])>,
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
    draw_map_scene_recorded(image, map, objects, view, art, scene, None)
}

/// SPEC-022 §3.1 one whole refresh of the reference path: the floor (lit
/// by the scene's light, or unlit without one) and then the scene's
/// layers ([`draw_map_scene`]), as the map view draws each view.
pub fn render_map_scene(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    scene: MapScene,
) -> Result<IndexedImage, MapRenderError> {
    render_map_scene_recorded(map, objects, view, art, scene, None)
}

/// [`render_map_scene`] logging every draw to `record`, as (layer, hex or
/// square rank, list index, part): SPEC-022 §3.4's draw-recording hook.
pub(crate) fn render_map_scene_recorded(
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    scene: MapScene,
    mut record: Option<&mut dyn FnMut(DrawRecord)>,
) -> Result<IndexedImage, MapRenderError> {
    let floor_record: Option<&mut dyn FnMut(DrawRecord)> = match record.as_mut() {
        Some(record) => Some(&mut **record),
        None => None,
    };
    let mut image = render_floor_recorded(map, view, art, scene.light, floor_record)?;
    draw_map_scene_recorded(&mut image, map, objects, view, art, scene, record)?;
    Ok(image)
}

/// [`draw_map_scene`] logging every draw to `record`.
fn draw_map_scene_recorded(
    image: &mut IndexedImage,
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &impl MapArt,
    scene: MapScene,
    mut record: Option<&mut dyn FnMut(DrawRecord)>,
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
    let suppress = Suppress::of(scene.backdrop, view);
    let extra = if scene.overscan {
        overscan_hexes_with(view, objects, art, suppress.as_ref())
    } else {
        Vec::new()
    };
    let cursor = scene
        .cursor
        .filter(|cursor| cursor_visited(view, cursor.hex, &extra));
    let (placed, hexless) = draw_sequence_with(view, objects, &extra, suppress.as_ref());
    // The list indexes, only when recording.
    let lists = record.is_some().then(|| ListIndexes::new(objects));
    let canvas = [0, 0, i64::from(image.width()), i64::from(image.height())];
    let mut log = |layer: FrameLayer, primary: i64, index: u32, part: u8| {
        if let Some(record) = record.as_mut() {
            record(DrawRecord::new(layer, primary as u32, index, part));
        }
    };
    // The backdrop: its floor pass ends the floor layer, and its groups
    // join the upright pass, each after the objects of its hex.
    let backdrop = scene
        .backdrop
        .and_then(|backdrop| Layout::of(backdrop, view));
    let fade = Fade::new(view, &scene);
    if let Some(floor) = backdrop.as_ref().and_then(|layout| layout.floor.as_ref()) {
        draw_floor(image, floor, &pens, scene.light, view.grid);
        log(FrameLayer::Floor, BACKDROP_FLOOR_PRIMARY.into(), 0, 0);
    }
    let mut groups = backdrop
        .as_ref()
        .map(|layout| layout.groups.iter().peekable());
    let mut outlines = Vec::new();
    let mut highlights = Vec::new();
    for index in placed {
        let object = &objects[index];
        // Every group of a lower hex draws before an upright object.
        if !object.flags.contains(BasicFlags::FLAT)
            && let Some(groups) = groups.as_mut()
        {
            let hex = object.hex.unwrap_or_default();
            while let Some((group_hex, group)) = groups.next_if(|(at, _)| i32::from(**at) < hex) {
                draw_group(image, *group_hex, group, &pens, scene.light, &fade);
                log(
                    FrameLayer::Upright,
                    (*group_hex).into(),
                    DrawRecord::BACKDROP_INDEX,
                    0,
                );
            }
        }
        let Some((frame, origin)) = sprite(object)? else {
            continue;
        };
        let level = level(object.hex);
        let cut = match cutout {
            Some((cut, mask)) if cut.cuts(view, object) => {
                pens.draw_cut(image, frame, origin, level, cut, mask);
                Some((cut, mask))
            }
            _ => {
                pens.draw(image, frame, origin, object.flags.0, level);
                None
            }
        };
        if let Some(lists) = &lists {
            let layer = if object.flags.contains(BasicFlags::FLAT) {
                FrameLayer::Flat
            } else {
                FrameLayer::Upright
            };
            let hex = object.hex.unwrap_or_default().into();
            let rect = frame_rect(frame, origin);
            for (part, ..) in cutout_parts(rect, canvas, cut.map(|(cut, mask)| cut.rect(mask))) {
                log(layer, hex, lists.objects[index], part);
            }
        }
        // At most 100 outlined objects per refresh, in drawing order; an
        // object without a hex is never outlined.
        if outlines.len() < 100
            && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(object.outline))
        {
            outlines.push((frame, origin, ink));
        }
        if let Some(word) = scene.highlight
            && Category::from_pid(object.pid) == Ok(Category::Item)
            && scene
                .outlines
                .and_then(|inks| inks.ink(object.outline))
                .is_none()
            && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(word))
        {
            highlights.push((frame, origin, ink));
        }
    }
    // The groups after the last upright object, and those of hexes the
    // pass did not visit, before the roofs.
    for (group_hex, group) in groups.into_iter().flatten() {
        draw_group(image, *group_hex, group, &pens, scene.light, &fade);
        log(
            FrameLayer::Upright,
            (*group_hex).into(),
            DrawRecord::BACKDROP_INDEX,
            0,
        );
    }
    if view.roofs {
        let ambient = scene
            .light
            .map_or(128, |light| MapLight::level(i64::from(light.ambient)));
        for (rank, frame, corner) in roof_squares(map, view, art) {
            let size = [i64::from(frame.width), i64::from(frame.height)];
            // Every drawn roof square goes through the mask; the
            // `enhanced` cutaway removes the pixels around its center.
            let mask = match (cutout, scene.roof_cutaway) {
                (_, Some(cutaway)) if cutaway.touches(corner, size) => {
                    let keeps = |at| cutaway.keeps(view.grid, at);
                    pens.draw_roof_peek(image, frame, corner, ambient, cutout, keeps);
                    cutout.map(|(cut, mask)| cut.rect(mask))
                }
                (Some((cut, mask)), _) => {
                    pens.draw_cut(image, frame, corner, ambient, cut, mask);
                    Some(cut.rect(mask))
                }
                (None, _) => {
                    pens.draw(image, frame, corner, 0, ambient);
                    None
                }
            };
            if lists.is_some() {
                for (part, ..) in cutout_parts(frame_rect(frame, corner), canvas, mask) {
                    log(FrameLayer::Roof, rank.into(), 0, part);
                }
            }
        }
    }
    // The `enhanced` shroud covers the scene, roofs included, but not the
    // outlines, floating text and hexless objects drawn after it.
    if let Some(shroud) = scene.shroud {
        let field = enhanced::cached_field(shroud.allowed);
        draw_shroud(image, view.grid, &field, shroud.dark);
        log(FrameLayer::Roof, SHROUD_PRIMARY.into(), 0, 0);
    }
    // The refresh covers the whole view, which is the map buffer.
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    for (position, (frame, origin, ink)) in outlines.into_iter().chain(highlights).enumerate() {
        draw_outline(
            image,
            frame,
            origin,
            ink,
            [0, 0, width, height],
            [width, height],
        );
        log(FrameLayer::Outline, 0, position as u32, 0);
    }
    // SPEC-014 §3.46: floating text, in creation order, transparently, at
    // its creation hex's current window position plus its offset; not
    // hidden by roofs and whether or not its owner shows.
    for (rank, text) in scene.floating.iter().enumerate() {
        let Ok([x, y]) = view.grid.hex_reference(text.hex) else {
            continue;
        };
        image.blit_image(
            &text.image,
            [x + i64::from(text.offset[0]), y + i64::from(text.offset[1])],
        );
        log(FrameLayer::FloatingText, 0, rank as u32, 0);
    }
    for (rank, index) in hexless.into_iter().enumerate() {
        let object = &objects[index];
        if let Some((frame, origin)) = sprite(object)? {
            pens.draw(image, frame, origin, object.flags.0, 128);
            log(FrameLayer::NoHex, 0, rank as u32, 0);
        }
    }
    // SPEC-014 §3.27 (R1-rev13): the cursor is the top layer, lit as any
    // object on its hex, with nothing drawn after it.
    if let Some(cursor) = cursor {
        let (art, origin) = cursor.art;
        let level = level(Some(cursor.hex));
        pens.draw_layer(image, art, origin, level);
        log(FrameLayer::Cursor, 0, SortKey::CURSOR_ART, 0);
        if let Some((frame, origin, word)) = cursor.outline
            && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(word))
        {
            draw_outline(
                image,
                frame,
                origin,
                ink,
                [0, 0, width, height],
                [width, height],
            );
            log(FrameLayer::Cursor, 0, SortKey::CURSOR_OUTLINE, 0);
        }
        if let Some((text, origin)) = cursor.text {
            pens.draw_layer(image, text, origin, level);
            log(FrameLayer::Cursor, 0, SortKey::CURSOR_TEXT, 0);
        }
    }
    Ok(())
}

/// A frame's rectangle `[x0, y0, x1, y1]` at `origin`.
fn frame_rect(frame: &FrmFrame, origin: [i64; 2]) -> [i64; 4] {
    [
        origin[0],
        origin[1],
        origin[0] + i64::from(frame.width),
        origin[1] + i64::from(frame.height),
    ]
}

#[cfg(test)]
use lighting::{FLOOR_MAP, fill_floor_map, floor_sources};
#[cfg(test)]
use objects::{hex_under, lattice, visited_hexes};
#[cfg(test)]
use roof::{K, TRANSPARENCY_END};
#[cfg(test)]
use tiles::square_order;
#[cfg(test)]
use vault_fixtures::world as fixture;
#[cfg(test)]
use vault_formats::Palette;
#[cfg(test)]
mod backdrop_tests;
#[cfg(test)]
mod enhanced_tests;
#[cfg(test)]
mod instanced_tests;
#[cfg(test)]
mod tests;
