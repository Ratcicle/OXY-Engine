struct OverlayUniform {
    mvp: mat4x4<f32>,
    viewport: vec4<f32>, // physical width/height, pixels per point, through
    ranges: vec4<u32>, // mode, face count, edge count, vertex count
    layer: vec4<f32>, // 2D, selected depth, number of scene draws, reserved
};
@group(0) @binding(0) var<uniform> overlay: OverlayUniform;
@group(0) @binding(1) var<storage, read> selected: array<u32>;
struct Component {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) c: vec3<f32>,
    @location(3) ordinal: u32,
};
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) kind: u32,
    @location(2) @interpolate(flat) chosen: u32,
    @location(3) original_depth: f32,
};
fn clip_near(a0: vec4<f32>, b0: vec4<f32>) -> mat2x4<f32> {
    var a = a0;
    var b = b0;
    if a.z < 0.0 && b.z >= 0.0 { a = mix(a, b, a.z / (a.z - b.z)); }
    if b.z < 0.0 && a.z >= 0.0 { b = mix(b, a, b.z / (b.z - a.z)); }
    return mat2x4<f32>(a, b);
}
@vertex fn vs_component(input: Component, @builtin(vertex_index) vertex: u32) -> Output {
    var output: Output;
    output.kind = select(1u, 0u, input.ordinal < overlay.ranges.y);
    if input.ordinal >= overlay.ranges.y + overlay.ranges.z { output.kind = 2u; }
    output.chosen = selected[input.ordinal];
    output.local = vec2<f32>(0.0);
    var a = overlay.mvp * vec4<f32>(input.a, 1.0);
    if output.kind == 0u {
        var p = input.a;
        if vertex == 1u { p = input.b; }
        if vertex == 2u { p = input.c; }
        a = overlay.mvp * vec4<f32>(p, 1.0);
        if output.chosen == 0u || overlay.ranges.x != 1u {
            a = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        }
    } else {
        let corners = array<vec2<f32>, 6>(
            vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
            vec2<f32>(0.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
        let corner = corners[vertex];
        if output.kind == 1u {
            let clipped = clip_near(a, overlay.mvp * vec4<f32>(input.b, 1.0));
            a = clipped[0];
            let b = clipped[1];
            let direction = (b.xy / max(b.w, 0.000001) - a.xy / max(a.w, 0.000001)) * overlay.viewport.xy;
            let normal = vec2<f32>(-direction.y, direction.x) / max(length(direction), 0.000001);
            let width = select(1.0, 2.4, output.chosen != 0u && overlay.ranges.x == 2u);
            a = mix(a, b, corner.x);
            a.x += normal.x * corner.y * width * overlay.viewport.z / overlay.viewport.x * a.w;
            a.y += normal.y * corner.y * width * overlay.viewport.z / overlay.viewport.y * a.w;
        } else {
            output.local = vec2<f32>(corner.x * 2.0 - 1.0, corner.y);
            let radius = select(2.8, 4.2, output.chosen != 0u);
            a = vec4<f32>(a.xy + output.local * radius * overlay.viewport.z * 2.0 / overlay.viewport.xy * a.w, a.zw);
            if overlay.ranges.x != 3u { a = vec4<f32>(2.0, 2.0, 2.0, 1.0); }
        }
    }
    output.original_depth = a.z / a.w;
    if overlay.layer.x > 0.5 { a.z = overlay.layer.y * a.w; }
    // Small slope-independent offset keeps on-surface outlines readable without writing depth.
    a.z -= 0.0000002 * a.w;
    output.position = a;
    return output;
}
fn component_color(input: Output, hidden: bool) -> vec4<f32> {
    if overlay.layer.x > 0.5 && (input.original_depth < 0.0 || input.original_depth > 1.0) { discard; }
    if input.kind == 2u && dot(input.local, input.local) > 1.0 { discard; }
    let chosen = input.chosen != 0u;
    if hidden && !chosen && overlay.viewport.w < 0.5 { discard; }
    if hidden && u32(floor((input.position.x + input.position.y) / (4.0 * overlay.viewport.z))) % 2u == 0u { discard; }
    var color = vec4<f32>(0.61, 0.61, 0.61, 1.0);
    if chosen { color = vec4<f32>(1.0, 0.72, 0.22, 1.0); }
    if input.kind == 0u { color.a = 0.29; }
    if hidden { color = vec4<f32>(color.rgb * 0.72, color.a * 0.42); }
    return color;
}
@fragment fn fs_visible(input: Output) -> @location(0) vec4<f32> { return component_color(input, false); }
@fragment fn fs_hidden(input: Output) -> @location(0) vec4<f32> { return component_color(input, true); }
