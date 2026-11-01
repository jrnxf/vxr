// Progressive path tracer for the preview corner. Prepended with brickmap.wgsl (group 1:
// brick map, palette and trace_h()) and preview_common.wgsl.
//
// One sample = jittered primary ray, sun shadow ray (next-event estimation), one cosine-
// weighted diffuse bounce, and a sun shadow ray from the bounce hit. Metal, plastic and glass add
// specular events on the way (up to 4), emissive voxels add their light where a path hits.

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> disp: Dispatch;
@group(0) @binding(2) var<storage, read_write> pixels: array<Pixel>;
// Material per palette index v, two vec4s. mats[2v]: x = type (0 diffuse, 1 metal, 2 glass,
// 3 emit, 4 plastic), y = roughness, z = metalness (metal), refractive index (glass), radiance
// (emit) or specular (plastic), w = transparency (glass). mats[2v + 1]: x = attenuation (glass).
@group(0) @binding(3) var<uniform> mats: array<vec4<f32>, 512>;
// A staged change's ghost (2 bits per voxel, 32 words per brick slot, as the editor view's):
// cells marked 3 are removed by the change but still in the map, and rays pass through them.
// Read only with the HIDE flag.
@group(0) @binding(4) var<storage, read> ghost: array<u32>;

fn hidden(brick: u32, local: u32) -> bool {
    if ((frame.flags.w & HIDE) == 0u) { return false; }
    let w = brick * 32u + (local >> 4u);
    if (w >= arrayLength(&ghost)) { return false; }
    return ((ghost[w] >> ((local & 15u) * 2u)) & 3u) == 3u;
}

const M_METAL: u32 = 1u;
const M_GLASS: u32 = 2u;
const M_EMIT: u32 = 3u;
const M_PLASTIC: u32 = 4u;
const MAX_GLASS: u32 = 24u;
const MAX_SPECULAR: u32 = 4u;

fn kind(v: u32) -> u32 {
    return u32(mats[2u * v].x + 0.5);
}

// Metal, plastic and glass pixels are stored with their colour (not demodulated).
fn specular(v: u32) -> bool {
    let k = kind(v);
    return k == M_METAL || k == M_GLASS || k == M_PLASTIC;
}

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
    if ((frame.flags.w & PHYSICAL_SKY) != 0u) {
        return physical_sky(d);
    }
    let up = d.y;
    if (up < 0.0) {
        // Below the horizon: a dim ground bounce colour.
        return mix(vec3<f32>(0.42, 0.44, 0.48), vec3<f32>(0.20, 0.19, 0.18), clamp(-up * 3.0, 0.0, 1.0));
    }
    let k = sqrt(clamp(up, 0.0, 1.0));
    return mix(vec3<f32>(0.70, 0.80, 0.95), vec3<f32>(0.26, 0.46, 0.86), k);
}

// The physical sky: single scattering in a thin atmosphere lit by the sun. Rayleigh
// scattering makes it blue overhead and paler toward the horizon, Mie scattering (haze) a glow
// around the sun, and the sun's light reddens as it crosses more air near the horizon, where
// ozone also takes out some green so twilight stays blue rather than grey. Optical depths are
// per vertical air column (at 680, 550 and 440 nm).
const RAYLEIGH: vec3<f32> = vec3<f32>(0.05, 0.12, 0.25);
const MIE: f32 = 0.01;
const OZONE: vec3<f32> = vec3<f32>(0.02, 0.05, 0.002);
// Scales the scattered light so the default sun's sky is about as bright as the gradient sky.
const SKY_GAIN: f32 = 10.0;

// Air crossed along a ray that climbs at sine c, relative to straight up.
fn airmass(c: f32) -> f32 {
    return 1.0 / (max(c, 0.0) + 0.035);
}

// The sun's light after crossing the air at the sun's height (the sun disk's colour).
fn sun_transmit() -> vec3<f32> {
    return exp(-(RAYLEIGH + MIE + OZONE) * airmass(frame.sun.y));
}

