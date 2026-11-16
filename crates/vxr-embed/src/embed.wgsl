// The embed's view: one primary ray per pixel through the brick map, a sun with a hard
// shadow and a sky. Prepended with vxr-render's brickmap.wgsl (bind group 1) at pipeline
// creation. Render space is the map's cell space, y up.

struct View {
    eye: vec4<f32>,    // xyz eye, w = tan(fov / 2)
    fwd: vec4<f32>,    // w = width / height
    right: vec4<f32>,
    up: vec4<f32>,
    sun: vec4<f32>,    // xyz direction toward the sun
    res: vec4<u32>,    // x width, y height
};

@group(0) @binding(0) var<uniform> view: View;
// The picture as packed sRGB RGBA8, one word per pixel. A storage buffer, not a storage
// texture: writing an rgba8unorm storage texture made the cold pipeline compile about 65 ms
// slower in Chrome and Safari (spikes/embed/RESULTS.md).
@group(0) @binding(1) var<storage, read_write> img: array<u32>;

// The embed draws every cell in the map.
fn hidden(brick: u32, local: u32) -> bool {
    return false;
}

fn sky(d: vec3<f32>) -> vec3<f32> {
    return mix(vec3<f32>(0.93, 0.92, 0.89), vec3<f32>(0.78, 0.84, 0.93), clamp(d.y * 0.8 + 0.3, 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= view.res.x || gid.y >= view.res.y) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(view.res.xy) * 2.0 - 1.0;
    let rd = normalize(view.fwd.xyz + view.right.xyz * (uv.x * view.eye.w * view.fwd.w)
        - view.up.xyz * (uv.y * view.eye.w));
    var col = sky(rd);
    // The primary ray, then a shadow ray toward the sun, through one call site so the march
    // loop is compiled once (half the pipeline compile time).
    var ro = view.eye.xyz;
    var dir = rd;
    var albedo = vec3<f32>(0.0);
    var amb = 0.0;
    var sun = 0.0;
    for (var k = 0; k < 2; k += 1) {
        let h = trace_h(ro, dir, 0.0, 1e9);
        if (k == 0) {
            if (h.axis == MISS) { break; }
            let n = hit_normal(h, dir);
            albedo = palette[h.value].rgb;
            amb = 0.30 + 0.12 * n.y + 0.04 * n.x;
            sun = max(dot(n, view.sun.xyz), 0.0);
            ro = ro + dir * h.t + n * 1e-3;
            dir = view.sun.xyz;
            if (sun <= 0.0) { break; }
        } else if (h.axis != MISS) {
            sun = 0.0;
        }
    }
    if (amb > 0.0) {
        col = pow(clamp(albedo * (amb + sun * 0.85), vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    }
    img[gid.x + gid.y * view.res.x] = pack4x8unorm(vec4<f32>(col, 1.0));
}
