//! SPEC-022 the instanced path's frame model: the frame layers and the
//! 64-bit sort key (§2.1), the sprite instance and its seven draw modes
//! (§2.2, §3.6), the batch plan (§3.7), the hex light buffer (§2.3, §3.8),
//! the layer uniform block (§2.4), and a CPU executor that draws a plan
//! exactly as the GPU executor ([`super::gpu`]) does.
//!
//! The reference path (the CPU compositor of [`crate::map`]) stays the
//! default ([`RenderPath::Reference`]); nothing here runs unless the
//! instanced path is asked for.
use crate::{IndexedImage, map::RoofPeekParams, outline::BlendTable};
use std::{borrow::Cow, fmt, ops::Range};
use vault_formats::Palette;

/// SPEC-022 §3.1 the two ways to draw the world canvas. The reference
/// path is the default everywhere (Decision D-7); the instanced path is
/// opt-in and draws the same canvas, byte for byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RenderPath {
    /// The CPU compositor of [`crate::map`].
    #[default]
    Reference,
    /// The GPU-instanced path of this module.
    Instanced,
}

impl RenderPath {
    /// The setting's value (`render_path`, `--render-path`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Instanced => "instanced",
        }
    }
    /// The path a setting value names, if any.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "reference" => Some(Self::Reference),
            "instanced" => Some(Self::Instanced),
            _ => None,
        }
    }
}

/// SPEC-022 §2.1 the frame layers, drawn in this order. Layer 7 is
/// reserved and has no variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FrameLayer {
    Floor = 0,
    Flat = 1,
    Upright = 2,
    Roof = 3,
    Outline = 4,
    FloatingText = 5,
    NoHex = 6,
}

impl FrameLayer {
    /// The layer numbered `index`; 7 and up are refused.
    pub fn from_index(index: u8) -> Result<Self, InstanceError> {
        Ok(match index {
            0 => Self::Floor,
            1 => Self::Flat,
            2 => Self::Upright,
            3 => Self::Roof,
            4 => Self::Outline,
            5 => Self::FloatingText,
            6 => Self::NoHex,
            _ => return Err(InstanceError::ReservedLayer(index)),
        })
    }
}

/// The largest primary field value plus one (29 bits).
const PRIMARY_LIMIT: u32 = 1 << 29;
/// The largest list index plus one.
pub const LIST_INDEX_LIMIT: u32 = 1 << 29;
/// The hex numbers, 0 to 39999.
pub const HEXES: u32 = 40000;
/// The square positions and ranks, 0 to 9999.
pub const SQUARES: u32 = 10000;
/// The roof layer's primary after every square rank: the enhanced shroud.
pub const SHROUD_PRIMARY: u32 = SQUARES;

/// SPEC-022 §2.1 the 64-bit sort key: layer in bits 61-63, primary in
/// bits 32-60, secondary in bits 0-31. Instances draw in ascending key
/// order. Every constructor checks the §2.1 field domains.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortKey(pub u64);

impl fmt::Debug for SortKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self}")
    }
}

impl fmt::Display for SortKey {
    /// 16 uppercase hexadecimal digits.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016X}", self.0)
    }
}

