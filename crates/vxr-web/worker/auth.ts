// The optional vxr account, through Better Auth on D1: sign-in by email only. One email carries
// a one-time link (the `magicLink` plugin) and a 6-digit code (the `emailOTP` plugin), so a person
// can open the link, or read the code on one device and type it on another. Either one makes the
// account on first use. Sessions are a same-site cookie on this origin, so the editor's fetch to
// /api/projects carries it with no token handling of its own. The desktop app has no cookie jar:
// it signs in by code only and keeps the session as a bearer token (the `bearer` plugin), and only
// its own requests get that token back (see `fromApp`).
import { betterAuth, type BetterAuthOptions } from 'better-auth';
import { bearer } from 'better-auth/plugins/bearer';
import { emailOTP } from 'better-auth/plugins/email-otp';
import { magicLink } from 'better-auth/plugins/magic-link';
import { appSignInMail, type Mailer, signInMail } from './mail';

/** How long the link and the code in a sign-in email work, in seconds. */
export const SIGN_IN_TTL = 600;

/**
 * The email-otp routes that stay off: codes are only made beside a link, through
 * /sign-in/magic-link (behind the Worker's Turnstile check and mail limits), and only
 * /sign-in/email-otp takes one.
 */
export const OTP_OFF = [
  '/email-otp/send-verification-otp',
  '/email-otp/check-verification-otp',
  '/email-otp/verify-email',
  '/email-otp/request-password-reset',
  '/email-otp/reset-password',
  '/forget-password/email-otp',
  '/email-otp/request-email-change',
  '/email-otp/change-email',
];

/** Sends the sign-in email for `email` with its link (only the code for the app); null when there is no sender. */
export type SendSignIn = ((email: string, link: string, app: boolean) => Promise<void>) | null;

/**
 * A request from the desktop app: it says so with `x-vxr-client: app` and has no Origin. Every
 * browser sends Origin on a POST, and the custom header needs a CORS preflight this Worker never
 * allows, so no web page can make one.
 */
export function fromApp(req: Request | undefined): boolean {
  return req?.headers.get('x-vxr-client') === 'app' && !req.headers.has('origin');
}

/** The Better Auth options for a Worker answering on `origin`. */
export function authOptions(origin: string, secret: string, database: BetterAuthOptions['database'], send: SendSignIn) {
  return {
    appName: 'vxr',
    baseURL: origin,
    basePath: '/api/auth',
    secret,
    database,
    trustedOrigins: [origin],
    session: { expiresIn: 60 * 60 * 24 * 60, updateAge: 60 * 60 * 24 },
    disabledPaths: OTP_OFF,
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
        '/sign-in/email-otp': { window: 600, max: 10 },
      },
    },
    plugins: [
      magicLink({
        expiresIn: SIGN_IN_TTL,
        async sendMagicLink({ email, url: link }, ctx) {
          if (!send) throw new Error('no email sender');
          await send(email, link, fromApp(ctx?.request));
        },
      }),
      emailOTP({
        expiresIn: SIGN_IN_TTL,
        otpLength: 6,
        allowedAttempts: 3,
        storeOTP: 'hashed',
        async sendVerificationOTP() {
          // Only reachable through the routes in OTP_OFF; codes go out with a link instead.
          throw new Error('codes are sent with a sign-in link');
        },
      }),
      // Only signed tokens: the session cookie's value, as `set-auth-token` gave it to the app.
      bearer({ requireSignature: true }),
    ],
  } satisfies BetterAuthOptions;
}

export type Auth = ReturnType<typeof makeAuth>;

export function makeAuth(origin: string, secret: string, db: D1Database, mailer: Mailer | null) {
  const auth = betterAuth(
    authOptions(
      origin,
      secret,
      db,
      mailer &&
        (async (email: string, link: string, app: boolean): Promise<void> => {
          // A fresh code for this address (it replaces any earlier one), mailed beside the link,
          // or alone for the app, where a link would sign in a browser instead.
          const code: string = await auth.api.createVerificationOTP({ body: { email, type: 'sign-in' } });
          await mailer.send(app ? appSignInMail(email, code) : signInMail(email, link, code));
        }),
    ),
  );
  return auth;
}
