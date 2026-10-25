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

  // Line and center brushes: shown live while dragging, one undo step each.
  await key('l');
  ptr('pointermove', mx - 60, my); await frames();
  ptr('pointerdown', mx - 60, my); await frames();
  for (let i = 1; i <= 8; i++) { ptr('pointermove', mx - 60 + i * 15, my + i * 2); await frames(1); }
  s = st();
  ptr('pointerup', mx + 60, my + 16); await frames();
  step('L line drag shows the line live', s.shape === 'Line' && s.span && s.span[0] === 'LINE' && s.voxels > s0.voxels + 3 && st().voxels === s.voxels,
    JSON.stringify([s.span, s0.voxels, s.voxels, st().voxels]));
  await key('z', cmd);
  await key('c');
  ptr('pointermove', mx, my); await frames();
  ptr('pointerdown', mx, my); await frames();
  for (let i = 1; i <= 8; i++) { ptr('pointermove', mx + i * 8, my); await frames(1); }
  s = st();
  ptr('pointerup', mx + 64, my); await frames();
  const circle = st().voxels - s0.voxels;
  step('C circle drag grows from its centre', s.shape === 'Circle' && s.span && s.span[0] === 'RADIUS' && circle > 4, JSON.stringify([s.span, circle]));
  await key('z', cmd);
  await key('c');
  await drag(mx, my, mx + 64, my);
  step('C again: a sphere drag adds more than the circle', st().shape === 'Sphere' && st().voxels - s0.voxels > circle,
    `${st().voxels - s0.voxels} vs ${circle}`);
  await key('z', cmd);
  await key('v');

  // Mirror axes from the flyout, and the axis lock.
  await key('m');
  const chip = async (name) => { const h = window.vxr.hit(name); if (h.length) await click(h[0], h[1]); return h.length > 0; };
  const at = (name) => { const h = window.vxr.hit(name); return h.length ? h : null; };
  const clickAt = async (name) => { const h = at(name); if (h) await click(h[0], h[1]); return !!h; };
  const typeInto = async (name, text) => {
    await clickAt(name);
    await key('a', cmd);
    for (const ch of text) await key(ch);
    await key('Enter');
  };
  const shown = await chip('mirror-y');
  step('the mirror flyout adds Y', shown && JSON.stringify(st().mirrorAxes) === '[true,true,false]', JSON.stringify(st().mirrorAxes));
  await chip('mirror-x');
  await chip('mirror-y');
  step('turning off the last axis turns mirror off', !st().mirror && window.vxr.hit('mirror-x').length === 0, JSON.stringify(st()));
  await key('k');
  step('K locks X', st().lock === 'X' && window.vxr.hit('lock-x').length === 2, JSON.stringify(st().lock));
  await key('k');
  await key('k');
  await key('k');
  step('K again locks Y, Z, then off', st().lock === null);

  // Selection and transform: S, colour click, Escape, box select, a live move drag, the
  // flyout, copy and paste, Backspace and Cmd-A, hollow and fill inside, crop and resize, each
  // edit undone again.
  await key('s');
  step('S picks the select tool and shows the selection flyout', st().tool === 'Select' && window.vxr.hit('rotate-y').length === 2, st().tool);
  await click(mx, my);
  const byColor = st().selected;
  step('a click selects every voxel of that color', byColor > 0 && st().voxels === s0.voxels, `${byColor}`);
  await click(mx, my, { shiftKey: false, altKey: true });
  step('Alt-click subtracts it again', st().selected === 0, `${st().selected}`);
  await click(mx, my);
  await key('Escape');
  step('Escape deselects', st().selected === 0);
  await drag(mx - 60, my - 40, mx + 60, my + 40);
  const boxed = st().selected;
  step('a box drag selects the voxels inside it', boxed > 0 && boxed !== byColor && same(bytes(), original), `${boxed}`);
  ptr('pointermove', mx, my); await frames();
  ptr('pointerdown', mx, my); await frames();
  for (let i = 1; i <= 6; i++) { ptr('pointermove', mx + i * 8, my); await frames(1); }
  const moving = st().span;
  ptr('pointerup', mx + 48, my); await frames();
  step('dragging the selection moves it live', moving && moving[0] === 'MOVE' && moving[1] !== '0,0,0' && !same(bytes(), original) && st().selected > 0, JSON.stringify(moving));
  await key('z', cmd);
  step('Cmd-Z puts the moved selection back', same(bytes(), original) && st().selected === boxed, `${st().selected}`);
  await chip('rotate-y');
  const turned = !same(bytes(), original);
  await key('z', cmd);
  await chip('flip-x');
  const flipped = !same(bytes(), original);
  await key('z', cmd);
  step('the flyout rotates and flips the selection, one undo step each', turned && flipped && same(bytes(), original));
  await key('c', cmd);
  ptr('pointermove', mx - 150, my + 60); await frames();
  await key('v', cmd);
  step('Cmd-C, Cmd-V pastes a copy at the pointer, selected', st().hasClip && !same(bytes(), original) && st().selected > 0, JSON.stringify([st().hasClip, st().selected, st().voxels - s0.voxels]));
  await key('z', cmd);
  await click(mx, my);
  const v0 = st().voxels;
  await key('Backspace');
  step('Backspace deletes the selection', st().voxels === v0 - byColor && st().selected === 0, `${v0} -> ${st().voxels}`);
  await key('z', cmd);
  await key('a', cmd);
  step('Cmd-A selects every voxel', st().selected === st().voxels, `${st().selected}`);
  await key('Escape');
  // Model tools on the whole island: hollow, fill inside, one undo step each.
  const solid = st().voxels;
  const hollowed = await chip('hollow') && st().voxels;
  step('the flyout hollows the model', hollowed > 0 && hollowed < solid, `${solid} -> ${hollowed}`);
  await chip('fill-in');
  const refilled = st().voxels;
  step('fill inside fills what the shell encloses', refilled >= solid, `${hollowed} -> ${refilled}`);
  await key('z', cmd);
  await key('z', cmd);
  step('hollow and fill inside are one undo step each', same(bytes(), original), `${st().voxels}`);
  // Crop to content and resize through the Bounds panel, one undo step each. The island
  // opens at its model's exact size, so first grow it 20 cells on X (10 on each side) for
  // crop to take off again.
  const fitted = st().bounds;
  await chip('resize');
  await typeInto('bounds-x', String(fitted[0] + 20));
  await key('Escape');
  const bounds0 = st().bounds;
  const grown = bytes();
  step('the Bounds panel grows the island', bounds0[0] === fitted[0] + 20 && st().voxels === solid, `${fitted} -> ${bounds0}`);
  await chip('crop');
  const bounds1 = st().bounds;
  step('the flyout crops the bounds to the island', bounds1.every((v, i) => v === fitted[i]) && st().voxels === solid, `${bounds0} -> ${bounds1}`);
  step('crop keeps every voxel: the save is the original bytes again', same(bytes(), original));
  await key('z', cmd);
  step('crop is one undo step', st().bounds.join() === bounds0.join(), `${st().bounds}`);
  step('Resize… opens the Bounds panel', (await chip('resize')) && st().boundsPanel && !!at('bounds-x'));
  await typeInto('bounds-x', String(bounds0[0] - 40));
  const narrowed = st().voxels;
  step('the X field resizes the bounds and erases what falls outside', st().bounds[0] === bounds0[0] - 40 && narrowed < solid, `${st().bounds} ${solid} -> ${narrowed}`);
  await key('z', cmd);
  step('resize is one undo step', same(bytes(), grown) && st().bounds.join() === bounds0.join(), `${st().bounds}`);
  await key('Escape');
  step('Escape closes the Bounds panel', !st().boundsPanel);
  await key('k', cmd);
  for (const ch of 'crop to content') await key(ch);
  await key('Enter');
  const bounds2 = st().bounds;
  await key('z', cmd);
  step('Cmd-K crops to content, and undo restores the bounds', bounds2.join() === bounds1.join() && st().bounds.join() === bounds0.join(), `${bounds2}`);
  await key('z', cmd);
  step('undoing the grow gives back the opened island', same(bytes(), original) && st().bounds.join() === fitted.join());
  await key('a');
  step('undo after every tool returns to the island', same(bytes(), original));

  // Camera and view: the View flyout, orthographic, the six side views by key, an edit from
  // the top orthographic view, isometric, O, Cmd-K, then back to perspective and framed.
  step('the view button opens the View flyout', (await chip('view')) && st().viewPanel && !!at('view-front'));
  await chip('view-ortho');
  step('Ortho turns orthographic on', st().ortho && st().view === null, JSON.stringify(st().cam));
  const sideViews = [['1', 'Front'], ['2', 'Back'], ['3', 'Left'], ['4', 'Right'], ['6', 'Bottom'], ['5', 'Top']];
  const seen = [];
  for (const [k, name] of sideViews) { await key(k); seen.push(st().view); }
  step('keys 1 to 6 snap to the side views', seen.join() === sideViews.map((v) => v[1]).join(), seen.join());
  const vTop = st().voxels;
  await click(640, 400);
  const vAdded = st().voxels;
  await key('z', cmd);
  step('a click from the top orthographic view adds a voxel, undo removes it', vAdded === vTop + 1 && same(bytes(), original), `${vTop} -> ${vAdded}`);
  await chip('view-iso');
  step('Iso: orthographic isometric', st().ortho && st().view === 'Isometric', `${st().view}`);
  await key('o');
  const oOff = !st().ortho;
  await key('o');
  step('O toggles orthographic', oOff && st().ortho);
  await key('k', cmd);
  for (const ch of 'front view') await key(ch);
  await key('Enter');
  step("Cmd-K 'Front view'", st().view === 'Front', `${st().view}`);
  await key('0');
  step('0 snaps to isometric', st().view === 'Isometric' && st().ortho);
  await chip('view-persp');
  step('Persp: back to perspective', !st().ortho && st().view === null);
  // Guides: grid, ground and bounds on by default; the flyout, G and Cmd-K toggle them.
  step('the grid, ground and bounds show by default', st().guides.join() === 'true,true,true', JSON.stringify(st().guides));
  const guideFlips = [];
  for (const [n, i] of [['view-grid', 0], ['view-ground', 1], ['view-bounds', 2]]) {
    await chip(n);
    const off = !st().guides[i];
    await chip(n);
    guideFlips.push(off && st().guides[i]);
  }
  step("the flyout's Grid, Ground and Bounds buttons hide and show them", guideFlips.every(Boolean), guideFlips.join());
  await key('g');
  const gridOff = !st().guides[0];
  await key('g');
  step('G toggles the grid', gridOff && st().guides[0]);
  await key('k', cmd);
  for (const ch of 'show bounds') await key(ch);
  await key('Enter');
  const boundsOff = !st().guides[2];
  await key('k', cmd);
  for (const ch of 'show bounds') await key(ch);
  await key('Enter');
  step("Cmd-K 'Show bounds' hides and shows the bounds", boundsOff && st().guides[2]);
  await key('Escape');
  step('Escape closes the View flyout', !st().viewPanel);

  // World mode: the island is one object, so Tab shows the same voxels as the world and back.
  const m0 = st();
  await key('Tab');
  const inWorld = st();
  await key('Tab');
  step('Tab shows the world and goes back to the model',
    m0.mode === 'object' && inWorld.mode === 'world' && inWorld.voxels === m0.voxels && st().mode === 'object',
    `${m0.mode} ${inWorld.mode} ${st().mode}`);
  await key('k', cmd);
  for (const ch of 'world mode') await key(ch);
  await key('Enter');
  const cmdWorld = st().mode;
  await key('Tab');
  step("Cmd-K 'World mode / edit model'; switching adds no undo step",
    cmdWorld === 'world' && st().mode === 'object' && st().undo === m0.undo);
  await key('.');

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
  const col = (slot) => Array.from(window.vxr.color(slot));
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

    // World mode on the generated 22-world file (several objects, groups, linked copies):
    // pick, drag, undo, nudge, duplicate, linked copy, delete, Escape, Cmd-K, and a save
    // that keeps every other chunk, then undo back to the file's bytes.
    const worldFile = new Uint8Array(await (await fetch('__fixtures/22-world.vox')).arrayBuffer());
    const wdt = new DataTransfer();
    wdt.items.add(new File([worldFile], '22-world.vox'));
    input.files = wdt.files;
    input.dispatchEvent(new Event('change'));
    for (let i = 0; i < 60 && st().name !== '22-world.vox'; i++) await frames(1);
    await frames(6);
    const w0 = st();
    step('a file of several objects opens in world mode', w0.mode === 'world' && w0.objects === 6 && same(bytes(), worldFile), JSON.stringify(w0));
    // A point on an object near the middle: a click there picks it and edits nothing.
    let at = null;
    for (const [dx, dy] of [[0, 0], [0, 30], [-40, 20], [40, 20], [0, 60], [-80, 40], [80, 40]]) {
      await click(640 + dx, 400 + dy);
      if (st().picked) { at = [640 + dx, 400 + dy]; break; }
    }
    s = st();
    step('a click in world mode picks an object and edits nothing', !!at && s.selected > 0 && s.voxels === w0.voxels && !s.undo, JSON.stringify(s));
    if (at) {
      const name = st().picked;
      await drag(at[0], at[1], at[0] + 90, at[1] + 10);
      s = st();
      step('a drag moves the picked object', s.picked === name && s.undo && !same(bytes(), worldFile), JSON.stringify(s));
      const moved = bytes();
      await key('z', cmd);
      step('one undo puts it back', same(bytes(), worldFile) && st().picked === name);
      await key('z', { metaKey: true, shiftKey: true });
      step('redo moves it again', same(bytes(), moved));
      await key('ArrowRight');
      step('an arrow nudges the picked object', !same(bytes(), moved) && st().voxels === w0.voxels);
      await key('d', cmd);
      s = st();
      step('Cmd-D duplicates it with its own model and picks the copy', s.objects === 7 && s.models === w0.models + 1 && !!s.picked, JSON.stringify(s));
      await key('d', { metaKey: true, shiftKey: true });
      s = st();
      step('Cmd-Shift-D makes a linked copy sharing the model', s.objects === 8 && s.models === w0.models + 1, JSON.stringify(s));
      await key('Backspace');
      s = st();
      step('Backspace deletes the picked object', s.objects === 7 && s.picked === null && s.selected === 0, JSON.stringify(s));
      await click(...at);
      const had = st().picked;
      await key('Escape');
      step('Escape picks nothing', !!had && st().picked === null);
      await click(...at);
      await key('k', cmd);
      for (const ch of 'duplicate object') await key(ch);
      await key('Enter');
      step("Cmd-K 'Duplicate object'", st().objects === 8, JSON.stringify(st()));
      const edited = bytes();
      step('the edited world keeps every other chunk of the file', others(edited).join('|') === others(worldFile).join('|'));
      for (let i = 0; i < 20 && st().undo; i++) await key('z', cmd);
      step('every world edit undone gives back the file', same(bytes(), worldFile) && st().objects === 6, JSON.stringify(st()));
    }

    // The scene tree: rows, a row click picks, the eye and H hide, a double-click renames
    // (Escape cancels), and in model mode a row click edits that object.
    const rows = st().tree;
    const node = (name) => (st().tree.find((r) => r.name === name) || {}).node;
    const treeC = rows.find((r) => r.name === 'tree c');
    step('the scene tree shows every object and group', rows.length === 8 && !!treeC && treeC.depth === 2 && treeC.linked === 3 && rows.some((r) => r.name === 'lost gem' && r.hidden), JSON.stringify(rows));
    const slab = node('slab');
    await chip(`tree-${slab}`);
    step('a tree row click picks its object', st().picked === 'slab' && st().treeOn === slab, JSON.stringify(st()));
    const t0 = st().voxels;
    await chip(`tree-eye-${slab}`);
    s = st();
    step('the row eye hides the object as one undo step', s.voxels === t0 - 64 && s.undo, JSON.stringify(s));
    await key('z', cmd);
    step('undo shows it again', st().voxels === t0 && same(bytes(), worldFile));
    await key('h');
    const hHid = st().voxels === t0 - 64;
    await key('h');
    step('H hides and shows the picked object', hHid && st().voxels === t0);
    const pause = () => new Promise((r) => setTimeout(r, 400));
    const dbl = async (name) => { await pause(); await chip(name); await chip(name); };
    await dbl(`tree-${slab}`);
    const field = window.vxr.hit('tree-rename').length > 0;
    await key('a', cmd);
    for (const ch of 'floor') await key(ch);
    await key('Enter');
    step('a double-click on a row renames it', field && node('floor') === slab, JSON.stringify(st().tree));
    await key('z', cmd);
    step('the rename is one undo step', node('slab') === slab && same(bytes(), worldFile));
    await dbl(`tree-${slab}`);
    await key('x');
    await key('Escape');
    step('Escape cancels a rename', node('slab') === slab && st().picked === 'slab' && window.vxr.hit('tree-rename').length === 0);

    // Groups: a group row picks the group, a drag on one of its objects and the arrows move
    // it whole, Cmd-D copies it, Cmd-Shift-G ungroups, Cmd-G and Cmd-K 'Ungroup' wrap an
    // object and take it out again, Tab edits the group's first object.
    const trees = node('trees');
    await pause();
    await chip(`tree-${trees}`);
    s = st();
    step('a group row picks the group and selects its objects', s.picked === 'trees' && s.treeOn === trees && s.selected > 0, JSON.stringify(s));
    let groupDrag = false;
    for (const [dx, dy] of [[0, 0], [0, 30], [-40, 20], [40, 20], [0, 60], [-80, 40], [80, 40], [-40, -20], [40, -20]]) {
      await pause();
      await chip(`tree-${trees}`);
      await drag(640 + dx, 400 + dy, 640 + dx + 90, 400 + dy + 10);
      if (st().picked === 'trees' && !same(bytes(), worldFile)) { groupDrag = true; break; }
      if (st().undo) await key('z', cmd);
    }
    step('a drag on an object of the picked group moves the group', groupDrag, JSON.stringify(st()));
    await key('z', cmd);
    step('the group drag is one undo step', same(bytes(), worldFile) && st().picked === 'trees');
    await key('ArrowRight');
    step('an arrow nudges the picked group', !same(bytes(), worldFile) && st().picked === 'trees');
    await key('z', cmd);
    await key('d', cmd);
    s = st();
    step('Cmd-D copies the group and its objects', s.objects === 9 && s.tree.length === 13, JSON.stringify(s));
    await key('z', cmd);
    await pause();
    await chip(`tree-${trees}`);
    await key('g', { metaKey: true, shiftKey: true });
    s = st();
    step('Cmd-Shift-G ungroups the picked group', s.tree.length === 7 && s.picked === 'tree a' && node('trees') === undefined, JSON.stringify(s));
    await key('z', cmd);
    step('ungrouping is one undo step', st().tree.length === 8 && same(bytes(), worldFile));
    await pause();
    await chip(`tree-${slab}`);
    await key('g', cmd);
    s = st();
    const wrapped = s.tree.find((r) => r.node === slab);
    step('Cmd-G puts the picked object into a new group', s.tree.length === 9 && s.picked === 'group 1' && wrapped && wrapped.depth === 1, JSON.stringify(s));
    await key('k', cmd);
    for (const ch of 'ungroup') await key(ch);
    await key('Enter');
    step("Cmd-K 'Ungroup' takes it out again", st().tree.length === 8 && st().picked === 'slab', JSON.stringify(st()));
    for (let i = 0; i < 2; i++) await key('z', cmd);
    step('every group edit undone gives back the file', same(bytes(), worldFile), JSON.stringify(st()));
    await pause();
    await chip(`tree-${trees}`);
    await key('Tab');
    step('Tab with a group picked edits its first object', st().mode === 'object' && st().treeOn === node('tree a'), JSON.stringify(st()));
    await key('Tab');
    await pause();
    await chip(`tree-${slab}`);
    await key('Tab');
    const treeA = node('tree a');
    await chip(`tree-${treeA}`);
    s = st();
    step('in model mode a tree row click edits that object', s.mode === 'object' && s.treeOn === treeA, JSON.stringify(s));
    await key('Tab');
    step('the tree leaves the file as it was', same(bytes(), worldFile) && st().mode === 'world');
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
