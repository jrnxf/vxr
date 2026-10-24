import fs from 'node:fs';
const src = fs.readFileSync(new URL('./core.js', import.meta.url), 'utf8');
const core = new Function(src + '\nreturn {VoxelWorld, meshChunk, readVox, writeVox, generateIsland, generateTerrain, defaultPalette};')();
const w = new core.VoxelWorld();
let t = performance.now(); core.generateIsland(w); console.log('island voxels', w.count, 'gen ms', (performance.now()-t).toFixed(0), 'chunks', w.chunks.size);
t = performance.now(); let quads = 0; for (const c of w.chunks.values()) { const r = core.meshChunk(w, c); if (r) quads += r.quads; }
console.log('mesh ms', (performance.now()-t).toFixed(0), 'quads', quads);
const pal = core.defaultPalette();
t = performance.now(); const bytes = core.writeVox(w, pal); console.log('vox bytes', bytes.length, 'ms', (performance.now()-t).toFixed(0));
const back = core.readVox(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length));
console.log('roundtrip count', back.world.count, 'models', back.models);
let diff = 0; w.forEach((x,y,z,v) => { if (back.world.get(x,y,z) !== v) diff++; }); console.log('mismatches', diff);
const w2 = new core.VoxelWorld(); t = performance.now(); core.generateTerrain(w2, 192); console.log('terrain voxels', w2.count, 'gen ms', (performance.now()-t).toFixed(0), 'chunks', w2.chunks.size);
t = performance.now(); quads = 0; for (const c of w2.chunks.values()) { const r = core.meshChunk(w2, c); if (r) quads += r.quads; } console.log('terrain mesh ms', (performance.now()-t).toFixed(0), 'quads', quads);
const b2 = core.writeVox(w2, pal); const back2 = core.readVox(b2.buffer); console.log('terrain vox bytes', b2.length, 'models', back2.models, 'count back', back2.world.count);
