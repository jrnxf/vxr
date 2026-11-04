import { Mark } from "./Mark"

// The editor's panels as outlines where they will be, until the editor draws its first frame
// (styles in page.css; the places follow the top bar, dock, view bar and corners in
// crates/vxr-app/src/ui.rs).
export function Boot() {
  return (
    <div id="boot" aria-hidden="true">
      <div class="sk sk-title" />
      <div class="sk sk-dock" />
      <div class="sk sk-actions" />
      <div class="sk sk-view" />
      <div class="sk sk-corner" />
      <div class="sk sk-stats" />
      <Mark class="sk-mark" />
    </div>
  )
}
