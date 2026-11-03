// The account page: sign in with a passkey (or make an account with one), or by an emailed link,
// through Better Auth's client; the Worker (worker/index.ts) checks the Turnstile token sent as
// the x-turnstile-token header before it makes an account or sends a link. The editor opens this
// page with ?from=editor and checks /api/me again when its tab comes back.
import { passkeyClient } from '@better-auth/passkey/client';
import { createAuthClient } from 'better-auth/client';
import { magicLinkClient } from 'better-auth/client/plugins';

/** The Turnstile site key, set by the build (TURNSTILE_SITE_KEY; Cloudflare's always-pass test key by default). */
declare const TURNSTILE_SITE_KEY: string;

interface Turnstile {
  render(el: HTMLElement, opts: Record<string, unknown>): string;
  getResponse(id: string): string | undefined;
  reset(id: string): void;
}
declare global {
  interface Window {
    turnstile?: Turnstile;
  }
}

interface Me {
  signIn: boolean;
  email: boolean;
  user: { id: string; email: string | null; name: string } | null;
  usage?: number;
  limits: { account: number; file: number };
}

const auth = createAuthClient({
  baseURL: location.origin,
  basePath: '/api/auth',
  plugins: [passkeyClient(), magicLinkClient()],
});

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const params = new URLSearchParams(location.search);
const back = params.get('from') === 'editor' ? '/account?from=editor' : '/account';

function show(id: 'loading' | 'off' | 'out' | 'sent' | 'in') {
  for (const s of document.querySelectorAll('main > section')) (s as HTMLElement).hidden = s.id !== id;
}

function say(text: string, error = false) {
  const m = $('msg');
  m.textContent = text;
  m.classList.toggle('error', error);
}

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  const mb = bytes / (1024 * 1024);
  return `${mb >= 10 || Number.isInteger(mb) ? Math.round(mb) : mb.toFixed(1)} MB`;
}

// What went wrong, in words, from a Worker problem ({error}) or a Better Auth error ({code, message}).
function reason(err: { status?: number; error?: unknown; code?: unknown; message?: unknown } | null): string {
  const code = String(err?.error ?? err?.code ?? '');
  switch (code) {
    case 'challenge':
      return 'The check did not pass. Try again.';
    case 'too_many_links':
      return 'Too many sign-in links asked for. Try again in an hour.';
    case 'email_off':
      return 'Email sign-in is off here. Use a passkey.';
    case 'sign_up_off':
    case 'sign_in_off':
      return 'Sign-in is off here.';
    case 'bad_email':
      return 'That email address does not look right.';
    case 'AUTH_CANCELLED':
    case 'ERROR_CEREMONY_ABORTED':
      return 'Passkey cancelled.';
  }
  if (err?.status === 429) return 'Too many tries. Wait a minute and try again.';
  return typeof err?.message === 'string' && err.message ? err.message : 'Something went wrong. Try again.';
}

// One Turnstile widget for the two actions that need it: a new passkey account and an email link.
let widget: string | null = null;
let ready: Promise<Turnstile> | null = null;
function turnstileJs(): Promise<Turnstile> {
  ready ??= new Promise((resolve, reject) => {
    if (window.turnstile) return resolve(window.turnstile);
    const s = $('turnstile-js');
    s.addEventListener('load', () => (window.turnstile ? resolve(window.turnstile) : reject(new Error('no turnstile'))));
    s.addEventListener('error', () => reject(new Error('turnstile did not load')));
  });
  return ready;
}
async function renderChallenge() {
  try {
    const t = await turnstileJs();
    widget ??= t.render($('challenge'), {
      sitekey: TURNSTILE_SITE_KEY,
      appearance: 'interaction-only',
      theme: 'auto',
      size: 'flexible',
      action: 'account',
    });
  } catch {
    say('The sign-up check could not load. Passkey sign-in still works.', true);
  }
}
/** A fresh token, or null (and a message) when the check has not passed yet. */
async function token(): Promise<string | null> {
  const t = await turnstileJs().catch(() => null);
  const value = t && widget ? t.getResponse(widget) : undefined;
  if (!value) {
    say('Finish the check below first.', true);
    return null;
  }
  return value;
}
function spent() {
  if (window.turnstile && widget) window.turnstile.reset(widget);
}

async function busy(button: HTMLButtonElement, run: () => Promise<void>) {
  const buttons = document.querySelectorAll('button');
  for (const b of buttons) b.disabled = true;
  say('');
  try {
    await run();
  } finally {
    for (const b of buttons) b.disabled = false;
    button.focus();
  }
}

async function me(): Promise<Me | null> {
  try {
    const res = await fetch('/api/me', { credentials: 'same-origin', cache: 'no-store' });
    return res.ok ? ((await res.json()) as Me) : null;
  } catch {
    return null;
  }
}

async function refresh() {
  const m = await me();
  if (!m || !m.signIn) return show('off');
  for (const el of document.querySelectorAll<HTMLElement>('[data-limit]')) {
    el.textContent = size(m.limits[el.dataset.limit as 'account' | 'file']);
  }
  if (!m.user) {
    $('email-form').hidden = !m.email;
    show('out');
    void renderChallenge();
    return;
  }
  $('who').textContent = m.user.email ? `As ${m.user.email}.` : 'With a passkey, and no email.';
  const used = m.usage ?? 0;
  $('usage').textContent = size(used);
  $('usage-bar').style.width = `${Math.min(100, (used / m.limits.account) * 100)}%`;
  $('return').hidden = params.get('from') !== 'editor';
  show('in');
}

$('passkey-in').addEventListener('click', (e) =>
  busy(e.currentTarget as HTMLButtonElement, async () => {
    const { error } = await auth.signIn.passkey();
    if (error) return say(reason(error), true);
    await refresh();
  }),
);

$('passkey-up').addEventListener('click', (e) =>
  busy(e.currentTarget as HTMLButtonElement, async () => {
    const t = await token();
    if (!t) return;
    const { error } = await auth.passkey.addPasskey({
      name: 'vxr',
      createSession: true,
      fetchOptions: { headers: { 'x-turnstile-token': t } },
    });
    spent();
    if (error) return say(reason(error), true);
    await refresh();
  }),
);

$('passkey-add').addEventListener('click', (e) =>
  busy(e.currentTarget as HTMLButtonElement, async () => {
    const { error } = await auth.passkey.addPasskey({ name: 'vxr' });
    if (error) return say(reason(error), true);
    say('Passkey added.');
  }),
);

$('email-form').addEventListener('submit', (e) => {
  e.preventDefault();
  const button = (e.currentTarget as HTMLFormElement).querySelector('button')!;
  void busy(button, async () => {
    const email = $<HTMLInputElement>('email').value.trim();
    const t = await token();
    if (!t) return;
    const { error } = await auth.signIn.magicLink({
      email,
      callbackURL: back,
      errorCallbackURL: `${back}${back.includes('?') ? '&' : '?'}link=expired`,
      fetchOptions: { headers: { 'x-turnstile-token': t } },
    });
    spent();
    if (error) return say(reason(error), true);
    $('sent-to').textContent = email;
    show('sent');
  });
});

$('sent-back').addEventListener('click', () => {
  say('');
  show('out');
});

$('sign-out').addEventListener('click', (e) =>
  busy(e.currentTarget as HTMLButtonElement, async () => {
    await auth.signOut();
    await refresh();
    say('Signed out. Projects stay in this browser.');
  }),
);

if (params.has('link') || params.has('error')) say('That sign-in link has expired or was used. Ask for a new one.', true);
void refresh();
