// Editor view: one primary ray per pixel through the brick map, simple directional shading.
// Prepended with brickmap.wgsl at pipeline creation.

struct Camera {
    inv_vp: mat4x4<f32>,
    eye: vec4<f32>,       // w = 0: orthographic
    size: vec4<f32>,      // width, height, 1/width, 1/height
    sun: vec4<f32>,       // xyz direction toward the sun
    cursor: vec4<i32>,    // voxel under the cursor (map coordinates); w = 1 to highlight it
    select: vec4<u32>,    // x = words in `sel`, 0 when nothing is selected; y = frame tiles per row
    // Guides: world size of a pixel at ray distance t is pix.x + pix.y * t; pix.z = the ground
    // layer (map y); pix.w = guide line width in pixels.
    pix: vec4<f32>,
    // The model bounds' size (map voxels); w = which guides show (GRID | GROUND | FRAME).
    bounds: vec4<u32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var out_color: texture_storage_2d<rgba8unorm, write>;
// G-buffer for other passes (the path-traced preview): ray distance (-1 on miss), and packed
// palette index | normal code << 8 | hit << 11, normal code = axis * 2 + (normal > 0).
@group(0) @binding(2) var out_depth: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var out_info: texture_storage_2d<r32uint, write>;
// One bit per bounds edge for each 32-pixel square of the view: the edges that pass near it.
@group(0) @binding(4) var<storage, read> frame_tiles: array<u32>;
// The selection: one bit per voxel in the masks' layout (16 words per brick slot).
@group(2) @binding(0) var<storage, read> sel: array<u32>;

// Selection highlight, the UI's accent #8ea2ff in linear light.
const SELECT_TINT: vec3<f32> = vec3<f32>(0.27, 0.36, 1.0);

fn selected(v: vec3<i32>) -> bool {
    if (cam.select.x == 0u) { return false; }
    let brick = grid_at(v >> vec3<u32>(3u));
    let l = vec3<u32>(v & vec3<i32>(7));
    let local = l.x | (l.y << 3u) | (l.z << 6u);
    let w = brick * 16u + (local >> 5u);
    if (brick == 0xffffffffu || w >= cam.select.x) { return false; }
    return ((sel[w] >> (local & 31u)) & 1u) != 0u;
}

override WRITE_GBUFFER: bool = false;
override WG_X: u32 = 8u;
override WG_Y: u32 = 8u;

struct Ray {
    o: vec3<f32>,
    d: vec3<f32>,
};

// The ray through a pixel: from the eye in perspective, from the near plane (all parallel)
// in orthographic.
fn primary_ray(px: vec2<f32>) -> Ray {
    let ndc = vec2<f32>(px.x * cam.size.z * 2.0 - 1.0, 1.0 - px.y * cam.size.w * 2.0);
    let pn = cam.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let pf = cam.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    let o = select(cam.eye.xyz, pn.xyz / pn.w, cam.eye.w == 0.0);
    return Ray(o, normalize(pf.xyz / pf.w - pn.xyz / pn.w));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

// The editor background from the Hybrid mockups (prototype CSS): a radial gradient from
// #262b37 at 50% 35% to #0d0f14 at 70% of a 120% x 90% ellipse.
fn background(px: vec2<f32>) -> vec3<f32> {
    let uv = px * cam.size.zw - vec2<f32>(0.5, 0.35);
    let r = length(uv / vec2<f32>(1.2, 0.9));
    let k = clamp(r / 0.7, 0.0, 1.0);
    let inner = vec3<f32>(38.0, 43.0, 55.0) / 255.0;
    let outer = vec3<f32>(13.0, 15.0, 20.0) / 255.0;
    return srgb_to_linear(mix(inner, outer, k));
}

const GRID: u32 = 1u;
const GROUND: u32 = 2u;
const FRAME: u32 = 4u;

// The ground under the model, seen from above: a plane that fades into the background away
// from the model, its footprint a shade lighter, and a line on every voxel edge inside it.
fn ground(ray: Ray, bg: vec3<f32>, max_t: f32) -> vec3<f32> {
    let t = (cam.pix.z - ray.o.y) / ray.d.y;
    if (ray.d.y >= 0.0 || t <= 0.0 || t > max_t) { return bg; }
    let size = vec2<f32>(cam.bounds.xz);
    let p = ray.o.xz + ray.d.xz * t;
    // Distance outside the footprint (0 inside), and the footprint's radius.
    let away = length(max(max(-p, p - size), vec2<f32>(0.0)));
    let r = max(length(size) * 0.5, 16.0);
    var col = bg;
    if ((cam.bounds.w & GROUND) != 0u) {
        let tile = srgb_to_linear(vec3<f32>(30.0, 34.0, 44.0) / 255.0);
        let base = srgb_to_linear(vec3<f32>(44.0, 50.0, 63.0) / 255.0);
        let fade = clamp(1.0 - away / (r * 2.0), 0.0, 1.0);
        col = mix(bg, tile, fade * fade * 0.85);
        if (away == 0.0) { col = base; }
    }
    if ((cam.bounds.w & GRID) != 0u && away == 0.0) {
        // The ground under one pixel: a pixel wide across the ray, stretched by 1 / |d.y|
        // along it.
        let pw = cam.pix.x + cam.pix.y * t;
        let h = abs(ray.d.xz) / max(length(ray.d.xz), 1e-6);
        let fw = pw * (1.0 + h * (1.0 / max(-ray.d.y, 0.05) - 1.0));
        let d = abs(fract(p + 0.5) - 0.5) / fw / cam.pix.w;
        let line = 1.0 - min(min(d.x, d.y), 1.0);
        // Lines closer than about three pixels fade out instead of turning to noise.
        let dense = clamp(1.5 - max(fw.x, fw.y) * 3.0, 0.0, 1.0);
        let ink = srgb_to_linear(vec3<f32>(78.0, 86.0, 104.0) / 255.0);
        col = mix(col, ink, line * dense * 0.8);
    }
    return col;
}

// Coverage of one bounds edge: the line through (ci, cj) along the third axis, from 0 to
// `len`, in a ray whose components are given in the order (i, j, k).
fn edge(o: vec3<f32>, d: vec3<f32>, ci: f32, cj: f32, len: f32, max_t: f32) -> f32 {
    let dd = d.x * d.x + d.y * d.y;
    if (dd < 1e-8) { return 0.0; }
    // Closest approach of the ray to the edge's line, then clamped to the edge.
    let t0 = -((o.x - ci) * d.x + (o.y - cj) * d.y) / dd;
    let q = vec3<f32>(ci, cj, clamp(o.z + d.z * t0, 0.0, len));
    let t = max(dot(q - o, d), 0.0);
    let dist = length(o + d * t - q);
    let pw = cam.pix.x + cam.pix.y * t;
    let a = clamp(cam.pix.w * 0.5 + 0.5 - dist / pw, 0.0, 1.0);
    return select(a * 0.3, a, t <= max_t + 0.01);
}

// How much of the pixel the bounds box's edges cover: full where the edge is in front of
// `max_t`, faint where something hides it. Only the edges binned to the pixel's tile are
// tested.
fn frame_cover(ray: Ray, px: vec2<u32>, max_t: f32) -> f32 {
    let s = vec3<f32>(cam.bounds.xyz);
    var mask = frame_tiles[px.x / 32u + px.y / 32u * cam.select.y];
    var c = 0.0;
    while (mask != 0u) {
        let e = firstTrailingBit(mask);
        mask &= mask - 1u;
        let n = e / 3u;
        let a = select(0.0, 1.0, (n & 1u) != 0u);
        let b = select(0.0, 1.0, (n & 2u) != 0u);
        switch (e % 3u) {
            case 0u: { c = max(c, edge(ray.o.yzx, ray.d.yzx, a * s.y, b * s.z, s.x, max_t)); }
            case 1u: { c = max(c, edge(ray.o.zxy, ray.d.zxy, a * s.z, b * s.x, s.y, max_t)); }
            default: { c = max(c, edge(ray.o.xyz, ray.d.xyz, a * s.x, b * s.y, s.z, max_t)); }
        }
    }
    return c;
}

@compute @workgroup_size(WG_X, WG_Y)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (f32(gid.x) >= cam.size.x || f32(gid.y) >= cam.size.y) { return; }
    let px = vec2<f32>(gid.xy) + 0.5;
    let ray = primary_ray(px);
    let ro = ray.o;
    let d = ray.d;
    let h = trace_h(ro, d, 0.0, 1e9);
    var col: vec3<f32>;
    if (h.axis != MISS) {
        let n = hit_normal(h, d);
        let albedo = palette[h.value].rgb;
        let diff = max(dot(n, cam.sun.xyz), 0.0);
        let amb = 0.32 + 0.10 * n.y;
        col = albedo * (amb + 0.85 * diff);
        let hp = ro + d * h.t;
        let voxel = vec3<i32>(floor(hp - n * 0.5));
        if (selected(voxel)) {
            // Tinted, with a brighter line along each voxel's edges so single cells read.
            let f = fract(hp);
            let e = min(min(f, 1.0 - f) + abs(n), vec3<f32>(1.0));
            let edge = min(e.x, min(e.y, e.z)) < 0.07;
            col = mix(col, SELECT_TINT * (amb + 0.85 * diff + 0.2), select(0.45, 0.8, edge));
        }
        if (cam.cursor.w == 1 && all(voxel == cam.cursor.xyz)) {
            col = mix(col, vec3<f32>(1.0), 0.35);
        }
    } else {
        col = background(px);
    }
    let far = select(1e9, h.t, h.axis != MISS);
    if ((cam.bounds.w & (GRID | GROUND)) != 0u) {
        col = ground(ray, col, far);
    }
    if ((cam.bounds.w & FRAME) != 0u) {
        let ink = srgb_to_linear(vec3<f32>(142.0, 162.0, 255.0) / 255.0);
        col = mix(col, ink, frame_cover(ray, gid.xy, far) * 0.55);
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
