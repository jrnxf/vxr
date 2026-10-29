// The vxr editor's page: everything on the canvas (scene and UI) is the Rust editor (pkg/,
// built by crates/vxr-web/build); this script forwards input, opens files, downloads saves,
// keeps remembered assistant keys and holds the WebSocket to vxr bridge.
import init, { start } from './pkg/vxr_web.js';

const canvas = document.getElementById('c');
const ime = document.getElementById('ime');
const fileInput = document.getElementById('file');
const msg = document.getElementById('msg');
const dpr = () => devicePixelRatio || 1;
let app, composing = false;
// The fingers down on the canvas. While one is, only the touch handlers focus the hidden
// field, since iPadOS opens its keyboard only for a focus inside the tap.
const fingers = new Set();
const focusIme = () => { if (app.wants_keyboard() && document.activeElement !== ime) ime.focus(); };

// The assistant's keys, when the user asks this browser to remember one: encrypted with an
// AES-GCM key that never leaves the browser (non-extractable, kept in IndexedDB), so what is
// stored is never the key in plain text. Code running on this page can still use it, as the
// settings say. By default keys stay in the tab's memory and nothing is stored.
const KEYS_DB = 'vxr-keys';
function keysDb() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open(KEYS_DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore('k');
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}
async function keysTx(mode, f) {
  const db = await keysDb();
  return new Promise((resolve, reject) => {
    const t = db.transaction('k', mode);
    const req = f(t.objectStore('k'));
    t.oncomplete = () => { db.close(); resolve(req?.result); };
    t.onerror = () => { db.close(); reject(t.error); };
  });
}
async function wrapKey() {
  let k = await keysTx('readonly', (s) => s.get('wrap'));
  if (!k) {
    k = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
    await keysTx('readwrite', (s) => s.put(k, 'wrap'));
  }
  return k;
}
window.vxrKeys = {
  async remember(kind, key) {
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, await wrapKey(), new TextEncoder().encode(key));
    await keysTx('readwrite', (s) => s.put({ iv, ct }, 'key:' + kind));
  },
  async forget(kind) {
    await keysTx('readwrite', (s) => s.delete('key:' + kind));
  },
  async load() {
    const out = [];
    for (const kind of ['anthropic', 'openai', 'compatible']) {
      const v = await keysTx('readonly', (s) => s.get('key:' + kind)).catch(() => null);
      if (!v) continue;
      try {
        const pt = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: v.iv }, await wrapKey(), v.ct);
        out.push([kind, new TextDecoder().decode(pt)]);
      } catch (e) {
        console.warn(`vxr: a remembered ${kind} key could not be read`, e);
      }
    }
    return out;
  },
};

// Terminal agents through vxr bridge on this computer (crates/vxr-mcp/src/bridge.rs): the
// editor says when to connect and what to send (app.bridge_out), and hears what happened. A
// browser tells a page nothing about why a WebSocket failed, so the reason is guessed from
// the local network permission and the browser.
let bridge = null, bridgeGen = 0;
const isSafari = () => /safari/i.test(navigator.userAgent) && !/chrome|chromium|crios|fxios|edg|android/i.test(navigator.userAgent);
async function bridgeDenied() {
  // Chrome 145 and later call the permission loopback-network, earlier ones local-network-access.
  for (const name of ['loopback-network', 'local-network-access', 'local-network']) {
    try {
      if ((await navigator.permissions.query({ name })).state === 'denied') return true;
    } catch {}
  }
  return false;
}
function bridgeClose() {
  const ws = bridge;
  bridge = null;
  ws?.close();
}
function bridgeConnect() {
  bridgeClose();
  const gen = ++bridgeGen;
  let ws;
  try {
    ws = new WebSocket(app.bridge_url());
  } catch (e) {
    console.warn('vxr: the browser would not open a WebSocket to vxr bridge', e);
    app.bridge_closed('blocked');
    return;
  }
  let opened = false;
  bridge = ws;
  ws.onopen = () => {
    if (bridge !== ws) return;
    opened = true;
    app.bridge_open();
    bridgeFlush();
  };
  // The editor runs an agent's request at once, so a tab the browser draws less often (behind
  // the terminal) still answers.
  ws.onmessage = (e) => {
    if (bridge !== ws || typeof e.data !== 'string') return;
    app.bridge_message(e.data);
    bridgeFlush();
  };
  ws.onclose = async () => {
    if (bridge !== ws) return;
    bridge = null;
    let why = '';
    if (!opened) why = (await bridgeDenied()) ? 'denied' : navigator.brave ? 'brave' : isSafari() ? 'safari' : '';
    if (gen === bridgeGen) app.bridge_closed(why);
  };
}
function bridgeFlush() {
  for (const m of app.bridge_out()) {
    if (m === 'connect') bridgeConnect();
    else if (m === 'close') bridgeClose();
    else if (bridge?.readyState === WebSocket.OPEN) bridge.send(m);
  }
}

