//! The vxr desktop editor.
//!
//! - `vxr [scene.vox]` opens a window, with the scene if one is given.
//! - `vxr --stage <script.json> [scene.vox]` also runs a scripted AI request through the
//!   review step (see `vxr_app::review::Script`), so the card and ghost can be tried by hand.
//! - `vxr mcp --root <dir>` serves MCP on stdin and stdout: an agent opens, edits and saves the
//!   `.vox` files under `<dir>` (see `vxr-mcp`). `vxr mcp --live` edits the scene open in a
//!   running window instead, once the assistant's Agents tab (Cmd-K) allows it.
//! - `vxr bridge` lets `vxr mcp --live` edit the scene open in the browser editor at
//!   vxr.jrnxf.co instead: the page pairs with it over a loopback WebSocket with the code it
//!   prints (see `vxr_mcp::bridge`).
//! - `vxr setup claude|codex [--root <dir>]` registers both with that agent, and `vxr setup app`
//!   adds `~/Applications/vxr.app` for Spotlight (see [`setup`]).
//! - `vxr --version` prints the version, which a release build takes from its tag.
//! - `vxr --shot <dir> [scene.vox]` runs headless: it drives the editor through every tool,
//!   saves and reopens the result, and writes its screenshots (`app-1x.png`, `app-2x.png` and
//!   one per feature) and exported files into `<dir>`.
//! - `vxr --live <dir>`, only in a build with the `live` feature (`cargo run --release -p
//!   vxr-app --features live -- --live <dir>`), runs the assistant headless against the real
//!   providers whose keys are in `ANTHROPIC_API_KEY` and `OPENAI_API_KEY`, and writes
//!   screenshots and each answer's stream into `<dir>` (the live check; it spends a few cents
//!   per provider).
//! - `vxr --agents <dir> [scene.vox]`, in the same `live` build, runs the editor headless with
//!   terminal agents allowed, for the live mode check from a real agent (`vxr mcp --live`),
//!   writing screenshots into `<dir>`.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod net;
#[cfg(not(target_arch = "wasm32"))]
mod setup;
#[cfg(not(target_arch = "wasm32"))]
mod shot;

#[cfg(not(target_arch = "wasm32"))]
const HELP: &str = "\
usage: vxr [scene.vox]          open the editor, with the scene if one is given
       vxr mcp --root <dir>     serve MCP for a terminal agent on the .vox files under <dir>
       vxr mcp --live           serve MCP on the scene open in the editor
       vxr bridge               let vxr mcp --live edit the scene open at vxr.jrnxf.co
       vxr setup claude|codex   register both MCP servers with that agent
       vxr setup app            add vxr to Spotlight (~/Applications/vxr.app)
       vxr --version";

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("mcp") => vxr_mcp::run(&args[1..]),
        Some("bridge") => vxr_mcp::bridge::run(&args[1..]),
        Some("setup") => setup::run(&args[1..]),
        Some("-V" | "--version") => {
            println!("vxr {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("-h" | "--help") => {
            println!("{HELP}");
            Ok(())
        }
        Some("--shot") => match args.get(1) {
            Some(dir) => shot::run(dir.as_ref(), args.get(2).map(|s| s.as_ref())),
            None => Err("usage: vxr --shot <dir> [scene.vox]".into()),
        },
        #[cfg(all(feature = "live", unix))]
        Some("--agents") => match args.get(1) {
            Some(dir) => shot::agents(dir.as_ref(), args.get(2).map(|s| s.as_ref())),
            None => Err("usage: vxr --agents <dir> [scene.vox]".into()),
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
        Some(flag) if flag.starts_with('-') => Err(format!("unknown flag {flag}; vxr --help lists the commands")),
        path => native::run(path.map(Into::into), None),
    };
    if let Err(e) = result {
        eprintln!("vxr: {e}");
        std::process::exit(1);
    }
}

// The browser build is the `vxr-web` crate; the binary does nothing there.
#[cfg(target_arch = "wasm32")]
fn main() {}
