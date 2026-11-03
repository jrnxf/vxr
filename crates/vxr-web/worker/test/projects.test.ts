import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { LIMITS, projects } from '../projects';
import { voxProblem } from '../vox';
import { type Client, png, vox, World } from './helpers';

// A new account each time, from its own IP so the sign-in email limits never apply.
let accounts = 0;
async function signedIn(w = new World(), ip?: string): Promise<Client> {
  const n = ++accounts;
  const c = w.client(ip ?? `198.51.100.${n}`);
  expect((await c.codeSignIn(`p${n}@example.com`)).status).toBe(200);
  return c;
}

const bytes = async (res: Response) => new Uint8Array(await res.arrayBuffer());

describe('projects', () => {
  it('needs a session', async () => {
    const c = new World().client();
    expect((await c.json('/api/projects')).status).toBe(401);
    expect((await c.put('p1', vox(), null)).status).toBe(401);
  });

  it('adds, lists, reads and replaces a project with the version check', async () => {
    const a = await signedIn();
    const first = vox(3);
    const made = await a.put('p1', first, null, 'Island');
    expect(made.status).toBe(201);
    const v1 = made.body.project.version;
    expect(made.res.headers.get('etag')).toBe(`"${v1}"`);

    const list = await a.json('/api/projects');
    expect(list.body.projects).toMatchObject([{ id: 'p1', name: 'Island', version: v1, size: first.length, device: 'dev-a' }]);
    expect(list.body.usage).toBe(first.length);

    const read = await a.fetch('/api/projects/p1');
    expect(read.headers.get('x-vxr-version')).toBe(v1);
    expect(await bytes(read)).toEqual(first);
    expect((await a.fetch('/api/projects/p1', { headers: { 'if-none-match': `"${v1}"` } })).status).toBe(304);

    const second = vox(5);
    const replaced = await a.put('p1', second, v1, 'Island 2');
    expect(replaced.status).toBe(200);
    const v2 = replaced.body.project.version;
    expect(v2).not.toBe(v1);
    expect(await bytes(await a.fetch('/api/projects/p1'))).toEqual(second);
    expect((await a.json('/api/projects')).body.projects).toHaveLength(1);
    // The old version's bytes are gone from R2.
    const kept = await env.PROJECTS.list({ prefix: `${(await a.me()).user.id}/p1/` });
    expect(kept.objects.map((o: R2Object) => o.key)).toEqual([`${(await a.me()).user.id}/p1/${v2}.vox`]);
  });

  it('needs a precondition to save', async () => {
    const a = await signedIn();
    const res = await a.json('/api/projects/p1', { method: 'PUT', body: vox() });
    expect(res.status).toBe(428);
  });

  it('keeps both copies when a save lost the version check', async () => {
    const w = new World();
    const a = await signedIn(w);
    const v1 = (await a.put('p1', vox(1), null)).body.project.version;
    // Another device of the same account saves first...
    const other = (await a.put('p1', vox(2), v1, 'Island', 'dev-b')).body.project.version;
    // ...then this one saves its own edit of v1.
    const mine = vox(4);
    const res = await a.put('p1', mine, v1, 'Island', 'dev-a');
    expect(res.status).toBe(409);
    expect(res.body).toMatchObject({ error: 'conflict', id: 'p1', current: { id: 'p1', version: other, device: 'dev-b' } });
    const copy = res.body.copy;
    expect(copy.name).toBe('Island (conflicted copy)');
    expect(copy.id).not.toBe('p1');

    const list = (await a.json('/api/projects')).body.projects;
    expect(list.map((p: { id: string }) => p.id).sort()).toEqual(['p1', copy.id].sort());
    expect(await bytes(await a.fetch(`/api/projects/${copy.id}`))).toEqual(mine);
    expect(await bytes(await a.fetch('/api/projects/p1'))).toEqual(vox(2));
  });

  it('keeps both when a new project id is taken, or the project was deleted', async () => {
    const a = await signedIn();
    const v1 = (await a.put('p1', vox(1), null)).body.project.version;
    const again = await a.put('p1', vox(2), null);
    expect(again.status).toBe(409);
    expect(again.body.current.version).toBe(v1);

    expect((await a.fetch('/api/projects/p1', { method: 'DELETE' })).status).toBe(204);
    const gone = await a.put('p1', vox(3), v1);
    expect(gone.status).toBe(409);
    expect(gone.body.current).toBeNull();
    expect(gone.body.copy.name).toBe('Island (conflicted copy)');
  });

  it('takes the same bytes saved again as saved, not as a conflict', async () => {
    const a = await signedIn();
    const v1 = (await a.put('p1', vox(1), null)).body.project.version;
    const v2 = (await a.put('p1', vox(2), v1)).body.project.version;
    // The answer to the second save was lost; the device retries it from v1.
    const retry = await a.put('p1', vox(2), v1);
    expect(retry.status).toBe(200);
    expect(retry.body.project.version).toBe(v2);
    expect((await a.json('/api/projects')).body.projects).toHaveLength(1);
  });

  it('keeps accounts apart', async () => {
    const w = new World();
    const a = await signedIn(w, '192.0.2.21');
    const b = await signedIn(w, '192.0.2.22');
    await a.put('p1', vox(1), null);
    expect((await b.json('/api/projects')).body.projects).toEqual([]);
    expect((await b.fetch('/api/projects/p1')).status).toBe(404);
    expect((await b.put('p1', vox(2), null)).status).toBe(201);
  });

  it('refuses a file over the file limit', async () => {
    const a = await signedIn(new World({ account: 10_000, file: 1_000 }));
    const big = vox(1, 2_000);
    const res = await a.put('p1', big, null);
    expect(res.status).toBe(413);
    expect(res.body).toMatchObject({ error: 'file_too_large', limit: 1_000 });
    // Without a Content-Length too (a streamed body), counted as it comes.
    const stream = new ReadableStream({
      start(c) {
        c.enqueue(big);
        c.close();
      },
    });
    const streamed = await a.json('/api/projects/p2', { method: 'PUT', headers: { 'if-none-match': '*' }, body: stream, duplex: 'half' } as RequestInit);
    expect(streamed.status).toBe(413);
  });

  it('refuses a save over the account limit, counting the file it replaces', async () => {
    const a = await signedIn(new World({ account: 3_000, file: 2_000 }));
    const v = (await a.put('p1', vox(1, 1_500), null)).body.project.version;
    const over = await a.put('p2', vox(1, 1_500), null);
    expect(over.status).toBe(413);
    expect(over.body).toMatchObject({ error: 'quota', limit: 3_000 });
    // Replacing p1 with a file of the same size fits.
    expect((await a.put('p1', vox(1, 1_400), v)).status).toBe(200);
    expect((await a.json('/api/projects')).body.projects).toHaveLength(1);
    // A conflict copy counts too.
    const copy = await a.put('p1', vox(2, 1_500), v);
    expect(copy.status).toBe(413);
  });

  it('keeps only .vox files', async () => {
    const a = await signedIn();
    const text = new TextEncoder().encode('not a voxel file at all, just text');
    const res = await a.put('p1', text, null);
    expect(res.status).toBe(400);
    expect(res.body.error).toBe('not_vox');
    const cut = vox(3).subarray(0, 40);
    expect((await a.put('p1', cut, null)).status).toBe(400);
    expect((await a.json('/api/projects')).body.projects).toEqual([]);
  });

  it('checks the structure of a .vox', () => {
    expect(voxProblem(vox(10))).toBeNull();
    const bad = vox(10);
    new DataView(bad.buffer).setUint32(28, 999, true);
    expect(voxProblem(bad)).not.toBeNull();
    const extra = new Uint8Array(vox(1).length + 4);
    extra.set(vox(1));
    expect(voxProblem(extra)).toBe('MAIN does not fill the file');
  });

  it('keeps a thumbnail beside a project, and refuses anything but a small PNG', async () => {
    const a = await signedIn();
    await a.put('p1', vox(1), null);
    expect((await a.fetch('/api/projects/p1/thumb', { method: 'PUT', body: png(160) })).status).toBe(204);
    const list = (await a.json('/api/projects')).body;
    expect(list.projects[0].thumb).toBe(true);
    expect(list.usage).toBe(vox(1).length + png(160).length);
    expect(await bytes(await a.fetch('/api/projects/p1/thumb'))).toEqual(png(160));
    expect((await a.fetch('/api/projects/p1/thumb', { method: 'PUT', body: vox(1) })).status).toBe(400);
    expect((await a.fetch('/api/projects/p1/thumb', { method: 'PUT', body: png(4096) })).status).toBe(400);
    expect((await a.fetch('/api/projects/nope/thumb', { method: 'PUT', body: png() })).status).toBe(404);
  });

  it('deletes a project and its files', async () => {
    const a = await signedIn();
    await a.put('p1', vox(1), null);
    await a.fetch('/api/projects/p1/thumb', { method: 'PUT', body: png() });
    expect((await a.fetch('/api/projects/p1', { method: 'DELETE' })).status).toBe(204);
    expect((await a.json('/api/projects')).body.projects).toEqual([]);
    const left = await env.PROJECTS.list({ prefix: `${(await a.me()).user.id}/p1/` });
    expect(left.objects).toEqual([]);
  });

  it('leaves no files behind when a delete races a save or a thumbnail', async () => {
    const a = await signedIn();
    const user = (await a.me()).user.id;
    const left = async () => (await env.PROJECTS.list({ prefix: `${user}/p1/` })).objects.map((o: R2Object) => o.key);
    const del = () => projects(new Request('https://vxr.test/', { method: 'DELETE' }), env, user, ['p1'], LIMITS);

    // A save's bytes land after the delete's first listing.
    await a.put('p1', vox(1), null);
    let listings = 0;
    const late: R2Bucket = Object.assign(Object.create(env.PROJECTS), {
      list: async (o: R2ListOptions) => {
        const out = await env.PROJECTS.list(o);
        if (listings++ === 0) await env.PROJECTS.put(`${user}/p1/late.vox`, vox(2));
        return out;
      },
      delete: (k: string | string[]) => env.PROJECTS.delete(k),
    });
    expect((await projects(new Request('https://vxr.test/', { method: 'DELETE' }), { ...env, PROJECTS: late }, user, ['p1'], LIMITS)).status).toBe(204);
    expect(await left()).toEqual([]);

    // The delete runs between a thumbnail's row update and its put.
    await a.put('p1', vox(1), null);
    const racing: R2Bucket = Object.assign(Object.create(env.PROJECTS), {
      put: async (k: string, v: Uint8Array, o?: R2PutOptions) => {
        if (k.endsWith('/thumb.png')) expect((await del()).status).toBe(204);
        return env.PROJECTS.put(k, v, o);
      },
      delete: (k: string | string[]) => env.PROJECTS.delete(k),
    });
    const put = new Request('https://vxr.test/', { method: 'PUT', body: png() });
    expect((await projects(put, { ...env, PROJECTS: racing }, user, ['p1', 'thumb'], LIMITS)).status).toBe(404);
    expect(await left()).toEqual([]);
  });

  it('refuses writes from another site and bad ids', async () => {
    const a = await signedIn();
    const res = await a.json('/api/projects/p1', {
      method: 'PUT',
      headers: { origin: 'https://evil.example', 'if-none-match': '*' },
      body: vox(),
    });
    expect(res.status).toBe(403);
    expect((await a.put('bad id!', vox(), null)).status).toBe(400);
  });
});
