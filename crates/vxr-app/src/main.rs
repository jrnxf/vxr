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
//! - `vxr pair claude|codex [--root <dir>] [--no-bridge]` registers both with that agent when
//!   they are missing or changed (and, for Claude Code, installs `/vxr`), then runs `vxr
//!   bridge` (see [`pair`]).
//! - `vxr claude [claude args...]` does what `vxr pair claude --no-bridge` does, then starts
//!   Claude Code connected to the window from the start (see [`claude`]).
//! - `vxr setup claude|codex [--root <dir>]` registers both with that agent always, and `vxr
//!   setup app` adds `~/Applications/vxr.app` for Spotlight (see [`setup`]).
//! - `vxr setup --remove` undoes `pair` and `setup` (see [`setup`]).
//! - `vxr --version` prints the version, which a release build takes from its tag. `vxr help`
//!   and `vxr --help` list the commands and their flags. A scene that is not a file, a
//!   mistyped command and a second argument are errors before any window opens ([`start`]).
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
mod claude;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod net;
#[cfg(not(target_arch = "wasm32"))]
mod pair;
#[cfg(not(target_arch = "wasm32"))]
mod setup;
#[cfg(not(target_arch = "wasm32"))]
mod shot;

#[cfg(not(target_arch = "wasm32"))]
const HELP: &str = "\
usage: vxr [scene.vox]
           open the editor, with the scene if one is given
       vxr mcp --root <dir>
           serve MCP for a terminal agent on the .vox files under <dir>
       vxr mcp --live [--connect-at-start]
           serve MCP on the scene open in the editor (--connect-at-start: connect when the
           session starts, not when it first asks)
       vxr claude [args...]
           start Claude Code connected to the scene open in the editor
       vxr pair claude|codex [--root <dir>] [--no-bridge]
           pair a terminal agent: register both MCP servers if needed (and /vxr, which
           connects an open Claude Code session), then run the bridge for the browser editor
           (--root: the folder files mode serves, ~/vxr by default; --no-bridge: skip the
           bridge)
       vxr bridge
           let vxr mcp --live edit the scene open at vxr.jrnxf.co
       vxr setup claude|codex [--root <dir>]
           register both MCP servers with that agent, replacing them
       vxr setup app
           add vxr to Spotlight (~/Applications/vxr.app)
       vxr setup --remove
           undo pair and setup: remove vxr's MCP servers, /vxr and the Spotlight app
       vxr help | --help | <command> --help
       vxr --version";

/// The words that start a command, for the hint on a mistyped one.
#[cfg(not(target_arch = "wasm32"))]
const COMMANDS: [&str; 6] = ["mcp", "claude", "pair", "bridge", "setup", "help"];

/// What `vxr` with no command does: the arguments are at most one scene file.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, PartialEq)]
enum Start {
    Help,
    Version,
    /// Open the window, with this scene.
    Editor(Option<std::path::PathBuf>),
}

/// Read the arguments of the plain form, `vxr [scene.vox]`, before a window opens: a file that
/// is not there, a mistyped command and a second argument are errors here, not later in the
/// window. `is_file` says whether a path is a file.
#[cfg(not(target_arch = "wasm32"))]
fn start(args: &[String], is_file: impl Fn(&std::path::Path) -> bool) -> Result<Start, String> {
    const SEE: &str = "vxr --help lists the commands";
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Start::Help);
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        return Ok(Start::Version);
    }
    if let Some(flag) = args.iter().find(|a| a.starts_with('-')) {
        return Err(format!("unknown flag {flag}; {SEE}"));
    }
    match args {
        [] => Ok(Start::Editor(None)),
        [one] if is_file(one.as_ref()) => Ok(Start::Editor(Some(one.into()))),
        [one] => Err(match nearest_command(one) {
            Some(c) => format!("unknown command {one}; did you mean vxr {c}? {SEE}"),
            None => format!("{one}: no such file; {SEE}"),
        }),
        [first, rest @ ..] => Err(match nearest_command(first) {
            Some(c) => format!("unknown command {first}; did you mean vxr {c}? {SEE}"),
            None => format!("vxr opens one scene, but got {}: {first} {}; {SEE}", args.len(), rest.join(" ")),
        }),
    }
}

