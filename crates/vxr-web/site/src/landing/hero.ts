// The island picture grows from the page's frame into the editor, over 680 ms: the same
// pixels throughout (landing.webp is a shot of the editor, covering the window like the
// editor does), so the editor is simply there when it ends. Transform and clip-path only.
const SHOT = { w: 1280, h: 800 }
const ISLAND = { x: 300, y: 212, w: 700, h: 520 }
const EASE = "cubic-bezier(0.77, 0, 0.175, 1)"

// Resolves when the picture covers the window. onStart runs once it is on screen.
export async function grow(
  hero: HTMLElement,
  frame: HTMLElement,
  onStart: () => void,
) {
  const [vw, vh] = [innerWidth, innerHeight]
  const k = Math.max(vw / SHOT.w, vh / SHOT.h)
  const [ox, oy] = [(vw - SHOT.w * k) / 2, (vh - SHOT.h * k) / 2]
  const img = hero.firstElementChild as HTMLImageElement
  img.src = (frame.firstElementChild as HTMLImageElement).currentSrc
  for (const [name, v] of Object.entries({
    left: ox,
    top: oy,
    width: SHOT.w * k,
    height: SHOT.h * k,
  })) {
    img.style.setProperty(name, `${v}px`)
  }
  await img.decode().catch(() => {})
  // Where the island's crop sits in the hero, and the frame's inside it is scaled onto.
  const f = frame.getBoundingClientRect()
  const [fx, fy, fw] = [
    f.left + frame.clientLeft,
    f.top + frame.clientTop,
    frame.clientWidth,
  ]
  const s = fw / (ISLAND.w * k)
  const [cx, cy] = [ox + ISLAND.x * k, oy + ISLAND.y * k]
  const inset = (r: number) =>
    `inset(${cy}px ${vw - cx - ISLAND.w * k}px ${vh - cy - ISLAND.h * k}px ${cx}px round ${r}px)`
  hero.hidden = false
  onStart()
  await hero.animate(
    [
      {
        transform: `translate(${fx - s * cx}px, ${fy - s * cy}px) scale(${s})`,
        clipPath: inset(16 / s),
      },
      { transform: "none", clipPath: "inset(0px 0px 0px 0px round 0px)" },
    ],
    { duration: 680, delay: 120, easing: EASE, fill: "both" },
  ).finished
}

// The editor is under the picture, drawn at the window's own size: let the picture go.
export async function release(hero: HTMLElement) {
  await hero.animate(
    { opacity: [1, 0] },
    { duration: 160, easing: "ease", fill: "both" },
  ).finished
  hero.hidden = true
}
