// Write the preview, scaled to `rect`, into the editor's colour texture before the editor's
// blit. A compute write into the existing texture avoids a second render pass over the whole
// target (a load + store of every tile on a tiled GPU). Pixels outside the rounded corners
// keep the editor image; the UI draws the panel's border on top.

struct Rect {
    origin: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
    pad0: f32,
    pad1: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> rect: Rect;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (f32(gid.x) >= rect.size.x || f32(gid.y) >= rect.size.y) { return; }
    let p = vec2<f32>(gid.xy) + 0.5;
    // Rounded corners: skip pixels whose centre lies outside the corner circle.
    let r = rect.radius;
    let q = abs(p - rect.size * 0.5) - (rect.size * 0.5 - vec2<f32>(r));
    if (length(max(q, vec2<f32>(0.0))) > r) { return; }
    let uv = p / rect.size;
    let c = textureSampleLevel(src, samp, uv, 0.0);
    textureStore(dst, vec2<u32>(rect.origin) + gid.xy, vec4<f32>(c.rgb, 1.0));
}
