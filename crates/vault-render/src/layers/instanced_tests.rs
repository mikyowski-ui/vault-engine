//! SPEC-022 §4 tests T1-T7 on the synthetic fixtures of
//! `tests/fixtures/spec022/` (the §5.1 formula tables, no palette).
use super::*;
use crate::layers::gpu;
use std::collections::BTreeMap;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/spec022/",
            $name
        ))
    };
}

/// The rows of a TSV fixture after its header, by column name.
fn tsv(text: &str) -> Vec<BTreeMap<&str, &str>> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    lines
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').collect();
            assert_eq!(cells.len(), header.len(), "{line}");
            header.iter().copied().zip(cells).collect()
        })
        .collect()
}

fn hex_u64(text: &str) -> u64 {
    u64::from_str_radix(text, 16).unwrap()
}

/// An offset-addressed hex dump (`0000: 18 00 ...`) as bytes.
fn hex_dump(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (offset, row) = line.split_once(':').unwrap();
        assert_eq!(
            usize::from_str_radix(offset.trim(), 16).unwrap(),
            bytes.len()
        );
        bytes.extend(
            row.split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap()),
        );
    }
    bytes
}

/// SPEC-022 §5.1 the synthetic tables.
pub(crate) fn synthetic_tables() -> LookupTables {
    let intensity = (0..65536_u32)
        .map(|index| {
            let (i, k) = (index >> 8, index & 255);
            if i == 0 {
                0
            } else {
                (i * k / 128).min(255) as u8
            }
        })
        .collect();
    let mix = (0..65536_u32)
        .map(|index| ((index >> 8) + (index & 255)).min(255) as u8)
        .collect();
    let gray = [
        std::array::from_fn(|s| (s % 8) as u8),
        std::array::from_fn(|s| (s / 8 % 8) as u8),
    ];
    let blends = [33_u32, 35, 220, 58, 133].map(|color| {
        Box::new(std::array::from_fn(|g| {
            std::array::from_fn(|d| {
                let (g, d) = (g as u32, d as u32);
                if g == 0 {
                    d as u8
                } else {
                    ((d * (7 - g) + color * g) / 7) as u8
                }
            })
        }))
    });
    LookupTables {
        intensity,
        mix,
        gray,
        blends,
    }
}

/// SPEC-022 §5.3 the falloff world's accumulated light by the SPEC-006
/// §3.6 formula: 655 everywhere, plus light A (hex 20100, radius 2,
/// intensity 65536) and light B (hex 20104, radius 3, intensity 32768).
pub(crate) fn falloff_light() -> Vec<i64> {
    let mut tiles = vec![655_i64; 40000];
    for (center, radius, intensity) in [(20100, 2, 65536), (20104, 3, 32768)] {
        let step = (intensity - 655) / (radius + 1);
        for (hex, tile) in tiles.iter_mut().enumerate() {
            let distance = vault_formats::HexGrid::lattice_distance(center, hex as i32).unwrap();
            if distance <= radius {
                *tile += i64::from(intensity - distance * step);
            }
        }
    }
    tiles
}

/// The scene's images, by name, in file order.
pub(crate) fn scene_images() -> Vec<(String, SpriteImage<'static>)> {
    let mut images: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
    for line in fixture!("scene-sprites.txt").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix("image ") {
            let fields: Vec<&str> = header.split_whitespace().collect();
            images.push((
                fields[0].to_owned(),
                fields[1].parse().unwrap(),
                fields[2].parse().unwrap(),
                Vec::new(),
            ));
        } else {
            let (_, width, _, pixels) = images.last_mut().unwrap();
            let row: Vec<u8> = line
                .split_whitespace()
                .map(|byte| u8::from_str_radix(byte, 16).unwrap())
                .collect();
            assert_eq!(row.len(), *width as usize);
            pixels.extend(row);
        }
    }
    images
        .into_iter()
        .map(|(name, width, height, pixels)| {
            assert_eq!(pixels.len(), (width * height) as usize, "{name}");
            (name, SpriteImage::new(width, height, pixels))
        })
        .collect()
}

