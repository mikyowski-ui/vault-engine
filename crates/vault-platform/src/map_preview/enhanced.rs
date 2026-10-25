//! The interactive world's `enhanced` preset presentation: the camera, the
//! dual-layer frame and the pointer in world and interface coordinates.
//! Engine extension, not part of any specification. With the Vanilla
//! preset (`set_enhanced(None)`, the default) every hook answers
//! `None`/`false` and nothing here runs, so the Vanilla path is unchanged;
//! the fixed viewport (`ViewportMode::Fixed`) keeps the Vanilla frame and
//! input too.
//!
//! In the unlocked viewport the world is drawn on its own canvas at the
//! layout's `world_size`, centered on the [`Camera`] (presentation state
//! only), and the 640-by-480 interface frame is drawn over it:
//! - **Canvas.** The canvas is drawn in 640-by-380 tiles, each one ordinary
//!   map refresh (SPEC-014 §3.44: a refresh draws the objects of a lattice
//!   of hexes around its rectangle, sized for the map window), and pointer
//!   picking uses the tile that drew the pixel, so what shows is what is
//!   picked.
//! - **Pointer.** A pointer over the world area is a canvas pixel
//!   (`PointerState::world`), which is map area at any size; the bar rows,
//!   windows and modal boxes take interface pixels through the Vanilla
//!   handlers. An action menu opened on the canvas stays there, placed in
//!   interface pixels relative to the pointer (`menu_point`).
//! - **Camera hex.** The world's `camera_hex` is kept at the hex nearest the
//!   camera center whenever it changes, driven only by logged input and
//!   tick times, so scripts that read the camera stay sensible and replays
//!   stay deterministic.
//! - **Roof peek.** The cursor X-ray cuts roofs away within 96 world pixels
//!   of the pointer, dissolving over a further 48, both divided by the zoom
//!   so the hole keeps one size on screen (96 and 48 interface pixels).
use super::{
    MapPreview, Screen,
    camera::{self, Camera, TickInput},
    pointer::{Edge, map_bottom},
};
use crate::AppResult;
use crate::enhanced::{
    EnhancedFrame, EnhancedSettings, PointerAt, PointerButton, RoofPeek, ViewportMode,
};
use crate::interface::Painter;
use std::{collections::VecDeque, sync::Arc};
use vault_core::{
    WorldState,
    camera::{CameraCommand, CameraRegion, SCROLL_BLOCKER_PID},
    ui::cursor,
};
use vault_formats::{HexGrid, Palette};
use vault_render::{
    EnhancedLayout, IndexedImage,
    map::{MapView, RoofCutaway, Shroud},
};
use winit::keyboard::KeyCode;

#[cfg(test)]
#[path = "enhanced_tests.rs"]
mod tests;

/// The tile the world canvas is drawn in: the map window's size.
const TILE: [u32; 2] = [640, 380];
/// The roof cutaway's radius and its dissolving ring at zoom 1, in world
/// pixels (divided by the zoom).
const PEEK_INNER: f64 = 96.0;
const PEEK_RING: f64 = 48.0;
/// The edge-scrolling band: this fraction of the world canvas, at least
/// [`EDGE_MIN`] canvas pixels.
const EDGE_FRACTION: f64 = 0.02;
const EDGE_MIN: f64 = 3.0;
/// The side-panel tile, in interface pixels.
const SIDE_PANEL: [u32; 2] = [32, 100];
/// The first palette-cycling index (SPEC-001 §3.2); the procedural colors
/// stay below it so they never animate.
const CYCLING: u8 = 229;
/// The camera demo's steps after its start, in presentation milliseconds.
const DEMO_ZOOM_AT: u64 = 1500;
const DEMO_SHAKE_AT: u64 = 2500;
const DEMO_RESET_AT: u64 = 4300;

/// A camera demo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoStep {
    Command(CameraCommand),
    /// The cursor X-ray until the given time.
    Peek(u64),
}

/// The pointer button that drags the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragButton {
    Middle,
    /// Space and the left button, out of combat.
    Left,
}

/// Where a pointer position goes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Route {
    /// The world canvas, at a canvas position.
    World([f64; 2]),
    /// The interface frame's Vanilla handlers, at a frame pixel (clamped to
    /// the frame when the pointer is outside it).
    Ui([i32; 2], bool),
    /// Nothing takes it (a side panel).
    Inert,
}

/// The region cache's key: the map (header name and number), the
/// elevation, and a fingerprint of the scroll blockers there.
type RegionKey = ([u8; 16], i32, u8, u64);

/// The `enhanced` presentation's state.
pub(crate) struct Enhanced {
    /// The current settings; Alt+R cycles their roof peek.
    pub(crate) settings: EnhancedSettings,
    pub(crate) camera: Camera,
    /// The layout of the last frame; pointer positions refer to it.
    pub(crate) layout: Option<EnhancedLayout>,
    /// The pointer the shell last reported, and its last canvas position.
    at: Option<PointerAt>,
    world_at: Option<[f64; 2]>,
    /// The pointer is over a side panel (it shows the arrow).
    inert: bool,
    region: Arc<CameraRegion>,
    region_key: Option<RegionKey>,
    /// The world's camera hex as last kept.
    synced_hex: i32,
    /// The debug free camera: no region, no clamp, roofs cut away.
    pub(crate) free_camera: bool,
    unbounded: Option<Arc<CameraRegion>>,
    demo: VecDeque<(u64, DemoStep)>,
    /// Scripted camera requests not yet started: they run in the order
    /// given, each once the one before has run its duration.
    scripted: VecDeque<CameraCommand>,
    /// When the running scripted request ends.
    scripted_until: u64,
    /// A temporary cursor X-ray (the demo) until this time.
    peek_until: Option<u64>,
    /// Up, right, down, left held for panning.
    arrows: [bool; 4],
    /// Space is held (Space and a left drag pan).
    space: bool,
    /// The keys being read are held keys read again, not new presses.
    replaying: bool,
    drag: Option<(DragButton, [f64; 2])>,
    /// The world behind a full-screen window, kept until input or a world
    /// change thaws it.
    pub(crate) frozen: Option<IndexedImage>,
    /// The last frame's world, interface and see-through index, held while
    /// a game fade freezes the screen.
    last: Option<(IndexedImage, IndexedImage, Option<u8>)>,
}

