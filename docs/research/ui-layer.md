# UI layer

**Summary**

- First choice for the Phase 2 bake-off: **egui** (0.37). It is the only Rust UI that is mature, wgpu-native, runs the same code on desktop and on WebGPU in the browser, and has a shipped pro app (Rerun) that looks polished.
- Runner-up: **HTML/CSS over the wgpu canvas** (the page on the web; a transparent child webview via wry on desktop). Best text, IME, accessibility and animation by far, at the cost of a second UI runtime, IPC, and a desktop overlay that is still rough (Wayland, Linux WebKitGTK).
- Not in the bake-off: iced (no accessibility), Slint (web is WebGL and "demo only", licence friction), Bevy UI (means adopting Bevy), Xilem (alpha, no release in a year), Dioxus Native/Blitz (beta). GPUI is a wildcard worth watching.
- Biggest egui risks: animation and visual effects (no blur, few easing tools), web accessibility (experimental only), wasm size.

Facts below are as of 2026-10-24. Star counts and release dates come from the GitHub API via `gh-axi`.

## What Hybrid needs from the UI

From `docs/decisions.md` and the mockups (`docs/screenshots/board-dir-d.png`, `docs/screenshots/editor-island.png`):

- Floating rounded panels with soft shadows over a full-bleed 3D view: toolbar, palette, hint bar, stats corner, live render corner.
- Crisp small text (labels at about 11 to 13 px, monospace numbers), key-cap hints, a Cmd-K palette with a text field.
- Small, fast motion: panel and tree reveal, hover states, coach card dismiss.
- Our own wgpu passes (voxels, path tracer) on the same device, under and inside the UI.
- The same UI in the browser build; the embed (a viewer) needs almost no UI.

## Comparison

| | egui | iced | Slint | Dioxus Native (Blitz) | Bevy UI | Xilem / Masonry | Web UI (HTML/CSS) |
|---|---|---|---|---|---|---|---|
| Latest release | 0.37.0, 9 days ago | 0.14.0, 10 months ago | 1.18.1, 1 month ago | Blitz: no tagged release; Dioxus 0.8.0-alpha.3 | 0.20.0, 15 days ago | 0.4.0, about 1 year ago | n/a (browser) / Tauri 2.12, wry 0.57 |
| Stars | 31.7k | 31.8k | 24.3k | 39.5k (Dioxus), 4.3k (Blitz) | 49.2k | 5.6k | 112k (Tauri) |
| Licence | MIT or Apache-2.0 | MIT | GPLv3, royalty-free (attribution), or commercial | MIT or Apache-2.0 | MIT or Apache-2.0 | Apache-2.0 | n/a / MIT or Apache-2.0 |
| Text | skrifa + vello_cpu, hinting, variable fonts, colour emoji, system fonts | cosmic-text, advanced shaping | own renderers (femtovg, Skia, wgpu) | Parley + Vello (full CSS text) | Parley | Parley + Vello | browser, best available |
| IME | reworked in 0.35/0.36; Tab was eaten in 2025 test | added in 0.14; failed in 2025 test | works, tofu in preedit (2025 test) | AccessKit/winit; untested | basic `EditableText` | works, some tofu (2025 test) | native |
| Accessibility | AccessKit native; web only experimental TTS | none in mainline | native yes; web none | AccessKit planned/in shell | AccessKit via bevy_a11y | AccessKit, partial | native (DOM) |
| Theming vs Hybrid | good with work: rounded frames, shadows, StyleProvider (experimental) | style functions, Oklch palettes | DSL, very good styling | full CSS | Feathers tokens, CSS-like nodes | limited widgets | full CSS, best |
| Animation | basic (`animate_bool`, value tweens) | `Animation` API in 0.14 | built-in property animations | CSS transitions (subset) | via ECS systems | minimal | CSS, WAAPI, best |
| Custom wgpu pass, same device | yes: paint callbacks share encoder and render pass | yes: `shader` widget, `shader::Pipeline` | yes on desktop (unstable feature); web uses WebGL | `wgpu_texture` example | Bevy owns the renderer | `masonry_core` embeds in wgpu apps | desktop: separate surface under a webview; web: separate canvas |
| Browser | yes, WebGPU or WebGL | yes, WebGPU or WebGL | WebGL, "demo purposes" | wasm proof only | yes, heavy | via separate `xilem_web` (DOM) | native |
| Shipped apps | Rerun Viewer | COSMIC desktop, Halloy | LibrePCB 2.0 | none known | Bevy games, editor not yet | none known | Figma (UI in React over a wasm canvas) |

