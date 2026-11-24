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
  // Held upright with the account control in the bar, the theme button gives up its place
  // (the themes are in the command list), so the two halves of the top bar never overlap.
  const noTheme = W < 400 && rect('account').length === 4;
  const apart = (what) => {
    const left = ['library', 'scene-select'].map(rect).filter((r) => r.length === 4);
    const right = ['help', 'theme', 'ai-toggle', 'keys', 'account'].map(rect).filter((r) => r.length === 4);
    const hit = left.some((a) => right.some((b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]));
    const gap = Math.min(...right.map((r) => r[0])) - Math.max(...left.map((r) => r[2]));
    step(`${what}: the two halves of the top bar do not overlap`, left.length === 2 && right.length >= 3 && !hit && gap >= 4, `gap ${gap}`);
  };
  const allTargets = ['tool-add', 'tool-erase', 'tool-paint', 'tool-pick', 'select', 'dock-color', 'undo', 'redo', 'voxel',
    'brush-box', 'brush-face', 'brush-line', 'center', 'pattern', 'mirror', 'lock', 'library', 'scene-select', 'theme', 'ai-toggle', 'keys'];
  const targets = allTargets.filter((n) => !(n === 'theme' && noTheme));
  // Every dock and top bar target is on screen, and the dock's are at least 44 CSS px.
  const reachable = (what) => {
    const lost = targets.filter((n) => !on(rect(n)));
    const small = targets.slice(0, 15).filter((n) => { const r = rect(n); return r.length === 4 && Math.min(r[2] - r[0], r[3] - r[1]) < 43.5; });
    step(`${what}: every dock and top bar target is on screen`, lost.length === 0, JSON.stringify(lost));
    step(`${what}: every dock target is 44 CSS px or more`, small.length === 0, JSON.stringify(small));
    apart(what);
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
    await tap('scene-select');
    await frames(14);
    step('the file button opens the file menu', window.vxr.hit('export-obj').length > 0);
    shown('export-obj', 'the file menu');
    reachable('with the file menu open');
  } else if (stage === 'colour') {
    await tap('scene-select');
    await tap('dock-color');
    await tap('edit-color');
    step('Edit opens the colour editor', window.vxr.hit('sv').length > 0);
    shown('sv', 'the colour editor');
    reachable('with the colour editor open');
  } else if (stage === 'commands') {
    await tap('keys');
    step('the command button opens the command list', window.vxr.hit('cmdk').length > 0);
    shown('cmdk', 'the command list');
  }
  return out.join('\n');
}
