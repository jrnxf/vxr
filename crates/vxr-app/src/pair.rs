//! `vxr pair claude|codex [--root <dir>] [--no-bridge]`: one command that leaves a terminal
//! agent ready to edit scenes, the first time or the tenth.
//!
//! - It registers vxr's two MCP servers with the agent (see [`crate::setup`]), but only the
//!   ones that are missing or differ from what it would register: with both in place it changes
//!   nothing and says so in one line.
//! - The desktop app needs nothing else: `vxr mcp --live` finds the window's socket by itself,
//!   and the user allows it from the Agents tab. So `--no-bridge` ends here and says the agent
//!   can start.
//! - The browser editor does: its page cannot host that socket, so without `--no-bridge` pair
//!   goes on to run `vxr bridge` in the foreground and prints the pairing code to type into the
//!   page, exactly as `vxr bridge` does.

use std::path::{Path, PathBuf};

use crate::setup::{self, AGENTS, Agent};

const USAGE: &str = "usage: vxr pair claude|codex [--root <dir>] [--no-bridge]";

const HELP: &str = "usage: vxr pair claude|codex [--root <dir>] [--no-bridge]

Pairs vxr with a terminal agent in one step; run it again any time. It registers vxr's MCP
servers with the agent, only the ones that are missing or changed:
  vxr       files mode on the .vox files under --root (default ~/vxr)
  vxr-live  live mode, the scene open in the vxr window or the browser tab
Then, for the browser editor at https://vxr.jrnxf.co, it runs the bridge and prints a pairing
code to type into the Agents tab (Cmd-K, \"Terminal agents (MCP)…\"). Ctrl-C stops it.
--no-bridge: for the desktop app, which needs no bridge. Registers and exits.";

/// `vxr pair`'s arguments (after the subcommand).
pub fn run(args: &[String]) -> Result<(), String> {
    let mut agent = None;
    let mut root = None;
    let mut bridge = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            "--no-bridge" => bridge = false,
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            name => match AGENTS.iter().find(|g| g.name == name) {
                Some(g) if agent.is_none() => agent = Some(*g),
                _ => return Err(format!("unknown argument {name}; {USAGE}")),
            },
        }
    }
    let agent = agent.ok_or(USAGE)?;
    let root = setup::files_root(root)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("could not make {}: {e}", root.display()))?;
    let exe = std::env::current_exe().map_err(|e| format!("could not find the vxr binary: {e}"))?;
    for line in pair(agent, Path::new(agent.name), &setup::servers(&setup::stable_exe(&exe), &root))? {
        println!("{line}");
    }
    if !bridge {
        println!("Start {} when you like: it edits the scene open in the vxr app.", agent.name);
        return Ok(());
    }
    println!();
    vxr_mcp::bridge::run(&[])
}

/// Register the servers `agent` (run as `program`) lacks or has differently, and say what was
/// done: one line per server changed, or one line when there was nothing to change.
pub fn pair(agent: Agent, program: &Path, servers: &[setup::Server]) -> Result<Vec<String>, String> {
    let stale: Vec<_> = servers.iter().filter(|s| !setup::is_registered(agent, program, s)).collect();
    if stale.is_empty() {
        return Ok(vec![format!("{}: already paired, nothing to change", agent.name)]);
    }
    let stale: Vec<_> = stale.into_iter().map(|s| setup::Server { name: s.name, command: s.command.clone() }).collect();
    setup::register(agent, program, &stale)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake agent CLI that keeps each added server's arguments in a file, shows them for
    /// `get` like `claude mcp get`, and logs every call.
    fn fake(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let fake = dir.join("claude");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{d}/log'\nfor a; do [ \"$a\" = -- ] && break; name=$a; done\n\
                 case $2 in\n\
                 get) [ -e '{d}/'\"$name\" ] && {{ echo \"Command: $(sed -n 1p '{d}/'\"$name\")\"; echo \"Args: $(sed -n 2p '{d}/'\"$name\")\"; }};;\n\
                 remove) [ -e '{d}/'\"$name\" ];;\n\
                 add) shift; shift; while [ \"$1\" != -- ]; do shift; done; shift; echo \"$1\" > '{d}/'\"$name\"; shift; echo \"$*\" >> '{d}/'\"$name\";;\n\
                 esac\n",
                d = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        fake
    }

    #[test]
    fn registers_what_is_missing_and_changes_nothing_when_it_is_all_there() {
        let dir = std::env::temp_dir().join(format!("vxr-pair-{}", std::process::id()));
        let program = fake(&dir);
        let exe = Path::new("/opt/homebrew/bin/vxr");
        let servers = setup::servers(exe, Path::new("/home/user/vxr"));
        let first = pair(AGENTS[0], &program, &servers).unwrap();
        assert_eq!(first.len(), 2, "{first:?}");
        assert!(first.iter().all(|l| l.starts_with("claude: added vxr")), "{first:?}");
        let before = std::fs::read_to_string(dir.join("log")).unwrap().lines().count();
        // A second run only asks what is registered.
        let second = pair(AGENTS[0], &program, &servers).unwrap();
        assert_eq!(second, ["claude: already paired, nothing to change"]);
        let log = std::fs::read_to_string(dir.join("log")).unwrap();
        let after: Vec<_> = log.lines().skip(before).collect();
        assert_eq!(after, ["mcp get vxr", "mcp get vxr-live"], "a second run only asks");
        // A new root replaces only files mode; live mode is left alone.
        let moved = setup::servers(exe, Path::new("/home/user/scenes"));
        let third = pair(AGENTS[0], &program, &moved).unwrap();
        assert_eq!(third, ["claude: replaced vxr: /opt/homebrew/bin/vxr mcp --root /home/user/scenes"]);
        // A moved binary replaces both.
        let other = setup::servers(Path::new("/usr/local/bin/vxr"), Path::new("/home/user/scenes"));
        let fourth = pair(AGENTS[0], &program, &other).unwrap();
        assert_eq!(fourth.len(), 2, "{fourth:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_cli_says_how_to_add_by_hand() {
        let servers = setup::servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        let err = pair(AGENTS[1], Path::new("/nonexistent/codex"), &servers).unwrap_err();
        assert!(err.contains("codex mcp add vxr -- /opt/homebrew/bin/vxr mcp --root /home/user/vxr"), "{err}");
    }
}
