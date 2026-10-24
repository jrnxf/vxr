// ---- voxel core: sparse chunked world, greedy mesher with AO, .vox I/O, scene generators ----
// Shared by the editor and the <voxel-scene> embed. Plain JS, no imports; THREE is passed in.

const CS = 32, CS3 = CS * CS * CS;
const PAD = CS + 2, PAD2 = PAD * PAD, PAD3 = PAD * PAD * PAD;

const DEFAULT_PALETTE_HEX = [
  '0e0f13','272a33','3e4250','5b6070','7e8496','a5abbb','cfd3dd','f4f5f8',
  '4a1621','7a2133','b83b3b','e5604f','ff8a6b','ffb199','f2557a','ff9bb8',
  '6b3a1f','a35a2a','d98a3a','f2b04a','ffd166','ffe7a3','c7a06a','e8cfa0',
  '1f3b2a','2f5d3a','3f7f43','5ea64a','8cc95b','bfe38a','4b6b3a','7d9a4c',
  '12343f','1b5466','22808a','33b0b0','7fd8d0','1d3f73','2f6db5','5aa0e6',
  '9cc9f5','cfe6ff','2b2554','4b3a8c','7559c2','a68cf0','d2c4ff','5a2b6b',
  '3b2a22','5c4033','80594a','a9796a','d4a58c','f0cdb4','8a6a4f','b89470',
  'ffffff','e9e2d0','c2b8a3','8f8775','5f5a4e','ff4d6d','ffcc00','00d1b2',
];

function defaultPalette() {
  // index 0 is "empty"; 1..255 are colors
  const p = new Uint8Array(256 * 4);
  for (let i = 1; i < 256; i++) {
    let r, g, b;
    if (i <= DEFAULT_PALETTE_HEX.length) {
      const n = parseInt(DEFAULT_PALETTE_HEX[i - 1], 16);
      r = n >> 16; g = (n >> 8) & 255; b = n & 255;
    } else {
      const t = (i - 65) / 190; // spare slots: a neutral ramp
      r = g = b = Math.round(20 + t * 225);
    }
    p[i * 4] = r; p[i * 4 + 1] = g; p[i * 4 + 2] = b; p[i * 4 + 3] = 255;
  }
  return p;
}

function chunkKey(cx, cy, cz) { return ((cx + 1024) * 2048 + (cy + 1024)) * 2048 + (cz + 1024); }
function voxelIndex(lx, ly, lz) { return lx | (ly << 5) | (lz << 10); }

