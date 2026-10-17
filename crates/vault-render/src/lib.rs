//! Minimal indexed-color rendering, independent of the windowing/event loop.

use wgpu::util::{DeviceExt, TextureDataOrder};

pub mod dialogue;
pub mod image;
pub mod map;

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
    viewport_buffer: wgpu::Buffer,
    image_size: [u32; 2],
    drawable: bool,
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
        let bind_group =
            image_bind_group(&device, &queue, &pipeline, &viewport_buffer, image, palette);
        let mut renderer = Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            viewport_buffer,
            image_size: [image.width(), image.height()],
            drawable: false,
        };
        renderer.resize(width, height);
        Ok(renderer)
    }

    /// Zero-sized/minimized windows are never configured or drawn.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.drawable = width != 0 && height != 0;
        if !self.drawable {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        // Integer pixel scaling when it fits, otherwise aspect-preserving downscale.
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

    pub fn set_image(&mut self, image: &IndexedImage, palette: &[[u8; 4]; 256]) {
        self.bind_group = image_bind_group(
            &self.device,
            &self.queue,
            &self.pipeline,
            &self.viewport_buffer,
            image,
            palette,
        );
        self.image_size = [image.width(), image.height()];
        if self.drawable {
            self.resize(self.config.width, self.config.height);
        }
    }

    /// Inverse of the centered image quad, for pointer picking in scene pixels.
    pub fn image_position(&self, point: [f64; 2]) -> Option<[i32; 2]> {
        if !self.drawable || point.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let available = (self.config.width as f64 / self.image_size[0] as f64)
            .min(self.config.height as f64 / self.image_size[1] as f64)
            * 0.75;
        let scale = if available >= 1.0 {
            available.floor()
        } else {
            available
        };
        let left = (self.config.width as f64 - self.image_size[0] as f64 * scale) / 2.0;
        let top = (self.config.height as f64 - self.image_size[1] as f64 * scale) / 2.0;
        let result = [
            ((point[0] - left) / scale).floor(),
            ((point[1] - top) / scale).floor(),
        ];
        (result[0] >= 0.0
            && result[1] >= 0.0
            && result[0] < self.image_size[0] as f64
            && result[1] < self.image_size[1] as f64)
            .then_some(result.map(|value| value as i32))
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
    queue: &wgpu::Queue,
    pipeline: &wgpu::RenderPipeline,
    viewport: &wgpu::Buffer,
    image: &IndexedImage,
    rgba: &[[u8; 4]; 256],
) -> wgpu::BindGroup {
    let sprite = texture(
        device,
        queue,
        "sprite indices",
        image.width(),
        image.height(),
        wgpu::TextureFormat::R8Uint,
        image.pixels(),
    );
    let palette = texture(
        device,
        queue,
        "256-color palette",
        256,
        1,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        bytemuck::cast_slice(rgba),
    );
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

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    bytes: &[u8],
) -> wgpu::Texture {
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        bytes,
    )
}
