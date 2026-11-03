// The vxr Worker. It runs only for /api/* and /account (wrangler.jsonc's run_worker_first);
// every other path is the static editor, served by Cloudflare without it.
//
//   /api/me          whether sign-in is on, who is signed in, their bytes used and the limits
//   /api/auth/*      Better Auth (worker/auth.ts): passkeys, email links, sessions
//   /api/projects*   the account's cloud projects (worker/projects.ts)
//   /account         the sign-in page (account/, built into dist/account.html)
import { type Auth, makeAuth, NO_EMAIL } from './auth';
import { type Mailer, mailerFor } from './mail';
import { LIMITS, type Limits, json, problem, projects, usage } from './projects';
import { turnstile, type Verify } from './turnstile';

export interface Env {
  ASSETS: Fetcher;
  DB: D1Database;
  PROJECTS: R2Bucket;
  /** Signs sessions (a Worker secret). Without it sign-in is off. */
  BETTER_AUTH_SECRET?: string;
  /** Checks the Turnstile challenge on sign-up and email links (a Worker secret). */
  TURNSTILE_SECRET_KEY?: string;
  /** `log` prints sign-in links in `wrangler dev`; unset, email sign-in is off. */
  EMAIL_SENDER?: string;
}

/** What the tests replace: the email sender, the Turnstile check and the limits. */
export interface Deps {
  mailer?: (env: Env, url: URL) => Mailer | null;
  verify?: (env: Env) => Verify | null;
  limits?: Limits;
}

/** Sign-in links one address, and one IP, may ask for in an hour. */
export const MAIL_PER_HOUR = { email: 5, ip: 20 };

