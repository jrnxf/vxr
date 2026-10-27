# Camera controls: MagicaVoxel against vxr

How MagicaVoxel 0.99.x moves the camera, from its own shortcut list and release notes, and what vxr does. Where vxr differs, the reason is that the difference keeps the scene from getting lost.

## Sources

- [S1] ephtracy's shortcut list, Camera section: [mv_controls.html](https://raw.githubusercontent.com/ephtracy/ephtracy.github.io/master/mv_controls.html) (the page at ephtracy.github.io renders it with JavaScript; the raw file is the same text).
- [S2] Release notes: [mv_main.html](https://ephtracy.github.io/mv_main.html) (0.97.1 middle-drag, 0.99 view cube, 0.99.3 X + left-click focus, 0.99.6.2 camera panel, 0.99.6.4 FOV and mouse sensitivity).
- [S3] Older user reference (0.9x, may be dated): [leonida.gitbooks.io](https://leonida.gitbooks.io/magicavoxel-user-reference-manual/interface/).
- [S4] Third-party claims, not confirmed by ephtracy: Space + right-drag pans ([A-Frame guide](https://aframe.io/docs/0.8.0/guides/building-with-magicavoxel.html)); on a Mac trackpad two fingers zoom and a two-finger click-drag rotates ([Styly](https://styly.cc/tips/3d_software_magica-voxel/)).

## Controls

| Move | MagicaVoxel | vxr | Why vxr differs |
|---|---|---|---|
| Orbit | Right-drag [S1] | Right-drag (a two-finger click-drag on a Mac trackpad) | - |
| Pan | Middle-drag, or Space + left-drag [S1]; Space + right-drag [S4] | Middle-drag, Space + left or right-drag, Shift + right-drag | Shift + right-drag kept: it was vxr's pan before and needs no third button |
| Zoom | Wheel [S1]; Z + drag on the view cube [S2] | Wheel and two-finger scroll, trackpad pinch, Cmd-= and Cmd--, the camera bar's zoom buttons | Pinch added for trackpads |
| Zoom target | Not documented | The voxel under the pointer stays under it; the buttons and keys zoom about the middle of the view | Kept from vxr: zooming in on what you point at, safe because of the bounds below |
| Orbit pivot | A focus point; middle-drag and Space-drag move it [S1] | The same: pan moves it | - |
| Set the pivot | X + left-click [S1, S2 0.99.3] | X + left-click: orbit around the voxel clicked, which moves to the middle of the view | - |
| Recenter | 4 / F4 [S1] | `.` (Frame scene), keeping the angle and projection | `4` is vxr's Right view |
| Reset | 5 / F5 resets to the nearest 90 degree view [S1]; "Reset" returns to the initial settings [S3] | Home (Reset camera): the starting angle in perspective, framing the scene | Home is the common key for it |
| Snap views | View cube clicks snap to 26 angles [S2]; 5 [S1] | 1 to 6 for the six sides, 0 isometric, the View flyout from the rail or the camera bar | - |
| Projection | Bottom bar: Perspective, Free, Orthogonal, Isometric [S2] | O toggles, Persp / Ortho / Iso in the View flyout and on the camera bar | No free (fly) camera yet |
| Frame selection | Not found | Cmd-K "Frame selection" and the camera bar's button | Added: framing what you work on |
| Save and load poses | 7 / 8, camera slots on the numpad [S1] | Not yet (cameras in files are kept and editable through the scene, not shown) | - |
| Limits | None documented [S1, S2] | The pivot stays inside the model's bounds and the zoom within 4 framings of them | The point of this change: see below |

## Never losing the scene

MagicaVoxel documents no limit on pan or zoom; it relies on recenter (4). vxr used to let the pivot drift without limit too: panning moved it anywhere, zooming out about a voxel at the edge of the view pushed it away from the model, and zoom-out had no maximum, so a few moves left the scene a speck or off screen, with only `.` (unlabelled on screen) to get it back.

vxr now bounds the camera after every pan and zoom (`OrbitCamera::bound`):

- **Pivot**: stays inside the model's bounds (the box the bounds frame draws, with any voxels outside it), so the middle of the view always crosses the space you build in and an orbit cannot swing it away. A small model in large bounds can still be panned and zoomed over the whole of them.
- **Distance**: at most `MAX_ZOOM_OUT` (4) times the distance whose view holds the bounds' bounding sphere, so the bounds never shrink below about a fifth of the view's height.
- **Framing** (`.`, Home, Frame selection) still fits the voxels (or the floor of an empty scene), not the bounds.
- **No jumps**: a camera already outside (the scene shrank under it, after an erase or a crop) is only kept from going further, never snapped back.

And every way back is on screen: the camera bar above the render corner has Frame scene, Frame selection and Reset camera, with zoom, the projection and the views, each with a tooltip naming its key, and Cmd-K lists every move. Holding Space shows the camera moves in the hint bar.
