//! SPEC-022 §3.2-§3.5 and §3.12 the instanced path over the map view: the
//! draw items of a refresh gathered from the same world, view and scene
//! the reference path ([`render_map_scene`](super::render_map_scene))
//! draws, as sprite instances with their keys ([`gather_map_frame`]), and
//! the renderer that picks a path and falls back to the reference
//! ([`SceneRenderer`]).
//!
//! Every refresh of the map view redraws the whole view (R is the whole
//! canvas): the reference path has no partial refresh, and the enhanced
//! canvas is drawn as separate views.
use super::{
    Cutout, FloorLight, ListIndexes, MapArt, MapLight, MapRenderError, MapScene, MapView,
    TRANSLUCENT, TileLight,
    backdrop::{Fade, Layout, Shade, Suppress, floor_level},
    check_floor_view, cursor_visited, cutout_parts, draw_sequence_with,
    enhanced::{self, RoofPeekParams},
    floor_squares, frame_rect, overscan_hexes_with, placement, render_map_scene, roof_squares,
    translucent_row,
};
use crate::{
    IndexedImage,
    layers::{
        BACKDROP_FLOOR_PRIMARY, BatchPlan, CanvasView, CutoutPlace, DrawMode, EXEMPT, FrameCounts,
        FrameInputs, FrameLayer, HexLightBuffer, InstanceError, LayerUniforms, LightSource,
        LookupTables, PEEK, RenderPath, SELECT, SortKey, SpriteImage, SpriteInstance, execute_plan,
        gpu::{GpuDevice, GpuExecutor},
    },
    outline::{OutlineInk, opaque_paint, ring},
};
use std::collections::{BTreeSet, HashMap};
use vault_formats::{
    MapFile, Palette,
    frm::FrmFrame,
    map::{MapObject, MapObjects},
    pro::{BasicFlags, Category},
};

/// SPEC-022 §3.4 one entry of the reference path's draw log, and of the
/// instanced path's sorted instance list: the layer, the hex or square
/// rank (10000 for the shroud), the list index (outline position, creation
/// rank or reference draw rank in layers 4 to 6) and the part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrawRecord {
    pub layer: FrameLayer,
    pub primary: u32,
    pub index: u32,
    pub part: u8,
}

impl DrawRecord {
    pub fn new(layer: FrameLayer, primary: u32, index: u32, part: u8) -> Self {
        Self {
            layer,
            primary,
            index,
            part,
        }
    }
    /// The list index of a backdrop group's record: no list index reaches
    /// it (they are below 2^29).
    pub const BACKDROP_INDEX: u32 = u32::MAX;
    /// A pre-rendered backdrop's floor pass: layer 0 after every square.
    pub fn backdrop_floor() -> Self {
        Self::new(FrameLayer::Floor, BACKDROP_FLOOR_PRIMARY, 0, 0)
    }
    /// A pre-rendered backdrop's group of hex `hex`, in the upright layer
    /// after the objects of that hex.
    pub fn backdrop_group(hex: u32) -> Self {
        Self::new(FrameLayer::Upright, hex, Self::BACKDROP_INDEX, 0)
    }
    /// Whether this is a backdrop group's record.
    pub fn is_backdrop_group(&self) -> bool {
        self.layer == FrameLayer::Upright && self.index == Self::BACKDROP_INDEX
    }
    /// The record a sort key stands for.
    pub fn of_key(key: SortKey) -> Result<Self, InstanceError> {
        let layer = FrameLayer::from_index(key.layer())?;
        let (primary, secondary) = (key.primary(), key.secondary());
        if key.is_backdrop_group() {
            return Ok(Self::backdrop_group(primary));
        }
        Ok(match layer {
            FrameLayer::Floor | FrameLayer::Roof => Self::new(layer, primary, 0, secondary as u8),
            FrameLayer::Flat | FrameLayer::Upright | FrameLayer::Outline => {
                Self::new(layer, primary, secondary >> 3, (secondary & 7) as u8)
            }
            FrameLayer::FloatingText | FrameLayer::NoHex | FrameLayer::Cursor => {
                Self::new(layer, primary, secondary, 0)
            }
        })
    }
}

/// Why the instanced path did not draw a refresh.
#[derive(Debug, thiserror::Error)]
pub enum GatherError {
    /// The instanced path refuses the refresh (§3.12).
    #[error(transparent)]
    Refused(#[from] InstanceError),
    /// The refresh itself fails, on either path.
    #[error(transparent)]
    Reference(#[from] MapRenderError),
}

/// The instanced path's input for one refresh of a view: its instances,
/// the images they read, the cutout mask, the uniform block and the hex
/// light buffer. The canvas starts as the reference's base fill, index 0.
pub struct MapFrame<'a> {
    pub instances: Vec<SpriteInstance>,
    pub images: Vec<SpriteImage<'a>>,
    pub mask: Option<SpriteImage<'a>>,
    pub uniforms: LayerUniforms,
    pub light: HexLightBuffer,
}

impl MapFrame<'_> {
    /// The executor inputs with `tables`.
    pub fn inputs<'s>(&'s self, tables: &'s LookupTables) -> FrameInputs<'s> {
        FrameInputs {
            tables,
            light: &self.light,
            uniforms: self.uniforms,
            mask: self.mask.as_ref(),
            images: &self.images,
        }
    }
    /// The base-filled canvas the refresh draws over.
    pub fn base_canvas(&self) -> Result<IndexedImage, InstanceError> {
        let [width, height] = self.uniforms.canvas;
        IndexedImage::new(width, height).map_err(|_| InstanceError::TextureLimit(width, height))
    }
    /// The instances' records in ascending key order.
    pub fn records(&self) -> Result<Vec<DrawRecord>, InstanceError> {
        let mut keys: Vec<SortKey> = self.instances.iter().map(|instance| instance.key).collect();
        keys.sort_unstable();
        keys.into_iter().map(DrawRecord::of_key).collect()
    }
    /// The canvas the CPU executor draws (§4 T5, T10).
    pub fn draw_cpu(&self, tables: &LookupTables) -> Result<IndexedImage, InstanceError> {
        let plan = BatchPlan::new(&self.instances)?;
        let mut canvas = self.base_canvas()?;
        execute_plan(
            &mut CanvasView::of(&mut canvas),
            &self.instances,
            &plan,
            &self.inputs(tables),
        )?;
        Ok(canvas)
    }
}

/// The instances and images gathered so far.
struct Builder<'a> {
    instances: Vec<SpriteInstance>,
    images: Vec<SpriteImage<'a>>,
    /// Borrowed images already added, by their pixels' address and length.
    borrowed: HashMap<(usize, usize), usize>,
    /// The refresh rectangle R, the whole canvas.
    clip: [i32; 4],
}

