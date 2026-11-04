import { hydrate } from "preact"

import { Account } from "./Account"
import "./account.css"

// The page's one script. At build time `prerender` writes the loading state into account.html (so
// the page paints with no JavaScript); in the browser, the same component hydrates it and asks
// /api/me who is signed in.
if (typeof document !== "undefined") {
  const root = document.getElementById("root")
  if (root) hydrate(<Account />, root)
}

export async function prerender() {
  const { html } = await import("./prerender")
  return { html: html() }
}