impl Enhanced {
    /// The canvas size: the last layout's, else a 640-by-380 view.
    fn world_size(&self) -> [u32; 2] {
        self.layout.map_or_else(
            || TILE.map(|value| (f64::from(value) / self.camera.zoom).ceil().max(1.0) as u32),
            |layout| layout.world_size,
        )
    }
    /// The zoom the canvas is drawn at.
    fn drawn_zoom(&self) -> f64 {
        self.layout
            .map_or(self.camera.zoom, |layout| f64::from(layout.zoom))
    }
    /// The camera center's soft bounds, or none for the free camera.
    fn bounds(&self) -> Option<[f64; 4]> {
        if self.free_camera {
            return None;
        }
        // The region bounds a hex's reference (cell corner); the camera
        // works with hex centers, 16 and 8 pixels further.
        let [x0, y0, x1, y1] = self.region.bounds.map(|value| value as f64);
        Some([x0 + 16.0, y0 + 8.0, x1 + 16.0, y1 + 8.0])
    }
    /// The region the shroud shows.
    fn shown_region(&mut self) -> Arc<CameraRegion> {
        if self.free_camera {
            self.unbounded
                .get_or_insert_with(|| Arc::new(CameraRegion::unbounded()))
                .clone()
        } else {
            self.region.clone()
        }
    }
}

/// The canvas tiles of `view`, each with its place on the canvas.
fn tiles(view: MapView) -> Vec<(MapView, [i64; 2])> {
    let mut tiles = Vec::new();
    for y in (0..view.height).step_by(TILE[1] as usize) {
        for x in (0..view.width).step_by(TILE[0] as usize) {
            tiles.push(tile(view, [x, y]));
        }
    }
    tiles
}

/// The tile of `view` whose corner is at canvas pixel `corner`.
fn tile(view: MapView, [x, y]: [u32; 2]) -> (MapView, [i64; 2]) {
    let offset = [i64::from(x), i64::from(y)];
    let mut tile = view;
    tile.grid = HexGrid {
        origin: [
            view.grid.origin[0] - offset[0],
            view.grid.origin[1] - offset[1],
        ],
    };
    tile.width = (view.width - x).min(TILE[0]);
    tile.height = (view.height - y).min(TILE[1]);
    (tile, offset)
}

/// The palette index nearest `rgb` (8-bit channels) below the cycling
/// range, index 0 excluded; the lowest index wins a tie.
fn nearest(palette: &Palette, rgb: [u8; 3]) -> u8 {
    let target = rgb.map(|channel| i32::from(channel >> 2));
    let distance = |index: u8| {
        let color = palette.rgb6(index).map(i32::from);
        (0..3)
            .map(|axis| (color[axis] - target[axis]).pow(2))
            .sum::<i32>()
    };
    let mapped = (1..CYCLING).filter(|index| palette.is_mapped(*index));
    let best = mapped.min_by_key(|index| (distance(*index), *index));
    best.unwrap_or_else(|| {
        (1..CYCLING)
            .min_by_key(|index| (distance(*index), *index))
            .unwrap_or(1)
    })
}

/// The canvas position of interface pixel `ui` (its center).
fn ui_to_world(layout: &EnhancedLayout, ui: [i32; 2]) -> [f64; 2] {
    let scale = f64::from(layout.ui_scale);
    let per_world = f64::from(layout.ui_scale * layout.zoom);
    [0, 1].map(|axis| {
        let surface = f64::from(layout.ui_rect[axis]) + (f64::from(ui[axis]) + 0.5) * scale;
        (surface - f64::from(layout.world_rect[axis])) / per_world
    })
}

/// The interface pixel nearest canvas position `world`, clamped to the
/// frame.
fn world_to_ui(layout: &EnhancedLayout, world: [f64; 2]) -> [i32; 2] {
    let scale = f64::from(layout.ui_scale);
    let per_world = f64::from(layout.ui_scale * layout.zoom);
    let frame = EnhancedLayout::UI_FRAME;
    [0, 1].map(|axis| {
        let surface = f64::from(layout.world_rect[axis]) + world[axis] * per_world;
        let pixel = ((surface - f64::from(layout.ui_rect[axis])) / scale).floor();
        pixel.clamp(0.0, f64::from(frame[axis] - 1)) as i32
    })
}

/// A canvas position's pixel.
fn pixel(world: [f64; 2]) -> [i32; 2] {
    world.map(|value| {
        value
            .floor()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    })
}

