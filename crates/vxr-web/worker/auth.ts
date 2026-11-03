// The optional vxr account, through Better Auth on D1: a passkey first (sign-up needs no email),
// and a one-time link by email as the fallback. Sessions are a same-site cookie on this origin,
// so the editor's fetch to /api/projects carries it with no token handling of its own.
import { betterAuth, type BetterAuthOptions } from 'better-auth';
import { magicLink } from 'better-auth/plugins/magic-link';
import { passkey } from '@better-auth/passkey';
import { type Mailer, signInMail } from './mail';

/** What a passkey-only account has for the email Better Auth needs: never mailed, never shown. */
export const NO_EMAIL = '@passkey.invalid';

/** The Better Auth options for a Worker answering on `origin`. */
export function authOptions(origin: string, secret: string, database: BetterAuthOptions['database'], mailer: Mailer | null) {
  const url = new URL(origin);
  return {
    appName: 'vxr',
    baseURL: origin,
    basePath: '/api/auth',
    secret,
    database,
    trustedOrigins: [origin],
    session: { expiresIn: 60 * 60 * 24 * 60, updateAge: 60 * 60 * 24 },
    advanced: {
      cookiePrefix: 'vxr',
      ipAddress: { ipAddressHeaders: ['cf-connecting-ip'] },
    },
    rateLimit: {
      enabled: true,
      storage: 'database',
      window: 60,
      max: 60,
      customRules: {
        '/sign-in/magic-link': { window: 600, max: 5 },
        '/magic-link/verify': { window: 600, max: 20 },
        '/passkey/verify-authentication': { window: 60, max: 20 },
      },
    },
    plugins: [
      magicLink({
        expiresIn: 600,
        async sendMagicLink({ email, url: link }) {
          if (!mailer) throw new Error('no email sender');
          await mailer.send(signInMail(email, link));
        },
      }),
      passkey({
        rpID: url.hostname,
        rpName: 'vxr',
        origin,
        authenticatorSelection: { residentKey: 'required', userVerification: 'preferred' },
        registration: {
          // Sign-up with a passkey: there is no user yet, so the Worker makes one after the
          // passkey checks out (the router has checked the Turnstile token by then).
          requireSession: false,
          async resolveUser() {
            return { id: crypto.randomUUID(), name: 'vxr account', displayName: 'vxr account' };
          },
          async afterVerification({ ctx, user }) {
            // Signed in already (by email link): the passkey joins that account.
            if (await ctx.context.internalAdapter.findUserById(user.id)) return;
            const id = crypto.randomUUID();
            const made = await ctx.context.internalAdapter.createUser(
              { email: `${id}${NO_EMAIL}`, name: '', emailVerified: false },
              { method: 'passkey' },
            );
            return { userId: made.id };
          },
        },
      }),
    ],
  } satisfies BetterAuthOptions;
}

export type Auth = ReturnType<typeof makeAuth>;

export function makeAuth(origin: string, secret: string, db: D1Database, mailer: Mailer | null) {
  return betterAuth(authOptions(origin, secret, db, mailer));
}