/// The §5.4 scene: its instances (in `scene.tsv` order, with their ids),
/// images, mask and uniforms.
pub(crate) struct Scene {
    pub ids: Vec<String>,
    pub instances: Vec<SpriteInstance>,
    pub images: Vec<SpriteImage<'static>>,
    pub mask: SpriteImage<'static>,
    pub uniforms: LayerUniforms,
    pub tables: LookupTables,
    pub light: HexLightBuffer,
}

fn rect(text: &str) -> Rect {
    let values: Vec<i32> = text
        .split(',')
        .map(|value| value.parse().unwrap())
        .collect();
    values.try_into().unwrap()
}

pub(crate) fn scene() -> Scene {
    let named = scene_images();
    let index = |name: &str| named.iter().position(|(other, _)| other == name).unwrap();
    let mut ids = Vec::new();
    let mut instances = Vec::new();
    for row in tsv(fixture!("scene.tsv")) {
        let mode = match row["mode"] {
            "plain" => DrawMode::Plain,
            "lit" => DrawMode::Lit,
            "floor_map" => DrawMode::FloorMap,
            "translucent" => DrawMode::Translucent,
            "egg" => DrawMode::Egg,
            "outline_blend" => DrawMode::OutlineBlend,
            other => panic!("mode {other}"),
        };
        let light = match row["light"] {
            "none" => LightSource::None,
            "hex" => LightSource::Hex,
            "ambient" => LightSource::Ambient,
            "fixed" => LightSource::Fixed,
            "map" => LightSource::LevelImage,
            other => panic!("light {other}"),
        };
        let number = |column: &str| -> u32 {
            match row[column] {
                "-" => 0,
                value => value.parse().unwrap(),
            }
        };
        let key = SortKey::new(
            row["layer"].parse().unwrap(),
            number("primary"),
            number("secondary"),
        )
        .unwrap();
        // T2: each row's key is the key of its fields.
        assert_eq!(key.0, hex_u64(row["key"]), "{}", row["id"]);
        ids.push(row["id"].to_owned());
        instances.push(SpriteInstance {
            key,
            mode,
            flags: row["flags"].parse().unwrap(),
            light,
            hex: number("hex"),
            fixed: number("fixed"),
            dst: [row["dst_x"].parse().unwrap(), row["dst_y"].parse().unwrap()],
            size: [number("width"), number("height")],
            image: index(row["src"]),
            src: [number("src_x"), number("src_y")],
            level: (row["levels"] != "-").then(|| (index(row["levels"]), [0, 0])),
            clip: rect(row["clip"]),
            aux: u16::from_str_radix(row["aux"], 16).unwrap(),
        });
    }
    let mask = named[index("egg")].1.clone();
    let uniforms = LayerUniforms {
        canvas: [24, 16],
        offset: [-4, 2],
        ambient: 32768,
        elevation: 0,
        cutout: Some(CutoutPlace {
            left: 9,
            top: 0,
            width: 6,
            height: 5,
        }),
        peek: Some(RoofPeekParams::new([88, 40], 32, 48)),
    };
    Scene {
        ids,
        instances,
        images: named.into_iter().map(|(_, image)| image).collect(),
        mask,
        uniforms,
        tables: synthetic_tables(),
        light: HexLightBuffer::pack(&falloff_light()),
    }
}

impl Scene {
    pub fn inputs(&self) -> FrameInputs<'_> {
        FrameInputs {
            tables: &self.tables,
            light: &self.light,
            uniforms: self.uniforms,
            mask: Some(&self.mask),
            images: &self.images,
        }
    }
    fn id(&self, index: usize) -> &str {
        &self.ids[index]
    }
}

/// The 24-by-16 canvas filled with `0x40`.
pub(crate) fn scene_canvas() -> IndexedImage {
    IndexedImage::from_pixels(24, 16, vec![0x40; 24 * 16]).unwrap()
}

pub(crate) fn scene_expected() -> Vec<u8> {
    let bytes = hex_dump(fixture!("scene-expected.hex"));
    assert_eq!(bytes.len(), 24 * 16);
    bytes
}

