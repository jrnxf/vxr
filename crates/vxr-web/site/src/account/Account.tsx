import { createAuthClient } from "better-auth/client"
import { emailOTPClient, magicLinkClient } from "better-auth/client/plugins"
import { useEffect, useRef, useState } from "preact/hooks"

import { reason, size, type Me } from "./reason"
import { renderChallenge, spent, token } from "./turnstile"

type View = "loading" | "off" | "out" | "sent" | "in"

// Better Auth's client, made on first use (the pre-render has no location).
let client: ReturnType<typeof makeAuth> | undefined
const makeAuth = () =>
  createAuthClient({
    baseURL: location.origin,
    basePath: "/api/auth",
    plugins: [magicLinkClient(), emailOTPClient()],
  })
const auth = () => (client ??= makeAuth())

// Editor tabs of this site listen here: a sign-in (a link opened in this tab, or a code) or a
// sign-out on this page reaches them at once, without waiting for their tab to come back.
let editors: BroadcastChannel | null | undefined
const tellEditors = (user: string | null) => {
  editors ??=
    typeof BroadcastChannel === "function"
      ? new BroadcastChannel("vxr-account")
      : null
  // A BroadcastChannel has no target origin to name.
  // oxlint-disable-next-line unicorn/require-post-message-target-origin
  editors?.postMessage({ user })
}

async function me(): Promise<Me | null> {
  try {
    const res = await fetch("/api/me", {
      credentials: "same-origin",
      cache: "no-store",
    })
    return res.ok ? ((await res.json()) as Me) : null
  } catch {
    return null
  }
}

