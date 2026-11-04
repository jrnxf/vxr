// Both stylesheets are in this chunk, so they are linked from the page and the pre-rendered
// landing page is styled before any script runs.
import "./page.css"
import "./landing/landing.css"

// The page's one script. At build time `prerender` writes the boot skeleton and the landing
// page into index.html (so they paint with no JavaScript); in the browser, a first visit
// (gate.js sets html[data-landing]) hydrates the landing page, and a returning visit loads
// nothing more.
if (
  typeof document !== "undefined" &&
  document.documentElement.dataset.landing === "on"
) {
  void import("./hydrate").then((m) => m.hydrateShell())
}

export async function prerender() {
  const { html } = await import("./prerender")
  return { html: html() }
}