class VoxelWorld {
  constructor() { this.chunks = new Map(); this.count = 0; this.dirty = new Set(); }
  getChunk(cx, cy, cz, create) {
    const k = chunkKey(cx, cy, cz);
    let c = this.chunks.get(k);
    if (!c && create) { c = { cx, cy, cz, key: k, data: new Uint8Array(CS3), n: 0 }; this.chunks.set(k, c); }
    return c;
  }
  get(x, y, z) {
    const c = this.chunks.get(chunkKey(x >> 5, y >> 5, z >> 5));
    return c ? c.data[voxelIndex(x & 31, y & 31, z & 31)] : 0;
  }
  // Writes one voxel; returns the previous value. Marks the chunk (and touched neighbors) for remeshing.
  set(x, y, z, v) {
    const cx = x >> 5, cy = y >> 5, cz = z >> 5;
    const c = this.getChunk(cx, cy, cz, v !== 0);
    if (!c) return 0;
    const lx = x & 31, ly = y & 31, lz = z & 31, i = voxelIndex(lx, ly, lz);
    const old = c.data[i];
    if (old === v) return old;
    c.data[i] = v;
    if (!old) { c.n++; this.count++; } else if (!v) { c.n--; this.count--; }
    this.dirty.add(c.key);
    if (lx === 0) this.markDirty(cx - 1, cy, cz); else if (lx === 31) this.markDirty(cx + 1, cy, cz);
    if (ly === 0) this.markDirty(cx, cy - 1, cz); else if (ly === 31) this.markDirty(cx, cy + 1, cz);
    if (lz === 0) this.markDirty(cx, cy, cz - 1); else if (lz === 31) this.markDirty(cx, cy, cz + 1);
    // AO samples diagonal neighbors too, so edges/corners touch more chunks
    if ((lx === 0 || lx === 31) && (ly === 0 || ly === 31)) this.markDirty(cx + (lx ? 1 : -1), cy + (ly ? 1 : -1), cz);
    if ((lx === 0 || lx === 31) && (lz === 0 || lz === 31)) this.markDirty(cx + (lx ? 1 : -1), cy, cz + (lz ? 1 : -1));
    if ((ly === 0 || ly === 31) && (lz === 0 || lz === 31)) this.markDirty(cx, cy + (ly ? 1 : -1), cz + (lz ? 1 : -1));
    return old;
  }
  // Fast bulk write for generators and imports (no neighbor bookkeeping; call markAllDirty after).
  setRaw(x, y, z, v) {
    const c = this.getChunk(x >> 5, y >> 5, z >> 5, true);
    const i = voxelIndex(x & 31, y & 31, z & 31);
    const old = c.data[i];
    c.data[i] = v;
    if (!old && v) { c.n++; this.count++; } else if (old && !v) { c.n--; this.count--; }
  }
  markDirty(cx, cy, cz) { const k = chunkKey(cx, cy, cz); if (this.chunks.has(k)) this.dirty.add(k); }
  markAllDirty() { for (const k of this.chunks.keys()) this.dirty.add(k); }
  clear() { this.dirtyRemoved = [...this.chunks.keys()]; this.chunks.clear(); this.count = 0; this.dirty.clear(); }
  forEach(cb) {
    for (const c of this.chunks.values()) {
      if (!c.n) continue;
      const d = c.data, ox = c.cx * CS, oy = c.cy * CS, oz = c.cz * CS;
      for (let i = 0; i < CS3; i++) {
        const v = d[i];
        if (v) cb(ox + (i & 31), oy + ((i >> 5) & 31), oz + (i >> 10), v);
      }
    }
  }
  bounds() {
    let minX = Infinity, minY = Infinity, minZ = Infinity, maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
    for (const c of this.chunks.values()) {
      if (!c.n) continue;
      const x0 = c.cx * CS, y0 = c.cy * CS, z0 = c.cz * CS;
      if (x0 < minX) minX = x0; if (y0 < minY) minY = y0; if (z0 < minZ) minZ = z0;
      if (x0 + CS > maxX) maxX = x0 + CS; if (y0 + CS > maxY) maxY = y0 + CS; if (z0 + CS > maxZ) maxZ = z0 + CS;
    }
    if (minX === Infinity) return null;
    return { minX, minY, minZ, maxX, maxY, maxZ };
  }
}

// ---- greedy mesher with per-vertex ambient occlusion ----
const _pad = new Uint8Array(PAD3);
const _mask = new Int32Array(CS * CS);

function gatherPadded(world, c) {
  const pad = _pad, d = c.data;
  for (let z = 0; z < CS; z++) for (let y = 0; y < CS; y++) {
    const src = (y << 5) | (z << 10), dst = 1 + (y + 1) * PAD + (z + 1) * PAD2;
    for (let x = 0; x < CS; x++) pad[dst + x] = d[src + x];
  }
  const ox = c.cx * CS, oy = c.cy * CS, oz = c.cz * CS;
  for (let z = -1; z <= CS; z++) for (let y = -1; y <= CS; y++) {
    const inner = z >= 0 && z < CS && y >= 0 && y < CS;
    const row = (y + 1) * PAD + (z + 1) * PAD2;
    if (inner) {
      pad[row] = world.get(ox - 1, oy + y, oz + z);
      pad[row + PAD - 1] = world.get(ox + CS, oy + y, oz + z);
    } else {
      for (let x = -1; x <= CS; x++) pad[row + x + 1] = world.get(ox + x, oy + y, oz + z);
    }
  }
}

function aoValue(s1, s2, corner) { return s1 && s2 ? 0 : 3 - (s1 + s2 + corner); }

