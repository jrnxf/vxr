import { preact } from "@preact/preset-vite"
import { createLogger, defineConfig } from "vite"

// The fonts are in dist/fonts (build copies them there); see vite.config.mts.
const logger = createLogger()
const warn = logger.warn
const warnOnce = logger.warnOnce
logger.warn = (msg, options) => {
  if (!msg.includes("fonts/")) warn(msg, options)
}
logger.warnOnce = (msg, options) => {
  if (!msg.includes("fonts/")) warnOnce(msg, options)
}

// The account page, pre-rendered into dist/account.html. A second build next to the landing
// page's (vite.config.mts, which writes dist/index.html): same plugin and output folder, its own
// HTML entry. TURNSTILE_SITE_KEY is the Turnstile site key (build checks it; unset, it is
// Cloudflare's always-pass test key).
export default defineConfig({
  customLogger: logger,
  plugins: [preact({ prerender: { enabled: true, renderTarget: "#root" } })],
  publicDir: false,
  define: {
    TURNSTILE_SITE_KEY: JSON.stringify(
      process.env.TURNSTILE_SITE_KEY ?? "1x00000000000000000000AA",
    ),
  },
  build: {
    outDir: "../dist",
    emptyOutDir: false,
    target: "es2022",
    modulePreload: { polyfill: false },
    rollupOptions: { input: "account.html" },
  },
})