impl SortKey {
    /// The key of a layer, primary and secondary field, each checked
    /// against its layer's domain (§2.1).
    pub fn new(layer: u8, primary: u32, secondary: u32) -> Result<Self, InstanceError> {
        let layer = FrameLayer::from_index(layer)?;
        let domain = |field: &'static str, value: u64| Err(InstanceError::Domain { field, value });
        if primary >= PRIMARY_LIMIT {
            return domain("primary", u64::from(primary));
        }
        let part = secondary & 7;
        match layer {
            FrameLayer::Floor => {
                if primary >= SQUARES {
                    return domain("square rank", u64::from(primary));
                }
                if secondary != 0 {
                    return domain("floor part", u64::from(secondary));
                }
            }
            FrameLayer::Flat | FrameLayer::Upright => {
                if primary >= HEXES {
                    return domain("hex", u64::from(primary));
                }
                if part > 4 {
                    return domain("part", u64::from(part));
                }
            }
            FrameLayer::Roof => {
                if primary > SHROUD_PRIMARY {
                    return domain("square rank", u64::from(primary));
                }
                if primary < SHROUD_PRIMARY && secondary > 4 {
                    return domain("part", u64::from(secondary));
                }
            }
            FrameLayer::Outline => {
                if primary != 0 {
                    return domain("outline primary", u64::from(primary));
                }
                if part > 1 {
                    return domain("outline part", u64::from(part));
                }
            }
            FrameLayer::FloatingText | FrameLayer::NoHex => {
                if primary != 0 {
                    return domain("primary", u64::from(primary));
                }
            }
        }
        Ok(Self(
            (u64::from(layer as u8) << 61) | (u64::from(primary) << 32) | u64::from(secondary),
        ))
    }
    /// A floor square of rank `rank` (part 0).
    pub fn floor(rank: u32) -> Result<Self, InstanceError> {
        Self::new(FrameLayer::Floor as u8, rank, 0)
    }
    /// An object of the flat or upright pass: its hex, its list index and
    /// its part (0 to 4).
    pub fn object(
        layer: FrameLayer,
        hex: u32,
        list_index: u32,
        part: u8,
    ) -> Result<Self, InstanceError> {
        if !matches!(layer, FrameLayer::Flat | FrameLayer::Upright) {
            return Err(InstanceError::Domain {
                field: "object layer",
                value: layer as u64,
            });
        }
        Self::new(layer as u8, hex, Self::list_secondary(list_index, part)?)
    }
    /// A roof square of rank `rank`, part 0 to 4.
    pub fn roof(rank: u32, part: u8) -> Result<Self, InstanceError> {
        if rank >= SQUARES {
            return Err(InstanceError::Domain {
                field: "square rank",
                value: u64::from(rank),
            });
        }
        Self::new(FrameLayer::Roof as u8, rank, u32::from(part))
    }
    /// The enhanced shroud's draw of rank `rank` in its refresh: after every
    /// roof square, before every outline.
    pub fn shroud(rank: u32) -> Result<Self, InstanceError> {
        Self::new(FrameLayer::Roof as u8, SHROUD_PRIMARY, rank)
    }
    /// An outline at running position `position` (a list entry, then the
    /// ground-item highlights): part 0 the ring, part 1 the wrap column.
    pub fn outline(position: u32, part: u8) -> Result<Self, InstanceError> {
        Self::new(
            FrameLayer::Outline as u8,
            0,
            Self::list_secondary(position, part)?,
        )
    }
    /// A floating text of creation rank `rank`.
    pub fn floating_text(rank: u32) -> Result<Self, InstanceError> {
        Self::new(FrameLayer::FloatingText as u8, 0, rank)
    }
    /// An object without a hex, of reference draw rank `rank`.
    pub fn no_hex(rank: u32) -> Result<Self, InstanceError> {
        Self::new(FrameLayer::NoHex as u8, 0, rank)
    }
    /// `8 × index + part`, with the index below 2^29 and the part below 8.
    fn list_secondary(index: u32, part: u8) -> Result<u32, InstanceError> {
        if index >= LIST_INDEX_LIMIT {
            return Err(InstanceError::Domain {
                field: "list index",
                value: u64::from(index),
            });
        }
        if part > 7 {
            return Err(InstanceError::Domain {
                field: "part",
                value: u64::from(part),
            });
        }
        Ok(8 * index + u32::from(part))
    }
    /// The layer field (bits 61-63).
    pub fn layer(self) -> u8 {
        (self.0 >> 61) as u8
    }
    /// The primary field (bits 32-60).
    pub fn primary(self) -> u32 {
        ((self.0 >> 32) as u32) & (PRIMARY_LIMIT - 1)
    }
    /// The secondary field (bits 0-31).
    pub fn secondary(self) -> u32 {
        self.0 as u32
    }
}

/// SPEC-022 §2.2 the seven draw modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DrawMode {
    /// `s`: opaque outlines, floating text, an unlit scene's items.
    Plain = 0,
    /// `lit(s, L)`: objects, roofs, uniform floor squares.
    Lit = 1,
    /// `lit(s, L)` with L from a level image: interpolated floor squares.
    FloorMap = 2,
    /// `I[F_t[gray(s)][D]][L]`: one translucency bit.
    Translucent = 3,
    /// The player cutout's masked blend over D.
    Egg = 4,
    /// `F_t[g][D]` applied once per paint: translucent outlines.
    OutlineBlend = 5,
    /// `M[s - 1][D]`, with M the remap table: the enhanced shroud.
    Remap = 6,
}

impl DrawMode {
    /// Whether the mode reads the canvas pixel under it (§3.7).
    pub fn reads_canvas(self) -> bool {
        matches!(
            self,
            Self::Translucent | Self::Egg | Self::OutlineBlend | Self::Remap
        )
    }
    /// The mode of a code, if any.
    pub fn from_code(code: u8) -> Result<Self, InstanceError> {
        Ok(match code {
            0 => Self::Plain,
            1 => Self::Lit,
            2 => Self::FloorMap,
            3 => Self::Translucent,
            4 => Self::Egg,
            5 => Self::OutlineBlend,
            6 => Self::Remap,
            _ => return Err(InstanceError::UnknownMode(code)),
        })
    }
}

