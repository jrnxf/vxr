// The browser check, run inside the page by crates/vxr-web/check (chrome-devtools-axi eval).
// It drives the editor through the page's own DOM listeners (synthetic pointer, wheel and key
// events on the canvas), the file input and the download path, and returns one line per step:
// "ok <step>" or "FAIL <step>: <why>". The page must be dist/index.html at 1280x800.
async () => {
  const out = [];
  const c = document.getElementById('c');
  const frames = (n = 3) => new Promise((r) => {
    const f = () => (--n > 0 ? requestAnimationFrame(f) : r());
    requestAnimationFrame(f);
  });
  for (let i = 0; i < 300 && !window.vxr; i++) await frames(1);
  const st = () => JSON.parse(window.vxr.state());
  const step = (name, pass, why = '') => out.push(pass ? `ok ${name}` : `FAIL ${name}: ${why}`);
  if (!window.vxr) return 'FAIL start: the editor did not start';

  let buttons = 0;
  const bit = { 0: 1, 1: 4, 2: 2 };
  const ptr = (type, x, y, button = 0, mods = {}) => {
    if (type === 'pointerdown') buttons |= bit[button];
    if (type === 'pointerup') buttons &= ~bit[button];
    c.dispatchEvent(new PointerEvent(type, {
      clientX: x, clientY: y, button: type === 'pointermove' ? -1 : button, buttons,
      pointerId: 1, pointerType: 'mouse', isPrimary: true, bubbles: true, cancelable: true, ...mods,
    }));
  };
  const click = async (x, y, mods = {}) => {
    ptr('pointermove', x, y, 0, mods); await frames();
    ptr('pointerdown', x, y, 0, mods); await frames();
    ptr('pointerup', x, y, 0, mods); await frames();
  };
  const drag = async (x0, y0, x1, y1, button = 0, mods = {}) => {
    ptr('pointermove', x0, y0, button, mods); await frames();
    ptr('pointerdown', x0, y0, button, mods); await frames();
    for (let i = 1; i <= 8; i++) {
      ptr('pointermove', x0 + ((x1 - x0) * i) / 8, y0 + ((y1 - y0) * i) / 8, button, mods);
      await frames(1);
    }
    ptr('pointerup', x1, y1, button, mods); await frames();
  };
  const key = async (k, mods = {}) => {
    const o = { key: k, bubbles: true, cancelable: true, ...mods };
    const down = new KeyboardEvent('keydown', o);
    c.dispatchEvent(down);
    c.dispatchEvent(new KeyboardEvent('keyup', o));
    await frames();
    return down.defaultPrevented;
  };
  const cmd = { metaKey: true };
  const near = (a, b) => Math.abs(a - b) < 1e-3;
  const camMoved = (a, b) => a.cam.some((v, i) => !near(v, b.cam[i]));
  const bytes = () => window.vxr.save();
  const same = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
  // The island's middle at 1280x800: a surface voxel, away from the panels.
  const [mx, my] = [640, 430];

  await frames(10);
  let s0 = st();
  step('starts with the island and the coach card', s0.voxels > 100000 && s0.coach && s0.name === 'island.vox', JSON.stringify(s0));
  const original = bytes();
  const file = new Uint8Array(await (await fetch('scene.vox')).arrayBuffer());
  step('saving the opened island unedited gives back its bytes', same(original, file), `${original.length} vs ${file.length}`);
  // The chunks a lossless save must keep: everything but models and scene graph nodes.
  const others = (b) => {
    const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
    const out = [];
    for (let at = 20; at + 12 <= b.length;) {
      const id = String.fromCharCode(...b.slice(at, at + 4));
      const end = at + 12 + v.getInt32(at + 4, true) + v.getInt32(at + 8, true);
      if (!['SIZE', 'XYZI', 'TDCZ', 'nTRN', 'nGRP', 'nSHP'].includes(id)) out.push(b.slice(at, end).join(','));
      at = end;
    }
    return out;
  };

  // Add, undo, redo.
  await click(mx, my);
  let s = st();
  step('click adds a voxel', s.voxels === s0.voxels + 1, `${s0.voxels} -> ${s.voxels}`);
  step('the first edit closes the coach card', !s.coach);
  step('Cmd-Z is kept from the browser', await key('z', cmd));
  step('Cmd-Z undoes', st().voxels === s0.voxels && st().redo);
  await key('z', { metaKey: true, shiftKey: true });
  step('Cmd-Shift-Z redoes', st().voxels === s0.voxels + 1);
  await key('z', cmd);

  // Erase, by key and by the rail button.
  await key('e');
  step('E picks Erase', st().tool === 'Erase', st().tool);
  await click(mx, my);
  step('erase removes a voxel', st().voxels === s0.voxels - 1, `${st().voxels}`);
  await key('z', cmd);
  await key('a');
  await click(38, 154);
  s = st();
  step('the rail button picks Erase without editing', s.tool === 'Erase' && s.voxels === s0.voxels, JSON.stringify(s));
  await key('a');

  // Paint with another colour, then pick the colour back elsewhere.
  for (let i = 0; i < 9; i++) await key(']');
  const slot = st().slot;
  await key('p');
  await click(mx, my);
  s = st();
  step('paint recolours without adding', s.tool === 'Paint' && s.voxels === s0.voxels && !same(bytes(), original));
  await key('i');
  await click(mx, my);
  step('pick takes the painted colour', st().slot === slot, `${st().slot} vs ${slot}`);
  await key('[');
  await click(mx + 60, my + 20);
  step('pick takes another voxel colour', st().slot !== slot - 1, `${st().slot}`);
  await key('z', cmd);
  step('undo restores the painted voxel', same(bytes(), original));

  // Shapes and mirror.
  await key('a');
  await key('b');
  await drag(mx - 40, my, mx + 40, my + 20);
  s = st();
  step('B box drag adds a box', s.shape === 'Box' && s.voxels > s0.voxels + 10, `${s0.voxels} -> ${s.voxels}`);
  await key('z', cmd);
  await key('f');
  await click(mx, my);
  s = st();
  step('F face fill adds a layer', s.shape === 'Face' && s.voxels > s0.voxels + 1, `${s0.voxels} -> ${s.voxels}`);
  await key('z', cmd);
  await key('v');
  await key('m');
  // The mirrored cell can be inside the terrain, so try a few spots for one that adds two.
  let pair = false;
  for (const dx of [30, -50, 80, -110, 0]) {
    const n = st().voxels;
    await click(mx + dx, my + 10);
    pair ||= st().voxels === n + 2;
    await key('z', cmd);
  }
  step('M mirror adds the mirrored voxel too', st().mirror && pair);
  await key('m');
  step('undo after every tool returns to the island', same(bytes(), original));

  // Camera.
  let a = st();
  await drag(mx, my, mx + 80, my + 30, 2);
  s = st();
  step('right-drag orbits', camMoved(a, s) && s.voxels === s0.voxels && near(s.cam[0], a.cam[0]));
  a = s;
  await drag(mx, my, mx + 60, my, 2, { shiftKey: true });
  s = st();
  step('Shift+right-drag pans', !near(s.cam[0], a.cam[0]) || !near(s.cam[2], a.cam[2]));
  a = s;
  await drag(mx, my, mx, my + 60, 1);
  s = st();
  step('middle-drag pans', camMoved(a, s) && near(s.cam[3], a.cam[3]));
  a = s;
  c.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
  await drag(mx, my, mx - 70, my, 0);
  c.dispatchEvent(new KeyboardEvent('keyup', { key: ' ', bubbles: true }));
  s = st();
  step('Space+left-drag orbits without editing', !near(s.cam[3], a.cam[3]) && s.voxels === s0.voxels);
  a = s;
  ptr('pointermove', mx, my); await frames();
  const wheel = new WheelEvent('wheel', { clientX: mx, clientY: my, deltaY: -240, bubbles: true, cancelable: true });
  c.dispatchEvent(wheel);
  await frames();
  s = st();
  step('wheel zooms in', s.cam[5] < a.cam[5] && wheel.defaultPrevented, `${a.cam[5]} -> ${s.cam[5]}`);
  // A chord: left pressed and released during a right-drag arrives as pointermoves.
  a = st();
  ptr('pointermove', mx, my); await frames();
  ptr('pointerdown', mx, my, 2); await frames();
  buttons |= 1;
  ptr('pointermove', mx + 10, my, 0); await frames();
  buttons &= ~2;
  ptr('pointermove', mx + 20, my, 2); await frames();
  ptr('pointerup', mx + 20, my, 0); await frames();
  const afterChord = st();
  ptr('pointermove', mx + 80, my + 40); await frames();
  s = st();
  step('a chorded click during orbit neither edits nor sticks', s.voxels === s0.voxels && !camMoved(afterChord, s), JSON.stringify(s));
  await key('.');
  s = st();
  step('. frames the scene again', near(s.cam[5], s0.cam[5]) && near(s.cam[0], s0.cam[0]), `${s.cam} vs ${s0.cam}`);

  // Cmd-K.
  step('Cmd-K is kept from the browser', await key('k', cmd));
  step('Cmd-K opens the command list', st().cmdk);
  for (const ch of 'box') await key(ch);
  await key('Enter');
  s = st();
  step('typing a command and Enter runs it', !s.cmdk && s.shape === 'Box', JSON.stringify(s));
  await key('k', cmd);
  await key('Escape');
  step('Escape closes the command list', !st().cmdk);
  await key('?', { shiftKey: true });
  step('? opens the command list, as the coach card says', st().cmdk);
  await key('Escape');
  await click(1155, 34);
  step('the keyboard button opens the command list', st().cmdk);
  await key('Escape');
  await key('v');

  // Palette editing, clicking the drawn UI where the editor says each part is.
  const at = (name) => { const h = window.vxr.hit(name); return h.length ? h : null; };
  const clickAt = async (name) => { const h = at(name); if (h) await click(h[0], h[1]); return !!h; };
  const col = (slot) => Array.from(window.vxr.color(slot));
  const typeInto = async (name, text) => {
    await clickAt(name);
    await key('a', cmd);
    for (const ch of text) await key(ch);
    await key('Enter');
  };
  let used = 1;
  for (let i = 1; i < 256; i++) if (col(i)[4] > col(used)[4]) used = i;
  while (st().slot > used) await key('[');
  while (st().slot < used) await key(']');
  await frames();
  step('the Edit button opens the color editor', (await clickAt('edit-color')) && st().colorEdit, JSON.stringify(st()));
  const c0 = col(used);
  const sv = at('sv');
  await drag(sv[0] - 80, sv[1] - 50, sv[0] + 30, sv[1] + 10);
  let c1 = col(used);
  step('dragging in the color square recolors every voxel of the slot', c1.slice(0, 3).join() !== c0.slice(0, 3).join() && c1[4] === c0[4] && st().voxels === s0.voxels, `${c0} -> ${c1}`);
  await key('z', cmd);
  step('one undo returns the color from before the drag', col(used).join() === c0.join(), `${col(used)}`);
  await typeInto('field-hex', '#ff8800');
  step('the hex field sets the color', col(used).slice(0, 3).join() === '255,136,0', `${col(used)}`);
  await typeInto('field-b', '64');
  step('the B field sets one channel', col(used).slice(0, 3).join() === '255,136,64', `${col(used)}`);
  await key('z', cmd);
  await key('z', cmd);
  step('each field edit is one undo step', col(used).join() === c0.join(), `${col(used)}`);
  let free = 1;
  while (col(free)[4] > 0) free++;
  const f0 = col(free);
  await clickAt('swap');
  step('Swap with… arms the next swatch click', st().slotOp.includes('Swap'), st().slotOp);
  await clickAt(`sw${free}`);
  step('swap trades the colors and moves the voxels', col(free).join() === c0.join() && col(used)[4] === 0 && col(used).slice(0, 3).join() === f0.slice(0, 3).join() && st().slot === free, `${col(free)} ${col(used)}`);
  await key('z', cmd);
  step('one undo reverts the swap', col(used).join() === c0.join());
  while (st().slot > used) await key('[');
  while (st().slot < used) await key(']');
  await clickAt('copy');
  await clickAt(`sw${free + 1}`);
  step('copy gives another slot the color', col(free + 1).slice(0, 3).join() === c0.slice(0, 3).join(), `${col(free + 1)}`);
  await key('z', cmd);
  while (st().slot > used) await key('[');
  while (st().slot < used) await key(']');
  const [a1, b1] = [at(`sw${used}`), at(`sw${free}`)];
  await drag(a1[0], a1[1], b1[0], b1[1], 0, { ctrlKey: true });
  step('Ctrl-drag a swatch onto another swaps them', col(free).join() === c0.join(), `${col(free)}`);
  await key('z', cmd);
  await key('Escape');
  step('Escape closes the color editor', !st().colorEdit);
  await key('k', cmd);
  for (const ch of 'edit color') await key(ch);
  await key('Enter');
  step('Cmd-K "Edit color" opens it', st().colorEdit);
  await key('Escape');
  step('undoing every palette edit gives back the file', same(bytes(), original));
  await key('v');

  // The live render corner: on screen, sampling, an edit resets only nearby pixels, Cmd-K hides it.
  {
    const r0 = st().render;
    step('the render corner is on screen', r0.shown && window.vxr.hit('render').length === 2, JSON.stringify(r0));
    let r = r0;
    for (let i = 0; i < 300 && r.spp < 8; i++) { await frames(1); r = st().render; }
    step('the render corner path traces samples', r.spp >= 8 && r.rows > 0, `${JSON.stringify(r)} (GPU timestamps: ${r.timestamps})`);
    await key('v');
    await key('a');
    await click(mx, my);
    await frames(2);
    const r1 = st().render;
    step('an edit resets only nearby pixels of the render', r1.restarts === r.restarts && r1.rows > 0, `${JSON.stringify(r)} -> ${JSON.stringify(r1)}`);
    await key('z', cmd);
    await key('k', cmd);
    for (const ch of 'render preview') await key(ch);
    await key('Enter');
    await frames(2);
    step('Cmd-K hides the render corner', !st().render.shown && window.vxr.hit('render').length === 0, JSON.stringify(st().render));
    await key('k', cmd);
    for (const ch of 'render preview') await key(ch);
    await key('Enter');
    await frames(2);
    step('and shows it again', st().render.shown);

    // The Render panel: R opens it, the sliders and buttons reach the path tracer, Export PNG
    // downloads a PNG at the chosen size, Escape closes it.
    const hit = (name) => { const h = window.vxr.hit(name); return h.length ? h : null; };
    await key('r');
    step('R opens the Render panel', st().render.panel && !!hit('rs-export'), JSON.stringify(st().render));
    const ex = hit('rs-exposure');
    await drag(ex[0], ex[1], ex[0] - 120, ex[1]);
    step('the exposure slider sets -3 EV', Math.abs(st().render.exposure - 0.125) < 1e-3, JSON.stringify(st().render));
    await click(ex[0], ex[1]);
    await click(...hit('rs-spp0'));
    await click(...hit('rs-size0'));
    step('Samples 64 stops the corner at 64 spp', st().render.maxSpp === 64, JSON.stringify(st().render));
    await click(...hit('rs-sky-backdrop'));
    const skyOn = st().render.sky;
    await click(...hit('rs-sky-backdrop'));
    step('the sky backdrop toggles', skyOn && !st().render.sky);
    const realUrl = URL.createObjectURL;
    const realA = HTMLAnchorElement.prototype.click;
    let png = null, pngName = null;
    URL.createObjectURL = (b) => { png = b; return realUrl.call(URL, b); };
    HTMLAnchorElement.prototype.click = function () { pngName = this.download; };
    try {
      await click(...hit('rs-export'));
      const exporting = st().render.export !== null;
      for (let i = 0; i < 3000 && !pngName; i++) await frames(1);
      const b = png && new Uint8Array(await png.arrayBuffer());
      const dv = b && new DataView(b.buffer);
      const ok = b && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47;
      const [w, h] = ok ? [dv.getUint32(16), dv.getUint32(20)] : [0, 0];
      step('Export PNG renders and downloads island-render.png at 1280x720', exporting && ok && pngName === 'island-render.png' && w === 1280 && h === 720, `${pngName} ${w}x${h} ${b && b.length} bytes`);
    } finally {
      URL.createObjectURL = realUrl;
      HTMLAnchorElement.prototype.click = realA;
    }
    await click(...hit('rs-spp2'));
    await key('Escape');
    step('Escape closes the Render panel', !st().render.panel && st().render.maxSpp === 1024 && near(st().render.exposure, 1), JSON.stringify(st().render));
    step('the render steps leave the file as it was', same(bytes(), original));

    // Materials: Cmd-K "Edit material", the type buttons and sliders, the render restarts,
    // the download carries MATL chunks, and undo gives back the file.
    const mat = (slot) => Array.from(window.vxr.material(slot));
    await key('k', cmd);
    for (const ch of 'edit material') await key(ch);
    await key('Enter');
    step('Cmd-K "Edit material" opens the Material tab', st().colorEdit && !!hit('mt1'), JSON.stringify(st()));
    const slot = st().slot;
    const rs0 = st().render.restarts;
    await click(...hit('mt1'));
    await frames(2);
    step('Metal makes the slot metal and restarts the render', mat(slot)[0] === 1 && st().render.restarts > rs0, `${mat(slot)} ${st().render.restarts}`);
    const ro = hit('mat-rough');
    await drag(ro[0], ro[1], ro[0] - 150, ro[1]);
    step('the roughness slider sets 0', mat(slot)[1] === 0, `${mat(slot)}`);
    await key('z', cmd);
    step('one undo returns the roughness', mat(slot)[0] === 1 && Math.abs(mat(slot)[1] - 0.1) < 1e-6, `${mat(slot)}`);
    await click(...hit('mt3'));
    const em = hit('mat-flux');
    await drag(em[0], em[1], em[0] + 150, em[1]);
    step('Emit and its power slider', mat(slot)[0] === 3 && mat(slot)[6] === 4, `${mat(slot)}`);
    const saved = bytes();
    let matl = 0;
    for (let i = 0; i + 4 <= saved.length; i++) if (saved[i] === 77 && saved[i + 1] === 65 && saved[i + 2] === 84 && saved[i + 3] === 76) matl++;
    step('the saved file has the material as a MATL chunk', matl >= 1, `${matl} MATL`);
    while (st().undo && mat(slot)[0] !== 0) await key('z', cmd);
    await key('Escape');
    step('undoing the material edits gives back the file', same(bytes(), original) && !st().colorEdit);
  }

  // Save (download) and Open (file input).
  const input = document.getElementById('file');
  let picked = 0;
  const realClick = input.click;
  input.click = () => { picked++; };
  let blob = null;
  const realUrl = URL.createObjectURL;
  URL.createObjectURL = (b) => { blob = b; return realUrl.call(URL, b); };
  let downloaded = null;
  const realA = HTMLAnchorElement.prototype.click;
  HTMLAnchorElement.prototype.click = function () { downloaded = this.download; };
  try {
    await click(mx, my);
    step('Cmd-S is kept from the browser', await key('s', cmd));
    await frames();
    const saved = blob && new Uint8Array(await blob.arrayBuffer());
    const magic = saved && String.fromCharCode(...saved.slice(0, 4));
    step('Cmd-S downloads the scene as island.vox', magic === 'VOX ' && downloaded === 'island.vox', `${magic} ${downloaded}`);
    const kept = others(file);
    step('the saved edit keeps every other chunk of the file', saved && kept.length > 0 && kept.join('|') === others(saved).join('|'), `${kept.length} chunks`);
    blob = null;
    await click(354, 34);
    step('the Save button downloads too', !!blob);
    await key('z', cmd);
    step('Cmd-O opens the file picker', (await key('o', cmd)) && picked === 1, `${picked}`);
    await click(250, 34);
    step('the Open button opens the file picker', picked === 2, `${picked}`);
    const dt = new DataTransfer();
    dt.items.add(new File([saved], 'saved.vox'));
    input.files = dt.files;
    input.dispatchEvent(new Event('change'));
    for (let i = 0; i < 60 && st().name !== 'saved.vox'; i++) await frames(1);
    s = st();
    step('opening the saved file restores the edit', s.name === 'saved.vox' && s.voxels === s0.voxels + 1, JSON.stringify(s));
    const bad = new DataTransfer();
    bad.items.add(new File([new Uint8Array([1, 2, 3])], 'bad.vox'));
    input.files = bad.files;
    input.dispatchEvent(new Event('change'));
    await frames(10);
    s = st();
    step('a broken file keeps the scene', s.name === 'saved.vox' && s.voxels === s0.voxels + 1);
  } finally {
    input.click = realClick;
    URL.createObjectURL = realUrl;
    HTMLAnchorElement.prototype.click = realA;
  }

  // Resize: the canvas follows its box at the device pixel ratio.
  c.style.width = '900px';
  c.style.height = '500px';
  await frames(6);
  s = st();
  step('resize follows the canvas', s.size[0] === Math.round(900 * devicePixelRatio) && s.size[1] === Math.round(500 * devicePixelRatio), `${s.size}`);
  c.style.width = '';
  c.style.height = '';
  await frames(6);
  s = st();
  step('resize back', s.size[0] === Math.round(innerWidth * devicePixelRatio), `${s.size}`);
  return out.join('\n');
}
