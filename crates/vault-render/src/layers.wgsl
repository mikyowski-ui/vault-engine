// The `enhanced` layers (`Layer` in layers.rs): each layer's index image
// drawn into its surface rectangle, back to front, through the shared
// palette. The pass covers the whole surface; the vertex shader places the
// quad, so a rectangle may be fractional or overhang the surface's edges.
@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<f32>;

// `LayerParams` in layers.rs: the rectangle and the surface size in
// surface pixels, the scale per axis (rectangle size over image size),
// whether the layer is drawn sharp-bilinear (a fractional scale), and the
// index drawn transparent (negative for none).
struct Params {
    rect: vec4<f32>,
    surface: vec2<f32>,
    scale: vec2<f32>,
    sharp: f32,
    transparent: f32,
    pad: vec2<f32>,
};
@group(0) @binding(2) var<uniform> params: Params;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let corner = corners[index];
    let at = params.rect.xy + corner * params.rect.zw;
    var output: VertexOutput;
    output.position = vec4(
        at.x / params.surface.x * 2.0 - 1.0,
        1.0 - at.y / params.surface.y * 2.0,
        0.0,
        1.0,
    );
    output.uv = corner;
    return output;
}

// The index of one texel, clamped to the image.
fn texel_index(texel: vec2<i32>, dimensions: vec2<i32>) -> u32 {
    let pixel = clamp(texel, vec2<i32>(0), dimensions - vec2<i32>(1));
    return textureLoad(indices, pixel, 0).r;
}

fn is_transparent(index: u32) -> bool {
    return params.transparent >= 0.0 && index == u32(params.transparent);
}

fn display_color(index: u32) -> vec4<f32> {
    return textureLoad(palette, vec2<i32>(i32(index), 0), 0);
}

// A neighbor's color for the bilinear blend: a transparent neighbor takes
// the color of the texel under the pixel, so transparency never bleeds a
// fringe of the transparent index's color into the layer's edges.
fn blend_color(texel: vec2<i32>, dimensions: vec2<i32>, under: vec4<f32>) -> vec4<f32> {
    let index = texel_index(texel, dimensions);
    if is_transparent(index) {
        return under;
    }
    return display_color(index);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(indices));
    let size = vec2<f32>(dimensions);
    let texel = input.uv * size;
    // The texel under the pixel center decides transparency, in either
    // mode: a transparent texel draws nothing (no blending).
    let index = texel_index(vec2<i32>(floor(texel)), dimensions);
    if is_transparent(index) {
        discard;
    }
    let under = display_color(index);
    if params.sharp == 0.0 {
        // Nearest texels: whole scales.
        return under;
    }
    // Sharp bilinear, as `indexed.wgsl` draws a fractional aspect fit.
    let prescale = max(floor(params.scale), vec2<f32>(1.0));
    let base = floor(texel);
    let from_center = texel - base - vec2<f32>(0.5);
    let band = vec2<f32>(0.5) - vec2<f32>(0.5) / prescale;
    let sharp = base + vec2<f32>(0.5)
        + (from_center - clamp(from_center, -band, band)) * prescale;
    let corner = sharp - vec2<f32>(0.5);
    let first = floor(corner);
    let weight = corner - first;
    let at = vec2<i32>(first);
    let top = mix(blend_color(at, dimensions, under),
                  blend_color(at + vec2<i32>(1, 0), dimensions, under), weight.x);
    let bottom = mix(blend_color(at + vec2<i32>(0, 1), dimensions, under),
                     blend_color(at + vec2<i32>(1, 1), dimensions, under), weight.x);
    return mix(top, bottom, weight.y);
}