/// SPEC-022 §2.2 where an instance's light level comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LightSource {
    None = 0,
    /// `max(ambient, B[hex])`.
    Hex = 1,
    /// The ambient light.
    Ambient = 2,
    /// A fixed light value.
    Fixed = 3,
    /// A level image, one level per pixel.
    LevelImage = 4,
}

impl LightSource {
    /// The light source of a code, if any.
    pub fn from_code(code: u8) -> Result<Self, InstanceError> {
        Ok(match code {
            0 => Self::None,
            1 => Self::Hex,
            2 => Self::Ambient,
            3 => Self::Fixed,
            4 => Self::LevelImage,
            _ => return Err(InstanceError::UnknownLightSource(code)),
        })
    }
}

/// Instance flag bit 0: indexes 229-255 are left unlit.
pub const EXEMPT: u8 = 1;
/// Instance flag bit 1: the roof peek test applies.
pub const PEEK: u8 = 2;

/// SPEC-022 §3.12 why the instanced path refuses a refresh. Every cause
/// is a typed error; the refresh is then drawn by the reference path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstanceError {
    #[error("layer {0} is reserved")]
    ReservedLayer(u8),
    #[error("{field} {value} is outside its domain")]
    Domain { field: &'static str, value: u64 },
    #[error("two instances share the key {0}")]
    DuplicateKey(SortKey),
    #[error("unknown draw mode {0}")]
    UnknownMode(u8),
    #[error("unknown light source {0}")]
    UnknownLightSource(u8),
    #[error("draw mode {mode:?} cannot use light source {light:?}")]
    ModeLight { mode: DrawMode, light: LightSource },
    #[error("a level image holds level {0}, above 128")]
    LevelRange(u8),
    #[error("a remap source value {0} is above its table")]
    RemapRange(u8),
    #[error("blend table {0} is outside 0-4")]
    BlendTable(u8),
    #[error("an instance's rectangle is empty or reads outside its image")]
    SourceRange,
    #[error("image {0} is missing")]
    MissingImage(usize),
    #[error("a {0}-by-{1} texture exceeds the limits")]
    TextureLimit(u32, u32),
    #[error("the atlas is full")]
    AtlasFull,
    #[error("upload failed: {0}")]
    Upload(String),
    #[error("no draw mode expresses {0}")]
    Unexpressible(&'static str),
    #[error("a peek cap or clamp changed a value (SPEC-022 §3.10)")]
    PeekBound,
    #[error("no GPU adapter or device: {0}")]
    NoDevice(String),
    #[error("the GPU device is lost")]
    DeviceLost,
}

impl InstanceError {
    /// The cause's name, logged once per session (§3.12).
    pub fn cause(&self) -> &'static str {
        match self {
            Self::ReservedLayer(_) => "reserved layer",
            Self::Domain { .. } => "field domain",
            Self::DuplicateKey(_) => "duplicate key",
            Self::UnknownMode(_) => "unknown mode",
            Self::UnknownLightSource(_) | Self::ModeLight { .. } => "unknown light source",
            Self::LevelRange(_) => "level range",
            Self::RemapRange(_) | Self::BlendTable(_) => "table range",
            Self::SourceRange | Self::MissingImage(_) => "source range",
            Self::TextureLimit(..) => "texture limit",
            Self::AtlasFull => "atlas full",
            Self::Upload(_) => "upload error",
            Self::Unexpressible(_) => "unexpressible draw",
            Self::PeekBound => "peek bound",
            Self::NoDevice(_) => "no device",
            Self::DeviceLost => "device lost",
        }
    }
}

/// An 8-bit image an instance reads: a sprite frame, a ring image, a text
/// box, a level image, a shroud image or a remap table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteImage<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: Cow<'a, [u8]>,
}

impl<'a> SpriteImage<'a> {
    /// An image of `width` by `height` `pixels`, row by row.
    pub fn new(width: u32, height: u32, pixels: impl Into<Cow<'a, [u8]>>) -> Self {
        Self {
            width,
            height,
            pixels: pixels.into(),
        }
    }
    /// Pixel (x, y); the caller checks the bounds.
    fn at(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * self.width + x) as usize]
    }
    /// Whether the pixel count matches the size.
    fn consistent(&self) -> bool {
        self.width as usize * self.height as usize == self.pixels.len()
    }
}

/// A half-open rectangle `[x0, y0, x1, y1]`.
pub type Rect = [i32; 4];

