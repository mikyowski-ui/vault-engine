//! SPEC-022 §3.7 and §3.11 the GPU executor: a batch plan drawn into an
//! `R8Uint` canvas with one instanced draw per batch, read batches
//! through a snapshot copy, every value read with `textureLoad` at
//! integer coordinates (`instanced.wgsl`).
//!
//! `[May-improve]` choices: one atlas texture per refresh (shelf-packed,
//! holding the sprite, ring, text and level images the plan draws), the
//! lookup tables in one texture re-uploaded only when they change, and the
//! canvas uploaded before and read back after each refresh, so the
//! display stage stays the shared CPU one.
use super::instanced::{
    BatchKind, BatchPlan, FrameInputs, HEXES, InstanceError, LayerUniforms, LookupTables, Rect,
    SpriteInstance, intersect,
};
use crate::IndexedImage;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

/// How long a GPU wait may take before the refresh is refused.
const WAIT: Duration = Duration::from_secs(20);

/// Run a wgpu future to completion; native backends resolve them on
/// polling. Bounded by [`WAIT`] (§3.12: no unbounded loop).
fn block_on<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    let start = Instant::now();
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return Some(output);
        }
        if start.elapsed() > WAIT {
            return None;
        }
        std::thread::yield_now();
    }
}

/// A headless wgpu device for the instanced path.
pub struct GpuDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Set by the device-lost callback.
    lost: Arc<AtomicBool>,
    /// The adapter's name and backend.
    adapter: String,
}

impl GpuDevice {
    /// A device on the first adapter wgpu finds (a software one if no
    /// other), with that adapter's limits. §3.12: no adapter or device is
    /// an error, never a panic.
    pub fn new() -> Result<Self, InstanceError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let request = |fallback| {
            block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: fallback,
            }))
            .and_then(Result::ok)
        };
        let adapter = request(false)
            .or_else(|| request(true))
            .ok_or_else(|| InstanceError::NoDevice("no adapter".into()))?;
        let info = adapter.get_info();
        let name = format!("{} ({:?})", info.name, info.backend);
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("vault instanced canvas"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok_or_else(|| InstanceError::NoDevice("device request timed out".into()))?
        .map_err(|error| InstanceError::NoDevice(error.to_string()))?;
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::SeqCst));
        // Errors are caught by error scopes; an uncaptured one marks the
        // device unusable instead of panicking.
        let flag = Arc::clone(&lost);
        device.on_uncaptured_error(Arc::new(move |_| flag.store(true, Ordering::SeqCst)));
        Ok(Self {
            device,
            queue,
            lost,
            adapter: name,
        })
    }
    /// The adapter's name and backend.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }
    /// Whether the device is lost.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }
    /// Destroy the device, as a lost device (test T12).
    pub fn destroy(&self) {
        self.device.destroy();
        self.lost.store(true, Ordering::SeqCst);
    }
}

/// The lock every GPU test holds while it creates, uses and drops its
/// devices: some drivers (the software Vulkan one among them) crash when
/// test threads create and destroy devices at once.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A GPU device for a GPU test (§4): `None`, after printing `skipped: no
/// GPU adapter`, when none can be created, unless `VAULT_REQUIRE_GPU` is
/// `1`, when the test fails. The caller holds [`test_lock`].
#[cfg(test)]
pub(crate) fn test_device() -> Option<GpuDevice> {
    match GpuDevice::new() {
        Ok(device) => {
            println!("GPU adapter: {}", device.adapter());
            Some(device)
        }
        Err(error) => {
            assert!(
                std::env::var("VAULT_REQUIRE_GPU").as_deref() != Ok("1"),
                "VAULT_REQUIRE_GPU=1 but {error}"
            );
            println!("skipped: no GPU adapter");
            None
        }
    }
}

/// The draw calls and snapshot copies of one GPU refresh.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuCounts {
    pub draws: usize,
    pub snapshots: usize,
}

/// The table texture's rows: intensity, mix, blends, gray levels.
const TABLE_ROWS: u32 = 256 + 256 + 40 + 2;
/// Words per instance record.
const RECORD_WORDS: usize = 12;

