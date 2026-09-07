struct ViewUniform { view_projection: mat4x4<f32> };
@group(0) @binding(0) var<uniform> view: ViewUniform;
@group(1) @binding(0) var surface_texture: texture_2d<f32>;
@group(1) @binding(1) var surface_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) world0: vec4<f32>,
    @location(4) world1: vec4<f32>,
    @location(5) world2: vec4<f32>,
    @location(6) world3: vec4<f32>,
    @location(7) normal0: vec4<f32>,
    @location(8) normal1: vec4<f32>,
    @location(9) normal2: vec4<f32>,
    @location(10) normal3: vec4<f32>,
    @location(11) color: vec4<f32>,
    @location(12) parameters: vec4<f32>,
};
struct LineInput {
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
    let world = mat4x4<f32>(input.world0, input.world1, input.world2, input.world3);
    let normal_matrix = mat4x4<f32>(input.normal0, input.normal1, input.normal2, input.normal3);
    let normal = (normal_matrix * vec4<f32>(input.normal, 0.0)).xyz;
    var output: VertexOutput;
    output.position = view.view_projection * (world * vec4<f32>(input.position, 1.0));
    output.normal = normal / max(length(normal), 1e-20);
    output.uv = input.uv;
    output.color = input.color;
    output.lit = input.parameters.x;
    return output;
}

@vertex fn vs_line(input: LineInput) -> VertexOutput {
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