// Returns typed arrays for one chunk, or null if it has no visible faces.
function meshChunk(world, c) {
  if (!c.n) return null;
  gatherPadded(world, c);
  const pad = _pad, mask = _mask;
  const pos = [], nrm = [], pal = [], aov = [], idx = [];
  let vcount = 0;
  const st = [1, PAD, PAD2];
  const p = [0, 0, 0];
  for (let d = 0; d < 3; d++) {
    const u = (d + 1) % 3, v = (d + 2) % 3;
    const uOff = st[u], vOff = st[v];
    for (let sgn = 1; sgn >= -1; sgn -= 2) {
      const nOff = sgn * st[d];
      for (let s = 0; s < CS; s++) {
        // build the face mask for this slice
        let k = 0;
        p[d] = s;
        for (let j = 0; j < CS; j++) {
          p[v] = j;
          for (let i = 0; i < CS; i++, k++) {
            p[u] = i;
            const b = (p[0] + 1) + (p[1] + 1) * PAD + (p[2] + 1) * PAD2;
            const a = pad[b];
            if (!a) { mask[k] = 0; continue; }
            const q = b + nOff;
            if (pad[q]) { mask[k] = 0; continue; }
            const sum = pad[q - uOff] ? 1 : 0, sup = pad[q + uOff] ? 1 : 0;
            const svm = pad[q - vOff] ? 1 : 0, svp = pad[q + vOff] ? 1 : 0;
            const a00 = aoValue(sum, svm, pad[q - uOff - vOff] ? 1 : 0);
            const a10 = aoValue(sup, svm, pad[q + uOff - vOff] ? 1 : 0);
            const a11 = aoValue(sup, svp, pad[q + uOff + vOff] ? 1 : 0);
            const a01 = aoValue(sum, svp, pad[q - uOff + vOff] ? 1 : 0);
            mask[k] = a | (a00 << 8) | (a10 << 10) | (a11 << 12) | (a01 << 14);
          }
        }
        // greedy merge equal faces
        const plane = s + (sgn > 0 ? 1 : 0);
        k = 0;
        for (let j = 0; j < CS; j++) {
          for (let i = 0; i < CS; ) {
            const m = mask[k];
            if (!m) { i++; k++; continue; }
            let w = 1;
            while (i + w < CS && mask[k + w] === m) w++;
            let h = 1;
            outer: for (; j + h < CS; h++) {
              const row = k + h * CS;
              for (let t = 0; t < w; t++) if (mask[row + t] !== m) break outer;
            }
            for (let hh = 0; hh < h; hh++) mask.fill(0, k + hh * CS, k + hh * CS + w);
            // emit quad
            const color = m & 255;
            const A00 = (m >> 8) & 3, A10 = (m >> 10) & 3, A11 = (m >> 12) & 3, A01 = (m >> 14) & 3;
            const corners = sgn > 0
              ? [[0, 0, A00], [1, 0, A10], [1, 1, A11], [0, 1, A01]]
              : [[0, 0, A00], [0, 1, A01], [1, 1, A11], [1, 0, A10]];
            for (const [cu, cv, ao] of corners) {
              const vp = [0, 0, 0];
              vp[d] = plane; vp[u] = i + cu * w; vp[v] = j + cv * h;
              pos.push(vp[0], vp[1], vp[2]);
              const n = [0, 0, 0]; n[d] = sgn * 127;
              nrm.push(n[0], n[1], n[2]);
              pal.push(color); aov.push(ao);
            }
            const c0 = corners[0][2], c1 = corners[1][2], c2 = corners[2][2], c3 = corners[3][2];
            // split along the diagonal with the brighter pair to avoid AO streaks
            if (c0 + c2 >= c1 + c3) idx.push(vcount, vcount + 1, vcount + 2, vcount, vcount + 2, vcount + 3);
            else idx.push(vcount + 1, vcount + 2, vcount + 3, vcount + 1, vcount + 3, vcount);
            vcount += 4;
            i += w; k += w;
          }
        }
      }
    }
  }
  if (!vcount) return null;
  return {
    positions: new Uint8Array(pos), normals: new Int8Array(nrm), pal: new Uint8Array(pal), ao: new Uint8Array(aov),
    indices: vcount > 65535 ? new Uint32Array(idx) : new Uint16Array(idx), quads: vcount / 4,
  };
}

function buildChunkGeometry(THREE, r) {
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.BufferAttribute(r.positions, 3));
  g.setAttribute('normal', new THREE.BufferAttribute(r.normals, 3, true));
  g.setAttribute('pal', new THREE.BufferAttribute(r.pal, 1));
  g.setAttribute('aoc', new THREE.BufferAttribute(r.ao, 1));
  g.setIndex(new THREE.BufferAttribute(r.indices, 1));
  g.boundingBox = new THREE.Box3(new THREE.Vector3(0, 0, 0), new THREE.Vector3(CS, CS, CS));
  g.boundingSphere = new THREE.Sphere(new THREE.Vector3(CS / 2, CS / 2, CS / 2), CS * 0.8661);
  return g;
}

