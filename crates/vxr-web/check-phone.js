async (stage, view) => {
  // The touch layout on a phone (check runs it at 390x844 and 844x390, 3x): one stage per
  // call, so the check can screenshot between them. The page keeps its state across calls.
  for (let i = 0; i < 300 && !window.vxr; i++) await new Promise((r) => setTimeout(r, 50));
  const c = document.getElementById('c');
  const frames = (n = 3) => new Promise((r) => { const f = () => (--n > 0 ? requestAnimationFrame(f) : r()); requestAnimationFrame(f); });
  if (!window.__phone) {
    await new Promise((r) => setTimeout(r, 800));
    window.__phone = true;
  }
  const touch = (type, x, y) => c.dispatchEvent(new PointerEvent(type, {
    clientX: x, clientY: y, button: type === 'pointermove' ? -1 : 0, buttons: type === 'pointerup' ? 0 : 1,
    pointerId: 30, pointerType: 'touch', isPrimary: true, bubbles: true, cancelable: true,
  }));
  const tap = async (name) => {
    const [x, y] = window.vxr.hit(name);
    touch('pointerdown', x, y); await frames(1); touch('pointerup', x, y); await frames(4);
  };
  const out = [];
  const step = (what, ok, extra = '') => out.push(`${ok ? 'ok' : 'FAIL'} phone ${view} ${what}${ok ? '' : ': ' + extra}`);
  const W = innerWidth, H = innerHeight;
  const rect = (n) => window.vxr.hit_rect(n);
  const on = (r) => r.length === 4 && r[0] >= -0.5 && r[1] >= -0.5 && r[2] <= W + 0.5 && r[3] <= H + 0.5;
  const targets = ['tool-add', 'tool-erase', 'tool-paint', 'tool-pick', 'select', 'dock-color', 'undo', 'redo', 'voxel',
    'brush-box', 'brush-face', 'brush-line', 'center', 'mirror', 'lock', 'open', 'save', 'export', 'theme', 'ai-toggle', 'keys'];
  // Every dock and top bar target is on screen, and the dock's are at least 44 CSS px.
  const reachable = (what) => {
    const lost = targets.filter((n) => !on(rect(n)));
    const small = targets.slice(0, 15).filter((n) => { const r = rect(n); return r.length === 4 && Math.min(r[2] - r[0], r[3] - r[1]) < 43.5; });
    step(`${what}: every dock and top bar target is on screen`, lost.length === 0, JSON.stringify(lost));
    step(`${what}: every dock target is 44 CSS px or more`, small.length === 0, JSON.stringify(small));
  };
  const shown = (name, what) => step(`${what} is on screen`, on(rect(name)), JSON.stringify(rect(name)));
  const state = () => JSON.parse(window.vxr.state());
  if (stage === 'plain') {
    step('starts in the touch layout', state().touch && window.vxr.hit('dock').length > 0);
    reachable('plain');
  } else if (stage === 'assistant') {
    await tap('ai-toggle');
    step('the assistant opens', window.vxr.hit('assistant').length > 0);
    shown('assistant', 'the assistant');
    reachable('with the assistant open');
  } else if (stage === 'palette') {
    await tap('dock-color');
    step('the colour button opens the palette with the assistant open', window.vxr.hit('palette').length > 0);
    shown('palette', 'the palette');
    reachable('with the assistant and the palette open');
  } else if (stage === 'select') {
    await tap('dock-color');
    await tap('ai-toggle');
    step('the palette and the assistant close', window.vxr.hit('palette').length === 0 && window.vxr.hit('assistant').length === 0);
    await tap('select');
    step('the select button opens its flyout', window.vxr.hit('selection').length > 0);
    shown('selection', 'the selection flyout');
    reachable('with the selection flyout open');
  } else if (stage === 'export') {
    await tap('tool-add');
    await tap('export');
    step('the export button opens the Export menu', window.vxr.hit('export-obj').length > 0);
    shown('export-obj', 'the Export menu');
    reachable('with the Export menu open');
  } else if (stage === 'colour') {
    await tap('export');
    await tap('dock-color');
    await tap('edit-color');
    step('Edit opens the colour editor', window.vxr.hit('sv').length > 0);
    shown('sv', 'the colour editor');
    reachable('with the colour editor open');
  } else if (stage === 'commands') {
    await tap('keys');
    step('the keyboard button opens the command list', window.vxr.hit('cmdk').length > 0);
    shown('cmdk', 'the command list');
  }
  return out.join('\n');
}