impl<'a> Builder<'a> {
    /// The index of a borrowed image, added once.
    fn borrowed(&mut self, width: u32, height: u32, pixels: &'a [u8]) -> usize {
        let key = (pixels.as_ptr() as usize, pixels.len());
        if let Some(index) = self.borrowed.get(&key) {
            return *index;
        }
        self.images.push(SpriteImage::new(width, height, pixels));
        self.borrowed.insert(key, self.images.len() - 1);
        self.images.len() - 1
    }
    fn frame(&mut self, frame: &'a FrmFrame) -> usize {
        self.borrowed(
            u32::from(frame.width),
            u32::from(frame.height),
            frame.pixels(),
        )
    }
    fn owned(&mut self, image: SpriteImage<'a>) -> usize {
        self.images.push(image);
        self.images.len() - 1
    }
    /// An instance over `rect` (canvas coordinates, part of the image at
    /// `origin`).
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        key: SortKey,
        image: usize,
        origin: [i64; 2],
        rect: [i64; 4],
        mode: DrawMode,
        flags: u8,
        light: Light,
        clip: [i32; 4],
    ) {
        // The image pixel (and level pixel) drawn at the rectangle's corner.
        let src = [(rect[0] - origin[0]) as u32, (rect[1] - origin[1]) as u32];
        let (light, hex, fixed, level) = match light {
            Light::None => (LightSource::None, 0, 0, None),
            Light::Remap(table) => (LightSource::None, 0, 0, Some((table, [0, 0]))),
            Light::Hex(hex) => (LightSource::Hex, hex, 0, None),
            Light::Ambient => (LightSource::Ambient, 0, 0, None),
            Light::Fixed(value) => (LightSource::Fixed, 0, value, None),
            Light::Level(levels) => (LightSource::LevelImage, 0, 0, Some((levels, src))),
        };
        // A rectangle far off the canvas keeps no pixel when its corner is
        // saturated into the i32 range.
        let saturate = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        self.instances.push(SpriteInstance {
            key,
            mode,
            flags,
            light,
            hex,
            fixed,
            dst: [saturate(rect[0]), saturate(rect[1])],
            size: [(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32],
            image,
            src,
            level,
            clip,
            aux: 0,
        });
    }
    fn set_aux(&mut self, aux: u16) {
        if let Some(last) = self.instances.last_mut() {
            last.aux = aux;
        }
    }
    /// The last instance's level image (a [`SELECT`] selector), read from
    /// its pixel (0, 0) at the instance's corner.
    fn set_selector(&mut self, image: usize) {
        if let Some(last) = self.instances.last_mut() {
            last.level = Some((image, [0, 0]));
        }
    }
}

/// An instance's light, before it is encoded.
#[derive(Clone, Copy)]
enum Light {
    None,
    Hex(u32),
    Ambient,
    Fixed(u32),
    /// The level image's index.
    Level(usize),
    /// No light, and the remap table's index.
    Remap(usize),
}

/// The draw mode, light and aux of an object drawn without the cutout
/// (SPEC-022 §3.3): translucent with one translucency bit (wall
/// translucency at full light), else lit; plain without a light.
fn object_mode(flags: u32, lit: bool, hex_light: Light) -> (DrawMode, u8, Light, u16) {
    if !lit {
        return (DrawMode::Plain, 0, Light::None, 0);
    }
    match translucent_row(flags) {
        Some(row) => {
            let light = if TRANSLUCENT[row].3 {
                Light::Fixed(65536)
            } else {
                hex_light
            };
            (DrawMode::Translucent, 0, light, row as u16)
        }
        None => (DrawMode::Lit, EXEMPT, hex_light, 0),
    }
}

/// The light source `Light::Hex` may name for an object without a hex is
/// never used: such objects are drawn in layer 6 at a fixed light.
fn hex_light(hex: i32) -> Light {
    Light::Hex(u32::try_from(hex).unwrap_or(u32::MAX))
}

