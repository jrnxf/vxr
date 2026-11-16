async () => {
  // The embed folder the browser editor exported (check.js handed its six files to the check's
  // server), opened as its own page from a plain host: crates/vxr-web/check opens
  // /__embed/island-embed/ and runs this in it.
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms = 20000) => {
    for (let t = 0; t < ms && !f(); t += 50) await sleep(50);
    return !!f();
  };
  const out = [];
  const step = (what, ok, extra = '') => out.push(`${ok ? 'ok' : 'FAIL'} embed page ${what}${ok ? '' : ': ' + extra}`);
  const el = document.querySelector('voxel-scene');
  if (!el) return 'FAIL embed page has a voxel-scene tag: ' + document.title;
  const img = el.querySelector('img');
  const tag = '<script src="voxel-scene.js" async></script>\n<voxel-scene src="island.vxrb" alt="island">\n  <img src="island.png" alt="">\n</voxel-scene>';
  step('shows the scene, the tag to paste and no file note', document.title === 'island' && document.querySelector('pre').textContent === tag
    && document.getElementById('file').hidden && el.getBoundingClientRect().width > 300, document.querySelector('pre').textContent);
  step('the exported viewer takes the exported scene live', (await until(() => el.dataset.live !== undefined || el.t?.error)) && !el.t.error, String(el.t?.error));

  // Which cells of a 320x200 grid hold the scene rather than the background. The background
  // is whatever the picture's left edge shows at that height (the poster's dark backdrop,
  // the viewer's sky).
  const [gw, gh] = [320, 200];
  const mask = (source) => {
    const c = new OffscreenCanvas(gw, gh);
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(source, 0, 0, gw, gh);
    const px = g.getImageData(0, 0, gw, gh).data;
    const m = new Uint8Array(gw * gh);
    const colours = new Set();
    for (let y = 0; y < gh; y++) {
      for (let x = 0; x < gw; x++) {
        const [i, e] = [(y * gw + x) * 4, y * gw * 4];
        if ([0, 1, 2].some((k) => Math.abs(px[i + k] - px[e + k]) > 24)) {
          m[y * gw + x] = 1;
          colours.add(((px[i] >> 4) << 8) | ((px[i + 1] >> 4) << 4) | (px[i + 2] >> 4));
        }
      }
    }
    return { m, share: m.reduce((a, b) => a + b, 0) / m.length, colours: colours.size };
  };
  const box = ({ m }) => {
    let [x0, y0, x1, y1] = [gw, gh, -1, -1];
    m.forEach((v, i) => {
      if (v) [x0, y0, x1, y1] = [Math.min(x0, i % gw), Math.min(y0, (i / gw) | 0), Math.max(x1, i % gw), Math.max(y1, (i / gw) | 0)];
    });
    return [x0, y0, x1, y1];
  };
  await until(() => img.complete && img.naturalWidth > 0, 5000);
  const poster = mask(img);
  // The first frame, before anything turns the scene.
  el.draw();
  const live = mask(el.canvas);
  step('its first frame shows lit voxels', live.share > 0.05 && live.colours >= 8 && el.yaw === 0.7 && el.pitch === 0.5, `${live.share} ${live.colours} ${el.yaw} ${el.pitch}`);
  // The poster is that first frame, path-traced: the same view, so the scene covers the same
  // part of the picture and nothing jumps when the live scene fades in.
  let [both, either] = [0, 0];
  for (let i = 0; i < gw * gh; i++) {
    both += poster.m[i] & live.m[i];
    either += poster.m[i] | live.m[i];
  }
  const [pb, lb] = [box(poster), box(live)];
  const off = Math.max(...pb.map((v, i) => Math.abs(v - lb[i])));
  step('the poster frames the scene as the first live frame does', img.naturalWidth === 1280 && img.naturalHeight === 800 && poster.share > 0.05
    && off <= 2 && both / either > 0.975, `poster ${pb} live ${lb} overlap ${(both / either).toFixed(3)} poster share ${poster.share.toFixed(3)}`);
  await sleep(500);
  step('the poster fades out under the live scene', getComputedStyle(img).opacity === '0', getComputedStyle(img).opacity);
  return out.join('\n');
}