function show(title, text) {
  msg.hidden = false;
  const p = document.createElement('p');
  const s = document.createElement('strong');
  s.textContent = title;
  p.append(s, text);
  msg.replaceChildren(p);
}

function fit() {
  canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr()));
  canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr()));
  app?.resize(canvas.width, canvas.height, dpr());
}

const mods = (e) => app.set_modifiers(e.shiftKey, e.altKey, e.ctrlKey, e.metaKey);

function wire() {
  canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  // Fingers go to the editor's touch gestures (app.touch); a mouse press brings back the
  // mouse layout, and a pen draws like the mouse in whichever layout shows.
  const finger = (e, phase) => {
    if (e.pointerType !== 'touch') return false;
    if (phase === 0) fingers.add(e.pointerId);
    else if (!fingers.has(e.pointerId)) return true;
    if (phase === 2) fingers.delete(e.pointerId);
    app.touch(e.pointerId, phase, e.offsetX, e.offsetY);
    e.preventDefault();
    if (phase === 2) {
      step(performance.now());
      focusIme();
    }
    return true;
  };
  canvas.addEventListener('pointercancel', (e) => finger(e, 2));
  canvas.addEventListener('touchend', focusIme);
  canvas.addEventListener('pointerdown', (e) => {
    if (finger(e, 0)) {
      canvas.setPointerCapture(e.pointerId);
      canvas.focus();
      return;
    }
    if (e.pointerType === 'mouse') app.set_touch(false);
    // A right click on the focused text field lands on the hidden field put under the pointer,
    // so the browser opens its own menu there and its Paste fires the paste event below.
    if (e.button === 2 && app.over_field(e.offsetX, e.offsetY)) {
      Object.assign(ime.style, { left: `${e.clientX - 8}px`, top: `${e.clientY - 8}px`, width: '16px', height: '16px' });
      ime.focus();
      e.preventDefault();
      addEventListener('contextmenu', () => setTimeout(() => ime.removeAttribute('style')), { once: true, capture: true });
      return;
    }
    canvas.setPointerCapture(e.pointerId);
    canvas.focus();
    mods(e);
    app.pointer_move(e.offsetX, e.offsetY);
    app.pointer_button(e.offsetX, e.offsetY, e.button, true);
    e.preventDefault();
  });
  canvas.addEventListener('pointermove', (e) => {
    if (finger(e, 1)) return;
    mods(e);
    app.pointer_move(e.offsetX, e.offsetY);
    // A second button pressed or released during a drag (a chord) comes as a pointermove.
    if (e.button >= 0) app.pointer_button(e.offsetX, e.offsetY, e.button, (e.buttons & [1, 4, 2][e.button]) !== 0);
  });
  canvas.addEventListener('pointerup', (e) => {
    if (finger(e, 2)) return;
    mods(e);
    app.pointer_button(e.offsetX, e.offsetY, e.button, false);
  });
  canvas.addEventListener('pointerleave', (e) => { if (e.pointerType !== 'touch') app.pointer_leave(); });
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    const k = e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? 400 : 1;
    // A trackpad pinch comes as a wheel with Ctrl held (Chrome, Firefox): zoom, not the page.
    if (e.ctrlKey && !e.metaKey) app.pinch(Math.exp(-e.deltaY * k * 0.01) - 1);
    else app.wheel(e.deltaX * k, e.deltaY * k);
  }, { passive: false });
  // Safari sends a trackpad pinch as gesture events instead. On a touch screen it sends them
  // for fingers too, which the touch gestures already take.
  let pinchScale = 1;
  canvas.addEventListener('gesturestart', (e) => { e.preventDefault(); pinchScale = 1; });
  canvas.addEventListener('gesturechange', (e) => {
    e.preventDefault();
    if (fingers.size) return;
    app.pinch(e.scale / pinchScale - 1);
    pinchScale = e.scale;
  });
  canvas.addEventListener('gestureend', (e) => e.preventDefault());
  const key = (e, down) => {
    if (composing || e.isComposing) return;
    mods(e);
    // A browser fires copy and cut only with a selection, so the hidden field gets one.
    if (down && (e.metaKey || e.ctrlKey) && ['c', 'x'].includes(e.key.toLowerCase()) && app.wants_keyboard()) {
      ime.value = ' ';
      ime.select();
    }
    if (app.key(e.key, down, e.repeat)) e.preventDefault();
    if ((e.metaKey || e.ctrlKey) && ['k', 'j', 'z', 'n', 'o', 's', 'd', 'g', '=', '+', '-'].includes(e.key.toLowerCase())) e.preventDefault();
  };
  addEventListener('keydown', (e) => key(e, true));
  addEventListener('keyup', (e) => key(e, false));
  addEventListener('blur', () => app.blur());
  // While a text field has focus Cmd-C, Cmd-X and Cmd-V keep their default action, which
  // fires these events (app.key leaves them unprevented).
  addEventListener('paste', (e) => {
    if (!app.wants_keyboard()) return;
    e.preventDefault();
    app.paste(e.clipboardData.getData('text'));
  });
  // The copied text goes on the clipboard inside the event (a later write can be refused), so
  // egui takes the copy or cut in a frame drawn now.
  for (const type of ['copy', 'cut']) {
    addEventListener(type, (e) => {
      ime.value = '';
      if (!app.wants_keyboard()) return;
      e.preventDefault();
      app.copy(type === 'cut');
      step(performance.now());
      const text = app.take_copied();
      if (text !== undefined) e.clipboardData.setData('text/plain', text);
    });
  }
  ime.addEventListener('compositionstart', () => { composing = true; });
  ime.addEventListener('compositionupdate', (e) => app.ime(e.data, false));
  ime.addEventListener('compositionend', (e) => { composing = false; app.ime(e.data, true); ime.value = ''; });
  ime.addEventListener('input', () => { if (!composing) ime.value = ''; });
  fileInput.addEventListener('change', async () => {
    const f = fileInput.files[0];
    fileInput.value = '';
    if (f) app.open(new Uint8Array(await f.arrayBuffer()), f.name);
  });
  new ResizeObserver(fit).observe(canvas);
}