/// SPEC-022 §3.2 steps 1-2 and §3.3: the instanced path's frame of one
/// refresh of `view`, gathered from the same map, objects, art and scene
/// the reference path draws. `tables` are the lookup tables of the
/// scene's light palette (`None` refuses every draw that needs them).
///
/// Refused (a typed [`InstanceError`]): a peek a safe bound of §3.10
/// changed, an opaque outline of index 0, a translucent outline whose blend
/// table is not one of the five, and tables that are not the light
/// palette's (or the cutout's, in a scene without a light).
pub fn gather_map_frame<'a, A: MapArt>(
    map: &'a MapFile,
    objects: &'a MapObjects,
    view: MapView,
    art: &'a A,
    scene: MapScene<'a>,
    tables: Option<&LookupTables>,
) -> Result<MapFrame<'a>, GatherError> {
    check_floor_view(view)?;
    IndexedImage::new(view.width, view.height).map_err(MapRenderError::from)?;
    if let Some(light) = scene.light {
        let tables = tables.ok_or(InstanceError::Upload("lookup tables".into()))?;
        if *tables.intensity != *light.palette.intensity_table() {
            return Err(InstanceError::Upload("stale lookup tables".into()).into());
        }
    }
    let peek = scene
        .roof_cutaway
        .filter(|_| view.roofs)
        .map(RoofPeekParams::of_cutaway);
    if peek.is_some_and(|peek| peek.bounded) {
        return Err(InstanceError::PeekBound.into());
    }
    let canvas = [0, 0, view.width as i32, view.height as i32];
    let refresh = [0, 0, i64::from(view.width), i64::from(view.height)];
    let mut builder = Builder {
        instances: Vec::new(),
        images: Vec::new(),
        borrowed: HashMap::new(),
        clip: canvas,
    };
    let clip = builder.clip;
    let lit = scene.light.is_some();

    // Layer 0: the floor.
    let mut floor_light = scene.light.map(FloorLight::new);
    for (rank, frame, corner) in floor_squares(map, view, art) {
        let key = SortKey::floor(rank)?;
        let image = builder.frame(frame);
        let rect = frame_rect(frame, corner);
        match &mut floor_light {
            None => builder.push(
                key,
                image,
                corner,
                rect,
                DrawMode::Plain,
                0,
                Light::None,
                clip,
            ),
            Some(floor_light) => match floor_light.tile(view, corner) {
                None => {}
                Some(TileLight::Uniform(light)) => {
                    let light = Light::Fixed(light.clamp(0, 65536) as u32);
                    builder.push(key, image, corner, rect, DrawMode::Lit, EXEMPT, light, clip);
                }
                Some(TileLight::Interpolated) => {
                    let (width, height) = (u32::from(frame.width), u32::from(frame.height));
                    let levels: Vec<u8> = (0..i64::from(height))
                        .flat_map(|py| (0..i64::from(width)).map(move |px| [px, py]))
                        .map(|pixel| floor_light.level_at(pixel))
                        .collect();
                    let levels = builder.owned(SpriteImage::new(width, height, levels));
                    let light = Light::Level(levels);
                    builder.push(
                        key,
                        image,
                        corner,
                        rect,
                        DrawMode::FloorMap,
                        EXEMPT,
                        light,
                        clip,
                    );
                }
            },
        }
    }

    // Layer 0, after every square: the backdrop's floor pass, each pixel
    // lit at the level of the hex under it.
    let backdrop = scene
        .backdrop
        .and_then(|backdrop| Layout::of(backdrop, view));
    if let Some(floor) = backdrop.as_ref().and_then(|layout| layout.floor.as_ref()) {
        let [width, height] = floor.size();
        let corner = [i64::from(floor.rect[0]), i64::from(floor.rect[1])];
        let rect = floor.rect.map(i64::from);
        let image = builder.owned(SpriteImage::new(
            width,
            height,
            floor.image(|pixel| pixel.colour),
        ));
        let key = SortKey::backdrop_floor();
        if lit {
            let levels =
                floor.image(|pixel| floor_level(scene.light, view.grid, [pixel.x, pixel.y]));
            let levels = builder.owned(SpriteImage::new(width, height, levels));
            builder.push(
                key,
                image,
                corner,
                rect,
                DrawMode::FloorMap,
                EXEMPT,
                Light::Level(levels),
                clip,
            );
        } else {
            builder.push(
                key,
                image,
                corner,
                rect,
                DrawMode::Plain,
                0,
                Light::None,
                clip,
            );
        }
    }

    // Layers 1 and 2: the flat and upright passes.
    let elevation = &objects.elevations[view.elevation as usize];
    let cutout = scene.cutout.and_then(|cut| Some((cut, cut.origin(view)?)));
    let mask_rect = cutout.map(|(cut, mask)| cut.rect(mask));
    let sprite = |object: &MapObject| -> Result<Option<(&'a FrmFrame, [i64; 2])>, MapRenderError> {
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
        overscan_hexes_with(view, elevation, art, suppress.as_ref())
    } else {
        Vec::new()
    };
    let cursor = scene
        .cursor
        .filter(|cursor| cursor_visited(view, cursor.hex, &extra));
    let (placed, hexless) = draw_sequence_with(view, elevation, &extra, suppress.as_ref());
    let lists = ListIndexes::new(elevation);
    let mut outlines: Vec<(&FrmFrame, [i64; 2], OutlineInk)> = Vec::new();
    let mut highlights = Vec::new();
    // A drawn image layer (the cursor objects): lit at its hex.
    let draw_layer = |builder: &mut Builder<'a>,
                      key: SortKey,
                      image: &'a IndexedImage,
                      origin: [i64; 2],
                      hex: i32| {
        let index = builder.borrowed(image.width(), image.height(), image.pixels());
        let rect = [
            origin[0],
            origin[1],
            origin[0] + i64::from(image.width()),
            origin[1] + i64::from(image.height()),
        ];
        let (mode, flags, light) = if lit {
            (DrawMode::Lit, EXEMPT, hex_light(hex))
        } else {
            (DrawMode::Plain, 0, Light::None)
        };
        builder.push(key, index, origin, rect, mode, flags, light, clip);
    };
    for index in placed {
        let object = &elevation[index];
        let Some((frame, origin)) = sprite(object)? else {
            continue;
        };
        let layer = if object.flags.contains(BasicFlags::FLAT) {
            FrameLayer::Flat
        } else {
            FrameLayer::Upright
        };
        let hex = object.hex.unwrap_or_default();
        let image = builder.frame(frame);
        let rect = frame_rect(frame, origin);
        let light = if lit { hex_light(hex) } else { Light::None };
        let cut = cutout.filter(|(cut, _)| cut.cuts(view, object)).is_some();
        let mask = if cut { mask_rect } else { None };
        for (part, part_rect, through) in cutout_parts(rect, refresh, mask) {
            let key = SortKey::object(layer, hex_u32(hex)?, lists.objects[index], part)?;
            let (mode, flags, light, aux) = if through {
                (DrawMode::Egg, 0, light, 0)
            } else if cut && lit {
                (DrawMode::Lit, EXEMPT, light, 0)
            } else {
                object_mode(object.flags.0, lit, light)
            };
            builder.push(key, image, origin, part_rect, mode, flags, light, clip);
            builder.set_aux(aux);
        }
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

    // Layer 2: the backdrop groups, each keyed after its hex's objects. The
    // cursor fade's removed pixels are left out of the image; the player
    // fade's pixels are selected for the masked blend.
    if let Some(layout) = &backdrop {
        let fade = Fade::new(view, &scene);
        for (hex, group) in &layout.groups {
            let shade = |pixel: &super::backdrop::Pixel| {
                fade.shade(*hex, pixel.fade, [pixel.x, pixel.y].map(i64::from))
            };
            let [width, height] = group.size();
            let corner = [i64::from(group.rect[0]), i64::from(group.rect[1])];
            let rect = group.rect.map(i64::from);
            let colour = group.image(|pixel| match shade(pixel) {
                Shade::Removed => 0,
                _ => pixel.colour,
            });
            let masked = group
                .pixels
                .iter()
                .any(|pixel| matches!(shade(pixel), Shade::Masked(_)));
            let image = builder.owned(SpriteImage::new(width, height, colour));
            let key = SortKey::backdrop_group(u32::from(*hex))?;
            let light = if lit {
                hex_light(i32::from(*hex))
            } else {
                Light::None
            };
            if masked {
                let selector =
                    group.image(|pixel| u8::from(matches!(shade(pixel), Shade::Masked(_))));
                let selector = builder.owned(SpriteImage::new(width, height, selector));
                builder.push(
                    key,
                    image,
                    corner,
                    rect,
                    DrawMode::Egg,
                    EXEMPT | SELECT,
                    light,
                    clip,
                );
                builder.set_selector(selector);
            } else if lit {
                builder.push(key, image, corner, rect, DrawMode::Lit, EXEMPT, light, clip);
            } else {
                builder.push(key, image, corner, rect, DrawMode::Plain, 0, light, clip);
            }
        }
    }

    // Layer 3: the roofs, at the ambient light, through the cutout and the
    // peek.
    if view.roofs {
        let peeked = if peek.is_some() { PEEK } else { 0 };
        for (rank, frame, corner) in roof_squares(map, view, art) {
            let image = builder.frame(frame);
            let rect = frame_rect(frame, corner);
            for (part, part_rect, through) in cutout_parts(rect, refresh, mask_rect) {
                let key = SortKey::roof(rank, part)?;
                let (mode, flags, light) = match (lit, through) {
                    (false, true) => (DrawMode::Egg, peeked, Light::None),
                    (false, false) => (DrawMode::Plain, peeked, Light::None),
                    (true, true) => (DrawMode::Egg, peeked, Light::Ambient),
                    (true, false) => (DrawMode::Lit, EXEMPT | peeked, Light::Ambient),
                };
                builder.push(key, image, corner, part_rect, mode, flags, light, clip);
            }
        }
    }

    // Layer 3, after every roof square: the enhanced shroud, recorded from
    // its painter.
    if let Some(shroud) = scene.shroud {
        let field = enhanced::cached_field(shroud.allowed);
        let recording =
            enhanced::record_shroud([view.width, view.height], view.grid, &field, shroud.dark)
                .map_err(MapRenderError::from)?;
        let image = builder.owned(SpriteImage::new(
            recording.width,
            recording.height,
            recording.image,
        ));
        let table = builder.owned(SpriteImage::new(256, 1, recording.table));
        builder.push(
            SortKey::shroud(0)?,
            image,
            [0, 0],
            refresh,
            DrawMode::Remap,
            0,
            Light::Remap(table),
            clip,
        );
    }

    // An outline, as a ring image the outline painter records.
    let push_ring = |builder: &mut Builder<'a>,
                     key: SortKey,
                     frame: &FrmFrame,
                     origin: [i64; 2],
                     ink: OutlineInk|
     -> Result<(), InstanceError> {
        let (ring_image, mode, aux) =
            ring_image(frame, origin, ink, [view.width, view.height], tables)?;
        let ring_origin = [origin[0] - 1, origin[1] - 1];
        let rect = [
            ring_origin[0],
            ring_origin[1],
            ring_origin[0] + i64::from(ring_image.width),
            ring_origin[1] + i64::from(ring_image.height),
        ];
        let image = builder.owned(ring_image);
        builder.push(key, image, ring_origin, rect, mode, 0, Light::None, canvas);
        builder.set_aux(aux);
        Ok(())
    };

    // Layer 4: the outlines.
    for (position, (frame, origin, ink)) in outlines.into_iter().chain(highlights).enumerate() {
        let key = SortKey::outline(u32::try_from(position).unwrap_or(u32::MAX), 0)?;
        push_ring(&mut builder, key, frame, origin, ink)?;
    }

    // Layer 5: the floating texts, in creation order.
    for (rank, text) in scene.floating.iter().enumerate() {
        let Ok([x, y]) = view.grid.hex_reference(text.hex) else {
            continue;
        };
        let key = SortKey::floating_text(rank as u32)?;
        let origin = [x + i64::from(text.offset[0]), y + i64::from(text.offset[1])];
        let image = builder.borrowed(text.image.width(), text.image.height(), text.image.pixels());
        let rect = [
            origin[0],
            origin[1],
            origin[0] + i64::from(text.image.width()),
            origin[1] + i64::from(text.image.height()),
        ];
        builder.push(
            key,
            image,
            origin,
            rect,
            DrawMode::Plain,
            0,
            Light::None,
            clip,
        );
    }

    // Layer 6: the objects without a hex, at full light.
    for (rank, index) in hexless.into_iter().enumerate() {
        let object = &elevation[index];
        let Some((frame, origin)) = sprite(object)? else {
            continue;
        };
        let key = SortKey::no_hex(rank as u32)?;
        let image = builder.frame(frame);
        let (mode, flags, light, aux) = object_mode(object.flags.0, lit, Light::Fixed(65536));
        builder.push(
            key,
            image,
            origin,
            frame_rect(frame, origin),
            mode,
            flags,
            light,
            clip,
        );
        builder.set_aux(aux);
    }

    // Layer 7: the cursor, after everything else (revision 4).
    if let Some(cursor) = cursor {
        let (art, origin) = cursor.art;
        let key = SortKey::cursor(SortKey::CURSOR_ART)?;
        draw_layer(&mut builder, key, art, origin, cursor.hex);
        if let Some((frame, origin, word)) = cursor.outline
            && let Some(ink) = scene.outlines.and_then(|inks| inks.ink(word))
        {
            let key = SortKey::cursor(SortKey::CURSOR_OUTLINE)?;
            push_ring(&mut builder, key, frame, origin, ink)?;
        }
        if let Some((text, origin)) = cursor.text {
            let key = SortKey::cursor(SortKey::CURSOR_TEXT)?;
            draw_layer(&mut builder, key, text, origin, cursor.hex);
        }
    }

    // The tables the cutout blends with must be the light palette's.
    let mask = match cutout {
        Some((cut, origin)) => {
            check_cut_palette(cut, scene.light, tables)?;
            let place = CutoutPlace {
                left: i32::try_from(origin[0]).map_err(|_| domain("cutout left", origin[0]))?,
                top: i32::try_from(origin[1]).map_err(|_| domain("cutout top", origin[1]))?,
                width: u32::from(cut.mask.width),
                height: u32::from(cut.mask.height),
            };
            Some((
                place,
                SpriteImage::new(place.width, place.height, cut.mask.pixels()),
            ))
        }
        None => None,
    };
    let uniforms = LayerUniforms {
        canvas: [view.width, view.height],
        offset: view.grid.origin.map(|value| value as i32),
        ambient: scene.light.map_or(0, |light| i64::from(light.ambient)),
        elevation: u32::from(view.elevation),
        cutout: mask.as_ref().map(|(place, _)| *place),
        peek,
    };
    let light = scene.light.map_or_else(HexLightBuffer::default, |light| {
        HexLightBuffer::pack(light.tiles)
    });
    Ok(MapFrame {
        instances: builder.instances,
        images: builder.images,
        mask: mask.map(|(_, image)| image),
        uniforms,
        light,
    })
}