/// FNV-1a over the scroll blockers' hexes on `world`'s current elevation.
fn blocker_fingerprint(world: &WorldState) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for (_, object) in world.iter() {
        if object.raw.pid == SCROLL_BLOCKER_PID
            && object.raw.elevation == world.elevation
            && object.owner.is_none()
            && let Some(hex) = object.raw.hex
        {
            for byte in hex.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    hash
}

/// The shell's hooks.
impl MapPreview {
    /// Select the enhanced presentation (`None` is Vanilla).
    pub fn set_enhanced(&mut self, settings: Option<EnhancedSettings>) {
        let Some(settings) = settings else {
            self.enhanced = None;
            self.pointer.world = false;
            return;
        };
        if settings.viewport == ViewportMode::Fixed {
            self.pointer.world = false;
        }
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.camera.smooth = settings.smooth_camera;
            enhanced.camera.default_zoom =
                f64::from(settings.default_zoom).clamp(camera::MIN_ZOOM, camera::MAX_ZOOM);
            enhanced.settings = settings;
            return;
        }
        let center = self
            .player_point()
            .or_else(|| camera::hex_point(self.host.world.camera_hex))
            .unwrap_or_default();
        self.enhanced = Some(Box::new(Enhanced {
            settings,
            camera: Camera::new(
                center,
                f64::from(settings.default_zoom),
                settings.smooth_camera,
            ),
            layout: None,
            at: None,
            world_at: None,
            inert: false,
            region: Arc::new(CameraRegion::unbounded()),
            region_key: None,
            synced_hex: self.host.world.camera_hex,
            free_camera: false,
            unbounded: None,
            demo: VecDeque::new(),
            scripted: VecDeque::new(),
            scripted_until: 0,
            peek_until: None,
            arrows: [false; 4],
            space: false,
            replaying: false,
            drag: None,
            frozen: None,
            last: None,
        }));
        self.enhanced_sync_map();
    }
    /// The enhanced presentation settings, when it is selected.
    pub fn enhanced(&self) -> Option<EnhancedSettings> {
        self.enhanced.as_ref().map(|enhanced| enhanced.settings)
    }
    /// The world zoom the shell lays the next frame out with.
    pub fn enhanced_zoom(&self) -> f32 {
        self.enhanced
            .as_ref()
            .map_or(1.0, |enhanced| enhanced.camera.zoom as f32)
    }
    /// The enhanced frame for `layout`, or `None` to show the Vanilla
    /// frame (Vanilla preset, or the fixed viewport).
    pub fn enhanced_frame(&mut self, layout: &EnhancedLayout) -> Option<AppResult<EnhancedFrame>> {
        let enhanced = self.unlocked_mut()?;
        let old = enhanced.layout.replace(*layout);
        // The layout may raise the zoom to keep the canvas within its
        // largest size. Only that floor binds the camera: the layout's own
        // zoom is the one it was asked for, and as a floor it would pin a
        // zoom-out (or any zoom from a limit) where it stands.
        enhanced.camera.raise_zoom(f64::from(layout.min_zoom()));
        if let Some(old) = old.filter(|old| old != layout) {
            self.enhanced_relayout(&old, layout);
        }
        Some(self.compose_enhanced())
    }
    /// The pointer moved; returns whether the frame changed.
    pub fn enhanced_pointer(&mut self, at: PointerAt) -> bool {
        if self.unlocked().is_none() {
            return false;
        }
        if let Some(enhanced) = self.unlocked_mut() {
            enhanced.at = Some(at);
        }
        self.drag_view(at);
        match self.route(at) {
            Route::World(world) => {
                if let Some(enhanced) = self.unlocked_mut() {
                    enhanced.inert = false;
                    enhanced.world_at = Some(world);
                }
                self.set_pointer_outside(false);
                self.hover_in(pixel(world), true);
            }
            Route::Ui(ui, outside) => {
                if let Some(enhanced) = self.unlocked_mut() {
                    enhanced.inert = false;
                }
                self.set_pointer_outside(outside);
                self.hover_in(ui, false);
            }
            Route::Inert => {
                if let Some(enhanced) = self.unlocked_mut() {
                    enhanced.inert = true;
                }
                self.pointer_left();
                self.pointer.world = false;
            }
        }
        true
    }
    /// The layout changed under a still pointer (a zoom or a resize): the
    /// surface point it was at is routed again in the new layout, so its
    /// canvas position never goes stale. A stale one past a smaller
    /// canvas's edge would read as edge scrolling, which discards keys
    /// (SPEC-014 §3.28-3.29): `-` and `0` would not reach the zoom. A
    /// drag's anchor moves with it, so the same point is no motion.
    fn enhanced_relayout(&mut self, old: &EnhancedLayout, layout: &EnhancedLayout) {
        let Some(at) = self.unlocked().and_then(|enhanced| enhanced.at) else {
            return;
        };
        let point = match (at.world, at.ui) {
            (Some(world), _) => old.surface_point(world),
            (None, Some(ui)) => {
                let scale = f64::from(old.ui_scale);
                [0, 1]
                    .map(|axis| f64::from(old.ui_rect[axis]) + (f64::from(ui[axis]) + 0.5) * scale)
            }
            (None, None) => return,
        };
        let moved = PointerAt {
            ui: layout.ui_pixel(point),
            world: layout.world_point(point),
            side: layout.in_side_panel(point),
        };
        if moved == at {
            return;
        }
        let drag = self
            .unlocked_mut()
            .and_then(|enhanced| enhanced.drag.take());
        self.enhanced_pointer(moved);
        if let Some(enhanced) = self.unlocked_mut() {
            enhanced.drag = drag.map(|(button, last)| (button, moved.world.unwrap_or(last)));
        }
    }
    /// A pointer button; returns whether the frame changed.
    pub fn enhanced_button(&mut self, at: PointerAt, button: PointerButton, pressed: bool) -> bool {
        let Some(enhanced) = self.unlocked() else {
            return false;
        };
        if enhanced.at != Some(at) {
            self.enhanced_pointer(at);
        }
        let route = self.route(at);
        let dragging = self.unlocked().and_then(|enhanced| enhanced.drag);
        let world_view = matches!(route, Route::World(_)) && self.world_pointer();
        let start = match button {
            // The middle button always drags the view; Space and the left
            // button drag it out of combat (in combat Space ends the turn).
            PointerButton::Middle => Some(DragButton::Middle),
            PointerButton::Left
                if self.unlocked().is_some_and(|enhanced| enhanced.space)
                    && !self.host.world.combat =>
            {
                Some(DragButton::Left)
            }
            _ => None,
        };
        match (pressed, dragging, start) {
            (true, _, Some(drag)) if world_view && self.pointer.menu.is_none() => {
                if let (Route::World(world), Some(enhanced)) = (route, self.unlocked_mut()) {
                    enhanced.drag = Some((drag, world));
                }
                return true;
            }
            (false, Some((held, _)), _)
                if matches!(
                    (held, button),
                    (DragButton::Middle, PointerButton::Middle)
                        | (DragButton::Left, PointerButton::Left)
                ) =>
            {
                if let Some(enhanced) = self.unlocked_mut() {
                    enhanced.drag = None;
                }
                return true;
            }
            _ => {}
        }
        if button == PointerButton::Middle {
            return false;
        }
        let right = button == PointerButton::Right;
        // The debug overlay's panel takes its own presses (it is drawn on
        // the interface frame over the world).
        if pressed
            && world_view
            && let Some(ui) = at.ui.filter(|ui| self.debug_panel_at(*ui))
        {
            self.debug_panel_press(ui);
            return true;
        }
        // A press or release can change the cursor art even when nothing
        // acts (a held button, the menu's art), so the frame is redrawn.
        match route {
            Route::World(world) if pressed => self.click(pixel(world), right),
            Route::World(world) => self.release(Some(pixel(world)), right),
            Route::Ui(ui, _) if pressed => self.click(ui, right),
            Route::Ui(ui, _) => self.release(Some(ui), right),
            Route::Inert if pressed => false,
            Route::Inert => self.release(None, right),
        };
        true
    }
    /// Wheel lines (positive away from the user); returns whether the
    /// frame changed. Over the world it zooms by [`camera::ZOOM_STEP`] a
    /// line, keeping the world point under the pointer in place.
    pub fn enhanced_wheel(&mut self, at: PointerAt, lines: f64) -> bool {
        if self.unlocked().is_none() {
            return false;
        }
        match self.route(at) {
            Route::World(world) if self.world_pointer() && self.pointer.menu.is_none() => {
                self.zoom_view(camera::ZOOM_STEP.powf(lines), Some(world));
                true
            }
            _ => self.scroll(lines),
        }
    }
}