function download(bytes, name, type) {
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const a = Object.assign(document.createElement('a'), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  app.toast(`Downloaded ${name}`);
}

function save() {
  download(app.save(), /\.vox$/i.test(app.name()) ? app.name() : 'scene.vox', 'application/octet-stream');
}

const THEME_KEY = 'vxr-theme';

let cursor = '';
function step(now) {
  for (const a of app.frame(now / 1000)) {
    if (a === 'open') fileInput.click();
    else if (a === 'save') save();
    else if (a === 'embed') app.toast('The embed export is not built yet');
    else if (a === 'obj') {
      const zip = app.export_obj();
      if (zip.length) download(zip, app.obj_name(), 'application/zip');
    }
    else if (a === 'slices') {
      const png = app.export_slices();
      if (png.length) download(png, app.slices_name(), 'image/png');
    }
    else if (a === 'exported') download(app.take_export(), app.export_name(), 'image/png');
    else if (a === 'theme') {
      try { localStorage.setItem(THEME_KEY, app.theme()); } catch {}
    }
  }
  bridgeFlush();
}

function tick(now) {
  step(now);
  const c = app.cursor();
  if (c !== cursor) canvas.style.cursor = cursor = c;
  // Keep the hidden field focused while egui is typing so the OS IME targets it.
  if (!fingers.size) focusIme();
  if (!app.wants_keyboard() && document.activeElement === ime) canvas.focus();
  requestAnimationFrame(tick);
}

async function main() {
  if (!navigator.gpu || !(await navigator.gpu.requestAdapter().catch(() => null))) {
    show('This browser has no WebGPU.', 'vxr needs WebGPU: try a recent Chrome, Edge or Safari on a desktop, or download the desktop app when it ships.');
    return;
  }
  await init();
  fit();
  const get = async (u) => {
    const r = await fetch(u);
    if (!r.ok) throw new Error(`${u}: ${r.status}`);
    return new Uint8Array(await r.arrayBuffer());
  };
  const font = (n) => get('fonts/' + n).catch(() => new Uint8Array());
  const [vox, ...fonts] = await Promise.all([
    get('scene.vox'),
    font('Geist-Regular.ttf'), font('Geist-Medium.ttf'), font('Geist-SemiBold.ttf'),
    font('GeistMono-Regular.ttf'), font('GeistMono-Medium.ttf'), font('Inter-Regular.ttf'), font('lucide.ttf'),
  ]);
  app = await start(canvas, vox, 'island.vox', dpr(), ...fonts);
  window.vxr = app; // for the console and crates/vxr-web/check
  // The System theme follows the page's light or dark setting; a picked theme is kept here.
  const dark = matchMedia('(prefers-color-scheme: dark)');
  app.set_system_dark(dark.matches);
  dark.addEventListener('change', (e) => app.set_system_dark(e.matches));
  try { app.set_theme(localStorage.getItem(THEME_KEY) ?? 'system'); } catch {}
  // A touch-first screen (a tablet) starts in the touch layout.
  if (matchMedia('(pointer: coarse)').matches) app.set_touch(true);
  for (const [kind, key] of await window.vxrKeys.load().catch(() => [])) app.restore_key(kind, key);
  msg.hidden = true;
  wire();
  canvas.focus();
  requestAnimationFrame(tick);
}
main().catch((e) => { console.error(e); show('vxr could not start.', String(e)); });