fn domain(field: &'static str, value: i64) -> InstanceError {
    InstanceError::Domain {
        field,
        value: value as u64,
    }
}

fn hex_u32(hex: i32) -> Result<u32, InstanceError> {
    u32::try_from(hex).map_err(|_| domain("hex", i64::from(hex)))
}

/// The cutout blends with its own palette's intensity and mix tables; the
/// instanced path draws with the given tables, so the two must agree.
fn check_cut_palette(
    cut: &Cutout,
    light: Option<MapLight>,
    tables: Option<&LookupTables>,
) -> Result<(), InstanceError> {
    let tables = tables.ok_or(InstanceError::Upload("lookup tables".into()))?;
    if light.is_some_and(|light| std::ptr::eq(cut.palette, light.palette)) {
        return Ok(());
    }
    let same = *tables.intensity == *cut.palette.intensity_table()
        && (0..=u16::MAX).all(|index| {
            cut.palette.additive((index >> 8) as u8, index as u8) == tables.mix[usize::from(index)]
        });
    if same {
        Ok(())
    } else {
        Err(InstanceError::Upload("cutout palette".into()))
    }
}

/// SPEC-022 §3.3 an outline's ring image, covering its frame widened by
/// one pixel on every side: the outline painter ([`ring`]) run with the
/// reference's clipping and buffer guards over the whole `canvas`,
/// recording each paint. An opaque or banded ink records the index that
/// stays; a translucent one the number of paints, drawn in mode 5 with its
/// blend table and gray level in aux. The painter skips a right paint past
/// the last column instead of wrapping it, so there is no wrap column.
fn ring_image<'a>(
    frame: &FrmFrame,
    origin: [i64; 2],
    ink: OutlineInk,
    canvas: [u32; 2],
    tables: Option<&LookupTables>,
) -> Result<(SpriteImage<'a>, DrawMode, u16), InstanceError> {
    let (width, height) = (u32::from(frame.width) + 2, u32::from(frame.height) + 2);
    let mut pixels = vec![0_u8; width as usize * height as usize];
    let buffer = [i64::from(canvas[0]), i64::from(canvas[1])];
    let mut zero_paint = false;
    ring(
        frame,
        origin,
        [0, 0, buffer[0], buffer[1]],
        buffer,
        |[x, y], row| {
            let at = (y - origin[1] + 1) as usize * width as usize + (x - origin[0] + 1) as usize;
            match opaque_paint(ink, frame, row) {
                Some(index) => {
                    zero_paint |= index == 0;
                    pixels[at] = index;
                }
                None => pixels[at] = pixels[at].saturating_add(1),
            }
        },
    );
    let image = SpriteImage::new(width, height, pixels);
    match ink {
        OutlineInk::Translucent { gray, blend } => {
            let tables = tables.ok_or(InstanceError::Upload("lookup tables".into()))?;
            // §3.6: a blend table has 8 rows, as the reference path clamps.
            let gray = gray.min(7);
            // The red table (4) or the wall table (0), as §2.2 lists them.
            let table = [4_u16, 0, 1, 2, 3]
                .into_iter()
                .find(|table| *tables.blends[usize::from(*table)] == *blend)
                .ok_or(InstanceError::Unexpressible("an outline blend table"))?;
            Ok((image, DrawMode::OutlineBlend, table | u16::from(gray) << 8))
        }
        _ if zero_paint => Err(InstanceError::Unexpressible("an outline of index 0")),
        _ => Ok((image, DrawMode::Plain, 0)),
    }
}