function createPaletteTexture(THREE, palette) {
  const t = new THREE.DataTexture(palette, 256, 1, THREE.RGBAFormat);
  t.colorSpace = THREE.SRGBColorSpace;
  t.magFilter = t.minFilter = THREE.NearestFilter;
  t.needsUpdate = true;
  return t;
}

// Standard PBR material whose color comes from a palette lookup, so recoloring a palette slot is instant.
function createVoxelMaterial(THREE, palTex) {
  const m = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.92, metalness: 0 });
  m.onBeforeCompile = (sh) => {
    sh.uniforms.palTex = { value: palTex };
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float pal;\nattribute float aoc;\nuniform sampler2D palTex;')
      .replace('#include <color_vertex>',
        'vColor = texture2D(palTex, vec2((pal + 0.5) / 256.0, 0.5)).rgb * mix(0.38, 1.0, pow(aoc / 3.0, 0.9));');
  };
  m.customProgramCacheKey = () => 'voxel-pal-v1';
  return m;
}

// ---- DDA voxel raycast (Amanatides & Woo) ----
function raycastVoxels(world, ox, oy, oz, dx, dy, dz, maxDist) {
  let x = Math.floor(ox), y = Math.floor(oy), z = Math.floor(oz);
  const sx = Math.sign(dx), sy = Math.sign(dy), sz = Math.sign(dz);
  const tdx = dx ? Math.abs(1 / dx) : Infinity, tdy = dy ? Math.abs(1 / dy) : Infinity, tdz = dz ? Math.abs(1 / dz) : Infinity;
  let tmx = dx > 0 ? (x + 1 - ox) * tdx : dx < 0 ? (ox - x) * tdx : Infinity;
  let tmy = dy > 0 ? (y + 1 - oy) * tdy : dy < 0 ? (oy - y) * tdy : Infinity;
  let tmz = dz > 0 ? (z + 1 - oz) * tdz : dz < 0 ? (oz - z) * tdz : Infinity;
  let nx = 0, ny = 0, nz = 0, t = 0;
  for (let steps = 0; steps < 6000 && t <= maxDist; steps++) {
    const v = world.get(x, y, z);
    if (v) return { x, y, z, nx, ny, nz, t, v };
    if (tmx < tmy && tmx < tmz) { x += sx; t = tmx; tmx += tdx; nx = -sx; ny = 0; nz = 0; }
    else if (tmy < tmz) { y += sy; t = tmy; tmy += tdy; nx = 0; ny = -sy; nz = 0; }
    else { z += sz; t = tmz; tmz += tdz; nx = 0; ny = 0; nz = -sz; }
  }
  return null;
}

