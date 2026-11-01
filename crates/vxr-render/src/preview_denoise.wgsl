// Albedo-demodulated A-trous denoise and resolve for the preview. Prepended with common.wgsl.
//
// init:   per-pixel mean demodulated radiance + variance of the mean, and world position.
//         A pixel with no sample since the last restart (a coarse sample left it out, or the
//         rows have not reached it yet) is a hole: variance -1.
// atrous: 5x5 B-spline taps at a growing step, stopped at normal changes, plane distance
//         (voxel units) and luminance differences scaled by the variance (SVGF style), so the
//         filter fades out by itself as samples pile up. Holes take the filtered value of the
//         samples around them on the same face, so every pixel shows the current view.
// fill:   after a coarse sample (one pixel of each k x k block, at `coarse_offset`), in place
//         of the A-trous steps: each pixel takes the samples of the 3 x 3 blocks around it on
//         the same face, weighted by a tent over two blocks. Cheap at any size. Only the block
//         samples are read, so a coarse frame traces nothing else.
// resolve: remodulate with the current palette, tonemap, write the preview texture.

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> disp: Dispatch;
@group(0) @binding(2) var<storage, read> pixels: array<Pixel>;
@group(0) @binding(3) var<storage, read_write> aux: array<vec4<f32>>;  // world pos, packed normal + valid
@group(0) @binding(4) var<storage, read> src: array<vec4<f32>>;       // rgb, variance of luminance
@group(0) @binding(5) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(6) var<uniform> pal: array<vec4<f32>, 256>;
@group(0) @binding(7) var out_tex: texture_storage_2d<rgba8unorm, write>;

const SIGMA_L: f32 = 4.0;
const SIGMA_P: f32 = 1.0;  // voxels

fn index(p: vec2<i32>) -> u32 {
    return u32(p.y) * u32(frame.size.x) + u32(p.x);
}

// Mean and variance of the mean. With few samples the per-pixel estimate is unreliable, so
// borrow a 3x3 spatial estimate (SVGF does the same).
@compute @workgroup_size(8, 8)
fn init_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = i32(frame.size.x);
    let h = i32(frame.size.y);
    let p = vec2<i32>(gid.xy);
    if (p.x >= w || p.y >= h) { return; }
    let i = index(p);
    let px = pixels[i];
    let d = camera_ray(vec2<f32>(p) + 0.5);
    // The face from the restart's primary ray, so holes know where they are too. Metal and
    // glass faces (stored in colour) only filter with each other.
    var code = 1.0 + f32((px.info >> 8u) & 7u) + select(0.0, 16.0, (px.info & SPEC_BIT) != 0u);
    if (px.t < 0.0) { code = 8.0; }
    aux[i] = vec4<f32>(camera_origin(vec2<f32>(p) + 0.5) + d * max(px.t, 0.0), code);
    if (px.n <= 0.0) {
        dst[i] = vec4<f32>(0.0, 0.0, 0.0, -1.0);
        return;
    }
    let mean = px.sum / px.n;
    let ml = luminance(mean);
    var var_mean: f32;
    if (px.n >= 4.0) {
        var_mean = max(px.lum2 / px.n - ml * ml, 0.0) / px.n;
    } else {
        var s1 = 0.0;
        var s2 = 0.0;
        var c = 0.0;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let q = p + vec2<i32>(dx, dy);
                if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
                let o = pixels[index(q)];
                if (o.n <= 0.0) { continue; }
                let l = luminance(o.sum / o.n);
                s1 += l;
                s2 += l * l;
                c += 1.0;
            }
        }
        let m = s1 / c;
        var_mean = max(s2 / c - m * m, 0.0) / px.n + 0.01;
        // No sampled neighbour (a coarse sample): as uncertain as one sample's own value.
        if (c < 2.0) { var_mean = ml * ml / px.n + 0.01; }
    }
    dst[i] = vec4<f32>(mean, var_mean);
}

@compute @workgroup_size(8, 8)
fn atrous_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = i32(frame.size.x);
    let h = i32(frame.size.y);
    let p = vec2<i32>(gid.xy);
    if (p.x >= w || p.y >= h) { return; }
    let i = index(p);
    let c = src[i];
    let a = aux[i];
    let hole = c.a < 0.0;
    let sky = a.w == 8.0;
    // Sky pixels pass through; sky holes fill from the sky around them.
    if (sky && !hole) {
        dst[i] = c;
        return;
    }
    let n = decode_normal(select(u32(a.w - 1.0) & 7u, 0u, sky) << 8u);
    let lc = luminance(c.rgb);
    let sl = SIGMA_L * sqrt(max(c.a, 0.0)) + 1e-4;
    var k = array<f32, 5>(1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0);
    let step = i32(disp.step);
    var sum = vec3<f32>(0.0);
    var vsum = 0.0;
    var wsum = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let q = p + vec2<i32>(dx, dy) * step;
            if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
            let j = index(q);
            let aq = aux[j];
            if (aq.w != a.w) { continue; }  // same face orientation only (or sky with sky)
            let s = src[j];
            if (s.a < 0.0) { continue; }  // a hole has nothing to give
            let pd = dot(aq.xyz - a.xyz, n);
            let wp = select(exp(-pd * pd / (SIGMA_P * SIGMA_P)), 1.0, sky);
            // A hole has no value of its own to stop at.
            let wl = select(exp(-abs(luminance(s.rgb) - lc) / sl), 1.0, hole);
            let wt = k[dx + 2] * k[dy + 2] * wp * wl;
            sum += s.rgb * wt;
            vsum += s.a * wt * wt;
            wsum += wt;
        }
    }
    if (wsum <= 0.0) {
        dst[i] = c;
        return;
    }
    dst[i] = vec4<f32>(sum / wsum, vsum / (wsum * wsum));
}