impl MapPreview {
    /// Route the pointer the shell last reported again (after a game fade
    /// held it); returns whether there was one.
    pub(crate) fn enhanced_pointer_again(&mut self) -> bool {
        match self.unlocked().and_then(|enhanced| enhanced.at) {
            Some(at) => self.enhanced_pointer(at),
            None => false,
        }
    }
    /// The enhanced state in the unlocked viewport.
    pub(crate) fn unlocked(&self) -> Option<&Enhanced> {
        self.enhanced
            .as_deref()
            .filter(|enhanced| enhanced.settings.viewport == ViewportMode::Unlocked)
    }
    fn unlocked_mut(&mut self) -> Option<&mut Enhanced> {
        self.enhanced
            .as_deref_mut()
            .filter(|enhanced| enhanced.settings.viewport == ViewportMode::Unlocked)
    }
    /// The player's drawn position: its hex center plus its displacement.
    fn player_point(&self) -> Option<[f64; 2]> {
        let player = self.host.world.get(self.host.world.player)?;
        let [x, y] = camera::hex_point(player.raw.hex?)?;
        let [dx, dy] = player.raw.displacement.map(f64::from);
        Some([x + dx, y + dy])
    }
    /// Whether the roofs draw on the enhanced canvas: as the map view's,
    /// unless the peek hides them or the free camera cuts them away.
    fn enhanced_roofs(&self) -> bool {
        self.unlocked().is_some_and(|enhanced| {
            self.view.roofs
                && enhanced.settings.roof_peek != RoofPeek::Hidden
                && !enhanced.free_camera
        })
    }
    /// The world canvas's view: the map view's elevation over the layout's
    /// world size, its grid centered on the camera (the sub-pixel center
    /// rounded to whole pixels, the shake included).
    pub(crate) fn enhanced_view(&self) -> Option<MapView> {
        let enhanced = self.unlocked()?;
        let [width, height] = enhanced.world_size();
        let center = enhanced.camera.drawn_center(self.clock);
        let round = |value: f64| value.round().clamp(-1e15, 1e15) as i64;
        Some(MapView {
            grid: HexGrid {
                origin: [
                    i64::from(width / 2) - round(center[0]),
                    i64::from(height / 2) - round(center[1]),
                ],
            },
            elevation: self.view.elevation,
            roofs: self.enhanced_roofs(),
            width,
            height,
        })
    }
    /// The view the pointer's map pixels refer to: the world canvas while
    /// the pointer is on it, else the 640-by-380 map view.
    pub(crate) fn pointer_view(&self) -> MapView {
        match self.enhanced_view() {
            Some(view) if self.pointer.world => view,
            _ => self.view,
        }
    }
    /// Whether a pointer pixel is in the map area: every canvas pixel, or
    /// a frame pixel above the bar (rows up to 379).
    pub(crate) fn map_area(&self, point: [i32; 2]) -> bool {
        self.pointer.world || point[1] <= self.map_bottom_row()
    }
    /// The roof cutaway under the pointer: the cursor X-ray, Alt held or
    /// the demo's peek, while the pointer is on the canvas and roofs draw.
    /// Canvas coordinates.
    fn peek_cutaway(&self) -> Option<RoofCutaway> {
        let enhanced = self.unlocked()?;
        let peeking = enhanced.settings.roof_peek == RoofPeek::CursorXray
            || self.hud.modifiers[2]
            || enhanced.peek_until.is_some_and(|until| self.clock < until);
        if !peeking || !self.enhanced_roofs() || !self.pointer.world {
            return None;
        }
        let at = self.pointer.at?;
        let zoom = enhanced.drawn_zoom();
        Some(RoofCutaway {
            center: at.map(i64::from),
            inner: (PEEK_INNER / zoom).round() as u32,
            ring: (PEEK_RING / zoom).round() as u32,
        })
    }
    /// A canvas pixel's pick: the tile drawing it, the pixel in the tile,
    /// and the cutaway there. `None` for a frame pixel (Vanilla picking).
    pub(crate) fn enhanced_pick(
        &self,
        point: [i32; 2],
    ) -> Option<(MapView, [i32; 2], Option<RoofCutaway>)> {
        if !self.pointer.world {
            return None;
        }
        let view = self.enhanced_view()?;
        let corner = [0, 1].map(|axis| {
            let size = [view.width, view.height][axis];
            let at = u32::try_from(point[axis].max(0))
                .unwrap_or(0)
                .min(size.saturating_sub(1));
            at - at % TILE[axis]
        });
        let (tile, [ox, oy]) = tile(view, corner);
        let cutaway = self.peek_cutaway().map(|cut| RoofCutaway {
            center: [cut.center[0] - ox, cut.center[1] - oy],
            ..cut
        });
        Some((tile, [point[0] - ox as i32, point[1] - oy as i32], cutaway))
    }
    /// The action menu's pointer: on the canvas, the pointer in interface
    /// pixels from the canvas's corner; else the frame pixel.
    pub(crate) fn menu_point(&self) -> Option<[i32; 2]> {
        match self.unlocked() {
            Some(enhanced) if self.pointer.world => {
                let world = enhanced.world_at?;
                let zoom = enhanced.drawn_zoom();
                Some(pixel([world[0] * zoom, world[1] * zoom]))
            }
            _ => self.pointer.at,
        }
    }
    /// The map area's right and bottom edges for the action menu's mirror
    /// and shift rules: 640 and 380, or the world area's in interface
    /// pixels on the canvas.
    pub(crate) fn menu_edges(&self) -> [i32; 2] {
        match self.unlocked().and_then(|enhanced| enhanced.layout) {
            Some(layout) if self.pointer.world => {
                let scale = layout.ui_scale.max(f32::EPSILON);
                [
                    (layout.world_rect[2] / scale) as i32,
                    (layout.world_rect[3] / scale) as i32,
                ]
            }
            _ => [640, 380],
        }
    }
    /// An action menu released on the canvas: the pointer returns to the
    /// canvas pixel under `at` (the menu's interface pixel), and the
    /// shell's warp (`take_warp`, an interface frame pixel) goes there when
    /// that place lies on the frame; beside the frame there is no warp.
    /// `None` off the canvas.
    pub(crate) fn enhanced_menu_return(&mut self, at: [i32; 2]) -> Option<[i32; 2]> {
        if !self.pointer.world {
            return None;
        }
        let enhanced = self.unlocked_mut()?;
        let zoom = enhanced.drawn_zoom();
        let world = [
            (f64::from(at[0]) + 0.5) / zoom,
            (f64::from(at[1]) + 0.5) / zoom,
        ];
        enhanced.world_at = Some(world);
        let warp = enhanced.layout.and_then(|layout| {
            let ui = world_to_ui(&layout, world);
            (ui_to_world(&layout, ui)
                .iter()
                .zip(world)
                .all(|(a, b)| (a - b).abs() <= 1.0 / zoom))
            .then_some(ui)
        });
        self.hud.warp = warp;
        Some(pixel(world))
    }
    /// Where a pointer position goes (see [`Route`]).
    fn route(&self, at: PointerAt) -> Route {
        let Some(enhanced) = self.unlocked() else {
            return Route::Inert;
        };
        let layout = enhanced.layout;
        let ui_world = |ui: [i32; 2]| layout.map(|layout| ui_to_world(&layout, ui));
        // An action menu opened on the canvas keeps the pointer there.
        if self.pointer.menu.is_some() && self.pointer.world {
            return at
                .world
                .or_else(|| at.ui.and_then(ui_world))
                .map_or(Route::Inert, Route::World);
        }
        // Windows, conversations and modal boxes take interface pixels; a
        // pointer outside the frame clamps to it (as in the letterbox).
        if !self.world_pointer() {
            if let Some(ui) = at.ui {
                return Route::Ui(ui, false);
            }
            return match (at.world, layout) {
                (Some(world), Some(layout)) => Route::Ui(world_to_ui(&layout, world), true),
                _ => Route::Inert,
            };
        }
        match (at.ui, at.world) {
            (Some(ui), _) if ui[1] > map_bottom() => Route::Ui(ui, false),
            (_, Some(world)) => Route::World(world),
            (Some(ui), None) => ui_world(ui).map_or(Route::Inert, Route::World),
            (None, None) => Route::Inert,
        }
    }
    /// A held drag moves the view by the pointer's motion.
    fn drag_view(&mut self, at: PointerAt) -> bool {
        let Some(enhanced) = self.unlocked_mut() else {
            return false;
        };
        let (Some((button, last)), Some(world)) = (enhanced.drag, at.world) else {
            return false;
        };
        enhanced
            .camera
            .drag([last[0] - world[0], last[1] - world[1]]);
        enhanced.drag = Some((button, world));
        true
    }
    /// Zoom by `factor` about a canvas position (`None`: the center).
    fn zoom_view(&mut self, factor: f64, anchor: Option<[f64; 2]>) {
        let goal = self
            .unlocked()
            .map_or(1.0, |enhanced| enhanced.camera.zoom_goal() * factor);
        self.zoom_view_to(goal, anchor);
    }
    /// Zoom to `zoom` about a canvas position (`None`: the center).
    fn zoom_view_to(&mut self, zoom: f64, anchor: Option<[f64; 2]>) {
        let origin = self.enhanced_view().map(|view| view.grid.origin);
        let now = self.clock;
        let Some(enhanced) = self.unlocked_mut() else {
            return;
        };
        let anchor = anchor
            .zip(origin)
            .map(|(point, origin)| [point[0] - origin[0] as f64, point[1] - origin[1] as f64]);
        enhanced.camera.zoom_user(zoom, anchor, now);
    }
    /// The pointer's canvas position for a key zoom, if it is on the
    /// canvas.
    fn zoom_anchor(&self) -> Option<[f64; 2]> {
        self.unlocked()
            .filter(|_| self.pointer.world && self.pointer.at.is_some())
            .and_then(|enhanced| enhanced.world_at)
    }

