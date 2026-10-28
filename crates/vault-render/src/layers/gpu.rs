//! SPEC-022 §3.7 and §3.11 the GPU executor: a batch plan drawn into an
//! `R8Uint` canvas with one instanced draw per batch, read batches
//! through a snapshot copy, every value read with `textureLoad` at
//! integer coordinates (`instanced.wgsl`).
//!
//! `[May-improve]` choices: one atlas page (shelf-packed each refresh
//! with the sprite, ring, text and level images the plan draws), the
//! lookup tables in one texture re-uploaded only when they change, and a
//! canvas kept on the GPU between refreshes: only what differs from it is
//! uploaded (a uniform base fill is a clear pass), and only the pixels the
//! refresh can write are read back, so the display stage stays the shared
//! CPU one. Every texture and buffer is kept and rewritten in place, and
//! only when its bytes change.
use super::instanced::{
    BatchKind, BatchPlan, FrameInputs, HEXES, InstanceError, LookupTables, Rect, SpriteInstance,
    intersect,
};
use crate::IndexedImage;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
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

/// Cumulative GPU resource and transfer counts of an executor, for
/// measuring its canvas retention (`[May-improve]`, SPEC-022 §3.7 and
/// §3.9 step 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuStats {
    /// Refreshes that reached the GPU (drawn or failed).
    pub refreshes: u64,
    /// Textures created.
    pub textures: u64,
    /// Buffers created.
    pub buffers: u64,
    /// Bind groups created.
    pub bind_groups: u64,
    /// Bytes written into the canvas texture.
    pub canvas_upload_bytes: u64,
    /// Canvas refreshes whose uniform base was set by a clear pass instead
    /// of an upload.
    pub canvas_clears: u64,
    /// Every byte written to a texture or buffer, the canvas included.
    pub upload_bytes: u64,
    /// Bytes copied back from the canvas (the readback buffer's rows).
    pub readback_bytes: u64,
}

/// [`GpuStats`] as counters an executor updates through `&self`.
#[derive(Default)]
struct Counters([AtomicU64; 8]);

impl Counters {
    const REFRESHES: usize = 0;
    const TEXTURES: usize = 1;
    const BUFFERS: usize = 2;
    const BIND_GROUPS: usize = 3;
    const CANVAS_UPLOAD: usize = 4;
    const CANVAS_CLEARS: usize = 5;
    const UPLOAD: usize = 6;
    const READBACK: usize = 7;
    fn add(&self, counter: usize, value: u64) {
        self.0[counter].fetch_add(value, Ordering::Relaxed);
    }
    fn get(&self) -> GpuStats {
        let at = |counter: usize| self.0[counter].load(Ordering::Relaxed);
        GpuStats {
            refreshes: at(Self::REFRESHES),
            textures: at(Self::TEXTURES),
            buffers: at(Self::BUFFERS),
            bind_groups: at(Self::BIND_GROUPS),
            canvas_upload_bytes: at(Self::CANVAS_UPLOAD),
            canvas_clears: at(Self::CANVAS_CLEARS),
            upload_bytes: at(Self::UPLOAD),
            readback_bytes: at(Self::READBACK),
        }
    }
}

/// The table texture's rows: intensity, mix, blends, gray levels.
const TABLE_ROWS: u32 = 256 + 256 + 40 + 2;
/// Words per instance record.
const RECORD_WORDS: usize = 12;

/// The canvas kept on the GPU between refreshes (§3.2), with its snapshot
/// texture and readback buffer.
struct CanvasTarget {
    size: [u32; 2],
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    snapshot: wgpu::Texture,
    /// Room for the whole canvas at the copy row alignment.
    readback: wgpu::Buffer,
    /// What the canvas texture holds, valid only when `known` is set: it
    /// is cleared while a refresh is in flight and after any failure.
    held: Vec<u8>,
    known: bool,
}

/// The canvas pixels read back after a refresh: a rectangle and its rows.
type Readback = (Rect, Vec<u8>);

/// How a refresh brings the GPU canvas to the CPU canvas it starts from.
enum CanvasUpload {
    /// The GPU canvas already holds it.
    Held,
    /// Every pixel is this index: a clear pass, no upload.
    Clear(u8),
    /// Upload these rows (half-open).
    Rows(u32, u32),
}