@compute @workgroup_size(8, 8)
fn fill_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = i32(frame.size.x);
    let h = i32(frame.size.y);
    let p = vec2<i32>(gid.xy);
    if (p.x >= w || p.y >= h) { return; }
    let i = index(p);
    let a = aux[i];
    let sky = a.w == 8.0;
    // Sky samples pass through, as in the A-trous steps.
    if (sky && src[i].a >= 0.0) {
        dst[i] = src[i];
        return;
    }
    let n = decode_normal(select(u32(a.w - 1.0) & 7u, 0u, sky) << 8u);
    let k = i32(disp.step);
    let o = vec2<i32>(coarse_offset(disp.step));
    let b = p / k;
    var sum = vec3<f32>(0.0);
    var vsum = 0.0;
    var wsum = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let c = (b + vec2<i32>(dx, dy)) * k;
            if (c.x < 0 || c.y < 0 || c.x >= w || c.y >= h) { continue; }
            let q = min(c + o, vec2<i32>(w - 1, h - 1));
            let j = index(q);
            let s = src[j];
            if (s.a < 0.0) { continue; }
            let aq = aux[j];
            if (aq.w != a.w) { continue; }
            let pd = dot(aq.xyz - a.xyz, n);
            let wp = select(exp(-pd * pd / (SIGMA_P * SIGMA_P)), 1.0, sky);
            let t = abs(vec2<f32>(q - p)) / f32(k);
            let wt = max(1.0 - 0.5 * t.x, 0.0) * max(1.0 - 0.5 * t.y, 0.0) * wp;
            sum += s.rgb * wt;
            vsum += s.a * wt * wt;
            wsum += wt;
        }
    }
    if (wsum <= 0.0) {
        dst[i] = src[i];
        return;
    }
    dst[i] = vec4<f32>(sum / wsum, vsum / (wsum * wsum));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// disp.step = 1 resolves the filtered buffer (src); 0 resolves the raw accumulation.
@compute @workgroup_size(8, 8)
fn resolve_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(frame.size.x);
    let h = u32(frame.size.y);
    if (gid.x >= w || gid.y >= h) { return; }
    let i = gid.y * w + gid.x;
    let px = pixels[i];
    let f = src[i];
    // A hole the filter could not fill (no sample on its face nearby) shows its colour half
    // lit rather than what an older view left there.
    var irr = vec3<f32>(0.5);
    if (disp.step == 1u && f.a >= 0.0) {
        irr = f.rgb;
    } else if (px.n > 0.0) {
        irr = px.sum / px.n;
    }
    if (px.n <= 0.0 && px.t < 0.0 && (frame.flags.w & SKY_BACKDROP) == 0u) {
        irr = backdrop(vec2<f32>(gid.xy) + 0.5);
    }
    // The editor's backdrop uses its encoding (gamma 2.2, no tonemap) so the two match.
    // Hits and the sky backdrop are tonemapped; with depth of field the samples are already in
    // colour (a backdrop pixel's blurred edge mixes in a little of the scene, unmapped), as are
// metal and glass hits.
    var mapped = pow(clamp(irr, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    let dof = (frame.flags.w & DOF) != 0u;
    let spec = (px.info & SPEC_BIT) != 0u;
    if (((dof || spec) && px.t >= 0.0) || (px.t < 0.0 && (frame.flags.w & SKY_BACKDROP) != 0u)) {
        mapped = to_srgb(aces(irr * frame.sun_col.w));
    } else if (px.t >= 0.0) {
        mapped = to_srgb(aces(pal[px.info & 0xffu].rgb * irr * frame.sun_col.w));
    }
    // Gamma and vignette apply to the whole image, backdrop included, like a photo's post.
    mapped = pow(mapped, vec3<f32>(frame.post.x));
    let c = (vec2<f32>(gid.xy) + 0.5) * frame.size.zw - 0.5;
    mapped *= 1.0 - frame.post.y * 2.0 * dot(c, c);
    textureStore(out_tex, gid.xy, vec4<f32>(mapped, 1.0));
}
