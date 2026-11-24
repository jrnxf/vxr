// The vxr Worker. It runs only for /api/* and /account (wrangler.jsonc's run_worker_first);
// every other path is the static editor, served by Cloudflare without it.
//
//   /api/me          whether sign-in is on, who is signed in, their bytes used and the limits
//   /api/auth/*      Better Auth (worker/auth.ts): emailed links and codes, sessions
//   /api/projects*   the account's cloud projects (worker/projects.ts)
//   /api/account     deleting the account and everything it keeps (worker/account.ts)
//   /account         the sign-in page (site/src/account/, built into dist/account.html)
import { deleteAccount } from './account';
import { type Auth, fromApp, makeAuth } from './auth';
import { HSTS } from './headers';
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
  /** In place of MAIL_PER_DAY. */
  mailPerDay?: number;
}

/** Sign-in emails one address, and one IP, may ask for in an hour. */
export const MAIL_PER_HOUR = { email: 5, ip: 20 };

/**
 * The desktop app's tighter limits on top: it has no Turnstile, so one IP may ask for fewer, and
 * all app requests together have a ceiling, which caps what a script posing as the app can send.
 */
export const APP_MAIL_PER_HOUR = { ip: 5, all: 200 };

/**
 * Every sign-in email of a day, for all addresses together. Past it no email goes out (it fails
 * closed) and the Worker logs an error, so a flood cannot spend the sender's quota or its good
 * name on mail to strangers.
 */
const MAIL_PER_DAY = 1000;

/** The address the email limits count: without its +tag, so `a+1@x` and `a+2@x` are one. */
export function mailKey(email: string): string {
  const at = email.lastIndexOf('@');
  const plus = email.indexOf('+');
  return plus > 0 && plus < at ? email.slice(0, plus) + email.slice(at) : email;
}

/**
 * The IP the email limits count. An IPv6 host is given a whole /64 (often a /56), so its
 * addresses count as one by their first 64 bits; an IPv4 address counts as it is.
 */
export function ipKey(address: string): string {
  if (!address.includes(':')) return address;
  const [head, tail] = address.toLowerCase().split('::');
  const first = head ? head.split(':') : [];
  const last = tail ? tail.split(':') : [];
  // An IPv4 address written as IPv6 (::ffff:192.0.2.1).
  const mapped = (last.at(-1) ?? first.at(-1) ?? '').includes('.');
  if (mapped) return (last.at(-1) ?? first.at(-1))!;
  const groups = tail === undefined ? first : [...first, ...Array<string>(Math.max(0, 8 - first.length - last.length)).fill('0'), ...last];
  return `${groups
    .slice(0, 4)
    .map((g) => (parseInt(g, 16) || 0).toString(16))
    .join(':')}::/64`;
}

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

const DEFAULT: Deps = {};

/**
 * Better Auth for this request, or null when sign-in is off. It is made anew for every request and
 * never kept: the object holds this request's D1 handle and one promise for its own start-up, and
 * a Worker must not use either from a later request (a promise made in a request that ended early
 * never settles, and every later request in that isolate then fails).
 */
function authFor(env: Env, url: URL, deps: Deps): Auth | null {
  const secret = env.BETTER_AUTH_SECRET;
  if (!secret || secret.length < 32) return null;
  return makeAuth(url.origin, secret, env.DB, (deps.mailer ?? mailerFor)(env, url));
}

function verifier(env: Env, deps: Deps): Verify | null {
  if (deps.verify) return deps.verify(env);
  return env.TURNSTILE_SECRET_KEY ? turnstile(env.TURNSTILE_SECRET_KEY) : null;
}

