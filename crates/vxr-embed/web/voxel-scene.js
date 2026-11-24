// <voxel-scene src="scene.vxrb" alt="A voxel island" autorotate>
//   <img src="scene.png" alt="">
// </voxel-scene>
//
// One tag for a vxr scene. The poster shows at once: an <img> child (best, the browser
// fetches it while it still parses the page) or a `poster` attribute. The viewer (the wasm
// beside this script) loads only when the element first comes on screen, draws only while it
// is on screen, and every element on a page shares one GPU device. Drag or the arrow keys
// turn the scene; the wheel is left alone, so the page keeps its scroll. `src` is a brick
// scene file or a .vox file. Without WebGPU, or when anything fails, the poster stays and a
// short note says why.
//
// Events, each with `detail` holding times in ms and, on failure, `error`:
// `vxr-frame` (the first frame is on screen), `vxr-nowebgpu`, `vxr-error`.
//
// Third-party notices: licenses.txt, beside this file. Keep it with the viewer's files.
(() => {
  const BASE = new URL('.', document.currentScript ? document.currentScript.src : location.href);
  const now = () => performance.now();
  let engine = null;

  // The glue, the wasm and the device, once per page.
  function loadEngine() {
    if (!engine) {
      engine = (async () => {
        const wasm = WebAssembly.compileStreaming(fetch(new URL('vxr_embed_bg.wasm', BASE)));
        const glue = await import(new URL('vxr_embed.js', BASE).href);
        await glue.default({ module_or_path: await wasm });
        const err = await glue.gpu_init();
        if (err) throw new Error(err);
        return glue;
      })();
    }
    return engine;
  }

  const STYLE =
    ':host{display:block;position:relative;aspect-ratio:16/10;overflow:hidden;border-radius:12px;background:#e7e4dc;contain:content}' +
    ':host(:focus-visible){outline:2px solid #2f6fed;outline-offset:2px}' +
    'canvas,img,::slotted(img){position:absolute;inset:0;width:100%;height:100%;display:block}' +
    'img,::slotted(img){object-fit:cover;transition:opacity .25s}' +
    'canvas{touch-action:pan-y;cursor:grab}' +
    ':host([data-live]) img,:host([data-live]) ::slotted(img){opacity:0;pointer-events:none}' +
    'p{position:absolute;left:8px;bottom:8px;max-width:calc(100% - 16px);box-sizing:border-box;margin:0;padding:4px 8px;' +
    'border-radius:6px;background:rgba(20,20,20,.72);color:#fff;font:12px/1.4 system-ui,sans-serif}' +
    'p:empty{display:none}' +
    '@media (prefers-reduced-motion:reduce){img,::slotted(img){transition:none}}';

  class VoxelScene extends HTMLElement {
    constructor() {
      super();
      this.t = { created: now() };
      const root = this.attachShadow({ mode: 'open' });
      root.innerHTML = '<style>' + STYLE + '</style><canvas></canvas><img alt=""><slot></slot><p role="status"></p>';
      this.canvas = root.querySelector('canvas');
      this.img = root.querySelector('img');
      this.note = root.querySelector('p');
      // The first view. The app traces its poster from the same one (VIEW_YAW and VIEW_PITCH
      // in crates/vxr-core/src/bricks.rs), so the live scene takes over in place.
      this.yaw = 0.7;
      this.pitch = 0.5;
      this.zoom = 1;
    }

    connectedCallback() {
      this.setAttribute('role', 'img');
      if (this.hasAttribute('alt')) this.setAttribute('aria-label', this.getAttribute('alt'));
      const poster = this.getAttribute('poster');
      if (this.querySelector('img')) this.img.remove();
      else if (poster && !this.img.src) this.img.src = poster;
      else if (!poster) this.img.remove();
      if (!navigator.gpu) {
        this.t.noWebGPU = true;
        this.note.textContent = 'This browser has no WebGPU, so the scene shows as a picture.';
        this.emit('nowebgpu');
        return;
      }
      this.io = new IntersectionObserver((es) => {
        this.visible = es[es.length - 1].isIntersecting;
        if (!this.visible) return;
        if (!this.started) this.start();
        else this.kick();
      });
      this.io.observe(this);
    }

    disconnectedCallback() {
      if (this.io) this.io.disconnect();
      if (this.ro) this.ro.disconnect();
      this.visible = false;
    }

    emit(type) {
      this.dispatchEvent(new CustomEvent('vxr-' + type, { detail: this.t }));
    }

    // The poster comes back, with why.
    fail(e) {
      this.viewer = null;
      delete this.dataset.live;
      this.t.error = String(e && e.message ? e.message : e);
      this.note.textContent = 'The live scene could not load, so it shows as a picture.';
      console.warn('voxel-scene: ' + this.t.error);
      this.emit('error');
    }

    async start() {
      this.started = true;
      const t = this.t;
      t.visible = now();
      try {
        const file = fetch(this.getAttribute('src')).then((r) => {
          if (!r.ok) throw new Error('the scene file answered ' + r.status);
          return r.arrayBuffer();
        });
        const glue = await loadEngine();
        t.engine = now();
        const viewer = new glue.Viewer(this.canvas);
        const err = viewer.load(new Uint8Array(await file));
        if (err) throw new Error(err);
        t.loaded = now();
        this.viewer = viewer;
        if (!this.draw()) throw new Error('the first frame did not draw');
        await glue.gpu_done();
        this.ro = new ResizeObserver(() => this.kick());
        this.ro.observe(this);
        this.setupInput();
        requestAnimationFrame(() => {
          if (!this.viewer) return;
          t.frame = now();
          this.dataset.live = '';
          this.emit('frame');
          this.loop();
        });
      } catch (e) {
        this.fail(e);
      }
    }

    draw() {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const r = this.getBoundingClientRect();
      const w = Math.max(1, Math.round(r.width * dpr));
      const h = Math.max(1, Math.round(r.height * dpr));
      if (this.canvas.width !== w || this.canvas.height !== h) {
        this.canvas.width = w;
        this.canvas.height = h;
      }
      this.frames = (this.frames || 0) + 1;
      return this.viewer.render(this.yaw, this.pitch, this.zoom, w, h);
    }

    loop() {
      if (this.raf || !this.viewer) return;
      const tick = (ts) => {
        this.raf = 0;
        if (!this.visible || !this.viewer) return;
        const spin = this.hasAttribute('autorotate') && !this.drag && !matchMedia('(prefers-reduced-motion:reduce)').matches;
        if (spin) {
          this.yaw += Math.min(ts - (this.last || ts), 50) * 0.00025;
          this.dirty = true;
        }
        this.last = ts;
        if (this.dirty) {
          this.dirty = false;
          if (!this.draw()) return this.fail('the GPU device failed');
        }
        if (spin || this.dirty) this.raf = requestAnimationFrame(tick);
      };
      this.raf = requestAnimationFrame(tick);
    }

    kick() {
      this.dirty = true;
      this.loop();
    }

    turn(dx, dy) {
      this.yaw -= dx;
      this.pitch = Math.max(-1.2, Math.min(1.45, this.pitch + dy));
      this.kick();
    }

    setupInput() {
      const c = this.canvas;
      c.addEventListener('pointerdown', (e) => {
        if (e.button !== 0) return;
        this.drag = { x: e.clientX, y: e.clientY, id: e.pointerId };
        c.setPointerCapture(e.pointerId);
        c.style.cursor = 'grabbing';
      });
      c.addEventListener('pointermove', (e) => {
        const d = this.drag;
        if (!d || d.id !== e.pointerId) return;
        this.turn((e.clientX - d.x) * 0.008, (e.clientY - d.y) * 0.008);
        d.x = e.clientX;
        d.y = e.clientY;
      });
      const up = (e) => {
        if (this.drag && this.drag.id === e.pointerId) {
          this.drag = null;
          c.style.cursor = '';
          this.last = 0;
          this.kick();
        }
      };
      c.addEventListener('pointerup', up);
      c.addEventListener('pointercancel', up);
      // The arrow keys turn the scene while it has the focus.
      if (!this.hasAttribute('tabindex')) this.tabIndex = 0;
      const keys = { ArrowLeft: [0.15, 0], ArrowRight: [-0.15, 0], ArrowUp: [0, 0.1], ArrowDown: [0, -0.1] };
      this.addEventListener('keydown', (e) => {
        const k = keys[e.key];
        if (!k || e.altKey || e.ctrlKey || e.metaKey) return;
        e.preventDefault();
        this.turn(k[0], k[1]);
      });
    }
  }

  if (!customElements.get('voxel-scene')) customElements.define('voxel-scene', VoxelScene);
})();