/// The intersection of two rectangles, if not empty.
pub fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let rect = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// The smallest rectangle holding both.
fn union(a: Rect, b: Rect) -> Rect {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// SPEC-022 §2.2 one rectangle of one image, drawn with one draw mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteInstance {
    pub key: SortKey,
    pub mode: DrawMode,
    /// [`EXEMPT`] and [`PEEK`].
    pub flags: u8,
    pub light: LightSource,
    /// For [`LightSource::Hex`]: the hex whose light is used.
    pub hex: u32,
    /// For [`LightSource::Fixed`]: the light value.
    pub fixed: u32,
    /// The canvas position of the rectangle's top-left pixel.
    pub dst: [i32; 2],
    /// The rectangle's size, at least 1 by 1.
    pub size: [u32; 2],
    /// The source image (an index into the frame's images), and the image
    /// pixel drawn at `dst`.
    pub image: usize,
    pub src: [u32; 2],
    /// For [`LightSource::LevelImage`]: the level image and its pixel at
    /// `dst`. For [`DrawMode::Remap`]: the remap table and its first row's
    /// first entry.
    pub level: Option<(usize, [u32; 2])>,
    /// The scissor rectangle.
    pub clip: Rect,
    /// Bits 0-2 the blend table, bits 8-10 the gray level (mode 5).
    pub aux: u16,
}

impl SpriteInstance {
    /// The instance's rectangle on the canvas.
    pub fn rect(&self) -> Rect {
        let x1 = i64::from(self.dst[0]) + i64::from(self.size[0]);
        let y1 = i64::from(self.dst[1]) + i64::from(self.size[1]);
        [
            self.dst[0],
            self.dst[1],
            x1.min(i64::from(i32::MAX)) as i32,
            y1.min(i64::from(i32::MAX)) as i32,
        ]
    }
    /// The rectangle clipped to the clip, if not empty.
    pub fn clipped(&self) -> Option<Rect> {
        intersect(self.rect(), self.clip)
    }
    /// The blend table (aux bits 0-2).
    pub fn blend_table(&self) -> u8 {
        (self.aux & 7) as u8
    }
    /// The gray level of mode 5 (aux bits 8-10).
    pub fn gray(&self) -> u8 {
        ((self.aux >> 8) & 7) as u8
    }
}

/// §3.7 the two batch kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BatchKind {
    /// Modes 0-2: one draw, later instances win.
    Overwrite,
    /// Modes 3-6: D comes from a snapshot; the clipped rectangles are
    /// disjoint.
    Read,
}

/// One draw call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub kind: BatchKind,
    pub clip: Rect,
    /// Positions in [`BatchPlan::order`].
    pub range: Range<usize>,
    /// For a read batch: the bounding box of its clipped rectangles.
    pub snapshot: Option<Rect>,
}

/// §3.7 counts per refresh, for tests and the F9 overlay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCounts {
    pub instances: usize,
    pub batches: usize,
    pub read_batches: usize,
}

/// §3.2 steps 3-5: the sorted, culled instances and their batches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchPlan {
    /// Indexes into the instance list, in ascending key order, culled.
    pub order: Vec<usize>,
    pub batches: Vec<Batch>,
}

impl BatchPlan {
    /// Sort `instances` by key (refusing equal keys), drop those whose
    /// clipped rectangle is empty, and split the rest into batches.
    pub fn new(instances: &[SpriteInstance]) -> Result<Self, InstanceError> {
        let sorted = sort_by_key(instances)?;
        let order: Vec<usize> = sorted
            .into_iter()
            .filter(|index| instances[*index].clipped().is_some())
            .collect();
        let mut batches: Vec<Batch> = Vec::new();
        // The clipped rectangles of the current read batch.
        let mut members: Vec<Rect> = Vec::new();
        for (position, index) in order.iter().enumerate() {
            let instance = &instances[*index];
            let rect = instance.clipped().expect("culled");
            let kind = if instance.mode.reads_canvas() {
                BatchKind::Read
            } else {
                BatchKind::Overwrite
            };
            let joins = batches.last().is_some_and(|batch| {
                batch.kind == kind
                    && batch.clip == instance.clip
                    && (kind == BatchKind::Overwrite
                        || members
                            .iter()
                            .all(|other| intersect(*other, rect).is_none()))
            });
            if joins {
                let batch = batches.last_mut().expect("joined");
                batch.range.end = position + 1;
                if kind == BatchKind::Read {
                    batch.snapshot = batch.snapshot.map(|snapshot| union(snapshot, rect));
                    members.push(rect);
                }
            } else {
                members.clear();
                if kind == BatchKind::Read {
                    members.push(rect);
                }
                batches.push(Batch {
                    kind,
                    clip: instance.clip,
                    range: position..position + 1,
                    snapshot: (kind == BatchKind::Read).then_some(rect),
                });
            }
        }
        Ok(Self { order, batches })
    }
    /// The counts of the plan.
    pub fn counts(&self) -> FrameCounts {
        FrameCounts {
            instances: self.order.len(),
            batches: self.batches.len(),
            read_batches: self
                .batches
                .iter()
                .filter(|batch| batch.kind == BatchKind::Read)
                .count(),
        }
    }
}

