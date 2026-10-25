// Progressive path tracer for the preview corner. Prepended with brickmap.wgsl (group 1:
// brick map, palette and trace_h()) and preview_common.wgsl.
//
// One sample = jittered primary ray, sun shadow ray (next-event estimation), one cosine-
// weighted diffuse bounce, and a sun shadow ray from the bounce hit. Up to 4 rays.

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> disp: Dispatch;
@group(0) @binding(2) var<storage, read_write> pixels: array<Pixel>;

fn tr(ro: vec3<f32>, d: vec3<f32>, t_min: f32, t_max: f32) -> Hit {
    return trace_h(ro, d, t_min, t_max);
}

const PI: f32 = 3.14159265;
const EPS: f32 = 0.002;

fn pcg(s: ptr<function, u32>) -> u32 {
    *s = *s * 747796405u + 2891336453u;
    let w = ((*s >> ((*s >> 28u) + 4u)) ^ *s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rand(s: ptr<function, u32>) -> f32 {
    return f32(pcg(s) >> 8u) * (1.0 / 16777216.0);
}

// Orthonormal basis around n (Duff et al. 2017).
fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bt = vec3<f32>(b, s + n.y * n.y * a, -n.y);
    return mat3x3<f32>(t, bt, n);
}

fn cosine_dir(n: vec3<f32>, s: ptr<function, u32>) -> vec3<f32> {
    let u1 = rand(s);
    let u2 = rand(s);
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    return normalize(basis(n) * vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(0.0, 1.0 - u1))));
}

// Uniform direction inside the sun's cone (soft shadows).
fn sun_dir(s: ptr<function, u32>) -> vec3<f32> {
    let cos_max = frame.sun.w;
    let u1 = rand(s);
    let u2 = rand(s);
    let ct = 1.0 - u1 * (1.0 - cos_max);
    let st = sqrt(max(0.0, 1.0 - ct * ct));
    let phi = 2.0 * PI * u2;
    return normalize(basis(frame.sun.xyz) * vec3<f32>(st * cos(phi), st * sin(phi), ct));
}

fn sky(d: vec3<f32>) -> vec3<f32> {
    let up = d.y;
    if (up < 0.0) {
        // Below the horizon: a dim ground bounce colour.
        return mix(vec3<f32>(0.42, 0.44, 0.48), vec3<f32>(0.20, 0.19, 0.18), clamp(-up * 3.0, 0.0, 1.0));
    }
    let k = sqrt(clamp(up, 0.0, 1.0));
    return mix(vec3<f32>(0.70, 0.80, 0.95), vec3<f32>(0.26, 0.46, 0.86), k);
}

// Sun light arriving at a surface point with normal n, divided by the albedo (Lambert / pi).
fn sun_light(p: vec3<f32>, n: vec3<f32>, s: ptr<function, u32>) -> vec3<f32> {
    let l = sun_dir(s);
    let ndl = dot(n, l);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = tr(p + n * EPS, l, 0.0, 1e9);
    if (h.axis != MISS) {
        return vec3<f32>(0.0);
    }
    return frame.sun_col.rgb * ndl / PI;
}

fn pack_info(h: Hit, d: vec3<f32>) -> u32 {
    let positive = select(0u, 1u, d[h.axis] < 0.0);
    return h.value | ((h.axis * 2u + positive) << 8u) | HIT_BIT;
}

fn ray_box(ro: vec3<f32>, d: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>, t0: f32, t1: f32) -> bool {
    let inv = 1.0 / select(d, vec3<f32>(1e-7), abs(d) < vec3<f32>(1e-7));
    let ta = (lo - ro) * inv;
    let tb = (hi - ro) * inv;
    let tn = max(max(min(ta.x, tb.x), min(ta.y, tb.y)), min(ta.z, tb.z));
    let tf = min(min(max(ta.x, tb.x), max(ta.y, tb.y)), max(ta.z, tb.z));
    return max(tn, t0) <= min(tf, t1);
}