#[test]
fn t1_sort_keys_give_every_fixture_row() {
    let rows = tsv(fixture!("sort-keys.tsv"));
    assert_eq!(rows.len(), 14);
    for row in rows {
        let (layer, primary, secondary) = (
            row["layer"].parse().unwrap(),
            row["primary"].parse().unwrap(),
            row["secondary"].parse().unwrap(),
        );
        let key = SortKey::new(layer, primary, secondary).unwrap();
        assert_eq!(key.to_string(), row["key"], "{}", row["item"]);
        assert_eq!(
            (key.layer(), key.primary(), key.secondary()),
            (layer, primary, secondary)
        );
    }
    // The typed constructors agree with the rows.
    assert_eq!(
        SortKey::floor(9999).unwrap().to_string(),
        "0000270F00000000"
    );
    assert_eq!(
        SortKey::object(FrameLayer::Upright, 20100, 1, 0)
            .unwrap()
            .to_string(),
        "40004E8400000008"
    );
    assert_eq!(
        SortKey::object(FrameLayer::Upright, 39999, 2, 4)
            .unwrap()
            .to_string(),
        "40009C3F00000014"
    );
    assert_eq!(
        SortKey::roof(5050, 1).unwrap().to_string(),
        "600013BA00000001"
    );
    assert_eq!(
        SortKey::outline(99, 0).unwrap().to_string(),
        "8000000000000318"
    );
    assert_eq!(
        SortKey::outline(3, 1).unwrap().to_string(),
        "8000000000000019"
    );
    assert_eq!(
        SortKey::floating_text(0).unwrap().to_string(),
        "A000000000000000"
    );
    assert_eq!(SortKey::no_hex(7).unwrap().to_string(), "C000000000000007");
}

#[test]
fn t1_sort_keys_refuse_fields_outside_their_domains() {
    // Layer 7.
    assert_eq!(SortKey::new(7, 0, 0), Err(InstanceError::ReservedLayer(7)));
    assert!(FrameLayer::from_index(7).is_err());
    // A hex of 40000, a square of 10000.
    assert!(SortKey::object(FrameLayer::Flat, 40000, 0, 0).is_err());
    assert!(SortKey::new(2, 40000, 0).is_err());
    assert!(SortKey::floor(10000).is_err());
    assert!(SortKey::roof(10000, 0).is_err());
    // A part of 5 in layers 1 to 3.
    assert!(SortKey::object(FrameLayer::Flat, 0, 0, 5).is_err());
    assert!(SortKey::object(FrameLayer::Upright, 0, 0, 5).is_err());
    assert!(SortKey::new(2, 0, 5).is_err());
    assert!(SortKey::roof(0, 5).is_err());
    // A part of 1 in layer 0.
    assert!(SortKey::new(0, 0, 1).is_err());
    // A part of 2 in layer 4, and an outline position of 100.
    assert!(SortKey::outline(0, 2).is_err());
    assert!(SortKey::new(4, 0, 2).is_err());
    assert!(SortKey::outline(100, 0).is_err());
    // A list index of 2^29.
    assert!(SortKey::object(FrameLayer::Upright, 0, 1 << 29, 0).is_err());
    assert!(SortKey::object(FrameLayer::Upright, 0, (1 << 29) - 1, 4).is_ok());
    // An object key needs an object layer; the primary is below 2^29.
    assert!(SortKey::object(FrameLayer::Roof, 0, 0, 0).is_err());
    assert!(SortKey::new(5, 1, 0).is_err());
}

#[test]
fn t2_scene_sorts_into_the_fixture_order() {
    let scene = scene();
    let order = sort_by_key(&scene.instances).unwrap();
    let sorted: Vec<String> = order
        .iter()
        .map(|index| format!("{}\t{}", scene.instances[*index].key, scene.id(*index)))
        .collect();
    let expected: Vec<&str> = fixture!("scene-order.txt").lines().collect();
    assert_eq!(sorted, expected);
    // Two equal keys refuse the refresh.
    let mut duplicated = scene.instances.clone();
    duplicated[1].key = duplicated[0].key;
    assert_eq!(
        sort_by_key(&duplicated),
        Err(InstanceError::DuplicateKey(duplicated[0].key))
    );
    assert!(BatchPlan::new(&duplicated).is_err());
}