/// The instance indexes in ascending key order; two equal keys refuse.
pub fn sort_by_key(instances: &[SpriteInstance]) -> Result<Vec<usize>, InstanceError> {
    let mut order: Vec<usize> = (0..instances.len()).collect();
    order.sort_unstable_by_key(|index| instances[*index].key);
    if let Some(pair) = order
        .windows(2)
        .find(|pair| instances[pair[0]].key == instances[pair[1]].key)
    {
        return Err(InstanceError::DuplicateKey(instances[pair[0]].key));
    }
    Ok(order)
}

/// The five blend tables' foreground color keys, by table number (§2.2):
/// wall, glass, steam, energy and red translucency.
pub const BLEND_KEYS: [u16; 5] = [25439, 10239, 32767, 30689, 31744];

/// SPEC-022 §3.11 the lookup tables a refresh draws with: the intensity
/// table `I[i][k]` at `256 i + k`, the additive mix `X[a][b]` at
/// `256 a + b`, the common and glass gray levels, and the five blend
/// tables `F_t[g][D]`.
#[derive(Clone, PartialEq, Eq)]
pub struct LookupTables {
    pub intensity: Box<[u8]>,
    pub mix: Box<[u8]>,
    /// `[common, glass]` gray level of every index.
    pub gray: [[u8; 256]; 2],
    pub blends: [Box<BlendTable>; 5],
}

impl fmt::Debug for LookupTables {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LookupTables")
    }
}

impl LookupTables {
    /// The tables of `palette`, as the reference path uses them.
    pub fn from_palette(palette: &Palette) -> Self {
        let intensity: Box<[u8]> = palette.intensity_table().into();
        let mix = (0..=u16::MAX)
            .map(|index| palette.additive((index >> 8) as u8, index as u8))
            .collect();
        let gray = std::array::from_fn(|glass| {
            std::array::from_fn(|index| palette.gray_level(index as u8, glass == 1))
        });
        let blends = BLEND_KEYS.map(|key| crate::outline::blend_table(palette, key));
        Self {
            intensity,
            mix,
            gray,
            blends,
        }
    }
    /// `I[i][k]`.
    fn intensity(&self, index: u8, level: u8) -> u8 {
        self.intensity[usize::from(index) * 256 + usize::from(level)]
    }
    /// `X[a][b]`.
    fn mix(&self, a: u8, b: u8) -> u8 {
        self.mix[usize::from(a) * 256 + usize::from(b)]
    }
    /// Whether the tables have the expected sizes.
    fn consistent(&self) -> bool {
        self.intensity.len() == 65536 && self.mix.len() == 65536
    }
}

/// SPEC-022 §2.3 the local light of every hex of the current elevation,
/// clamped to `[0, 65536]`.
#[derive(Clone, PartialEq, Eq)]
pub struct HexLightBuffer {
    values: Box<[u32]>,
}

impl fmt::Debug for HexLightBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "HexLightBuffer({:016X})", self.digest())
    }
}

impl Default for HexLightBuffer {
    fn default() -> Self {
        Self {
            values: vec![0; HEXES as usize].into(),
        }
    }
}

impl HexLightBuffer {
    /// §3.8 step 2: one accumulated light clamped to `[0, 65536]`.
    pub fn pack_value(accumulated: i64) -> u32 {
        accumulated.clamp(0, 65536) as u32
    }
    /// The buffer of a world's light, `tiles[h]` the accumulated light of
    /// hex h; a missing entry is 0.
    pub fn pack(tiles: &[i64]) -> Self {
        Self {
            values: (0..HEXES as usize)
                .map(|hex| Self::pack_value(tiles.get(hex).copied().unwrap_or(0)))
                .collect(),
        }
    }
    /// The 40000 entries in hex order.
    pub fn values(&self) -> &[u32] {
        &self.values
    }
    /// `B[hex]`, 0 for a hex outside 0 to 39999.
    pub fn get(&self, hex: u32) -> u32 {
        self.values.get(hex as usize).copied().unwrap_or(0)
    }
    /// §2.3 the byte image: little-endian u32 in hex order.
    pub fn bytes(&self) -> Vec<u8> {
        self.values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }
    /// §2.3 the FNV-1a 64 digest of the byte image.
    pub fn digest(&self) -> u64 {
        fnv1a64(&self.bytes())
    }
}

