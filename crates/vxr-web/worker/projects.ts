// The cloud projects API, for a signed-in account:
//
//   GET    /api/projects            the list (newest first), the bytes used and the limits
//   GET    /api/projects/:id        the .vox (ETag is the version)
//   PUT    /api/projects/:id        save: If-None-Match: * to add a new one, If-Match: "<version>"
//                                   to replace that version; x-vxr-name and x-vxr-device
//   DELETE /api/projects/:id        remove it from the account
//   GET    /api/projects/:id/thumb  its thumbnail (PNG)
//   PUT    /api/projects/:id/thumb  set it
//
// A save from a copy that is no longer the newest (someone saved it elsewhere since) never
// overwrites: it is kept as a new project, "<name> (conflicted copy)", and answered with 409 and
// both rows, so both copies stay. Limits are enforced here: 20 MB a file and 100 MB an account,
// thumbnails included, and only .vox files (and PNG thumbnails) are kept.
import { thumbProblem, voxProblem } from './vox';

export interface Limits {
  /** The most one account keeps, files and thumbnails together. */
  account: number;
  /** The largest file. */
  file: number;
}

export const LIMITS: Limits = { account: 100 * 1024 * 1024, file: 20 * 1024 * 1024 };

export interface Store {
  DB: D1Database;
  PROJECTS: R2Bucket;
}

interface Row {
  id: string;
  name: string;
  version: string;
  hash: string;
  size: number;
  thumb: number;
  device: string;
  created_at: number;
  updated_at: number;
}

/** A project as the API shows it. */
export interface Project {
  id: string;
  name: string;
  version: string;
  size: number;
  thumb: boolean;
  device: string;
  created: number;
  updated: number;
}

const ID = /^[A-Za-z0-9_-]{1,64}$/;
const NAME_CHARS = 200;
const CONFLICT = ' (conflicted copy)';

function show(r: Row): Project {
  return {
    id: r.id,
    name: r.name,
    version: r.version,
    size: r.size,
    thumb: r.thumb > 0,
    device: r.device,
    created: r.created_at,
    updated: r.updated_at,
  };
}

export function json(body: unknown, status = 200, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', 'cache-control': 'no-store', ...headers },
  });
}

export function problem(status: number, error: string, extra: Record<string, unknown> = {}): Response {
  return json({ error, ...extra }, status);
}

function newVersion(): string {
  const b = crypto.getRandomValues(new Uint8Array(9));
  return btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_');
}

async function sha256(bytes: Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return [...d].map((b) => b.toString(16).padStart(2, '0')).join('');
}

const voxKey = (user: string, id: string, version: string) => `${user}/${id}/${version}.vox`;
const thumbKey = (user: string, id: string) => `${user}/${id}/thumb.png`;

/** The bytes the account keeps, files and thumbnails. */
export async function usage(db: D1Database, user: string): Promise<number> {
  const r = await db
    .prepare('select coalesce(sum(size + thumb), 0) as used from project where user_id = ?')
    .bind(user)
    .first<{ used: number }>();
  return r?.used ?? 0;
}

async function row(db: D1Database, user: string, id: string): Promise<Row | null> {
  return db.prepare('select * from project where user_id = ? and id = ?').bind(user, id).first<Row>();
}

/** Reads a request body of at most `max` bytes, or null when it is longer. */
async function body(req: Request, max: number): Promise<Uint8Array | null> {
  const declared = Number(req.headers.get('content-length') ?? NaN);
  if (declared > max) return null;
  if (!req.body) return new Uint8Array();
  const parts: Uint8Array[] = [];
  let length = 0;
  const reader = req.body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.length;
    if (length > max) {
      await reader.cancel();
      return null;
    }
    parts.push(value);
  }
  const out = new Uint8Array(length);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/** The version an If-Match names (`"v"`, `W/"v"` or bare), or null. */
function matchVersion(h: string | null): string | null {
  if (!h) return null;
  const m = /^(?:W\/)?"?([A-Za-z0-9_-]{1,64})"?$/.exec(h.trim());
  return m ? m[1] : null;
}