/// The GPU executor: its pipeline and the resources kept across
/// refreshes.
pub struct GpuExecutor {
    gpu: GpuDevice,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// The table texture and the tables it holds.
    tables: Option<(Box<LookupTables>, wgpu::Texture)>,
    /// The hex light texture and the values it holds.
    light: Option<(Vec<u32>, wgpu::Texture)>,
}

impl GpuExecutor {
    pub fn new(gpu: GpuDevice) -> Self {
        let device = &gpu.device;
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instanced canvas bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
                texture_entry(6),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("instanced canvas shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("instanced.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("instanced canvas layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("instanced canvas pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Uint,
                    blend: None,
                    write_mask: wgpu::ColorWrites::RED,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            gpu,
            layout,
            pipeline,
            tables: None,
            light: None,
        }
    }

    /// The device this executor draws with.
    pub fn device(&self) -> &GpuDevice {
        &self.gpu
    }

    fn max_side(&self) -> u32 {
        self.gpu.device.limits().max_texture_dimension_2d
    }

    fn texture(&self, label: &str, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn write(&self, texture: &wgpu::Texture, size: [u32; 2], bytes_per_texel: u32, data: &[u8]) {
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * bytes_per_texel),
                rows_per_image: Some(size[1]),
            },
            extent(size),
        );
    }

    /// Upload the tables when they differ from the last ones.
    fn upload_tables(&mut self, tables: &LookupTables) {
        if self
            .tables
            .as_ref()
            .is_some_and(|(held, _)| **held == *tables)
        {
            return;
        }
        let mut bytes = Vec::with_capacity(256 * TABLE_ROWS as usize);
        bytes.extend_from_slice(&tables.intensity);
        bytes.extend_from_slice(&tables.mix);
        for blend in &tables.blends {
            for row in blend.iter() {
                bytes.extend_from_slice(row);
            }
        }
        bytes.extend_from_slice(&tables.gray[0]);
        bytes.extend_from_slice(&tables.gray[1]);
        let texture = self.texture(
            "lookup tables",
            [256, TABLE_ROWS],
            wgpu::TextureFormat::R8Uint,
        );
        self.write(&texture, [256, TABLE_ROWS], 1, &bytes);
        self.tables = Some((Box::new(tables.clone()), texture));
    }

    /// Upload the hex light buffer when it differs from the last one
    /// (§3.8 step 4).
    fn upload_light(&mut self, values: &[u32]) {
        if self.light.as_ref().is_some_and(|(held, _)| held == values) {
            return;
        }
        let texture = self.texture("hex light", [200, 200], wgpu::TextureFormat::R32Uint);
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.write(&texture, [200, 200], 4, &bytes);
        self.light = Some((values.to_vec(), texture));
    }

    /// Draw `plan` over `canvas` on the GPU: the same canvas as
    /// [`super::execute_plan`]. A refused refresh leaves `canvas` as it
    /// was.
    pub fn execute(
        &mut self,
        canvas: &mut IndexedImage,
        instances: &[SpriteInstance],
        plan: &BatchPlan,
        inputs: &FrameInputs,
    ) -> Result<GpuCounts, InstanceError> {
        if self.gpu.is_lost() {
            return Err(InstanceError::DeviceLost);
        }
        let size = [canvas.width(), canvas.height()];
        if size != inputs.uniforms.canvas {
            return Err(InstanceError::TextureLimit(size[0], size[1]));
        }
        inputs.validate(instances)?;
        let max = self.max_side();
        if size.iter().any(|side| *side > max) {
            return Err(InstanceError::TextureLimit(size[0], size[1]));
        }
        let atlas = Atlas::pack(plan, instances, inputs, max)?;
        let device = self.gpu.device.clone();
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        self.upload_tables(inputs.tables);
        self.upload_light(inputs.light.values());
        let result = self.draw(canvas, instances, plan, inputs, &atlas);
        let validation = block_on(device.pop_error_scope()).flatten();
        let memory = block_on(device.pop_error_scope()).flatten();
        if let Some(error) = validation.or(memory) {
            // The cached uploads may be the cause: drop them.
            self.tables = None;
            self.light = None;
            return Err(InstanceError::Upload(error.to_string()));
        }
        if self.gpu.is_lost() {
            return Err(InstanceError::DeviceLost);
        }
        let (counts, pixels) = result?;
        canvas.pixels_mut().copy_from_slice(&pixels);
        Ok(counts)
    }

    fn draw(
        &self,
        canvas: &IndexedImage,
        instances: &[SpriteInstance],
        plan: &BatchPlan,
        inputs: &FrameInputs,
        atlas: &Atlas,
    ) -> Result<(GpuCounts, Vec<u8>), InstanceError> {
        let device = &self.gpu.device;
        let size = [canvas.width(), canvas.height()];
        let canvas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("instanced canvas"),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.write(&canvas_texture, size, 1, canvas.pixels());
        let snapshot = self.texture("canvas snapshot", size, wgpu::TextureFormat::R8Uint);
        let atlas_texture = self.texture("sprite atlas", atlas.size, wgpu::TextureFormat::R8Uint);
        self.write(&atlas_texture, atlas.size, 1, &atlas.pixels);
        let (mask_size, mask_pixels) = match inputs.mask {
            Some(mask) => ([mask.width, mask.height], &mask.pixels[..]),
            None => ([1, 1], &[0_u8][..]),
        };
        if mask_size.iter().any(|side| *side > self.max_side()) {
            return Err(InstanceError::TextureLimit(mask_size[0], mask_size[1]));
        }
        let mask = self.texture("cutout mask", mask_size, wgpu::TextureFormat::R8Uint);
        self.write(&mask, mask_size, 1, mask_pixels);
        let uniforms = uniform_buffer(device, &self.gpu.queue, &inputs.uniforms);
        let records = record_buffer(device, &self.gpu.queue, plan, instances, atlas)?;
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let (tables, light) = (
            &self.tables.as_ref().expect("uploaded").1,
            &self.light.as_ref().expect("uploaded").1,
        );
        let views = [
            view(&atlas_texture),
            view(tables),
            view(light),
            view(&mask),
            view(&snapshot),
        ];
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: records.as_entire_binding(),
            },
        ];
        entries.extend(
            views
                .iter()
                .enumerate()
                .map(|(index, view)| wgpu::BindGroupEntry {
                    binding: 2 + index as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                }),
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("instanced canvas bind group"),
            layout: &self.layout,
            entries: &entries,
        });
        let target = view(&canvas_texture);
        let bounds: Rect = [0, 0, size[0] as i32, size[1] as i32];
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut counts = GpuCounts::default();
        for batch in &plan.batches {
            let Some(clip) = intersect(batch.clip, bounds) else {
                continue;
            };
            if batch.kind == BatchKind::Read {
                // Copy the bounding box before the draw reads it.
                if let Some(rect) = batch.snapshot.and_then(|rect| intersect(rect, bounds)) {
                    let origin = wgpu::Origin3d {
                        x: rect[0] as u32,
                        y: rect[1] as u32,
                        z: 0,
                    };
                    let copy = |texture| wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin,
                        aspect: wgpu::TextureAspect::All,
                    };
                    encoder.copy_texture_to_texture(
                        copy(&canvas_texture),
                        copy(&snapshot),
                        extent([(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32]),
                    );
                    counts.snapshots += 1;
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("instanced canvas batch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_scissor_rect(
                clip[0] as u32,
                clip[1] as u32,
                (clip[2] - clip[0]) as u32,
                (clip[3] - clip[1]) as u32,
            );
            pass.draw(0..6, batch.range.start as u32..batch.range.end as u32);
            counts.draws += 1;
        }
        // Read the canvas back.
        let row = size[0].div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas readback"),
            size: u64::from(row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &canvas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size[1]),
                },
            },
            extent(size),
        );
        self.gpu.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let mapped = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (done, error) = (Arc::clone(&mapped), Arc::clone(&failed));
        slice.map_async(wgpu::MapMode::Read, move |result| {
            if result.is_err() {
                error.store(true, Ordering::SeqCst);
            }
            done.store(true, Ordering::SeqCst);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(WAIT),
            })
            .map_err(|error| InstanceError::Upload(error.to_string()))?;
        if !mapped.load(Ordering::SeqCst) || failed.load(Ordering::SeqCst) {
            return Err(InstanceError::Upload("canvas readback".into()));
        }
        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((size[0] * size[1]) as usize);
        for y in 0..size[1] as usize {
            let start = y * row as usize;
            pixels.extend_from_slice(&data[start..start + size[0] as usize]);
        }
        drop(data);
        readback.unmap();
        Ok((counts, pixels))
    }
}

