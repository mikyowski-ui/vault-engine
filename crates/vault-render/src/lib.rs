//! Minimal indexed-color rendering, independent of the windowing/event loop.

use wgpu::util::DeviceExt;

pub mod dialogue;
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
    viewport_buffer: wgpu::Buffer,
    image_size: [u32; 2],
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
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite viewport"),
            contents: bytemuck::cast_slice(&[1.0_f32, 1.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sprite = sprite_texture(&device, [image.width(), image.height()]);
        let palette_texture = texture(
            &device,
            "256-color palette",
            [256, 1],
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let bind_group = image_bind_group(
            &device,
            &pipeline,
            &viewport_buffer,
            &sprite,
            &palette_texture,
        );
        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            sprite,
            palette: palette_texture,
            viewport_buffer,
            image_size: [image.width(), image.height()],
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

    /// Integer pixel scaling when it fits, otherwise aspect-preserving
    /// downscale, for the current image and surface sizes.
    fn update_viewport(&mut self) {
        let [width, height] = [self.config.width, self.config.height];
        let available_scale = (width as f32 / self.image_size[0] as f32)
            .min(height as f32 / self.image_size[1] as f32)
            * 0.75;
        let scale = if available_scale >= 1.0 {
            available_scale.floor()
        } else {
            available_scale
        };
        self.queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::cast_slice(&[
                scale * self.image_size[0] as f32 / width as f32,
                scale * self.image_size[1] as f32 / height as f32,
                0.0,
                0.0,
            ]),
        );
    }

    /// Present a new index image and palette. An image of the same size is
    /// written into the existing texture (P-01: no texture, bind group or
    /// swapchain is rebuilt); a new size recreates the sprite texture and
    /// its bind group and rescales the quad.
    pub fn set_image(&mut self, image: &IndexedImage, palette: &[[u8; 4]; 256]) {
        let size = [image.width(), image.height()];
        if size != self.image_size {
            self.sprite = sprite_texture(&self.device, size);
            self.bind_group = image_bind_group(
                &self.device,
                &self.pipeline,
                &self.viewport_buffer,
                &self.sprite,
                &self.palette,
            );
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

    /// Inverse of the centered image quad, for pointer picking in scene pixels.
    pub fn image_position(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        if !self.drawable {
            return None;
        }
        image_position(
            [self.config.width, self.config.height],
            self.image_size,
            point,
        )
    }

    /// [`image_position_clamped`] for this surface: the pointer outside the
    /// frame maps to the nearest edge pixel.
    pub fn image_position_clamped(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        if !self.drawable {
            return None;
        }
        image_position_clamped(
            [self.config.width, self.config.height],
            self.image_size,
            point,
        )
    }

    /// Return true only when a frame was submitted and presented.
    /// Recoverable surface errors request a retry through a false result.
    /// Call `before_present` only for a completed frame, allowing the platform
    /// to notify its compositor without scheduling callbacks for skipped frames.
    pub fn render(&mut self, before_present: impl FnOnce()) -> Result<bool, wgpu::SurfaceError> {
        if !self.drawable {
            return Ok(false);
        }
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
    viewport: &wgpu::Buffer,
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: viewport.as_entire_binding(),
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

/// The image pixel under a point of a `surface`-sized window presenting an
/// `image`-sized frame centered at the renderer's scale (integer when it
/// fits in three quarters of the surface, otherwise an aspect-preserving
/// downscale); `None` outside the frame. Headless replays use this to map
/// logged pointer positions exactly as the desktop does.
pub fn image_position(surface: [u32; 2], image: [u32; 2], point: [f64; 2]) -> Option<[i32; 2]> {
    let result = unclamped_position(surface, image, point)?;
    (result[0] >= 0.0
        && result[1] >= 0.0
        && result[0] < image[0] as f64
        && result[1] < image[1] as f64)
        .then_some(result.map(|value| value as i32))
}

/// [`image_position`] with a pointer outside the frame (in the letterbox)
/// clamped to the nearest frame pixel, so the frame's edge pixels stay
/// reachable for edge scrolling.
pub fn image_position_clamped(
    surface: [u32; 2],
    image: [u32; 2],
    point: [f64; 2],
) -> Option<[i32; 2]> {
    let result = unclamped_position(surface, image, point)?;
    Some([0, 1].map(|axis| result[axis].clamp(0.0, image[axis] as f64 - 1.0) as i32))
}

fn unclamped_position(surface: [u32; 2], image: [u32; 2], point: [f64; 2]) -> Option<[f64; 2]> {
    if surface.contains(&0) || image.contains(&0) || point.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let available =
        (surface[0] as f64 / image[0] as f64).min(surface[1] as f64 / image[1] as f64) * 0.75;
    let scale = if available >= 1.0 {
        available.floor()
    } else {
        available
    };
    let left = (surface[0] as f64 - image[0] as f64 * scale) / 2.0;
    let top = (surface[1] as f64 - image[1] as f64 * scale) / 2.0;
    Some([
        ((point[0] - left) / scale).floor(),
        ((point[1] - top) / scale).floor(),
    ])
}

#[cfg(test)]
mod tests {
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
        // 2560 x 1600: integer scale 2, inset 640 x 320.
        assert_eq!(
            super::image_position([2560, 1600], [640, 480], [643.0, 325.0]),
            Some([1, 2])
        );
        assert_eq!(
            super::image_position([0, 640], [640, 480], [1.0, 1.0]),
            None
        );
    }
}
