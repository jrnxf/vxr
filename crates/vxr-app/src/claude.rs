//! `vxr claude [claude args...]` starts Claude Code already connected to the vxr window, and
//! `/vxr` ([`install_command`]) connects a Claude Code session that is already open.
//!
//! - `vxr mcp --live` stays quiet until a session asks to connect, so the user-scope
//!   `vxr-live` that `vxr pair` registers never joins the window from every Claude session the
//!   user opens elsewhere.
//! - `vxr claude` first makes sure the servers are registered as `vxr pair claude --no-bridge`
//!   would (keeping files mode's root if one is set) and `/vxr` is installed, then runs `claude`
//!   in this terminal with `--mcp-config` naming a `vxr-live` that connects at start
//!   (`--connect-at-start`). A server from `--mcp-config` replaces the user-scope one of the
//!   same name for that session, so it has one `vxr-live`, and only this session connects at
//!   start. The other arguments go to `claude` as they are.
//! - `/vxr` is a user-level Claude Code command file, `~/.claude/commands/vxr.md` (under
//!   `$CLAUDE_CONFIG_DIR` when that is set), that has the model call `vxr-live`'s `connect`
//!   tool. It is written when missing, replaced when it is an older one of vxr's (it carries
//!   [`MARK`]), and left alone when it is the user's own.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::setup::{self, AGENTS};

const HELP: &str = "usage: vxr claude [claude args...]

Starts Claude Code in this terminal, connected to the scene open in the vxr window: it shows
in the Agents tab (Cmd-K, \"Terminal agents (MCP)…\") at once. Other Claude Code sessions stay
quiet until you type /vxr in them. First it registers vxr's MCP servers and installs /vxr, as
vxr pair claude --no-bridge does, if they are missing or changed. Every other argument goes to
claude as it is.";

/// The `/vxr` command file's name, in Claude Code's `commands` folder.
pub const COMMAND_FILE: &str = "vxr.md";

/// The line that marks a `/vxr` file as vxr's own, so a later vxr may replace it.
pub const MARK: &str = "<!-- Written by vxr pair claude, which replaces this file while this line is here. Delete the line to keep your changes. -->";

/// The `/vxr` command file.
pub fn command_text() -> String {
    format!(
        "---\ndescription: Connect this Claude Code session to the vxr window\nallowed-tools: mcp__vxr-live__connect\n---\n{MARK}\n\n\
         Call the `connect` tool of the vxr-live MCP server (mcp__vxr-live__connect) now, and tell me in one line what it \
         answered. If that tool is not there, say that vxr-live is not registered and that `vxr pair claude --no-bridge` \
         registers it.\n"
    )
}

/// Claude Code's user folder: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_dir() -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(d.into());
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".claude"))
        .ok_or_else(|| "HOME is not set".into())
}

