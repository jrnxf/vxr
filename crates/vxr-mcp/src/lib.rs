//! `vxr mcp --root <dir>`: a stdio MCP server that opens, edits and saves `.vox` files under
//! one folder (`docs/research/ai.md` section 3, files mode). `vxr mcp --live` edits the
//! scene open in the running desktop app instead, over a local socket ([`live`]), or in the
//! browser editor, whose socket `vxr bridge` hosts ([`bridge`]).
//!
//! - One tool per `vxr-ops` operation, with the schemas generated from its types, plus
//!   `list_files`, `new_scene`, `open`, `edit_object` and `save` ([`files`]).
//! - Resources: `vxr://scene/summary`, `vxr://scene/palette` and one per `.vox` under the root.
//! - Transport: newline-delimited JSON-RPC on stdin and stdout, MCP revision 2026-07-28, and
//!   the `initialize` handshake of the earlier revisions ([`protocol`]).
//!
//! It makes no model calls, has no network or shell tool, and reaches only files under the
//! root. No GPU.

pub mod bridge;
#[cfg(all(test, unix))]
mod bridge_tests;
pub mod files;
pub mod fixture;
pub mod live;
#[cfg(all(test, unix))]
mod live_tests;
pub mod protocol;
#[cfg(test)]
mod tests;

pub use protocol::{Backend, Server, serve};

use std::path::PathBuf;

const USAGE: &str = "usage: vxr mcp --root <dir> | vxr mcp --live";

const HELP: &str = r#"usage: vxr mcp --root <dir> | vxr mcp --live

Serves MCP on stdin and stdout for a terminal agent such as Claude Code.

  --root <dir>  open, edit and save the .vox files under <dir>
  --live        edit the scene open in the vxr window, once its Terminal agents panel
                (Cmd-K "Terminal agents (MCP)…") allows it, or in a vxr.jrnxf.co tab
                paired with `vxr bridge`

`vxr setup claude` (or codex) registers both with the agent."#;

/// `vxr mcp`'s arguments (after the subcommand): serve on stdin and stdout until it closes.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut root = None;
    let mut live = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = Some(it.next().ok_or(USAGE)?.clone()),
            "--live" => live = true,
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}; {USAGE}")),
        }
    }
    if live {
        if root.is_some() {
            return Err(format!("--live edits the open window, not files: give one of them; {USAGE}"));
        }
        return serve_live();
    }
    let root = root.ok_or(USAGE)?;
    let root = std::fs::canonicalize(&root).map_err(|e| format!("{root}: {e}"))?;
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    serve_stdio(root)
}

#[cfg(unix)]
fn serve_live() -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let server = Server::with(live::Live(live::Remote::new(live::dir())));
    serve(server, stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn serve_live() -> Result<(), String> {
    Err("live mode is not built for this platform yet; use files mode (--root <dir>)".into())
}

fn serve_stdio(root: PathBuf) -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(Server::new(root), stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}
