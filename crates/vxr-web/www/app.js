// The vxr editor's page: everything on the canvas (scene and UI) is the Rust editor (pkg/,
// built by crates/vxr-web/build); this script forwards input, opens and saves files, keeps
// the projects list and its drafts, keeps remembered assistant keys and holds the WebSocket
// to vxr bridge.
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

// The projects list (crates/vxr-app/src/projects.rs) in IndexedDB: this device's id
// (device.json), each project's record (list/<id>.json), draft (drafts/<id>.vox) and
// thumbnail (thumbs/<id>.png), and its file, as a File System Access handle where the browser
// has them (Chrome, Edge) or else a copy of its bytes (files/<id>). The editor decides what to
// keep; this only reads and writes, and tells the other tabs of this page what changed.
const PROJECTS_DB = 'vxr-projects';
let projectsDb;
function projectsTx(mode, f) {
  projectsDb ??= new Promise((resolve, reject) => {
    const r = indexedDB.open(PROJECTS_DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore('p');
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
  return projectsDb.then((db) => new Promise((resolve, reject) => {
    const t = db.transaction('p', mode);
    const req = f(t.objectStore('p'));
    t.oncomplete = () => resolve(req?.result);
    t.onerror = t.onabort = () => reject(t.error);
  }));
}
const handles = () => typeof window.showOpenFilePicker === 'function' && typeof window.showSaveFilePicker === 'function';
let persisted = false;
window.vxrProjects = {
  get: (key) => projectsTx('readonly', (s) => s.get(key)),
  put(key, value) {
    // Without handles the copies are the only way back to a file: ask the browser not to
    // evict them under storage pressure (Safari and Firefox).
    if (!persisted && !handles()) {
      persisted = true;
      navigator.storage?.persist?.().catch(() => {});
    }
    return projectsTx('readwrite', (s) => s.put(value, key));
  },
  delete: (key) => projectsTx('readwrite', (s) => s.delete(key)),
  clear: () => projectsTx('readwrite', (s) => s.clear()),
  // The device id, the records and the thumbnails, which the editor reads at launch: all but
  // the drafts and the files.
  async preload() {
    let keys;
    const values = await projectsTx('readonly', (s) => {
      keys = s.getAllKeys();
      return s.getAll();
    });
    return keys.result.map((k, i) => [k, values[i]])
      .filter(([k, v]) => v instanceof Uint8Array && !k.startsWith('drafts/') && !k.startsWith('files/'));
  },
  // The project whose kept handle is to the same file as `handle`, else a new id.
  async idFor(handle) {
    const range = IDBKeyRange.bound('files/', 'files/\uffff');
    let keys;
    const values = await projectsTx('readonly', (s) => {
      keys = s.getAllKeys(range);
      return s.getAll(range);
    });
    for (const [i, v] of values.entries()) {
      if (v instanceof FileSystemFileHandle && (await v.isSameEntry(handle).catch(() => false))) return keys.result[i].slice(6);
    }
    return crypto.randomUUID();
  },
};
const warn = (what) => (e) => console.warn(`vxr: could not ${what}`, e);

// Other tabs of this page share the store. Each hears what the others changed in the list
// (records and thumbnails) on this channel, and holds the project it has open with a Web
// Lock, so another tab's Clear or storage cap leaves it alone.
const projectsChannel = typeof BroadcastChannel === 'function' ? new BroadcastChannel('vxr-projects') : null;
const listedKey = (key) => key.startsWith('list/') || key.startsWith('thumbs/');
const HOLD = 'vxr-open-';
let holding = null; // { id, release } for the lock this tab has
let wanted = null; // the project it has open, whose lock it asked for last
let heldElsewhere = new Set();
async function readHolds() {
  const q = await navigator.locks?.query?.().catch(() => null);
  if (!q) return;
  heldElsewhere = new Set(q.held.map((l) => l.name)
    .filter((n) => n.startsWith(HOLD) && n !== HOLD + holding?.id)
    .map((n) => n.slice(HOLD.length)));
}
function hold(id) {
  wanted = id;
  if (holding?.id === id) return;
  holding?.release();
  holding = null;
  projectsChannel?.postMessage({ holds: true });
  if (!id || !navigator.locks) return;
  navigator.locks.request(HOLD + id, { ifAvailable: true }, (lock) => {
    // Another tab has it open already: it holds it, and this tab does not. A lock granted
    // after this tab moved on to another project goes again at once.
    if (!lock || wanted !== id) return;
    return new Promise((release) => {
      holding = { id, release };
      projectsChannel?.postMessage({ holds: true });
    });
  }).catch(warn('hold the open project'));
}
if (projectsChannel) {
  projectsChannel.onmessage = ({ data }) => {
    if (data.holds) readHolds();
    else if (data.key && app) app.listed(data.key, data.bytes ?? undefined);
  };
}
// A tab closed lets its lock go without a word: read the locks again on coming back.
addEventListener('focus', readHolds);
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') readHolds(); });
async function allowed(handle, mode) {
  const o = { mode };
  return (await handle.queryPermission(o)) === 'granted' || (await handle.requestPermission(o)) === 'granted';
}
const VOX_TYPES = [{ description: 'MagicaVoxel scene', accept: { 'application/octet-stream': ['.vox'] } }];
async function openHandle(handle, id, picked = false) {
  if (!(await allowed(handle, 'read'))) throw new Error('the browser was not allowed to read it');
  const f = await handle.getFile();
  app.open_file(new Uint8Array(await f.arrayBuffer()), f.name, id, true, f.lastModified / 1000, picked);
}
async function pickOpen() {
  if (!handles()) return fileInput.click();
  let handle;
  try {
    [handle] = await showOpenFilePicker({ types: VOX_TYPES });
  } catch (e) {
    if (e.name !== 'AbortError') console.warn('vxr: the file picker failed', e);
    return;
  }
  const id = await window.vxrProjects.idFor(handle);
  await window.vxrProjects.put('files/' + id, handle).catch(warn('keep the file handle'));
  await openHandle(handle, id, true).catch((e) => app.open_failed(handle.name, e.message || String(e)));
}
async function saveVox(id, name, bytes) {
  try {
    let handle = await window.vxrProjects.get('files/' + id).catch(() => null);
    if (!(handle instanceof FileSystemFileHandle)) {
      handle = await showSaveFilePicker({ suggestedName: name, types: VOX_TYPES });
      await window.vxrProjects.put('files/' + id, handle);
    }
    if (!(await allowed(handle, 'readwrite'))) throw new Error('the browser was not allowed to write it');
    const w = await handle.createWritable();
    await w.write(bytes);
    await w.close();
    app.saved(handle.name);
  } catch (e) {
    app.save_failed(e.name === 'AbortError' ? '' : e.message || String(e));
  }
}
async function openProject(id, name) {
  try {
    const v = await window.vxrProjects.get('files/' + id);
    if (v instanceof FileSystemFileHandle) await openHandle(v, id);
    else if (v instanceof Uint8Array) app.open_file(v, name, id, false, 0, false);
    else app.open_failed(name, 'this browser no longer keeps its file');
  } catch (e) {
    app.open_failed(name, e.name === 'NotFoundError' ? 'the file is gone' : e.message || String(e));
  }
}

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
  agentDownloads();
  for (const m of app.bridge_out()) {
    if (m === 'connect') bridgeConnect();
    else if (m === 'close') bridgeClose();
    else if (bridge?.readyState === WebSocket.OPEN) bridge.send(m);
  }
}

function show(title, text) {
  // An error replaces the landing page and the boot skeleton.
  document.documentElement.dataset.landing = 'off';
  document.documentElement.dataset.booted = '';
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
      stepAndCopy(performance.now());
      focusIme();
    }
    return true;
  };
  canvas.addEventListener('pointercancel', (e) => finger(e, 2));
  canvas.addEventListener('touchend', focusIme);
  canvas.addEventListener('pointerdown', (e) => {
    if (finger(e, 0)) {
      if (document.activeElement !== ime) canvas.focus();
      canvas.setPointerCapture(e.pointerId);
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
    // The click lands on this frame: copy inside the gesture (Safari wants that).
    stepAndCopy(performance.now());
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

// What the editor asks of the page (the Browser platform in crates/vxr-web/src/web.rs): its
// file picker, downloads and the kept theme. The editor decides what to write and says so.
const THEME_KEY = 'vxr-theme';
// The first-run tour was skipped or finished: it shows once.
const TOURED_KEY = 'vxr-toured';
function download(bytes, name, type) {
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const a = Object.assign(document.createElement('a'), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
// The library (crates/vxr-app/src/sync.rs): the editor makes each call to the vxr Worker's
// /api (same origin, with the session cookie the account page set) and reads the answer;
// status 0 is a network failure.
async function libraryFetch(id, method, path, flat, body) {
  const headers = new Headers();
  for (let i = 0; i + 1 < flat.length; i += 2) headers.set(flat[i], flat[i + 1]);
  try {
    const r = await fetch(path, {
      method, headers, credentials: 'same-origin', cache: 'no-store',
      body: method === 'GET' || method === 'HEAD' ? undefined : body,
    });
    const bytes = new Uint8Array(await r.arrayBuffer());
    app.fetched(id, r.status, r.headers.get('x-vxr-version') ?? '', bytes);
  } catch {
    app.fetched(id, 0, '', new Uint8Array());
  }
}

// Bytes from the editor are a view of its memory, which moves: anything kept past the call
// takes a copy (slice).
window.vxrHost = {
  pickOpen,
  download,
  keepToured() {
    try { localStorage.setItem(TOURED_KEY, '1'); } catch {}
  },
  keepTheme(id) {
    try { localStorage.setItem(THEME_KEY, id); } catch {}
  },
  hasHandles: handles,
  saveVox: (id, name, bytes) => saveVox(id, name, bytes.slice()),
  openProject,
  storeGet(key) {
    window.vxrProjects.get(key).catch(() => null)
      .then((v) => app.stored(key, v instanceof Uint8Array ? v : new Uint8Array()));
  },
  storePut(key, bytes) {
    const copy = bytes.slice();
    window.vxrProjects.put(key, copy)
      .then(() => { if (listedKey(key)) projectsChannel?.postMessage({ key, bytes: copy }); })
      .catch(warn(`keep ${key}`));
  },
  storeDelete(key) {
    window.vxrProjects.delete(key)
      .then(() => { if (listedKey(key)) projectsChannel?.postMessage({ key, bytes: null }); })
      .catch(warn(`delete ${key}`));
  },
  storeHold: (id) => hold(id),
  storeHeld: (id) => heldElsewhere.has(id),
  newId: () => crypto.randomUUID(),
  fetch: (id, method, path, headers, body) => libraryFetch(id, method, path, headers, body.slice()),
  challenge: () => void challenge(),
};

// The bot check before a sign-in email: Cloudflare Turnstile, loaded the first time the account
// popover asks for an email. It runs out of sight unless it wants a press; then the editor makes
// room in the popover (hit_rect('account-challenge')) and the widget's box sits over that slot.
const TURNSTILE_SITE_KEY = '1x00000000000000000000AA'; // the build sets it (TURNSTILE_SITE_KEY)
const TURNSTILE_JS = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
const challengeBox = document.getElementById('challenge');
let turnstileReady = null;
let widget = null;
let asking = false; // the editor waits for an answer to its last ask
function turnstileJs() {
  turnstileReady ??= new Promise((resolve, reject) => {
    // The page takes script URLs only through Trusted Types: this policy passes Turnstile's alone.
    const policy = window.trustedTypes?.createPolicy('vxr-turnstile', {
      createScriptURL: (u) => { if (u !== TURNSTILE_JS) throw new TypeError(`not Turnstile: ${u}`); return u; },
    });
    const s = document.createElement('script');
    s.async = true;
    s.onload = () => (window.turnstile ? resolve(window.turnstile) : reject(new Error('no turnstile')));
    s.onerror = () => reject(new Error('turnstile did not load'));
    s.src = policy ? policy.createScriptURL(TURNSTILE_JS) : TURNSTILE_JS;
    document.head.append(s);
  });
  return turnstileReady;
}
function challenged(token) {
  challengeShown(false);
  if (!asking) return;
  asking = false;
  app.challenged(token);
}
function challengeShown(on) {
  challengeBox?.classList.toggle('shown', on);
  app.challenge_shown(on);
}
async function challenge() {
  asking = true;
  try {
    if (!challengeBox) throw new Error('the page has no #challenge box');
    const t = await turnstileJs();
    if (widget === null) {
      widget = t.render(challengeBox, {
        sitekey: TURNSTILE_SITE_KEY,
        action: 'editor',
        appearance: 'interaction-only',
        execution: 'execute',
        theme: 'auto',
        callback: (token) => challenged(token),
        'error-callback': () => { challenged(''); return true; },
        'timeout-callback': () => challenged(''),
        'before-interactive-callback': () => challengeShown(true),
        'after-interactive-callback': () => challengeShown(false),
      });
    } else {
      t.reset(widget);
    }
    t.execute(widget);
  } catch (e) {
    console.warn('vxr: the sign-in check could not load', e);
    turnstileReady = null;
    challenged('');
  }
}
// The widget's box follows its slot in the popover, which moves while the popover animates;
// the rest of the time it waits off screen (not hidden, so the check still runs).
let slot = '';
function placeChallenge() {
  if (!challengeBox) return;
  const r = challengeBox.classList.contains('shown') ? app.hit_rect('account-challenge') : [];
  const at = r.length === 4 ? `translate(${r[0]}px, ${r[1]}px)` : '';
  if (at === slot) return;
  slot = at;
  challengeBox.style.transform = at;
}

// What terminal agents exported (the editor toasts who exported what).
function agentDownloads() {
  for (let name; (name = app.agent_download_name()); ) {
    const type = name.endsWith('.zip') ? 'application/zip' : name.endsWith('.png') ? 'image/png' : 'application/octet-stream';
    download(app.take_agent_download(), name, type);
  }
}

// Text a button in the page copied (the Agents tab's commands; egui's CopyText), onto the system
// clipboard. A field's Cmd-C and Cmd-X take theirs inside the copy event instead (above), and
// find nothing here. The click that asked for it is still the user's gesture, which the
// clipboard API wants; execCommand covers a page where the API is missing.
function copyText(text) {
  // Started synchronously, so it runs inside the user's gesture.
  if (navigator.clipboard?.writeText) {
    navigator.clipboard.writeText(text).catch((e) => {
      console.warn('vxr: the clipboard API refused the copy, trying execCommand', e);
      copyWithExec(text);
    });
    return;
  }
  copyWithExec(text);
}

function copyWithExec(text) {
  const box = document.createElement('textarea');
  box.value = text;
  box.setAttribute('readonly', '');
  box.style.cssText = 'position:fixed;left:-9999px;top:0;opacity:0';
  document.body.append(box);
  box.select();
  try {
    document.execCommand('copy');
  } catch (e) {
    console.warn('vxr: could not copy', e);
  }
  box.remove();
  if (app.wants_keyboard()) focusIme();
  else canvas.focus();
}

let cursor = '';
function step(now) {
  app.frame(now / 1000);
  bridgeFlush();
  // The first drawn frame ends the boot skeleton (www/style.css).
  document.documentElement.dataset.booted = '';
}

// A frame, then whatever a button in it copied, while still inside the event that asked.
function stepAndCopy(now) {
  step(now);
  const copied = app.take_copied();
  if (copied !== undefined) copyText(copied);
}

function tick(now) {
  try {
    // An idle editor owes no frame: input, a call into the app or its own timer asks for one.
    if (app.wants_frame(now / 1000)) {
      stepAndCopy(now);
    }
    placeChallenge();
    const c = app.cursor();
    if (c !== cursor) canvas.style.cursor = cursor = c;
    // Keep the hidden field focused while egui is typing so the OS IME targets it.
    if (!fingers.size) focusIme();
    if (!app.wants_keyboard() && document.activeElement === ime) canvas.focus();
  } catch (e) {
    // A frame that throws (a Rust panic included) leaves the module unfit to run another, so
    // the loop ends here: say so instead of leaving a canvas that no longer answers.
    console.error(e);
    show('vxr stopped.', `${e} Reload the page to start again.`);
    return;
  }
  requestAnimationFrame(tick);
}

// Run `f` once the editor is in front: now, or when the landing page leaves (it sets
// html[data-landing] to off as it opens the editor).
function whenEntered(f) {
  const root = document.documentElement;
  if (root.dataset.landing !== 'on') return f();
  const seen = new MutationObserver(() => {
    if (root.dataset.landing === 'on') return;
    seen.disconnect();
    f();
  });
  seen.observe(root, { attributes: true, attributeFilter: ['data-landing'] });
}

async function main() {
  if (!navigator.gpu || !(await navigator.gpu.requestAdapter().catch(() => null))) {
    show('This browser has no WebGPU.', 'vxr needs WebGPU: try a recent Chrome, Edge or Safari on a desktop, or download the desktop app when it ships.');
    return;
  }
  const get = async (u) => {
    const r = await fetch(u);
    if (!r.ok) throw new Error(`${u}: ${r.status}`);
    return new Uint8Array(await r.arrayBuffer());
  };
  const font = (n) => get('fonts/' + n).catch(() => new Uint8Array());
  // The scene and fonts do not need the module: fetch them alongside the wasm, not after it.
  const assets = Promise.all([
    get('scene.vox'),
    font('Geist-Regular.ttf'), font('Geist-Medium.ttf'), font('Geist-SemiBold.ttf'),
    font('GeistMono-Regular.ttf'), font('GeistMono-Medium.ttf'), font('GeistMono-UltraBlack.ttf'),
    font('Inter-Regular.ttf'), font('lucide.ttf'),
  ]);
  assets.catch(() => {}); // if init() throws first, its error is the one shown
  await init();
  fit();
  const [vox, ...fonts] = await assets;
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
  for (const [key, bytes] of await window.vxrProjects.preload().catch(warn('read the projects list')) ?? []) app.preload(key, bytes);
  await readHolds();
  app.launch();
  // The first-run tour, once, and never for a link to a scene or project or an embed. It starts
  // when the editor is in front: at once, or when the landing page opens it.
  let toured = false;
  try { toured = localStorage.getItem(TOURED_KEY) === '1'; } catch {}
  app.set_tour_seen(toured);
  if (document.documentElement.dataset.deep !== 'on') whenEntered(() => app.first_tour());
  app.check_account();
  // Edits not yet in a draft are written as the page goes or hides; leaving with unsaved
  // edits asks first (the draft stays either way, as after a crash).
  addEventListener('pagehide', () => app.flush());
  // A sign-in link opened in another tab (the account page) says so here: who is signed in now.
  if (typeof BroadcastChannel === 'function') new BroadcastChannel('vxr-account').onmessage = () => app.check_account();
  // The landing page's Sign in opens the account popover once the editor is in.
  addEventListener('vxr-sign-in', () => app.open_account());
  // Back from the account page or another tab: who is signed in, and what the library has now.
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') app.flush();
    else app.check_account();
  });
  addEventListener('beforeunload', (e) => {
    if (!app.unsaved()) return;
    app.flush();
    e.preventDefault();
    e.returnValue = '';
  });
  msg.hidden = true;
  wire();
  canvas.focus();
  step(performance.now());
  // landing.js waits for this to open the editor; a visit that skipped the landing counts as
  // having opened it, so the next one does too (www/gate.js).
  document.documentElement.dataset.ready = '';
  if (document.documentElement.dataset.landing === 'off') {
    try { localStorage.setItem('vxr-visited', '1'); } catch {}
  }
  dispatchEvent(new Event('vxr-ready'));
  requestAnimationFrame(tick);
}
main().catch((e) => { console.error(e); show('vxr could not start.', String(e)); });
