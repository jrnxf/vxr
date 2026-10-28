//! The vxr desktop editor.
//!
//! - `vxr [scene.vox]` opens a window, with the scene if one is given.
//! - `vxr --stage <script.json> [scene.vox]` also runs a scripted AI request through the
//!   review step (see `vxr_app::review::Script`), so the card and ghost can be tried by hand.
//! - `vxr mcp --root <dir>` serves MCP on stdin and stdout: an agent opens, edits and saves the
//!   `.vox` files under `<dir>` (see `vxr-mcp`).
//! - `vxr --shot <dir> [scene.vox]` runs headless: it drives the editor through every tool,
//!   saves and reopens the result, and writes `app-1x.png` and `app-2x.png` into `<dir>`.
//! - `vxr --live <dir>`, only in a build with the `live` feature (`cargo run --release -p
//!   vxr-app --features live -- --live <dir>`), runs the assistant headless against the real
//!   providers whose keys are in `ANTHROPIC_API_KEY` and `OPENAI_API_KEY`, and writes
//!   screenshots and each answer's stream into `<dir>` (the live check; it spends a few cents
//!   per provider).

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod net;
#[cfg(not(target_arch = "wasm32"))]
mod shot;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("mcp") => vxr_mcp::run(&args[1..]),
        Some("--shot") => match args.get(1) {
            Some(dir) => shot::run(dir.as_ref(), args.get(2).map(|s| s.as_ref())),
            None => Err("usage: vxr --shot <dir> [scene.vox]".into()),
        },
        #[cfg(feature = "live")]
        Some("--live") => match args.get(1) {
            Some(dir) => shot::live(dir.as_ref()),
            None => Err("usage: vxr --live <dir>".into()),
        },
        Some("--stage") => match args.get(1) {
            Some(script) => native::run(args.get(2).map(Into::into), Some(script.into())),
            None => Err("usage: vxr --stage <script.json> [scene.vox]".into()),
        },
        Some(flag) if flag.starts_with('-') => Err(format!("unknown flag {flag}; usage: vxr [scene.vox]")),
        path => native::run(path.map(Into::into), None),
    };
    if let Err(e) = result {
        eprintln!("vxr: {e}");
        std::process::exit(1);
    }
}

// The browser build is a library entry point (not yet written); the binary does nothing there.
#[cfg(target_arch = "wasm32")]
fn main() {}