/// SPEC-021 §2.1 FNV-1a 64.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// §2.4 the player cutout's place: the canvas position of mask pixel
/// (0, 0) and the mask's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutoutPlace {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// SPEC-022 §2.4 the layer uniform block of one refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerUniforms {
    /// The canvas size in pixels.
    pub canvas: [u32; 2],
    /// OX, OY: the canvas's shared translation.
    pub offset: [i32; 2],
    /// The ambient light, clamped to 0-65536 when packed.
    pub ambient: i64,
    pub elevation: u32,
    /// The cutout, when its mask is bound.
    pub cutout: Option<CutoutPlace>,
    /// The roof peek, when active.
    pub peek: Option<RoofPeekParams>,
}

impl LayerUniforms {
    /// The block's size in bytes.
    pub const SIZE: usize = 128;
    /// The ambient light written into the block.
    pub fn ambient(&self) -> u32 {
        self.ambient.clamp(0, 65536) as u32
    }
    /// §2.4 the 128 little-endian bytes.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut words = [0_u32; 32];
        words[0] = self.canvas[0];
        words[1] = self.canvas[1];
        words[2] = self.offset[0] as u32;
        words[3] = self.offset[1] as u32;
        words[4] = self.ambient();
        words[5] = self.elevation;
        words[6] = u32::from(self.cutout.is_some()) | u32::from(self.peek.is_some()) << 1;
        if let Some(cutout) = self.cutout {
            words[8] = cutout.left as u32;
            words[9] = cutout.top as u32;
            words[10] = cutout.width;
            words[11] = cutout.height;
        }
        if let Some(peek) = self.peek {
            words[12] = peek.center[0] as u32;
            words[13] = peek.center[1] as u32;
            words[14] = peek.outer();
            words[15] = peek.inner;
            words[16..32].copy_from_slice(&peek.thresholds_squared());
        }
        let mut bytes = [0; Self::SIZE];
        for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *chunk = word.to_le_bytes();
        }
        bytes
    }
}

/// Everything a refresh's instances read besides the canvas.
#[derive(Debug, Clone, Copy)]
pub struct FrameInputs<'a> {
    pub tables: &'a LookupTables,
    pub light: &'a HexLightBuffer,
    pub uniforms: LayerUniforms,
    /// The cutout mask's pixels (bound when `uniforms.cutout` is set).
    pub mask: Option<&'a SpriteImage<'a>>,
    pub images: &'a [SpriteImage<'a>],
}

/// The largest canvas side (§3.11 step 6).
pub const MAX_CANVAS: u32 = 16384;

impl FrameInputs<'_> {
    /// §3.6 the level `L = E >> 9` of an instance (0-128), for every light
    /// source but the level image.
    pub fn level(&self, instance: &SpriteInstance) -> u8 {
        (self.effective(instance) >> 9) as u8
    }
    /// §3.6 the effective light E of an instance (0-65536): 65536 for a
    /// source that has none (the level is then unused).
    pub fn effective(&self, instance: &SpriteInstance) -> u32 {
        let ambient = self.uniforms.ambient();
        match instance.light {
            LightSource::Hex => ambient.max(self.light.get(instance.hex)),
            LightSource::Ambient => ambient,
            LightSource::Fixed => instance.fixed.min(65536),
            LightSource::None | LightSource::LevelImage => 65536,
        }
    }
    /// The image `index`.
    fn image(&self, index: usize) -> Result<&SpriteImage<'_>, InstanceError> {
        self.images
            .get(index)
            .filter(|image| image.consistent())
            .ok_or(InstanceError::MissingImage(index))
    }
    /// §3.12 every check that refuses a refresh before any pixel is drawn:
    /// the canvas size, each instance's mode, light source, tables, and
    /// source and level rectangles, and every level image's values.
    pub fn validate(&self, instances: &[SpriteInstance]) -> Result<(), InstanceError> {
        let [width, height] = self.uniforms.canvas;
        if width == 0 || height == 0 || width > MAX_CANVAS || height > MAX_CANVAS {
            return Err(InstanceError::TextureLimit(width, height));
        }
        if !self.tables.consistent() {
            return Err(InstanceError::Upload("lookup tables".into()));
        }
        if self.uniforms.cutout.is_some() != self.mask.is_some()
            || self.mask.is_some_and(|mask| {
                let place = self.uniforms.cutout.expect("bound");
                !mask.consistent() || [mask.width, mask.height] != [place.width, place.height]
            })
        {
            return Err(InstanceError::Upload("cutout mask".into()));
        }
        for instance in instances {
            FrameLayer::from_index(instance.key.layer())?;
            let allowed = match instance.mode {
                DrawMode::Plain | DrawMode::OutlineBlend | DrawMode::Remap => {
                    matches!(instance.light, LightSource::None)
                }
                DrawMode::Lit => matches!(
                    instance.light,
                    LightSource::Hex | LightSource::Ambient | LightSource::Fixed
                ),
                DrawMode::FloorMap => matches!(instance.light, LightSource::LevelImage),
                DrawMode::Translucent => {
                    matches!(instance.light, LightSource::Hex | LightSource::Fixed)
                }
                DrawMode::Egg => matches!(
                    instance.light,
                    LightSource::Hex | LightSource::Ambient | LightSource::None
                ),
            };
            if !allowed {
                return Err(InstanceError::ModeLight {
                    mode: instance.mode,
                    light: instance.light,
                });
            }
            if instance.mode == DrawMode::Egg && self.mask.is_none() {
                return Err(InstanceError::Upload("cutout mask".into()));
            }
            if instance.size[0] == 0 || instance.size[1] == 0 {
                return Err(InstanceError::SourceRange);
            }
            if matches!(
                instance.mode,
                DrawMode::Translucent | DrawMode::OutlineBlend
            ) && instance.blend_table() > 4
            {
                return Err(InstanceError::BlendTable(instance.blend_table()));
            }
            let image = self.image(instance.image)?;
            let fits = |image: &SpriteImage, at: [u32; 2]| {
                u64::from(at[0]) + u64::from(instance.size[0]) <= u64::from(image.width)
                    && u64::from(at[1]) + u64::from(instance.size[1]) <= u64::from(image.height)
            };
            if !fits(image, instance.src) {
                return Err(InstanceError::SourceRange);
            }
            if instance.mode == DrawMode::FloorMap {
                let (level, at) = instance.level.ok_or(InstanceError::SourceRange)?;
                let levels = self.image(level)?;
                if !fits(levels, at) {
                    return Err(InstanceError::SourceRange);
                }
                if let Some(high) = levels.pixels.iter().copied().find(|level| *level > 128) {
                    return Err(InstanceError::LevelRange(high));
                }
            }
            if instance.mode == DrawMode::Remap {
                let (table, at) = instance.level.ok_or(InstanceError::SourceRange)?;
                let table = self.image(table)?;
                if at[0].checked_add(256).is_none_or(|end| end > table.width)
                    || at[1] > table.height
                {
                    return Err(InstanceError::SourceRange);
                }
                let rows = table.height - at[1];
                let source = (0..instance.size[1]).flat_map(|v| {
                    (0..instance.size[0])
                        .map(move |u| image.at(instance.src[0] + u, instance.src[1] + v))
                });
                if let Some(high) = source
                    .into_iter()
                    .find(|code| u32::from(*code) > rows.min(255))
                {
                    return Err(InstanceError::RemapRange(high));
                }
            }
        }
        Ok(())
    }
}