/// Install `/vxr` in `commands` (Claude Code's `commands` folder), and say what was done, or
/// None when it was already there. A file of the same name that is not vxr's is left as it is,
/// and so is one that cannot be written: the line says so and how to add it by hand.
pub fn install_command(commands: &Path) -> Option<String> {
    let file = commands.join(COMMAND_FILE);
    let shown = file.display();
    let want = command_text();
    let verb = match std::fs::read_to_string(&file) {
        Ok(have) if have == want => return None,
        Ok(have) if have.contains(MARK) => "replaced",
        Ok(_) => {
            return Some(format!(
                "claude: left {shown} alone: it is not vxr's, so /vxr runs it; in a session, ask Claude to call vxr-live's connect tool instead"
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "added",
        Err(e) => return Some(format!("claude: could not read {shown} ({e}), so /vxr is not installed")),
    };
    match std::fs::create_dir_all(commands).and_then(|()| std::fs::write(&file, &want)) {
        Ok(()) => Some(format!("claude: {verb} /vxr: {shown}")),
        Err(e) => Some(format!(
            "claude: could not write {shown} ({e}), so /vxr is not installed; in a session, ask Claude to call vxr-live's connect tool instead"
        )),
    }
}

/// The `--mcp-config` for this session: a `vxr-live` run by `exe` that connects at start.
pub fn mcp_config(exe: &Path) -> String {
    serde_json::json!({"mcpServers": {"vxr-live": {
        "type": "stdio",
        "command": exe.display().to_string(),
        "args": ["mcp", "--live", "--connect-at-start"],
    }}})
    .to_string()
}

/// `claude` (as `program`) with this session's `vxr-live` and the user's `args`. The config
/// goes in as `--mcp-config=<json>`: the flag takes several values, and in that form it takes
/// only this one, so a prompt or flag after it stays the user's.
pub fn command(program: &Path, exe: &Path, args: &[String]) -> Command {
    let mut c = Command::new(program);
    c.arg(format!("--mcp-config={}", mcp_config(exe))).args(args);
    c
}

/// Everything `vxr claude` does before it starts `claude` (run as `program`): register the
/// servers that are missing or differ (files mode keeps any root it already has, else
/// `root`) and install `/vxr` in `commands`. Says what it changed, nothing when it changed
/// nothing.
pub fn prepare(program: &Path, exe: &Path, root: &Path, commands: &Path) -> Result<Vec<String>, String> {
    let agent = AGENTS[0];
    let servers = setup::servers(exe, root);
    let stale: Vec<setup::Server> = servers
        .into_iter()
        .filter(|s| {
            let keep_root = s.name == "vxr" && setup::is_registered_any_root(agent, program, exe);
            !keep_root && !setup::is_registered(agent, program, s)
        })
        .collect();
    let mut done = if stale.is_empty() { Vec::new() } else { setup::register(agent, program, &stale)? };
    done.extend(install_command(commands));
    Ok(done)
}

/// `vxr claude`'s arguments (after the subcommand).
pub fn run(args: &[String]) -> Result<(), String> {
    if matches!(args, [one] if one == "-h" || one == "--help") {
        println!("{HELP}");
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| format!("could not find the vxr binary: {e}"))?;
    let exe = setup::stable_exe(&exe);
    let root = setup::files_root(None)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("could not make {}: {e}", root.display()))?;
    let program = Path::new("claude");
    for line in prepare(program, &exe, &root, &claude_dir()?.join("commands"))? {
        eprintln!("{line}");
    }
    let mut c = command(program, &exe, args);
    #[cfg(unix)]
    {
        // Claude Code takes this process's place, and with it the terminal and its signals.
        use std::os::unix::process::CommandExt;
        let e = c.exec();
        Err(format!("could not start claude: {e}"))
    }
    #[cfg(not(unix))]
    {
        let status = c.status().map_err(|e| format!("could not start claude: {e}"))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A fake `claude` that answers `mcp get` from the servers added before (as the pair
    /// tests' fake does), logs `mcp` calls to `log` and any other run's arguments, one per
    /// line, to `run`.
    fn fake(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let fake = dir.join("claude");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nif [ \"$1\" != mcp ]; then for a; do echo \"$a\"; done > '{d}/run'; exit 0; fi\n\
                 echo \"$*\" >> '{d}/log'\nfor a; do [ \"$a\" = -- ] && break; name=$a; done\n\
                 case $2 in\n\
                 get) [ -e '{d}/'\"$name\" ] && {{ echo \"Scope: User config\"; echo \"Command: $(sed -n 1p '{d}/'\"$name\")\"; echo \"Args: $(sed -n 2p '{d}/'\"$name\")\"; }};;\n\
                 remove) [ -e '{d}/'\"$name\" ] && rm '{d}/'\"$name\";;\n\
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
    fn vxr_claude_registers_installs_vxr_and_starts_claude_with_its_own_vxr_live() {
        let dir = std::env::temp_dir().join(format!("vxr-claude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let program = fake(&dir);
        let commands = dir.join("home/.claude/commands");
        let exe = Path::new("/opt/homebrew/bin/vxr");
        let root = Path::new("/home/user/vxr");
        // The first time: both servers and /vxr.
        let first = prepare(&program, exe, root, &commands).unwrap();
        assert_eq!(
            first,
            [
                "claude: added vxr: /opt/homebrew/bin/vxr mcp --root /home/user/vxr".to_string(),
                "claude: added vxr-live: /opt/homebrew/bin/vxr mcp --live".to_string(),
                format!("claude: added /vxr: {}", commands.join(COMMAND_FILE).display()),
            ]
        );
        assert_eq!(std::fs::read_to_string(commands.join(COMMAND_FILE)).unwrap(), command_text());
        // The second time changes nothing and says nothing.
        assert!(prepare(&program, exe, root, &commands).unwrap().is_empty());
        // A root the user paired with is kept.
        let mine = setup::servers(exe, Path::new("/home/user/scenes"));
        setup::register(AGENTS[0], &program, &mine[..1]).unwrap();
        assert!(prepare(&program, exe, root, &commands).unwrap().is_empty(), "files mode keeps its root");
        assert!(setup::is_registered(AGENTS[0], &program, &mine[0]));

        // The command line: this session's vxr-live, then the user's arguments untouched.
        let args = vec!["--model".to_string(), "opus".into(), "make a red roof".into()];
        assert!(command(&program, exe, &args).status().unwrap().success());
        let run: Vec<String> = std::fs::read_to_string(dir.join("run")).unwrap().lines().map(String::from).collect();
        assert_eq!(run.len(), 4, "{run:?}");
        let config = run[0].strip_prefix("--mcp-config=").expect("one --mcp-config=<json> argument");
        let config: serde_json::Value = serde_json::from_str(config).unwrap();
        assert_eq!(
            config,
            serde_json::json!({"mcpServers": {"vxr-live": {"type": "stdio", "command": "/opt/homebrew/bin/vxr",
                "args": ["mcp", "--live", "--connect-at-start"]}}}),
            "one vxr-live, by the user-scope name, so it replaces that one for this session"
        );
        assert_eq!(run[1..], args);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vxr_is_installed_once_updated_when_vxrs_and_never_over_the_users_own() {
        let dir = std::env::temp_dir().join(format!("vxr-command-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let commands = dir.join("commands");
        let file = commands.join(COMMAND_FILE);
        assert!(install_command(&commands).unwrap().starts_with("claude: added /vxr"));
        let first = std::fs::metadata(&file).unwrap().modified().unwrap();
        assert_eq!(install_command(&commands), None, "a second install changes nothing");
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), first);
        // An older vxr's file (it carries the mark) is replaced.
        std::fs::write(&file, format!("{MARK}\nan older text\n")).unwrap();
        assert!(install_command(&commands).unwrap().starts_with("claude: replaced /vxr"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), command_text());
        // The user's own vxr.md is never touched.
        std::fs::write(&file, "my own /vxr\n").unwrap();
        let line = install_command(&commands).unwrap();
        assert!(line.contains("left") && line.contains("not vxr's"), "{line}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "my own /vxr\n");
        // A folder that cannot be written says so and fails nothing.
        std::fs::remove_file(&file).unwrap();
        std::fs::set_permissions(&commands, std::fs::Permissions::from_mode(0o500)).unwrap();
        let line = install_command(&commands).unwrap();
        assert!(line.contains("could not write"), "{line}");
        std::fs::set_permissions(&commands, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_command_calls_the_connect_tool_and_may_run_it_without_asking() {
        let t = command_text();
        assert!(t.starts_with("---\ndescription: "));
        assert!(t.contains("allowed-tools: mcp__vxr-live__connect\n"));
        assert!(t.contains(MARK));
        assert_eq!(vxr_mcp::live::CONNECT, "connect", "the tool the command names");
    }
}