// ---- MagicaVoxel .vox read/write (multi-model scene graph, palette) ----
// Our space is y-up; .vox is z-up. Mapping: vox(x, y, z) = (x, -z-1, y).
function readVox(buffer) {
  const dv = new DataView(buffer);
  const id = (o) => String.fromCharCode(dv.getUint8(o), dv.getUint8(o + 1), dv.getUint8(o + 2), dv.getUint8(o + 3));
  if (id(0) !== 'VOX ') throw new Error('Not a .vox file');
  let palette = null, pendingSize = null;
  const models = [], nodes = new Map();
  let o = 8;
  const readStr = () => { const n = dv.getInt32(o, true); o += 4; let s = ''; for (let i = 0; i < n; i++) s += String.fromCharCode(dv.getUint8(o + i)); o += n; return s; };
  const readDict = () => { const n = dv.getInt32(o, true); o += 4; const d = {}; for (let i = 0; i < n; i++) { const k = readStr(); d[k] = readStr(); } return d; };
  // MAIN header
  if (id(o) !== 'MAIN') throw new Error('Missing MAIN chunk');
  const mainContent = dv.getInt32(o + 4, true);
  o += 12 + mainContent;
  while (o + 12 <= buffer.byteLength) {
    const cid = id(o), csize = dv.getInt32(o + 4, true), childSize = dv.getInt32(o + 8, true);
    const start = o + 12;
    o = start;
    if (cid === 'SIZE') pendingSize = [dv.getInt32(o, true), dv.getInt32(o + 4, true), dv.getInt32(o + 8, true)];
    else if (cid === 'XYZI') {
      const n = dv.getInt32(o, true);
      models.push({ size: pendingSize, data: new Uint8Array(buffer, o + 4, n * 4) });
    } else if (cid === 'RGBA') {
      palette = new Uint8Array(256 * 4);
      for (let i = 1; i < 256; i++) for (let c = 0; c < 4; c++) palette[i * 4 + c] = dv.getUint8(o + (i - 1) * 4 + c);
    } else if (cid === 'nTRN') {
      const nid = dv.getInt32(o, true); o += 4; readDict();
      const child = dv.getInt32(o, true); o += 12; // child, reserved, layer
      const frames = dv.getInt32(o, true); o += 4;
      let t = [0, 0, 0];
      for (let f = 0; f < frames; f++) { const fd = readDict(); if (f === 0 && fd._t) t = fd._t.split(' ').map(Number); }
      nodes.set(nid, { type: 'T', child, t });
    } else if (cid === 'nGRP') {
      const nid = dv.getInt32(o, true); o += 4; readDict();
      const n = dv.getInt32(o, true); o += 4;
      const children = []; for (let i = 0; i < n; i++) { children.push(dv.getInt32(o, true)); o += 4; }
      nodes.set(nid, { type: 'G', children });
    } else if (cid === 'nSHP') {
      const nid = dv.getInt32(o, true); o += 4; readDict();
      const n = dv.getInt32(o, true); o += 4;
      const ids = []; for (let i = 0; i < n; i++) { ids.push(dv.getInt32(o, true)); o += 4; readDict(); }
      nodes.set(nid, { type: 'S', models: ids });
    }
    o = start + csize + childSize;
  }
  const world = new VoxelWorld();
  const place = (m, tx, ty, tz) => {
    const [sx, sy, sz] = m.size, d = m.data;
    const bx = tx - Math.floor(sx / 2), by = ty - Math.floor(sy / 2), bz = tz - Math.floor(sz / 2);
    for (let i = 0; i < d.length; i += 4) {
      const vx = bx + d[i], vy = by + d[i + 1], vz = bz + d[i + 2];
      world.setRaw(vx, vz, -vy - 1, d[i + 3]);
    }
  };
  if (nodes.has(0)) {
    const walk = (nid, ox, oy, oz, depth) => {
      const n = nodes.get(nid);
      if (!n || depth > 64) return;
      if (n.type === 'T') walk(n.child, ox + n.t[0], oy + n.t[1], oz + n.t[2], depth + 1);
      else if (n.type === 'G') for (const c of n.children) walk(c, ox, oy, oz, depth + 1);
      else for (const mi of n.models) if (models[mi]) place(models[mi], ox, oy, oz);
    };
    walk(0, 0, 0, 0, 0);
  } else {
    for (const m of models) place(m, 0, 0, Math.floor(m.size[2] / 2));
  }
  return { world, palette: palette || defaultPalette(), models: models.length };
}

