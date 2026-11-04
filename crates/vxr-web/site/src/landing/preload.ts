// What the editor loads at start: the wasm, the island and the fonts app.js starts with. The
// editor asks for the same URLs, which come from the HTTP cache this fills.
const FONTS = [
  "Geist-Regular",
  "Geist-Medium",
  "Geist-SemiBold",
  "GeistMono-Regular",
  "GeistMono-Medium",
  "GeistMono-UltraBlack",
  "Inter-Regular",
  "lucide",
]
const FILES = [
  "/pkg/vxr_web_bg.wasm",
  "/scene.vox",
  ...FONTS.map((name) => `/fonts/${name}.ttf`),
]

// Downloads every file, calling onProgress(0..99) as bytes arrive, counted against the sizes
// in preload.json (the build writes it). The page sets 100 itself once the editor is ready.
export async function preload(onProgress: (percent: number) => void) {
  let sizes: Record<string, number> = {}
  try {
    sizes = (await (await fetch("/preload.json")).json()) as Record<
      string,
      number
    >
  } catch {}
  const total = FILES.reduce(
    (sum, file) => sum + (sizes[file.slice(1)] ?? 0),
    0,
  )
  let got = 0
  const report = () =>
    onProgress(total ? Math.min(99, Math.floor((got / total) * 100)) : 0)
  report()
  await Promise.all(
    FILES.map(async (file) => {
      try {
        const reader = (await fetch(file)).body?.getReader()
        if (!reader) return
        for (let r = await reader.read(); !r.done; r = await reader.read()) {
          got += r.value.length
          report()
        }
      } catch {}
    }),
  )
}