/// A texture kept across refreshes with the bytes it holds.
struct Kept<T> {
    key: T,
    texture: wgpu::Texture,
}

/// A buffer kept across refreshes, its capacity and the bytes it holds.
struct KeptBuffer {
    bytes: Vec<u8>,
    buffer: wgpu::Buffer,
}

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
    canvas: Option<CanvasTarget>,
    /// The atlas page: its row capacity, and the pixels of its used part.
    atlas: Option<Kept<([u32; 2], Vec<u8>)>>,
    /// The cutout mask texture, its size and pixels.
    mask: Option<Kept<([u32; 2], Vec<u8>)>>,
    uniforms: Option<KeptBuffer>,
    records: Option<KeptBuffer>,
    /// The bind group over the kept resources; dropped whenever one of
    /// them is created again.
    bind_group: Option<wgpu::BindGroup>,
    counters: Counters,
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
            canvas: None,
            atlas: None,
            mask: None,
            uniforms: None,
            records: None,
            bind_group: None,
            counters: Counters::default(),
        }
    }

    /// The device this executor draws with.
    pub fn device(&self) -> &GpuDevice {
        &self.gpu
    }

    /// The resource and transfer counts so far.
    pub fn stats(&self) -> GpuStats {
        self.counters.get()
    }

    fn max_side(&self) -> u32 {
        self.gpu.device.limits().max_texture_dimension_2d
    }

    fn texture(&self, label: &str, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
        self.texture_with(label, size, format, wgpu::TextureUsages::empty())
    }

    fn texture_with(
        &self,
        label: &str,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        self.counters.add(Counters::TEXTURES, 1);
        self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | usage,
            view_formats: &[],
        })
    }

    fn buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.counters.add(Counters::BUFFERS, 1);
        self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    /// Write `data`, `size` texels of `bytes_per_texel`, at row `top` of
    /// `texture`.
    fn write(
        &self,
        texture: &wgpu::Texture,
        top: u32,
        size: [u32; 2],
        bytes_per_texel: u32,
        data: &[u8],
    ) {
        self.counters.add(Counters::UPLOAD, data.len() as u64);
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: top, z: 0 },
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

    /// Drop every kept resource, after a failure that may have left one
    /// of them wrong: the next refresh uploads everything again.
    fn forget(&mut self) {
        self.tables = None;
        self.light = None;
        self.canvas = None;
        self.atlas = None;
        self.mask = None;
        self.uniforms = None;
        self.records = None;
        self.bind_group = None;
    }

    /// Upload the tables when they differ from the last ones, into the
    /// kept texture.
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
        let texture = match self.tables.take() {
            Some((_, texture)) => texture,
            None => {
                self.bind_group = None;
                self.texture(
                    "lookup tables",
                    [256, TABLE_ROWS],
                    wgpu::TextureFormat::R8Uint,
                )
            }
        };
        self.write(&texture, 0, [256, TABLE_ROWS], 1, &bytes);
        self.tables = Some((Box::new(tables.clone()), texture));
    }

    /// Upload the hex light buffer when it differs from the last one
    /// (§3.8 step 4), into the kept texture.
    fn upload_light(&mut self, values: &[u32]) {
        if self.light.as_ref().is_some_and(|(held, _)| held == values) {
            return;
        }
        let texture = match self.light.take() {
            Some((_, texture)) => texture,
            None => {
                self.bind_group = None;
                self.texture("hex light", [200, 200], wgpu::TextureFormat::R32Uint)
            }
        };
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        self.write(&texture, 0, [200, 200], 4, &bytes);
        self.light = Some((values.to_vec(), texture));
    }

    /// The atlas page, grown (by doubling its rows) only when the packed
    /// images do not fit, and written only when they changed.
    fn upload_atlas(&mut self, atlas: &Atlas) {
        let [width, height] = atlas.size;
        let fits = self.atlas.as_ref().is_some_and(|kept| {
            let capacity = kept.key.0;
            capacity[0] == width && capacity[1] >= height
        });
        if !fits {
            let rows = height.next_power_of_two().min(self.max_side()).max(height);
            let texture = self.texture("sprite atlas", [width, rows], wgpu::TextureFormat::R8Uint);
            self.bind_group = None;
            self.atlas = Some(Kept {
                key: ([width, rows], Vec::new()),
                texture,
            });
        }
        let kept = self.atlas.as_mut().expect("kept");
        if kept.key.1 == atlas.pixels {
            return;
        }
        kept.key.1.clone_from(&atlas.pixels);
        let kept = self.atlas.as_ref().expect("kept");
        self.write(&kept.texture, 0, atlas.size, 1, &atlas.pixels);
    }

    /// The cutout mask texture (1 by 1 when not bound), written only when
    /// it changed.
    fn upload_mask(&mut self, size: [u32; 2], pixels: &[u8]) {
        if self.mask.as_ref().is_some_and(|kept| kept.key.0 == size) {
            if self.mask.as_ref().is_some_and(|kept| kept.key.1 == pixels) {
                return;
            }
        } else {
            let texture = self.texture("cutout mask", size, wgpu::TextureFormat::R8Uint);
            self.bind_group = None;
            self.mask = Some(Kept {
                key: (size, Vec::new()),
                texture,
            });
        }
        let kept = self.mask.as_mut().expect("kept");
        kept.key.1.clear();
        kept.key.1.extend_from_slice(pixels);
        let kept = self.mask.as_ref().expect("kept");
        self.write(&kept.texture, 0, size, 1, pixels);
    }

    /// Write `bytes` into the kept buffer `slot` (created, or grown to the
    /// next power of two, as needed), unless it already holds them.
    fn upload_buffer(
        &mut self,
        slot: fn(&mut Self) -> &mut Option<KeptBuffer>,
        label: &str,
        usage: wgpu::BufferUsages,
        bytes: &[u8],
    ) {
        let size = bytes.len() as u64;
        let fits = slot(self)
            .as_ref()
            .is_some_and(|kept| kept.buffer.size() >= size);
        if !fits {
            let capacity = size.next_power_of_two();
            let buffer = self.buffer(label, capacity, usage | wgpu::BufferUsages::COPY_DST);
            self.bind_group = None;
            *slot(self) = Some(KeptBuffer {
                bytes: Vec::new(),
                buffer,
            });
        }
        let kept = slot(self).as_mut().expect("kept");
        if kept.bytes == bytes {
            return;
        }
        kept.bytes.clear();
        kept.bytes.extend_from_slice(bytes);
        let buffer = kept.buffer.clone();
        self.gpu.queue.write_buffer(&buffer, 0, bytes);
        self.counters.add(Counters::UPLOAD, size);
    }

    /// The kept canvas of `size`, created (with its snapshot and readback
    /// buffer) when there is none of that size.
    fn canvas_target(&mut self, size: [u32; 2]) {
        if self
            .canvas
            .as_ref()
            .is_some_and(|canvas| canvas.size == size)
        {
            return;
        }
        self.canvas = None;
        let texture = self.texture_with(
            "instanced canvas",
            size,
            wgpu::TextureFormat::R8Uint,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let snapshot = self.texture("canvas snapshot", size, wgpu::TextureFormat::R8Uint);
        let readback = self.buffer(
            "canvas readback",
            u64::from(copy_row(size[0])) * u64::from(size[1]),
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        self.bind_group = None;
        self.canvas = Some(CanvasTarget {
            size,
            view: texture.create_view(&Default::default()),
            texture,
            snapshot,
            readback,
            held: Vec::new(),
            known: false,
        });
    }

    /// Draw `plan` over `canvas` on the GPU: the same canvas as
    /// [`super::execute_plan`]. A refused refresh leaves `canvas` as it
    /// was.
    ///
    /// `[May-improve]` retention: the canvas stays on the GPU between
    /// refreshes. A canvas the GPU already holds is not uploaded, a
    /// uniform one (a base fill) is set by a clear pass, and otherwise
    /// only the rows that differ from the held canvas are uploaded. Only
    /// the bounding box of the pixels the plan can write is read back.
    /// The atlas, mask, uniform and record resources are kept too, and
    /// written only when their bytes change.
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
        let (mask_size, mask_pixels) = match inputs.mask {
            Some(mask) => ([mask.width, mask.height], &mask.pixels[..]),
            None => ([1, 1], &[0_u8][..]),
        };
        if mask_size.iter().any(|side| *side > max) {
            return Err(InstanceError::TextureLimit(mask_size[0], mask_size[1]));
        }
        let records = record_bytes(plan, instances, &atlas)?;
        let written = written_rect(plan, instances, size);
        let device = self.gpu.device.clone();
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        self.counters.add(Counters::REFRESHES, 1);
        self.upload_tables(inputs.tables);
        self.upload_light(inputs.light.values());
        self.upload_atlas(&atlas);
        self.upload_mask(mask_size, mask_pixels);
        self.upload_buffer(
            |executor| &mut executor.uniforms,
            "layer uniforms",
            wgpu::BufferUsages::UNIFORM,
            &inputs.uniforms.to_bytes(),
        );
        self.upload_buffer(
            |executor| &mut executor.records,
            "instance records",
            wgpu::BufferUsages::STORAGE,
            &records,
        );
        self.canvas_target(size);
        let result = self.draw(canvas, plan, written);
        let validation = block_on(device.pop_error_scope()).flatten();
        let memory = block_on(device.pop_error_scope()).flatten();
        if let Some(error) = validation.or(memory) {
            // A kept resource may be the cause: drop them all.
            self.forget();
            return Err(InstanceError::Upload(error.to_string()));
        }
        if self.gpu.is_lost() {
            return Err(InstanceError::DeviceLost);
        }
        let (counts, region) = result?;
        if let Some((rect, pixels)) = region {
            let width = size[0] as usize;
            let row = (rect[2] - rect[0]) as usize;
            let target = canvas.pixels_mut();
            for (line, y) in (rect[1]..rect[3]).enumerate() {
                let start = y as usize * width + rect[0] as usize;
                target[start..start + row].copy_from_slice(&pixels[line * row..][..row]);
            }
        }
        let kept = self.canvas.as_mut().expect("kept");
        kept.held.clear();
        kept.held.extend_from_slice(canvas.pixels());
        kept.known = true;
        Ok(counts)
    }

    /// How the kept canvas is brought to `pixels`; marks it unknown until
    /// the refresh succeeds.
    fn canvas_upload(&mut self, pixels: &[u8]) -> CanvasUpload {
        let kept = self.canvas.as_mut().expect("kept");
        let known = std::mem::replace(&mut kept.known, false);
        if known && kept.held == pixels {
            return CanvasUpload::Held;
        }
        if let Some((first, rest)) = pixels.split_first()
            && rest.iter().all(|pixel| pixel == first)
        {
            return CanvasUpload::Clear(*first);
        }
        let [width, height] = kept.size;
        if !known {
            return CanvasUpload::Rows(0, height);
        }
        let differs = |y: &u32| {
            let row = *y as usize * width as usize..(*y as usize + 1) * width as usize;
            kept.held[row.clone()] != pixels[row]
        };
        let first = (0..height).find(differs).unwrap_or(0);
        let last = (first..height).rev().find(differs).map_or(first, |y| y + 1);
        CanvasUpload::Rows(first, last)
    }

    fn draw(
        &mut self,
        canvas: &IndexedImage,
        plan: &BatchPlan,
        written: Option<Rect>,
    ) -> Result<(GpuCounts, Option<Readback>), InstanceError> {
        let size = [canvas.width(), canvas.height()];
        let upload = self.canvas_upload(canvas.pixels());
        if let CanvasUpload::Rows(top, bottom) = upload
            && top < bottom
        {
            let row = size[0] as usize;
            let data = &canvas.pixels()[top as usize * row..bottom as usize * row];
            let texture = &self.canvas.as_ref().expect("kept").texture;
            self.write(texture, top, [size[0], bottom - top], 1, data);
            self.counters
                .add(Counters::CANVAS_UPLOAD, data.len() as u64);
        }
        if self.bind_group.is_none() {
            self.bind_group = Some(self.bind_group());
        }
        let device = &self.gpu.device;
        let target = self.canvas.as_ref().expect("kept");
        let bind_group = self.bind_group.as_ref().expect("bound");
        let bounds: Rect = [0, 0, size[0] as i32, size[1] as i32];
        let mut encoder = device.create_command_encoder(&Default::default());
        if let CanvasUpload::Clear(index) = upload {
            self.counters.add(Counters::CANVAS_CLEARS, 1);
            // An integer target clears to the integer value of `r`.
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("instanced canvas base fill"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(index),
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            }));
        }
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
                        copy(&target.texture),
                        copy(&target.snapshot),
                        extent([(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32]),
                    );
                    counts.snapshots += 1;
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("instanced canvas batch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
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
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_scissor_rect(
                clip[0] as u32,
                clip[1] as u32,
                (clip[2] - clip[0]) as u32,
                (clip[3] - clip[1]) as u32,
            );
            pass.draw(0..6, batch.range.start as u32..batch.range.end as u32);
            counts.draws += 1;
        }
        let Some(rect) = written else {
            // Nothing can change: the canvas is the one uploaded.
            self.gpu.queue.submit([encoder.finish()]);
            return Ok((counts, None));
        };
        // Read back the pixels the plan can write.
        let [width, height] = [(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32];
        let row = copy_row(width);
        let bytes = u64::from(row) * u64::from(height);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect[0] as u32,
                    y: rect[1] as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            extent([width, height]),
        );
        self.gpu.queue.submit([encoder.finish()]);
        let slice = target.readback.slice(..bytes);
        let mapped = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (done, error) = (Arc::clone(&mapped), Arc::clone(&failed));
        slice.map_async(wgpu::MapMode::Read, move |result| {
            if result.is_err() {
                error.store(true, Ordering::SeqCst);
            }
            done.store(true, Ordering::SeqCst);
        });
        let polled = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(WAIT),
        });
        if let Err(error) = polled {
            // The map may still be pending: the buffer cannot be reused.
            self.canvas = None;
            return Err(InstanceError::Upload(error.to_string()));
        }
        if !mapped.load(Ordering::SeqCst) || failed.load(Ordering::SeqCst) {
            self.canvas = None;
            return Err(InstanceError::Upload("canvas readback".into()));
        }
        let data = slice.get_mapped_range();
        self.counters.add(Counters::READBACK, data.len() as u64);
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height as usize {
            let start = y * row as usize;
            pixels.extend_from_slice(&data[start..start + width as usize]);
        }
        drop(data);
        target.readback.unmap();
        Ok((counts, Some((rect, pixels))))
    }

    /// The bind group over the kept resources.
    fn bind_group(&self) -> wgpu::BindGroup {
        self.counters.add(Counters::BIND_GROUPS, 1);
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let views = [
            view(&self.atlas.as_ref().expect("uploaded").texture),
            view(&self.tables.as_ref().expect("uploaded").1),
            view(&self.light.as_ref().expect("uploaded").1),
            view(&self.mask.as_ref().expect("uploaded").texture),
            view(&self.canvas.as_ref().expect("kept").snapshot),
        ];
        fn buffer(kept: &Option<KeptBuffer>) -> wgpu::BindingResource<'_> {
            kept.as_ref().expect("uploaded").buffer.as_entire_binding()
        }
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer(&self.uniforms),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer(&self.records),
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
        self.gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("instanced canvas bind group"),
                layout: &self.layout,
                entries: &entries,
            })
    }
}