const ip = (req: Request) => req.headers.get('cf-connecting-ip');
const count = (r: D1Result<{ n: number }>) => r.results[0]?.n ?? 0;

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
    email = String(((await req.clone().json()) as { email?: unknown }).email ?? '')
      .trim()
      .toLowerCase();
  } catch {
    return problem(400, 'bad_body');
  }
  if (!email) return problem(400, 'bad_email');
  const now = Date.now();
  const hour = now - 3600_000;
  const from = ipKey(ip(req) ?? '');
  const key = mailKey(email);
  const [byEmail, byIp, byAppIp, byApp, byDay] = await env.DB.batch<{ n: number }>([
    env.DB.prepare('select count(*) as n from mail_log where email = ? and at > ?').bind(key, hour),
    env.DB.prepare('select count(*) as n from mail_log where ip = ? and at > ?').bind(from, hour),
    env.DB.prepare('select count(*) as n from mail_log where ip = ? and app = 1 and at > ?').bind(from, hour),
    env.DB.prepare('select count(*) as n from mail_log where app = 1 and at > ?').bind(hour),
    env.DB.prepare('select count(*) as n from mail_log where at > ?').bind(now - 86400_000),
  ]);
  if (count(byDay) >= (deps.mailPerDay ?? MAIL_PER_DAY)) {
    console.error('vxr worker: the daily cap on sign-in emails is reached, no email goes out');
    return problem(503, 'mail_cap');
  }
  if (count(byEmail) >= MAIL_PER_HOUR.email) return problem(429, 'too_many_links');
  if (from && count(byIp) >= MAIL_PER_HOUR.ip) return problem(429, 'too_many_links');
  if (app && (!from || count(byAppIp) >= APP_MAIL_PER_HOUR.ip)) return problem(429, 'too_many_links');
  if (app && count(byApp) >= APP_MAIL_PER_HOUR.all) return problem(429, 'too_many_links');
  await env.DB.batch([
    env.DB.prepare('insert into mail_log (email, ip, at, app) values (?, ?, ?, ?)').bind(key, from, now, app ? 1 : 0),
    env.DB.prepare('delete from mail_log where at < ?').bind(now - 86400_000),
  ]);
  return null;
}

/**
 * The response without the session token: not in the bearer plugin's `set-auth-token` header and
 * not in a JSON body (`token` of a sign-in, `session.token` of get-session). The token is for the
 * app's code sign-in only, and on the web it stays in its httpOnly cookie, out of reach of the
 * page's scripts.
 */
async function withoutToken(res: Response): Promise<Response> {
  let out = new Response(res.body, res);
  if ((out.headers.get('content-type') ?? '').includes('application/json')) {
    const text = await out.text();
    let body: unknown = null;
    try {
      body = JSON.parse(text);
    } catch {
      // Not JSON after all: it goes out as it came.
    }
    const top = body as { token?: unknown; session?: { token?: unknown } | null } | null;
    const holds = top !== null && typeof top === 'object' && ('token' in top || (top.session != null && 'token' in top.session));
    if (holds) {
      delete top.token;
      if (top.session) delete top.session.token;
    }
    out.headers.delete('content-length');
    out = new Response(holds ? JSON.stringify(top) : text, out);
  }
  if (!out.headers.has('set-auth-token')) return out;
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

async function route(req: Request, env: Env, deps: Deps): Promise<Response> {
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
    // A sign-in link cut short in an email: the account page says what to do, not raw JSON.
    if (req.method === 'GET' && path === '/api/auth/magic-link/verify' && !url.searchParams.get('token'))
      return Response.redirect(new URL('/account?link=expired', url).href, 302);
    // The app reads its token only from its code sign-in; every other answer drops it.
    const keep = fromApp(req) && path === '/api/auth/sign-in/email-otp';
    return keep ? auth.handler(req) : withoutToken(await auth.handler(req));
  }
  if (path === '/api/projects' || path.startsWith('/api/projects/')) {
    if (req.method !== 'GET' && req.method !== 'HEAD' && foreign(req, url)) return problem(403, 'origin');
    const user = await who(auth, req);
    if (!user) return problem(401, 'signed_out');
    const rest = path.slice('/api/projects'.length).split('/').filter(Boolean);
    return projects(req, env, user.id, rest, limits);
  }
  if (path === '/api/account') {
    if (foreign(req, url)) return problem(403, 'origin');
    const user = await who(auth, req);
    if (!user) return problem(401, 'signed_out');
    return deleteAccount(req, env, auth, user);
  }
  return problem(404, 'not_found');
}

/**
 * Answers one request. Anything thrown on the way is logged (Workers Logs keep it) and answered as
 * a JSON 500, so a caller always gets a problem it can read, not Cloudflare's error page.
 */
export async function handle(req: Request, env: Env, deps: Deps = DEFAULT): Promise<Response> {
  let res: Response;
  try {
    res = await route(req, env, deps);
  } catch (e) {
    const path = new URL(req.url).pathname;
    console.error(`vxr worker: ${req.method} ${path} threw`, e instanceof Error ? (e.stack ?? e.message) : e);
    res = problem(500, 'server');
  }
  // A WebSocket upgrade carries its socket, which a copy would drop.
  if (res.status === 101) return res;
  const out = new Response(res.body, res);
  out.headers.set('strict-transport-security', HSTS);
  return out;
}

export default {
  fetch(req, env) {
    return handle(req, env);
  },
} satisfies ExportedHandler<Env>;
