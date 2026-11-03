// Cloudflare Turnstile, the challenge on sign-up and on asking for an email link. The page gets a
// token from the widget and sends it as the x-turnstile-token header; the Worker checks it with
// Cloudflare's siteverify, and that it was solved on this site (`host`, the request's host name).
// Dev and tests use Cloudflare's documented test keys, whose tokens belong to no site.

export type Verify = (token: string, ip: string | null, host: string) => Promise<boolean>;

const SITEVERIFY = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

/** The test secret that always passes, for `wrangler dev` and the tests. */
export const TEST_SECRET = '1x0000000000000000000000000000000AA';

export function turnstile(secret: string, fetcher: typeof fetch = fetch): Verify {
  return async (token, ip, host) => {
    if (!token || token.length > 2048) return false;
    const body = new FormData();
    body.set('secret', secret);
    body.set('response', token);
    if (ip) body.set('remoteip', ip);
    try {
      const res = await fetcher(SITEVERIFY, { method: 'POST', body });
      if (!res.ok) return false;
      const out = (await res.json()) as { success?: boolean; hostname?: string };
      return out.success === true && (secret === TEST_SECRET || out.hostname === host);
    } catch {
      return false;
    }
  };
}