fn extent([width, height]: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn uniform_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    uniforms: &LayerUniforms,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layer uniforms"),
        size: LayerUniforms::SIZE as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &uniforms.to_bytes());
    buffer
}

/// §2.2 the recommended 48-byte records, in plan order.
fn record_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: &BatchPlan,
    instances: &[SpriteInstance],
    atlas: &Atlas,
) -> Result<wgpu::Buffer, InstanceError> {
    let mut words: Vec<u32> = Vec::with_capacity(plan.order.len().max(1) * RECORD_WORDS);
    let pair = |low: u32, high: u32| -> Result<u32, InstanceError> {
        if low > 0xffff || high > 0xffff {
            return Err(InstanceError::TextureLimit(low, high));
        }
        Ok(low | high << 16)
    };
    for index in &plan.order {
        let instance = &instances[*index];
        let at = atlas.place(instance.image)?;
        let src = [at[0] + instance.src[0], at[1] + instance.src[1]];
        let level = match instance.level {
            Some((image, offset)) => {
                let at = atlas.place(image)?;
                [at[0] + offset[0], at[1] + offset[1]]
            }
            None => [0, 0],
        };
        words.extend([
            instance.dst[0] as u32,
            instance.dst[1] as u32,
            pair(instance.size[0], instance.size[1])?,
            pair(src[0], src[1])?,
            0,
            instance.mode as u32 | u32::from(instance.flags) << 8 | (instance.light as u32) << 16,
            instance.hex,
            instance.fixed,
            u32::from(instance.aux),
            pair(level[0], level[1])?,
            0,
            0,
        ]);
    }
    if words.is_empty() {
        words.resize(RECORD_WORDS, 0);
    }
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance records"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &bytes);
    Ok(buffer)
}