/// SPEC-022 §3.12 how one refresh was drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshReport {
    /// By the reference path, as asked.
    Reference,
    /// By the instanced path, with its counts.
    Instanced(FrameCounts),
    /// The instanced path refused; the reference path drew it.
    Refused(InstanceError),
}

/// The instanced path's executor.
enum Executor {
    /// No device asked for yet.
    Untried,
    Gpu(Box<GpuExecutor>),
    /// The CPU executor (headless tests and tools).
    Cpu,
    /// No device, or a lost one: the reference path to the session's end
    /// (§3.12 step 1), with the cause.
    Unavailable(InstanceError),
}

/// Every part of a palette [`LookupTables::from_palette`] reads: the six-bit
/// entries, which are mapped, the RGB555 lookup and the supplied tables.
/// Nothing else feeds the tables: fades and brightness change the display
/// colors, not these.
struct PaletteInputs {
    entries: [[u8; 3]; 256],
    mapped: [bool; 256],
    lookup: Vec<u8>,
    supplied: Option<Vec<u8>>,
}

/// The five-bit color at `packed` in the RGB555 lookup.
fn five_bit(packed: usize) -> [u8; 3] {
    [
        (packed >> 10) as u8,
        (packed >> 5) as u8 & 31,
        packed as u8 & 31,
    ]
}

impl PaletteInputs {
    fn of(palette: &Palette) -> Self {
        Self {
            entries: std::array::from_fn(|index| palette.rgb6(index as u8)),
            mapped: std::array::from_fn(|index| palette.is_mapped(index as u8)),
            lookup: (0..32768)
                .map(|packed| palette.lookup_rgb5(five_bit(packed)).unwrap_or(0))
                .collect(),
            supplied: palette.supplied_tables().map(<[u8]>::to_vec),
        }
    }
    /// Whether `palette` has these contents, wherever it lives.
    fn matches(&self, palette: &Palette) -> bool {
        palette.supplied_tables() == self.supplied.as_deref()
            && (0..=255).all(|index| {
                palette.rgb6(index) == self.entries[usize::from(index)]
                    && palette.is_mapped(index) == self.mapped[usize::from(index)]
            })
            && (0..32768)
                .all(|packed| palette.lookup_rgb5(five_bit(packed)) == Some(self.lookup[packed]))
    }
}

/// The lookup tables of the last palette drawn with. A refresh is one tile
/// of the canvas, so a frame asks for the same tables many times. The key is
/// the palette's contents rather than its address or a version, so a palette
/// replaced in place can never serve stale tables.
///
/// Comparing the contents walks the whole palette, so a frame does it once:
/// a caller that has opened a frame ([`SceneRenderer::begin_frame`]) borrows
/// one palette for all of that frame's refreshes, and nothing can replace it
/// while it is borrowed, so a later refresh with the same address needs no
/// new comparison. The next frame compares again. A refresh outside any frame
/// always compares.
#[derive(Default)]
struct TableCache {
    held: Option<(PaletteInputs, LookupTables)>,
    /// How many times the tables were built.
    builds: usize,
    /// How many times a palette was compared with the held contents.
    comparisons: usize,
    /// A caller has opened a frame.
    framed: bool,
    /// The address of the palette the open frame found equal to `held`.
    proved: Option<usize>,
}

impl TableCache {
    fn begin_frame(&mut self) {
        self.framed = true;
        self.proved = None;
    }
    fn tables(&mut self, palette: &Palette) -> &LookupTables {
        let address = std::ptr::from_ref(palette).addr();
        let proved = self.proved == Some(address);
        let held = self.held.take().filter(|(inputs, _)| {
            proved || {
                self.comparisons += 1;
                inputs.matches(palette)
            }
        });
        let held = held.unwrap_or_else(|| {
            self.builds += 1;
            (
                PaletteInputs::of(palette),
                LookupTables::from_palette(palette),
            )
        });
        self.proved = self.framed.then_some(address);
        &self.held.insert(held).1
    }
}

/// SPEC-022 §3.1 and §3.12 the world canvas's renderer: the reference
/// path (the default), or the instanced path with a fallback to the
/// reference for every refresh it refuses. The setting is not saved and
/// writes no input-log record (§3.13).
pub struct SceneRenderer {
    path: RenderPath,
    executor: Executor,
    tables: TableCache,
    /// The causes logged so far (each once per session).
    logged: BTreeSet<&'static str>,
    /// How many refreshes each cause refused.
    refusals: HashMap<&'static str, usize>,
    last: Option<RefreshReport>,
    /// A canvas side limit below the device's (tests).
    canvas_limit: Option<u32>,
    /// A hook run over the gathered instances (tests).
    tamper: Option<fn(&mut Vec<SpriteInstance>)>,
}

