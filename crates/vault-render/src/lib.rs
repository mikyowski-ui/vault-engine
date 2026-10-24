//! Minimal indexed-color rendering, independent of the windowing/event loop.

pub mod dialogue;
pub mod floating;
pub mod image;
pub mod map;
pub mod movie;
pub mod outline;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
pub use image::{ImageError, IndexedImage, TextStyle, compose_preview};

const SPRITE_SIDE: u32 = 16;

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("cannot create a window surface: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("no compatible graphics adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("cannot create a graphics device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("adapter has no supported surface configuration")]
    SurfaceConfiguration,
}

/// An R8Uint sprite and a separate 256-entry sRGB palette, resolved in WGSL.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    /// P-01: the index image and the palette live in persistent textures
    /// that new frames overwrite in place; the sprite texture (and its bind
    /// group) is recreated only when the image size changes.
    sprite: wgpu::Texture,
    palette: wgpu::Texture,
    image_size: [u32; 2],
    /// H2/M-R1: the integer-snapped rectangle the frame is drawn into;
    /// pointer mapping and warps use the same one.
    viewport: Option<Viewport>,
    drawable: bool,
    /// The surface size last configured; the surface is reconfigured only
    /// when the window size changes (or after a lost surface).
    configured: Option<[u32; 2]>,
}

impl Renderer {
    /// Pass an owned window handle (for example `Arc<Window>`) so the surface
    /// retains its window for its entire lifetime without unsafe code.
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, RendererError> {
        let indices = include_str!("../../../tests/fixtures/render/indices.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("committed synthetic sprite"))
            .collect();
        let image = IndexedImage::from_pixels(SPRITE_SIDE, SPRITE_SIDE, indices)
            .expect("synthetic image dimensions");
        let palette = std::array::from_fn(|index| {
            let index = index as u16;
            [
                (((index >> 5) & 7) * 255 / 7) as u8,
                (((index >> 2) & 7) * 255 / 7) as u8,
                ((index & 3) * 255 / 3) as u8,
                255,
            ]
        });
        Self::new_with_image(target, width, height, &image, &palette).await
    }

    pub async fn new_with_image(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        image: &IndexedImage,
        palette: &[[u8; 4]; 256],
    ) -> Result<Self, RendererError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance.create_surface(target)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;
        let info = adapter.get_info();
        tracing::info!(adapter = %info.name, backend = ?info.backend, "graphics adapter selected");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("vault device"),
                ..Default::default()
            })
            .await?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or(RendererError::SurfaceConfiguration)?;
        if let Some(format) = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(wgpu::TextureFormat::is_srgb)
        {
            config.format = format;
        }
        config.present_mode = wgpu::PresentMode::Fifo;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("indexed sprite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("indexed.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("indexed sprite pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let sprite = sprite_texture(&device, [image.width(), image.height()]);
        let palette_texture = texture(
            &device,
            "256-color palette",
            [256, 1],
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let bind_group = image_bind_group(&device, &pipeline, &sprite, &palette_texture);
        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            sprite,
            palette: palette_texture,
            image_size: [image.width(), image.height()],
            viewport: None,
            drawable: false,
            configured: None,
        };
        renderer.write_indices(image);
        renderer.set_palette(palette);
        renderer.resize(width, height);
        Ok(renderer)
    }

    /// Replace display-palette entries (for example palette cycling) in place,
    /// leaving the uploaded index image untouched.
    pub fn set_palette(&mut self, palette: &[[u8; 4]; 256]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.palette,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(palette),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Zero-sized/minimized windows are never configured or drawn. The
    /// surface is configured only when its size changes (P-01).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.drawable = width != 0 && height != 0;
        if !self.drawable {
            return;
        }
        if self.configured != Some([width, height]) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.configured = Some([width, height]);
        }
        self.update_viewport();
    }

    /// The frame's [`Viewport`] for the current image and surface sizes.
    fn update_viewport(&mut self) {
        self.viewport = Viewport::fit([self.config.width, self.config.height], self.image_size);
    }

    /// The rectangle the frame is drawn into, while the surface is drawable.
    pub fn viewport(&self) -> Option<Viewport> {
        self.viewport.filter(|_| self.drawable)
    }

    /// Present a new index image and palette. An image of the same size is
    /// written into the existing texture (P-01: no texture, bind group or
    /// swapchain is rebuilt); a new size recreates the sprite texture and
    /// its bind group and rescales the quad.
    pub fn set_image(&mut self, image: &IndexedImage, palette: &[[u8; 4]; 256]) {
        let size = [image.width(), image.height()];
        if size != self.image_size {
            self.sprite = sprite_texture(&self.device, size);
            self.bind_group =
                image_bind_group(&self.device, &self.pipeline, &self.sprite, &self.palette);
            self.image_size = size;
            if self.drawable {
                self.update_viewport();
            }
        }
        self.write_indices(image);
        self.set_palette(palette);
    }

    fn write_indices(&self, image: &IndexedImage) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.sprite,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width()),
                rows_per_image: Some(image.height()),
            },
            wgpu::Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
        );
    }

    /// The frame pixel under a surface point, for pointer picking; `None`
    /// outside the frame.
    pub fn image_position(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        self.viewport()?.position(point)
    }

    /// The frame pixel under a surface point, a pointer outside the frame
    /// clamped to the nearest frame pixel, with whether it was outside.
    pub fn frame_point(&self, point: [f64; 2]) -> Option<FramePoint> {
        self.viewport()?.clamped(point)
    }

    /// The surface point a pointer warp to frame pixel `pixel` goes to.
    pub fn surface_point(&self, pixel: [i32; 2]) -> Option<[f64; 2]> {
        Some(self.viewport()?.surface_point(pixel))
    }

    /// Return true only when a frame was submitted and presented.
    /// Recoverable surface errors request a retry through a false result.
    /// Call `before_present` only for a completed frame, allowing the platform
    /// to notify its compositor without scheduling callbacks for skipped frames.
    pub fn render(&mut self, before_present: impl FnOnce()) -> Result<bool, wgpu::SurfaceError> {
        let Some(viewport) = self.viewport() else {
            return Ok(false);
        };
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => {
                tracing::warn!("surface acquisition timed out");
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sprite frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("indexed sprite pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.015,
                            g: 0.02,
                            b: 0.025,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_viewport(
                viewport.left as f32,
                viewport.top as f32,
                viewport.width as f32,
                viewport.height as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        before_present();
        frame.present();
        Ok(true)
    }
}

fn image_bind_group(
    device: &wgpu::Device,
    pipeline: &wgpu::RenderPipeline,
    sprite: &wgpu::Texture,
    palette: &wgpu::Texture,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("indexed sprite resources"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &sprite.create_view(&Default::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(
                    &palette.create_view(&Default::default()),
                ),
            },
        ],
    })
}

