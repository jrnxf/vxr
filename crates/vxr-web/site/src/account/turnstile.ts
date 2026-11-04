/** The Turnstile site key, set by the build (TURNSTILE_SITE_KEY; Cloudflare's always-pass test key by default). */
declare const TURNSTILE_SITE_KEY: string

interface Turnstile {
  render(el: HTMLElement, opts: Record<string, unknown>): string
  getResponse(id: string): string | undefined
  reset(id: string): void
}
declare global {
  interface Window {
    turnstile?: Turnstile
  }
}

// One Turnstile widget, for the action that needs it: asking for a sign-in email. The script is
// loaded by account.html (async, so it may still be on its way).
let widget: string | null = null
let ready: Promise<Turnstile> | null = null
function script(): Promise<Turnstile> {
  ready ??= new Promise((resolve, reject) => {
    if (window.turnstile) return resolve(window.turnstile)
    const s = document.getElementById("turnstile-js")
    if (!s) return reject(new Error("no turnstile script"))
    s.addEventListener("load", () =>
      window.turnstile
        ? resolve(window.turnstile)
        : reject(new Error("no turnstile")),
    )
    s.addEventListener("error", () =>
      reject(new Error("turnstile did not load")),
    )
  })
  return ready
}

/** Draws the widget in `el` (once); false when Turnstile could not load. */
export async function renderChallenge(el: HTMLElement): Promise<boolean> {
  try {
    const t = await script()
    widget ??= t.render(el, {
      sitekey: TURNSTILE_SITE_KEY,
      appearance: "interaction-only",
      theme: "auto",
      size: "flexible",
      action: "account",
    })
    return true
  } catch {
    return false
  }
}

/** A fresh token, or null when the check has not passed yet. */
export async function token(): Promise<string | null> {
  const t = await script().catch(() => null)
  return (t && widget ? t.getResponse(widget) : undefined) || null
}

/** The token was used: the widget needs a new one. */
export function spent() {
  if (window.turnstile && widget) window.turnstile.reset(widget)
}
