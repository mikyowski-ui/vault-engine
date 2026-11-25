use super::*;

/// An enhanced frame, its layout, its side panels tiled to that layout,
/// and the bezel around a full-screen window, if it shows one.
pub(crate) struct EnhancedShown {
    pub(crate) layout: EnhancedLayout,
    pub(crate) frame: enhanced::EnhancedFrame,
    pub(crate) sides: Vec<(IndexedImage, [f32; 4])>,
    pub(crate) bezel: Option<(IndexedImage, [f32; 4])>,
}

/// The widescreen bezel's layer around a 640x480 frame drawn into
/// `frame_rect` on a `surface`-sized window, in the display palette's
/// nearest colors (`vault_render::bezel`).
pub(crate) fn bezel_layer(
    surface: [u32; 2],
    frame_rect: [f32; 4],
    palette: &[[u8; 4]; 256],
) -> Option<(IndexedImage, [f32; 4])> {
    let inks = vault_render::bezel::BezelInks::nearest(palette);
    vault_render::bezel::bezel_layer(surface, frame_rect, inks)
}

/// The side panels' images and surface rectangles: `panel` tiled to each
/// side rectangle's size in interface pixels (rounded up), the left one
/// aligned to its right edge and the right one to its left edge, so both
/// continue from the interface bar. Empty sides have none.
pub(crate) fn side_tiles(
    layout: &EnhancedLayout,
    panel: &IndexedImage,
) -> Vec<(IndexedImage, [f32; 4])> {
    let scale = layout.ui_scale;
    let [panel_w, panel_h] = [panel.width() as usize, panel.height() as usize];
    let mut out = Vec::new();
    for (side, [x, y, w, h]) in layout.side_rects.into_iter().enumerate() {
        let width = (w / scale).ceil() as u32;
        let height = (h / scale).ceil() as u32;
        let Ok(mut tile) = IndexedImage::new(width, height) else {
            continue;
        };
        let cols = width as usize;
        // The left panel's last column is the tile's last column.
        let offset = if side == 0 {
            panel_w - cols % panel_w
        } else {
            0
        };
        let pixels = panel.pixels();
        for (row, line) in tile.pixels_mut().chunks_exact_mut(cols).enumerate() {
            let source = &pixels[(row % panel_h) * panel_w..][..panel_w];
            for (col, pixel) in line.iter_mut().enumerate() {
                *pixel = source[(col + offset) % panel_w];
            }
        }
        let (drawn_w, drawn_h) = (width as f32 * scale, height as f32 * scale);
        let left = if side == 0 { x + w - drawn_w } else { x };
        out.push((tile, [left, y, drawn_w, drawn_h]));
    }
    out
}

/// The enhanced layers back to front: the world (opaque; from the world
/// area's corner, at its own size), or the bezel in its place around a
/// full-screen window, the side panels, the interface frame (see-through
/// where it says), then the pointer art at the pointer, its hotspot and
/// size at interface scale (index 0 see-through).
pub(crate) fn enhanced_layers<'a>(
    shown: &'a EnhancedShown,
    pointer: Option<[f64; 2]>,
) -> Vec<Layer<'a>> {
    let EnhancedShown {
        layout,
        frame,
        sides,
        bezel,
    } = shown;
    let mut layers = vec![match bezel {
        Some((image, rect)) => Layer {
            image,
            rect: *rect,
            transparent: None,
            sharp: true,
        },
        None => Layer {
            image: &frame.world,
            rect: layout.canvas_rect([frame.world.width(), frame.world.height()]),
            transparent: None,
            sharp: true,
        },
    }];
    layers.extend(sides.iter().map(|(image, rect)| Layer {
        image,
        rect: *rect,
        transparent: None,
        sharp: true,
    }));
    layers.push(Layer {
        image: &frame.ui,
        rect: layout.ui_rect,
        transparent: frame.ui_transparent,
        sharp: true,
    });
    if let (Some((cursor, [hot_x, hot_y])), Some([x, y])) = (&frame.cursor, pointer) {
        let scale = layout.ui_scale;
        layers.push(Layer {
            image: cursor,
            rect: [
                x as f32 - *hot_x as f32 * scale,
                y as f32 - *hot_y as f32 * scale,
                cursor.width() as f32 * scale,
                cursor.height() as f32 * scale,
            ],
            transparent: Some(0),
            sharp: true,
        });
    }
    layers
}

