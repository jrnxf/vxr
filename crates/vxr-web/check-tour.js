// The first-run tour, checked in the browser by crates/vxr-web/check (chrome-devtools-axi eval,
// one stage per call so the check can screenshot and reload between them):
//   async (stage) => lines of "ok <step>" or "FAIL <step>: <why>".
// The page is dist/ at 1280x800 with the tour's seen flag cleared before `first`:
//   first   the tour starts on Add, its card at the top right, its demo playing, the scene left of it
//   steps   each step's real action does it: add, orbit, box, move, then Open Help finishes
//   replay  Cmd-K and Help's tour row replay it, F1 hides and keeps it, Skip ends it and the
//           scene is framed in the whole view again
//   again   a new page leaves the tour out once it was seen
//   touch   a touch screen (1180x820) gets the card at the top left, the touch words and the
//           scene framed clear of the card and the panels
//   skip    Skip on the touch screen keeps the seen flag again and frames the scene in the whole view
async (stage) => {
  const out = [];
  const step = (name, pass, why = '') => out.push(pass ? `ok tour ${name}` : `FAIL tour ${name}: ${why}`);
  const c = document.getElementById('c');
  const frames = (n = 3) => new Promise((r) => { const f = () => (--n > 0 ? requestAnimationFrame(f) : r()); requestAnimationFrame(f); });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !window.vxr; i++) await sleep(50);
  if (!window.vxr) return 'FAIL tour: the editor did not start';
  const st = () => JSON.parse(window.vxr.state());
  const tour = () => st().tour;
  const until = async (f, ms = 5000) => { for (let t = 0; t < ms && !f(); t += 50) await sleep(50); return !!f(); };
  const kept = () => { try { return localStorage.getItem('vxr-toured'); } catch { return 'blocked'; } };
  const rect = (n) => window.vxr.hit_rect(n);
  // The scene's voxels on screen, in CSS pixels: clear of the card and the panels while the
  // card shows, under the top bar and on screen; in the middle of the view once it goes.
  const scene = () => window.vxr.scene_rect();
  const meets = (a, b) => a.length === 4 && b.length === 4 && a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
  const sceneClear = () => {
    const s = scene();
    const top = 60 * st().ppp / devicePixelRatio;
    const hit = ['tour', 'dock', 'dock-brushes', 'dock-more', 'gestures', 'stats', 'hint', 'tree', 'render', 'nav'].filter((n) => meets(s, rect(n)));
    const on = s[0] >= 0 && s[1] >= top && s[2] <= innerWidth && s[3] <= innerHeight;
    return [hit.length === 0 && on, JSON.stringify({ scene: s, hit })];
  };
  const sceneFramed = () => {
    const s = scene();
    const c = [(s[0] + s[2]) / 2, (s[1] + s[3]) / 2];
    return [Math.abs(c[0] - innerWidth / 2) < innerWidth * 0.05 && Math.abs(c[1] - innerHeight / 2) < innerHeight * 0.05, JSON.stringify(c)];
  };
  let buttons = 0;
  const bit = { 0: 1, 1: 4, 2: 2 };
  const ptr = (type, x, y, button = 0, kind = 'mouse') => {
    if (type === 'pointerdown') buttons |= bit[button];
    if (type === 'pointerup') buttons &= ~bit[button];
    c.dispatchEvent(new PointerEvent(type, {
      clientX: x, clientY: y, button: type === 'pointermove' ? -1 : button, buttons,
      pointerId: kind === 'touch' ? 31 : 1, pointerType: kind, isPrimary: true, bubbles: true, cancelable: true,
    }));
  };
  const click = async (x, y) => {
    ptr('pointermove', x, y); await frames();
    ptr('pointerdown', x, y); await frames();
    ptr('pointerup', x, y); await frames();
  };
  const clickAt = async (name) => { const [x, y] = window.vxr.hit(name); await click(x, y); };
  const drag = async (x0, y0, x1, y1, button = 0) => {
    ptr('pointermove', x0, y0, button); await frames();
    ptr('pointerdown', x0, y0, button); await frames();
    for (let i = 1; i <= 8; i++) { ptr('pointermove', x0 + ((x1 - x0) * i) / 8, y0 + ((y1 - y0) * i) / 8, button); await frames(1); }
    ptr('pointerup', x1, y1, button); await frames();
  };
  const key = async (k, mods = {}) => {
    const o = { key: k, bubbles: true, cancelable: true, ...mods };
    c.dispatchEvent(new KeyboardEvent('keydown', o));
    c.dispatchEvent(new KeyboardEvent('keyup', o));
    await frames();
  };
  // The island's middle: the tour frames it left of the card, so it is read from the page.
  const middle = () => { const s = scene(); return [(s[0] + s[2]) / 2, (s[1] + s[3]) / 2]; };
  // A step's action shows Done, and a moment later the next step.
  const doneThen = async (n, what) => {
    step(`${what} does the step`, tour().done, JSON.stringify(tour()));
    step(`then the tour moves on to step ${n}`, await until(() => tour().step === n && !tour().done), JSON.stringify(tour()));
  };

  if (stage === 'first') {
    await frames(10);
    const t = tour();
    const card = rect('tour');
    step('a first visit starts the tour on Add', t.step === 1 && t.id === 'add' && !t.done && !t.seen && t.ended === null, JSON.stringify(t));
    step('the browser has not kept the seen flag yet', kept() === null, kept());
    step('the card is 400 wide at the top right of the view', card.length === 4 && Math.abs(card[2] - card[0] - 400) < 0.5 && Math.abs(card[2] - 1268) < 0.5 && Math.abs(card[1] - 60) < 0.5, JSON.stringify(card));
    step('the card plays the add demo', t.card && t.demo === 'add' && t.image && t.marched <= 1 && rect('tour-demo').length === 4, JSON.stringify(t));
    step('it says what to do with the mouse', /^Click the scene/.test(t.body), t.body);
    step('Add is ringed, Skip shows and Next waits for the action', rect('tool-add').length === 4 && rect('tour-skip').length === 4 && rect('tour-next').length === 0);
    const [ok, why] = sceneClear();
    step('the scene stands left of the card, clear of it', ok && scene()[2] <= card[0], why);
    await sleep(500);
  } else if (stage === 'steps') {
    const [mx, my] = middle();
    // 1, Add: a click on the island.
    const v = st().voxels;
    await click(mx, my);
    step('a click adds a voxel', st().voxels === v + 1, `${v} -> ${st().voxels}`);
    await doneThen(2, 'adding a voxel');
    // 2, Look: a right-drag orbits; Next moves on at once.
    step('Look plays the orbit demo', tour().id === 'orbit' && tour().demo === 'orbit', JSON.stringify(tour()));
    const cam = st().cam;
    await drag(mx, my, mx + 40, my, 2);
    step('a right-drag orbits and does the step', st().cam.some((x, i) => x !== cam[i]) && tour().done, JSON.stringify(tour()));
    await clickAt('tour-next');
    step('Next moves on to Box at once', tour().step === 3 && tour().id === 'box', JSON.stringify(tour()));
    // 3, Box: B, a drag for the base, up for its height, a click to place it.
    step('Box rings the box brush', rect('brush-box').length === 4);
    await key('b');
    await drag(mx - 40, my, mx + 40, my + 20);
    step('the base alone is not the box yet', st().lifting && !tour().done, JSON.stringify([st().lifting, tour().done]));
    ptr('pointermove', mx + 40, my - 100); await frames();
    await click(mx + 40, my - 100);
    step('placing the lifted box places it', !st().lifting);
    await doneThen(4, 'placing the box');
    // 4, Move: S, a box select, a drag by one of its voxels.
    step('Move rings Select', rect('select').length === 4);
    await key('s');
    await drag(mx - 60, my - 40, mx + 60, my + 40);
    step('a box select alone is not the move', st().selected > 0 && !tour().done, JSON.stringify([st().selected, tour().done]));
    ptr('pointermove', mx, my); await frames();
    ptr('pointerdown', mx, my); await frames();
    for (let i = 1; i <= 6; i++) { ptr('pointermove', mx + i * 8, my); await frames(1); }
    ptr('pointerup', mx + 48, my); await frames();
    await doneThen(5, 'dragging the selection');
    // 5, Help: no demo, Open Help finishes the tour and the page keeps the flag.
    const t = tour();
    step('the last step points at Help with no demo', t.id === 'help' && t.demo === null && rect('tour-help').length === 4 && rect('tour-demo').length === 0 && rect('help').length === 4, JSON.stringify(t));
    await clickAt('tour-help');
    const e = tour();
    step('Open Help finishes the tour', st().help.open && e.step === null && e.ended === 'finished' && e.seen && !e.card, JSON.stringify(e));
    step('the page keeps the seen flag', kept() === '1', kept());
  } else if (stage === 'replay') {
    // Help is still open from Open Help: its tour row replays it.
    step('Help has the tour row', rect('help-tour').length === 4 && st().help.open);
    await clickAt('help-tour');
    await frames();
    step('Help\'s tour row closes Help and starts the tour again', !st().help.open && tour().step === 1 && tour().ended === null, JSON.stringify(tour()));
    await key('F1');
    step('Help opened hides the card and keeps the step', st().help.open && tour().step === 1 && !tour().card, JSON.stringify(tour()));
    await key('F1');
    step('Help closed again shows the card', !st().help.open && tour().card, JSON.stringify(tour()));
    await clickAt('tour-skip');
    step('Skip ends the tour', tour().step === null && tour().ended === 'skipped' && tour().seen && kept() === '1', JSON.stringify(tour()));
    await frames(3);
    step('closed, the tour draws and marches nothing', !tour().card && tour().marched === 0 && rect('tour').length === 0, JSON.stringify(tour()));
    step('the card gone, the scene is framed in the whole view again', ...sceneFramed());
    await key('k', { metaKey: true });
    for (const ch of 'take the tour') await key(ch);
    await key('Enter');
    await frames();
    step('Cmd-K Take the tour starts it again', tour().step === 1 && !st().cmdk, JSON.stringify([tour(), st().cmdk]));
    await clickAt('tour-skip');
    step('and Skip ends it again', tour().step === null && tour().ended === 'skipped', JSON.stringify(tour()));
  } else if (stage === 'again') {
    await frames(10);
    step('a new page leaves the tour out once it was seen', tour().step === null && tour().seen && !tour().card, JSON.stringify(tour()));
  } else if (stage === 'touch') {
    await sleep(800);
    const t = tour();
    // The touch layout's points are larger than CSS pixels.
    const z = st().ppp / devicePixelRatio;
    const card = rect('tour').map((v) => v / z);
    step('a touch screen starts the tour', st().touch && t.step === 1 && t.card, JSON.stringify([st().touch, t]));
    step('on a tablet the card is 400 points wide at the top left', card.length === 4 && Math.abs(card[0] - 12) < 0.5 && Math.abs(card[1] - 60) < 0.5 && Math.abs(card[2] - card[0] - 400) < 0.5, JSON.stringify(card));
    step('it says what to do with taps', /^Tap the scene/.test(t.body) && !/click|right-drag/i.test(t.body), t.body);
    const clear = ['dock', 'render', 'nav'].filter((n) => { const r = rect(n).map((v) => v / z); return r.length === 4 && r[0] < card[2] && card[0] < r[2] && r[1] < card[3] && card[1] < r[3]; });
    step('the card is clear of the dock, the render corner and the camera bar', clear.length === 0, JSON.stringify(clear));
    step('on a tablet the scene stands clear of the card and the panels', ...sceneClear());
  } else if (stage === 'skip') {
    const [x, y] = window.vxr.hit('tour-skip');
    ptr('pointerdown', x, y, 0, 'touch'); await frames(1); ptr('pointerup', x, y, 0, 'touch'); await frames(4);
    step('a tap on Skip ends it and keeps the flag', tour().step === null && tour().ended === 'skipped' && kept() === '1', JSON.stringify([tour(), kept()]));
    step('the card gone, the tablet frames the scene in the whole view again', ...sceneFramed());
  }
  return out.join('\n');
}
