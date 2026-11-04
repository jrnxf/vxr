import { useEffect, useRef, useState } from "preact/hooks"

import { Mark } from "../Mark"
import { grow, release } from "./hero"
import { preload } from "./preload"

const root =
  typeof document === "undefined" ? undefined : document.documentElement
const editorReady = () => !!root?.hasAttribute("data-ready")
const whenReady = () =>
  new Promise<void>((done) => {
    if (editorReady()) done()
    else addEventListener("vxr-ready", () => done(), { once: true })
  })

type Phase = "idle" | "leaving" | "gone"

// The landing page: a dark page over the editor, which loads behind it (gate.js shows it on a
// first visit and at /about). It downloads what the editor needs while the page is read, fills
// the primary button with the progress, and opens the editor, the island picture growing into
// it, as soon as the button is clicked and the editor (app.js, running behind the page) is
// ready. Pre-rendered at build time, so everything here but the progress shows with no script.
export function Landing() {
  const [percent, setPercent] = useState<number | undefined>()
  const [ready, setReady] = useState(false)
  const [opening, setOpening] = useState(false)
  const [phase, setPhase] = useState<Phase>("idle")
  const [status, setStatus] = useState("")
  const open = useRef<HTMLButtonElement>(null)
  const frame = useRef<HTMLDivElement>(null)
  const hero = useRef<HTMLDivElement>(null)
  const entered = useRef(false)

  useEffect(() => {
    // The editor draws behind the page: keep keys and pointers out of it until it is opened.
    const canvas = document.getElementById("c") as HTMLCanvasElement
    canvas.inert = true
    void preload(setPercent)
    void whenReady().then(() => {
      setPercent(100)
      setReady(true)
      setStatus("The editor is ready.")
    })
  }, [])

  // The fill's width is a custom property, set through the CSSOM (the page's CSP has no
  // inline styles).
  useEffect(() => {
    open.current?.style.setProperty("--p", `${percent ?? 0}%`)
  }, [percent])

  async function enter() {
    if (entered.current || !hero.current || !frame.current) return
    entered.current = true
    const canvas = document.getElementById("c") as HTMLCanvasElement
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches
    if (reduced) {
      setPhase("gone")
      await new Promise((done) => setTimeout(done, 200))
    } else {
      await grow(hero.current, frame.current, () => setPhase("leaving"))
    }
    root!.dataset.landing = "off"
    canvas.inert = false
    canvas.focus()
    try {
      localStorage.setItem("vxr-visited", "1")
    } catch {}
    if (location.pathname !== "/") history.replaceState(null, "", "/")
    if (!hero.current.hidden) await release(hero.current)
    root!.dataset.entered = ""
  }

  // Clicked before the editor is ready: say so on the button and go the moment it is.
  async function go() {
    if (!editorReady()) {
      setOpening(true)
      setStatus("Opening the editor")
      await whenReady()
    }
    void enter()
  }

  // A scene picked before the editor is ready waits for it, then goes in as the editor's own
  // Open would (app.js listens for the same change).
  useEffect(() => {
    const file = document.getElementById("file") as HTMLInputElement
    const onChange = async () => {
      if (editorReady()) return void enter()
      setOpening(true)
      await whenReady()
      file.dispatchEvent(new Event("change"))
      void enter()
    }
    file.addEventListener("change", onChange)
    return () => file.removeEventListener("change", onChange)
  }, [])

  const line =
    percent === undefined
      ? ""
      : percent >= 100
        ? "Ready"
        : `Getting the editor ready${percent > 0 ? ` · ${percent}%` : ""}`

  return (
    <>
      <main
        id="landing"
        class={
          phase === "idle" ? "" : phase === "gone" ? "leaving gone" : "leaving"
        }
        aria-label="vxr"
      >
        <header class="land-top">
          <a class="lock" href="/about" aria-label="vxr">
            <Mark />
            <span>vxr</span>
          </a>
          <nav>
            <a
              class="btn ghost"
              href="https://github.com/jrnxf/vxr"
              rel="noopener"
            >
              <svg class="i" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4" />
                <path d="M9 18c-4.51 2-5-2-7-2" />
              </svg>
              GitHub
            </a>
            <a class="btn ghost" href="/account">
              Sign in
            </a>
          </nav>
        </header>
        <div class="land-body">
          <div class="land-copy">
            <p class="eyebrow">Open source · runs in your browser</p>
            <h1>
              Voxel scenes,
              <br />
              fast.
            </h1>
            <p class="lede">
              Sculpt, paint and path-trace MagicaVoxel scenes on the GPU. Open a
              .vox or start from the island. Put it on any page with one tag.
            </p>
            <div class="ctas">
              <button
                id="land-open"
                ref={open}
                class={`btn primary big${ready ? " ready" : ""}${opening ? " opening" : ""}`}
                type="button"
                onClick={() => void go()}
              >
                <span class="fill" />
                <span class="lbl lbl-open">
                  Open the editor
                  <svg class="i" viewBox="0 0 24 24" aria-hidden="true">
                    <path d="M5 12h14M12 5l7 7-7 7" />
                  </svg>
                </span>
                <span class="lbl lbl-opening" aria-hidden="true">
                  Opening <span class="spin" />
                </span>
              </button>
              <button
                id="land-file"
                class="btn big"
                type="button"
                onClick={() => document.getElementById("file")?.click()}
              >
                Open a .vox file
              </button>
            </div>
            <p id="land-load" class="load-line" aria-hidden="true">
              {line}
            </p>
            <p class="sr-only" id="land-status" role="status">
              {status}
            </p>
            <p class="reassure">
              <span>
                <svg class="i" viewBox="0 0 24 24" aria-hidden="true">
                  <rect width="18" height="11" x="3" y="11" rx="2" />
                  <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                </svg>
                No account needed
              </span>
              <span>
                <svg class="i" viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
                </svg>
                <a href="/account">Sign in</a> to sync
              </span>
            </p>
          </div>
          <div class="frame" id="land-frame" ref={frame}>
            <img
              src="/landing.webp"
              width="1280"
              height="800"
              alt="The vxr editor open on a small voxel island"
              fetchpriority="high"
            />
          </div>
        </div>
      </main>
      <div id="hero" ref={hero} hidden>
        <img alt="" />
      </div>
    </>
  )
}
