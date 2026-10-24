// <voxel-scene src="scene.vox"> - drop-in web component that renders a .vox scene.
// One script tag + one element. Loads three.js from a CDN on first use; renders only while visible.
(() => {
  const THREE_URL = 'https://cdn.jsdelivr.net/npm/three@0.170.0/build/three.module.js';

  //@@CORE@@

  let threePromise = null;
  const STYLE = `
    :host { display: block; position: relative; aspect-ratio: 16 / 10; border-radius: 14px; overflow: hidden; background: radial-gradient(120% 90% at 50% 35%, #2a303d 0%, #12141a 75%); contain: content; }
    canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; cursor: grab; touch-action: pan-y; }
    canvas:active { cursor: grabbing; }
    .badge { position: absolute; left: 10px; bottom: 10px; font: 500 11px/1 Inter, system-ui, sans-serif; color: rgba(255,255,255,.72); background: rgba(10,12,16,.55); padding: 6px 9px; border-radius: 999px; backdrop-filter: blur(8px); pointer-events: none; }
    .load { position: absolute; inset: 0; display: grid; place-items: center; font: 13px Inter, system-ui, sans-serif; color: rgba(255,255,255,.55); }`;

  class VoxelScene extends HTMLElement {
    static observedAttributes = ['src'];
    connectedCallback() {
      if (this._root) return;
      this._root = this.attachShadow({ mode: 'open' });
      this._root.innerHTML = `<style>${STYLE}</style><canvas part="canvas"></canvas><div class="load">Loading scene…</div><div class="badge" hidden></div>`;
      this._load();
    }
    attributeChangedCallback(name, a, b) { if (this._root && a !== b) this._load(); }
    disconnectedCallback() { this._io?.disconnect(); this._ro?.disconnect(); this._renderer?.setAnimationLoop(null); }

    async _load() {
      const src = this.getAttribute('src');
      if (!src) return;
      const THREE = await (threePromise ||= import(THREE_URL));
      let buf;
      if (src.startsWith('local:')) {
        const b64 = localStorage.getItem(src.slice(6));
        if (!b64) throw new Error('no local scene');
        const bin = atob(b64); buf = new Uint8Array(bin.length);
        for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
        buf = buf.buffer;
      } else buf = await (await fetch(src)).arrayBuffer();
      const { world, palette } = readVox(buf);
      this._build(THREE, world, palette);
    }

    _build(THREE, world, palette) {
      const canvas = this._root.querySelector('canvas');
      this._root.querySelector('.load').remove();
      const renderer = this._renderer || new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
      this._renderer = renderer;
      renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
      renderer.shadowMap.enabled = true; renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      renderer.toneMapping = THREE.NeutralToneMapping; renderer.toneMappingExposure = 1.05;
      const scene = new THREE.Scene();
      const mat = createVoxelMaterial(THREE, createPaletteTexture(THREE, palette));
      for (const c of world.chunks.values()) {
        const r = meshChunk(world, c);
        if (!r) continue;
        const m = new THREE.Mesh(buildChunkGeometry(THREE, r), mat);
        m.position.set(c.cx * CS, c.cy * CS, c.cz * CS);
        m.castShadow = m.receiveShadow = true;
        scene.add(m);
      }
      const b = world.bounds() || { minX: -8, minY: 0, minZ: -8, maxX: 8, maxY: 8, maxZ: 8 };
      const cx = (b.minX + b.maxX) / 2, cz = (b.minZ + b.maxZ) / 2, span = Math.max(b.maxX - b.minX, b.maxZ - b.minZ);
      const R = span * 0.75 + (b.maxY - b.minY);
      scene.add(new THREE.HemisphereLight(0xdfe8ff, 0x3a3226, 1.15));
      const sun = new THREE.DirectionalLight(0xfff1dc, 2.4);
      sun.position.set(cx + R * 0.55, R * 1.1, cz + R * 0.75); sun.target.position.set(cx, 0, cz);
      sun.castShadow = true; sun.shadow.mapSize.set(2048, 2048); sun.shadow.bias = -0.0004; sun.shadow.normalBias = 0.03;
      Object.assign(sun.shadow.camera, { left: -R, right: R, top: R, bottom: -R, near: 1, far: R * 4 });
      sun.shadow.camera.updateProjectionMatrix();
      scene.add(sun, sun.target);
      const ground = new THREE.Mesh(new THREE.PlaneGeometry(span * 6, span * 6), new THREE.ShadowMaterial({ opacity: 0.28 }));
      ground.rotation.x = -Math.PI / 2; ground.position.set(cx, -0.01, cz); ground.receiveShadow = true; scene.add(ground);
      const camera = new THREE.PerspectiveCamera(35, 1, 0.5, R * 20);
      const target = new THREE.Vector3(cx, Math.min(b.maxY, 24) * 0.35, cz);
      const dist = span * 1.6 + (b.maxY - b.minY);
      let az = 0.7, el = 0.62, vaz = 0, auto = this.hasAttribute('autorotate');
      const place = () => {
        camera.position.set(target.x + Math.sin(az) * Math.cos(el) * dist, target.y + Math.sin(el) * dist, target.z + Math.cos(az) * Math.cos(el) * dist);
        camera.lookAt(target);
      };
      // drag to orbit; never captures page scroll
      let dragging = null;
      canvas.onpointerdown = (e) => { dragging = [e.clientX, e.clientY]; canvas.setPointerCapture(e.pointerId); auto = false; };
      canvas.onpointermove = (e) => {
        if (!dragging) return;
        const dx = e.clientX - dragging[0], dy = e.clientY - dragging[1];
        dragging = [e.clientX, e.clientY];
        vaz = -dx * 0.006; az += vaz; el = Math.min(1.35, Math.max(0.08, el + dy * 0.005));
      };
      canvas.onpointerup = () => { dragging = null; };
      const resize = () => {
        const w = this.clientWidth, h = this.clientHeight;
        if (!w || !h) return;
        renderer.setSize(w, h, false); camera.aspect = w / h; camera.updateProjectionMatrix();
      };
      this._ro = new ResizeObserver(resize); this._ro.observe(this); resize();
      const loop = () => {
        if (auto) az += 0.0025; else if (!dragging) { az += vaz; vaz *= 0.92; }
        place(); renderer.render(scene, camera);
      };
      this._io = new IntersectionObserver(([en]) => renderer.setAnimationLoop(en.isIntersecting ? loop : null));
      this._io.observe(this);
      const badge = this._root.querySelector('.badge');
      badge.textContent = `${world.count.toLocaleString()} voxels · drag to orbit`;
      badge.hidden = false;
    }
  }
  if (!customElements.get('voxel-scene')) customElements.define('voxel-scene', VoxelScene);
})();
