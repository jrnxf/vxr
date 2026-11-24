async (stage) => {
  // The embed in a real page (crates/vxr-embed/check opens check.html): one stage per call,
  // so the check can take a screenshot between them.
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms = 15000) => {
    for (let t = 0; t < ms && !f(); t += 50) await sleep(50);
    return !!f();
  };
  const out = [];
  const step = (what, ok, extra = '') => out.push(`${ok ? 'ok' : 'FAIL'} embed ${what}${ok ? '' : ': ' + extra}`);
  // Where the browser cannot show a WebGPU canvas (headless Chrome on a CI runner with a software
  // GPU: the device works and the viewer runs, but the canvas reads back empty), the steps that
  // read pixels print "skip" and everything else still runs. crates/vxr-embed/check sets it.
  const pixels = !window.__vxrNoPixels;
  const seen = (what, ok, extra = '') => (pixels ? step(what, ok, extra) : out.push(`skip embed ${what} (no pixels to read here)`));
  const $ = (id) => document.getElementById(id);
  const live = (el) => el.dataset.live !== undefined;
  // The picture an element draws now, as RGBA bytes.
  const grab = (el) => {
    el.draw();
    const c = new OffscreenCanvas(el.canvas.width, el.canvas.height);
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(el.canvas, 0, 0);
    return g.getImageData(0, 0, c.width, c.height).data;
  };
  // Pixels that are not the sky (every sky channel is 198 or more), and their colours.
  const scene = (px) => {
    const colours = new Set();
    let n = 0;
    for (let i = 0; i < px.length; i += 4) {
      if (Math.min(px[i], px[i + 1], px[i + 2]) < 190) {
        n++;
        colours.add((px[i] << 16) | (px[i + 1] << 8) | px[i + 2]);
      }
    }
    return { share: n / (px.length / 4), colours: colours.size };
  };
  const differ = (a, b) => {
    if (a.length !== b.length) return -1;
    let n = 0;
    for (let i = 0; i < a.length; i += 4) if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]) n++;
    return n;
  };
  const wasms = () => performance.getEntriesByType('resource').filter((e) => e.name.endsWith('vxr_embed_bg.wasm')).length;
  const [a, b, c] = [$('bricks'), $('vox'), $('cut')];

  if (stage === 'first') {
    step('a brick scene file goes live on screen', await until(() => live(a) || a.t.error), a.t.error);
    const s = scene(grab(a));
    seen('the first frame shows lit voxels', s.share > 0.03 && s.colours >= 4, JSON.stringify(s));
    step('the picture is the element at up to 2x', a.canvas.width === Math.round(a.getBoundingClientRect().width * Math.min(devicePixelRatio, 2)), String(a.canvas.width));
    await sleep(400);
    step('the poster fades out under the live scene', getComputedStyle(a.querySelector('img')).opacity === '0');
    step('the tags below have not started', !b.started && !c.started && wasms() === 1, `${b.started} ${c.started} ${wasms()}`);
    step('the tag is an image with its alt text', a.getAttribute('role') === 'img' && a.getAttribute('aria-label') === 'A small voxel world');
    // The page keeps its scroll: no wheel handler, and touches pan the page up and down.
    const wheel = new WheelEvent('wheel', { deltaY: 120, bubbles: true, cancelable: true, composed: true });
    a.canvas.dispatchEvent(wheel);
    step('the wheel is left to the page', !wheel.defaultPrevented && getComputedStyle(a.canvas).touchAction === 'pan-y');
    const before = grab(a);
    const yaw = a.yaw;
    const r = a.getBoundingClientRect();
    const ptr = (type, x) => a.canvas.dispatchEvent(new PointerEvent(type, {
      clientX: r.left + x, clientY: r.top + 100, button: 0, buttons: type === 'pointerup' ? 0 : 1, pointerId: 7, isPrimary: true, bubbles: true,
    }));
    ptr('pointerdown', 100); ptr('pointermove', 200); ptr('pointerup', 200);
    const turned = grab(a);
    step('a drag turns the scene', Math.abs(a.yaw - (yaw - 0.8)) < 1e-6 && (!pixels || differ(before, turned) > 500), `${a.yaw} ${differ(before, turned)}`);
    const key = new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true });
    a.dispatchEvent(key);
    step('the arrow keys turn it and it takes the focus', key.defaultPrevented && Math.abs(a.yaw - (yaw - 0.95)) < 1e-6 && a.tabIndex === 0, String(a.yaw));
    await sleep(200);
    const idle = a.frames;
    await sleep(300);
    step('a still scene draws nothing more', a.frames === idle, `${idle} ${a.frames}`);
  }

  if (stage === 'vox') {
    b.scrollIntoView();
    step('a .vox file goes live when scrolled to', await until(() => live(b) || b.t.error), b.t.error);
    step('one wasm serves every tag', wasms() === 1, String(wasms()));
    const n = b.frames;
    await sleep(400);
    step('autorotate draws while on screen', b.frames > n + 5, `${n} ${b.frames}`);
    scrollTo(0, 0);
    await sleep(300);
    const off = b.frames;
    await sleep(400);
    step('and nothing while off screen', b.frames === off, `${off} ${b.frames}`);
    // The .vox fallback and the brick scene file of the same scene are the same picture.
    b.removeAttribute('autorotate');
    [b.yaw, b.pitch] = [a.yaw, a.pitch];
    const d = differ(grab(a), grab(b));
    seen('the .vox file and its brick scene file draw the same picture', d === 0, String(d));
  }

  if (stage === 'fail') {
    c.scrollIntoView();
    step('a damaged file is refused with the reason', (await until(() => c.t.error)) && c.t.error === 'the brick scene file is cut short', c.t.error);
    const note = c.shadowRoot.querySelector('p');
    step('its poster stays, with a note', !live(c) && getComputedStyle(c.shadowRoot.querySelector('img')).opacity === '1'
      && note.textContent === 'The live scene could not load, so it shows as a picture.' && note.getBoundingClientRect().height > 10, note.textContent);
    const gone = document.createElement('voxel-scene');
    gone.setAttribute('src', 'missing.vxrb');
    c.after(gone);
    step('a missing file says so', (await until(() => gone.t.error)) && gone.t.error === 'the scene file answered 404', gone.t.error);
    gone.remove();
    // A browser without WebGPU: the poster and a note, and nothing is fetched.
    const gpu = Object.getOwnPropertyDescriptor(Navigator.prototype, 'gpu');
    Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined, configurable: true });
    const plain = document.createElement('voxel-scene');
    plain.setAttribute('src', 'world.vxrb');
    plain.setAttribute('poster', c.getAttribute('poster'));
    c.after(plain);
    await sleep(300);
    Object.defineProperty(Navigator.prototype, 'gpu', gpu);
    step('without WebGPU the poster stays with a note and nothing loads', plain.t.noWebGPU && !plain.started && !live(plain)
      && plain.shadowRoot.querySelector('p').textContent === 'This browser has no WebGPU, so the scene shows as a picture.'
      && plain.shadowRoot.querySelector('img').complete);
    plain.scrollIntoView({ block: 'center' });
  }
  return out.join('\n');
}
