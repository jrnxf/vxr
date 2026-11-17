import { Mark } from "./Mark"

// The cube in the middle until the editor draws its first frame (styles in page.css), then it
// fades out over the editor, which shows whole at once.
export function Boot() {
  return (
    <div id="boot" aria-hidden="true">
      <Mark class="sk-mark" />
    </div>
  )
}
