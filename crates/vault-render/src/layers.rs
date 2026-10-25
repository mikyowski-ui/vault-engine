//! The `enhanced` preset's dual-layer presentation: the world drawn at an
//! aspect-matched virtual resolution across the window, and the 640x480
//! interface frame drawn over it at its own scale, anchored bottom-center,
//! with side panels extending the interface bar. Engine extension, not part
//! of any specification; the Vanilla presentation never uses it.

use crate::IndexedImage;

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

    /// The layout for `surface`. `ui_scale` `None` fits the frame's height
    /// (fractional); `zoom` is clamped to [`Self::MIN_ZOOM`]-[`Self::MAX_ZOOM`].
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
        let zoom = zoom.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
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
        let per_world = ui_scale * zoom;
        let world_size = [
            (world_rect[2] / per_world).ceil().max(1.0) as u32,
            (world_rect[3] / per_world).ceil().max(1.0) as u32,
        ];
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
    /// The interface frame pixel under `point` (surface pixels), if inside
    /// the frame.
    pub fn ui_pixel(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        let [x, y, w, h] = self.ui_rect.map(f64::from);
        let inside = (x..x + w).contains(&point[0]) && (y..y + h).contains(&point[1]);
        inside.then(|| {
            let scale = f64::from(self.ui_scale);
            [
                ((point[0] - x) / scale).floor() as i32,
                ((point[1] - y) / scale).floor() as i32,
            ]
        })
    }
    /// The world-canvas position under `point` (surface pixels), in
    /// fractional world pixels, if inside the world's area.
    pub fn world_point(&self, point: [f64; 2]) -> Option<[f64; 2]> {
        let [x, y, w, h] = self.world_rect.map(f64::from);
        let inside = (x..x + w).contains(&point[0]) && (y..y + h).contains(&point[1]);
        let per_world = f64::from(self.ui_scale * self.zoom);
        inside.then(|| [(point[0] - x) / per_world, (point[1] - y) / per_world])
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
