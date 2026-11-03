// Sending the sign-in link. One small interface, so the sender can be chosen later without
// touching the sign-in code: today there is the log sender (prints the link, for `wrangler dev` on
// this machine only) and no sender (email sign-in is off). Tests pass their own, which keeps what
// it was given.

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

const local = new Set(['localhost', '127.0.0.1', '[::1]']);

/** The sender for this Worker: EMAIL_SENDER=log on a local host, or none. */
export function mailerFor(env: { EMAIL_SENDER?: string }, url: URL): Mailer | null {
  if (env.EMAIL_SENDER === 'log' && local.has(url.hostname)) return logMailer;
  return null;
}

export function signInMail(to: string, url: string): Mail {
  return {
    to,
    subject: 'Sign in to vxr',
    text: `Open this link to sign in to vxr:\n\n${url}\n\nIt works once, for 10 minutes. If you did not ask for it, ignore this email.`,
  };
}
