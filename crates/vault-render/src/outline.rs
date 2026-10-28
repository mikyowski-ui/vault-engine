//! SPEC-014 §3.44 object outlines: the colors an outline word selects and
//! the one-pixel ring R1 paints around an object's frame in two passes.
use crate::IndexedImage;
use vault_formats::{Palette, frm::FrmFrame, map::outline};

/// A per-color blend table `blend_F[g][D]` (SPEC-002 §3.2): level 0 gives
/// D, levels 1 to 7 the seven-step mix of F over D.
pub type BlendTable = [[u8; 256]; 8];

/// The red translucency color, key 31744 (index 133 in the corpus).
pub const RED_KEY: u16 = 31744;
/// The wall translucency color, key 25439 (index 33 in the corpus).
pub const WALL_KEY: u16 = 25439;
/// The grey outline, key 15855 (index 9 in the corpus).
pub const GREY_KEY: u16 = 15855;
/// The item outline, key 30632 (index 58 in the corpus).
pub const ITEM_KEY: u16 = 30632;
/// Any other nonzero type, key 31775 (index 49 in the corpus).
pub const OTHER_KEY: u16 = 31775;
/// The hostile row bands: the fast-fire cycle 243-247.
const HOSTILE_BANDS: (u8, u8) = (243, 5);
/// The friendly row bands: the slime cycle 229-232.
const FRIENDLY_BANDS: (u8, u8) = (229, 4);

/// The palette index of an RGB555 color key (0 for a palette without the
/// lookup).
pub fn key_index(palette: &Palette, key: u16) -> u8 {
    let key = u32::from(key);
    palette
        .lookup_rgb5([
            (key >> 10) as u8 & 31,
            (key >> 5) as u8 & 31,
            key as u8 & 31,
        ])
        .unwrap_or(0)
}

/// The blend table of the color `key` over `palette`.
pub fn blend_table(palette: &Palette, key: u16) -> Box<BlendTable> {
    let foreground = key_index(palette, key);
    Box::new(std::array::from_fn(|gray| {
        std::array::from_fn(|destination| {
            palette
                .blend_coverage(gray as u8, foreground, destination as u8)
                .unwrap_or(destination as u8)
        })
    }))
}

/// The blend-table row of gray level `gray`. A palette's gray levels are
/// 0 to 7 (SPEC-014 §3.44, over five-bit components), but a hand-built
/// [`OutlineInk`] or gray table may hold more: `[May-improve]` safe bound,
/// a level above 7 uses row 7 instead of indexing past the table.
pub(crate) fn blend_row(gray: u8) -> usize {
    usize::from(gray.min(7))
}

/// How an outline paints each ring pixel.
#[derive(Clone, Copy)]
pub enum OutlineInk<'a> {
    /// One palette index.
    Opaque(u8),
    /// Row bands over `count` cycling indexes from `first` (hostile and
    /// friendly).
    Banded { first: u8, count: u8 },
    /// `blend[gray][D]` over the destination D, with no light.
    Translucent { gray: u8, blend: &'a BlendTable },
}

/// SPEC-014 §3.44 the outline colors of one palette.
pub struct OutlineInks {
    red: u8,
    grey: u8,
    item: u8,
    other: u8,
    red_blend: Box<BlendTable>,
    wall_blend: Box<BlendTable>,
    /// The common gray levels of the red, grey and item indexes.
    gray: [u8; 3],
}