// Sunlight scattered toward the eye along a view ray climbing at d.y >= 0.
fn scattered(d: vec3<f32>) -> vec3<f32> {
    let mu = dot(d, frame.sun.xyz);
    let ext = RAYLEIGH + MIE;
    // In-scattered light along the view ray, attenuated on its way to the eye.
    let along = (vec3<f32>(1.0) - exp(-ext * airmass(d.y))) / ext;
    let pr = 3.0 / (16.0 * PI) * (1.0 + mu * mu);
    let g = 0.76;
    let pm = (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * mu, 1.5));
    return frame.sun_col.rgb * sun_transmit() * along * (RAYLEIGH * pr + MIE * pm) * SKY_GAIN;
}

fn physical_sky(d: vec3<f32>) -> vec3<f32> {
    let l = scattered(vec3<f32>(d.x, max(d.y, 0.0), d.z));
    if (d.y >= 0.0) {
        return l;
    }
    // Below the horizon: the haze at the horizon fading into a ground lit by the sun and the
    // sky overhead.
    let lit = frame.sun_col.rgb * sun_transmit() * max(frame.sun.y, 0.0) + scattered(vec3<f32>(0.0, 1.0, 0.0)) * PI;
    let ground = vec3<f32>(0.30, 0.28, 0.25) * lit / PI;
    return mix(l, ground, clamp(-d.y * 12.0, 0.0, 1.0));
}

// The sun disk seen straight along a camera ray (other rays find the sun by sampling it).
fn sun_disk(d: vec3<f32>) -> vec3<f32> {
    let c = dot(d, frame.sun.xyz);
    if (c < frame.sun.w) {
        return vec3<f32>(0.0);
    }
    return frame.sun_col.rgb * sun_transmit() * 40.0;
}

// Distance along ro + t d where the ray leaves the voxel it is in at distance t, and the
// axis of that face.
fn voxel_exit(ro: vec3<f32>, d: vec3<f32>, t: f32) -> vec2<f32> {
    let c = floor(ro + d * (t + 1e-3));
    let dd = select(d, vec3<f32>(1e-7), abs(d) < vec3<f32>(1e-7));
    let tf = (c + select(vec3<f32>(0.0), vec3<f32>(1.0), dd > vec3<f32>(0.0)) - ro) / dd;
    if (tf.x <= tf.y && tf.x <= tf.z) { return vec2<f32>(tf.x, 0.0); }
    if (tf.y <= tf.z) { return vec2<f32>(tf.y, 1.0); }
    return vec2<f32>(tf.z, 2.0);
}

// A ray that passes straight through glass voxels (thin glass: no bending inside), each one
// tinting it by its colour as much as it is not transparent, and attenuation taking on more
// of the colour the longer the ray's run inside (Beer-Lambert, the colour as the transmittance
// of one voxel at full attenuation). Returns the first other hit and the tint in `tint`. Too
// much glass on the way counts as opaque.
fn tr_solid(ro: vec3<f32>, d: vec3<f32>, t_max: f32, tint: ptr<function, vec3<f32>>) -> Hit {
    var t0 = 0.0;
    var axis = 4u;
    for (var k = 0u; k < MAX_GLASS; k++) {
        var h = tr(ro, d, t0, t_max);
        if (h.axis != MISS && axis < 3u && h.t <= t0 + 1e-3) {
            // The hit is the voxel right behind the glass: it was entered through the face
            // the glass was left by, not the one the traversal set up with.
            h.axis = axis;
        }
        if (h.axis == MISS || kind(h.value) != M_GLASS) {
            return h;
        }
        let c = palette[h.value].rgb;
        let e = voxel_exit(ro, d, h.t);
        let att = mats[2u * h.value + 1u].x * max(e.x - h.t, 0.0);
        *tint *= mix(c, vec3<f32>(1.0), mats[2u * h.value].w) * pow(max(c, vec3<f32>(1e-3)), vec3<f32>(att));
        t0 = e.x + 1e-4;
        axis = u32(e.y);
    }
    *tint = vec3<f32>(0.0);
    var h: Hit;
    h.axis = 0u;
    h.t = t0;
    return h;
}

// Sun light arriving at a surface point with normal n, divided by the albedo (Lambert / pi).
fn sun_light(p: vec3<f32>, n: vec3<f32>, s: ptr<function, u32>) -> vec3<f32> {
    let l = sun_dir(s);
    let ndl = dot(n, l);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    var tint = vec3<f32>(1.0);
    let h = tr_solid(p + n * EPS, l, 1e9, &tint);
    if (h.axis != MISS) {
        return vec3<f32>(0.0);
    }
    return frame.sun_col.rgb * tint * ndl / PI;
}

