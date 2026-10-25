//! The `enhanced` preset's dual-layer presentation: the world drawn at an
//! aspect-matched virtual resolution across the window, and the 640x480
//! interface frame drawn over it at its own scale, anchored bottom-center,
//! with side panels extending the interface bar. Engine extension, not part
//! of any specification; the Vanilla presentation never uses it.

use crate::{ImageError, IndexedImage};

/// One indexed image drawn into a surface rectangle through the shared
/// palette.
#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub image: &'a IndexedImage,
    /// `[x, y, width, height]` in surface pixels.
    pub rect: [f32; 4],
    /// A palette index drawn as fully transparent, if any.
    pub transparent: Option<u8>,
    /// Sharp-bilinear sampling at fractional scales (else nearest).
    pub sharp: bool,
}

impl Layer<'_> {
    /// The layer shader's parameters for this layer on a `surface`-sized
    /// window.
    pub fn params(&self, surface: [u32; 2]) -> LayerParams {
        LayerParams::new(
            [self.image.width(), self.image.height()],
            self.rect,
            self.transparent,
            self.sharp,
            surface,
        )
    }
}

/// The layer shader's uniform block (`Params` in `layers.wgsl`): the
/// rectangle `[x, y, w, h]`, the surface size, the scale per axis
/// (rectangle size over image size), 1.0 when the layer is drawn
/// sharp-bilinear (else 0.0), the transparent index (−1.0 for none), and
/// padding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerParams(pub [f32; 12]);

impl LayerParams {
    /// The parameters of an `image`-sized layer. Sharp-bilinear applies
    /// only when `sharp` is asked for and the scale is fractional on some
    /// axis; a whole scale draws exactly the nearest texels, as the
    /// single-image path does.
    pub fn new(
        image: [u32; 2],
        rect: [f32; 4],
        transparent: Option<u8>,
        sharp: bool,
        surface: [u32; 2],
    ) -> Self {
        let scale = [
            rect[2] / image[0].max(1) as f32,
            rect[3] / image[1].max(1) as f32,
        ];
        let whole = scale
            .iter()
            .all(|scale| *scale >= 1.0 && scale.fract() == 0.0);
        let sharp = sharp && !whole;
        Self([
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            surface[0].max(1) as f32,
            surface[1].max(1) as f32,
            scale[0],
            scale[1],
            if sharp { 1.0 } else { 0.0 },
            transparent.map_or(-1.0, f32::from),
            0.0,
            0.0,
        ])
    }
    /// Whether the layer is drawn sharp-bilinear.
    pub fn sharp(self) -> bool {
        self.0[8] != 0.0
    }
    /// The index drawn transparent.
    pub fn transparent(self) -> Option<u8> {
        (self.0[9] >= 0.0).then_some(self.0[9] as u8)
    }
}

/// The surface pixels a layer rectangle covers along one axis: those whose
/// centers lie in `[start, start + length)`, as the rasterizer fills them,
/// clipped to `0..limit`.
fn covered(start: f64, length: f64, limit: u32) -> std::ops::Range<u32> {
    let first = (start - 0.5).ceil().max(0.0);
    let end = (start + length - 0.5).ceil().clamp(0.0, f64::from(limit));
    (first.min(end) as u32)..end as u32
}

