struct SkyUniform { directions: mat4x4<f32> };
@group(0) @binding(0) var<uniform> sky: SkyUniform;
@group(1) @binding(0) var sky_texture: texture_2d<f32>;
@group(1) @binding(1) var sky_sampler: sampler;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.2831853;

struct SkyOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) direction: vec3<f32>,
};

// One triangle over the whole screen, (-1, -1), (3, -1) and (-1, 3). Each corner carries the
// direction the camera looks along through it, which varies linearly across the screen.
@vertex fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    let far = sky.directions * vec4<f32>(corner, 1.0, 1.0);
    var output: SkyOutput;
    output.position = vec4<f32>(corner, 0.5, 1.0);
    output.direction = far.xyz / far.w;
    return output;
}

// Equirectangular panorama: -Z at the middle column, turning towards +X goes right, +Y at the
// top row. The sampler repeats across the wrap and the level is fixed, so there is no seam.
@fragment fn fs_sky(input: SkyOutput) -> @location(0) vec4<f32> {
    let direction = normalize(input.direction);
    let uv = vec2<f32>(
        atan2(direction.x, -direction.z) / TAU + 0.5,
        acos(clamp(direction.y, -1.0, 1.0)) / PI,
    );
    return vec4<f32>(textureSampleLevel(sky_texture, sky_sampler, uv, 0.0).rgb, 1.0);
}
