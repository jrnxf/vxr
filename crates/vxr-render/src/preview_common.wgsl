// Shared by the preview passes: per-frame parameters, per-dispatch parameters, the per-pixel
// accumulation record, and camera rays.

struct Frame {
    inv_vp: mat4x4<f32>,
    eye: vec4<f32>,
    size: vec4<f32>,     // width, height, 1/width, 1/height
    sun: vec4<f32>,      // xyz toward the sun, w = cos of the sun's angular radius
    sun_col: vec4<f32>,  // sun irradiance (linear rgb), w = exposure
    edit_lo: vec4<f32>,  // reset box of this frame's edit, voxel units
    edit_hi: vec4<f32>,
    cap_lo: vec4<f32>,   // box where history is capped after an edit (indirect light changed)
    cap_hi: vec4<f32>,
    // x: 1 = restart (camera moved), 2 = edit this frame; y: history cap after an edit;
    // z: frame counter (random seed); w: 1 = denoise on
    flags: vec4<u32>,
};

struct Dispatch {
    row0: u32,  // first row of this trace dispatch
    rows: u32,
    seed: u32,
    step: u32,  // A-trous step size
};

// One record per preview pixel. Radiance is stored demodulated (divided by the albedo of the
// primary hit) so a palette edit recolors the converged image at once.
struct Pixel {
    sum: vec3<f32>,  // sum of demodulated radiance
    lum2: f32,       // sum of luminance squared (variance estimate)
    t: f32,          // pixel-centre primary hit distance, -1 = sky
    info: u32,       // palette index | normal code << 8 | hit << 11
    n: f32,          // samples accumulated
    pad: f32,
};

const HIT_BIT: u32 = 2048u;

fn camera_ray(px: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(px.x * frame.size.z * 2.0 - 1.0, 1.0 - px.y * frame.size.w * 2.0);
    let pn = frame.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let pf = frame.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize(pf.xyz / pf.w - pn.xyz / pn.w);
}

// Normal from the packed code: axis * 2 + (normal is positive).
fn decode_normal(info: u32) -> vec3<f32> {
    let code = (info >> 8u) & 7u;
    var n = vec3<f32>(0.0);
    n[code >> 1u] = select(-1.0, 1.0, (code & 1u) == 1u);
    return n;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}