impl OutlineInks {
    pub fn new(palette: &Palette) -> Self {
        let [red, grey, item, other] =
            [RED_KEY, GREY_KEY, ITEM_KEY, OTHER_KEY].map(|key| key_index(palette, key));
        Self {
            red,
            grey,
            item,
            other,
            red_blend: blend_table(palette, RED_KEY),
            wall_blend: blend_table(palette, WALL_KEY),
            gray: [red, grey, item].map(|index| palette.gray_level(index, false)),
        }
    }
    /// The ink of an outline word, or `None` when the word draws no
    /// outline (no type, or disabled). The type is compared whole; the
    /// translucent bit is honored for the red, grey and item types only.
    pub fn ink(&self, word: u32) -> Option<OutlineInk<'_>> {
        if !outline::outlined(word) {
            return None;
        }
        let translucent = word & outline::TRANSLUCENT != 0;
        Some(match outline::kind(word) {
            outline::HOSTILE => OutlineInk::Banded {
                first: HOSTILE_BANDS.0,
                count: HOSTILE_BANDS.1,
            },
            outline::FRIENDLY => OutlineInk::Banded {
                first: FRIENDLY_BANDS.0,
                count: FRIENDLY_BANDS.1,
            },
            outline::RED if translucent => OutlineInk::Translucent {
                gray: self.gray[0],
                blend: &self.red_blend,
            },
            outline::GREY if translucent => OutlineInk::Translucent {
                gray: self.gray[1],
                blend: &self.wall_blend,
            },
            outline::ITEM if translucent => OutlineInk::Translucent {
                gray: self.gray[2],
                blend: &self.red_blend,
            },
            outline::RED => OutlineInk::Opaque(self.red),
            outline::GREY => OutlineInk::Opaque(self.grey),
            outline::ITEM => OutlineInk::Opaque(self.item),
            _ => OutlineInk::Opaque(self.other),
        })
    }
}

/// The paints of R1's two-pass ring of `frame` at `origin`, in order: each
/// target pixel with the frame row that was being scanned when it was
/// painted (the row the bands use).
///
/// - Rows, then columns, each scanned from 0 in the "outside" state: an
///   opaque pixel met outside paints the pixel before it, a transparent
///   one met inside paints itself, and an opaque last pixel paints the
///   one past the end.
/// - A paint happens only when the scanned pixel (the opaque one for the
///   before and past-the-end paints, else the transparent one) lies in
///   `refreshed` (left, top, right, bottom, exclusive) widened by 1 on
///   every side and limited to the `buffer` (width, height).
/// - The buffer guards skip a paint that would land before the buffer
///   start or left of column 0. `[May-improve]`: a paint right of the
///   last column, which R1 lands in column 0 of the next row, and one
///   past the last row are skipped too.
pub fn ring(
    frame: &FrmFrame,
    origin: [i64; 2],
    refreshed: [i64; 4],
    buffer: [i64; 2],
    mut paint: impl FnMut([i64; 2], i64),
) {
    let (width, height) = (i64::from(frame.width), i64::from(frame.height));
    let pixels = frame.pixels();
    let opaque = |x: i64, y: i64| pixels[(y * width + x) as usize] != 0;
    let clip = [
        (refreshed[0] - 1).max(0),
        (refreshed[1] - 1).max(0),
        (refreshed[2] + 1).min(buffer[0]),
        (refreshed[3] + 1).min(buffer[1]),
    ];
    let mut try_paint = |scanned: [i64; 2], target: [i64; 2], row: i64| {
        let [sx, sy] = [origin[0] + scanned[0], origin[1] + scanned[1]];
        let [tx, ty] = [origin[0] + target[0], origin[1] + target[1]];
        if (clip[0]..clip[2]).contains(&sx)
            && (clip[1]..clip[3]).contains(&sy)
            && (0..buffer[0]).contains(&tx)
            && (0..buffer[1]).contains(&ty)
        {
            paint([tx, ty], row);
        }
    };
    for y in 0..height {
        let mut inside = false;
        for x in 0..width {
            match (opaque(x, y), inside) {
                (true, false) => {
                    try_paint([x, y], [x - 1, y], y);
                    inside = true;
                }
                (false, true) => {
                    try_paint([x, y], [x, y], y);
                    inside = false;
                }
                _ => {}
            }
        }
        if width > 0 && opaque(width - 1, y) {
            try_paint([width - 1, y], [width, y], y);
        }
    }
    for x in 0..width {
        let mut inside = false;
        for y in 0..height {
            match (opaque(x, y), inside) {
                (true, false) => {
                    try_paint([x, y], [x, y - 1], y);
                    inside = true;
                }
                (false, true) => {
                    try_paint([x, y], [x, y], y);
                    inside = false;
                }
                _ => {}
            }
        }
        if height > 0 && opaque(x, height - 1) {
            try_paint([x, height - 1], [x, height], height - 1);
        }
    }
}

