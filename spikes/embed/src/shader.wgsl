// Embed viewer: two-level DDA ray march over an 8^3 brick map, sun + sky lighting, hard shadows.
// Z is up (MagicaVoxel space).

struct Cam {
  eye: vec4f,   // xyz eye, w tan(fov / 2)
  fwd: vec4f,   // w aspect
  right: vec4f,
  up: vec4f,
  sun: vec4f,
  dims: vec4u,  // xyz grid size in bricks
  res: vec4u,   // x width, y height, w always 0 (used with NONCE)
}

@group(0) @binding(0) var<uniform> cam: Cam;
@group(0) @binding(1) var<storage, read> grid: array<u32>;
@group(0) @binding(2) var<storage, read> masks: array<u32>;
@group(0) @binding(3) var<storage, read> colors: array<u32>;
@group(0) @binding(4) var<storage, read> palette: array<u32>;
// Output pixels as packed RGBA8 in a storage buffer. A storage texture would be the obvious
// choice, but writing one made Chrome's Metal pipeline compile about 70 ms slower.
@group(0) @binding(5) var<storage, read_write> img: array<u32>;
@group(0) @binding(6) var<storage, read> src: array<u32>;

// Bench mode swaps this for a random value so every load compiles a fresh shader.
const NONCE: u32 = 4294967295u;

struct Hit { t: f32, n: vec3f, ci: u32 }

fn trace(ro: vec3f, rd0: vec3f) -> Hit {
  var h: Hit;
  h.t = -1.0;
  let rd = rd0 + select(vec3f(0.0), vec3f(1e-7), abs(rd0) < vec3f(1e-7));
  let inv = 1.0 / rd;
  let dims = vec3i(cam.dims.xyz);
  let size = vec3f(cam.dims.xyz) * 8.0;
  let ta = -ro * inv;
  let tb = (size - ro) * inv;
  let t0 = min(ta, tb);
  let t1 = max(ta, tb);
  let tn = max(max(t0.x, t0.y), max(t0.z, 0.0));
  let tf = min(min(t1.x, t1.y), t1.z);
  if (tn >= tf) { return h; }

  let stp = select(vec3i(-1), vec3i(1), rd > vec3f(0.0));
  let up = vec3f(max(stp, vec3i(0)));
  var n = vec3f(0.0);
  if (tn > 0.0) {
    if (t0.x >= t0.y && t0.x >= t0.z) { n.x = -f32(stp.x); }
    else if (t0.y >= t0.z) { n.y = -f32(stp.y); }
    else { n.z = -f32(stp.z); }
  }

  var t = tn;
  var cb = clamp(vec3i(floor((ro + rd * (tn + 1e-4)) / 8.0)), vec3i(0), dims - 1);
  var tm = ((vec3f(cb) + up) * 8.0 - ro) * inv;
  let td = abs(8.0 * inv);
  let tdv = abs(inv);

  for (var i = 0; i < 2048; i++) {
    let b = grid[u32(cb.x) + cam.dims.x * (u32(cb.y) + cam.dims.y * u32(cb.z))];
    if (b != 0u) {
      let bi = b - 1u;
      let base = cb * 8;
      var cv = clamp(vec3i(floor(ro + rd * (t + 1e-4))), base, base + 7);
      var tv = ((vec3f(cv) + up) - ro) * inv;
      var nv = n;
      var tt = t;
      for (var j = 0; j < 24; j++) {
        let l = vec3u(cv - base);
        let li = l.x + 8u * (l.y + 8u * l.z);
        if ((masks[bi * 16u + (li >> 5u)] & (1u << (li & 31u))) != 0u) {
          h.t = tt;
          h.n = nv;
          h.ci = (colors[bi * 128u + (li >> 2u)] >> ((li & 3u) * 8u)) & 255u;
          return h;
        }
        if (tv.x < tv.y && tv.x < tv.z) { cv.x += stp.x; tt = tv.x; tv.x += tdv.x; nv = vec3f(-f32(stp.x), 0.0, 0.0); }
        else if (tv.y < tv.z) { cv.y += stp.y; tt = tv.y; tv.y += tdv.y; nv = vec3f(0.0, -f32(stp.y), 0.0); }
        else { cv.z += stp.z; tt = tv.z; tv.z += tdv.z; nv = vec3f(0.0, 0.0, -f32(stp.z)); }
        if (any(cv < base) || any(cv > base + 7)) { break; }
      }
    }
    if (tm.x < tm.y && tm.x < tm.z) { cb.x += stp.x; t = tm.x; tm.x += td.x; n = vec3f(-f32(stp.x), 0.0, 0.0); }
    else if (tm.y < tm.z) { cb.y += stp.y; t = tm.y; tm.y += td.y; n = vec3f(0.0, -f32(stp.y), 0.0); }
    else { cb.z += stp.z; t = tm.z; tm.z += td.z; n = vec3f(0.0, 0.0, -f32(stp.z)); }
    if (t >= tf || any(cb < vec3i(0)) || any(cb >= dims)) { break; }
  }
  return h;
}

fn sky(d: vec3f) -> vec3f {
  return mix(vec3f(0.93, 0.92, 0.89), vec3f(0.78, 0.84, 0.93), clamp(d.z * 0.8 + 0.3, 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn march(@builtin(global_invocation_id) id: vec3u) {
  if (id.x >= cam.res.x || id.y >= cam.res.y) { return; }
  let uv = (vec2f(id.xy) + 0.5) / vec2f(cam.res.xy) * 2.0 - 1.0;
  let rd = normalize(cam.fwd.xyz + cam.right.xyz * (uv.x * cam.eye.w * cam.fwd.w) - cam.up.xyz * (uv.y * cam.eye.w));
  var col = sky(rd);
  // Primary ray, then a shadow ray toward the sun, through one call site so the
  // march loop is compiled once (halves shader compile time).
  var ro = cam.eye.xyz;
  var dir = rd;
  var albedo = vec3f(0.0);
  var amb = 0.0;
  var ndl = 0.0;
  for (var k = 0; k < 2; k++) {
    let h = trace(ro, dir);
    if (k == 0) {
      if (h.t < 0.0) { break; }
      albedo = pow(unpack4x8unorm(palette[h.ci]).rgb, vec3f(2.2));
      amb = 0.30 + 0.12 * h.n.z + 0.04 * h.n.x;
      ndl = max(dot(h.n, cam.sun.xyz), 0.0);
      ro = ro + dir * h.t + h.n * 1e-3;
      dir = cam.sun.xyz;
      if (ndl <= 0.0) { k = 2; }
    } else if (h.t >= 0.0) {
      ndl = 0.0;
    }
  }
  if (amb > 0.0) {
    col = pow(albedo * (amb + ndl * 0.85), vec3f(1.0 / 2.2));
  }
  if (cam.res.w == NONCE) { col = col.bgr; }
  img[id.x + id.y * cam.res.x] = pack4x8unorm(vec4f(col, 1.0));
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) p: vec4f) -> @location(0) vec4f {
  return unpack4x8unorm(src[u32(p.x) + u32(p.y) * cam.res.x]);
}
