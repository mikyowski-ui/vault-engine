// SPEC-022 the instanced path's canvas pass (`layers/gpu.rs`): each
// instance is a quad over its rectangle, drawn into the R8Uint world
// canvas with the draw modes of §3.6. Integer textures and `textureLoad`
// only: no sampler, filtering, blending or float color math (§3.11).

// §2.4 the layer uniform block (`LayerUniforms` in layers/instanced.rs).
struct Uniforms {
    canvas: vec2<u32>,
    offset: vec2<i32>,
    ambient: u32,
    elevation: u32,
    flags: u32,
    reserved: u32,
    cutout_at: vec2<i32>,
    cutout_size: vec2<u32>,
    peek_center: vec2<i32>,
    peek_outer: u32,
    peek_inner: u32,
    thresholds: array<vec4<u32>, 4>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
// §2.2 the instance records, 12 words each, in key order.
@group(0) @binding(1) var<storage, read> records: array<u32>;
// The sprite, ring, text and level images.
@group(0) @binding(2) var atlas: texture_2d<u32>;
// Rows 0-255 the intensity table I[i][k] (texel (k, i)); rows 256-511 the
// mix table X[a][b] (texel (b, 256 + a)); rows 512-551 the blend tables
// F_t[g][D] (texel (D, 512 + 8 t + g)); rows 552 and 553 the common and
// glass gray levels.
@group(0) @binding(3) var tables: texture_2d<u32>;
// §2.3 the hex light buffer: texel (column, row) holds hex column + 200 row.
@group(0) @binding(4) var hex_light: texture_2d<u32>;
// The player cutout mask (1 by 1 when not bound).
@group(0) @binding(5) var mask: texture_2d<u32>;
// The canvas as it was before a read batch.
@group(0) @binding(6) var snapshot: texture_2d<u32>;

const RECORD_WORDS: u32 = 12u;
const EXEMPT: u32 = 1u;
const PEEK: u32 = 2u;
const BAYER = array<u32, 16>(0u, 8u, 2u, 10u, 12u, 4u, 14u, 6u, 3u, 11u, 1u, 9u, 15u, 7u, 13u, 5u);

struct Instance {
    dst: vec2<i32>,
    size: vec2<i32>,
    src: vec2<i32>,
    mode: u32,
    flags: u32,
    light: u32,
    hex: u32,
    fixed_light: u32,
    aux: u32,
    level: vec2<i32>,
};

fn instance(index: u32) -> Instance {
    let base = index * RECORD_WORDS;
    var out: Instance;
    out.dst = vec2<i32>(bitcast<i32>(records[base]), bitcast<i32>(records[base + 1u]));
    let size = records[base + 2u];
    out.size = vec2<i32>(i32(size & 0xffffu), i32(size >> 16u));
    let src = records[base + 3u];
    out.src = vec2<i32>(i32(src & 0xffffu), i32(src >> 16u));
    let kinds = records[base + 5u];
    out.mode = kinds & 0xffu;
    out.flags = (kinds >> 8u) & 0xffu;
    out.light = (kinds >> 16u) & 0xffu;
    out.hex = records[base + 6u];
    out.fixed_light = records[base + 7u];
    out.aux = records[base + 8u];
    let level = records[base + 9u];
    out.level = vec2<i32>(i32(level & 0xffffu), i32(level >> 16u));
    return out;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
};

// The instance's rectangle widened by one pixel on every side (§3.11
// step 3); the fragment shader discards the pixels outside it.
@vertex
fn vs_main(@builtin(vertex_index) corner_index: u32, @builtin(instance_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let item = instance(index);
    let corner = corners[corner_index];
    let at = vec2<f32>(item.dst - vec2<i32>(1)) + corner * vec2<f32>(item.size + vec2<i32>(2));
    let canvas = vec2<f32>(uniforms.canvas);
    var out: VertexOutput;
    out.position = vec4(at.x / canvas.x * 2.0 - 1.0, 1.0 - at.y / canvas.y * 2.0, 0.0, 1.0);
    out.index = index;
    return out;
}

fn table(column: u32, row: u32) -> u32 {
    return textureLoad(tables, vec2<i32>(i32(column), i32(row)), 0).r;
}

fn intensity(index: u32, level: u32) -> u32 {
    return table(level, index);
}

// §3.6 the light level L (0-128) of every source but the level image.
fn level_of(item: Instance) -> u32 {
    let ambient = uniforms.ambient;
    var light = 65536u;
    switch item.light {
        case 1u: {
            var tile = 0u;
            if item.hex < 40000u {
                tile = textureLoad(hex_light, vec2<i32>(i32(item.hex % 200u), i32(item.hex / 200u)), 0).r;
            }
            light = max(ambient, tile);
        }
        case 2u: {
            light = ambient;
        }
        case 3u: {
            light = item.fixed_light;
        }
        default: {}
    }
    return min(light, 65536u) >> 9u;
}

fn lit(item: Instance, s: u32, level: u32) -> u32 {
    if item.light == 0u || ((item.flags & EXEMPT) != 0u && s >= 229u) {
        return s;
    }
    return intensity(s, level);
}

// §3.10 the peek test.
fn peek_cuts(pixel: vec2<i32>) -> bool {
    let world = pixel - uniforms.offset;
    let wx = u32(((world.x % 4) + 4) % 4);
    let wy = u32(((world.y % 4) + 4) % 4);
    let b = BAYER[wy * 4u + wx];
    let dx = pixel.x - uniforms.peek_center.x;
    let dy = pixel.y - uniforms.peek_center.y;
    let outer = i32(uniforms.peek_outer);
    if abs(dx) > outer || abs(dy) > outer {
        return false;
    }
    let d2 = u32(dx * dx + dy * dy);
    let inner = uniforms.peek_inner;
    if d2 <= inner * inner {
        return true;
    }
    if d2 > u32(outer) * u32(outer) {
        return false;
    }
    return 256u * d2 < uniforms.thresholds[b / 4u][b % 4u];
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<u32> {
    let item = instance(input.index);
    let pixel = vec2<i32>(floor(input.position.xy));
    let uv = pixel - item.dst;
    if uv.x < 0 || uv.y < 0 || uv.x >= item.size.x || uv.y >= item.size.y {
        discard;
    }
    let s = textureLoad(atlas, item.src + uv, 0).r;
    if s == 0u {
        discard;
    }
    if (item.flags & PEEK) != 0u && (uniforms.flags & 2u) != 0u && peek_cuts(pixel) {
        discard;
    }
    let level = level_of(item);
    var result = s;
    switch item.mode {
        case 0u: {
            result = s;
        }
        case 1u: {
            result = lit(item, s, level);
        }
        case 2u: {
            let map_level = textureLoad(atlas, item.level + uv, 0).r;
            result = lit(item, s, map_level);
        }
        case 3u: {
            let t = item.aux & 7u;
            var gray_row = 552u;
            if t == 1u {
                gray_row = 553u;
            }
            let gray = min(table(s, gray_row), 7u);
            let d = textureLoad(snapshot, pixel, 0).r;
            let blended = table(d, 512u + 8u * t + gray);
            result = intensity(blended, level);
        }
        case 4u: {
            let s1 = lit(item, s, level);
            var weight = 0u;
            if (uniforms.flags & 1u) != 0u {
                let m = pixel - uniforms.cutout_at;
                let size = vec2<i32>(uniforms.cutout_size);
                if m.x >= 0 && m.y >= 0 && m.x < size.x && m.y < size.y {
                    weight = min(textureLoad(mask, m, 0).r, 128u);
                }
            }
            if weight == 0u {
                result = s1;
            } else {
                let d = textureLoad(snapshot, pixel, 0).r;
                let a = intensity(s1, weight);
                let b = intensity(d, 128u - weight);
                result = table(b, 256u + a);
            }
        }
        case 5u: {
            let t = item.aux & 7u;
            let g = (item.aux >> 8u) & 7u;
            var r = textureLoad(snapshot, pixel, 0).r;
            for (var paint = 0u; paint < s; paint++) {
                r = table(r, 512u + 8u * t + g);
            }
            result = r;
        }
        case 6u: {
            let d = textureLoad(snapshot, pixel, 0).r;
            result = textureLoad(atlas, item.level + vec2<i32>(i32(d), i32(s) - 1), 0).r;
        }
        default: {
            discard;
        }
    }
    return vec4<u32>(result, 0u, 0u, 0u);
}