// Sun highlight on a glossy surface around the mirror direction r (normalized Phong lobe).
fn sun_gloss(p: vec3<f32>, n: vec3<f32>, r: vec3<f32>, rough: f32, s: ptr<function, u32>) -> vec3<f32> {
    let l = sun_dir(s);
    let ndl = dot(n, l);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    let e = clamp(2.0 / (rough * rough + 1e-3), 1.0, 2000.0);
    let lobe = (e + 2.0) / (2.0 * PI) * pow(max(dot(r, l), 0.0), e);
    if (lobe < 1e-4) {
        return vec3<f32>(0.0);
    }
    var tint = vec3<f32>(1.0);
    let h = tr_solid(p + n * EPS, l, 1e9, &tint);
    if (h.axis != MISS) {
        return vec3<f32>(0.0);
    }
    return frame.sun_col.rgb * tint * min(lobe * ndl, 8.0);
}

// A direction in the unit ball.
fn in_ball(s: ptr<function, u32>) -> vec3<f32> {
    let z = rand(s) * 2.0 - 1.0;
    let phi = 2.0 * PI * rand(s);
    let r = sqrt(max(0.0, 1.0 - z * z));
    return vec3<f32>(r * cos(phi), r * sin(phi), z) * pow(rand(s), 1.0 / 3.0);
}

// Mirror direction of d about n, spread by roughness; None (zero) if it points into the surface.
fn glossy(d: vec3<f32>, n: vec3<f32>, rough: f32, s: ptr<function, u32>) -> vec3<f32> {
    let r = normalize(reflect(d, n) + in_ball(s) * rough);
    return select(vec3<f32>(0.0), r, dot(r, n) > 0.0);
}

