// <voxel-scene src="scene.vox" poster="scene.webp" alt="..." autorotate></voxel-scene>
// One tag. The poster shows at once. The wasm viewer loads only when the element is on screen,
// renders only while on screen, and every element on the page shares one GPU device.
// Without WebGPU, or if anything fails, the poster stays.
(() => {
  const BASE = new URL('.', document.currentScript ? document.currentScript.src : location.href);
  const now = () => performance.now();
  let engine = null;

  // Glue + wasm + device, once per page. Times go into the first element's `t`.
  function loadEngine(t, bench) {
    if (!engine) {
      engine = (async () => {
        t.engineStart = now();
        const resp = fetch(new URL('pkg/vxr_embed_spike_bg.wasm', BASE));
        if (bench) resp.then((r) => (t.wasmCopy = r.clone()));
        const modP = WebAssembly.compileStreaming(resp);
        const glue = await import(new URL('pkg/vxr_embed_spike.js', BASE).href);
        t.glueLoaded = now();
        const mod = await modP;
        t.wasmCompiled = now();
        await glue.default({ module_or_path: mod });
        t.wasmInstantiated = now();
        const err = await glue.gpu_init();
        t.deviceReady = now();
        if (err) throw new Error('gpu init ' + err);
        return glue;
      })();
    }
    return engine;
  }

  class VoxelScene extends HTMLElement {
    constructor() {
      super();
      this.t = { created: now() };
      const root = this.attachShadow({ mode: 'open' });
      root.innerHTML =
        '<style>' +
        ':host{display:block;position:relative;aspect-ratio:16/10;overflow:hidden;border-radius:12px;background:#e7e4dc;contain:content}' +
        'canvas,img{position:absolute;inset:0;width:100%;height:100%;display:block}' +
        'img{object-fit:cover;transition:opacity .25s}' +
        'canvas{touch-action:pan-y;cursor:grab}' +
        ':host([data-live]) img{opacity:0;pointer-events:none}' +
        '::slotted(img){position:absolute;inset:0;width:100%;height:100%;object-fit:cover;transition:opacity .25s}' +
        ':host([data-live]) ::slotted(img){opacity:0;pointer-events:none}' +
        '</style><canvas></canvas><img alt=""><slot></slot>';
      this.canvas = root.querySelector('canvas');
      this.img = root.querySelector('img');
      this.yaw = 0.9;
      this.pitch = 0.55;
      this.zoom = 0.88;
    }

    connectedCallback() {
      const t = this.t;
      t.connected = now();
      this.setAttribute('role', 'img');
      if (this.hasAttribute('alt')) this.setAttribute('aria-label', this.getAttribute('alt'));
      // Poster: a `poster` attribute, or an <img> child, which the browser starts fetching
      // while it parses the page, before this script has even loaded.
      const child = this.querySelector('img');
      const poster = this.getAttribute('poster');
      if (child) {
        this.img.remove();
      } else if (poster && !this.img.src) {
        this.img.onload = () => requestAnimationFrame(() => {
          t.posterVisible = now();
          this.emit('poster');
        });
        this.img.src = poster;
      }
      if (!navigator.gpu) {
        t.noWebGPU = true;
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

    async start() {
      this.started = true;
      const t = this.t;
      const bench = this.hasAttribute('bench');
      const split = this.hasAttribute('split');
      const nonce = +(this.getAttribute('nonce') || window.VXR_NONCE || 0) >>> 0;
      t.visible = now();
      try {
        const voxP = fetch(this.getAttribute('src')).then((r) => {
          if (!r.ok) throw new Error('vox http ' + r.status);
          return r.arrayBuffer();
        });
        const glue = await loadEngine(t, bench);
        this.glue = glue;
        this.viewer = new glue.Viewer(this.canvas, nonce);
        t.pipelinesCreated = now();
        if (split) {
          await glue.gpu_done();
          t.pipelinesReady = now();
        }
        const bytes = new Uint8Array(await voxP);
        t.voxFetched = now();
        const n = this.viewer.load(bytes);
        if (n >>> 31) throw new Error('vox parse ' + (n & 0x7fffffff));
        t.voxels = n;
        t.loaded = now();
        this.ro = new ResizeObserver(() => this.kick());
        this.ro.observe(this);
        this.setupInput();
        if (this.draw()) throw new Error('first frame');
        t.firstSubmit = now();
        await glue.gpu_done();
        t.firstDone = now();
        requestAnimationFrame(() => {
          t.firstPresented = now();
          this.dataset.live = '';
          this.emit('frame');
          this.loop();
        });
      } catch (e) {
        t.error = String(e && e.message ? e.message : e);
        this.emit('error');
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
      const err = this.viewer.render(this.yaw, this.pitch, this.zoom, w, h);
      if (this.onframe) this.onframe(this.canvas);
      return err;
    }

    loop() {
      if (this.raf || !this.viewer) return;
      const spin = this.hasAttribute('autorotate');
      const tick = (ts) => {
        this.raf = 0;
        if (!this.visible) return;
        if (spin && !this.drag) {
          this.yaw += Math.min(ts - (this.last || ts), 50) * 0.00025;
          this.dirty = true;
        }
        this.last = ts;
        if (this.dirty) {
          this.dirty = false;
          this.draw();
        }
        if (spin || this.dirty) this.raf = requestAnimationFrame(tick);
      };
      this.raf = requestAnimationFrame(tick);
    }

    kick() {
      this.dirty = true;
      this.loop();
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
        this.yaw -= (e.clientX - d.x) * 0.008;
        this.pitch = Math.max(-1.2, Math.min(1.45, this.pitch + (e.clientY - d.y) * 0.008));
        d.x = e.clientX;
        d.y = e.clientY;
        this.kick();
      });
      const up = (e) => {
        if (this.drag && this.drag.id === e.pointerId) {
          this.drag = null;
          c.style.cursor = '';
          this.last = 0;
        }
      };
      c.addEventListener('pointerup', up);
      c.addEventListener('pointercancel', up);
    }
  }

  if (!customElements.get('voxel-scene')) customElements.define('voxel-scene', VoxelScene);
})();