    /// The physical key went down (any screen): Space for the left-button
    /// drag.
    pub(crate) fn enhanced_key_down(&mut self, key: KeyCode) {
        if let Some(enhanced) = &mut self.enhanced
            && !enhanced.replaying
            && key == KeyCode::Space
        {
            enhanced.space = true;
        }
    }
    /// A key went up: Space and the panning arrows end.
    pub(crate) fn enhanced_key_up(&mut self, key: KeyCode) {
        let Some(enhanced) = &mut self.enhanced else {
            return;
        };
        match key {
            KeyCode::Space => enhanced.space = false,
            KeyCode::ArrowUp => enhanced.arrows[0] = false,
            KeyCode::ArrowRight => enhanced.arrows[1] = false,
            KeyCode::ArrowDown => enhanced.arrows[2] = false,
            KeyCode::ArrowLeft => enhanced.arrows[3] = false,
            _ => {}
        }
    }
    /// The focus went: no key or drag is held any more.
    pub(crate) fn enhanced_release_keys(&mut self) {
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.arrows = [false; 4];
            enhanced.space = false;
            enhanced.drag = None;
        }
    }
    /// Held keys are being read again (SPEC-014 §3.27): they are not new
    /// presses, so they start no panning.
    pub(crate) fn set_enhanced_replay(&mut self, replaying: bool) {
        if let Some(enhanced) = &mut self.enhanced {
            enhanced.replaying = replaying;
        }
    }
    /// The unlocked map screen's camera and roof keys: `=`, `-` and `0`
    /// zoom about the pointer (Ctrl keeps them as brightness), Alt+R
    /// cycles the roof peek, and held arrow keys pan. Returns whether the
    /// key was taken.
    pub(crate) fn enhanced_key(&mut self, key: KeyCode) -> bool {
        if self.unlocked().is_none() || !matches!(self.screen, Screen::Map) {
            return false;
        }
        let [_, control, alt] = self.hud.modifiers;
        let arrow = match key {
            KeyCode::ArrowUp => Some(0),
            KeyCode::ArrowRight => Some(1),
            KeyCode::ArrowDown => Some(2),
            KeyCode::ArrowLeft => Some(3),
            _ => None,
        };
        if let Some(arrow) = arrow {
            if self.host.world.ui_disabled {
                return false;
            }
            if let Some(enhanced) = self.unlocked_mut()
                && !enhanced.replaying
            {
                enhanced.arrows[arrow] = true;
            }
            return true;
        }
        match key {
            KeyCode::KeyR if alt => {
                if let Some(enhanced) = self.unlocked_mut() {
                    enhanced.settings.roof_peek = enhanced.settings.roof_peek.next();
                }
                self.pointer.redraw = true;
            }
            KeyCode::Equal | KeyCode::NumpadAdd if !control => {
                let anchor = self.zoom_anchor();
                self.zoom_view(camera::ZOOM_STEP, anchor);
            }
            KeyCode::Minus | KeyCode::NumpadSubtract if !control => {
                let anchor = self.zoom_anchor();
                self.zoom_view(1.0 / camera::ZOOM_STEP, anchor);
            }
            KeyCode::Digit0 | KeyCode::Numpad0 if !control => {
                let anchor = self.zoom_anchor();
                let zoom = self
                    .unlocked()
                    .map_or(1.0, |enhanced| enhanced.camera.default_zoom);
                self.zoom_view_to(zoom, anchor);
            }
            _ => return false,
        }
        true
    }
    /// Home in the unlocked view: the camera follows the player again.
    /// Returns whether it did (else the Vanilla walk runs).
    pub(crate) fn enhanced_follow_player(&mut self) -> bool {
        let Some(enhanced) = self.unlocked_mut() else {
            return false;
        };
        enhanced.camera.resume_follow();
        self.last_hover = None;
        true
    }
    /// A scripted camera request (`WorldEffect::Camera`), queued behind
    /// the ones still running. Without the unlocked view it does nothing.
    pub(crate) fn camera_command(&mut self, command: CameraCommand) {
        let now = self.clock;
        let Some(enhanced) = self.unlocked_mut() else {
            tracing::debug!(?command, "camera request without the enhanced view");
            return;
        };
        enhanced.scripted.push_back(command);
        enhanced.scripted_tick(now);
    }
}

