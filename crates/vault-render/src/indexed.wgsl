// The frame fills the render pass viewport, an integer-snapped surface
// rectangle (`Viewport` in lib.rs); each fragment samples the texel under
// its pixel center, the rule the pointer mapping uses.
@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<f32>;

// `DisplayParams` in lib.rs: the viewport's scale per axis, and whether
// the frame is drawn sharp-bilinear (an aspect fit at a fractional scale).
struct Display {
    scale: vec2<f32>,
    sharp: f32,
    pad: f32,
};
@group(0) @binding(2) var<uniform> display: Display;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    );
    let point = positions[index];
    var output: VertexOutput;
    output.position = vec4(point, 0.0, 1.0);
    output.uv = vec2((point.x + 1.0) * 0.5, (1.0 - point.y) * 0.5);
    return output;
}

// The display color of one texel, clamped to the frame: its index
// resolved through the palette.
fn texel_color(texel: vec2<i32>, dimensions: vec2<i32>) -> vec4<f32> {
    let pixel = clamp(texel, vec2<i32>(0), dimensions - vec2<i32>(1));
    let color_index = textureLoad(indices, pixel, 0).r;
    return textureLoad(palette, vec2<i32>(i32(color_index), 0), 0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(indices));
    if display.sharp == 0.0 {
        // Nearest texels: whole scales in both modes, and Integer mode.
        let pixel = clamp(vec2<i32>(input.uv * vec2<f32>(dimensions)),
                          vec2<i32>(0), dimensions - vec2<i32>(1));
        let color_index = textureLoad(indices, pixel, 0).r;
        return textureLoad(palette, vec2<i32>(i32(color_index), 0), 0);
    }
    // Sharp bilinear: the frame as if pre-scaled by the whole part of the
    // scale with nearest texels, then interpolated to the viewport. Inside
    // a texel the coordinate stays on the texel's center; only a band
    // 1 / prescale texels wide across each texel edge (about one surface
    // pixel and the fractional remainder) blends the two sides. The
    // mapping is fixed per surface pixel, so a frame scrolled by whole
    // texels shows the same edges without shimmer.
    let size = vec2<f32>(dimensions);
    let texel = input.uv * size;
    let prescale = max(floor(display.scale), vec2<f32>(1.0));
    let base = floor(texel);
    let from_center = texel - base - vec2<f32>(0.5);
    let band = vec2<f32>(0.5) - vec2<f32>(0.5) / prescale;
    let sharp = base + vec2<f32>(0.5)
        + (from_center - clamp(from_center, -band, band)) * prescale;
    // Bilinear between the four texels around the adjusted point. The
    // palette is resolved per texel, so the four indices are each looked
    // up and their display colors blended.
    let corner = sharp - vec2<f32>(0.5);
    let first = floor(corner);
    let weight = corner - first;
    let at = vec2<i32>(first);
    let top = mix(texel_color(at, dimensions),
                  texel_color(at + vec2<i32>(1, 0), dimensions), weight.x);
    let bottom = mix(texel_color(at + vec2<i32>(0, 1), dimensions),
                     texel_color(at + vec2<i32>(1, 1), dimensions), weight.x);
    return mix(top, bottom, weight.y);
}
