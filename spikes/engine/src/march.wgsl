// Editor view: one primary ray per pixel through the brick map, simple directional shading.
// Prepended with brickmap.wgsl at pipeline creation.

struct Camera {
    inv_vp: mat4x4<f32>,
    eye: vec4<f32>,
    size: vec4<f32>, // width, height, 1/width, 1/height
    sun: vec4<f32>,  // xyz direction toward the sun; w = debug bits (1 heat map, 2 secondary rays)
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var out_color: texture_storage_2d<rgba8unorm, write>;
// G-buffer for other passes (path tracer): ray distance (-1 on miss), and packed
// palette index | normal code << 8 | hit << 11, normal code = axis * 2 + (normal > 0).
@group(0) @binding(2) var out_depth: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var out_info: texture_storage_2d<r32uint, write>;

override WRITE_GBUFFER: bool = false;
override LEVELS: u32 = 3u;
override WG_X: u32 = 8u;
override WG_Y: u32 = 8u;

fn primary_ray(px: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(px.x * cam.size.z * 2.0 - 1.0, 1.0 - px.y * cam.size.w * 2.0);
    let pn = cam.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let pf = cam.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize(pf.xyz / pf.w - pn.xyz / pn.w);
}

@compute @workgroup_size(WG_X, WG_Y)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (f32(gid.x) >= cam.size.x || f32(gid.y) >= cam.size.y) { return; }
    let ro = cam.eye.xyz;
    let d = primary_ray(vec2<f32>(gid.xy) + 0.5);
    var h: Hit;
    if (LEVELS == 4u) {
        h = trace_h(ro, d, 0.0, 1e9);
    } else if (LEVELS == 3u) {
        h = trace(ro, d, 0.0, 1e9);
    } else {
        h = trace2(ro, d, 0.0, 1e9);
    }
    var steps = h.steps;
    var occl = 1.0;
    // Test mode (sun.w bit 2): one secondary ray per pixel from the hit, as a path tracer
    // would shoot, to time traversal from points on voxel faces.
    if (h.axis != MISS && (u32(cam.sun.w) & 2u) != 0u) {
        let n = hit_normal(h, d);
        let o = ro + d * h.t + n * 1e-3;
        var k = gid.x * 1973u + gid.y * 9277u + 26699u;
        k = (k ^ (k >> 15u)) * 0x2c1b3c6du;
        k = (k ^ (k >> 12u)) * 0x297a2d39u;
        let r = vec3<f32>(f32(k & 1023u), f32((k >> 10u) & 1023u), f32((k >> 20u) & 1023u)) / 511.5 - 1.0;
        let sd = normalize(n + normalize(r + vec3<f32>(1e-4)));
        var h2: Hit;
        if (LEVELS == 4u) {
            h2 = trace_h(o, sd, 0.0, 1e9);
        } else if (LEVELS == 3u) {
            h2 = trace(o, sd, 0.0, 1e9);
        } else {
            h2 = trace2(o, sd, 0.0, 1e9);
        }
        steps += h2.steps;
        occl = select(1.0, 0.55, h2.axis != MISS);
    }
    var col: vec3<f32>;
    if (h.axis != MISS) {
        let n = hit_normal(h, d);
        let albedo = palette[h.value].rgb;
        let diff = max(dot(n, cam.sun.xyz), 0.0);
        let amb = 0.32 + 0.10 * n.y;
        col = albedo * (amb + 0.85 * diff) * occl;
    } else {
        col = mix(vec3<f32>(0.55, 0.63, 0.74), vec3<f32>(0.25, 0.38, 0.60), clamp(d.y * 1.5, 0.0, 1.0));
    }
    if ((u32(cam.sun.w) & 1u) != 0u) {
        let k = clamp(f32(steps) / 256.0, 0.0, 1.0);
        col = vec3<f32>(k, k * k, 0.2 * (1.0 - k));
    }
    let srgb = pow(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    textureStore(out_color, gid.xy, vec4<f32>(srgb, 1.0));
    if (WRITE_GBUFFER) {
        var depth = -1.0;
        var info = 0u;
        if (h.axis != MISS) {
            depth = h.t;
            let positive = select(0u, 1u, d[h.axis] < 0.0);
            info = h.value | ((h.axis * 2u + positive) << 8u) | (1u << 11u);
        }
        textureStore(out_depth, gid.xy, vec4<f32>(depth, 0.0, 0.0, 0.0));
        textureStore(out_info, gid.xy, vec4<u32>(info, 0u, 0u, 0u));
    }
}