/// The layers composed on the CPU with nearest texels, back to front, over
/// `background`: each covered surface pixel shows the texel under its
/// center unless that texel is the layer's transparent index. The headless
/// counterpart of the GPU pass (which differs only where sharp-bilinear
/// blends across texel edges, and by float rounding exactly on an edge),
/// for replays and tests without a GPU.
pub fn compose_layers(
    layers: &[Layer<'_>],
    surface: [u32; 2],
    background: u8,
) -> Result<IndexedImage, ImageError> {
    let mut image = IndexedImage::new(surface[0], surface[1])?;
    image.pixels_mut().fill(background);
    let width = surface[0] as usize;
    for layer in layers {
        let [x, y, w, h] = layer.rect.map(f64::from);
        if !(w > 0.0 && h > 0.0) {
            continue;
        }
        let (source, size) = (
            layer.image.pixels(),
            [layer.image.width(), layer.image.height()],
        );
        let texel = |pixel: u32, start: f64, length: f64, size: u32| {
            let u = (f64::from(pixel) + 0.5 - start) / length;
            ((u * f64::from(size)).floor() as i64).clamp(0, i64::from(size) - 1) as usize
        };
        let columns: Vec<(usize, usize)> = covered(x, w, surface[0])
            .map(|px| (px as usize, texel(px, x, w, size[0])))
            .collect();
        for py in covered(y, h, surface[1]) {
            let row = texel(py, y, h, size[1]) * size[0] as usize;
            let target = &mut image.pixels_mut()[py as usize * width..][..width];
            for (px, tx) in &columns {
                let index = source[row + tx];
                if Some(index) != layer.transparent {
                    target[*px] = index;
                }
            }
        }
    }
    Ok(image)
}

/// Where the layers go on a surface, and how pointer positions map back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnhancedLayout {
    pub surface: [u32; 2],
    /// Surface pixels per interface pixel.
    pub ui_scale: f32,
    /// The world's zoom: surface pixels per world pixel are
    /// `ui_scale * zoom`.
    pub zoom: f32,
    /// The 640x480 interface frame, bottom-centered: `[x, y, w, h]`.
    pub ui_rect: [f32; 4],
    /// The interface bar, the frame's bottom [`Self::BAR_HEIGHT`] rows.
    pub bar_rect: [f32; 4],
    /// The side panels left and right of the bar (zero width when the
    /// frame spans the surface).
    pub side_rects: [[f32; 4]; 2],
    /// The world's area: the full width, from the top to the bar.
    pub world_rect: [f32; 4],
    /// The world canvas in world pixels: `world_rect / (ui_scale * zoom)`,
    /// rounded up.
    pub world_size: [u32; 2],
}

impl EnhancedLayout {
    pub const UI_FRAME: [u32; 2] = [640, 480];
    pub const BAR_HEIGHT: u32 = 100;
    pub const MIN_ZOOM: f32 = 0.5;
    pub const MAX_ZOOM: f32 = 3.0;
    /// The world canvas's largest side, the largest [`IndexedImage`].
    pub const MAX_WORLD: u32 = 4096;

