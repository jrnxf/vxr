import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { OTP_OFF } from '../auth';
import { handle, MAIL_PER_HOUR } from '../index';
import { FROM, mailerFor, resendMailer, signInMail } from '../mail';
import { TEST_SECRET, turnstile } from '../turnstile';
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

  it('mails one email with both a link and a 6-digit code', async () => {
    const w = new World();
    expect((await w.client('192.0.2.30').askLink('eve@example.com')).status).toBe(200);
    expect(w.mail).toHaveLength(1);
    const [m] = w.mail;
    const code = w.code('eve@example.com');
    expect(code).toMatch(/^\d{6}$/);
    expect(m.subject).toBe(`Sign in to vxr: ${code}`);
    expect(w.link('eve@example.com')).toContain('/api/auth/magic-link/verify?token=');
  });

  it('signs in by email code, into the same account as the link', async () => {
    const w = new World();
    const a = w.client('192.0.2.12');
    expect((await a.codeSignIn('bea@example.com')).status).toBe(200);
    const me = (await a.me()).user;
    expect(me).toMatchObject({ email: 'bea@example.com' });

    // Signed out, the link from a second email finds the same account.
    await a.fetch('/api/auth/sign-out', { method: 'POST', json: {} });
    expect((await a.me()).user).toBeNull();
    await a.emailSignIn('bea@example.com');
    expect((await a.me()).user).toMatchObject({ id: me.id, email: 'bea@example.com' });

    // And a code on another device, from a third email, finds it again.
    const b = w.client('192.0.2.15');
    expect((await b.codeSignIn('bea@example.com')).status).toBe(200);
    expect((await b.me()).user.id).toBe(me.id);
  });

  it('takes a code once, only for its address, and only the latest one', async () => {
    const w = new World();
    const a = w.client('192.0.2.16');
    await a.askLink('fay@example.com');
    const first = w.code('fay@example.com');
    await a.askLink('fay@example.com');
    const code = w.code('fay@example.com');
    if (first !== code) expect((await a.enterCode('fay@example.com', first)).status).toBe(400);
    // Another address's code signs nobody in.
    await a.askLink('gus@example.com');
    expect((await a.enterCode('gus@example.com', code)).status).toBe(400);
    expect((await a.me()).user).toBeNull();

    expect((await a.enterCode('fay@example.com', code)).status).toBe(200);
    const b = w.client('192.0.2.17');
    const again = await b.enterCode('fay@example.com', code);
    expect(again.status).toBe(400);
    expect(await again.json()).toMatchObject({ code: 'INVALID_OTP' });
    expect((await b.me()).user).toBeNull();
  });

  it('drops a code after three wrong tries', async () => {
    const w = new World();
    const a = w.client('192.0.2.18');
    await a.askLink('hal@example.com');
    const code = w.code('hal@example.com');
    const wrong = code === '000000' ? '000001' : '000000';
    for (let i = 0; i < 3; i++) expect((await a.enterCode('hal@example.com', wrong)).status).toBe(400);
    const late = await a.enterCode('hal@example.com', code);
    expect(late.status).toBe(403);
    expect(await late.json()).toMatchObject({ code: 'TOO_MANY_ATTEMPTS' });
    expect((await a.me()).user).toBeNull();
  });

  it('expires a code with its link', async () => {
    const w = new World();
    const a = w.client('192.0.2.19');
    await a.askLink('ida@example.com');
    const code = w.code('ida@example.com');
    const past = new Date(Date.now() - 1000).toISOString();
    await env.DB.prepare('update verification set expiresAt = ?').bind(past).run();
    const res = await a.enterCode('ida@example.com', code);
    expect(res.status).toBe(400);
    expect(await res.json()).toMatchObject({ code: 'OTP_EXPIRED' });
    const link = new URL(w.link('ida@example.com'));
    await a.fetch(link.pathname + link.search);
    expect((await a.me()).user).toBeNull();
  });

  it('limits code tries per IP in the rate-limit table', async () => {
    const w = new World();
    const a = w.client('192.0.2.20');
    let last = 0;
    for (let i = 0; i <= 10 && last !== 429; i++) last = (await a.enterCode(`try${i}@example.com`, '123456')).status;
    expect(last).toBe(429);
    const { n } = (await env.DB.prepare('select count(*) as n from rateLimit where key like ?').bind('%/sign-in/email-otp').first<{ n: number }>())!;
    expect(n).toBeGreaterThan(0);
  });

  it('answers no passkey route, and no code route but sign-in', async () => {
    const w = new World();
    const a = w.client('192.0.2.21');
    for (const route of [
      'generate-register-options',
      'generate-authenticate-options',
      'verify-registration',
      'verify-authentication',
      'list-user-passkeys',
      'delete-passkey',
    ]) {
      expect((await a.fetch(`/api/auth/passkey/${route}`)).status, route).toBe(404);
      expect((await a.fetch(`/api/auth/passkey/${route}`, { method: 'POST', json: {} })).status, route).toBe(404);
    }
    // A code sent on its own would skip the Turnstile check and the mail limits.
    for (const route of OTP_OFF) {
      const res = await a.fetch(`/api/auth${route}`, { method: 'POST', json: { email: 'jo@example.com', type: 'sign-in' } });
      expect(res.status, route).toBe(404);
    }
    expect(w.mail).toHaveLength(0);
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
    expect(await a.me()).toMatchObject({ signIn: false, email: false });
    expect((await a.askLink('dee@example.com')).status).toBe(503);
  });

  it('prints links only for a local host', () => {
    expect(mailerFor({ EMAIL_SENDER: 'log' }, new URL('http://localhost:8787/'))).not.toBeNull();
    expect(mailerFor({ EMAIL_SENDER: 'log' }, new URL('https://vxr.jrnxf.co/'))).toBeNull();
    expect(mailerFor({}, new URL('http://localhost:8787/'))).toBeNull();
    expect(mailerFor({ RESEND_API_KEY: 're_test' }, new URL('https://vxr.jrnxf.co/'))).not.toBeNull();
  });

  it('sends a link through Resend', async () => {
    const seen: { url: string; init: RequestInit }[] = [];
    const answer = (status: number) =>
      (async (url: RequestInfo | URL, init?: RequestInit) => {
        seen.push({ url: String(url), init: init! });
        return new Response('{}', { status });
      }) as typeof fetch;
    await resendMailer('re_test', answer(200)).send(signInMail('dee@example.com', 'https://example.com/link', '123456'));
    expect(seen[0].url).toBe('https://api.resend.com/emails');
    expect(new Headers(seen[0].init.headers).get('authorization')).toBe('Bearer re_test');
    const body = JSON.parse(String(seen[0].init.body));
    expect(body).toMatchObject({ from: FROM, to: ['dee@example.com'], subject: 'Sign in to vxr: 123456' });
    expect(body.text).toContain('https://example.com/link');
    expect(body.text).toContain('\n123456\n');
    await expect(resendMailer('re_test', answer(422)).send(signInMail('dee@example.com', 'x', '123456'))).rejects.toThrow('422');
  });

  it('checks Turnstile tokens with siteverify, solved on this site', async () => {
    const seen: FormData[] = [];
    const fake = (ok: boolean, hostname = 'vxr.example.com') =>
      (async (_url: RequestInfo | URL, init?: RequestInit) => {
        seen.push(init!.body as FormData);
        return Response.json({ success: ok, hostname });
      }) as typeof fetch;
    expect(await turnstile('secret', fake(true))('token', '192.0.2.1', 'vxr.example.com')).toBe(true);
    expect(seen[0].get('secret')).toBe('secret');
    expect(seen[0].get('response')).toBe('token');
    expect(seen[0].get('remoteip')).toBe('192.0.2.1');
    expect(await turnstile('secret', fake(false))('token', null, 'vxr.example.com')).toBe(false);
    expect(await turnstile('secret', fake(true))('', null, 'vxr.example.com')).toBe(false);
    // A token solved on another site that uses the same site key.
    expect(await turnstile('secret', fake(true, 'other.example'))('token', null, 'vxr.example.com')).toBe(false);
    // The test keys' tokens belong to no site, for `wrangler dev`.
    expect(await turnstile(TEST_SECRET, fake(true, 'example.com'))('token', null, 'localhost')).toBe(true);
  });

  it('serves the account page with its own content security policy', async () => {
    const page = new Response('<!doctype html>', { headers: { 'content-security-policy': "default-src 'none'" } });
    const res = await handle(new Request(`${ORIGIN}/account`), { ...env, ASSETS: { fetch: async () => page } as unknown as Fetcher });
    expect(res.headers.get('content-security-policy')).toContain('https://challenges.cloudflare.com');
  });
});