// The account page: sign in by email. One email carries a link and a 6-digit code; the link signs
// in where it is opened, the code is typed here. The Worker (worker/index.ts) checks the
// Turnstile token sent as the x-turnstile-token header before it sends the email. A sign-in link
// the editor's popover asked for opens this page with ?from=editor, and the editor checks /api/me
// again when this page says so on the vxr-account channel.
export function Account() {
  const [view, setView] = useState<View>("loading")
  const [info, setInfo] = useState<Me | null>(null)
  const [message, setMessage] = useState({ text: "", error: false })
  const [busy, setBusy] = useState(false)
  const [email, setEmail] = useState("")
  const [code, setCode] = useState("")
  const [sentTo, setSentTo] = useState("")
  const [fromEditor, setFromEditor] = useState(false)
  const challenge = useRef<HTMLDivElement>(null)
  const codeField = useRef<HTMLInputElement>(null)
  const bar = useRef<HTMLDivElement>(null)

  const used = info?.usage ?? 0
  const say = (text: string, error = false) => setMessage({ text, error })
  const back = fromEditor ? "/account?from=editor" : "/account"

  async function refresh() {
    const m = await me()
    if (!m || !m.signIn) return setView("off")
    tellEditors(m.user?.id ?? null)
    setInfo(m)
    if (m.user) setView("in")
    else {
      setView("out")
      // The widget is drawn into the form's slot, which is in the page already (hidden sections).
      if (challenge.current && !(await renderChallenge(challenge.current)))
        say(
          "The sign-in check could not load. Reload the page to try again.",
          true,
        )
    }
  }

  useEffect(() => {
    const params = new URLSearchParams(location.search)
    setFromEditor(params.get("from") === "editor")
    if (params.has("link") || params.has("error"))
      say("That sign-in link has expired or was used. Ask for a new one.", true)
    void refresh()
  }, [])

  // The bar is set through the DOM, not a style attribute, which the page's policy forbids.
  useEffect(() => {
    if (bar.current && info)
      bar.current.style.width = `${Math.min(100, (used / info.limits.account) * 100)}%`
  }, [info, used])

  useEffect(() => {
    if (view === "sent") codeField.current?.focus()
  }, [view])

  // Every button is off while one action runs; focus goes back to the one that was pressed.
  async function run(button: HTMLElement, action: () => Promise<void>) {
    setBusy(true)
    say("")
    try {
      await action()
    } finally {
      setBusy(false)
      // The buttons are enabled again once this render lands.
      setTimeout(() => button.focus())
    }
  }

  function askForEmail(e: Event) {
    e.preventDefault()
    const form = e.currentTarget as HTMLFormElement
    void run(form.querySelector("button")!, async () => {
      const address = email.trim()
      const t = await token()
      if (!t) return say("Finish the check below first.", true)
      const { error } = await auth().signIn.magicLink({
        email: address,
        callbackURL: back,
        errorCallbackURL: `${back}${back.includes("?") ? "&" : "?"}link=expired`,
        fetchOptions: { headers: { "x-turnstile-token": t } },
      })
      spent()
      if (error) return say(reason(error), true)
      setSentTo(address)
      setCode("")
      setView("sent")
    })
  }

  function sendCode(e: Event) {
    e.preventDefault()
    const form = e.currentTarget as HTMLFormElement
    void run(form.querySelector("button")!, async () => {
      const otp = code.replace(/\D/g, "")
      const { error } = await auth().signIn.emailOtp({ email: sentTo, otp })
      if (error) return say(reason(error), true)
      await refresh()
    })
  }

  function signOut(e: Event) {
    void run(e.currentTarget as HTMLElement, async () => {
      await auth().signOut()
      await refresh()
      say("Signed out. Projects stay in this browser.")
    })
  }

  const limit = (which: "account" | "file") =>
    info ? size(info.limits[which]) : ""

  return (
    <main>
      <a class="brand" href="/">
        <img src="/favicon.svg" alt="" width="28" height="28" />
        <span>vxr</span>
      </a>

      <section id="loading" hidden={view !== "loading"}>
        <p class="muted">Loading…</p>
      </section>

      <section id="off" hidden={view !== "off"}>
        <h1>Accounts are off</h1>
        <p class="muted">
          This copy of vxr has no sign-in. The editor still keeps your projects
          in this browser.
        </p>
        <a class="button" href="/">
          Open the editor
        </a>
      </section>

      <section id="out" hidden={view !== "out"}>
        <h1>Sign in to vxr</h1>
        <p class="muted">
          Optional. Signed in, your projects sync between your browsers, up to{" "}
          <span>{limit("account")}</span> in all and{" "}
          <span>{limit("file")}</span> a file.
        </p>
        <form id="email-form" class="stack" onSubmit={askForEmail}>
          <label for="email">Email</label>
          <input
            id="email"
            type="email"
            autocomplete="email"
            placeholder="you@example.com"
            required
            value={email}
            onInput={(e) => setEmail(e.currentTarget.value)}
          />
          <button class="primary" type="submit" disabled={busy}>
            Email me a sign-in link and code
          </button>
        </form>
        <div id="challenge" ref={challenge} />
      </section>

      <section id="sent" hidden={view !== "sent"}>
        <h1>Check your email</h1>
        <p class="muted">
          A sign-in link and code are on their way to{" "}
          <strong id="sent-to">{sentTo}</strong>. Open the link, or enter the
          code here. Each works once, for 10 minutes.
        </p>
        <form id="code-form" class="stack" onSubmit={sendCode}>
          <label for="code">Code</label>
          <input
            id="code"
            ref={codeField}
            type="text"
            inputMode="numeric"
            autocomplete="one-time-code"
            pattern="[0-9 ]{6,7}"
            maxLength={7}
            placeholder="123456"
            required
            value={code}
            onInput={(e) => setCode(e.currentTarget.value)}
          />
          <button class="primary" type="submit" disabled={busy}>
            Sign in
          </button>
          <button
            id="sent-back"
            type="button"
            disabled={busy}
            onClick={() => {
              say("")
              setView("out")
            }}
          >
            Back
          </button>
        </form>
      </section>

      <section id="in" hidden={view !== "in"}>
        <h1>Signed in</h1>
        <p class="muted" id="who">
          {info?.user ? `As ${info.user.email}.` : ""}
        </p>
        <div class="usage">
          <div class="bar">
            <div id="usage-bar" ref={bar} />
          </div>
          <p class="muted">
            <span id="usage">{size(used)}</span> of{" "}
            <span>{limit("account")}</span> used
          </p>
        </div>
        <p class="muted" id="return" hidden={!fromEditor}>
          Your projects sync now. Go back to the editor tab to see them there.
        </p>
        <div class="stack">
          <a class="button primary" href="/">
            Open the editor
          </a>
          <button id="sign-out" type="button" disabled={busy} onClick={signOut}>
            Sign out
          </button>
        </div>
      </section>

      <p
        id="msg"
        role="status"
        aria-live="polite"
        class={message.error ? "error" : ""}
      >
        {message.text}
      </p>
    </main>
  )
}