impl Enhanced {
    /// Starts the scripted requests that came due by `now`: a script gives
    /// a whole camera sequence at once (a pan, a zoom, a shake, a reset),
    /// so each waits for the one before it to run its duration. Returns
    /// whether any started.
    fn scripted_tick(&mut self, now: u64) -> bool {
        let mut any = false;
        while now >= self.scripted_until
            && let Some(command) = self.scripted.pop_front()
        {
            let duration = match command {
                CameraCommand::PanTo { duration_ms, .. }
                | CameraCommand::SetZoom { duration_ms, .. }
                | CameraCommand::Shake { duration_ms, .. }
                | CameraCommand::Reset { duration_ms } => u64::from(duration_ms),
            };
            self.scripted_until = now + duration;
            self.camera.apply(command, now);
            any = true;
        }
        any
    }
}

impl Camera {
    /// Starts `command` at `now`.
    fn apply(&mut self, command: CameraCommand, now: u64) {
        match command {
            CameraCommand::PanTo { tile, duration_ms } => {
                if let Some(point) = camera::hex_point(tile) {
                    self.pan_to(point, now, u64::from(duration_ms));
                }
            }
            CameraCommand::SetZoom {
                percent,
                duration_ms,
            } => self.zoom_to(
                f64::from(percent) / 100.0,
                None,
                now,
                u64::from(duration_ms),
            ),
            CameraCommand::Shake {
                intensity,
                duration_ms,
            } => self.shake(f64::from(intensity), now, u64::from(duration_ms)),
            CameraCommand::Reset { duration_ms } => self.reset(now, u64::from(duration_ms)),
        }
    }
}

impl MapPreview {
    /// The debug free camera on or off; returns whether it is now on.
    pub(crate) fn toggle_free_camera(&mut self) -> bool {
        let Some(enhanced) = self.unlocked_mut() else {
            return false;
        };
        enhanced.free_camera = !enhanced.free_camera;
        enhanced.frozen = None;
        self.pointer.redraw = true;
        self.unlocked().is_some_and(|enhanced| enhanced.free_camera)
    }
    /// The debug camera demo: a pan to the hex under the pointer (else 10
    /// hexes from the player) over 1.5 s, a zoom to 200% over 1 s, a shake
    /// of 6 over 0.8 s with a cursor X-ray, then a reset over 1.5 s, timed
    /// in presentation time.
    pub(crate) fn start_camera_demo(&mut self) {
        let now = self.clock;
        let under = self
            .pointer
            .at
            .filter(|_| self.pointer.world)
            .and_then(|at| self.pointer_view().grid.pick_hex(at))
            .filter(|hex| (0..40000).contains(hex));
        let near = || {
            let player = self.host.world.get(self.host.world.player)?.raw.hex?;
            HexGrid::step(player, 2, 10).ok()
        };
        let target = under.or_else(near);
        let Some(enhanced) = self.unlocked_mut() else {
            return;
        };
        enhanced.demo.clear();
        if let Some(tile) = target {
            enhanced.demo.push_back((
                now,
                DemoStep::Command(CameraCommand::PanTo {
                    tile,
                    duration_ms: 1500,
                }),
            ));
        }
        enhanced.demo.extend([
            (
                now + DEMO_ZOOM_AT,
                DemoStep::Command(CameraCommand::SetZoom {
                    percent: 200,
                    duration_ms: 1000,
                }),
            ),
            (
                now + DEMO_SHAKE_AT,
                DemoStep::Command(CameraCommand::Shake {
                    intensity: 6,
                    duration_ms: 800,
                }),
            ),
            (now + DEMO_SHAKE_AT, DemoStep::Peek(now + DEMO_RESET_AT)),
            (
                now + DEMO_RESET_AT,
                DemoStep::Command(CameraCommand::Reset { duration_ms: 1500 }),
            ),
        ]);
        self.demo_tick(now);
    }
    /// The demo steps that came due by `now`; returns whether any did.
    fn demo_tick(&mut self, now: u64) -> bool {
        let mut due = Vec::new();
        if let Some(enhanced) = self.unlocked_mut() {
            while enhanced.demo.front().is_some_and(|(at, _)| *at <= now) {
                due.extend(enhanced.demo.pop_front().map(|(_, step)| step));
            }
        }
        let any = !due.is_empty();
        let Some(enhanced) = self.unlocked_mut() else {
            return any;
        };
        for step in due {
            match step {
                // The demo's own timeline spaces its steps (not the
                // scripted queue).
                DemoStep::Command(command) => enhanced.camera.apply(command, now),
                DemoStep::Peek(until) => enhanced.peek_until = Some(until),
            }
        }
        any
    }
    /// The camera page's lines (the debug overlay).
    pub(crate) fn camera_lines(&self) -> Vec<String> {
        let Some(enhanced) = self.unlocked() else {
            return vec!["the unlocked enhanced view is off".into()];
        };
        let camera = &enhanced.camera;
        let [x0, y0, x1, y1] = enhanced.region.bounds;
        vec![
            format!(
                "center {:.1}, {:.1}  zoom {:.2}",
                camera.center[0], camera.center[1], camera.zoom
            ),
            format!(
                "following {}  camera hex {}",
                if camera.follow { "yes" } else { "no" },
                self.host.world.camera_hex
            ),
            format!(
                "free camera {}  roof peek {:?}",
                if enhanced.free_camera { "on" } else { "off" },
                enhanced.settings.roof_peek
            ),
            format!("region {x0},{y0} - {x1},{y1}"),
        ]
    }