fn in_box(p: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> bool {
    return all(p >= lo) && all(p <= hi);
}

// Restart (camera moved) or edit: decide per pixel whether its history is still valid.
// After an edit a pixel restarts if its primary ray or its sun shadow ray crosses the edited
// box; pixels near the edit keep at most `cap` samples so changed indirect light fades in.
@compute @workgroup_size(8, 8)
fn reset_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(frame.size.x);
    let h = u32(frame.size.y);
    if (gid.x >= w || gid.y >= h) { return; }
    let i = gid.y * w + gid.x;
    var px = pixels[i];
    let d = camera_ray(vec2<f32>(gid.xy) + 0.5);
    var redo = (frame.flags.x & 1u) != 0u;
    if (!redo && (frame.flags.x & 2u) != 0u) {
        let lo = frame.edit_lo.xyz;
        let hi = frame.edit_hi.xyz;
        let t_end = select(1e9, px.t + 0.5, px.t >= 0.0);
        if (ray_box(frame.eye.xyz, d, lo, hi, 0.0, t_end)) {
            redo = true;
        } else if (px.t >= 0.0) {
            let n = decode_normal(px.info);
            let p = frame.eye.xyz + d * px.t;
            if (ray_box(p + n * EPS, frame.sun.xyz, lo, hi, 0.0, 1e9)) {
                redo = true;
            } else if (in_box(p, frame.cap_lo.xyz, frame.cap_hi.xyz)) {
                let cap = f32(frame.flags.y);
                if (px.n > cap) {
                    let k = cap / px.n;
                    px.sum *= k;
                    px.lum2 *= k;
                    px.n = cap;
                }
            }
        }
    }
    if (redo) {
        let hit = tr(frame.eye.xyz, d, 0.0, 1e9);
        if (hit.axis != MISS) {
            px.t = hit.t;
            px.info = pack_info(hit, d);
        } else {
            px.t = -1.0;
            px.info = 0u;
        }
        px.sum = vec3<f32>(0.0);
        px.lum2 = 0.0;
        px.n = 0.0;
    }
    pixels[i] = px;
}

@compute @workgroup_size(8, 8)
fn trace_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(frame.size.x);
    let hgt = u32(frame.size.y);
    let y = disp.row0 + gid.y;
    if (gid.x >= w || gid.y >= disp.rows || y >= hgt) { return; }
    let i = y * w + gid.x;
    var s = (i * 9781u + 6271u) ^ (disp.seed * 26699u) ^ (frame.flags.z * 0x9e3779b9u);
    _ = pcg(&s);

    let jitter = vec2<f32>(rand(&s), rand(&s));
    var d = camera_ray(vec2<f32>(f32(gid.x), f32(y)) + jitter);
    var ro = frame.eye.xyz;
    let dof = (frame.flags.w & DOF) != 0u;
    if (dof) {
        // Thin lens: a point on the lens disc, aimed at the pixel's point on the focus plane.
        let focus_p = ro + d * (frame.cam_u.w / max(dot(d, frame.cam_f.xyz), 1e-3));
        let r = sqrt(rand(&s)) * frame.cam_r.w;
        let phi = 2.0 * PI * rand(&s);
        ro += frame.cam_r.xyz * (r * cos(phi)) + frame.cam_u.xyz * (r * sin(phi));
        d = normalize(focus_p - ro);
    }
    let hit = tr(ro, d, 0.0, 1e9);
    var demod: vec3<f32>;
    if (hit.axis == MISS && (frame.flags.w & SKY_BACKDROP) != 0u) {
        demod = sky(d) * frame.cam_f.w;
    } else if (hit.axis == MISS) {
        // Backdrop: the editor view's background (march.wgsl), so the corner sits in the
        // Hybrid UI like the main view does. The sky still lights the scene.
        let uv = vec2<f32>(f32(gid.x), f32(y)) * frame.size.zw + jitter * frame.size.zw - vec2<f32>(0.5, 0.35);
        let k = clamp(length(uv / vec2<f32>(1.2, 0.9)) / 0.7, 0.0, 1.0);
        demod = pow(mix(vec3<f32>(38.0, 43.0, 55.0), vec3<f32>(13.0, 15.0, 20.0), k) / 255.0, vec3<f32>(2.2));
    } else {
        let n = hit_normal(hit, d);
        let p = ro + d * hit.t;
        // Direct sun, then one diffuse bounce lit by the sun or the sky.
        var l = sun_light(p, n, &s);
        let b = cosine_dir(n, &s);
        let h2 = tr(p + n * EPS, b, 0.0, 1e9);
        if (h2.axis == MISS) {
            l += sky(b) * frame.cam_f.w;
        } else {
            let n2 = hit_normal(h2, b);
            let p2 = p + n * EPS + b * h2.t;
            l += palette[h2.value].rgb * sun_light(p2, n2, &s);
        }
        demod = l;
        if (dof) {
            demod *= palette[hit.value].rgb;
        }
    }
    var px = pixels[i];
    px.sum += demod;
    let lum = luminance(demod);
    px.lum2 += lum * lum;
    px.n += 1.0;
    pixels[i] = px;
}
