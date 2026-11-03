// Sending the sign-in link, behind one small interface: Resend in production (its HTTP API, with
// the RESEND_API_KEY Worker secret), the log sender for `wrangler dev` on this machine only (prints
// the link), or no sender (email sign-in is off). Tests pass their own, which keeps what it was
// given, and never reach Resend.

export interface Mail {
  to: string;
  subject: string;
  text: string;
}

export interface Mailer {
  send(mail: Mail): Promise<void>;
}

/** Prints each mail to the Worker's log. Never used for a request from another host. */
export const logMailer: Mailer = {
  async send(mail) {
    console.log(`[vxr mail] to ${mail.to}: ${mail.subject}\n${mail.text}`);
  },
};

/** The address sign-in links come from (its domain is verified at Resend). */
export const FROM = 'vxr <vxr@jrnxf.co>';
const RESEND = 'https://api.resend.com/emails';

/** Sends through Resend's HTTP API with `key`; `post` is `fetch` but for the tests. */
export function resendMailer(key: string, post: typeof fetch = fetch): Mailer {
  return {
    async send(mail) {
      const res = await post(RESEND, {
        method: 'POST',
        headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json' },
        body: JSON.stringify({ from: FROM, to: [mail.to], subject: mail.subject, text: mail.text }),
      });
      if (!res.ok) throw new Error(`resend answered ${res.status}`);
    },
  };
}

const local = new Set(['localhost', '127.0.0.1', '[::1]']);

/** The sender for this Worker: EMAIL_SENDER=log on a local host, else Resend when RESEND_API_KEY is set, else none. */
export function mailerFor(env: { EMAIL_SENDER?: string; RESEND_API_KEY?: string }, url: URL): Mailer | null {
  if (env.EMAIL_SENDER === 'log') return local.has(url.hostname) ? logMailer : null;
  if (env.RESEND_API_KEY) return resendMailer(env.RESEND_API_KEY);
  return null;
}

export function signInMail(to: string, url: string): Mail {
  return {
    to,
    subject: 'Sign in to vxr',
    text: `Open this link to sign in to vxr:\n\n${url}\n\nIt works once, for 10 minutes. If you did not ask for it, ignore this email.`,
  };
}