function writeVox(world, palette) {
  // gather voxels into 256^3 blocks in vox space
  const b = world.bounds();
  const blocks = new Map();
  if (b) {
    const vminX = b.minX, vminY = -b.maxZ, vminZ = b.minY;
    world.forEach((x, y, z, v) => {
      const vx = x - vminX, vy = (-z - 1) - vminY, vz = y - vminZ;
      const key = ((vx >> 8) * 4096 + (vy >> 8)) * 4096 + (vz >> 8);
      let blk = blocks.get(key);
      if (!blk) { blk = { bx: vx >> 8, by: vy >> 8, bz: vz >> 8, xyzi: [], max: [0, 0, 0] }; blocks.set(key, blk); }
      const lx = vx & 255, ly = vy & 255, lz = vz & 255;
      blk.xyzi.push(lx, ly, lz, v);
      if (lx > blk.max[0]) blk.max[0] = lx; if (ly > blk.max[1]) blk.max[1] = ly; if (lz > blk.max[2]) blk.max[2] = lz;
    });
    for (const blk of blocks.values()) {
      blk.size = [blk.max[0] + 1, blk.max[1] + 1, blk.max[2] + 1];
      blk.min = [vminX + blk.bx * 256, vminY + blk.by * 256, vminZ + blk.bz * 256];
    }
  }
  const parts = [];
  const enc = (s) => { const a = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i); return a; };
  const i32 = (...vals) => { const a = new Uint8Array(vals.length * 4); const d = new DataView(a.buffer); vals.forEach((v, i) => d.setInt32(i * 4, v, true)); return a; };
  const str = (s) => { const e = enc(s); return concat([i32(e.length), e]); };
  const dict = (o) => { const ks = Object.keys(o); return concat([i32(ks.length), ...ks.flatMap((k) => [str(k), str(String(o[k]))])]); };
  function concat(arrs) { let n = 0; for (const a of arrs) n += a.length; const out = new Uint8Array(n); let o = 0; for (const a of arrs) { out.set(a, o); o += a.length; } return out; }
  const chunk = (id, content) => concat([enc(id), i32(content.length, 0), content]);
  const list = [...blocks.values()];
  list.forEach((blk) => {
    parts.push(chunk('SIZE', i32(...blk.size)));
    const n = blk.xyzi.length / 4;
    parts.push(chunk('XYZI', concat([i32(n), new Uint8Array(blk.xyzi)])));
  });
  parts.push(chunk('nTRN', concat([i32(0), dict({}), i32(1, -1, -1, 1), dict({})])));
  parts.push(chunk('nGRP', concat([i32(1), dict({}), i32(list.length, ...list.map((_, k) => 2 + 2 * k))])));
  list.forEach((blk, k) => {
    const t = blk.min.map((m, a) => m + Math.floor(blk.size[a] / 2)).join(' ');
    parts.push(chunk('nTRN', concat([i32(2 + 2 * k), dict({}), i32(3 + 2 * k, -1, 0, 1), dict({ _t: t })])));
    parts.push(chunk('nSHP', concat([i32(3 + 2 * k), dict({}), i32(1, k), dict({})])));
  });
  parts.push(chunk('LAYR', concat([i32(0), dict({}), i32(-1)])));
  const rgba = new Uint8Array(1024);
  for (let i = 1; i < 256; i++) for (let c = 0; c < 4; c++) rgba[(i - 1) * 4 + c] = palette[i * 4 + c];
  parts.push(chunk('RGBA', rgba));
  const children = concat(parts);
  return concat([enc('VOX '), i32(200), enc('MAIN'), i32(0, children.length), children]);
}

