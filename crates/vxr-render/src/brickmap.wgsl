// Brick map bindings and ray traversal, shared by the editor view (march.wgsl) and any
// other pass that traces rays through the scene. Bind group 1. The pass that includes it
// defines `hidden(brick, local) -> bool`: cells in the map that its rays pass through (the
// path-traced preview hides a staged change's removals; the editor view hides none).
//
// Layout: a dense grid of brick slots (EMPTY or a brick id), 16 mask words per brick
// (512 bits, local index x | y << 3 | z << 6), and a coarse level of 64 bits per 4x4x4
// bricks (32^3 voxels) so empty space is skipped 32 voxels at a time. Palette bytes are
// stored for occupied voxels only, in a heap of up to four pages (128 MiB, or fewer bytes in tests). After the masks
// come two words per brick (masks[map_info.sdims.w + brick * 2]): the byte offset of the
// brick's bytes in the heap, and set-bit counts of the first 4, 8 and 12 mask words. A
// voxel's byte is at the offset plus the set bits below it.

struct MapInfo {
    bdims: vec4<u32>, // bricks per axis; w = log2 of the heap page size in bytes
    sdims: vec4<u32>, // coarse cells per axis (4x4x4 bricks each); w = word index of the
                      // per-brick heap offsets inside `masks`
    lo: vec4<f32>,    // tight bounds of occupied bricks, voxel units
    hi: vec4<f32>,
};

@group(1) @binding(0) var<uniform> map_info: MapInfo;
// Linear RGB per palette index. Editing a slot recolors every voxel using it.
@group(1) @binding(1) var<uniform> palette: array<vec4<f32>, 256>;
@group(1) @binding(2) var<storage, read> grid: array<u32>;
@group(1) @binding(3) var<storage, read> masks: array<u32>;
@group(1) @binding(4) var<storage, read> coarse: array<u32>;
@group(1) @binding(5) var<storage, read> pal0: array<u32>;
@group(1) @binding(6) var<storage, read> pal1: array<u32>;
@group(1) @binding(7) var<storage, read> pal2: array<u32>;
@group(1) @binding(8) var<storage, read> pal3: array<u32>;

const EMPTY: u32 = 0xffffffffu;
const MAX_STEPS: u32 = 4096u;
const MISS: u32 = 3u;

struct Hit {
    t: f32,      // ray distance to the hit face
    axis: u32,   // axis of the face crossed (0, 1, 2), or MISS
    brick: u32,
    local: u32,
    value: u32,  // palette index
    steps: u32,  // traversal steps taken
};

fn heap_byte(pos: u32) -> u32 {
    let shift = map_info.bdims.w;
    let page = pos >> shift;
    let word = (pos & ((1u << shift) - 1u)) >> 2u;
    var w: u32;
    switch page {
        case 0u: { w = pal0[word]; }
        case 1u: { w = pal1[word]; }
        case 2u: { w = pal2[word]; }
        default: { w = pal3[word]; }
    }
    return (w >> ((pos & 3u) * 8u)) & 0xffu;
}

// Rank of a voxel among the brick's occupied voxels: set mask bits below it. `prefix` holds
// the counts for mask words 0..4, 0..8 and 0..12, 9 bits each.
fn compact_rank(brick: u32, local: u32, prefix: u32) -> u32 {
    let base = brick * 16u;
    let wi = local >> 5u;
    let q = wi >> 2u;
    var rank = select(0u, (prefix >> (9u * (max(q, 1u) - 1u))) & 0x1ffu, q > 0u);
    for (var i = q * 4u; i < wi; i += 1u) {
        rank += countOneBits(masks[base + i]);
    }
    return rank + countOneBits(masks[base + wi] & ((1u << (local & 31u)) - 1u));
}

fn palette_value(brick: u32, local: u32) -> u32 {
    let at = map_info.sdims.w + brick * 2u;
    return heap_byte(masks[at] + compact_rank(brick, local, masks[at + 1u]));
}

fn mask_bit(brick: u32, local: u32) -> bool {
    return ((masks[brick * 16u + (local >> 5u)] >> (local & 31u)) & 1u) != 0u;
}

fn min_axis(v: vec3<f32>) -> u32 {
    if (v.x < v.y && v.x < v.z) { return 0u; }
    if (v.y < v.z) { return 1u; }
    return 2u;
}

fn grid_at(c: vec3<i32>) -> u32 {
    return grid[u32(c.x) + map_info.bdims.x * (u32(c.y) + map_info.bdims.y * u32(c.z))];
}

// Normal of the face hit: minus the step direction on the crossed axis.
fn hit_normal(h: Hit, d: vec3<f32>) -> vec3<f32> {
    var n = vec3<f32>(0.0);
    if (h.axis < 3u) {
        n[h.axis] = select(1.0, -1.0, d[h.axis] > 0.0);
    }
    return n;
}

