async (stage) => {
  // A draft outlives its page (check runs it in two pages, in this order): 'edit' adds a
  // voxel to the island and waits for its draft; in a new page 'offer' finds the restore
  // prompt (the check screenshots it), and 'restore' brings the edit back.
  for (let i = 0; i < 300 && !window.vxr; i++) await new Promise((r) => setTimeout(r, 50));
  const c = document.getElementById('c');
  const frames = (n = 3) => new Promise((r) => { const f = () => (--n > 0 ? requestAnimationFrame(f) : r()); requestAnimationFrame(f); });
  const until = async (f, ms = 6000) => {
    const t = performance.now();
    while (!f() && performance.now() - t < ms) await frames(1);
    return f();
  };
  const st = () => JSON.parse(window.vxr.state());
  const ptr = (type, x, y) => c.dispatchEvent(new PointerEvent(type, {
    clientX: x, clientY: y, button: type === 'pointermove' ? -1 : 0, buttons: type === 'pointerdown' ? 1 : 0,
    pointerId: 1, pointerType: 'mouse', isPrimary: true, bubbles: true, cancelable: true,
  }));
  const click = async (x, y) => {
    ptr('pointermove', x, y); await frames();
    ptr('pointerdown', x, y); await frames();
    ptr('pointerup', x, y); await frames();
  };
  const out = [];
  const step = (what, ok, extra = '') => out.push(`${ok ? 'ok' : 'FAIL'} ${what}${ok ? '' : ': ' + extra}`);
  await frames(10);
  const KEY = 'vxr-check-voxels';
  if (stage === 'edit') {
    const before = st().voxels;
    await click(640, 430);
    const drafted = await until(() => st().current !== null && st().projects[st().current].draft);
    const s = st();
    localStorage.setItem(KEY, String(s.voxels));
    step('an edit to the island is kept as a draft before the page goes', s.voxels === before + 1 && drafted && s.unsaved, JSON.stringify([s.voxels - before, s.projects]));
  } else if (stage === 'offer') {
    await until(() => st().restore);
    const s = st();
    step('a new page offers the draft back: Restore or Discard', s.restore?.name === 'island.vox' && !s.restore.changed && s.voxels === +localStorage.getItem(KEY) - 1 && window.vxr.hit('restore-restore').length === 2, JSON.stringify([s.restore, s.voxels]));
  } else {
    await click(...window.vxr.hit('restore-restore'));
    await until(() => st().voxels === +localStorage.getItem(KEY));
    const s = st();
    step('Restore brings the edit back, still unsaved', !s.restore && s.voxels === +localStorage.getItem(KEY) && s.unsaved && s.name === 'island.vox', JSON.stringify([s.voxels, s.unsaved, s.name]));
    localStorage.removeItem(KEY);
  }
  return out.join('\n');
}