    /// The layout for `surface`. `ui_scale` `None` fits the frame's height
    /// (fractional); `zoom` is clamped to [`Self::MIN_ZOOM`]-[`Self::MAX_ZOOM`]
    /// (a NaN zoom is 1), and raised further when the world canvas would
    /// otherwise exceed [`Self::MAX_WORLD`] on a side (a very large window
    /// zoomed far out); the `zoom` field is the zoom in effect.
    pub fn compute(surface: [u32; 2], ui_scale: Option<f32>, zoom: f32) -> Option<Self> {
        let [width, height] = surface.map(|value| value as f32);
        if width < 1.0 || height < 1.0 {
            return None;
        }
        let [frame_w, frame_h] = Self::UI_FRAME.map(|value| value as f32);
        let fit = (width / frame_w).min(height / frame_h);
        let ui_scale = ui_scale
            .filter(|scale| *scale > 0.0)
            .unwrap_or(fit)
            .min(fit);
        let zoom = if zoom.is_nan() {
            1.0
        } else {
            zoom.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM)
        };
        let (ui_w, ui_h) = (frame_w * ui_scale, frame_h * ui_scale);
        let ui_rect = [((width - ui_w) / 2.0).floor(), height - ui_h, ui_w, ui_h];
        let bar_h = Self::BAR_HEIGHT as f32 * ui_scale;
        let bar_rect = [ui_rect[0], height - bar_h, ui_w, bar_h];
        let side_rects = [
            [0.0, height - bar_h, ui_rect[0], bar_h],
            [
                ui_rect[0] + ui_w,
                height - bar_h,
                (width - ui_rect[0] - ui_w).max(0.0),
                bar_h,
            ],
        ];
        let world_rect = [0.0, 0.0, width, (height - bar_h).max(1.0)];
        let zoom = zoom.max(Self::canvas_floor(world_rect, ui_scale));
        // Divided in f64, as `world_point` maps: an f32 quotient can round
        // down to a whole number the pointer mapping then passes.
        let per_world = f64::from(ui_scale * zoom);
        let canvas =
            |side: f32| ((f64::from(side) / per_world).ceil().max(1.0) as u32).min(Self::MAX_WORLD);
        let world_size = [canvas(world_rect[2]), canvas(world_rect[3])];
        Some(Self {
            surface,
            ui_scale,
            zoom,
            ui_rect,
            bar_rect,
            side_rects,
            world_rect,
            world_size,
        })
    }
    /// The least zoom that keeps a `world_rect` canvas within
    /// [`Self::MAX_WORLD`] at `ui_scale`.
    fn canvas_floor(world_rect: [f32; 4], ui_scale: f32) -> f32 {
        world_rect[2].max(world_rect[3]) / (Self::MAX_WORLD as f32 * ui_scale)
    }
    /// The least zoom this surface and interface scale can show:
    /// [`Self::MIN_ZOOM`], or more when the canvas would exceed
    /// [`Self::MAX_WORLD`]. A zoom the layout raised is raised to this; the
    /// `zoom` field is no floor (it is only the zoom asked for, often
    /// mid-animation).
    pub fn min_zoom(&self) -> f32 {
        Self::MIN_ZOOM.max(Self::canvas_floor(self.world_rect, self.ui_scale))
    }
    /// The interface frame pixel under `point` (surface pixels), if inside
    /// the frame.
    pub fn ui_pixel(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        let [x, y, w, h] = self.ui_rect.map(f64::from);
        let inside = (x..x + w).contains(&point[0]) && (y..y + h).contains(&point[1]);
        inside.then(|| {
            let scale = f64::from(self.ui_scale);
            // Rounding just inside the far edge never reaches past it.
            let pixel = |offset: f64, size: u32| {
                ((offset / scale).floor() as i32).clamp(0, size as i32 - 1)
            };
            [
                pixel(point[0] - x, Self::UI_FRAME[0]),
                pixel(point[1] - y, Self::UI_FRAME[1]),
            ]
        })
    }
    /// The world-canvas position under `point` (surface pixels), in
    /// fractional world pixels, if inside the world's area; always on the
    /// canvas (below `world_size`).
    pub fn world_point(&self, point: [f64; 2]) -> Option<[f64; 2]> {
        let [x, y, w, h] = self.world_rect.map(f64::from);
        let inside = (x..x + w).contains(&point[0]) && (y..y + h).contains(&point[1]);
        let per_world = f64::from(self.ui_scale * self.zoom);
        let on_canvas =
            |offset: f64, size: u32| (offset / per_world).min(f64::from(size).next_down());
        inside.then(|| {
            [
                on_canvas(point[0] - x, self.world_size[0]),
                on_canvas(point[1] - y, self.world_size[1]),
            ]
        })
    }
    /// The surface point at world-canvas position `world`: the inverse of
    /// [`Self::world_point`] (not limited to the canvas).
    pub fn surface_point(&self, world: [f64; 2]) -> [f64; 2] {
        let per_world = f64::from(self.ui_scale * self.zoom);
        [0, 1].map(|axis| f64::from(self.world_rect[axis]) + world[axis] * per_world)
    }
    /// Whether `point` is on a side panel.
    pub fn in_side_panel(&self, point: [f64; 2]) -> bool {
        self.side_rects.iter().any(|rect| {
            let [x, y, w, h] = rect.map(f64::from);
            (x..x + w).contains(&point[0]) && (y..y + h).contains(&point[1])
        })
    }
    /// The world canvas's surface rectangle (it may overhang the world's
    /// area by up to one world pixel, from the rounding up).
    pub fn world_layer_rect(&self) -> [f32; 4] {
        let per_world = self.ui_scale * self.zoom;
        [
            self.world_rect[0],
            self.world_rect[1],
            self.world_size[0] as f32 * per_world,
            self.world_size[1] as f32 * per_world,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACES: [[u32; 2]; 7] = [
        [640, 480],
        [1280, 720],
        [1366, 768],
        [1920, 1080],
        [2560, 1080],
        [1280, 800],
        [3840, 2160],
    ];
    const ZOOMS: [f32; 4] = [0.5, 1.0, 2.0, 3.0];
    const UI_SCALES: [Option<f32>; 4] = [None, Some(1.0), Some(1.5), Some(2.0)];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-3 * b.abs().max(1.0)
    }

    #[test]
    fn every_layout_anchors_the_frame_bottom_center_with_the_bar_and_side_panels() {
        for surface in SURFACES {
            let [width, height] = surface.map(|value| value as f32);
            let fit = (width / 640.0).min(height / 480.0);
            for ui_scale in UI_SCALES {
                for zoom in ZOOMS {
                    let layout = EnhancedLayout::compute(surface, ui_scale, zoom).unwrap();
                    let label = format!("{surface:?} {ui_scale:?} {zoom}");
                    let scale = ui_scale.unwrap_or(fit).min(fit);
                    assert_eq!(layout.ui_scale, scale, "{label}");
                    assert_eq!(layout.surface, surface);
                    // The frame: 640x480 at the scale, on the bottom edge,
                    // centered (its left margin rounded down).
                    let [x, y, w, h] = layout.ui_rect;
                    assert!(
                        close(w, 640.0 * scale) && close(h, 480.0 * scale),
                        "{label}"
                    );
                    assert!(close(y + h, height), "{label}");
                    assert_eq!(x, ((width - w) / 2.0).floor(), "{label}");
                    assert!(x >= 0.0 && x + w <= width + 1e-3, "{label}");
                    let right = width - x - w;
                    assert!((right - x).abs() < 2.0, "{label}: centered");
                    // The bar: the frame's bottom 100 rows.
                    let bar = layout.bar_rect;
                    assert!(close(bar[3], 100.0 * scale), "{label}");
                    assert_eq!([bar[0], bar[2]], [x, w], "{label}");
                    assert!(close(bar[1], y + 380.0 * scale), "{label}");
                    assert!(close(bar[1] + bar[3], height), "{label}");
                    // The side panels fill the bar's row to the edges.
                    let [left, right_rect] = layout.side_rects;
                    assert_eq!([left[0], left[2]], [0.0, x], "{label}");
                    assert!(close(right_rect[0], x + w), "{label}");
                    assert!(close(left[2] + w + right_rect[2], width), "{label}");
                    for side in [left, right_rect] {
                        assert_eq!([side[1], side[3]], [bar[1], bar[3]], "{label}");
                        assert!(side[2] >= 0.0);
                    }
                    // The world: full width, down to the bar, its canvas
                    // the area over the world scale rounded up.
                    let world = layout.world_rect;
                    assert_eq!([world[0], world[1], world[2]], [0.0, 0.0, width]);
                    assert!(close(world[3], height - bar[3]), "{label}");
                    // The zoom, raised only when the canvas would pass
                    // the largest image.
                    let floor = world[2].max(world[3]) / (4096.0 * scale);
                    assert_eq!(layout.zoom, zoom.max(floor), "{label}");
                    assert!(layout.world_size.iter().all(|side| *side <= 4096));
                    let per_world = scale * layout.zoom;
                    for axis in 0..2 {
                        let exact = world[2 + axis] / per_world;
                        let size = layout.world_size[axis] as f32;
                        assert!(size >= exact - 1e-3 && size < exact + 1.0 + 1e-3, "{label}");
                    }
                    let canvas = layout.world_layer_rect();
                    assert!(canvas[2] >= world[2] - 1e-2 && canvas[3] >= world[3] - 1e-2);
                    assert!(canvas[2] < world[2] + per_world + 1e-2, "{label}");
                }
            }
        }
    }

    #[test]
    fn known_layouts() {
        let layout = |surface, scale, zoom| EnhancedLayout::compute(surface, scale, zoom).unwrap();
        // 640x480 is the Vanilla frame: no side panels, a 640x380 world.
        let vanilla = layout([640, 480], None, 1.0);
        assert_eq!(vanilla.ui_rect, [0.0, 0.0, 640.0, 480.0]);
        assert_eq!(vanilla.bar_rect, [0.0, 380.0, 640.0, 100.0]);
        assert_eq!(
            vanilla.side_rects,
            [[0.0, 380.0, 0.0, 100.0], [640.0, 380.0, 0.0, 100.0]]
        );
        assert_eq!(vanilla.world_size, [640, 380]);
        // A larger interface scale than fits is the fit.
        assert_eq!(layout([640, 480], Some(2.0), 1.0), vanilla);
        // 1920x1080 fits the frame's height at 2.25.
        let full_hd = layout([1920, 1080], None, 1.0);
        assert_eq!(full_hd.ui_rect, [240.0, 0.0, 1440.0, 1080.0]);
        assert_eq!(full_hd.bar_rect, [240.0, 855.0, 1440.0, 225.0]);
        assert_eq!(
            full_hd.side_rects,
            [[0.0, 855.0, 240.0, 225.0], [1680.0, 855.0, 240.0, 225.0]]
        );
        assert_eq!(full_hd.world_rect, [0.0, 0.0, 1920.0, 855.0]);
        assert_eq!(full_hd.world_size, [854, 380]);
        assert_eq!(layout([1920, 1080], None, 3.0).world_size, [285, 127]);
        // 21:9 widens the side panels; the world stays 380 rows at 1x.
        let wide = layout([2560, 1080], None, 1.0);
        assert_eq!(wide.ui_rect[0], 560.0);
        assert_eq!(wide.side_rects[1][2], 560.0);
        assert_eq!(wide.world_size, [1138, 380]);
        // 3840x2160 at interface scale 2: a 1280x960 frame, and at zoom
        // 0.5 one world pixel per surface pixel.
        let uhd = layout([3840, 2160], Some(2.0), 0.5);
        assert_eq!(uhd.ui_rect, [1280.0, 1200.0, 1280.0, 960.0]);
        assert_eq!(uhd.world_size, [3840, 1960]);
        // 1280x800 (16:10) fits 5/3: a fractional frame, its margin
        // rounded down.
        let tall = layout([1280, 800], None, 1.0);
        assert_eq!(tall.ui_rect[0], 106.0);
        assert!(close(tall.ui_rect[2], 3200.0 / 3.0));
    }

    #[test]
    fn interface_pixels_round_trip_to_the_frame_edges() {
        for surface in SURFACES {
            for ui_scale in UI_SCALES {
                let layout = EnhancedLayout::compute(surface, ui_scale, 1.0).unwrap();
                let [x, y, w, h] = layout.ui_rect.map(f64::from);
                let scale = f64::from(layout.ui_scale);
                let label = format!("{surface:?} {ui_scale:?}");
                // Each frame pixel's center maps back to it.
                for pixel in [
                    [0, 0],
                    [639, 479],
                    [320, 240],
                    [0, 479],
                    [639, 0],
                    [17, 401],
                ] {
                    let center = [
                        x + (f64::from(pixel[0]) + 0.5) * scale,
                        y + (f64::from(pixel[1]) + 0.5) * scale,
                    ];
                    assert_eq!(layout.ui_pixel(center), Some(pixel), "{label}");
                }
                // The edges: the corner pixel itself, the last surface
                // position before the far edge, and just outside.
                assert_eq!(layout.ui_pixel([x, y]), Some([0, 0]), "{label}");
                let inner = [x + w - 1e-9, y + h - 1e-9];
                assert_eq!(layout.ui_pixel(inner), Some([639, 479]), "{label}");
                assert_eq!(layout.ui_pixel([x + w, y]), None, "{label}");
                assert_eq!(layout.ui_pixel([x, y + h]), None, "{label}");
                assert_eq!(layout.ui_pixel([x - 0.01, y]), None, "{label}");
                assert_eq!(layout.ui_pixel([x, y - 0.01]), None, "{label}");
                assert_eq!(layout.ui_pixel([f64::NAN, y]), None, "{label}");
            }
        }
    }

    #[test]
    fn world_points_round_trip_to_the_canvas_edges() {
        for surface in SURFACES {
            for ui_scale in UI_SCALES {
                for zoom in ZOOMS {
                    let layout = EnhancedLayout::compute(surface, ui_scale, zoom).unwrap();
                    let label = format!("{surface:?} {ui_scale:?} {zoom}");
                    let per_world = f64::from(layout.ui_scale * layout.zoom);
                    let [_, _, w, h] = layout.world_rect.map(f64::from);
                    let [cw, ch] = layout.world_size.map(f64::from);
                    // World pixel centers inside the area map back to them.
                    for pixel in [
                        [0.0, 0.0],
                        [cw - 1.0, 0.0],
                        [3.0, 7.0],
                        [(cw / 2.0).floor(), (ch / 2.0).floor()],
                    ] {
                        let surface_point =
                            [(pixel[0] + 0.5) * per_world, (pixel[1] + 0.5) * per_world];
                        if surface_point[0] >= w || surface_point[1] >= h {
                            continue;
                        }
                        let back = layout.world_point(surface_point).unwrap();
                        assert_eq!(back.map(f64::floor), pixel.map(f64::floor), "{label}");
                    }
                    assert_eq!(layout.world_point([0.0, 0.0]), Some([0.0, 0.0]));
                    // The last position in the area is on the canvas.
                    let last = layout.world_point([w - 1e-6, h - 1e-6]).unwrap();
                    assert!(last[0] < cw && last[1] < ch, "{label}: {last:?}");
                    assert_eq!(layout.world_point([w, 0.0]), None, "{label}");
                    assert_eq!(layout.world_point([0.0, h]), None, "{label}");
                    assert_eq!(layout.world_point([-0.01, 0.0]), None, "{label}");
                    // The bar's row is the interface's, not the world's.
                    let bar = layout.bar_rect.map(f64::from);
                    assert_eq!(layout.world_point([bar[0] + 1.0, bar[1] + 1.0]), None);
                    let side = layout.side_rects[0].map(f64::from);
                    if side[2] > 0.0 {
                        assert!(layout.in_side_panel([side[0], side[1]]), "{label}");
                        assert!(layout.ui_pixel([side[0], side[1]]).is_none(), "{label}");
                    }
                    assert!(!layout.in_side_panel([0.0, 0.0]));
                }
            }
        }
    }

    #[test]
    fn degenerate_inputs_have_safe_layouts() {
        assert_eq!(EnhancedLayout::compute([0, 480], None, 1.0), None);
        assert_eq!(EnhancedLayout::compute([640, 0], None, 1.0), None);
        // A NaN zoom is 1; out-of-range zooms are clamped; a zero, negative
        // or NaN interface scale fits.
        let base = EnhancedLayout::compute([1920, 1080], None, 1.0).unwrap();
        assert_eq!(
            EnhancedLayout::compute([1920, 1080], None, f32::NAN),
            Some(base)
        );
        assert_eq!(
            EnhancedLayout::compute([1920, 1080], None, 9.0)
                .unwrap()
                .zoom,
            3.0
        );
        assert_eq!(
            EnhancedLayout::compute([1920, 1080], None, 0.1)
                .unwrap()
                .zoom,
            0.5
        );
        for scale in [0.0, -1.0, f32::NAN] {
            assert_eq!(
                EnhancedLayout::compute([1920, 1080], Some(scale), 1.0),
                Some(base)
            );
        }
        // A window smaller than the frame scales the frame down.
        let small = EnhancedLayout::compute([320, 240], None, 1.0).unwrap();
        assert_eq!(small.ui_scale, 0.5);
        assert_eq!(small.world_size, [640, 380]);
        // The canvas never exceeds the largest image: 3840x2160 at
        // interface scale 1 and zoom 0.5 would be 7680 wide, so the zoom
        // is raised to 0.9375.
        let huge = EnhancedLayout::compute([3840, 2160], Some(1.0), 0.5).unwrap();
        assert!(
            huge.world_size
                .iter()
                .all(|side| *side <= EnhancedLayout::MAX_WORLD)
        );
        assert_eq!(huge.zoom, 0.9375);
        assert_eq!(huge.world_size, [4096, 2198]);
        assert!(IndexedImage::new(huge.world_size[0], huge.world_size[1]).is_ok());
    }

    #[test]
    fn layer_params_select_sharp_bilinear_only_at_fractional_scales() {
        let image = IndexedImage::new(640, 480).unwrap();
        let layer = |rect, sharp| Layer {
            image: &image,
            rect,
            transparent: None,
            sharp,
        };
        let whole = layer([240.0, 0.0, 1280.0, 960.0], true).params([1920, 1080]);
        assert!(!whole.sharp());
        assert_eq!(
            whole.0,
            [
                240.0, 0.0, 1280.0, 960.0, 1920.0, 1080.0, 2.0, 2.0, 0.0, -1.0, 0.0, 0.0
            ]
        );
        assert!(
            layer([240.0, 0.0, 1440.0, 1080.0], true)
                .params([1920, 1080])
                .sharp()
        );
        assert!(
            !layer([240.0, 0.0, 1440.0, 1080.0], false)
                .params([1920, 1080])
                .sharp()
        );
        // A downscale is fractional.
        assert!(
            layer([0.0, 0.0, 320.0, 240.0], true)
                .params([320, 240])
                .sharp()
        );
        let keyed = Layer {
            transparent: Some(0),
            ..layer([0.0, 0.0, 640.0, 480.0], true)
        };
        assert_eq!(keyed.params([640, 480]).transparent(), Some(0));
        assert_eq!(layer([0.0; 4], true).params([640, 480]).transparent(), None);
        let top = Layer {
            transparent: Some(255),
            ..keyed
        };
        assert_eq!(top.params([640, 480]).transparent(), Some(255));
    }

    #[test]
    fn the_layer_shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("layers.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        // The uniform block matches `LayerParams`.
        assert_eq!(std::mem::size_of::<LayerParams>(), 48);
        let params = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Params"))
            .map(|(_, ty)| ty.inner.size(module.to_ctx()))
            .unwrap();
        assert_eq!(params, 48);
    }

    #[test]
    fn layers_compose_back_to_front_with_their_transparent_index() {
        // The world: 4x2 at scale 2 over the whole 8x4 surface; the
        // interface: 2x2 with index 0 transparent, at scale 1 bottom-right.
        let world = IndexedImage::from_pixels(4, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let ui = IndexedImage::from_pixels(2, 2, vec![0, 9, 9, 0]).unwrap();
        let layers = [
            Layer {
                image: &world,
                rect: [0.0, 0.0, 8.0, 4.0],
                transparent: None,
                sharp: false,
            },
            Layer {
                image: &ui,
                rect: [6.0, 2.0, 2.0, 2.0],
                transparent: Some(0),
                sharp: true,
            },
        ];
        let image = compose_layers(&layers, [8, 4], 0).unwrap();
        assert_eq!(
            image.pixels(),
            [
                1, 1, 2, 2, 3, 3, 4, 4, //
                1, 1, 2, 2, 3, 3, 4, 4, //
                5, 5, 6, 6, 7, 7, 8, 9, //
                5, 5, 6, 6, 7, 7, 9, 8,
            ]
        );
        // An overhanging, fractional rectangle is clipped to the surface,
        // and pixels no layer covers keep the background.
        let shifted = [Layer {
            rect: [-1.0, 1.5, 8.0, 4.0],
            ..layers[0]
        }];
        let image = compose_layers(&shifted, [8, 4], 77).unwrap();
        assert_eq!(&image.pixels()[..8], [77; 8]);
        assert_eq!(&image.pixels()[8..16], [1, 2, 2, 3, 3, 4, 4, 77]);
        assert_eq!(&image.pixels()[24..32], [5, 6, 6, 7, 7, 8, 8, 77]);
        assert!(compose_layers(&layers, [0, 4], 0).is_err());
    }

    #[test]
    fn covered_pixels_follow_the_pixel_center_rule() {
        assert_eq!(covered(0.0, 8.0, 8), 0..8);
        assert_eq!(covered(-1.0, 8.0, 8), 0..7);
        assert_eq!(covered(1.5, 4.0, 8), 1..5);
        assert_eq!(covered(0.4, 0.2, 8), 0..1);
        assert_eq!(covered(0.6, 0.2, 8), 1..1);
        assert_eq!(covered(10.0, 4.0, 8), 8..8);
        assert_eq!(covered(-9.0, 4.0, 8), 0..0);
    }
}
