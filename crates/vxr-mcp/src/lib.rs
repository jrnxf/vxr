//! `vxr mcp --root <dir>`: a stdio MCP server that opens, edits and saves `.vox` files under
//! one folder (`docs/research/ai.md` section 3, files mode).
//!
//! - One tool per `vxr-ops` operation, with the schemas generated from its types, plus
//!   `list_files`, `new_scene`, `open`, `edit_object` and `save` ([`files`]).
//! - Resources: `vxr://scene/summary`, `vxr://scene/palette` and one per `.vox` under the root.
//! - Transport: newline-delimited JSON-RPC on stdin and stdout, MCP revision 2026-07-28, and
//!   the `initialize` handshake of the earlier revisions ([`protocol`]).
//!
//! It makes no model calls, has no network or shell tool, and reaches only files under the
//! root. No GPU.

pub mod files;
pub mod fixture;
pub mod protocol;
#[cfg(test)]
mod tests;

pub use protocol::{Server, serve};

use std::path::PathBuf;

const USAGE: &str = "usage: vxr mcp --root <dir>";

/// `vxr mcp`'s arguments (after the subcommand): serve on stdin and stdout until it closes.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut root = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = Some(it.next().ok_or(USAGE)?.clone()),
            other => return Err(format!("unknown argument {other}; {USAGE}")),
        }
    }
    let root = root.ok_or(USAGE)?;
    let root = std::fs::canonicalize(&root).map_err(|e| format!("{root}: {e}"))?;
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    serve_stdio(root)
}

fn serve_stdio(root: PathBuf) -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(Server::new(root), stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}