/// The canvas an executor draws: `width` by `height` indexes.
pub struct CanvasView<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u8],
}

impl<'a> CanvasView<'a> {
    /// A view of an indexed image.
    pub fn of(image: &'a mut IndexedImage) -> Self {
        let (width, height) = (image.width(), image.height());
        Self {
            width,
            height,
            pixels: image.pixels_mut(),
        }
    }
    fn bounds(&self) -> Rect {
        [0, 0, self.width as i32, self.height as i32]
    }
}

/// The value at a canvas pixel for the snapshot a read batch reads.
struct Snapshot {
    rect: Rect,
    pixels: Vec<u8>,
}

impl Snapshot {
    fn take(canvas: &CanvasView, rect: Rect) -> Self {
        let width = (rect[2] - rect[0]).max(0) as usize;
        let mut pixels = Vec::with_capacity(width * (rect[3] - rect[1]).max(0) as usize);
        for y in rect[1]..rect[3] {
            let start = y as usize * canvas.width as usize + rect[0] as usize;
            pixels.extend_from_slice(&canvas.pixels[start..start + width]);
        }
        Self { rect, pixels }
    }
    fn get(&self, x: i32, y: i32) -> u8 {
        let width = (self.rect[2] - self.rect[0]) as usize;
        self.pixels[(y - self.rect[1]) as usize * width + (x - self.rect[0]) as usize]
    }
}