#[test]
fn t3_scene_plans_into_the_fixture_batches() {
    let scene = scene();
    let plan = BatchPlan::new(&scene.instances).unwrap();
    // `flat_x` is culled.
    assert!(plan.order.iter().all(|index| scene.id(*index) != "flat_x"));
    let rows = tsv(fixture!("scene-batches.tsv"));
    assert_eq!(plan.batches.len(), rows.len());
    for (batch, row) in plan.batches.iter().zip(&rows) {
        let kind = match batch.kind {
            BatchKind::Overwrite => "overwrite",
            BatchKind::Read => "dst_read",
        };
        let ids: Vec<&str> = plan.order[batch.range.clone()]
            .iter()
            .map(|index| scene.id(*index))
            .collect();
        assert_eq!(kind, row["kind"], "batch {}", row["batch"]);
        assert_eq!(batch.clip, rect(row["clip"]), "batch {}", row["batch"]);
        assert_eq!(ids.join(","), row["instances"], "batch {}", row["batch"]);
        let snapshot = batch.snapshot.map_or("-".to_owned(), |rect| {
            rect.map(|value| value.to_string()).join(",")
        });
        assert_eq!(snapshot, row["snapshot"], "batch {}", row["batch"]);
    }
    assert_eq!(
        plan.counts(),
        FrameCounts {
            instances: 15,
            batches: 8,
            read_batches: 3,
        }
    );
}

#[test]
fn t3_one_clip_of_overwrite_instances_is_one_draw() {
    let scene = scene();
    let overwrite: Vec<SpriteInstance> = scene
        .instances
        .iter()
        .filter(|instance| !instance.mode.reads_canvas() && instance.clip == [0, 0, 20, 16])
        .cloned()
        .collect();
    let plan = BatchPlan::new(&overwrite).unwrap();
    assert_eq!(plan.counts().batches, 1);
    assert_eq!(plan.counts().read_batches, 0);
}

#[test]
fn t4_scene_uniforms_pack_into_the_fixture_bytes() {
    let scene = scene();
    let bytes = scene.uniforms.to_bytes();
    assert_eq!(bytes.to_vec(), hex_dump(fixture!("scene-uniforms.hex")));
    // An inactive cutout and peek leave their bytes zero.
    let bare = LayerUniforms {
        cutout: None,
        peek: None,
        ..scene.uniforms
    }
    .to_bytes();
    assert_eq!(bare[0x18], 0);
    assert!(bare[0x20..].iter().all(|byte| *byte == 0));
    // The ambient light is clamped when written.
    let bright = LayerUniforms {
        ambient: 70000,
        ..scene.uniforms
    };
    assert_eq!(bright.to_bytes()[0x10..0x14], 65536_u32.to_le_bytes());
}

#[test]
fn t5_cpu_executor_draws_the_fixture_canvas_batched_and_one_at_a_time() {
    let scene = scene();
    let inputs = scene.inputs();
    let plan = BatchPlan::new(&scene.instances).unwrap();
    let mut batched = scene_canvas();
    execute_plan(
        &mut CanvasView::of(&mut batched),
        &scene.instances,
        &plan,
        &inputs,
    )
    .unwrap();
    let expected = scene_expected();
    for (index, (got, want)) in batched.pixels().iter().zip(&expected).enumerate() {
        assert_eq!(got, want, "pixel ({}, {})", index % 24, index / 24);
    }
    let mut single = scene_canvas();
    execute_unbatched(&mut CanvasView::of(&mut single), &scene.instances, &inputs).unwrap();
    assert_eq!(single.pixels(), expected.as_slice());
    // The worked pixels.
    let at = |x: usize, y: usize| batched.pixels()[y * 24 + x];
    assert_eq!(at(6, 5), 0x4D);
    assert_eq!(at(10, 2), 0x3F);
    assert_eq!(at(9, 9), 0xC1);
    assert_eq!(at(5, 2), 0x40);
    assert_eq!(at(6, 2), 0x31);
}