function nameOf(req: Request): string {
  let raw = req.headers.get('x-vxr-name') ?? '';
  try {
    raw = decodeURIComponent(raw);
  } catch {
    // Not percent-encoded: take it as it is.
  }
  const name = raw.replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, NAME_CHARS);
  return name || 'Untitled';
}

function deviceOf(req: Request): string {
  return (req.headers.get('x-vxr-device') ?? '').replace(/[^A-Za-z0-9_-]/g, '').slice(0, 64);
}

function conflictName(name: string): string {
  const base = name.endsWith(CONFLICT) ? name : name + CONFLICT;
  return base.length > NAME_CHARS ? name.slice(0, NAME_CHARS - CONFLICT.length) + CONFLICT : base;
}

const etag = (version: string) => `"${version}"`;

export async function projects(req: Request, env: Store, user: string, path: string[], limits: Limits): Promise<Response> {
  const [id, part, ...rest] = path;
  if (rest.length) return problem(404, 'not_found');
  if (id === undefined) {
    if (req.method !== 'GET') return problem(405, 'method');
    const { results } = await env.DB.prepare('select * from project where user_id = ? order by updated_at desc')
      .bind(user)
      .all<Row>();
    const used = results.reduce((s, r) => s + r.size + r.thumb, 0);
    return json({ projects: results.map(show), usage: used, limits });
  }
  if (!ID.test(id)) return problem(400, 'bad_id');
  if (part === 'thumb') return thumb(req, env, user, id, limits);
  if (part !== undefined) return problem(404, 'not_found');
  switch (req.method) {
    case 'GET':
    case 'HEAD':
      return read(req, env, user, id);
    case 'PUT':
      return save(req, env, user, id, limits);
    case 'DELETE':
      return remove(env, user, id);
    default:
      return problem(405, 'method');
  }
}

async function read(req: Request, env: Store, user: string, id: string): Promise<Response> {
  const r = await row(env.DB, user, id);
  if (!r) return problem(404, 'not_found');
  const headers = {
    etag: etag(r.version),
    'x-vxr-version': r.version,
    'x-vxr-name': encodeURIComponent(r.name),
    'cache-control': 'private, no-cache',
    'content-type': 'application/octet-stream',
  };
  if (matchVersion(req.headers.get('if-none-match')) === r.version) return new Response(null, { status: 304, headers });
  const obj = await env.PROJECTS.get(voxKey(user, id, r.version));
  if (!obj) return problem(404, 'not_found');
  return new Response(req.method === 'HEAD' ? null : obj.body, { headers: { ...headers, 'content-length': String(r.size) } });
}

async function save(req: Request, env: Store, user: string, id: string, limits: Limits): Promise<Response> {
  const create = req.headers.get('if-none-match')?.trim() === '*';
  const base = matchVersion(req.headers.get('if-match'));
  if (!create && !base) return problem(428, 'precondition_required');
  const bytes = await body(req, limits.file);
  if (!bytes) return problem(413, 'file_too_large', { limit: limits.file });
  const bad = voxProblem(bytes);
  if (bad) return problem(400, 'not_vox', { detail: bad });
  const name = nameOf(req);
  const device = deviceOf(req);
  const hash = await sha256(bytes);
  const now = Date.now();
  const current = await row(env.DB, user, id);
  // The same bytes saved again (a retried save whose answer was lost): nothing to do.
  if (current && current.hash === hash) return saved(current, 200);
  const fresh = create ? !current : current?.version === base;
  if (!fresh) return conflict(env, user, id, name, device, bytes, hash, now, current, limits);
  const version = newVersion();
  const key = voxKey(user, id, version);
  await env.PROJECTS.put(key, bytes, { httpMetadata: { contentType: 'application/octet-stream' } });
  // Write the row only if nobody saved in between and the account still has room; the bytes go
  // first so a row never points at a missing object.
  const res = current
    ? await env.DB.prepare(
        `update project set name = ?, version = ?, hash = ?, size = ?, device = ?, updated_at = ?
         where user_id = ? and id = ? and version = ?
           and (select coalesce(sum(size + thumb), 0) from project where user_id = ?) - ? + ? <= ?`,
      )
        .bind(name, version, hash, bytes.length, device, now, user, id, base, user, current.size, bytes.length, limits.account)
        .run()
    : await env.DB.prepare(
        `insert into project (user_id, id, name, version, hash, size, device, created_at, updated_at)
         select ?, ?, ?, ?, ?, ?, ?, ?, ?
         where (select coalesce(sum(size + thumb), 0) from project where user_id = ?) + ? <= ?
         on conflict do nothing`,
      )
        .bind(user, id, name, version, hash, bytes.length, device, now, now, user, bytes.length, limits.account)
        .run();
  if (res.meta.changes !== 1) {
    await env.PROJECTS.delete(key);
    const now2 = await row(env.DB, user, id);
    const moved = create ? now2 !== null : now2?.version !== base;
    if (moved) return conflict(env, user, id, name, device, bytes, hash, now, now2, limits);
    return problem(413, 'quota', { limit: limits.account, usage: await usage(env.DB, user) });
  }
  if (current) await env.PROJECTS.delete(voxKey(user, id, current.version));
  const r = await row(env.DB, user, id);
  return saved(r!, current ? 200 : 201);
}