fn sprite_texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    texture(device, "sprite indices", size, wgpu::TextureFormat::R8Uint)
}

/// A sampled texture written through the queue.
fn texture(
    device: &wgpu::Device,
    label: &str,
    [width, height]: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The surface rectangle the frame is drawn into, in whole surface pixels.
/// One rectangle serves drawing (the render pass viewport), pointer
/// mapping, the clamped mapping, pointer warps and headless replays, so a
/// pointer always maps to the texel drawn under it.
///
/// The scale is the largest integer `floor(min(W / w, H / h))` when that is
/// at least 1; only a surface smaller than the frame on some axis gets an
/// aspect-preserving fractional downscale. The rectangle is centered, its
/// left and top margins rounded down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    /// The frame size.
    pub image: [u32; 2],
}

/// A pointer mapped to the frame: the pixel under it, or the nearest frame
/// pixel when the pointer is outside the frame (in the letterbox), and
/// whether it was outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePoint {
    pub pixel: [i32; 2],
    pub outside: bool,
}

impl Viewport {
    /// The rectangle for an `image`-sized frame on a `surface`-sized window;
    /// `None` when either is empty.
    pub fn fit(surface: [u32; 2], image: [u32; 2]) -> Option<Self> {
        if surface.contains(&0) || image.contains(&0) {
            return None;
        }
        let [sw, sh] = surface.map(u64::from);
        let [iw, ih] = image.map(u64::from);
        let scale = (sw / iw).min(sh / ih);
        let (width, height) = if scale >= 1 {
            (iw * scale, ih * scale)
        } else if sw * ih <= sh * iw {
            // The width limits: the frame spans the surface's width.
            (sw, (ih * sw / iw).max(1))
        } else {
            ((iw * sh / ih).max(1), sh)
        };
        Some(Self {
            left: ((sw - width) / 2) as u32,
            top: ((sh - height) / 2) as u32,
            width: width as u32,
            height: height as u32,
            image,
        })
    }
    fn origin(self) -> [i64; 2] {
        [i64::from(self.left), i64::from(self.top)]
    }
    fn size(self) -> [i64; 2] {
        [i64::from(self.width), i64::from(self.height)]
    }
    /// The surface pixel holding `point` (a physical position).
    fn surface_pixel(point: [f64; 2]) -> Option<[i64; 2]> {
        point
            .iter()
            .all(|value| value.is_finite())
            .then(|| point.map(|value| value.floor().clamp(-1e15, 1e15) as i64))
    }
    /// The frame pixel a surface pixel inside the rectangle shows: the
    /// texel sampled at the surface pixel's center, as the shader samples
    /// it, so odd margins never shift the pointer by one.
    fn texel(self, pixel: [i64; 2]) -> [i32; 2] {
        let (origin, size) = (self.origin(), self.size());
        std::array::from_fn(|axis| {
            let image = i64::from(self.image[axis]);
            // floor((q - origin + 0.5) * image / size), in integers.
            let texel = ((2 * (pixel[axis] - origin[axis]) + 1) * image).div_euclid(2 * size[axis]);
            texel.clamp(0, image - 1) as i32
        })
    }
    fn contains(self, pixel: [i64; 2]) -> bool {
        let (origin, size) = (self.origin(), self.size());
        (0..2).all(|axis| (origin[axis]..origin[axis] + size[axis]).contains(&pixel[axis]))
    }
    /// The frame pixel under `point`, or `None` outside the frame.
    pub fn position(self, point: [f64; 2]) -> Option<[i32; 2]> {
        let pixel = Self::surface_pixel(point)?;
        self.contains(pixel).then(|| self.texel(pixel))
    }
    /// The frame pixel under `point`, a pointer outside the frame clamped
    /// to the nearest frame pixel, and whether it was outside.
    pub fn clamped(self, point: [f64; 2]) -> Option<FramePoint> {
        let pixel = Self::surface_pixel(point)?;
        let (origin, size) = (self.origin(), self.size());
        let inside = std::array::from_fn(|axis| {
            pixel[axis].clamp(origin[axis], origin[axis] + size[axis] - 1)
        });
        Some(FramePoint {
            pixel: self.texel(inside),
            outside: !self.contains(pixel),
        })
    }
    /// A surface point that maps back to frame pixel `pixel` (clamped to
    /// the frame): the surface pixel under the texel's center, always inside
    /// the rectangle, so a warp to an edge pixel never lands in the
    /// letterbox.
    pub fn surface_point(self, pixel: [i32; 2]) -> [f64; 2] {
        let (origin, size) = (self.origin(), self.size());
        std::array::from_fn(|axis| {
            let image = i64::from(self.image[axis]);
            let texel = i64::from(pixel[axis]).clamp(0, image - 1);
            let mut q = ((2 * texel + 1) * size[axis]).div_euclid(2 * image);
            q = q.clamp(0, size[axis] - 1);
            // A downscaled frame skips texels: the pixel before may still
            // sample this one.
            let samples = |q: i64| ((2 * q + 1) * image).div_euclid(2 * size[axis]);
            if samples(q) > texel && q > 0 && samples(q - 1) == texel {
                q -= 1;
            }
            (origin[axis] + q) as f64
        })
    }
}

