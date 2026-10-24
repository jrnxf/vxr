// Write the preview, scaled to `rect`, into the editor's colour texture before the editor's
// blit. A compute write into the existing texture avoids a second render pass over the whole
// 3024x1964 target (a load + store of every tile on a tiled GPU).

struct Rect {
    origin: vec2<f32>,
    size: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> rect: Rect;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (f32(gid.x) >= rect.size.x || f32(gid.y) >= rect.size.y) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / rect.size;
    var c = textureSampleLevel(src, samp, uv, 0.0);
    // A thin frame so the inset reads as a separate view (the real look is test 3's job).
    let p = vec2<f32>(gid.xy);
    let edge = min(min(p.x, p.y), min(rect.size.x - 1.0 - p.x, rect.size.y - 1.0 - p.y));
    if (edge < 3.0) {
        c = vec4<f32>(0.20, 0.23, 0.28, 1.0);
    }
    textureStore(dst, vec2<u32>(rect.origin) + gid.xy, c);
}
