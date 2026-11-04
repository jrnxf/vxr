/** What the Worker's /api/me answers. */
export interface Me {
  signIn: boolean
  email: boolean
  user: { id: string; email: string; name: string } | null
  usage?: number
  limits: { account: number; file: number }
}

export function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  const mb = bytes / (1024 * 1024)
  return `${mb >= 10 || Number.isInteger(mb) ? Math.round(mb) : mb.toFixed(1)} MB`
}

interface Problem {
  status?: number
  error?: unknown
  code?: unknown
  message?: unknown
}

// What went wrong, in words, from a Worker problem ({error}) or a Better Auth error ({code, message}).
export function reason(err: Problem | null): string {
  const code = String(err?.error ?? err?.code ?? "")
  switch (code) {
    case "challenge":
      return "The check did not pass. Try again."
    case "too_many_links":
      return "Too many sign-in links asked for. Try again in an hour."
    case "email_off":
    case "sign_up_off":
    case "sign_in_off":
      return "Sign-in is off here."
    case "bad_email":
      return "That email address does not look right."
    case "INVALID_OTP":
      return "That code is not right. Check it and try again."
    case "OTP_EXPIRED":
    case "TOO_MANY_ATTEMPTS":
      return "That code has expired or was tried too often. Ask for a new email."
  }
  if (err?.status === 429) return "Too many tries. Wait a minute and try again."
  return typeof err?.message === "string" && err.message
    ? err.message
    : "Something went wrong. Try again."
}
