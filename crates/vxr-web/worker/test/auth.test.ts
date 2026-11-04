import { env } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { OTP_OFF } from '../auth';
import { APP_MAIL_PER_HOUR, handle, MAIL_PER_HOUR } from '../index';
import { FROM, mailerFor, resendMailer, signInMail } from '../mail';
import { TEST_SECRET, turnstile } from '../turnstile';
import { ORIGIN, vox, World } from './helpers';

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

  it('signs the app in by code, with no challenge, and keeps it in by bearer token', async () => {
    const w = new World();
    const app = w.app();
    expect((await app.askCode('fay@example.com')).status).toBe(200);
    // The app's email has only the code: a link would sign in a browser, not the app.
    const [m] = w.mail;
    expect(m.text).not.toMatch(/https?:\/\//);
    expect(m.text).toContain('vxr app');
    const code = w.code('fay@example.com');
    expect(m.subject).toBe(`Sign in to vxr: ${code}`);

    const res = await app.fetch('/api/auth/sign-in/email-otp', { method: 'POST', json: { email: 'fay@example.com', otp: code } });
    expect(res.status).toBe(200);
    expect(app.token).toMatch(/\./);
    expect((await app.me()).user).toMatchObject({ email: 'fay@example.com' });
    // The token reaches the projects too.
    expect((await app.json('/api/projects')).status).toBe(200);
    const put = await app.json('/api/projects/p1', { method: 'PUT', headers: { 'if-none-match': '*', 'x-vxr-name': 'Island' }, body: vox() });
    expect(put.status).toBe(201);
  });

  it('revokes the app session on sign-out', async () => {
    const w = new World();
    const app = w.app('192.0.2.91');
    await app.codeSignIn('gus@example.com');
    const token = app.token;
    expect((await app.fetch('/api/auth/sign-out', { method: 'POST', json: {} })).status).toBe(200);
    // The same token, kept by someone else, no longer signs anybody in.
    app.token = token;
    expect((await app.me()).user).toBeNull();
    expect((await app.json('/api/projects')).status).toBe(401);
  });

  it('takes only signed tokens', async () => {
    const w = new World();
    const app = w.app('192.0.2.92');
    await app.codeSignIn('hal@example.com');
    const [raw] = app.token!.split('.');
    app.token = raw;
    expect((await app.me()).user).toBeNull();
    app.token = `${raw}.forged`;
    expect((await app.me()).user).toBeNull();
  });

  it('never shows the session token to a web page', async () => {
    const w = new World();
    const a = w.client('192.0.2.15');
    const res = await a.codeSignIn('ida@example.com');
    expect(res.status).toBe(200);
    expect(res.headers.get('set-auth-token')).toBeNull();
    expect(res.headers.get('access-control-expose-headers') ?? '').not.toMatch(/set-auth-token/i);
    expect((await a.me()).user).toMatchObject({ email: 'ida@example.com' });

    const b = w.client('192.0.2.16');
    const link = await b.emailSignIn('jan@example.com');
    expect(link.headers.get('set-auth-token')).toBeNull();

    // A same-origin GET carries no Origin and needs no preflight, so page script can send the
    // app's header with it. A session past updateAge is refreshed, which sets the cookie again,
    // and the token must not ride along.
    const soon = new Date(Date.now() + 58 * 24 * 60 * 60 * 1000).toISOString();
    await env.DB.prepare('update session set expiresAt = ?').bind(soon).run();
    const refreshed = await handle(
      new Request(`${ORIGIN}/api/auth/get-session`, {
        headers: {
          'x-vxr-client': 'app',
          'cf-connecting-ip': '192.0.2.15',
          cookie: [...a.jar].map(([k, v]) => `${k}=${v}`).join('; '),
        },
      }),
      env,
      w.deps,
    );
    expect(refreshed.status).toBe(200);
    expect(refreshed.headers.getSetCookie().join('\n')).toMatch(/session_token/);
    expect(refreshed.headers.get('set-auth-token')).toBeNull();
  });

  it('refuses the app path from a browser, which always sends Origin', async () => {
    const w = new World();
    const a = w.client('192.0.2.17');
    const res = await a.fetch('/api/auth/sign-in/magic-link', { method: 'POST', headers: { 'x-vxr-client': 'app' }, json: { email: 'kim@example.com' } });
    expect(res.status).toBe(403);
    expect(await res.json()).toMatchObject({ error: 'origin' });
    // And without the app header, a request with no Origin still needs the challenge.
    const bare = await handle(
      new Request(`${ORIGIN}/api/auth/sign-in/magic-link`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'cf-connecting-ip': '192.0.2.18' },
        body: JSON.stringify({ email: 'kim@example.com' }),
      }),
      env,
      w.deps,
    );
    expect(bare.status).toBe(403);
    expect(w.mail).toHaveLength(0);
  });

  it('limits app codes per address, per IP and for all app requests', async () => {
    const w = new World();
    // Per address, shared with the web: the web's links count too.
    await w.client('198.51.100.50').askLink('lou@example.com');
    for (let i = 1; i < MAIL_PER_HOUR.email; i++) expect((await w.app(`198.51.100.${50 + i}`).askCode('lou@example.com')).status).toBe(200);
    expect((await w.app('198.51.100.60').askCode('lou@example.com')).status).toBe(429);

    // Per IP, tighter than the web's.
    const one = w.app('198.51.100.70');
    let last = 200;
    let sent = 0;
    for (let i = 0; i <= APP_MAIL_PER_HOUR.ip && last === 200; i++) {
      last = (await one.askCode(`ip${i}@example.com`)).status;
      if (last === 200) sent++;
    }
    expect(last).toBe(429);
    expect(sent).toBeLessThanOrEqual(APP_MAIL_PER_HOUR.ip);

    // All app requests together, whatever the IP: the web is not held up by it.
    const at = Date.now();
    await env.DB.batch(
      Array.from({ length: APP_MAIL_PER_HOUR.all }, (_, i) =>
        env.DB.prepare('insert into mail_log (email, ip, at, app) values (?, ?, ?, 1)').bind(`all${i}@example.com`, `203.0.113.${i % 250}`, at),
      ),
    );
    expect((await w.app('198.51.100.80').askCode('mo@example.com')).status).toBe(429);
    expect((await w.client('198.51.100.81').askLink('mo@example.com')).status).toBe(200);
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
