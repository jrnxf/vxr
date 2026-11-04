// Deleting an account, for the signed-in user:
//
//   DELETE /api/account   body {"email": "<the account's email>"}, typed by the person to confirm
//
// It signs out (its answer carries the cookies that clear the session), deletes the user row,
// which takes its sessions, sign-in records and project rows with it (`on delete cascade`), and
// then every object under `<userId>/` in the projects bucket. Copies of scenes on the person's
// own devices stay. The sign-in email log keeps the address for up to a day, for the email
// limits, and then drops it.
import type { Auth } from './auth';
import { problem, type Store } from './projects';

interface User {
  id: string;
  email: string;
}

export async function deleteAccount(req: Request, env: Store, auth: Auth, user: User): Promise<Response> {
  if (req.method !== 'DELETE') return problem(405, 'method');
  let email = '';
  try {
    email = String(((await req.json()) as { email?: unknown }).email ?? '').trim().toLowerCase();
  } catch {
    return problem(400, 'bad_body');
  }
  if (email !== user.email.toLowerCase()) return problem(400, 'confirm');
  const out = await auth.api.signOut({ headers: req.headers, asResponse: true });
  await env.DB.batch([
    env.DB.prepare('delete from "user" where id = ?').bind(user.id),
    // A sign-in code not yet used (verification rows have no user to cascade from).
    env.DB.prepare('delete from verification where identifier = ?').bind(`sign-in-otp-${email}`),
  ]);
  // No row is left for a save to land on, so one in flight deletes its own bytes (see
  // `projects.ts`); clear twice to also take what such a save put before it noticed.
  for (let i = 0; i < 2; i++) await clear(env.PROJECTS, `${user.id}/`);
  const res = new Response(null, { status: 204, headers: { 'cache-control': 'no-store' } });
  for (const c of out.headers.getSetCookie()) res.headers.append('set-cookie', c);
  return res;
}

/** Deletes every object under `prefix`, a page of listing at a time. */
async function clear(bucket: R2Bucket, prefix: string): Promise<void> {
  let cursor: string | undefined;
  do {
    const listed = await bucket.list({ prefix, cursor });
    if (listed.objects.length) await bucket.delete(listed.objects.map((o) => o.key));
    cursor = listed.truncated ? listed.cursor : undefined;
  } while (cursor);
}
