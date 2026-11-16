// Draws the marched picture (embed.wgsl's `img`) onto the canvas.

@group(0) @binding(0) var<uniform> res: vec4<u32>;
@group(0) @binding(1) var<storage, read> img: array<u32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let x = min(u32(p.x), res.x - 1u);
    let y = min(u32(p.y), res.y - 1u);
    return unpack4x8unorm(img[x + y * res.x]);
}
