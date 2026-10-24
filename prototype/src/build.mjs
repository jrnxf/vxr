// Builds the self-contained demo into ../demo: inlines core.js into the editor and embed script,
// and writes the starter scene.vox the embed page loads.
import fs from 'node:fs';
const here = new URL('.', import.meta.url);
const out = new URL('../demo/', import.meta.url);
fs.mkdirSync(out, { recursive: true });
const core = fs.readFileSync(new URL('core.js', here), 'utf8');
const inline = (file) => fs.readFileSync(new URL(file, here), 'utf8').replace('//@@CORE@@', () => core);
fs.writeFileSync(new URL('index.html', out), inline('editor.html'));
fs.writeFileSync(new URL('voxel-scene.js', out), inline('voxel-scene.js'));
fs.copyFileSync(new URL('embed.html', here), new URL('embed.html', out));
const api = new Function(core + '\nreturn { VoxelWorld, generateIsland, writeVox, defaultPalette };')();
const w = new api.VoxelWorld();
api.generateIsland(w);
fs.writeFileSync(new URL('scene.vox', out), api.writeVox(w, api.defaultPalette()));
console.log('built', fs.readdirSync(out).join(' '), '| island voxels', w.count);