// ---- deterministic noise + scene generators ----
function hash2(x, z, seed) {
  let h = (x * 374761393 + z * 668265263 + seed * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
function valueNoise(x, z, seed) {
  const xi = Math.floor(x), zi = Math.floor(z), xf = x - xi, zf = z - zi;
  const u = xf * xf * (3 - 2 * xf), w = zf * zf * (3 - 2 * zf);
  const a = hash2(xi, zi, seed), b = hash2(xi + 1, zi, seed), c = hash2(xi, zi + 1, seed), d = hash2(xi + 1, zi + 1, seed);
  return a + (b - a) * u + (c - a) * w + (a - b - c + d) * u * w;
}
function fbm(x, z, seed, oct) {
  let s = 0, amp = 0.5, f = 1, norm = 0;
  for (let i = 0; i < oct; i++) { s += amp * valueNoise(x * f, z * f, seed + i * 17); norm += amp; amp *= 0.5; f *= 2; }
  return s / norm;
}

const C = {
  stoneD: 3, stone: 4, stoneL: 5, rockL: 6, soilD: 49, soil: 50, dirt: 51, sand: 24, sandD: 23,
  grassD: 27, grass: 28, grassL: 29, pine: 26, pineD: 25, trunk: 50, trunkL: 51, waterD: 39, water: 40, shore: 37,
  wall: 58, trim: 59, roof: 11, roofL: 12, door: 18, window: 41, plank: 55, plankL: 56, path: 59,
  flowerR: 15, flowerY: 21, flowerW: 57, snow: 8, lamp: 21,
};

function generateIsland(world) {
  const R = 64, SEA = 6, BASE = 4;
  const hx = -6, hz = 6; // house anchor
  const heights = new Int32Array(128 * 128);
  for (let z = -R; z < R; z++) for (let x = -R; x < R; x++) {
    const dx = (x + 4) / 50, dz = (z - 2) / 44;
    const d = Math.sqrt(dx * dx + dz * dz) + (fbm(x / 22, z / 22, 7, 3) - 0.5) * 0.55;
    const f = Math.max(0, 1 - d);
    let h = BASE + Math.floor(Math.pow(f, 0.9) * (6 + fbm(x / 18, z / 18, 3, 4) * 18));
    if (f <= 0) h = BASE - 1 + Math.floor(fbm(x / 9, z / 9, 5, 2) * 2);
    // flatten a pad for the house
    const hd = Math.max(Math.abs(x - hx), Math.abs(z - hz));
    if (hd <= 7) h = Math.round(h * (hd / 7) + 12 * (1 - hd / 7));
    heights[(x + R) + (z + R) * 128] = h;
  }
  for (let z = -R; z < R; z++) for (let x = -R; x < R; x++) {
    const h = heights[(x + R) + (z + R) * 128];
    const top = Math.max(h, SEA);
    for (let y = 0; y <= top; y++) {
      let c;
      if (y <= h) {
        if (y < 2) c = C.soilD;
        else if (y < BASE) c = (y + ((x ^ z) & 1)) % 2 ? C.soil : C.soilD;
        else if (h <= SEA + 1) c = y === h ? C.sand : C.sandD;
        else if (y === h) c = hash2(x, z, 11) < 0.18 ? C.grassL : hash2(x, z, 12) < 0.12 ? C.grassD : C.grass;
        else if (y >= h - 2) c = C.dirt;
        else c = hash2(x, y * 31 + z, 2) < 0.5 ? C.stone : C.stoneD;
      } else {
        const depth = SEA - h;
        c = y === SEA ? (depth <= 1 ? C.shore : depth <= 3 ? C.water : C.waterD) : C.waterD;
      }
      world.setRaw(x, y, z, c);
    }
  }
  const H = (x, z) => (x >= -R && x < R && z >= -R && z < R ? heights[(x + R) + (z + R) * 128] : 0);
  // house
  const W = 5, D = 4, wallH = 5, floorY = 13;
  for (let x = hx - W; x <= hx + W; x++) for (let z = hz - D; z <= hz + D; z++) {
    world.setRaw(x, floorY - 1, z, C.plank);
    const edge = x === hx - W || x === hx + W || z === hz - D || z === hz + D;
    if (!edge) continue;
    for (let y = floorY; y < floorY + wallH; y++) {
      const corner = (x === hx - W || x === hx + W) && (z === hz - D || z === hz + D);
      let c = corner ? C.trim : C.wall;
      const isDoor = z === hz + D && (x === hx || x === hx - 1) && y < floorY + 3;
      const isWin = y === floorY + 2 && !corner && ((z === hz + D && (x === hx + 3 || x === hx - 3)) || (x === hx + W && Math.abs(z - hz) === 1) || (x === hx - W && Math.abs(z - hz) === 1) || (z === hz - D && Math.abs(x - hx) === 2));
      if (isDoor) c = y === floorY + 2 ? C.trim : 0;
      if (isWin) c = C.window;
      if (c) world.setRaw(x, y, z, c);
    }
  }
  // gable roof along x
  for (let k = 0; k <= D + 2; k++) {
    const y = floorY + wallH + k, zl = hz - D - 1 + k, zr = hz + D + 1 - k;
    if (zl > zr) break;
    for (let x = hx - W - 1; x <= hx + W + 1; x++) {
      world.setRaw(x, y, zl, (x + k) % 2 ? C.roof : C.roofL);
      world.setRaw(x, y, zr, (x + k) % 2 ? C.roof : C.roofL);
      if (zl === zr || zl + 1 === zr) continue;
      if (x === hx - W || x === hx + W) for (let z = zl + 1; z < zr; z++) world.setRaw(x, y, z, C.wall);
    }
  }
  // chimney
  for (let y = floorY + wallH; y < floorY + wallH + 7; y++) for (const [cx, cz] of [[hx + 3, hz - 1], [hx + 4, hz - 1]]) world.setRaw(cx, y, cz, C.stoneL);
  // path from door to the shore
  for (let z = hz + D + 1; z < hz + D + 30; z++) {
    const x = hx + Math.round(Math.sin(z / 5) * 2);
    const h = H(x, z);
    if (h <= SEA) break;
    for (const ox of [-1, 0]) world.setRaw(x + ox, H(x + ox, z), z, hash2(x + ox, z, 5) < 0.3 ? C.stoneL : C.path);
  }
  // dock out over the water on the east side
  let dockX = 0, dockZ = -2;
  for (dockX = 0; dockX < R && H(dockX, dockZ) > SEA; dockX++);
  for (let x = dockX - 3; x < dockX + 12; x++) for (let z = dockZ - 1; z <= dockZ + 1; z++) {
    world.setRaw(x, SEA + 2, z, (x & 1) ? C.plank : C.plankL);
    if ((x - dockX) % 4 === 0 && z !== dockZ) for (let y = BASE; y <= SEA + 1; y++) world.setRaw(x, y, z, C.trunk);
  }
  // trees, rocks, flowers
  const tree = (tx, tz, kind) => {
    const g = H(tx, tz) + 1;
    const th = 4 + Math.floor(hash2(tx, tz, 21) * 3);
    for (let y = g; y < g + th; y++) world.setRaw(tx, y, tz, C.trunk);
    if (kind === 0) {
      const r = 2.6 + hash2(tx, tz, 22) * 1.4, cy = g + th + 1;
      for (let x = -4; x <= 4; x++) for (let y = -4; y <= 4; y++) for (let z = -4; z <= 4; z++) {
        const dd = (x * x + y * y * 1.3 + z * z) / (r * r);
        if (dd <= 1 && hash2(tx + x, (tz + z) * 7 + y, 23) > dd * 0.35) {
          const shade = y > 0 ? C.grassL : y < -1 ? C.grassD : C.grass;
          world.setRaw(tx + x, cy + y, tz + z, hash2(tx + x, tz + z + y * 13, 24) < 0.15 ? C.grassD : shade);
        }
      }
    } else {
      const top = g + th + 7;
      for (let y = g + 2; y < top; y++) {
        const r = Math.max(0, Math.floor((top - y) / 2.2));
        for (let x = -r; x <= r; x++) for (let z = -r; z <= r; z++) if (x * x + z * z <= r * r + 1) world.setRaw(tx + x, y, tz + z, (y & 1) ? C.pine : C.pineD);
      }
      world.setRaw(tx, top, tz, C.pine);
    }
  };
  let placed = 0;
  for (let i = 0; i < 900 && placed < 30; i++) {
    const tx = Math.floor(hash2(i, 1, 31) * 110) - 55, tz = Math.floor(hash2(i, 2, 31) * 110) - 55;
    const h = H(tx, tz);
    if (h < SEA + 3) continue;
    if (Math.abs(tx - hx) < 11 && Math.abs(tz - hz) < 12) continue;
    if (Math.abs(tz - dockZ) < 4 && tx > dockX - 8) continue;
    tree(tx, tz, h > 16 || hash2(i, 3, 31) < 0.35 ? 1 : 0);
    placed++;
  }
  for (let i = 0; i < 400; i++) {
    const x = Math.floor(hash2(i, 4, 41) * 120) - 60, z = Math.floor(hash2(i, 5, 41) * 120) - 60;
    const h = H(x, z);
    if (h < SEA + 2 || world.get(x, h + 1, z)) continue;
    if (i % 5 === 0) { // rock
      for (let a = -1; a <= 1; a++) for (let b = -1; b <= 1; b++) if (hash2(x + a, z + b, 42) < 0.7) world.setRaw(x + a, H(x + a, z + b) + 1, z + b, hash2(x + a, z + b, 43) < 0.5 ? C.stoneL : C.rockL);
    } else {
      world.setRaw(x, h + 1, z, [C.flowerR, C.flowerY, C.flowerW][i % 3]);
    }
  }
  world.markAllDirty();
}

function generateTerrain(world, half) {
  const SEA = 10;
  for (let z = -half; z < half; z++) for (let x = -half; x < half; x++) {
    const n = Math.min(1, Math.max(0, (fbm(x / 110, z / 110, 101, 5) - 0.28) / 0.44));
    const ridge = 1 - Math.abs(fbm(x / 70, z / 70, 202, 4) * 2 - 1);
    const mount = Math.pow(Math.max(0, n - 0.45) / 0.55, 1.5);
    const h = Math.floor(3 + n * 16 + mount * (30 + ridge * ridge * 40));
    const top = Math.max(h, SEA);
    for (let y = 0; y <= top; y++) {
      let c;
      if (y > h) c = y === SEA ? (SEA - h <= 1 ? C.shore : SEA - h <= 3 ? C.water : C.waterD) : C.waterD;
      else if (y === h) c = h <= SEA + 1 ? C.sand : h > 52 ? C.snow : h > 34 ? (hash2(x, z, 8) < 0.3 ? C.stoneL : C.rockL) : h > 26 ? C.grassD : hash2(x, z, 9) < 0.12 ? C.grassL : C.grass;
      else if (y > h - 3 && h <= 34) c = h <= SEA + 1 ? C.sandD : C.dirt;
      else c = hash2(x, y * 31 + z, 2) < 0.5 ? C.stone : C.stoneD;
      world.setRaw(x, y, z, c);
    }
  }
  world.markAllDirty();
}