/// A canvas row's bytes at the texture-to-buffer copy alignment.
fn copy_row(width: u32) -> u32 {
    width.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// The bounding box of every pixel `plan` can write on a canvas of
/// `size`: each drawn instance's rectangle within its batch's scissor.
/// `None` when no batch can write a pixel.
fn written_rect(plan: &BatchPlan, instances: &[SpriteInstance], size: [u32; 2]) -> Option<Rect> {
    let bounds: Rect = [0, 0, size[0] as i32, size[1] as i32];
    let mut written: Option<Rect> = None;
    for batch in &plan.batches {
        let Some(clip) = intersect(batch.clip, bounds) else {
            continue;
        };
        for position in batch.range.clone() {
            // A position or index outside the plan: the whole scissor.
            let rect = plan
                .order
                .get(position)
                .and_then(|index| instances.get(*index))
                .map_or(Some(clip), |instance| intersect(instance.rect(), clip));
            if let Some(rect) = rect {
                written = Some(written.map_or(rect, |was| {
                    [
                        was[0].min(rect[0]),
                        was[1].min(rect[1]),
                        was[2].max(rect[2]),
                        was[3].max(rect[3]),
                    ]
                }));
            }
        }
    }
    written
}

fn extent([width, height]: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

/// §2.2 the recommended 48-byte records, in plan order.
fn record_bytes(
    plan: &BatchPlan,
    instances: &[SpriteInstance],
    atlas: &Atlas,
) -> Result<Vec<u8>, InstanceError> {
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
    Ok(words.iter().flat_map(|word| word.to_le_bytes()).collect())
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
