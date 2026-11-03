// What the Worker tests share: a browser-like client (one origin, a cookie jar), the Worker with
// its email and Turnstile replaced, passkey and email-link sign-in, and generated .vox files.
import { env } from 'cloudflare:workers';
import { type Deps, handle } from '../index';
import type { Mail } from '../mail';
import type { Limits } from '../projects';
import { Authenticator } from './authenticator';

export const ORIGIN = 'https://vxr.test';
/** The Turnstile token the replaced check accepts. */
export const PASS = 'turnstile-pass';

export class World {
  mail: Mail[] = [];
  deps: Deps;
  constructor(limits?: Limits) {
    this.deps = {
      mailer: () => ({ send: async (m) => void this.mail.push(m) }),
      verify: () => async (token) => token === PASS,
      limits,
    };
  }
  client(ip = '192.0.2.10') {
    return new Client(this, ip);
  }
  /** The sign-in link in the last mail to `to`. */
  link(to: string): string {
    const m = [...this.mail].reverse().find((x) => x.to === to);
    const url = m?.text.match(/https?:\/\/\S+/)?.[0];
    if (!url) throw new Error(`no link mailed to ${to}`);
    return url;
  }
}

export class Client {
  jar = new Map<string, string>();
  passkeys = new Authenticator(ORIGIN);
  constructor(
    readonly world: World,
    readonly ip: string,
  ) {}

  async fetch(path: string, init: RequestInit & { json?: unknown } = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (!headers.has('origin')) headers.set('origin', ORIGIN);
    headers.set('cf-connecting-ip', this.ip);
    if (this.jar.size) headers.set('cookie', [...this.jar].map(([k, v]) => `${k}=${v}`).join('; '));
    let body = init.body;
    if (init.json !== undefined) {
      headers.set('content-type', 'application/json');
      body = JSON.stringify(init.json);
    }
    const res = await handle(new Request(new URL(path, ORIGIN), { ...init, headers, body, redirect: 'manual' }), env, this.world.deps);
    for (const c of res.headers.getSetCookie()) {
      const [pair, ...attrs] = c.split(';');
      const i = pair.indexOf('=');
      const name = pair.slice(0, i).trim();
      const value = pair.slice(i + 1).trim();
      const gone = !value || attrs.some((a) => /^\s*max-age=0\s*$/i.test(a));
      if (gone) this.jar.delete(name);
      else this.jar.set(name, value);
    }
    return res;
  }

  async json<T = any>(path: string, init: RequestInit & { json?: unknown } = {}): Promise<{ status: number; body: T; res: Response }> {
    const res = await this.fetch(path, init);
    const text = await res.text();
    return { status: res.status, body: text ? JSON.parse(text) : null, res };
  }

  /** Signs up with a new passkey, as the account page does. */
  async passkeySignUp(token = PASS): Promise<Response> {
    const opts = await this.fetch('/api/auth/passkey/generate-register-options');
    if (opts.status !== 200) return opts;
    const response = await this.passkeys.create(await opts.json());
    return this.fetch('/api/auth/passkey/verify-registration', {
      method: 'POST',
      headers: { 'x-turnstile-token': token },
      json: { response, createSession: true },
    });
  }

  /** Signs in with this client's passkey. */
  async passkeySignIn(): Promise<Response> {
    const opts = await this.fetch('/api/auth/passkey/generate-authenticate-options');
    const response = await this.passkeys.get(await opts.json());
    return this.fetch('/api/auth/passkey/verify-authentication', { method: 'POST', json: { response } });
  }

  /** Asks for an email link to `email`. */
  askLink(email: string, token = PASS): Promise<Response> {
    return this.fetch('/api/auth/sign-in/magic-link', {
      method: 'POST',
      headers: { 'x-turnstile-token': token },
      json: { email, callbackURL: '/account' },
    });
  }

  /** Signs in by email link, following the mailed link. */
  async emailSignIn(email: string): Promise<Response> {
    const asked = await this.askLink(email);
    if (asked.status !== 200) throw new Error(`asking for a link: ${asked.status} ${await asked.text()}`);
    const link = new URL(this.world.link(email));
    return this.fetch(link.pathname + link.search);
  }

  async me() {
    return (await this.json('/api/me')).body;
  }

  /** Saves `bytes` as project `id`: `base` is the version it replaces, or null for a new one. */
  put(id: string, bytes: Uint8Array, base: string | null, name = 'Island', device = 'dev-a') {
    const headers: Record<string, string> = { 'x-vxr-name': encodeURIComponent(name), 'x-vxr-device': device };
    if (base) headers['if-match'] = `"${base}"`;
    else headers['if-none-match'] = '*';
    return this.json(`/api/projects/${id}`, { method: 'PUT', headers, body: bytes });
  }
}

function u32(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, true);
  return b;
}
function chunk(id: string, content: Uint8Array, children: Uint8Array = new Uint8Array()): Uint8Array {
  const out = new Uint8Array(12 + content.length + children.length);
  out.set(new TextEncoder().encode(id), 0);
  out.set(u32(content.length), 4);
  out.set(u32(children.length), 8);
  out.set(content, 12);
  out.set(children, 12 + content.length);
  return out;
}

/** A valid .vox with one model of `voxels` voxels (and `pad` extra bytes in a NOTE chunk). */
export function vox(voxels = 1, pad = 0): Uint8Array {
  const size = chunk('SIZE', Uint8Array.from([...u32(8), ...u32(8), ...u32(8)]));
  const xyzi = new Uint8Array(4 + voxels * 4);
  xyzi.set(u32(voxels));
  for (let i = 0; i < voxels; i++) xyzi.set([i % 8, (i >> 3) % 8, (i >> 6) % 8, 1], 4 + i * 4);
  const kids = [size, chunk('XYZI', xyzi)];
  if (pad) kids.push(chunk('NOTE', new Uint8Array(pad)));
  const children = new Uint8Array(kids.reduce((n, k) => n + k.length, 0));
  let at = 0;
  for (const k of kids) {
    children.set(k, at);
    at += k.length;
  }
  const main = chunk('MAIN', new Uint8Array(), children);
  const out = new Uint8Array(8 + main.length);
  out.set(new TextEncoder().encode('VOX '), 0);
  out.set(u32(150), 4);
  out.set(main, 8);
  return out;
}

/** A 1x1 PNG header (all the Worker checks of a thumbnail). */
export function png(side = 1): Uint8Array {
  const b = new Uint8Array(64);
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
  new DataView(b.buffer).setUint32(16, side);
  new DataView(b.buffer).setUint32(20, side);
  return b;
}
