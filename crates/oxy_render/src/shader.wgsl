struct ViewUniform { view_projection: mat4x4<f32> };
@group(0) @binding(0) var<uniform> view: ViewUniform;
@group(1) @binding(0) var surface_texture: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) lit: f32,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) lit: f32,
};

@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = view.view_projection * vec4<f32>(input.position, 1.0);
    output.normal = input.normal;
    output.uv = input.uv;
    output.color = input.color;
    output.lit = input.lit;
    return output;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sample_color = textureSample(surface_texture, surface_sampler, input.uv) * input.color;
    if sample_color.a < 0.01 { discard; }
    let normal = normalize(input.normal);
    let diffuse = max(dot(normal, normalize(vec3<f32>(0.45, 0.8, 0.55))), 0.0);
    let light = mix(1.0, 0.35 + 0.65 * diffuse, input.lit);
    return vec4<f32>(sample_color.rgb * light, sample_color.a);
}
