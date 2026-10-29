// Shared by the preview passes: per-frame parameters, per-dispatch parameters, the per-pixel
// accumulation record, and camera rays.

struct Frame {
    inv_vp: mat4x4<f32>,
    eye: vec4<f32>,      // w = 0: orthographic
    size: vec4<f32>,     // width, height, 1/width, 1/height
    sun: vec4<f32>,      // xyz toward the sun, w = cos of the sun's angular radius
    sun_col: vec4<f32>,  // sun irradiance (linear rgb), w = exposure
    edit_lo: vec4<f32>,  // reset box of this frame's edit, voxel units
    edit_hi: vec4<f32>,
    cap_lo: vec4<f32>,   // box where history is capped after an edit (indirect light changed)
    cap_hi: vec4<f32>,
    // x: 1 = restart (camera moved), 2 = edit this frame; y: history cap after an edit;
    // z: frame counter (random seed); w: 1 = denoise on, 2 = sky backdrop, 4 = depth of field
    // (samples stored with their own hit's colour, not demodulated), 8 = physical sky,
    // 16 = hide a staged change's removals (`hidden` in preview_trace.wgsl)
    flags: vec4<u32>,
    cam_r: vec4<f32>,    // camera right, w = lens radius (voxels, 0 = pinhole)
    cam_u: vec4<f32>,    // camera up, w = focus distance along the view direction
    cam_f: vec4<f32>,    // view direction, w = sky light intensity
    post: vec4<f32>,     // x = 2.2 / display gamma, y = vignette strength
    backdrop: array<vec4<f32>, 2>, // the editor view's background gradient, inner and outer (sRGB)
};

const SKY_BACKDROP: u32 = 2u;
const DOF: u32 = 4u;
const PHYSICAL_SKY: u32 = 8u;
const HIDE: u32 = 16u;

struct Dispatch {
    row0: u32,  // first row of this trace dispatch
    rows: u32,
    seed: u32,
    step: u32,  // A-trous step size
};

// One record per preview pixel. Radiance is stored demodulated (divided by the albedo of the
// primary hit) so a palette edit recolors the converged image at once; metal and glass hits
// (SPEC_BIT) and depth of field are stored in colour.
struct Pixel {
    sum: vec3<f32>,  // sum of demodulated radiance
    lum2: f32,       // sum of luminance squared (variance estimate)
    t: f32,          // pixel-centre primary hit distance, -1 = sky
    info: u32,       // palette index | normal code << 8 | hit << 11 | metal or glass << 12
    n: f32,          // samples accumulated
    pad: f32,
};

const HIT_BIT: u32 = 2048u;
// The primary hit is metal or glass: its samples are stored in colour, not demodulated.
const SPEC_BIT: u32 = 4096u;

fn camera_ray(px: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(px.x * frame.size.z * 2.0 - 1.0, 1.0 - px.y * frame.size.w * 2.0);
    let pn = frame.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let pf = frame.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize(pf.xyz / pf.w - pn.xyz / pn.w);
}

// Where the ray through a pixel starts: the eye in perspective, the near plane in orthographic.
fn camera_origin(px: vec2<f32>) -> vec3<f32> {
    if (frame.eye.w != 0.0) { return frame.eye.xyz; }
    let ndc = vec2<f32>(px.x * frame.size.z * 2.0 - 1.0, 1.0 - px.y * frame.size.w * 2.0);
    let pn = frame.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    return pn.xyz / pn.w;
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
