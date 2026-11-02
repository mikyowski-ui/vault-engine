use super::{
    MapArt, MapView,
    lighting::TRANSLUCENCY,
    tiles::{beyond, corner, square_order},
};
use vault_formats::{MapFile, Palette, frm::FrmFrame, map::MapObject, pro::Category};

/// The `enhanced` preset's roof peek: roof pixels within `inner` view
/// pixels of `center` are cut away, and within the `ring` beyond it they
/// dissolve through a 4x4 ordered (Bayer) dither anchored to world pixels
/// ([`RoofCutaway::keeps`]). Only roofs are cut: what the scene drew
/// beneath shows through, and the cutout, translucency and outlines are
/// unchanged. Engine extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoofCutaway {
    /// The center in view pixels.
    pub center: [i64; 2],
    pub inner: u32,
    pub ring: u32,
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

/// The basic wall-transparency end marker.
pub(super) const TRANSPARENCY_END: u32 = 0x1000_0000;
/// The orientation constant, `0x3FF5555555555556` (not the nearest double
/// to 4/3).
pub(super) const K: f64 = f64::from_bits(0x3FF5_5555_5555_5556);

impl Cutout<'_> {
    /// The mask's top-left corner in the view: the player's hex center plus
    /// its displacement, less (64, 72).
    pub(super) fn origin(&self, view: MapView) -> Option<[i64; 2]> {
        let [x, y] = view.grid.hex_center(self.hex).ok()?;
        Some([
            x + i64::from(self.displacement[0]) - 64,
            y + i64::from(self.displacement[1]) - 72,
        ])
    }
    /// The mask weight at a view pixel, or `None` outside the mask.
    pub(super) fn weight(&self, origin: [i64; 2], [x, y]: [i64; 2]) -> Option<u8> {
        let (u, v) = (x - origin[0], y - origin[1]);
        let (width, height) = (i64::from(self.mask.width), i64::from(self.mask.height));
        ((0..width).contains(&u) && (0..height).contains(&v))
            .then(|| self.mask.pixels()[(v * width + u) as usize])
    }
    /// The mask's rectangle `[x0, y0, x1, y1]` with its top-left at
    /// `origin`.
    pub(super) fn rect(&self, origin: [i64; 2]) -> [i64; 4] {
        [
            origin[0],
            origin[1],
            origin[0] + i64::from(self.mask.width),
            origin[1] + i64::from(self.mask.height),
        ]
    }
    /// The §3.37 orientation table for a wall or scenery object, with or
    /// without the wall-transparency-end exceptions.
    pub(super) fn oriented(&self, view: MapView, object: &MapObject, ends: bool) -> bool {
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
    pub(super) fn cuts(&self, view: MapView, object: &MapObject) -> bool {
        matches!((object.art_id >> 24) & 0x0f, 2 | 3)
            && !self.hidden
            && object.flags.0 & TRANSLUCENCY == 0
            && self.oriented(view, object, true)
    }
}

/// The roof squares a refresh draws, in drawing order: each one's rank
/// (its position in [`square_order`]), art and corner (96 pixels above
/// the floor corner). Roof index 1, hidden roofs, squares past the view
/// and squares without art are skipped (SPEC-014 §3.36).
pub(super) fn roof_squares<'a, A: MapArt>(
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
            let corner = [corner[0], corner[1] - 96];
            if beyond(view, corner) {
                return None;
            }
            let tile = map
                .tile(view.elevation as usize, *index as usize)
                .expect("validated map grid");
            if tile.roof() == 1 || tile.roof_hidden() {
                return None;
            }
            let (frame, _) = art.frame(Category::Tile, u32::from(tile.roof()), 0, 0)?;
            Some((rank as u32, frame, corner))
        })
}

/// SPEC-022 §3.5 an item split by the player cutout: its parts (0 to 4),
/// each with its rectangle and whether it is drawn through the mask. With
/// no mask, or no overlap between the mask and the item's rectangle
/// clipped to `refresh`, the item is one part 0 over its whole rectangle,
/// not through the mask.
pub(crate) fn cutout_parts(
    item: [i64; 4],
    refresh: [i64; 4],
    mask: Option<[i64; 4]>,
) -> Vec<(u8, [i64; 4], bool)> {
    let overlap = |a: [i64; 4], b: [i64; 4]| {
        let rect = [
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].min(b[2]),
            a[3].min(b[3]),
        ];
        (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
    };
    let Some((clipped, mask, inner)) = mask.and_then(|mask| {
        let clipped = overlap(item, refresh)?;
        Some((clipped, mask, overlap(clipped, mask)?))
    }) else {
        return vec![(0, item, false)];
    };
    let mut parts = vec![(0, inner, true)];
    let candidates = [
        (1, [clipped[0], clipped[1], clipped[2], mask[1]]),
        (2, [clipped[0], mask[3], clipped[2], clipped[3]]),
        (3, [clipped[0], inner[1], mask[0], inner[3]]),
        (4, [mask[2], inner[1], clipped[2], inner[3]]),
    ];
    for (part, rect) in candidates {
        if let Some(rect) = overlap(rect, clipped) {
            parts.push((part, rect, false));
        }
    }
    parts
}
