struct Viewport {
    scale: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0) var indices: texture_2d<u32>;
@group(0) @binding(1) var palette: texture_2d<f32>;
@group(0) @binding(2) var<uniform> viewport: Viewport;

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
    output.position = vec4(point * viewport.scale, 0.0, 1.0);
    output.uv = vec2((point.x + 1.0) * 0.5, (1.0 - point.y) * 0.5);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(indices));
    let pixel = clamp(vec2<i32>(input.uv * vec2<f32>(dimensions)),
                      vec2<i32>(0), dimensions - vec2<i32>(1));
    let color_index = textureLoad(indices, pixel, 0).r;
    return textureLoad(palette, vec2<i32>(i32(color_index), 0), 0);
}