/// The command a mistyped `word` most likely meant: one within two edits, when the word does
/// not look like a file (no dot or slash).
#[cfg(not(target_arch = "wasm32"))]
fn nearest_command(word: &str) -> Option<&'static str> {
    if word.contains(['.', '/', '\\']) {
        return None;
    }
    let word = word.to_ascii_lowercase();
    COMMANDS.iter().map(|c| (edits(&word, c), *c)).filter(|(d, _)| *d <= 2).min_by_key(|(d, _)| *d).map(|(_, c)| c)
}

/// The edit distance between two words, a swap of neighbours counted as one.
#[cfg(not(target_arch = "wasm32"))]
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("mcp") => vxr_mcp::run(&args[1..]),
        Some("bridge") => vxr_mcp::bridge::run(&args[1..]),
        Some("pair") => pair::run(&args[1..]),
        Some("claude") => claude::run(&args[1..]),
        Some("setup") => setup::run(&args[1..]),
        Some("help") => {
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
        _ => match start(&args, |p| p.is_file()) {
            Ok(Start::Help) => {
                println!("{HELP}");
                Ok(())
            }
            Ok(Start::Version) => {
                println!("vxr {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
            Ok(Start::Editor(path)) => native::run(path, None),
            Err(e) => Err(e),
        },
    };
    if let Err(e) = result {
        eprintln!("vxr: {e}");
        std::process::exit(1);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Start, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        start(&args, |p| p == std::path::Path::new("scene.vox") || p == std::path::Path::new("pairs"))
    }

    #[test]
    fn a_scene_that_is_there_opens_and_nothing_else_reaches_the_window() {
        assert_eq!(run(&[]), Ok(Start::Editor(None)));
        assert_eq!(run(&["scene.vox"]), Ok(Start::Editor(Some("scene.vox".into()))));
        // A file whose name is close to a command still opens.
        assert_eq!(run(&["pairs"]), Ok(Start::Editor(Some("pairs".into()))));
        let missing = run(&["castle.vox"]).unwrap_err();
        assert!(missing.starts_with("castle.vox: no such file"), "{missing}");
        let two = run(&["scene.vox", "other.vox"]).unwrap_err();
        assert!(two.contains("opens one scene, but got 2"), "{two}");
    }

    #[test]
    fn a_mistyped_command_names_the_one_meant() {
        for (typed, meant) in [("piar", "pair"), ("setpu", "setup"), ("brige", "bridge"), ("Claude", "claude")] {
            let e = run(&[typed, "claude"]).unwrap_err();
            assert!(e.contains(&format!("did you mean vxr {meant}?")), "{typed}: {e}");
            let e = run(&[typed]).unwrap_err();
            assert!(e.contains(&format!("did you mean vxr {meant}?")), "{typed}: {e}");
        }
        // A word far from every command is a missing file.
        assert!(run(&["castle"]).unwrap_err().starts_with("castle: no such file"));
    }

    #[test]
    fn help_and_version_work_after_other_arguments() {
        assert_eq!(run(&["--help"]), Ok(Start::Help));
        assert_eq!(run(&["scene.vox", "-h"]), Ok(Start::Help));
        assert_eq!(run(&["-V"]), Ok(Start::Version));
        assert_eq!(run(&["scene.vox", "--version"]), Ok(Start::Version));
        assert!(run(&["--frobnicate"]).unwrap_err().starts_with("unknown flag --frobnicate"));
    }

    /// Every flag a command takes is in the main help, so nobody needs the README to find one.
    #[test]
    fn the_help_lists_every_command_and_flag() {
        for word in ["--root", "--no-bridge", "--connect-at-start", "--live", "--remove", "--version", "help"] {
            assert!(HELP.contains(word), "{word} is missing from vxr --help");
        }
        for c in COMMANDS {
            assert!(HELP.contains(&format!("vxr {c}")), "vxr {c} is missing from vxr --help");
        }
    }
}

// The browser build is the `vxr-web` crate; the binary does nothing there.
#[cfg(target_arch = "wasm32")]
fn main() {}