/// SPEC-014 §3.44 paint `frame`'s outline ring at `origin` over `image` in
/// `ink`, clipped as [`ring`] describes. A pixel painted twice keeps the
/// column pass's paint; a translucent one blends twice. Row bands use
/// `first + ((trunc(row / b) + 1) mod n)` with `b = trunc(height / n)`, or
/// `first` when b is 0.
pub fn draw_outline(
    image: &mut IndexedImage,
    frame: &FrmFrame,
    origin: [i64; 2],
    ink: OutlineInk,
    refreshed: [i64; 4],
    buffer: [i64; 2],
) {
    let stride = i64::from(image.width());
    let buffer = [
        buffer[0].min(stride),
        buffer[1].min(i64::from(image.height())),
    ];
    let pixels = image.pixels_mut();
    ring(frame, origin, refreshed, buffer, |[x, y], row| {
        let pixel = &mut pixels[(y * stride + x) as usize];
        *pixel = match (opaque_paint(ink, frame, row), ink) {
            (Some(index), _) => index,
            (None, OutlineInk::Translucent { gray, blend }) => {
                blend[blend_row(gray)][usize::from(*pixel)]
            }
            (None, _) => *pixel,
        };
    });
}

/// The index an opaque or banded ink paints for a paint scanned on frame
/// row `row` of `frame`; `None` for a translucent ink, which blends.
pub(crate) fn opaque_paint(ink: OutlineInk, frame: &FrmFrame, row: i64) -> Option<u8> {
    match ink {
        OutlineInk::Opaque(index) => Some(index),
        OutlineInk::Banded { first, count } => {
            let band = i64::from(frame.height) / i64::from(count.max(1));
            Some(if band == 0 {
                first
            } else {
                first + ((row / band + 1) % i64::from(count)) as u8
            })
        }
        OutlineInk::Translucent { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_formats::frm::FrmArchive;

    /// A one-frame FRM of `width` by `height` with `pixels`.
    fn frame(width: u16, height: u16, pixels: &[u8]) -> FrmArchive {
        let mut bytes = Vec::new();
        bytes.extend(4_u32.to_be_bytes());
        bytes.extend(10_u16.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend([0_u8; 12]);
        bytes.extend([0_u8; 12]);
        bytes.extend([0_u8; 24]);
        let size = 12 + pixels.len() as u32;
        bytes.extend(size.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend((pixels.len() as u32).to_be_bytes());
        bytes.extend([0_u8; 4]);
        bytes.extend(pixels);
        FrmArchive::parse(&bytes).unwrap()
    }

    /// The paints of a ring drawn at `origin` with nothing clipped.
    fn paints(archive: &FrmArchive, origin: [i64; 2]) -> Vec<[i64; 2]> {
        let mut painted = Vec::new();
        ring(
            archive.frame(0, 0).unwrap(),
            origin,
            [0, 0, 100, 100],
            [100, 100],
            |[x, y], _| painted.push([x - origin[0], y - origin[1]]),
        );
        painted
    }

    /// SPEC-014 Fixture T: the 3-by-2 frame (0, 5, 0) over (5, 5, 5).
    #[test]
    fn spec014_fixture_t_ring_paints_in_order() {
        let archive = frame(3, 2, &[0, 5, 0, 5, 5, 5]);
        assert_eq!(
            paints(&archive, [10, 10]),
            [
                // Rows.
                [0, 0],
                [2, 0],
                [-1, 1],
                [3, 1],
                // Columns.
                [0, 0],
                [0, 2],
                [1, -1],
                [1, 2],
                [2, 0],
                [2, 2],
            ]
        );
        let mut ring_pixels = paints(&archive, [10, 10]);
        ring_pixels.sort();
        ring_pixels.dedup();
        assert_eq!(ring_pixels.len(), 8);
    }

    /// A red ramp: entry i is (2i, 0, 0) for i < 32, and every RGB5 color
    /// looks up the index of its red component. Entry 31, the red key's
    /// index, has common gray level 2, as index 133 has in the corpus.
    fn palette() -> Palette {
        let mut bytes = Vec::with_capacity(0x8300);
        for index in 0..256_u32 {
            let red = if index < 32 { 2 * index } else { 63 } as u8;
            bytes.extend([red, 0, 0]);
        }
        for packed in 0..32768_u32 {
            bytes.push((packed >> 10) as u8);
        }
        Palette::parse(&bytes).unwrap()
    }

    #[test]
    fn spec014_fixture_t_outline_colors() {
        let archive = frame(3, 2, &[0, 5, 0, 5, 5, 5]);
        let inks = OutlineInks::new(&palette());
        let draw = |word: u32, archive: &FrmArchive, fill: u8| {
            let mut image = IndexedImage::from_pixels(20, 20, vec![fill; 400]).unwrap();
            if let Some(ink) = inks.ink(word) {
                let frame = archive.frame(0, 0).unwrap();
                draw_outline(&mut image, frame, [5, 5], ink, [0, 0, 20, 20], [20, 20]);
            }
            image
        };
        let at = |image: &IndexedImage, [x, y]: [i64; 2]| image.pixels()[(y * 20 + x) as usize];
        // Friendly over a 2-row frame: b = 0, every pixel is 229.
        let friendly = draw(outline::FRIENDLY, &archive, 3);
        for pixel in paints(&archive, [5, 5]) {
            assert_eq!(at(&friendly, [pixel[0] + 5, pixel[1] + 5]), 229);
        }
        // Red with bit 30: blend_red[2][D], twice where painted twice.
        let red = key_index(&palette(), RED_KEY);
        let gray = palette().gray_level(red, false);
        assert_eq!((red, gray), (31, 2));
        let blend = blend_table(&palette(), RED_KEY);
        let translucent = draw(outline::RED | outline::TRANSLUCENT, &archive, 3);
        let once = blend[usize::from(gray)][3];
        let twice = blend[usize::from(gray)][usize::from(once)];
        assert_eq!((once, twice), (11, 16));
        for pixel in [[1, -1], [-1, 1], [3, 1], [0, 2], [1, 2], [2, 2]] {
            assert_eq!(at(&translucent, [pixel[0] + 5, pixel[1] + 5]), once);
        }
        for pixel in [[0, 0], [2, 0]] {
            assert_eq!(at(&translucent, [pixel[0] + 5, pixel[1] + 5]), twice);
        }
        // A 10-row hostile frame: b = 2, rows 0-1 244 ... rows 8-9 243.
        let tall = frame(1, 10, &[7; 10]);
        let hostile = draw(outline::HOSTILE, &tall, 3);
        for (row, index) in [244, 244, 245, 245, 246, 246, 247, 247, 243, 243]
            .into_iter()
            .enumerate()
        {
            assert_eq!(at(&hostile, [4, 5 + row as i64]), index, "row {row}");
            assert_eq!(at(&hostile, [6, 5 + row as i64]), index, "row {row}");
        }
        // The pixel above the column's run takes the band of row 0, the
        // one below that of row 9.
        assert_eq!(at(&hostile, [5, 4]), 244);
        assert_eq!(at(&hostile, [5, 15]), 243);
        // Disabled: not drawn. Type 3: the other key, opaque.
        assert!(inks.ink(0x8000_0001).is_none());
        assert!(inks.ink(0).is_none());
        let other = key_index(&palette(), OTHER_KEY);
        assert!(matches!(inks.ink(3), Some(OutlineInk::Opaque(index)) if index == other));
        // The translucent bit is never honored for the banded or other types.
        assert!(matches!(
            inks.ink(outline::HOSTILE | outline::TRANSLUCENT),
            Some(OutlineInk::Banded { first: 243, .. })
        ));
        assert!(matches!(
            inks.ink(3 | outline::TRANSLUCENT),
            Some(OutlineInk::Opaque(_))
        ));
    }

    /// The outline inks' gray levels stay in the blend tables' 8 rows even
    /// for the brightest palette, and a hand-built translucent ink with a
    /// gray level above 7 (only reachable through the public
    /// [`OutlineInk`]) is bounded to row 7 instead of indexing past the
    /// table (`[May-improve]` safe bound).
    #[test]
    fn translucent_outline_gray_levels_stay_in_the_blend_rows() {
        let mut bytes = vec![63_u8; 768];
        for packed in 0..32768_u32 {
            bytes.push((packed % 255 + 1) as u8);
        }
        let white = Palette::parse(&bytes).unwrap();
        let inks = OutlineInks::new(&white);
        assert_eq!(inks.gray, [7; 3]);
        for kind in [outline::RED, outline::GREY, outline::ITEM] {
            let Some(OutlineInk::Translucent { gray, .. }) = inks.ink(kind | outline::TRANSLUCENT)
            else {
                panic!("{kind}: translucent ink");
            };
            assert_eq!(gray, 7);
        }
        let archive = frame(3, 2, &[0, 5, 0, 5, 5, 5]);
        let frame = archive.frame(0, 0).unwrap();
        let blend = blend_table(&palette(), RED_KEY);
        let draw = |gray: u8| {
            let mut image = IndexedImage::from_pixels(20, 20, vec![3; 400]).unwrap();
            let ink = OutlineInk::Translucent {
                gray,
                blend: &blend,
            };
            draw_outline(&mut image, frame, [5, 5], ink, [0, 0, 20, 20], [20, 20]);
            image
        };
        let row7 = draw(7);
        assert_ne!(row7.pixels(), &[3; 400][..], "the ring blends");
        for gray in [8, 15, u8::MAX] {
            assert_eq!(draw(gray).pixels(), row7.pixels(), "gray {gray}");
        }
    }

    /// Clipping: only paints whose scanned pixel lies in the refreshed
    /// rectangle widened by 1, and the buffer guards.
    #[test]
    fn spec014_outline_clipping_and_buffer_guards() {
        let archive = frame(3, 2, &[0, 5, 0, 5, 5, 5]);
        let frame = archive.frame(0, 0).unwrap();
        let collect = |origin: [i64; 2], refreshed: [i64; 4], buffer: [i64; 2]| {
            let mut painted = Vec::new();
            ring(frame, origin, refreshed, buffer, |at, _| painted.push(at));
            painted
        };
        // Refreshed rectangle x 12.., widened to 11..: scanned pixels in
        // column 10 trigger nothing. The row-1 left paint (scanned (10,
        // 11)) is dropped, and the column-0 paints too; a ring pixel lands
        // 2 outside the rectangle (column 10, scanned in column 11).
        let painted = collect([10, 10], [12, 0, 100, 100], [100, 100]);
        assert!(!painted.contains(&[9, 11]));
        assert!(!painted.contains(&[10, 12]));
        assert!(painted.contains(&[10, 10]), "{painted:?}");
        // Frame at the buffer's left edge: the left paint of row 1 is
        // skipped (column -1); at the top, the above paint is skipped.
        let painted = collect([0, 0], [0, 0, 100, 100], [100, 100]);
        assert!(painted.iter().all(|[x, y]| *x >= 0 && *y >= 0));
        assert!(!painted.contains(&[1, -1]));
        // [May-improve] a right paint past the last column is skipped
        // rather than wrapped into the next row.
        let painted = collect([7, 0], [0, 0, 10, 10], [10, 10]);
        assert!(!painted.contains(&[10, 1]));
        assert!(!painted.contains(&[0, 2]));
    }
}
