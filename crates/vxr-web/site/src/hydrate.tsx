import { hydrate } from "preact"

import { Shell } from "./Shell"

export function hydrateShell() {
  const root = document.getElementById("root")
  if (root) hydrate(<Shell />, root)
}