/// The image pixel under a point of a `surface`-sized window presenting an
/// `image`-sized frame in its [`Viewport`]; `None` outside the frame.
/// Headless replays use this to map logged pointer positions exactly as
/// the desktop does.
pub fn image_position(surface: [u32; 2], image: [u32; 2], point: [f64; 2]) -> Option<[i32; 2]> {
    Viewport::fit(surface, image)?.position(point)
}

/// [`image_position`] with a pointer outside the frame (in the letterbox)
/// clamped to the nearest frame pixel, so the frame's edge pixels stay
/// reachable.
pub fn image_position_clamped(
    surface: [u32; 2],
    image: [u32; 2],
    point: [f64; 2],
) -> Option<[i32; 2]> {
    frame_point(surface, image, point).map(|point| point.pixel)
}

/// The clamped frame pixel under `point`, and whether the pointer was
/// outside the frame.
pub fn frame_point(surface: [u32; 2], image: [u32; 2], point: [f64; 2]) -> Option<FramePoint> {
    Viewport::fit(surface, image)?.clamped(point)
}

#[cfg(test)]
mod tests {
    use super::Viewport;

    #[test]
    fn image_position_inverts_the_centered_scaled_quad() {
        // 960 x 640 holds a 640 x 480 frame at scale 1, inset 160 x 80.
        let position = |point| super::image_position([960, 640], [640, 480], point);
        assert_eq!(position([160.0, 80.0]), Some([0, 0]));
        assert_eq!(position([799.9, 559.9]), Some([639, 479]));
        assert_eq!(position([159.9, 80.0]), None);
        assert_eq!(position([800.0, 80.0]), None);
        assert_eq!(position([f64::NAN, 80.0]), None);
        let clamped = |point| super::image_position_clamped([960, 640], [640, 480], point);
        assert_eq!(clamped([0.0, 0.0]), Some([0, 0]));
        assert_eq!(clamped([959.0, 300.0]), Some([639, 220]));
        assert_eq!(clamped([500.0, 639.0]), Some([340, 479]));
        assert_eq!(clamped([f64::NAN, 80.0]), None);
        let framed = |point| super::frame_point([960, 640], [640, 480], point).unwrap();
        assert!(framed([0.0, 0.0]).outside);
        assert!(!framed([160.0, 80.0]).outside);
        assert!(framed([800.0, 300.0]).outside);
        // 2560 x 1600: the largest integer scale, 3, inset 320 x 80.
        assert_eq!(
            super::image_position([2560, 1600], [640, 480], [323.0, 86.0]),
            Some([1, 2])
        );
        assert_eq!(
            super::image_position([0, 640], [640, 480], [1.0, 1.0]),
            None
        );
    }

