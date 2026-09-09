// Private depth-only replay of the current 2D draws. The game color/material is untouched.
struct Camera { matrix: mat4x4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var sampler_image: sampler;
struct OverlayUniform {
    mvp: mat4x4<f32>, viewport: vec4<f32>, ranges: vec4<u32>, layer: vec4<f32>
};
@group(2) @binding(0) var<uniform> overlay: OverlayUniform;
struct Input {
    @location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
    @location(3) world0: vec4<f32>, @location(4) world1: vec4<f32>, @location(5) world2: vec4<f32>, @location(6) world3: vec4<f32>,
    @location(7) normal0: vec4<f32>, @location(8) normal1: vec4<f32>, @location(9) normal2: vec4<f32>, @location(10) normal3: vec4<f32>,
    @location(11) color: vec4<f32>, @location(12) parameters: vec4<f32>,
};
struct Output { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) alpha: f32, @location(2) original_depth: f32 };
@vertex fn vs_main(input: Input) -> Output {
    var output: Output;
    output.position = camera.matrix * mat4x4<f32>(input.world0, input.world1, input.world2, input.world3) * vec4<f32>(input.position, 1.0);
    output.original_depth = output.position.z / output.position.w;
    output.position.z = (1.0 - (input.parameters.w + 1.0) / (overlay.layer.z + 1.0)) * output.position.w;
    output.uv = input.uv;
    output.alpha = input.color.a;
    return output;
}
@fragment fn fs_main(input: Output) -> @location(0) vec4<f32> {
    if input.original_depth < 0.0 || input.original_depth > 1.0 || textureSample(image, sampler_image, input.uv).a * input.alpha < 0.01 { discard; }
    return vec4<f32>(0.0);
}