impl SceneRenderer {
    /// A renderer on `path`; the instanced path's GPU device is created on
    /// its first refresh.
    pub fn new(path: RenderPath) -> Self {
        Self {
            path,
            executor: Executor::Untried,
            tables: TableCache::default(),
            logged: BTreeSet::new(),
            refusals: HashMap::new(),
            last: None,
            canvas_limit: None,
            tamper: None,
        }
    }
    /// The instanced path drawn by the CPU executor, without a device.
    pub fn cpu() -> Self {
        Self {
            executor: Executor::Cpu,
            ..Self::new(RenderPath::Instanced)
        }
    }
    /// The instanced path on `device`.
    pub fn with_device(device: GpuDevice) -> Self {
        Self {
            executor: Executor::Gpu(Box::new(GpuExecutor::new(device))),
            ..Self::new(RenderPath::Instanced)
        }
    }
    /// The path asked for.
    pub fn path(&self) -> RenderPath {
        self.path
    }
    /// How the last refresh was drawn.
    pub fn last_refresh(&self) -> Option<&RefreshReport> {
        self.last.as_ref()
    }
    /// The causes logged so far, each once.
    pub fn logged_causes(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.logged.iter().copied()
    }
    /// How many refreshes `cause` refused.
    pub fn refusals(&self, cause: &str) -> usize {
        self.refusals.get(cause).copied().unwrap_or(0)
    }
    /// How many times the lookup tables were built.
    pub fn table_builds(&self) -> usize {
        self.tables.builds
    }
    /// How many times a palette's contents were compared with the held
    /// tables' (once per frame after [`Self::begin_frame`]).
    pub fn table_comparisons(&self) -> usize {
        self.tables.comparisons
    }
    /// A new frame starts: the next refresh compares its palette's contents
    /// with the held tables', and the frame's later refreshes with the same
    /// palette reference do not. Every refresh up to the next call must
    /// borrow that palette for as long as they run, as one frame's loop does.
    pub fn begin_frame(&mut self) {
        self.tables.begin_frame();
    }
    /// The GPU executor, if one is in use.
    pub fn gpu(&self) -> Option<&GpuExecutor> {
        match &self.executor {
            Executor::Gpu(executor) => Some(executor),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn limit_canvas(&mut self, side: u32) {
        self.canvas_limit = Some(side);
    }
    #[cfg(test)]
    pub(crate) fn tamper(&mut self, hook: fn(&mut Vec<SpriteInstance>)) {
        self.tamper = Some(hook);
    }

    /// One whole refresh of `view`, on the chosen path: the same canvas as
    /// [`render_map_scene`]. A refresh the instanced path refuses is drawn
    /// by the reference path, and its cause is logged once.
    pub fn render<A: MapArt>(
        &mut self,
        map: &MapFile,
        objects: &MapObjects,
        view: MapView,
        art: &A,
        scene: MapScene,
    ) -> Result<IndexedImage, MapRenderError> {
        if self.path == RenderPath::Reference {
            self.last = Some(RefreshReport::Reference);
            return render_map_scene(map, objects, view, art, scene);
        }
        match self.instanced(map, objects, view, art, scene) {
            Ok((image, counts)) => {
                self.last = Some(RefreshReport::Instanced(counts));
                Ok(image)
            }
            Err(GatherError::Reference(_)) => {
                // The refresh fails on either path: the reference's error.
                self.last = Some(RefreshReport::Reference);
                render_map_scene(map, objects, view, art, scene)
            }
            Err(GatherError::Refused(error)) => {
                let cause = error.cause();
                *self.refusals.entry(cause).or_default() += 1;
                if self.logged.insert(cause) {
                    tracing::warn!(%error, cause, "instanced path refused a refresh; drawn by the reference path");
                }
                self.last = Some(RefreshReport::Refused(error));
                render_map_scene(map, objects, view, art, scene)
            }
        }
    }

    fn instanced<A: MapArt>(
        &mut self,
        map: &MapFile,
        objects: &MapObjects,
        view: MapView,
        art: &A,
        scene: MapScene,
    ) -> Result<(IndexedImage, FrameCounts), GatherError> {
        if matches!(self.executor, Executor::Untried) {
            self.executor = match GpuDevice::new() {
                Ok(device) => Executor::Gpu(Box::new(GpuExecutor::new(device))),
                Err(error) => Executor::Unavailable(error),
            };
        }
        if let Executor::Unavailable(error) = &self.executor {
            return Err(error.clone().into());
        }
        if let Some(limit) = self.canvas_limit
            && (view.width > limit || view.height > limit)
        {
            return Err(InstanceError::TextureLimit(view.width, view.height).into());
        }
        // The tables of the light palette, built when its contents change
        // (the GPU executor re-uploads them only then too).
        let palette = scene
            .light
            .map(|light| light.palette)
            .or(scene.cutout.map(|cut| cut.palette));
        let tables = palette.map(|palette| self.tables.tables(palette));
        let mut frame = gather_map_frame(map, objects, view, art, scene, tables)?;
        if let Some(tamper) = self.tamper {
            tamper(&mut frame.instances);
        }
        let plan = BatchPlan::new(&frame.instances)?;
        let empty;
        let tables = match tables {
            Some(tables) => tables,
            None => {
                empty = LookupTables {
                    intensity: vec![0; 65536].into(),
                    mix: vec![0; 65536].into(),
                    gray: [[0; 256]; 2],
                    blends: std::array::from_fn(|_| Box::new([[0; 256]; 8])),
                };
                &empty
            }
        };
        let inputs = frame.inputs(tables);
        let mut canvas = frame.base_canvas()?;
        match &mut self.executor {
            Executor::Gpu(executor) => {
                match executor.execute(&mut canvas, &frame.instances, &plan, &inputs) {
                    Ok(_) => {}
                    Err(error @ InstanceError::DeviceLost) => {
                        // §3.12 step 1: the reference path to the session's end.
                        self.executor = Executor::Unavailable(error.clone());
                        return Err(error.into());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Executor::Cpu => execute_plan(
                &mut CanvasView::of(&mut canvas),
                &frame.instances,
                &plan,
                &inputs,
            )?,
            Executor::Untried => {
                return Err(InstanceError::NoDevice("no adapter or device".into()).into());
            }
            Executor::Unavailable(error) => return Err(error.clone().into()),
        }
        Ok((canvas, plan.counts()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        RoofCutaway, SceneCursor, fixture,
        tests::{Keyed, preview, scene},
    };
    use super::*;
    use crate::layers::gpu;
    use std::collections::BTreeMap;
    use vault_formats::{FrmArchive, HexGrid, map::TilePair};

    /// A palette's file bytes: scrambled entries and RGB555 lookup.
    fn palette_bytes() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(0x8300);
        for index in 0..256_u32 {
            bytes.extend(
                [
                    (index * 7) % 32,
                    (index * 13 + 5) % 32,
                    (index * 29 + 11) % 32,
                ]
                .map(|channel| channel as u8),
            );
        }
        for packed in 0..32768_u32 {
            let (r, g, b) = (packed >> 10, (packed >> 5) & 31, packed & 31);
            bytes.push(((r * 3 + g * 5 + b * 7 + 1) % 255 + 1) as u8);
        }
        bytes
    }

    fn map() -> MapFile {
        let mut map = preview();
        let word = (1_u32 << 16) | 4;
        map.tiles[0] = Some(vec![TilePair { stored: word, word }; 10000]);
        map
    }

    fn art() -> Keyed {
        let frame = |color| FrmArchive::parse(&fixture::frm(80, 36, color)).unwrap();
        Keyed(BTreeMap::from([
            ((true, 1), frame(0)),
            ((true, 4), frame(60)),
        ]))
    }

    fn view_of(width: u32, height: u32) -> MapView {
        MapView {
            grid: HexGrid::centered(20100, width, height).unwrap(),
            elevation: 0,
            roofs: false,
            width,
            height,
        }
    }

    fn scene_of<'a>(light: Option<MapLight<'a>>, cursor: Option<SceneCursor<'a>>) -> MapScene<'a> {
        MapScene {
            cutout: None,
            light,
            outlines: None,
            floating: &[],
            cursor,
            roof_cutaway: None::<RoofCutaway>,
            shroud: None,
            overscan: false,
            highlight: None,
            backdrop: None,
        }
    }

    /// One palette change at a time, each palette assigned over the last
    /// (the same address), rebuilds the tables, and a repeat does not.
    #[test]
    fn a_palette_change_rebuilds_the_lookup_tables() {
        let (map, art) = (map(), art());
        let objects = scene(Vec::new());
        let view = view_of(640, 380);
        let tiles = vec![40000_i64; 40000];
        let mut renderer = SceneRenderer::cpu();
        let mut draw = |palette: &Palette| {
            let light = MapLight {
                palette,
                ambient: 30000,
                tiles: &tiles,
            };
            let scene = scene_of(Some(light), None);
            let image = renderer.render(&map, &objects, view, &art, scene).unwrap();
            let reference = render_map_scene(&map, &objects, view, &art, scene).unwrap();
            assert_eq!(image.pixels(), reference.pixels());
            assert!(matches!(
                renderer.last_refresh(),
                Some(RefreshReport::Instanced(_))
            ));
            let held = &renderer.tables.held.as_ref().unwrap().1;
            assert!(*held == LookupTables::from_palette(palette), "stale tables");
            renderer.table_builds()
        };

        let bytes = palette_bytes();
        let mut palette = Palette::parse(&bytes).unwrap();
        assert_eq!(draw(&palette), 1);
        assert_eq!(draw(&palette), 1, "the same palette");
        assert_eq!(draw(&palette.clone()), 1, "equal contents elsewhere");

        let mut entry = bytes.clone();
        entry[3 * 60 + 1] ^= 1;
        let mut unmapped = bytes.clone();
        unmapped[3 * 60] = 200;
        let mut lookup = bytes.clone();
        lookup[768 + 12345] ^= 1;
        let mut supplied = bytes.clone();
        supplied.extend(b"CWEN");
        supplied.extend((0..3 * 65536_u32).map(|at| (at % 251) as u8));
        let mut other_supplied = supplied.clone();
        *other_supplied.last_mut().unwrap() ^= 1;
        let mut builds = 1;
        for (label, changed) in [
            ("an entry", entry),
            ("a mapped entry", unmapped),
            ("the RGB555 lookup", lookup),
            ("supplied tables", supplied),
            ("the supplied tables' last byte", other_supplied),
            ("back to the first", bytes),
        ] {
            palette = Palette::parse(&changed).unwrap();
            builds += 1;
            assert_eq!(draw(&palette), builds, "{label}");
            assert_eq!(draw(&palette), builds, "{label}, repeated");
        }
    }

    /// A frame compares its palette's contents once, however many refreshes
    /// it holds; the next frame compares again, so a palette assigned over
    /// the last between frames (the same address) is noticed. A repeated
    /// frame builds nothing.
    #[test]
    fn a_frame_compares_the_palette_once() {
        let (map, art) = (map(), art());
        let objects = scene(Vec::new());
        let view = view_of(640, 380);
        let tiles = vec![40000_i64; 40000];
        let mut renderer = SceneRenderer::cpu();
        let frame = |renderer: &mut SceneRenderer, palette: &Palette| {
            renderer.begin_frame();
            let light = MapLight {
                palette,
                ambient: 30000,
                tiles: &tiles,
            };
            let scene = scene_of(Some(light), None);
            for _ in 0..4 {
                renderer.render(&map, &objects, view, &art, scene).unwrap();
            }
            (renderer.table_builds(), renderer.table_comparisons())
        };

        let bytes = palette_bytes();
        let mut palette = Palette::parse(&bytes).unwrap();
        assert_eq!(frame(&mut renderer, &palette), (1, 0), "nothing to compare");
        assert_eq!(frame(&mut renderer, &palette), (1, 1), "a repeat");
        assert_eq!(frame(&mut renderer, &palette), (1, 2), "a repeat");

        let mut changed = bytes.clone();
        changed[768 + 12345] ^= 1;
        palette = Palette::parse(&changed).unwrap();
        assert_eq!(frame(&mut renderer, &palette), (2, 3), "the same address");
        let held = &renderer.tables.held.as_ref().unwrap().1;
        assert!(
            *held == LookupTables::from_palette(&palette),
            "stale tables"
        );
    }

    /// A refresh with no palette draws without building any tables.
    #[test]
    fn an_unlit_refresh_builds_no_tables() {
        let (map, art) = (map(), art());
        let objects = scene(Vec::new());
        let mut renderer = SceneRenderer::cpu();
        renderer
            .render(
                &map,
                &objects,
                view_of(640, 380),
                &art,
                scene_of(None, None),
            )
            .unwrap();
        assert_eq!(renderer.table_builds(), 0);
    }

    /// A layer's pattern at view pixel `[x, y]`: nonzero, with holes.
    fn ink([x, y]: [i64; 2]) -> u8 {
        match (x * 5 + y * 3).rem_euclid(23) {
            0 => 0,
            value => 100 + value as u8,
        }
    }

    /// The view-sized layer with `rect` (`[left, top, right, bottom]`, kept
    /// on the view) inked, and the same layer cropped to `rect`, with its
    /// origin. A rectangle with no pixels crops to one transparent pixel.
    fn layers(view: MapView, rect: [i64; 4]) -> (IndexedImage, IndexedImage, [i64; 2]) {
        let [left, top, right, bottom] = rect;
        let mut full = IndexedImage::new(view.width, view.height).unwrap();
        if left >= right || top >= bottom {
            return (full, IndexedImage::new(1, 1).unwrap(), [0, 0]);
        }
        let mut crop = IndexedImage::new((right - left) as u32, (bottom - top) as u32).unwrap();
        let stride = i64::from(view.width);
        for y in top..bottom {
            for x in left..right {
                let color = ink([x, y]);
                full.pixels_mut()[(y * stride + x) as usize] = color;
                let at = (y - top) * (right - left) + x - left;
                crop.pixels_mut()[at as usize] = color;
            }
        }
        (full, crop, [left, top])
    }

    /// The canvas of `view`, drawn in `tile`-sized refreshes as the
    /// `enhanced` world is, on both paths with both layers, with the cursor
    /// on the hex at `at`: the four must agree, and the layer must show
    /// when it has pixels.
    fn assert_same_canvas(
        label: &str,
        view: MapView,
        tile: [u32; 2],
        at: [i32; 2],
        rect: [i64; 4],
        lit: bool,
    ) {
        let _gpu = gpu::test_lock();
        let (map, art) = (map(), art());
        let objects = scene(Vec::new());
        let palette = Palette::parse(&palette_bytes()).unwrap();
        let tile_light = vec![40000_i64; 40000];
        let light = lit.then_some(MapLight {
            palette: &palette,
            ambient: 30000,
            tiles: &tile_light,
        });
        let (full, crop, origin) = layers(view, rect);
        let hex = view.grid.pick_hex(at).unwrap();
        let mut cpu = SceneRenderer::cpu();
        let mut gpu = gpu::test_device().map(SceneRenderer::with_device);
        let mut reference = SceneRenderer::new(RenderPath::Reference);
        let mut shown = false;
        for y in (0..view.height).step_by(tile[1] as usize) {
            for x in (0..view.width).step_by(tile[0] as usize) {
                let offset = [i64::from(x), i64::from(y)];
                let part = part_of(view, tile, offset);
                let cursor = |layer, at: [i64; 2]| SceneCursor {
                    hex,
                    art: (layer, [at[0] - offset[0], at[1] - offset[1]]),
                    outline: None,
                    text: Some((layer, [at[0] - offset[0], at[1] - offset[1]])),
                };
                let whole = scene_of(light, Some(cursor(&full, [0, 0])));
                let tight = scene_of(light, Some(cursor(&crop, origin)));
                let want = reference.render(&map, &objects, part, &art, whole).unwrap();
                let bare = reference
                    .render(&map, &objects, part, &art, scene_of(light, None))
                    .unwrap();
                shown |= want.pixels() != bare.pixels();
                for (path, scene, instanced) in [
                    ("reference, cropped", tight, false),
                    ("instanced, view-sized", whole, true),
                    ("instanced, cropped", tight, true),
                ] {
                    let renderer = if instanced { &mut cpu } else { &mut reference };
                    let got = renderer.render(&map, &objects, part, &art, scene).unwrap();
                    assert!(
                        got.pixels() == want.pixels(),
                        "{label}: {path} at tile {offset:?}"
                    );
                    if instanced {
                        assert!(matches!(
                            renderer.last_refresh(),
                            Some(RefreshReport::Instanced(_))
                        ));
                    }
                }
                if let Some(renderer) = &mut gpu {
                    for scene in [whole, tight] {
                        let got = renderer.render(&map, &objects, part, &art, scene).unwrap();
                        assert!(got.pixels() == want.pixels(), "{label}: GPU at {offset:?}");
                        assert!(matches!(
                            renderer.last_refresh(),
                            Some(RefreshReport::Instanced(_))
                        ));
                    }
                }
            }
        }
        assert_eq!(shown, rect[0] < rect[2] && rect[1] < rect[3], "{label}");
    }

    /// The tile of `view` at canvas pixel `offset`.
    fn part_of(view: MapView, tile: [u32; 2], offset: [i64; 2]) -> MapView {
        MapView {
            grid: HexGrid {
                origin: [0, 1].map(|axis| view.grid.origin[axis] - offset[axis]),
            },
            width: (view.width - offset[0] as u32).min(tile[0]),
            height: (view.height - offset[1] as u32).min(tile[1]),
            ..view
        }
    }

    /// A cropped cursor layer draws the view-sized layer's pixels on both
    /// paths, at each edge and corner of the canvas and with no pixels.
    #[test]
    fn a_cropped_cursor_layer_draws_the_same_pixels_at_every_edge() {
        let view = view_of(640, 380);
        for (label, at, rect) in [
            ("left edge", [15, 170], [0, 150, 30, 190]),
            ("right edge", [625, 170], [610, 150, 640, 190]),
            ("top edge", [320, 12], [300, 0, 340, 25]),
            ("bottom edge", [320, 367], [300, 355, 340, 380]),
            ("top left", [10, 10], [0, 0, 20, 20]),
            ("top right", [630, 10], [620, 0, 640, 20]),
            ("bottom left", [10, 370], [0, 360, 20, 380]),
            ("bottom right", [630, 370], [620, 360, 640, 380]),
            ("nothing drawn", [320, 190], [0, 0, 0, 0]),
        ] {
            for lit in [false, true] {
                assert_same_canvas(label, view, [640, 380], at, rect, lit);
            }
        }
    }

    /// Tiled as the `enhanced` world is: a layer across a seam, and layers
    /// wholly outside a tile that still visits the cursor's hex, which then
    /// draws an instance off its canvas.
    #[test]
    fn a_cropped_cursor_layer_draws_the_same_pixels_across_tiles() {
        let view = view_of(1280, 380);
        for (label, at, rect) in [
            ("across the seam", [640, 190], [620, 170, 660, 210]),
            ("in the left tile", [520, 190], [500, 170, 540, 210]),
            ("in the right tile", [780, 190], [760, 170, 800, 210]),
        ] {
            assert_same_canvas(label, view, [640, 380], at, rect, true);
        }
    }

    /// A 4096-pixel canvas, tiled: the view-sized layer is an image past a
    /// 2048-pixel atlas limit in every refresh that draws it, the cropped
    /// one is not, and they draw alike.
    #[test]
    fn a_cropped_cursor_layer_fits_the_atlas_on_a_4096_pixel_canvas() {
        let view = view_of(4096, 100);
        let (at, rect) = ([4078, 60], [4060, 40, 4096, 80]);
        assert_same_canvas("wide canvas", view, [640, 380], at, rect, true);
        let (map, art) = (map(), art());
        let objects = scene(Vec::new());
        let (full, crop, origin) = layers(view, rect);
        let hex = view.grid.pick_hex(at).unwrap();
        let offset = [3840, 0];
        let part = part_of(view, [640, 380], offset);
        let widest = |layer: &IndexedImage, at: [i64; 2]| {
            let cursor = SceneCursor {
                hex,
                art: (layer, [at[0] - offset[0], at[1] - offset[1]]),
                outline: None,
                text: None,
            };
            let scene = scene_of(None, Some(cursor));
            let frame = gather_map_frame(&map, &objects, part, &art, scene, None).unwrap();
            frame.images.iter().map(|image| image.width).max().unwrap()
        };
        assert_eq!(widest(&full, [0, 0]), 4096);
        assert!(widest(&crop, origin) <= 80);
    }
}