/// §3.6 the result of one instance pixel, or `None` when nothing happens.
/// `destination` gives D on demand (read modes only).
fn shade(
    inputs: &FrameInputs,
    instance: &SpriteInstance,
    level: u8,
    [u, v]: [u32; 2],
    [x, y]: [i32; 2],
    destination: impl Fn() -> u8,
) -> Option<u8> {
    let image = &inputs.images[instance.image];
    let s = image.at(instance.src[0] + u, instance.src[1] + v);
    if s == 0 {
        return None;
    }
    if instance.flags & PEEK != 0
        && let Some(peek) = inputs.uniforms.peek
        && peek.cuts([x, y], inputs.uniforms.offset)
    {
        return None;
    }
    let tables = inputs.tables;
    let exempt = instance.flags & EXEMPT != 0;
    let lit = |s: u8, level: u8| {
        if instance.light == LightSource::None || (exempt && s >= 229) {
            s
        } else {
            tables.intensity(s, level)
        }
    };
    Some(match instance.mode {
        DrawMode::Plain => s,
        DrawMode::Lit => lit(s, level),
        DrawMode::FloorMap => {
            let (levels, at) = instance.level.expect("validated");
            lit(s, inputs.images[levels].at(at[0] + u, at[1] + v))
        }
        DrawMode::Translucent => {
            let t = usize::from(instance.blend_table());
            let gray = tables.gray[usize::from(t == 1)][usize::from(s)].min(7);
            let blended = tables.blends[t][usize::from(gray)][usize::from(destination())];
            tables.intensity(blended, level)
        }
        DrawMode::Egg => {
            let s1 = lit(s, level);
            let weight = inputs
                .uniforms
                .cutout
                .zip(inputs.mask)
                .and_then(|(place, mask)| {
                    let mx = i64::from(x) - i64::from(place.left);
                    let my = i64::from(y) - i64::from(place.top);
                    ((0..i64::from(mask.width)).contains(&mx)
                        && (0..i64::from(mask.height)).contains(&my))
                    .then(|| mask.at(mx as u32, my as u32))
                })
                .unwrap_or(0)
                .min(128);
            if weight == 0 {
                s1
            } else {
                tables.mix(
                    tables.intensity(s1, weight),
                    tables.intensity(destination(), 128 - weight),
                )
            }
        }
        DrawMode::OutlineBlend => {
            let table = &tables.blends[usize::from(instance.blend_table())];
            let row = &table[usize::from(instance.gray())];
            (0..s).fold(destination(), |r, _| row[usize::from(r)])
        }
        DrawMode::Remap => {
            let (table, at) = instance.level.expect("validated");
            let row = at[1] + u32::from(s) - 1;
            inputs.images[table].at(at[0] + u32::from(destination()), row)
        }
    })
}

/// Draw one instance over the canvas, reading D from `snapshot` when
/// given, else from the canvas itself.
fn draw_instance(
    canvas: &mut CanvasView,
    inputs: &FrameInputs,
    instance: &SpriteInstance,
    snapshot: Option<&Snapshot>,
) {
    let Some([x0, y0, x1, y1]) = instance
        .clipped()
        .and_then(|rect| intersect(rect, canvas.bounds()))
    else {
        return;
    };
    let level = inputs.level(instance);
    let width = canvas.width as usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let at = y as usize * width + x as usize;
            let uv = [(x - instance.dst[0]) as u32, (y - instance.dst[1]) as u32];
            let live = canvas.pixels[at];
            let destination = || snapshot.map_or(live, |snapshot| snapshot.get(x, y));
            if let Some(value) = shade(inputs, instance, level, uv, [x, y], destination) {
                canvas.pixels[at] = value;
            }
        }
    }
}

/// SPEC-022 §3.7 the CPU executor: the batches of `plan` in order, each
/// read batch through a snapshot of its bounding box taken before its
/// draw. Validates first, so a refused refresh leaves the canvas as it
/// was.
pub fn execute_plan(
    canvas: &mut CanvasView,
    instances: &[SpriteInstance],
    plan: &BatchPlan,
    inputs: &FrameInputs,
) -> Result<(), InstanceError> {
    check_canvas(canvas, inputs)?;
    inputs.validate(instances)?;
    for batch in &plan.batches {
        let snapshot = batch.snapshot.map(|rect| {
            let rect = intersect(rect, canvas.bounds()).unwrap_or([0, 0, 0, 0]);
            Snapshot::take(canvas, rect)
        });
        for index in &plan.order[batch.range.clone()] {
            draw_instance(canvas, inputs, &instances[*index], snapshot.as_ref());
        }
    }
    Ok(())
}

/// The instances one at a time in key order, each reading D from the
/// canvas as it stands: the meaning batching must keep (§3.7).
pub fn execute_unbatched(
    canvas: &mut CanvasView,
    instances: &[SpriteInstance],
    inputs: &FrameInputs,
) -> Result<(), InstanceError> {
    check_canvas(canvas, inputs)?;
    inputs.validate(instances)?;
    for index in sort_by_key(instances)? {
        draw_instance(canvas, inputs, &instances[index], None);
    }
    Ok(())
}

fn check_canvas(canvas: &CanvasView, inputs: &FrameInputs) -> Result<(), InstanceError> {
    if [canvas.width, canvas.height] != inputs.uniforms.canvas
        || canvas.pixels.len() != canvas.width as usize * canvas.height as usize
    {
        return Err(InstanceError::TextureLimit(canvas.width, canvas.height));
    }
    Ok(())
}

#[cfg(test)]
#[path = "instanced_tests.rs"]
mod tests;
