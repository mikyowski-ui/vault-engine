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
    Cutout, Drawn, FloorLight, ListIndexes, MapArt, MapLight, MapRenderError, MapScene, MapView,
    TRANSLUCENT, TileLight, check_floor_view, cutout_parts, draw_sequence_with, floor_squares,
    frame_rect, overscan_hexes, placement, render_map_scene, roof_squares, translucent_row,
};
use crate::{
    IndexedImage,
    layers::{
        BatchPlan, CanvasView, CutoutPlace, DrawMode, EXEMPT, FrameCounts, FrameInputs, FrameLayer,
        HexLightBuffer, InstanceError, LayerUniforms, LightSource, LookupTables, RenderPath,
        SortKey, SpriteImage, SpriteInstance, execute_plan,
        gpu::{GpuDevice, GpuExecutor},
    },
    outline::{OutlineInk, opaque_paint, ring},
};
use std::collections::{BTreeSet, HashMap};
use vault_formats::{
    MapFile,
    frm::FrmFrame,
    map::{MapObject, MapObjects},
    pro::{BasicFlags, Category},
};

/// SPEC-022 §3.4 one entry of the reference path's draw log, and of the
/// instanced path's sorted instance list: the layer, the hex or square
/// rank, the list index (outline position, creation rank or reference
/// draw rank in layers 4 to 6) and the part.
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
    /// The record a sort key stands for.
    pub fn of_key(key: SortKey) -> Result<Self, InstanceError> {
        let layer = FrameLayer::from_index(key.layer())?;
        let (primary, secondary) = (key.primary(), key.secondary());
        Ok(match layer {
            FrameLayer::Floor | FrameLayer::Roof => Self::new(layer, primary, 0, secondary as u8),
            FrameLayer::Flat | FrameLayer::Upright | FrameLayer::Outline => {
                Self::new(layer, primary, secondary >> 3, (secondary & 7) as u8)
            }
            FrameLayer::FloatingText | FrameLayer::NoHex => Self::new(layer, primary, secondary, 0),
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
/// Refused (a typed [`InstanceError`]): the `enhanced` shroud (it has no
/// layer in §2.1), the roof peek (the reference X-ray differs from §3.10),
/// a ground-item highlight (an outline past position 99), a cutout drawn
/// without a light, an opaque outline of index 0, a translucent outline
/// whose blend table is not one of the five, and tables that are not the
/// light palette's.
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
    if scene.shroud.is_some() {
        return Err(InstanceError::Unexpressible("the enhanced shroud").into());
    }
    if view.roofs && scene.roof_cutaway.is_some() {
        return Err(InstanceError::PeekMismatch.into());
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
    let cursor = scene.cursor;
    let extra = if scene.overscan {
        overscan_hexes(view, elevation, art)
    } else {
        Vec::new()
    };
    let cursor_place = cursor.map(|cursor| (cursor.hex, cursor.order));
    let (placed, hexless) = draw_sequence_with(view, elevation, cursor_place, &extra);
    let lists = ListIndexes::new(elevation, cursor_place);
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
    for drawn in placed {
        let index = match (drawn, cursor) {
            (Drawn::Object(index), _) => index,
            (Drawn::Cursor, Some(cursor)) => {
                let key =
                    SortKey::object(FrameLayer::Flat, hex_u32(cursor.hex)?, lists.cursor[0], 0)?;
                draw_layer(&mut builder, key, cursor.art.0, cursor.art.1, cursor.hex);
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
                    let key = SortKey::object(
                        FrameLayer::Upright,
                        hex_u32(cursor.hex)?,
                        lists.cursor[1],
                        0,
                    )?;
                    draw_layer(&mut builder, key, text, origin, cursor.hex);
                }
                continue;
            }
            (Drawn::Cursor | Drawn::CursorText, None) => continue,
        };
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
        let light = hex_light(hex);
        let cut = cutout.filter(|(cut, _)| cut.cuts(view, object)).is_some();
        if cut && !lit {
            return Err(InstanceError::Unexpressible("a cutout without a light").into());
        }
        let mask = if cut { mask_rect } else { None };
        for (part, part_rect, through) in cutout_parts(rect, refresh, mask) {
            let key = SortKey::object(layer, hex_u32(hex)?, lists.objects[index], part)?;
            let (mode, flags, light, aux) = if through {
                (DrawMode::Egg, 0, light, 0)
            } else if cut {
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

    // Layer 3: the roofs, at the ambient light, through the cutout.
    if view.roofs {
        for (rank, frame, corner) in roof_squares(map, view, art) {
            let image = builder.frame(frame);
            let rect = frame_rect(frame, corner);
            if mask_rect.is_some() && !lit {
                return Err(InstanceError::Unexpressible("a cutout without a light").into());
            }
            for (part, part_rect, through) in cutout_parts(rect, refresh, mask_rect) {
                let key = SortKey::roof(rank, part)?;
                let (mode, flags, light) = match (lit, through) {
                    (false, _) => (DrawMode::Plain, 0, Light::None),
                    (true, true) => (DrawMode::Egg, 0, Light::Ambient),
                    (true, false) => (DrawMode::Lit, EXEMPT, Light::Ambient),
                };
                builder.push(key, image, corner, part_rect, mode, flags, light, clip);
            }
        }
    }

    // Layer 4: the outlines, as ring images the outline painter records.
    for (position, (frame, origin, ink)) in outlines.into_iter().chain(highlights).enumerate() {
        let key = SortKey::outline(u32::try_from(position).unwrap_or(u32::MAX), 0)?;
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
        peek: None,
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
/// instanced path draws with the light palette's, so the two must agree.
fn check_cut_palette(
    cut: &Cutout,
    light: Option<MapLight>,
    tables: Option<&LookupTables>,
) -> Result<(), InstanceError> {
    let (Some(light), Some(tables)) = (light, tables) else {
        return Ok(());
    };
    if std::ptr::eq(cut.palette, light.palette) {
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
            if gray > 7 {
                return Err(InstanceError::GrayRange(gray));
            }
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

/// SPEC-022 §3.1 and §3.12 the world canvas's renderer: the reference
/// path (the default), or the instanced path with a fallback to the
/// reference for every refresh it refuses. The setting is not saved and
/// writes no input-log record (§3.13).
pub struct SceneRenderer {
    path: RenderPath,
    executor: Executor,
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
        // The tables of the light palette, rebuilt each refresh (the GPU
        // executor re-uploads them only when they change).
        let palette = scene
            .light
            .map(|light| light.palette)
            .or(scene.cutout.map(|cut| cut.palette));
        let tables = palette.map(LookupTables::from_palette);
        let mut frame = gather_map_frame(map, objects, view, art, scene, tables.as_ref())?;
        if let Some(tamper) = self.tamper {
            tamper(&mut frame.instances);
        }
        let plan = BatchPlan::new(&frame.instances)?;
        let empty = LookupTables {
            intensity: vec![0; 65536].into(),
            mix: vec![0; 65536].into(),
            gray: [[0; 256]; 2],
            blends: std::array::from_fn(|_| Box::new([[0; 256]; 8])),
        };
        let inputs = frame.inputs(tables.as_ref().unwrap_or(&empty));
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