function saved(r: Row, status: number): Response {
  return json({ project: show(r) }, status, { etag: etag(r.version) });
}

/** Keeps a save that lost the version check as a new project beside the newer one. */
async function conflict(
  env: Store,
  user: string,
  id: string,
  name: string,
  device: string,
  bytes: Uint8Array,
  hash: string,
  now: number,
  current: Row | null,
  limits: Limits,
): Promise<Response> {
  const copy = crypto.randomUUID();
  const version = newVersion();
  const key = voxKey(user, copy, version);
  await env.PROJECTS.put(key, bytes, { httpMetadata: { contentType: 'application/octet-stream' } });
  const res = await env.DB.prepare(
    `insert into project (user_id, id, name, version, hash, size, device, created_at, updated_at)
     select ?, ?, ?, ?, ?, ?, ?, ?, ?
     where (select coalesce(sum(size + thumb), 0) from project where user_id = ?) + ? <= ?`,
  )
    .bind(user, copy, conflictName(name), version, hash, bytes.length, device, now, now, user, bytes.length, limits.account)
    .run();
  if (res.meta.changes !== 1) {
    await env.PROJECTS.delete(key);
    return problem(413, 'quota', { limit: limits.account, usage: await usage(env.DB, user), current: current && show(current) });
  }
  const r = await row(env.DB, user, copy);
  return json({ error: 'conflict', id, copy: show(r!), current: current && show(current) }, 409);
}

async function remove(env: Store, user: string, id: string): Promise<Response> {
  const r = await row(env.DB, user, id);
  if (!r) return problem(404, 'not_found');
  await env.DB.prepare('delete from project where user_id = ? and id = ?').bind(user, id).run();
  const listed = await env.PROJECTS.list({ prefix: `${user}/${id}/` });
  if (listed.objects.length) await env.PROJECTS.delete(listed.objects.map((o) => o.key));
  return new Response(null, { status: 204 });
}

async function thumb(req: Request, env: Store, user: string, id: string, limits: Limits): Promise<Response> {
  const r = await row(env.DB, user, id);
  if (!r) return problem(404, 'not_found');
  if (req.method === 'GET') {
    const obj = r.thumb ? await env.PROJECTS.get(thumbKey(user, id)) : null;
    if (!obj) return problem(404, 'not_found');
    return new Response(obj.body, { headers: { 'content-type': 'image/png', 'cache-control': 'private, no-cache' } });
  }
  if (req.method !== 'PUT') return problem(405, 'method');
  const bytes = await body(req, 256 * 1024);
  if (!bytes) return problem(413, 'thumb_too_large');
  const bad = thumbProblem(bytes);
  if (bad) return problem(400, 'not_png', { detail: bad });
  const res = await env.DB.prepare(
    `update project set thumb = ? where user_id = ? and id = ?
       and (select coalesce(sum(size + thumb), 0) from project where user_id = ?) - thumb + ? <= ?`,
  )
    .bind(bytes.length, user, id, user, bytes.length, limits.account)
    .run();
  if (res.meta.changes !== 1) return problem(413, 'quota', { limit: limits.account });
  await env.PROJECTS.put(thumbKey(user, id), bytes, { httpMetadata: { contentType: 'image/png' } });
  return new Response(null, { status: 204 });
}
