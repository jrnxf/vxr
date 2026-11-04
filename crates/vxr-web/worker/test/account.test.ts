import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { LIMITS, projects } from '../projects';
import { type App, type Client, png, vox, World } from './helpers';

let accounts = 0;
async function signedIn(w = new World()): Promise<{ c: Client; email: string; id: string }> {
  const n = ++accounts;
  const email = `del${n}@example.com`;
  const c = w.client(`198.51.100.${100 + n}`);
  expect((await c.codeSignIn(email)).status).toBe(200);
  return { c, email, id: (await c.me()).user.id };
}

const keys = async (prefix: string) => (await env.PROJECTS.list({ prefix })).objects.map((o: R2Object) => o.key);
const rows = async (sql: string, ...args: unknown[]) =>
  (await env.DB.prepare(sql).bind(...args).first<{ n: number }>())?.n ?? 0;

describe('delete account', () => {
  it('deletes the user, its sessions, projects and files, and signs out', async () => {
    const w = new World();
    const { c, email, id } = await signedIn(w);
    await c.put('p1', vox(3), null, 'Island');
    await c.put('p2', vox(5), null, 'Castle');
    expect((await c.fetch('/api/projects/p1/thumb', { method: 'PUT', body: png() })).status).toBe(204);
    // Another account's files stay.
    const other = await signedIn(w);
    await other.c.put('p1', vox(2), null);
    expect(await keys(`${id}/`)).toHaveLength(3);

    const res = await c.fetch('/api/account', { method: 'DELETE', json: { email: email.toUpperCase() } });
    expect(res.status).toBe(204);
    expect(res.headers.getSetCookie().some((s) => /max-age=0/i.test(s))).toBe(true);

    expect((await c.me()).user).toBeNull();
    expect((await c.json('/api/projects')).status).toBe(401);
    expect(await keys(`${id}/`)).toEqual([]);
    expect(await rows('select count(*) as n from "user" where id = ?', id)).toBe(0);
    expect(await rows('select count(*) as n from session where "userId" = ?', id)).toBe(0);
    expect(await rows('select count(*) as n from account where "userId" = ?', id)).toBe(0);
    expect(await rows('select count(*) as n from project where user_id = ?', id)).toBe(0);

    expect(await keys(`${other.id}/`)).toHaveLength(1);
    expect((await other.c.json('/api/projects')).body.projects).toHaveLength(1);
  });

  it('signs in afterwards as a new, empty account', async () => {
    const w = new World();
    const { c, email, id } = await signedIn(w);
    await c.put('p1', vox(), null);
    expect((await c.fetch('/api/account', { method: 'DELETE', json: { email } })).status).toBe(204);
    const again = w.client(`198.51.100.${200 + accounts}`);
    expect((await again.codeSignIn(email)).status).toBe(200);
    const me = await again.me();
    expect(me.user.id).not.toBe(id);
    expect(me.usage).toBe(0);
  });

  it('needs the account email typed to confirm, a session and this site', async () => {
    const w = new World();
    const { c, id } = await signedIn(w);
    await c.put('p1', vox(), null);
    for (const body of [{ email: 'someone@example.com' }, {}]) {
      const res = await c.json('/api/account', { method: 'DELETE', json: body });
      expect(res).toMatchObject({ status: 400, body: { error: 'confirm' } });
    }
    expect((await c.json('/api/account', { method: 'DELETE', body: 'not json' })).status).toBe(400);
    expect((await c.json('/api/account', { method: 'POST', json: {} })).status).toBe(405);
    const foreign = await c.json('/api/account', { method: 'DELETE', headers: { origin: 'https://evil.example' }, json: {} });
    expect(foreign.status).toBe(403);
    expect((await w.client('192.0.2.77').json('/api/account', { method: 'DELETE', json: {} })).status).toBe(401);
    // Nothing was deleted.
    expect((await c.me()).user.id).toBe(id);
    expect(await keys(`${id}/`)).toHaveLength(1);
  });

  it('works for the desktop app with its bearer token', async () => {
    const w = new World();
    const a: App = w.app(`192.0.2.${150 + ++accounts}`);
    const email = `app${accounts}@example.com`;
    expect((await a.codeSignIn(email)).status).toBe(200);
    const id = (await a.me()).user.id;
    const put = await a.fetch('/api/projects/p1', { method: 'PUT', headers: { 'if-none-match': '*' }, body: vox() });
    expect(put.status).toBe(201);
    expect((await a.fetch('/api/account', { method: 'DELETE', json: { email } })).status).toBe(204);
    expect((await a.me()).user).toBeNull();
    expect(await keys(`${id}/`)).toEqual([]);
  });

  it('leaves no files behind when a save lands after the delete', async () => {
    const { c, email, id } = await signedIn();
    expect((await c.fetch('/api/account', { method: 'DELETE', json: { email } })).status).toBe(204);
    // A save that passed its session check before the delete: its row has no user to belong to,
    // so the write fails and takes its bytes with it.
    const req = new Request('https://vxr.test/api/projects/p9', { method: 'PUT', headers: { 'if-none-match': '*' }, body: vox() });
    await expect(projects(req, env, id, ['p9'], LIMITS)).rejects.toThrow();
    expect(await keys(`${id}/`)).toEqual([]);
  });
});