// The account page's policy: the editor's (www/_headers) plus Turnstile's script and frame.
const ACCOUNT_CSP = [
  "default-src 'none'",
  "script-src 'self' https://challenges.cloudflare.com",
  'frame-src https://challenges.cloudflare.com',
  "connect-src 'self'",
  "style-src 'self'",
  "img-src 'self' data:",
  "font-src 'self'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join('; ');

const auths = new WeakMap<Deps, Map<string, Auth>>();
const DEFAULT: Deps = {};

function authFor(env: Env, url: URL, deps: Deps): Auth | null {
  const secret = env.BETTER_AUTH_SECRET;
  if (!secret || secret.length < 32) return null;
  let byOrigin = auths.get(deps);
  if (!byOrigin) auths.set(deps, (byOrigin = new Map()));
  const key = `${url.origin} ${secret}`;
  let auth = byOrigin.get(key);
  if (!auth) {
    const mailer = (deps.mailer ?? mailerFor)(env, url);
    auth = makeAuth(url.origin, secret, env.DB, mailer);
    byOrigin.set(key, auth);
  }
  return auth;
}

function verifier(env: Env, deps: Deps): Verify | null {
  if (deps.verify) return deps.verify(env);
  return env.TURNSTILE_SECRET_KEY ? turnstile(env.TURNSTILE_SECRET_KEY) : null;
}

const ip = (req: Request) => req.headers.get('cf-connecting-ip');

/** The signed-in user, or null. */
async function who(auth: Auth, req: Request) {
  const s = await auth.api.getSession({ headers: req.headers });
  return s?.user ?? null;
}

/** Turnstile and the per-address limit before Better Auth sends an email link. */
async function emailLink(req: Request, env: Env, url: URL, deps: Deps): Promise<Response | null> {
  if (!(deps.mailer ?? mailerFor)(env, url)) return problem(503, 'email_off');
  const verify = verifier(env, deps);
  if (!verify) return problem(503, 'sign_up_off');
  if (!(await verify(req.headers.get('x-turnstile-token') ?? '', ip(req)))) return problem(403, 'challenge');
  let email = '';
  try {
    email = String(((await req.clone().json()) as { email?: unknown }).email ?? '').trim().toLowerCase();
  } catch {
    return problem(400, 'bad_body');
  }
  if (!email || email.endsWith(NO_EMAIL)) return problem(400, 'bad_email');
  const now = Date.now();
  const hour = now - 3600_000;
  const from = ip(req) ?? '';
  const [byEmail, byIp] = await env.DB.batch<{ n: number }>([
    env.DB.prepare('select count(*) as n from mail_log where email = ? and at > ?').bind(email, hour),
    env.DB.prepare('select count(*) as n from mail_log where ip = ? and at > ?').bind(from, hour),
  ]);
  if ((byEmail.results[0]?.n ?? 0) >= MAIL_PER_HOUR.email) return problem(429, 'too_many_links');
  if (from && (byIp.results[0]?.n ?? 0) >= MAIL_PER_HOUR.ip) return problem(429, 'too_many_links');
  await env.DB.batch([
    env.DB.prepare('insert into mail_log (email, ip, at) values (?, ?, ?)').bind(email, from, now),
    env.DB.prepare('delete from mail_log where at < ?').bind(now - 86400_000),
  ]);
  return null;
}

/** Turnstile before a passkey sign-up (adding a passkey to a signed-in account needs none). */
async function passkeySignUp(req: Request, env: Env, auth: Auth, deps: Deps): Promise<Response | null> {
  if (await who(auth, req)) return null;
  const verify = verifier(env, deps);
  if (!verify) return problem(503, 'sign_up_off');
  if (!(await verify(req.headers.get('x-turnstile-token') ?? '', ip(req)))) return problem(403, 'challenge');
  return null;
}

/** A write from another site (the cookie is same-site, so this is belt and braces). */
function foreign(req: Request, url: URL): boolean {
  const origin = req.headers.get('origin');
  return origin !== null && origin !== url.origin;
}

export async function handle(req: Request, env: Env, deps: Deps = DEFAULT): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname;
  if (path === '/account') {
    const page = await env.ASSETS.fetch(new Request(new URL('/account', url), req));
    const res = new Response(page.body, page);
    res.headers.set('content-security-policy', ACCOUNT_CSP);
    res.headers.set('cache-control', 'no-cache');
    return res;
  }
  if (!path.startsWith('/api/')) return env.ASSETS.fetch(req);
  const auth = authFor(env, url, deps);
  const limits = deps.limits ?? LIMITS;
  if (path === '/api/me') {
    if (!auth) return json({ signIn: false, email: false, user: null, limits });
    const user = await who(auth, req);
    return json({
      signIn: true,
      email: (deps.mailer ?? mailerFor)(env, url) !== null,
      user: user && {
        id: user.id,
        email: user.email.endsWith(NO_EMAIL) ? null : user.email,
        name: user.name,
      },
      usage: user ? await usage(env.DB, user.id) : 0,
      limits,
    });
  }
  if (!auth) return problem(503, 'sign_in_off');
  if (path.startsWith('/api/auth/')) {
    if (req.method === 'POST' && path === '/api/auth/sign-in/magic-link') {
      const stop = await emailLink(req, env, url, deps);
      if (stop) return stop;
    }
    // Checked where the account is made, since the client sends headers only with this call.
    if (req.method === 'POST' && path === '/api/auth/passkey/verify-registration') {
      const stop = await passkeySignUp(req, env, auth, deps);
      if (stop) return stop;
    }
    return auth.handler(req);
  }
  if (path === '/api/projects' || path.startsWith('/api/projects/')) {
    if (req.method !== 'GET' && req.method !== 'HEAD' && foreign(req, url)) return problem(403, 'origin');
    const user = await who(auth, req);
    if (!user) return problem(401, 'signed_out');
    const rest = path.slice('/api/projects'.length).split('/').filter(Boolean);
    return projects(req, env, user.id, rest, limits);
  }
  return problem(404, 'not_found');
}

export default {
  fetch(req, env) {
    return handle(req, env);
  },
} satisfies ExportedHandler<Env>;