/// One atlas page: the images a plan draws, shelf-packed.
struct Atlas {
    size: [u32; 2],
    pixels: Vec<u8>,
    places: BTreeMap<usize, [u32; 2]>,
}

impl Atlas {
    /// Pack every image (and level image) the plan's instances read.
    /// §3.11 step 6: an image wider than the texture limit, or a page
    /// taller than it, refuses the refresh.
    fn pack(
        plan: &BatchPlan,
        instances: &[SpriteInstance],
        inputs: &FrameInputs,
        max: u32,
    ) -> Result<Self, InstanceError> {
        let mut used: Vec<usize> = Vec::new();
        for index in &plan.order {
            let instance = &instances[*index];
            used.push(instance.image);
            used.extend(instance.level.map(|(image, _)| image));
        }
        used.sort_unstable();
        used.dedup();
        let width = max.min(4096);
        let mut places = BTreeMap::new();
        let (mut x, mut y, mut shelf) = (0_u32, 0_u32, 0_u32);
        for image in &used {
            let image_size = inputs
                .images
                .get(*image)
                .map(|image| [image.width, image.height])
                .ok_or(InstanceError::MissingImage(*image))?;
            if image_size[0] > width || image_size[1] > max {
                return Err(InstanceError::TextureLimit(image_size[0], image_size[1]));
            }
            if x + image_size[0] > width {
                (x, y, shelf) = (0, y + shelf, 0);
            }
            places.insert(*image, [x, y]);
            x += image_size[0];
            shelf = shelf.max(image_size[1]);
        }
        let height = (y + shelf).max(1);
        if height > max || height > 0xffff {
            return Err(InstanceError::AtlasFull);
        }
        let mut pixels = vec![0; width as usize * height as usize];
        for (image, [px, py]) in &places {
            let image = &inputs.images[*image];
            for row in 0..image.height as usize {
                let source = &image.pixels[row * image.width as usize..][..image.width as usize];
                let start = (*py as usize + row) * width as usize + *px as usize;
                pixels[start..start + image.width as usize].copy_from_slice(source);
            }
        }
        Ok(Self {
            size: [width, height],
            pixels,
            places,
        })
    }
    fn place(&self, image: usize) -> Result<[u32; 2], InstanceError> {
        self.places
            .get(&image)
            .copied()
            .ok_or(InstanceError::MissingImage(image))
    }
}

/// The hex light texture's side.
const _: () = assert!(200 * 200 == HEXES);