    /// The region follows the map and elevation (and the scroll blockers'
    /// places); a new map or elevation also puts the camera on the player.
    /// Returns whether anything changed.
    fn enhanced_sync_map(&mut self) -> bool {
        let world = &self.host.world;
        let key: RegionKey = (
            world.map.header.name,
            world.map.header.map_number,
            world.elevation,
            blocker_fingerprint(world),
        );
        if self
            .unlocked()
            .is_none_or(|enhanced| enhanced.region_key == Some(key))
        {
            return false;
        }
        let region = Arc::new(CameraRegion::compute(&self.host.world));
        let player = self.player_point();
        let camera_hex = self.host.world.camera_hex;
        let Some(enhanced) = self.unlocked_mut() else {
            return false;
        };
        let moved = enhanced
            .region_key
            .is_none_or(|old| (old.0, old.1, old.2) != (key.0, key.1, key.2));
        enhanced.region = region;
        enhanced.region_key = Some(key);
        if moved {
            if let Some(player) = player {
                enhanced.camera.snap(player);
            }
            enhanced.camera.follow = true;
            enhanced.synced_hex = camera_hex;
            enhanced.frozen = None;
        }
        true
    }
    /// The pan direction the arrow keys and the edges ask for.
    fn manual_pan(&self) -> [f64; 2] {
        let Some(enhanced) = self.unlocked() else {
            return [0.0; 2];
        };
        if !self.world_pointer() || self.pointer.menu.is_some() {
            return [0.0; 2];
        }
        let mut pan = [0.0; 2];
        if !self.keyboard_locked() && !self.host.world.ui_disabled {
            let [up, right, down, left] = enhanced.arrows.map(f64::from);
            pan = [right - left, down - up];
        }
        if let Some(edge) = self.pointer.edge {
            let [dx, dy] = cursor::edge_step(edge.direction).map(f64::from);
            pan = [pan[0] + dx, pan[1] + dy];
        }
        pan.map(|value| value.clamp(-1.0, 1.0))
    }
    /// One presentation tick of the unlocked view: the region, the demo,
    /// the camera, and the world's camera hex kept at the hex nearest the
    /// camera center. Returns whether the view changed.
    pub(crate) fn enhanced_tick(&mut self, now: u64) -> bool {
        if self.unlocked().is_none() {
            return false;
        }
        let mut changed = self.enhanced_sync_map();
        changed |= self.demo_tick(now);
        changed |= self
            .unlocked_mut()
            .is_some_and(|enhanced| enhanced.scripted_tick(now));
        let player = self.player_point();
        let player_hex = self
            .host
            .world
            .get(self.host.world.player)
            .and_then(|player| player.raw.hex);
        let pan = self.manual_pan();
        let camera_hex = self.host.world.camera_hex;
        let Some(enhanced) = self.unlocked_mut() else {
            return changed;
        };
        // A view center the world moved itself (a script or a load), not
        // to the player (the player's own steps center it in Vanilla),
        // takes the camera there.
        if camera_hex != enhanced.synced_hex
            && Some(camera_hex) != player_hex
            && let Some(point) = camera::hex_point(camera_hex)
        {
            enhanced.camera.follow = false;
            enhanced.camera.snap(point);
            changed = true;
        }
        let bounds = enhanced.bounds();
        changed |= enhanced.camera.tick(
            now,
            TickInput {
                player,
                pan,
                bounds,
            },
        );
        changed |= enhanced.camera.moving(now);
        let nearest = camera::point_hex(enhanced.camera.center);
        if let Some(hex) = nearest
            && hex != self.host.world.camera_hex
        {
            self.host.world.set_view_center(hex);
        }
        let camera_hex = self.host.world.camera_hex;
        if let Some(enhanced) = self.unlocked_mut() {
            enhanced.synced_hex = camera_hex;
        }
        if changed {
            self.thaw_view();
            self.last_hover = None;
        }
        changed
    }
    /// The unlocked view's edge scrolling, from the world canvas's edges
    /// only (the side panels and the bar are never edges): it shows the
    /// scroll art (blocked where the camera is held at its bounds) while
    /// the camera pans that way. `None` outside the unlocked view.
    pub(crate) fn enhanced_edge_scroll(&mut self) -> Option<bool> {
        let enhanced = self.unlocked()?;
        let [width, height] = enhanced.world_size().map(f64::from);
        let direction = self
            .pointer
            .at
            .filter(|_| {
                self.pointer.world
                    && self.hud.panel.animation.is_none()
                    && self.host.world.scrolling_enabled
            })
            .filter(|[x, y]| f64::from(*x) < width && f64::from(*y) < height)
            .and_then(|[x, y]| {
                let (x, y) = (f64::from(x), f64::from(y));
                let band = |size: f64| (size * EDGE_FRACTION).max(EDGE_MIN);
                let (bx, by) = (band(width), band(height));
                let column = if x < bx {
                    0
                } else if x >= width - bx {
                    639
                } else {
                    320
                };
                let row = if y < by {
                    0
                } else if y >= height - by {
                    479
                } else {
                    240
                };
                cursor::edge_direction(column, row)
            });
        let Some(direction) = direction else {
            let left = self.pointer.edge.take().is_some();
            if left {
                self.restart_rest(true);
            }
            return Some(left);
        };
        let blocked = enhanced.bounds().is_some_and(|[x0, y0, x1, y1]| {
            let [dx, dy] = cursor::edge_step(direction);
            let center = enhanced.camera.center;
            let held = |step: i32, value: f64, low: f64, high: f64| match step {
                ..0 => value <= low + 0.5,
                0 => true,
                _ => value >= high - 0.5,
            };
            held(dx, center[0], x0, x1) && held(dy, center[1], y0, y1)
        });
        let edge = Edge { direction, blocked };
        let changed = self
            .pointer
            .edge
            .is_none_or(|old| (old.direction, old.blocked) != (direction, blocked));
        self.pointer.edge = Some(edge);
        Some(changed)
    }