#[test]
fn t5_overlapping_reads_split_into_separate_read_batches() {
    // Read batches hold only disjoint rectangles: two overlapping
    // translucent instances go to two read batches, so the second blends
    // over the first one's result, as drawing them one at a time does.
    let scene = scene();
    let inputs = scene.inputs();
    let obj_b = scene
        .instances
        .iter()
        .position(|instance| instance.mode == DrawMode::Translucent)
        .unwrap();
    let mut twice = vec![scene.instances[obj_b].clone(); 2];
    twice[1].key = SortKey(twice[0].key.0 + 8);
    let plan = BatchPlan::new(&twice).unwrap();
    assert_eq!(plan.counts().read_batches, 2);
    let mut batched = scene_canvas();
    execute_plan(&mut CanvasView::of(&mut batched), &twice, &plan, &inputs).unwrap();
    let mut single = scene_canvas();
    execute_unbatched(&mut CanvasView::of(&mut single), &twice, &inputs).unwrap();
    assert_eq!(batched.pixels(), single.pixels());
}

#[test]
fn t5_executor_refuses_before_drawing() {
    let scene = scene();
    let inputs = scene.inputs();
    let plan = BatchPlan::new(&scene.instances).unwrap();
    let refused = |instances: &[SpriteInstance], inputs: &FrameInputs| {
        let mut canvas = scene_canvas();
        let result = execute_plan(&mut CanvasView::of(&mut canvas), instances, &plan, inputs);
        assert!(canvas.pixels().iter().all(|pixel| *pixel == 0x40));
        result.unwrap_err()
    };
    // A source rectangle past its image.
    let mut wide = scene.instances.clone();
    wide[0].size[0] = 100;
    assert_eq!(refused(&wide, &inputs), InstanceError::SourceRange);
    // A missing image.
    let mut missing = scene.instances.clone();
    missing[0].image = 99;
    assert_eq!(refused(&missing, &inputs), InstanceError::MissingImage(99));
    // A mode with a light source it cannot use.
    let mut unlit = scene.instances.clone();
    unlit[0].light = LightSource::Ambient;
    assert!(matches!(
        refused(&unlit, &inputs),
        InstanceError::ModeLight { .. }
    ));
    // A level image holding a level above 128.
    let mut images = scene.images.clone();
    let levels = scene
        .instances
        .iter()
        .find_map(|instance| instance.level)
        .unwrap()
        .0;
    images[levels].pixels.to_mut()[0] = 129;
    let high = FrameInputs {
        images: &images,
        ..inputs
    };
    assert_eq!(
        refused(&scene.instances, &high),
        InstanceError::LevelRange(129)
    );
    // An oversized canvas.
    let oversized = FrameInputs {
        uniforms: LayerUniforms {
            canvas: [MAX_CANVAS + 1, 16],
            ..inputs.uniforms
        },
        ..inputs
    };
    let mut canvas = scene_canvas();
    assert!(matches!(
        execute_plan(
            &mut CanvasView::of(&mut canvas),
            &scene.instances,
            &plan,
            &oversized
        ),
        Err(InstanceError::TextureLimit(..))
    ));
    // Unknown codes are refused when decoded.
    assert_eq!(DrawMode::from_code(6), Err(InstanceError::UnknownMode(6)));
    assert_eq!(
        LightSource::from_code(5),
        Err(InstanceError::UnknownLightSource(5))
    );
}

#[test]
fn t6_gpu_executor_draws_the_fixture_canvas() {
    let _gpu = gpu::test_lock();
    let Some(device) = gpu::test_device() else {
        return;
    };
    let scene = scene();
    let inputs = scene.inputs();
    let plan = BatchPlan::new(&scene.instances).unwrap();
    let mut canvas = scene_canvas();
    let mut executor = gpu::GpuExecutor::new(device);
    let counts = executor
        .execute(&mut canvas, &scene.instances, &plan, &inputs)
        .unwrap();
    assert_eq!(canvas.pixels(), scene_expected().as_slice());
    assert_eq!(
        counts,
        gpu::GpuCounts {
            draws: 8,
            snapshots: 3,
        }
    );
    // A second refresh on the same executor reuses its resources.
    let mut again = scene_canvas();
    executor
        .execute(&mut again, &scene.instances, &plan, &inputs)
        .unwrap();
    assert_eq!(again.pixels(), scene_expected().as_slice());
}

