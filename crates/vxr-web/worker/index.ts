// The vxr Worker. It runs only for /api/* and /account (wrangler.jsonc's run_worker_first);
// every other path is the static editor, served by Cloudflare without it.
//
//   /api/me          whether sign-in is on, who is signed in, their bytes used and the limits
//   /api/auth/*      Better Auth (worker/auth.ts): emailed links and codes, sessions
//   /api/projects*   the account's cloud projects (worker/projects.ts)
//   /account         the sign-in page (site/src/account/, built into dist/account.html)
import { type Auth, fromApp, makeAuth } from './auth';
import { type Mailer, mailerFor } from './mail';
import { LIMITS, type Limits, json, problem, projects, usage } from './projects';
import { turnstile, type Verify } from './turnstile';

export interface Env {
  ASSETS: Fetcher;
  DB: D1Database;
  PROJECTS: R2Bucket;
  /** Signs sessions (a Worker secret). Without it sign-in is off. */
  BETTER_AUTH_SECRET?: string;
  /** Checks the Turnstile challenge before a sign-in email (a Worker secret). */
  TURNSTILE_SECRET_KEY?: string;
  /** `log` prints sign-in emails in `wrangler dev` instead of sending them. */
  EMAIL_SENDER?: string;
  /** Sends sign-in emails through Resend (a Worker secret). Without it sign-in is off. */
  RESEND_API_KEY?: string;
}

/** What the tests replace: the email sender, the Turnstile check and the limits. */
export interface Deps {
  mailer?: (env: Env, url: URL) => Mailer | null;
  verify?: (env: Env) => Verify | null;
  limits?: Limits;
}

/** Sign-in emails one address, and one IP, may ask for in an hour. */
export const MAIL_PER_HOUR = { email: 5, ip: 20 };

/**
 * The desktop app's tighter limits on top: it has no Turnstile, so one IP may ask for fewer, and
 * all app requests together have a ceiling, which caps what a script posing as the app can send.
 */
export const APP_MAIL_PER_HOUR = { ip: 5, all: 200 };

// The account page's policy: Turnstile's script and frame, and otherwise only this site.
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

/**
 * Turnstile and the per-address and per-IP limits before Better Auth sends a sign-in email (a link
 * and a code). The desktop app has no page to solve Turnstile on, so its requests (no Origin, see
 * `fromApp`) skip it and get the tighter APP_MAIL_PER_HOUR limits instead; a request that says it
 * is the app but carries an Origin came from a browser and is refused.
 */
async function emailLink(req: Request, env: Env, url: URL, deps: Deps): Promise<Response | null> {
  if (!(deps.mailer ?? mailerFor)(env, url)) return problem(503, 'email_off');
  const app = fromApp(req);
  if (!app && req.headers.get('x-vxr-client') === 'app') return problem(403, 'origin');
  const verify = verifier(env, deps);
  if (!verify) return problem(503, 'sign_up_off');
  if (!app && !(await verify(req.headers.get('x-turnstile-token') ?? '', ip(req), url.hostname))) return problem(403, 'challenge');
  let email = '';
  try {
    email = String(((await req.clone().json()) as { email?: unknown }).email ?? '').trim().toLowerCase();
  } catch {
    return problem(400, 'bad_body');
  }
  if (!email) return problem(400, 'bad_email');
  const now = Date.now();
  const hour = now - 3600_000;
  const from = ip(req) ?? '';
  const [byEmail, byIp, byAppIp, byApp] = await env.DB.batch<{ n: number }>([
    env.DB.prepare('select count(*) as n from mail_log where email = ? and at > ?').bind(email, hour),
    env.DB.prepare('select count(*) as n from mail_log where ip = ? and at > ?').bind(from, hour),
    env.DB.prepare('select count(*) as n from mail_log where ip = ? and app = 1 and at > ?').bind(from, hour),
    env.DB.prepare('select count(*) as n from mail_log where app = 1 and at > ?').bind(hour),
  ]);
  const count = (r: D1Result<{ n: number }>) => r.results[0]?.n ?? 0;
  if (count(byEmail) >= MAIL_PER_HOUR.email) return problem(429, 'too_many_links');
  if (from && count(byIp) >= MAIL_PER_HOUR.ip) return problem(429, 'too_many_links');
  if (app && (!from || count(byAppIp) >= APP_MAIL_PER_HOUR.ip)) return problem(429, 'too_many_links');
  if (app && count(byApp) >= APP_MAIL_PER_HOUR.all) return problem(429, 'too_many_links');
  await env.DB.batch([
    env.DB.prepare('insert into mail_log (email, ip, at, app) values (?, ?, ?, ?)').bind(email, from, now, app ? 1 : 0),
    env.DB.prepare('delete from mail_log where at < ?').bind(now - 86400_000),
  ]);
  return null;
}

/**
 * The response without the bearer plugin's `set-auth-token`: the session token is for the app
 * only, and on the web it stays in its httpOnly cookie, out of reach of the page's scripts.
 */
function withoutToken(res: Response): Response {
  if (!res.headers.has('set-auth-token')) return res;
  const out = new Response(res.body, res);
  out.headers.delete('set-auth-token');
  const exposed = (out.headers.get('access-control-expose-headers') ?? '')
    .split(',')
    .map((h) => h.trim())
    .filter((h) => h && h.toLowerCase() !== 'set-auth-token');
  if (exposed.length) out.headers.set('access-control-expose-headers', exposed.join(', '));
  else out.headers.delete('access-control-expose-headers');
  return out;
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
    const email = (deps.mailer ?? mailerFor)(env, url) !== null;
    return json({
      // Email is the only way in: without a sender only a session already made stays signed in.
      signIn: email || user !== null,
      email,
      user: user && {
        id: user.id,
        email: user.email,
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
    return fromApp(req) ? auth.handler(req) : withoutToken(await auth.handler(req));
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