/// A hint's plate as a layer, at its place in the interface frame.
fn toast_layer<'a>(plate: &'a IndexedImage, shown: &EnhancedShown) -> Layer<'a> {
    let scale = shown.layout.ui_scale;
    let [left, top, _, _] = shown.layout.ui_rect;
    let [x, y] = toast::plate_origin(shown.frame.ui.width(), plate);
    Layer {
        image: plate,
        rect: [
            left + x as f32 * scale,
            top + y as f32 * scale,
            plate.width() as f32 * scale,
            plate.height() as f32 * scale,
        ],
        transparent: Some(0),
        sharp: true,
    }
}

impl Shell {
    /// A key press (or an engine repeat) reaches the preview. A standalone
    /// sprite or map preview, or the bare color test, exits on an Escape it
    /// does not consume; the game never exits on a key (G-01).
    pub(super) fn press_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        key: KeyCode,
    ) {
        let pressed = match &mut self.session {
            Some(session) => {
                let exits = session.escape_exits();
                session.press_key(key, exits)
            }
            None if key == KeyCode::Escape => session::KeyPress::Exit,
            None => session::KeyPress::Ignored,
        };
        match pressed {
            session::KeyPress::Changed => self.refresh_preview(event_loop, window),
            session::KeyPress::Exit => {
                self.log(event_loop, InputCommand::Quit);
                event_loop.exit();
            }
            session::KeyPress::Ignored => {}
        }
    }
    pub(super) fn refresh_preview(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        if self.enhanced_layout(window).is_some() {
            return self.refresh_enhanced(event_loop, window);
        }
        self.shown = None;
        let Some(preview) = &mut self.session else {
            return;
        };
        match preview.image() {
            Ok(image) => {
                let palette = self.palette();
                self.show_frame(image, &palette);
                // The composed frame already shows the cursor where it is.
                self.pointer_moved = false;
                let Some(preview) = &self.session else {
                    return;
                };
                let title = preview.title();
                if title != self.title {
                    window.set_title(&title);
                    self.title = title;
                }
                window.set_cursor_visible(preview.system_cursor());
                window.request_redraw();
            }
            Err(error) => self.fail(event_loop, error),
        }
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
        }
    }
    /// A composed frame on the screen, with the shown hint over a copy, so
    /// that the frame itself never holds a toast (SPEC-020 §2.6).
    pub(super) fn show_frame(&mut self, image: IndexedImage, palette: &[[u8; 4]; 256]) {
        let plate = self.toast_plate(palette);
        if let Some(renderer) = &mut self.renderer {
            match &plate {
                Some(plate) => renderer.set_image(&toast::over(&image, plate), palette),
                None => renderer.set_image(&image, palette),
            }
        }
        self.frame = Some(image);
    }
    /// The hint showing, as a plate in `palette`'s nearest colors.
    fn toast_plate(&self, palette: &[[u8; 4]; 256]) -> Option<IndexedImage> {
        let toasts = self.toasts.as_ref()?;
        let (_, step) = toasts.shown()?;
        Some(toast::plate(
            toasts.text()?,
            step,
            toast::Inks::nearest(palette),
        ))
    }
    /// One frame of the hints: the timer runs on the shell clock, and only
    /// reads the session. A changed hint redraws what is already composed.
    pub(super) fn tick_toasts(&mut self, window: &Window, now: u64) {
        let ready = self
            .session
            .as_ref()
            .is_some_and(|session| session.hints_ready());
        let Some(toasts) = &mut self.toasts else {
            return;
        };
        let changed = toasts.tick(now, ready);
        if toasts.take_finished() {
            self.finish_hints();
        }
        if changed {
            self.redraw_toast(window);
        }
    }
    /// The hint changed: the same frame again, with it or without it.
    fn redraw_toast(&mut self, window: &Window) {
        if self.shown.is_none()
            && let Some(frame) = self.frame.take()
        {
            let palette = self.palette();
            self.show_frame(frame, &palette);
        }
        window.request_redraw();
    }
    /// A key press ends the hint that names it (it changes no key's action).
    pub(super) fn toast_key(&mut self, window: &Window, key: KeyCode, alt: bool) {
        if self
            .toasts
            .as_mut()
            .is_some_and(|toasts| toasts.key(key, alt))
        {
            self.redraw_toast(window);
        }
    }
    /// The list is finished: `once` becomes `off`, in one commit, or in
    /// memory with no settings file. A failed commit is a diagnostic, and
    /// the session goes on (SPEC-020 §2.6).
    fn finish_hints(&mut self) {
        self.toasts = None;
        if !std::mem::take(&mut self.hints_once) {
            return;
        }
        match boot::finish_hints(&config::search_dirs()) {
            Ok(saved) => self.hints_off = !saved,
            Err(error) => tracing::warn!(%error, "hotkey_hints not saved"),
        }
    }
    /// The enhanced presentation's refresh: the next redraw builds the
    /// frame for the window's layout then; the title and the pointer
    /// follow as in [`Shell::refresh_preview`].
    fn refresh_enhanced(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(preview) = &mut self.session else {
            return;
        };
        self.enhanced_dirty = true;
        self.pointer_moved = false;
        let title = preview.title();
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
        window.request_redraw();
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
        }
    }
    /// The enhanced layout for the window, while the preview's world shows
    /// the unlocked viewport (it follows the window size and the zoom, so
    /// resizes, DPI and fullscreen changes and zooms all lay it out anew).
    pub(super) fn enhanced_layout(&mut self, window: &Window) -> Option<EnhancedLayout> {
        let size = window.inner_size();
        replay::enhanced_layout(self.session.as_mut()?, [size.width, size.height])
    }
    /// Present the enhanced layers for this redraw, building the frame
    /// again when the world or the layout changed; returns whether the
    /// enhanced frame is shown. When the world answers with no enhanced
    /// frame the Vanilla frame is shown instead.
    pub(super) fn present_enhanced(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
    ) -> bool {
        let Some(layout) = self.enhanced_layout(window) else {
            return false;
        };
        let stale = self.enhanced_dirty
            || self
                .shown
                .as_ref()
                .is_none_or(|shown| shown.layout != layout);
        if stale {
            let Some(preview) = &mut self.session else {
                return false;
            };
            match preview.enhanced_frame(&layout) {
                Some(Ok(frame)) => {
                    let sides = frame
                        .side_panel
                        .as_ref()
                        .map(|panel| side_tiles(&layout, panel))
                        .unwrap_or_default();
                    let bezel = if frame.bezel {
                        bezel_layer(layout.surface, layout.ui_rect, &self.palette())
                    } else {
                        None
                    };
                    self.shown = Some(EnhancedShown {
                        layout,
                        frame,
                        sides,
                        bezel,
                    });
                    self.enhanced_dirty = false;
                }
                Some(Err(error)) => {
                    self.fail(event_loop, error);
                    return false;
                }
                None => {
                    self.shown = None;
                    self.enhanced_dirty = false;
                    match preview.image() {
                        Ok(image) => {
                            let palette = self.palette();
                            self.show_frame(image, &palette);
                        }
                        Err(error) => self.fail(event_loop, error),
                    }
                    return false;
                }
            }
        }
        let palette = self.palette();
        let plate = self.toast_plate(&palette);
        let (Some(shown), Some(preview)) = (&self.shown, &self.session) else {
            return false;
        };
        if let Some(renderer) = &mut self.renderer {
            let mut layers = enhanced_layers(shown, self.pointer);
            if let Some(plate) = &plate {
                // Over the interface, under the pointer's art.
                let cursor = shown.frame.cursor.is_some() && self.pointer.is_some();
                let at = layers.len() - usize::from(cursor);
                layers.insert(at, toast_layer(plate, shown));
            }
            renderer.set_layers(&layers, &palette);
        }
        // The frame draws its own pointer art: the OS pointer hides then.
        window.set_cursor_visible(shown.frame.cursor.is_none() && preview.system_cursor());
        true
    }
    /// The widescreen bezel under the single 640x480 frame of a
    /// full-screen game screen (the main menu, character selection and
    /// editor; `AssetPreview::bezel_screen`) while the session shows the
    /// unlocked widescreen view (the enhanced preset's, or the touch UI's
    /// navigation-only view). The frame's viewport, and with it every
    /// pointer mapping, stays as it is: the bezel only fills the
    /// letterbox. Vanilla, the fixed 640x480 view, the launcher and movies
    /// keep the plain clear color.
    pub(super) fn present_backdrop(&mut self) {
        let widescreen = self
            .enhanced
            .is_some_and(|settings| settings.viewport == enhanced::ViewportMode::Unlocked);
        let wanted = widescreen
            && self.launcher.is_none()
            && self.boot.is_none()
            && self
                .session
                .as_mut()
                .is_some_and(|session| session.bezel_screen());
        let palette = self.palette();
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        let viewport = renderer
            .viewport()
            .filter(|viewport| wanted && viewport.image == vault_render::bezel::FRAME);
        let key = viewport
            .zip(renderer.surface_size())
            .map(|(viewport, surface)| {
                (
                    viewport,
                    surface,
                    vault_render::bezel::BezelInks::nearest(&palette),
                )
            });
        if key == self.backdrop_key {
            return;
        }
        self.backdrop_key = key;
        let layer = key.and_then(|(viewport, surface, _)| {
            let rect = [
                viewport.left as f32,
                viewport.top as f32,
                viewport.width as f32,
                viewport.height as f32,
            ];
            bezel_layer(surface, rect, &palette)
        });
        renderer.set_backdrop(layer.as_ref().map(|(image, rect)| Layer {
            image,
            rect: *rect,
            transparent: None,
            sharp: true,
        }));
    }
    /// The display palette the frame is drawn through.
    pub(super) fn palette(&self) -> [[u8; 4]; 256] {
        let Some(preview) = &self.session else {
            return [[0, 0, 0, 255]; 256];
        };
        preview.display_override().unwrap_or_else(|| {
            self.display
                .as_ref()
                .map_or_else(|| preview.palette().rgba8(), |display| display.entries)
        })
    }
    /// A mouse button in enhanced routing: a movie takes any button, else
    /// the left, right and middle ones reach the world in both layers'
    /// coordinates (extra buttons do nothing). Returns `false`, leaving
    /// the button to the Vanilla routing, outside the enhanced viewport.
    pub(super) fn enhanced_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        button: MouseButton,
        pressed: bool,
    ) -> bool {
        let Some(layout) = self.enhanced_layout(window) else {
            return false;
        };
        let at = replay::pointer_at(&layout, self.pointer);
        let Some(preview) = &mut self.session else {
            return true;
        };
        let button = match button {
            MouseButton::Left => enhanced::PointerButton::Left,
            MouseButton::Right => enhanced::PointerButton::Right,
            MouseButton::Middle => enhanced::PointerButton::Middle,
            _ => {
                if preview.movie_button(pressed) {
                    self.refresh_preview(event_loop, window);
                }
                return true;
            }
        };
        if preview.movie_button(pressed) || preview.enhanced_button(at, button, pressed) {
            self.refresh_preview(event_loop, window);
        }
        if !pressed {
            self.warp_pointer(window);
        }
        true
    }
    /// Alt+V: the other viewport, applied, logged and drawn.
    fn toggle_viewport(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let Some(session) = self.enhanced else {
            return;
        };
        let current = self
            .session
            .as_mut()
            .and_then(|session| session.enhanced_settings())
            .unwrap_or(session);
        let settings = replay::toggled_viewport(current);
        tracing::info!(
            viewport = config::viewport_name(settings.viewport),
            "viewport changed"
        );
        self.enhanced = Some(settings);
        self.log(
            event_loop,
            InputCommand::Enhanced {
                settings: settings.into(),
            },
        );
        if let Some(preview) = &mut self.session {
            preview.set_enhanced(Some(settings));
        }
        self.refresh_preview(event_loop, window);
    }
    /// The drawn cursor moved: the next redraw presents it.
    pub(super) fn refresh_pointer(&mut self, window: &Window) {
        if self
            .session
            .as_ref()
            .is_some_and(|preview| !preview.system_cursor())
        {
            self.pointer_moved = true;
            window.request_redraw();
        }
    }
    /// Present the drawn cursor's move without composing the scene again.
    pub(super) fn present_pointer(&mut self) {
        let Some(preview) = &mut self.session else {
            return;
        };
        let Some(image) = preview.pointer_frame() else {
            return;
        };
        let palette = self.palette();
        self.show_frame(image, &palette);
    }
    /// G-31 / SPEC-014 §3.28: the action menu's release moves the system
    /// pointer to the frame pixel the preview asks for (where supported).
    pub(super) fn warp_pointer(&mut self, window: &Window) {
        let Some(pixel) = self
            .session
            .as_mut()
            .and_then(|session| session.take_warp())
        else {
            return;
        };
        // The renderer's own viewport: the warp lands on the surface pixel
        // that shows the frame pixel, never in the letterbox. In the
        // enhanced presentation the pixel is an interface pixel.
        let target = match self.enhanced_layout(window) {
            Some(layout) => {
                let [x, y, _, _] = layout.ui_rect.map(f64::from);
                let scale = f64::from(layout.ui_scale);
                let center = |origin: f64, pixel: i32, size: u32| {
                    (origin + (f64::from(pixel.clamp(0, size as i32 - 1)) + 0.5) * scale).floor()
                };
                Some([
                    center(x, pixel[0], EnhancedLayout::UI_FRAME[0]),
                    center(y, pixel[1], EnhancedLayout::UI_FRAME[1]),
                ])
            }
            None => self
                .renderer
                .as_ref()
                .and_then(|renderer| renderer.surface_point(pixel)),
        };
        let Some([x, y]) = target else {
            return;
        };
        match window.set_cursor_position(winit::dpi::PhysicalPosition::new(x, y)) {
            Ok(()) => self.virtual_pointer.anchor([x, y]),
            Err(error) => tracing::debug!(%error, "pointer warp unsupported"),
        }
    }
    /// The OS pointer moved to `os`: the game's pointer, scaled by the
    /// applied mouse sensitivity while the software cursor is drawn. The
    /// hidden OS pointer is warped after it, so it cannot reach the window
    /// edge first; where warps are unsupported OS positions stay unscaled.
    pub(super) fn scaled_pointer(&mut self, window: &Window, os: [f64; 2]) -> [f64; 2] {
        let (scale, software) = self.session.as_mut().map_or((1.0, false), |preview| {
            (preview.mouse_sensitivity(), !preview.system_cursor())
        });
        let scale = if self.warp_unsupported { 1.0 } else { scale };
        let size = window.inner_size();
        let surface = [f64::from(size.width), f64::from(size.height)];
        let point = self.virtual_pointer.moved(os, scale, software, surface);
        let Some(target) = self.virtual_pointer.drifted() else {
            return point;
        };
        match window.set_cursor_position(winit::dpi::PhysicalPosition::new(target[0], target[1])) {
            Ok(()) => {
                self.virtual_pointer.anchor(target);
                point
            }
            Err(error) => {
                tracing::debug!(%error, "pointer warp unsupported: mouse sensitivity off");
                self.warp_unsupported = true;
                self.virtual_pointer.anchor(os);
                os
            }
        }
    }
    /// F11 and Alt+Enter toggle borderless fullscreen, Alt+S the scaling
    /// mode, and in an enhanced session Alt+V the viewport. The shell
    /// keeps these keys: neither their presses (with the
    /// repeats) nor their releases reach the game or the input log.
    /// Returns whether the shell took the key.
    pub(super) fn shell_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> bool {
        if !pressed {
            let Some(at) = self.shell_keys.iter().position(|held| *held == key) else {
                return false;
            };
            self.shell_keys.swap_remove(at);
            return true;
        }
        if repeat {
            return self.shell_keys.contains(&key);
        }
        let hotkey = shell_hotkey(key, self.alt, self.enhanced.is_some())
            .or_else(|| qol_hotkey(key, self.alt, self.game_speed.is_some()));
        // No session, or a smoke test, has no warm switch (SPEC-020 §2.5).
        let switches = self.session.is_some() && !self.smoke_test;
        let Some(hotkey) = hotkey.filter(|hotkey| *hotkey != ShellHotkey::Switch || switches)
        else {
            return false;
        };
        if !self.shell_keys.contains(&key) {
            self.shell_keys.push(key);
        }
        // The session under the selector takes no key of its own.
        if self.stopped.is_some()
            && matches!(hotkey, ShellHotkey::Viewport | ShellHotkey::GameSpeed)
        {
            return true;
        }
        match hotkey {
            ShellHotkey::Switch => {
                self.switch_key(event_loop, window);
                return true;
            }
            ShellHotkey::Fullscreen => {
                let fullscreen = window.fullscreen().is_none();
                window.set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
                tracing::info!(fullscreen, "fullscreen toggled");
            }
            ShellHotkey::Scaling => {
                self.scaling = self.scaling.toggled();
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_scaling(self.scaling);
                }
                tracing::info!(
                    scaling = config::scaling_name(self.scaling),
                    "scaling mode changed"
                );
                self.log_scaling();
            }
            ShellHotkey::Viewport => self.toggle_viewport(event_loop, window),
            ShellHotkey::GameSpeed => {
                let speed = self.game_speed.unwrap_or_default().next();
                tracing::info!(speed = speed.label(), "game speed changed");
                self.apply_game_speed(speed);
                self.refresh_preview(event_loop, window);
            }
        }
        self.viewport_changed(event_loop, window);
        true
    }
    /// M-P1: the frame's viewport changed (a resize, or a scaling mode or
    /// fullscreen change): the stored pointer maps to another frame pixel,
    /// so hover runs again and the drawn cursor follows, and the next click
    /// acts where the cursor shows.
    pub(super) fn viewport_changed(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        if self.boot.is_some() {
            return self.refresh_boot(window);
        }
        // The launcher from Setup, over a session that is still stopped.
        if self.stopped.is_some() {
            return self.refresh_launcher(window);
        }
        if let Some(pointer) = self.pointer {
            self.log_hover(event_loop, pointer);
            self.refresh_pointer(window);
        }
        window.request_redraw();
    }
    /// The frame pixel under the pointer, or `None` outside the frame.
    pub(super) fn pixel(&self) -> Option<[i32; 2]> {
        self.pointer
            .and_then(|point| self.renderer.as_ref()?.image_position(point))
    }
    /// H3: the pointer's frame pixel, a pointer in the letterbox clamped to
    /// the nearest frame pixel, as hover sees it.
    pub(super) fn clamped_pixel(&self) -> Option<[i32; 2]> {
        self.pointer
            .and_then(|point| self.renderer.as_ref()?.frame_point(point))
            .map(|point| point.pixel)
    }

    /// The pointer's frame pixel reaches the preview, clamped to the frame
    /// so its edge pixels stay reachable (SPEC-014 §3.29 edge scrolling),
    /// with whether it is outside the frame (H3: never an edge).
    pub(super) fn log_hover(&mut self, event_loop: &ActiveEventLoop, point: [f64; 2]) {
        // In the enhanced presentation the pointer reaches the world in
        // both layers' coordinates (no hover record: the replay recomputes
        // it), and the next redraw moves the cursor layer.
        if let Some(window) = self.window.clone()
            && let Some(layout) = self.enhanced_layout(&window)
        {
            let at = replay::pointer_at(&layout, Some(point));
            if self
                .session
                .as_mut()
                .is_some_and(|preview| preview.enhanced_pointer(at))
            {
                self.enhanced_dirty = true;
            }
            window.request_redraw();
            return;
        }
        let hover = self
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.frame_point(point))
            .and_then(|point| self.session.as_mut()?.pointer_moved(point));
        if let Some(hover) = hover {
            self.log(
                event_loop,
                InputCommand::MapHover {
                    elevation: hover.elevation,
                    hex: hover.hex,
                    floor_square: hover.floor,
                    roof_square: hover.roof,
                },
            );
        }
    }
}