    /// One enhanced frame.
    fn compose_enhanced(&mut self) -> AppResult<EnhancedFrame> {
        self.drain_effects();
        self.enhanced_sync_map();
        // SPEC-014 §3.2: a game fade to black holds the frame on screen.
        if self.fade_blocks()
            && self.fades.freezes()
            && let Some((world, ui, ui_transparent)) =
                self.unlocked().and_then(|enhanced| enhanced.last.clone())
        {
            return Ok(EnhancedFrame {
                world,
                ui,
                ui_transparent,
                side_panel: Some(self.side_panel()),
                cursor: self.enhanced_cursor(),
            });
        }
        self.check_barter();
        self.check_loot();
        self.cursor_in_frame = false;
        let map = matches!(self.screen, Screen::Map) && !self.host.world.dialogue.open;
        let (world, ui, ui_transparent) = if map {
            let world = self.enhanced_world()?;
            let (ui, transparent) = self.enhanced_map_ui()?;
            (world, ui, transparent)
        } else {
            let ui = self.screen_image()?;
            (self.enhanced_backdrop()?, ui, None)
        };
        self.held_shown = self.held_button().is_some();
        self.frame = Some(ui.clone());
        if let Some(enhanced) = self.unlocked_mut() {
            enhanced.last = Some((world.clone(), ui.clone(), ui_transparent));
        }
        Ok(EnhancedFrame {
            world,
            ui,
            ui_transparent,
            side_panel: Some(self.side_panel()),
            cursor: self.enhanced_cursor(),
        })
    }
    /// The world canvas: every tile drawn as one map refresh, with the
    /// in-world cursor (while the pointer is on the canvas), the roof
    /// peek and the shroud over the hexes outside the camera's region.
    fn enhanced_world(&mut self) -> AppResult<IndexedImage> {
        let view = self.enhanced_view().ok_or("no enhanced view")?;
        let cursor = if self.pointer.world {
            self.cursor_objects(view)
        } else {
            None
        };
        let cutaway = self.peek_cutaway();
        let dark = nearest(self.game_palette(), [0, 0, 0]);
        let region = self.unlocked_mut().map(|enhanced| enhanced.shown_region());
        let shroud = region.as_deref().map(|region| Shroud {
            allowed: region.allowed(),
            dark,
        });
        let tiles = tiles(view);
        // The tiles overscan: a sprite crossing a seam draws in both.
        let images = self.render_scenes(&tiles, cursor.as_ref(), cutaway, shroud, true)?;
        let mut canvas = IndexedImage::new(view.width, view.height)?;
        for ((_, [x, y]), image) in tiles.iter().zip(&images) {
            canvas.copy_image(image, [*x as i32, *y as i32]);
        }
        Ok(canvas)
    }
    /// The world behind a full-screen window: black over the world and
    /// town maps and movies, else the map frozen until input or a world
    /// change.
    fn enhanced_backdrop(&mut self) -> AppResult<IndexedImage> {
        let [width, height] = self
            .unlocked()
            .map_or([1, 1], |enhanced| enhanced.world_size());
        if matches!(
            self.screen,
            Screen::World(_)
                | Screen::Town(_)
                | Screen::World2(_)
                | Screen::Town2(_)
                | Screen::Movie(_)
        ) {
            return Ok(IndexedImage::new(width, height)?);
        }
        if let Some(frozen) = self
            .unlocked()
            .and_then(|enhanced| enhanced.frozen.as_ref())
            && [frozen.width(), frozen.height()] == [width, height]
        {
            return Ok(frozen.clone());
        }
        let world = self.enhanced_world()?;
        if let Some(enhanced) = self.unlocked_mut() {
            enhanced.frozen = Some(world.clone());
        }
        Ok(world)
    }
    /// The map screen's 640-by-480 frame over a `fill` map view.
    fn map_ui_over(&mut self, fill: u8) -> AppResult<IndexedImage> {
        let mut view = IndexedImage::new(TILE[0], TILE[1])?;
        view.fill_rect([0, 0, TILE[0] as i32, TILE[1] as i32], fill);
        let mut image = self.compose_world(view);
        self.draw_overlays(&mut image);
        Ok(image)
    }
    /// The map screen's interface frame without the map view: what the
    /// Vanilla frame composes over the map rows (the indicator boxes, the
    /// bar and every window over the map) stays, and the rest is the
    /// see-through index, the lowest index the frame does not use. The
    /// frame is composed over two different fills; a pixel that differs
    /// shows the map.
    fn enhanced_map_ui(&mut self) -> AppResult<(IndexedImage, Option<u8>)> {
        self.choices.clear();
        let mut ui = self.map_ui_over(0)?;
        let other = self.map_ui_over(255)?;
        let mut used = [false; 256];
        for (first, second) in ui.pixels().iter().zip(other.pixels()) {
            if first == second {
                used[usize::from(*first)] = true;
            }
        }
        let Some(clear) = (0..=255).find(|index| !used[usize::from(*index)]) else {
            return Ok((ui, None));
        };
        for (pixel, second) in ui.pixels_mut().iter_mut().zip(other.pixels()) {
            if pixel != second {
                *pixel = clear;
            }
        }
        Ok((ui, Some(clear)))
    }
    /// The pointer art `draw_cursor` would draw and its hotspot: drawn
    /// apart with the pointer at the image's middle, then cropped. Over a
    /// side panel the arrow. `None` when no art shows (the in-world cursor
    /// is part of the world canvas).
    fn enhanced_cursor(&self) -> Option<(IndexedImage, [i32; 2])> {
        let enhanced = self.unlocked()?;
        // An action menu can sit far from the pointer.
        let size: u32 = if self.pointer.menu.is_some() {
            1024
        } else {
            256
        };
        let middle = (size / 2) as i32;
        let mut scratch = IndexedImage::new(size, size).ok()?;
        match self.pointer.at {
            Some(at) => self.draw_cursor_shifted(&mut scratch, [middle - at[0], middle - at[1]]),
            None if enhanced.inert => {
                let painter = Painter {
                    assets: &self.host.assets,
                    palette: &self.palette,
                };
                painter.cursor(&mut scratch, cursor::ARROW.art, [middle, middle]);
            }
            None => return None,
        }
        let width = size as usize;
        let (mut low, mut high) = ([usize::MAX; 2], [0usize; 2]);
        for (index, value) in scratch.pixels().iter().enumerate() {
            if *value != 0 {
                let at = [index % width, index / width];
                for axis in 0..2 {
                    low[axis] = low[axis].min(at[axis]);
                    high[axis] = high[axis].max(at[axis]);
                }
            }
        }
        if low[0] > high[0] {
            return None;
        }
        let [w, h] = [high[0] - low[0] + 1, high[1] - low[1] + 1];
        let mut pixels = Vec::with_capacity(w * h);
        for row in low[1]..=high[1] {
            let start = row * width + low[0];
            pixels.extend_from_slice(&scratch.pixels()[start..start + w]);
        }
        let image = IndexedImage::from_pixels(w as u32, h as u32, pixels).ok()?;
        Some((image, [middle - low[0] as i32, middle - low[1] as i32]))
    }
    /// The side panel's tile: a dark riveted plate with two grooves, in the
    /// game palette's nearest colors (no game art).
    fn side_panel(&self) -> IndexedImage {
        let palette = self.game_palette();
        let plate = nearest(palette, [46, 48, 42]);
        let light = nearest(palette, [82, 84, 72]);
        let shadow = nearest(palette, [20, 21, 18]);
        let rivet = nearest(palette, [124, 120, 100]);
        let [width, height] = SIDE_PANEL.map(|value| value as usize);
        let mut pixels = vec![plate; width * height];
        for y in 0..height {
            for x in 0..width {
                // A bevelled edge, and two grooves across the plate.
                let color = if y == 0 || x == 0 {
                    light
                } else if y == height - 1 || x == width - 1 || y == 33 || y == 66 {
                    shadow
                } else if y == 34 || y == 67 {
                    light
                } else {
                    plate
                };
                pixels[y * width + x] = color;
            }
        }
        // Four rivets, a highlight above-left and a shadow below-right.
        for [cx, cy] in [[6, 16], [25, 16], [6, 83], [25, 83]] {
            for dy in -2_i32..=2 {
                for dx in -2_i32..=2 {
                    let distance = dx * dx + dy * dy;
                    let color = match distance {
                        0..=2 if dx + dy < 0 => light,
                        0..=2 => rivet,
                        3..=4 if dx + dy > 0 => shadow,
                        _ => continue,
                    };
                    let (x, y) = ((cx + dx) as usize, (cy + dy) as usize);
                    pixels[y * width + x] = color;
                }
            }
        }
        IndexedImage::from_pixels(SIDE_PANEL[0], SIDE_PANEL[1], pixels)
            .expect("the side panel's fixed size")
    }
}