// Voxel DDA inside one brick, from ray distance t entering across `axis`.
fn march_brick(brick: u32, bc: vec3<i32>, ro: vec3<f32>, d: vec3<f32>, inv: vec3<f32>,
               stepi: vec3<i32>, step01: vec3<f32>, t_in: f32, axis_in: u32, t_exit: f32,
               steps: ptr<function, u32>) -> Hit {
    var h: Hit;
    h.axis = MISS;
    let base = vec3<f32>(bc * 8);
    var v = clamp(vec3<i32>(floor(ro + d * t_in - base)), vec3<i32>(0), vec3<i32>(7));
    var tm = (base + vec3<f32>(v) + step01 - ro) * inv;
    let td = abs(inv);
    var t = t_in;
    var ax = axis_in;
    loop {
        *steps += 1u;
        let local = u32(v.x) | (u32(v.y) << 3u) | (u32(v.z) << 6u);
        if (mask_bit(brick, local) && !hidden(brick, local)) {
            h.t = t;
            h.axis = ax;
            h.brick = brick;
            h.local = local;
            h.value = palette_value(brick, local);
            return h;
        }
        ax = min_axis(tm);
        t = tm[ax];
        v[ax] += stepi[ax];
        tm[ax] += td[ax];
        if (any(v < vec3<i32>(0)) || any(v > vec3<i32>(7)) || t >= t_exit) { break; }
    }
    return h;
}

struct RaySetup {
    d: vec3<f32>,
    inv: vec3<f32>,
    stepi: vec3<i32>,
    step01: vec3<f32>,
    t: f32,
    t_exit: f32,
    axis: u32,
};

fn setup(ro: vec3<f32>, d_in: vec3<f32>, t_min: f32, t_max: f32) -> RaySetup {
    var s: RaySetup;
    let eps = 1e-7;
    s.d = select(d_in, vec3<f32>(eps), abs(d_in) < vec3<f32>(eps));
    s.inv = 1.0 / s.d;
    let ta = (map_info.lo.xyz - ro) * s.inv;
    let tb = (map_info.hi.xyz - ro) * s.inv;
    let tn = min(ta, tb);
    let tf = max(ta, tb);
    s.t = max(max(max(tn.x, tn.y), tn.z), t_min);
    s.t_exit = min(min(min(tf.x, tf.y), tf.z), t_max);
    if (tn.x >= tn.y && tn.x >= tn.z) { s.axis = 0u; } else if (tn.y >= tn.z) { s.axis = 1u; } else { s.axis = 2u; }
    s.stepi = select(vec3<i32>(-1), vec3<i32>(1), s.d > vec3<f32>(0.0));
    s.step01 = select(vec3<f32>(0.0), vec3<f32>(1.0), s.d > vec3<f32>(0.0));
    return s;
}

// Brick DDA that reads the coarse level instead of the grid for empty bricks, and jumps
// across a whole empty 32^3 coarse cell in one step. Fewer live registers than `trace`.
fn trace_h(ro: vec3<f32>, d_in: vec3<f32>, t_min: f32, t_max: f32) -> Hit {
    var miss: Hit;
    miss.axis = MISS;
    miss.t = t_max;
    let s = setup(ro, d_in, t_min, t_max);
    if (s.t >= s.t_exit) { return miss; }
    let bdims = vec3<i32>(map_info.bdims.xyz);
    var steps = 0u;
    var t = s.t;
    var axis = s.axis;
    var bc = clamp(vec3<i32>(floor((ro + s.d * t) / 8.0)), vec3<i32>(0), bdims - 1);
    var tm = ((vec3<f32>(bc) + s.step01) * 8.0 - ro) * s.inv;
    let td = abs(8.0 * s.inv);
    loop {
        if (steps >= MAX_STEPS || t >= s.t_exit) { break; }
        steps += 1u;
        let sc = bc >> vec3<u32>(2u);
        let si = u32(sc.x) + map_info.sdims.x * (u32(sc.y) + map_info.sdims.y * u32(sc.z));
        let occ0 = coarse[si * 2u];
        let occ1 = coarse[si * 2u + 1u];
        if ((occ0 | occ1) == 0u) {
            // Empty coarse cell: jump to where the ray leaves it. The ray leaves through the
            // face of `axis` first, so on the other axes it is still inside this cell: keep
            // those brick coordinates inside it, and never let t go back. Without both,
            // rounding (worst with Metal's fast math) could put the ray back in a cell it had
            // left and cycle between two empty cells until MAX_STEPS.
            let tc = ((vec3<f32>(sc) + s.step01) * 32.0 - ro) * s.inv;
            axis = min_axis(tc);
            t = max(t, tc[axis]);
            let cell_lo = sc * 4;
            bc = clamp(vec3<i32>(floor((ro + s.d * t) / 8.0)), cell_lo, min(cell_lo + 3, bdims - 1));
            bc[axis] = select(sc[axis] * 4 - 1, sc[axis] * 4 + 4, s.stepi[axis] > 0);
            if (bc[axis] < 0 || bc[axis] >= bdims[axis]) { break; }
            tm = ((vec3<f32>(bc) + s.step01) * 8.0 - ro) * s.inv;
            continue;
        }
        let lb = vec3<u32>(bc & vec3<i32>(3));
        let bit = lb.x + lb.y * 4u + lb.z * 16u;
        let word = select(occ1, occ0, bit < 32u);
        if (((word >> (bit & 31u)) & 1u) != 0u) {
            var h = march_brick(grid_at(bc), bc, ro, s.d, s.inv, s.stepi, s.step01, t, axis, s.t_exit, &steps);
            if (h.axis != MISS) { h.steps = steps; return h; }
        }
        axis = min_axis(tm);
        t = tm[axis];
        bc[axis] += s.stepi[axis];
        tm[axis] += td[axis];
        if (any(bc < vec3<i32>(0)) || any(bc >= bdims)) { break; }
    }
    miss.steps = steps;
    return miss;
}