#[test]
fn t7_light_packing_levels_and_digests() {
    for row in tsv(fixture!("light-pack.tsv")) {
        let accumulated: i64 = row["accumulated"].parse().unwrap();
        assert_eq!(
            HexLightBuffer::pack_value(accumulated).to_string(),
            row["packed"]
        );
    }
    // No light source: every entry 655; and a buffer of zeros.
    assert_eq!(
        HexLightBuffer::pack(&vec![655; 40000]).digest(),
        0xDC22_FAE7_4E4B_BAA5
    );
    assert_eq!(HexLightBuffer::default().digest(), 0x5D22_1D50_7044_7725);
    assert_eq!(HexLightBuffer::pack(&[]).digest(), 0x5D22_1D50_7044_7725);
    assert_eq!(HexLightBuffer::default().bytes().len(), 160000);
    // The falloff world.
    let light = HexLightBuffer::pack(&falloff_light());
    let rows = tsv(fixture!("light-falloff.tsv"));
    let mut listed = std::collections::BTreeSet::new();
    for row in &rows {
        let hex: u32 = row["hex"].parse().unwrap();
        listed.insert(hex);
        assert_eq!(
            falloff_light()[hex as usize].to_string(),
            row["accumulated"]
        );
        assert_eq!(light.get(hex).to_string(), row["packed"], "hex {hex}");
        let uniforms = LayerUniforms {
            canvas: [1, 1],
            offset: [0, 0],
            ambient: 32768,
            elevation: 0,
            cutout: None,
            peek: None,
        };
        let tables = synthetic_tables();
        let inputs = FrameInputs {
            tables: &tables,
            light: &light,
            uniforms,
            mask: None,
            images: &[],
        };
        let instance = hex_instance(hex);
        assert_eq!(
            inputs.effective(&instance).to_string(),
            row["effective_32768"]
        );
        assert_eq!(inputs.level(&instance).to_string(), row["level_32768"]);
    }
    let differing = light.values().iter().filter(|value| **value != 655).count();
    assert_eq!(differing, 49);
    for (hex, value) in light.values().iter().enumerate() {
        if !listed.contains(&(hex as u32)) {
            assert_eq!(*value, 655, "hex {hex}");
        }
    }
    assert_eq!(light.digest(), 0xD450_03EF_0BA7_8613);
    // E and L for each light source.
    let tables = synthetic_tables();
    for row in tsv(fixture!("light-levels.tsv")) {
        let ambient: i64 = row["ambient"].parse().unwrap();
        let mut instance = hex_instance(0);
        match row["source"] {
            "hex" => instance.hex = row["hex"].parse().unwrap(),
            "ambient" => instance.light = LightSource::Ambient,
            "fixed" => {
                instance.light = LightSource::Fixed;
                instance.fixed = row["fixed"].parse().unwrap();
            }
            other => panic!("source {other}"),
        }
        let inputs = FrameInputs {
            tables: &tables,
            light: &light,
            uniforms: LayerUniforms {
                canvas: [1, 1],
                offset: [0, 0],
                ambient,
                elevation: 0,
                cutout: None,
                peek: None,
            },
            mask: None,
            images: &[],
        };
        assert_eq!(inputs.effective(&instance).to_string(), row["effective"]);
        assert_eq!(inputs.level(&instance).to_string(), row["level"]);
    }
}

/// A lit instance of hex `hex`, for the light tests.
fn hex_instance(hex: u32) -> SpriteInstance {
    SpriteInstance {
        key: SortKey::floor(0).unwrap(),
        mode: DrawMode::Lit,
        flags: EXEMPT,
        light: LightSource::Hex,
        hex,
        fixed: 0,
        dst: [0, 0],
        size: [1, 1],
        image: 0,
        src: [0, 0],
        level: None,
        clip: [0, 0, 1, 1],
        aux: 0,
    }
}

#[test]
fn render_path_defaults_to_the_reference() {
    assert_eq!(RenderPath::default(), RenderPath::Reference);
    for path in [RenderPath::Reference, RenderPath::Instanced] {
        assert_eq!(RenderPath::parse(path.name()), Some(path));
    }
    assert_eq!(RenderPath::parse("gpu"), None);
}
