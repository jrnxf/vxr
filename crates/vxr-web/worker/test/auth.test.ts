import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { handle, MAIL_PER_HOUR } from '../index';
import { mailerFor } from '../mail';
import { turnstile } from '../turnstile';
import { ORIGIN, World } from './helpers';

describe('sign-in', () => {
  it('reports sign-in off without the secret, and still answers /api/me', async () => {
    const res = await handle(new Request(`${ORIGIN}/api/me`), { ...env, BETTER_AUTH_SECRET: undefined });
    expect(await res.json()).toMatchObject({ signIn: false, user: null });
    const list = await handle(new Request(`${ORIGIN}/api/projects`), { ...env, BETTER_AUTH_SECRET: undefined });
    expect(list.status).toBe(503);
  });

  it('starts signed out', async () => {
    const me = await new World().client().me();
    expect(me).toMatchObject({ signIn: true, email: true, user: null, limits: { account: 100 * 1024 * 1024, file: 20 * 1024 * 1024 } });
  });

  it('signs up with a passkey and no email, then signs in with it again', async () => {
    const a = new World().client();
    const up = await a.passkeySignUp();
    expect(up.status).toBe(200);
    const me = await a.me();
    expect(me.user).toMatchObject({ email: null });
    const id = me.user.id;

    const out = await a.fetch('/api/auth/sign-out', { method: 'POST', json: {} });
    expect(out.status).toBe(200);
    expect((await a.me()).user).toBeNull();

    const back = await a.passkeySignIn();
    expect(back.status).toBe(200);
    expect((await a.me()).user.id).toBe(id);
  });

  it('needs the Turnstile challenge to sign up with a passkey', async () => {
    const a = new World().client();
    const res = await a.passkeySignUp('wrong');
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ error: 'challenge' });
  });

  it('signs in by email link, once per link', async () => {
    const w = new World();
    const a = w.client();
    const res = await a.emailSignIn('ada@example.com');
    expect([200, 302]).toContain(res.status);
    expect((await a.me()).user).toMatchObject({ email: 'ada@example.com' });

    // The same link a second time signs nobody in.
    const b = w.client('192.0.2.11');
    const link = new URL(w.link('ada@example.com'));
    await b.fetch(link.pathname + link.search);
    expect((await b.me()).user).toBeNull();
  });

  it('adds a passkey to an account made by email link, with no challenge', async () => {
    const w = new World();
    const a = w.client('192.0.2.12');
    await a.emailSignIn('bea@example.com');
    const id = (await a.me()).user.id;
    const added = await a.passkeySignUp('no-token');
    expect(added.status).toBe(200);
    await a.fetch('/api/auth/sign-out', { method: 'POST', json: {} });
    await a.passkeySignIn();
    expect((await a.me()).user).toMatchObject({ id, email: 'bea@example.com' });
  });

  it('needs the challenge for an email link, and limits links per address', async () => {
    const w = new World();
    const a = w.client('192.0.2.13');
    expect((await a.askLink('cy@example.com', 'wrong')).status).toBe(403);
    for (let i = 0; i < MAIL_PER_HOUR.email; i++) {
      // A fresh IP each time, so only the per-address limit applies.
      const res = await w.client(`198.51.100.${i + 1}`).askLink('cy@example.com');
      expect(res.status).toBe(200);
    }
    const over = await w.client('198.51.100.99').askLink('cy@example.com');
    expect(over.status).toBe(429);
    expect(w.mail.filter((m) => m.to === 'cy@example.com')).toHaveLength(MAIL_PER_HOUR.email);
  });

  it('limits email links per IP', async () => {
    const w = new World();
    const a = w.client('192.0.2.200');
    let last = 200;
    for (let i = 0; i <= MAIL_PER_HOUR.ip && last !== 429; i++) last = (await a.askLink(`ip${i}@example.com`)).status;
    expect(last).toBe(429);
  });

  it('turns email sign-in off without a sender', async () => {
    const w = new World();
    w.deps.mailer = () => null;
    const a = w.client('192.0.2.14');
    expect((await a.me()).email).toBe(false);
    expect((await a.askLink('dee@example.com')).status).toBe(503);
  });

  it('prints links only for a local host', () => {
    expect(mailerFor({ EMAIL_SENDER: 'log' }, new URL('http://localhost:8787/'))).not.toBeNull();
    expect(mailerFor({ EMAIL_SENDER: 'log' }, new URL('https://vxr.jrnxf.co/'))).toBeNull();
    expect(mailerFor({}, new URL('http://localhost:8787/'))).toBeNull();
  });

  it('checks Turnstile tokens with siteverify', async () => {
    const seen: FormData[] = [];
    const fake = (ok: boolean) =>
      (async (_url: RequestInfo | URL, init?: RequestInit) => {
        seen.push(init!.body as FormData);
        return Response.json({ success: ok });
      }) as typeof fetch;
    expect(await turnstile('secret', fake(true))('token', '192.0.2.1')).toBe(true);
    expect(seen[0].get('secret')).toBe('secret');
    expect(seen[0].get('response')).toBe('token');
    expect(seen[0].get('remoteip')).toBe('192.0.2.1');
    expect(await turnstile('secret', fake(false))('token', null)).toBe(false);
    expect(await turnstile('secret', fake(true))('', null)).toBe(false);
  });

  it('serves the account page with its own content security policy', async () => {
    const page = new Response('<!doctype html>', { headers: { 'content-security-policy': "default-src 'none'" } });
    const res = await handle(new Request(`${ORIGIN}/account`), { ...env, ASSETS: { fetch: async () => page } as unknown as Fetcher });
    expect(res.headers.get('content-security-policy')).toContain('https://challenges.cloudflare.com');
  });
});
