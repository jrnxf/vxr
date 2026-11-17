import { Mark } from "./Mark"

// The cube in the middle until the editor draws its first frame (styles in page.css). Then the
// editor's panels fade in and the scene settles in (Hybrid::intro_tick in crates/vxr-app/src/ui.rs).
export function Boot() {
  return (
    <div id="boot" aria-hidden="true">
      <Mark class="sk-mark" />
    </div>
  )
}