fn pack_info(h: Hit, d: vec3<f32>) -> u32 {
    let positive = select(0u, 1u, d[h.axis] < 0.0);
    return h.value | ((h.axis * 2u + positive) << 8u) | HIT_BIT | select(0u, SPEC_BIT, specular(h.value));
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
    let o = camera_origin(vec2<f32>(gid.xy) + 0.5);
    var redo = (frame.flags.x & 1u) != 0u;
    if (!redo && (frame.flags.x & 2u) != 0u) {
        let lo = frame.edit_lo.xyz;
        let hi = frame.edit_hi.xyz;
        let t_end = select(1e9, px.t + 0.5, px.t >= 0.0);
        if (ray_box(o, d, lo, hi, 0.0, t_end)) {
            redo = true;
        } else if (px.t >= 0.0) {
            let n = decode_normal(px.info);
            let p = o + d * px.t;
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
        let hit = tr(o, d, 0.0, 1e9);
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
    // disp.step > 1: a coarse sample, one pixel of each step x step block (rows count blocks),
    // so a whole image of the current view fits the budget while the camera moves.
    let blk = max(disp.step, 1u);
    if (gid.x * blk >= w || gid.y >= disp.rows || (disp.row0 + gid.y) * blk >= hgt) { return; }
    var s = ((disp.row0 + gid.y) * w + gid.x) * 9781u + 6271u;
    s = s ^ (disp.seed * 26699u) ^ (frame.flags.z * 0x9e3779b9u);
    _ = pcg(&s);
    var x = gid.x;
    var y = disp.row0 + gid.y;
    if (blk > 1u) {
        let o = coarse_offset(blk);
        x = min(x * blk + o.x, w - 1u);
        y = min(y * blk + o.y, hgt - 1u);
    }
    let i = y * w + x;

    let jitter = vec2<f32>(rand(&s), rand(&s));
    var d = camera_ray(vec2<f32>(f32(x), f32(y)) + jitter);
    var ro = camera_origin(vec2<f32>(f32(x), f32(y)) + jitter);
    let dof = (frame.flags.w & DOF) != 0u;
    if (dof) {
        // Thin lens: a point on the lens disc, aimed at the pixel's point on the focus plane.
        let focus_p = ro + d * (frame.cam_u.w / max(dot(d, frame.cam_f.xyz), 1e-3));
        let r = sqrt(rand(&s)) * frame.cam_r.w;
        let phi = 2.0 * PI * rand(&s);
        ro += frame.cam_r.xyz * (r * cos(phi)) + frame.cam_u.xyz * (r * sin(phi));
        d = normalize(focus_p - ro);
    }
    let jpx = vec2<f32>(f32(x), f32(y)) + jitter;
    let sky_backdrop = (frame.flags.w & SKY_BACKDROP) != 0u;
    // Walk the path. `thr` is what the light found so far is multiplied by. A diffuse primary
    // hit is stored divided by its colour (the resolve multiplies the current palette back
    // in, so recolours need no restart); metal, glass and depth of field are stored in colour.
    var thr = vec3<f32>(1.0);
    var demod = vec3<f32>(0.0);
    var h = tr(ro, d, 0.0, 1e9);
    let demodulate = !dof && h.axis != MISS && !specular(h.value);
    var straight = true;  // still the camera ray, seen through glass at most
    var diffuse_hits = 0u;
    var spec_hits = 0u;
    loop {
        if (h.axis == MISS) {
            if (straight && !sky_backdrop) {
                demod += thr * backdrop(jpx);
            } else {
                demod += thr * sky(d) * frame.cam_f.w;
                if (straight && (frame.flags.w & PHYSICAL_SKY) != 0u) {
                    demod += thr * sun_disk(d);
                }
            }
            break;
        }
        let n = hit_normal(h, d);
        let p = ro + d * h.t;
        let m = mats[2u * h.value];
        let k = kind(h.value);
        let first = diffuse_hits == 0u && spec_hits == 0u && straight;
        let alb = select(palette[h.value].rgb, vec3<f32>(1.0), first && demodulate);
        if (k == M_EMIT) {
            demod += thr * alb * m.z;
        }
        if (spec_hits < MAX_SPECULAR && (k == M_GLASS || (k == M_METAL && rand(&s) < m.z))) {
            spec_hits += 1u;
            var reflect_it = true;
            if (k == M_GLASS) {
                // Schlick's Fresnel: reflect, or pass through the glass run.
                let f0 = pow((m.z - 1.0) / (m.z + 1.0), 2.0);
                let c = clamp(-dot(d, n), 0.0, 1.0);
                reflect_it = rand(&s) < f0 + (1.0 - f0) * pow(1.0 - c, 5.0);
            } else {
                thr *= alb;
                demod += thr * sun_gloss(p, n, reflect(d, n), m.y, &s);
            }
            if (reflect_it) {
                let r = glossy(d, n, m.y, &s);
                if (all(r == vec3<f32>(0.0))) { break; }
                ro = p + n * EPS;
                d = r;
                straight = false;
                h = tr(ro, d, 0.0, 1e9);
            } else {
                // Frosted glass spreads the ray a little; clear glass keeps the camera ray.
                if (m.y > 0.0) {
                    d = normalize(d + in_ball(&s) * m.y * 0.5);
                    straight = false;
                }
                ro = p + d * EPS;
                var tint = vec3<f32>(1.0);
                h = tr_solid(ro, d, 1e9, &tint);
                thr *= tint;
            }
            continue;
        }
        if (k == M_PLASTIC && spec_hits < MAX_SPECULAR) {
            // A clear coat over the diffuse colour: Schlick's Fresnel for a 1.5 index, scaled
            // by the specular setting, picks a white glossy reflection; the rest is diffuse.
            let c = clamp(-dot(d, n), 0.0, 1.0);
            if (rand(&s) < m.z * (0.04 + 0.96 * pow(1.0 - c, 5.0))) {
                spec_hits += 1u;
                let r = glossy(d, n, m.y, &s);
                if (all(r == vec3<f32>(0.0))) { break; }
                demod += thr * sun_gloss(p, n, reflect(d, n), m.y, &s);
                ro = p + n * EPS;
                d = r;
                straight = false;
                h = tr(ro, d, 0.0, 1e9);
                continue;
            }
        }
        // Diffuse: direct sun, then one bounce lit by the sun or the sky.
        demod += thr * alb * sun_light(p, n, &s);
        diffuse_hits += 1u;
        if (diffuse_hits == 2u) { break; }
        thr *= alb;
        ro = p + n * EPS;
        d = cosine_dir(n, &s);
        straight = false;
        h = tr(ro, d, 0.0, 1e9);
    }
    var px = pixels[i];
    px.sum += demod;
    let lum = luminance(demod);
    px.lum2 += lum * lum;
    px.n += 1.0;
    pixels[i] = px;
}