Sources for each row are in the per-option sections below.

## egui

- **Maturity.** 0.37.0 shipped 9 days ago; releases come every 1 to 3 months ([releases](https://github.com/emilk/egui/releases)). Sponsored by Rerun, whose viewer is the reference "professional looking" egui app ([README](https://github.com/emilk/egui#state), [Rerun web viewer](https://app.rerun.io/)). README warns that APIs are still in flux and releases break ([README](https://github.com/emilk/egui#state)).
- **Text.** 0.34 swapped `ab_glyph` for `skrifa` + `vello_cpu`, adding hinting and variable font axes ([0.34.0](https://github.com/emilk/egui/releases/tag/0.34.0)). 0.35 added subpixel binning and a configurable hinting target ([0.35.0](https://github.com/emilk/egui/releases/tag/0.35.0)). 0.37 renders any system glyph including colour emoji and non-Latin scripts, via a font provider on native and an off-screen canvas rasteriser on web ([0.37.0](https://github.com/emilk/egui/releases/tag/0.37.0)). Text shaping is not yet Parley; 0.33 called the refactor "a step towards using parley" ([0.33.0](https://github.com/emilk/egui/releases/tag/0.33.0)).
- **IME.** A 2025 test found the composer worked but egui ate the Tab used to pick kanji ([boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)). Since then: IME composition visuals reworked in 0.35, and IME/autocorrect fixed on iOS and Android web in 0.36 ([0.35.0](https://github.com/emilk/egui/releases/tag/0.35.0), [0.36.0](https://github.com/emilk/egui/releases/tag/0.36.0)). Needs a fresh test in Phase 2.
- **Accessibility.** AccessKit is always on since 0.34 ([0.34.0](https://github.com/emilk/egui/releases/tag/0.34.0)); 0.37 improved menus, popups and roles ([0.37.0](https://github.com/emilk/egui/releases/tag/0.37.0)). On the web there is only an experimental built-in screen reader, because AccessKit has no web adapter ([README](https://github.com/emilk/egui#what-about-accessibility-such-as-screen-readers), [AccessKit](https://github.com/AccessKit/accesskit)).
- **Theming.** `Visuals`/`Style` plus frames with rounding and shadows; 0.37 adds an experimental `StyleProvider` with class-based per-widget styles and roomier default buttons ([0.37.0](https://github.com/emilk/egui/releases/tag/0.37.0)). "Native looking interface" is an explicit non-goal ([README](https://github.com/emilk/egui#non-goals)). No backdrop blur or CSS-like gradients: estimate, based on the epaint shape list in the same release notes; a custom paint callback can fill the gap.
- **Animation.** Built-in tweens only; 0.35 slowed the default from 0.1 s to 0.2 s and smoothed panel and header animations ([0.35.0](https://github.com/emilk/egui/releases/tag/0.35.0)). Anything richer (springs, staggered reveal) we write ourselves.
- **wgpu integration.** `egui_wgpu::CallbackTrait` gets `Device`, `Queue` and the shared `CommandEncoder` in `prepare`, then draws into "the same wgpu::RenderPass that is used for all other egui elements" ([docs.rs](https://docs.rs/egui-wgpu/latest/egui_wgpu/trait.CallbackTrait.html)). We can also render our scene first and draw egui on top with `egui_wgpu::Renderer`. egui-wgpu 0.37 is on wgpu 30 ([docs.rs](https://docs.rs/egui-wgpu/latest/egui_wgpu/struct.Callback.html)).
- **Web and size.** eframe builds the same app for web and native ([README](https://github.com/emilk/egui#integrations)). The full egui demo app's wasm is 11.7 MB raw and 4.4 MB gzip as served today (measured with `curl` on `https://www.egui.rs/egui_demo_app_bg.wasm`). A minimal app is much smaller; estimate 1.5 to 3 MB raw, unverified. The embed can leave egui out entirely.
- **Immediate mode.** Layout is one pass, so sizes of new windows are known a frame late, causing occasional first-frame flicker ([README](https://github.com/emilk/egui#disadvantages-of-immediate-mode)). For a handful of floating panels this is minor.

## iced

- 0.14.0 (10 months ago) added reactive rendering, an `Animation` API, input method support, and a `shader::Pipeline` trait for wgpu ([0.14.0](https://github.com/iced-rs/iced/releases/tag/0.14.0)). Mainline `main` depends on git forks of `cosmic-text` and `winit`, so the next release timing is unclear ([Cargo.toml](https://github.com/iced-rs/iced/blob/master/Cargo.toml)).
- Accessibility: the request has been open since 2020 ([issue #552](https://github.com/iced-rs/iced/issues/552)); a 2025 test found Narrator "can't see into this window" and the IME would not activate ([boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)). A Gleam project uses an iced fork with AccessKit, which shows it is possible but not upstream ([Plushie docs](https://plushie-gleam.hexdocs.pm/0.5.0/accessibility.html)).
- Shipped: System76's COSMIC desktop is built on iced via libcosmic ([ArchWiki](https://wiki.archlinux.org/title/Libcosmic), [Wikipedia](https://en.wikipedia.org/wiki/COSMIC_desktop)).
- Web: builds with WebGPU or WebGL (`webgl` feature) ([Cargo.toml](https://github.com/iced-rs/iced/blob/master/Cargo.toml)); one developer reported a 5.9 MB wasm after `wasm-opt -Oz` ([blog](https://akshy.codeberg.page/projects/i-was-able-to-reduce-size-of-my-wasm-app-by-29_6-perc.html)).
- Verdict: good architecture, but no accessibility and a slow release cadence. Not worth a bake-off slot over egui.

## Slint

- Active: 1.18.1 a month ago, releases about every 2 months ([blog](https://slint.dev/blog)). Shipped in LibrePCB 2.0, a real desktop CAD tool with a 3D viewer next to the editors ([LibrePCB 2.0.0](https://librepcb.org/blog/2026-01-28_release_2.0.0/)).
- Best Rust IME and Narrator result in the 2025 test ([boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)). DSL with strong styling and built-in animations.
- wgpu: an `unstable-wgpu-29` feature lets Slint use our device and queue (`WGPUConfiguration::Manual` or a rendering notifier); the module "may be removed or changed in future minor releases" ([docs](https://docs.slint.dev/latest/docs/rust/slint/wgpu_29/)).
- Web: renders to a canvas with WebGL, has no screen reader support, and is "not recommended for building general-purpose web applications" ([web guide](https://docs.slint.dev/latest/docs/slint/guide/platforms/web/)). That breaks "same UI in the browser on WebGPU".
- Licence: GPLv3, a royalty-free licence that requires showing Slint attribution, or commercial ([README](https://github.com/slint-ui/slint), [royalty-free licence text](https://open.windriver.com/info/uni-license-list/licenses/slint-royalty-free-1.0.html)). Workable for an open-source app, but it constrains vxr's own licence choice.
- Verdict: the best desktop Rust UI for polish, ruled out by the web target.

## Dioxus Native (Blitz)

- Blitz is an HTML/CSS engine (Stylo, Taffy, Parley, Vello) that renders Dioxus apps without a webview; its README calls it **beta**, "usable for making apps if you are an early adopter" ([Blitz README](https://github.com/DioxusLabs/blitz)). It has a `wgpu_texture` integration example and a minimal wasm proof ([examples](https://github.com/DioxusLabs/blitz/tree/main/examples)). Dioxus 0.7 still calls the native renderer experimental ([0.7 release](https://dioxuslabs.com/blog/release-070)); 0.8 is in alpha ([releases](https://github.com/DioxusLabs/dioxus/releases)).
- The interesting angle: the same RSX + CSS can target the real DOM on the web (`dioxus-web`) and Blitz or a webview on desktop, which is the web-UI route in one language. Two different renderers will drift, though.
- Verdict: watch it. If the web-UI route wins the bake-off, Dioxus is the way to keep the UI in Rust.

## Bevy UI

- 0.20 (15 days ago) added Feathers editor widgets (colour input, dropdowns, list view, scrubbable number input), contextual theme tokens, and `em`/`rem` units ([Bevy 0.20](https://bevy.org/news/bevy-0-20/)). Text is on Parley; UI depends on AccessKit ([bevy_text Cargo.toml](https://github.com/bevyengine/bevy/blob/main/crates/bevy_text/Cargo.toml), [bevy_ui Cargo.toml](https://github.com/bevyengine/bevy/blob/main/crates/bevy_ui/Cargo.toml)). Text input is a "basic primitive" and clipboard is off by default ([example](https://bevy.org/examples/ui-user-interface/text-input/)).
- Using it means adopting Bevy's ECS, renderer and release churn for the whole app. The voxel engine and path tracer would live inside Bevy's render graph. Too big a commitment for a UI choice.

## Xilem / Masonry

- Last release 0.4.0 about a year ago; repo last pushed 2026-09-14 ([releases](https://github.com/linebender/xilem/releases)). Self-described alpha ([docs.rs](https://docs.rs/crate/xilem/latest)). `masonry_core` can embed in existing wgpu apps ([Linebender, March 2025](https://linebender.org/blog/tmil-15/)). The web target is a separate DOM crate, `xilem_web`.
- 2025 test: IME works with some tofu, Narrator sees text at the wrong position ([boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)).
- Verdict: good foundations (Parley, Vello, AccessKit), not ready.

## GPUI (wildcard)

- Zed's UI framework, Apache-2.0 ([heise on Zed 1.0](https://heise.de/-11278661)). The Zed repo now has `gpui_web` and `gpui_wgpu` crates, with commits as recent as 2026-10-22 ([crates/](https://github.com/zed-industries/zed/tree/main/crates)). But on macOS it uses its own Metal renderer (`gpui_macos`), `gpui_wgpu`'s wasm path uses WebGL, and the last crates.io release is 0.2.2 from a year ago ([crates.io](https://crates.io/crates/gpui), [gpui_wgpu Cargo.toml](https://github.com/zed-industries/zed/blob/main/crates/gpui_wgpu/Cargo.toml)). Not usable as a dependency today.

## Web UI over the wgpu canvas

- **Browser and embed.** Natural fit: the editor UI is DOM over a WebGPU canvas, like Figma, whose engine is C++/wasm drawing to the canvas while "the UI around the canvas is in JavaScript", later TypeScript + React ([Evan Wallace](https://madebyevan.com/figma/building-a-professional-design-tool-on-the-web/)). Native text, IME, screen readers, CSS animation, backdrop blur, and `font-feature-settings`. UI weight is tiny next to the wasm core.
- **Desktop shell.** A native wgpu window with a transparent child webview on top; wry ships exactly this as an example ([wry examples/wgpu.rs](https://github.com/tauri-apps/wry/blob/dev/examples/wgpu.rs)). Our scene keeps full native wgpu; the webview only draws panels. Caveats in that example: it panics on Wayland, and Windows needs `with_clip_children(false)`. Webviews are WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux ([Tauri webview versions](https://v2.tauri.app/reference/webview-versions/)), so CSS must be tested three ways. Running the whole wasm build inside the webview instead is not an option on Linux: WebKitGTK has no WebGPU ([WebKit bug 257694](https://bugs.webkit.org/show_bug.cgi?id=257694)).
- **2025 test of Tauri:** Narrator sees everything, IME works but the composer draws in a screen corner ([boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)).
- **Costs.**
  1. Two languages and toolchains (Rust + TypeScript), unless the UI is written in Dioxus (see above).
  2. IPC for every UI action and every readout (FPS, voxel count, palette). Tauri IPC is async message passing ([Tauri IPC](https://v2.tauri.app/concept/inter-process-communication/)). Cursor-following hints need per-frame updates; estimate this is fine at 60 Hz but must be measured.
  3. Input routing: pointer events over the 3D view must pass through the webview to the native window, and focus must move cleanly between them. The wry example does not cover this; estimate this is the hardest part on desktop.
  4. Two compositors on desktop (native surface plus webview), so panel and scene can drift a frame on resize. Estimate, to verify.
  5. A web UI runtime in the desktop build (a webview process, plus Tauri if used).

## Pick for Phase 2

- **First: egui.** Same Rust code and same wgpu device on desktop and web, shared render pass, AccessKit on desktop, steady releases, Rerun as proof that a polished pro tool is reachable. Text got much better in the last three releases (hinting, variable fonts, colour emoji, system fonts, IME rework).
- **Runner-up: HTML/CSS over the canvas.** Build it first in the browser build (no shell needed, cheapest), then the wry child-webview overlay on the Mac. It wins on every polish axis; the bake-off must decide whether that is worth the IPC, input routing and second language.
- **Bake-off checks** (to be turned into thresholds in Phase 1 item 8):
  1. Side-by-side screenshots of the Hybrid toolbar, palette, hint bar and Cmd-K against the mockups, at 1x and 2x.
  2. Text at 11 to 13 px on a non-retina and a retina display; Japanese IME in the Cmd-K field.
  3. VoiceOver on the toolbar and palette, desktop and browser.
  4. Panel reveal and hover animation feel; frame time added to the 3D view.
  5. Gzip wasm size of the editor shell without the engine, and input latency from click to voxel.