    #[test]
    fn the_viewport_takes_the_largest_integer_scale_and_downscales_only_below_the_frame() {
        let fit = |surface| Viewport::fit(surface, [640, 480]).unwrap();
        let rect = |v: Viewport| [v.left, v.top, v.width, v.height];
        // H2: no three-quarter factor; the frame fills the window when it
        // fits at an integer scale.
        assert_eq!(rect(fit([640, 480])), [0, 0, 640, 480]);
        assert_eq!(rect(fit([800, 600])), [80, 60, 640, 480]);
        assert_eq!(rect(fit([1280, 960])), [0, 0, 1280, 960]);
        assert_eq!(rect(fit([1920, 1080])), [320, 60, 1280, 960]);
        assert_eq!(rect(fit([2560, 1440])), [320, 0, 1920, 1440]);
        // Smaller than the frame: an aspect-preserving fractional scale.
        assert_eq!(rect(fit([320, 480])), [0, 120, 320, 240]);
        assert_eq!(rect(fit([600, 400])), [33, 0, 533, 400]);
        assert_eq!(Viewport::fit([0, 480], [640, 480]), None);
    }

    #[test]
    fn odd_margins_map_every_column_to_its_drawn_texel() {
        // M-R1: 1001 x 777 has odd margins; the rectangle is integer, so the
        // pointer never lands one pixel off.
        let view = Viewport::fit([1001, 777], [640, 480]).unwrap();
        assert_eq!(
            [view.left, view.top, view.width, view.height],
            [180, 148, 640, 480]
        );
        for x in 0..640 {
            let surface = f64::from(180 + x);
            assert_eq!(view.position([surface + 0.75, 148.0]), Some([x, 0]));
        }
        assert_eq!(view.position([179.99, 148.0]), None);
        assert_eq!(view.position([820.0, 148.0]), None);
        // Scale 2 with odd margins: each texel is two surface pixels.
        let view = Viewport::fit([1281, 961], [640, 480]).unwrap();
        assert_eq!([view.left, view.top], [0, 0]);
        assert_eq!(view.position([1279.5, 959.5]), Some([639, 479]));
        assert_eq!(view.position([1280.0, 0.0]), None);
    }

    #[test]
    fn warps_land_inside_the_frame_on_the_texel_asked_for() {
        for surface in [
            [960, 640],
            [1001, 777],
            [1920, 1080],
            [600, 400],
            [2560, 1600],
        ] {
            let view = Viewport::fit(surface, [640, 480]).unwrap();
            for pixel in [[0, 0], [639, 479], [320, 190], [1, 478], [638, 1]] {
                let point = view.surface_point(pixel);
                let mapped = view.clamped(point).unwrap();
                assert!(!mapped.outside, "{surface:?} {pixel:?} -> {point:?}");
                if view.width >= 640 {
                    assert_eq!(mapped.pixel, pixel, "{surface:?} {point:?}");
                } else {
                    // A downscale skips texels; the warp lands next to it.
                    for (mapped, asked) in mapped.pixel.into_iter().zip(pixel) {
                        assert!((mapped - asked).abs() <= 1);
                    }
                }
            }
        }
    }
}
