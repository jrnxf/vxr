import { preact } from "@preact/preset-vite"
import { createLogger, defineConfig } from "vite"

const logger = createLogger()
const warn = logger.warn
const warnOnce = logger.warnOnce
// The fonts are in dist/fonts (build copies them there), not in www/: the stylesheet names them
// by URL for the browser, and Vite is right to leave those alone.
logger.warn = (msg, options) => {
  if (!msg.includes("fonts/")) warn(msg, options)
}
logger.warnOnce = (msg, options) => {
  if (!msg.includes("fonts/")) warnOnce(msg, options)
}

// The landing page and the boot skeleton, pre-rendered into dist/index.html (build runs this
// after the wasm is in dist/). www/ is the public folder: app.js, gate.js, _headers and the
// icons are copied as they are.
export default defineConfig({
  customLogger: logger,
  plugins: [preact({ prerender: { enabled: true, renderTarget: "#root" } })],
  publicDir: "../www",
  build: {
    outDir: "../dist",
    emptyOutDir: false,
    target: "es2022",
    modulePreload: { polyfill: false },
  },
})
