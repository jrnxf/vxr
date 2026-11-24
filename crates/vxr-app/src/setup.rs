//! `vxr setup claude|codex [--root <dir>]`: registers vxr's two MCP servers with a terminal
//! agent through the agent's own CLI, for every project of the user:
//!
//! - `vxr`: files mode, `vxr mcp --root <dir>`, on `~/vxr` unless `--root` names another folder
//!   (made if missing).
//! - `vxr-live`: live mode, `vxr mcp --live`, the scene open in the desktop app.
//!
//! Each server is removed first and added again (the agents' CLIs refuse to add over a name
//! that is taken), so a second run leaves the same setup (with the new root, if one is given)
//! and says it replaced them. A server of the same name that is not vxr's is never replaced,
//! and when the add fails the entry removed is put back.
//!
//! `vxr setup --remove` undoes all of it for both agents: vxr's two servers, `/vxr` and the
//! Spotlight app. It leaves the user's scenes, keys and library, and says where they are.
//!
//! `vxr setup app` makes `~/Applications/vxr.app` for Spotlight: a folder whose `Contents` links
//! to the `vxr.app` this binary runs from (Homebrew's `opt` path, so it follows upgrades).
//! Spotlight indexes that as an app; it skips a symlink to the app and Homebrew's folders.

use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: vxr setup claude|codex [--root <dir>] | vxr setup app | vxr setup --remove";

const HELP: &str = "usage: vxr setup claude|codex [--root <dir>]
       vxr setup app
       vxr setup --remove

claude, codex: registers vxr's MCP servers with the agent, for every project:
  vxr       files mode on the .vox files under --root (default ~/vxr)
  vxr-live  live mode, the scene open in the vxr window
app: adds ~/Applications/vxr.app, so Spotlight opens vxr
--remove: undoes vxr pair and vxr setup, before an uninstall: removes vxr's MCP servers from
  Claude Code and Codex, /vxr (~/.claude/commands/vxr.md) and ~/Applications/vxr.app. It
  leaves your scenes, keys and library, and says where they are.";

/// A terminal agent's CLI: how it removes and adds a stdio MCP server for every project.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Agent {
    pub name: &'static str,
    get: &'static [&'static str],
    remove: &'static [&'static str],
    add: &'static [&'static str],
}

pub const AGENTS: [Agent; 2] = [
    Agent {
        name: "claude",
        get: &["mcp", "get"],
        remove: &["mcp", "remove", "--scope", "user"],
        add: &["mcp", "add", "--scope", "user"],
    },
    Agent { name: "codex", get: &["mcp", "get"], remove: &["mcp", "remove"], add: &["mcp", "add"] },
];

/// One server to register: its name and the command the agent runs.
#[derive(Debug, PartialEq)]
pub struct Server {
    pub name: &'static str,
    pub command: Vec<String>,
}

/// Whether the agent already has `server` registered as it would be: its `mcp get` succeeds and
/// its `command` and `args` lines equal the server's exactly, at user scope (a `Scope:` line,
/// which Claude prints for the entry that wins in the current folder, must say User) and not
/// disabled (Codex's `enabled: false`). Anything else (a different path or root, a local or
/// project entry, the server missing, an output this does not recognise) is false, and the
/// server is registered again, which is safe.
pub fn is_registered(agent: Agent, program: &Path, server: &Server) -> bool {
    let Ok(out) = Command::new(program).args(agent.get).arg(server.name).output() else { return false };
    out.status.success() && matches_get(&String::from_utf8_lossy(&out.stdout), server)
}

/// Whether the agent has files mode (`vxr`) registered as [`is_registered`] would accept it,
/// run by `exe`, on any root: `vxr claude` keeps the folder the user paired with.
pub fn is_registered_any_root(agent: Agent, program: &Path, exe: &Path) -> bool {
    let Ok(out) = Command::new(program).args(agent.get).arg("vxr").output() else { return false };
    if !out.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let root = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Args: mcp --root ").or(l.trim().strip_prefix("args: mcp --root ")));
    root.is_some_and(|r| !r.trim().is_empty() && matches_get(&text, &servers(exe, Path::new(r.trim()))[0]))
}

/// Whether the text of an `mcp get` shows `server`: see [`is_registered`].
pub(crate) fn matches_get(text: &str, server: &Server) -> bool {
    let (mut command, mut args, mut scope_ok, mut enabled) = (None, None, true, true);
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(':') else { continue };
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "command" => command = Some(value),
            "args" => args = Some(value),
            "scope" => scope_ok = value.starts_with("User"),
            "enabled" => enabled = value != "false",
            _ => {}
        }
    }
    scope_ok
        && enabled
        && command == Some(server.command[0].as_str())
        && args.unwrap_or("") == server.command[1..].join(" ")
}

/// The folder files mode serves, and the folder's absolute path: `root` or `~/vxr`.
pub fn files_root(root: Option<PathBuf>) -> Result<PathBuf, String> {
    match root {
        Some(r) => std::path::absolute(&r).map_err(|e| format!("{}: {e}", r.display())),
        None => Ok(home()?.join("vxr")),
    }
}

/// Make the folder files mode serves if it is missing, and say so when it was made: nothing
/// appears in the user's home folder without a line about it.
pub fn make_root(root: &Path) -> Result<Option<String>, String> {
    if root.is_dir() {
        return Ok(None);
    }
    std::fs::create_dir_all(root).map_err(|e| format!("could not make {}: {e}", root.display()))?;
    Ok(Some(format!("files: made {} for the scenes files mode edits", root.display())))
}

/// `vxr setup`'s arguments (after the subcommand).
pub fn run(args: &[String]) -> Result<(), String> {
    if let [only] = args
        && only == "app"
    {
        let line = app(&home()?.join("Applications/vxr.app"))?;
        println!("{line}");
        return Ok(());
    }
    if let [only] = args
        && only == "--remove"
    {
        let home = home()?;
        for agent in AGENTS {
            for line in unregister(agent, Path::new(agent.name)) {
                println!("{line}");
            }
        }
        match crate::claude::claude_dir() {
            Ok(d) => println!("{}", crate::claude::remove_command(&d.join("commands"))),
            Err(e) => println!("claude: /vxr was not looked for: {e}"),
        }
        println!("{}", unlink_app(&home.join("Applications/vxr.app")));
        println!("{}", left_behind(&home));
        return Ok(());
    }
    let mut agent = None;
    let mut root = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
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
    let root = files_root(root)?;
    if let Some(line) = make_root(&root)? {
        println!("{line}");
    }
    let exe = std::env::current_exe().map_err(|e| format!("could not find the vxr binary: {e}"))?;
    for line in register(agent, Path::new(agent.name), &servers(&stable_exe(&exe), &root))? {
        println!("{line}");
    }
    Ok(())
}

/// Link `dest` to the `vxr.app` this binary runs from, and say what was done.
fn app(dest: &Path) -> Result<String, String> {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("could not find the vxr binary: {e}"))?;
    let contents = stable_exe(&exe)
        .parent()
        .and_then(Path::parent)
        .filter(|c| c.ends_with("Contents") && c.parent().is_some_and(|a| a.extension().is_some_and(|e| e == "app")))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!("{} is not inside vxr.app; install vxr with brew install jrnxf/tap/vxr", exe.display())
        })?;
    link_app(&contents, dest)
}

/// Make `dest` a folder whose `Contents` links to `contents`, replacing an earlier one (or a
/// symlink to an app), but never anything else.
pub fn link_app(contents: &Path, dest: &Path) -> Result<String, String> {
    let shown = dest.display();
    let inner = dest.join("Contents");
    let verb = match std::fs::symlink_metadata(dest) {
        Err(_) => "added",
        Ok(m) if m.is_symlink() => {
            std::fs::remove_file(dest).map_err(|e| format!("could not replace {shown}: {e}"))?;
            "replaced"
        }
        Ok(m) if m.is_dir() && std::fs::symlink_metadata(&inner).is_ok_and(|c| c.is_symlink()) => {
            std::fs::remove_file(&inner).map_err(|e| format!("could not replace {shown}: {e}"))?;
            "replaced"
        }
        Ok(_) => return Err(format!("{shown} is already there and is not vxr's link; move it away first")),
    };
    std::fs::create_dir_all(dest).map_err(|e| format!("could not make {shown}: {e}"))?;
    symlink(contents, &inner).map_err(|e| format!("could not link {}: {e}", inner.display()))?;
    Ok(format!("app: {verb} {shown}, linked to {}; Spotlight opens vxr through it", contents.display()))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("vxr setup app is for macOS"))
}

/// The two servers, run by `exe`.
pub fn servers(exe: &Path, root: &Path) -> Vec<Server> {
    let exe = exe.display().to_string();
    vec![
        Server { name: "vxr", command: vec![exe.clone(), "mcp".into(), "--root".into(), root.display().to_string()] },
        Server { name: "vxr-live", command: vec![exe, "mcp".into(), "--live".into()] },
    ]
}

/// What an agent's `mcp get` shows of a server: its command and its arguments, as one line.
/// None when the output has no command line (the server is missing, or the output is one this
/// does not recognise).
pub(crate) fn shown_entry(text: &str) -> Option<(String, String)> {
    let (mut command, mut args) = (None, String::new());
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(':') else { continue };
        match key.to_ascii_lowercase().as_str() {
            "command" => command = Some(value.trim().to_string()),
            "args" => args = value.trim().to_string(),
            _ => {}
        }
    }
    command.filter(|c| !c.is_empty()).map(|c| (c, args))
}

/// Whether a registered entry is one vxr made: a binary named `vxr` run as `mcp --root <dir>`
/// or `mcp --live`. Another tool that took the name `vxr` is not.
pub(crate) fn is_ours(command: &str, args: &str) -> bool {
    let named_vxr = Path::new(command).file_name().is_some_and(|n| n == "vxr" || n == "vxr.exe");
    named_vxr && (args.starts_with("mcp --root ") || args == "mcp --live" || args.starts_with("mcp --live "))
}

/// The entry the agent has under `name`, if its `mcp get` shows one.
fn registered(agent: Agent, program: &Path, name: &str) -> std::io::Result<Option<(String, String)>> {
    let out = Command::new(program).args(agent.get).arg(name).output()?;
    Ok(if out.status.success() { shown_entry(&String::from_utf8_lossy(&out.stdout)) } else { None })
}

fn not_on_path(agent: Agent, server: &Server) -> String {
    format!(
        "{} is not on the PATH; install it, or add the server by hand: {} {} {} -- {}",
        agent.name,
        agent.name,
        agent.add.join(" "),
        server.name,
        server.command.join(" ")
    )
}

/// Remove and add each server with the agent's CLI `program`, and say what was done. A server
/// of the same name that is not vxr's stops it, untouched. When the add fails, the entry that
/// was removed is added back, so a failure never leaves the agent with less than it had.
pub fn register(agent: Agent, program: &Path, servers: &[Server]) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for server in servers {
        let run = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::NotFound => not_on_path(agent, server),
            _ => format!("could not run {}: {e}", agent.name),
        };
        let before = registered(agent, program, server.name).map_err(run)?;
        if let Some((command, args)) = &before
            && !is_ours(command, args)
        {
            return Err(format!(
                "{} already has a server named {} that is not vxr's ({command} {args}); it is left as it is. \
                 Remove it first: {} {} {}",
                agent.name,
                server.name,
                agent.name,
                agent.remove.join(" "),
                server.name
            ));
        }
        let removed = Command::new(program).args(agent.remove).arg(server.name).output().map_err(run)?.status.success();
        let add = |command: &[String]| {
            Command::new(program).args(agent.add).arg(server.name).arg("--").args(command).output()
        };
        let added = add(&server.command).map_err(|e| format!("could not run {}: {e}", agent.name))?;
        if !added.status.success() {
            let why = String::from_utf8_lossy(&added.stderr);
            let mut err = format!("{} could not add {}: {}", agent.name, server.name, why.trim());
            if let (true, Some((command, args))) = (removed, &before) {
                let old: Vec<String> =
                    std::iter::once(command.clone()).chain(args.split_whitespace().map(String::from)).collect();
                err.push_str(match add(&old) {
                    Ok(o) if o.status.success() => "; the entry it had before is back in place",
                    _ => "; the entry it had before could not be put back, so run vxr setup again",
                });
            }
            return Err(err);
        }
        let verb = if removed { "replaced" } else { "added" };
        done.push(format!("{}: {verb} {}: {}", agent.name, server.name, server.command.join(" ")));
    }
    Ok(done)
}

/// Remove vxr's two servers from the agent run as `program`, and say what was done: one line
/// per server. An agent that is not installed, a server that is not registered and a server of
/// the same name that is not vxr's are each left alone and named.
pub fn unregister(agent: Agent, program: &Path) -> Vec<String> {
    let mut done = Vec::new();
    for name in ["vxr", "vxr-live"] {
        match registered(agent, program, name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return vec![format!("{}: not on the PATH, so nothing to remove", agent.name)];
            }
            Err(e) => done.push(format!("{}: could not look for {name}: {e}", agent.name)),
            Ok(None) => done.push(format!("{}: {name} is not registered", agent.name)),
            Ok(Some((command, args))) if !is_ours(&command, &args) => {
                done.push(format!("{}: left {name} alone: it is not vxr's ({command} {args})", agent.name));
            }
            Ok(Some(_)) => {
                let ok = Command::new(program).args(agent.remove).arg(name).output().is_ok_and(|o| o.status.success());
                done.push(if ok {
                    format!("{}: removed {name}", agent.name)
                } else {
                    format!(
                        "{}: could not remove {name}; by hand: {} {} {name}",
                        agent.name,
                        agent.name,
                        agent.remove.join(" ")
                    )
                });
            }
        }
    }
    done
}

/// Remove the Spotlight app `vxr setup app` made at `dest`, and say what was done. Only vxr's
/// own link goes: a folder whose `Contents` is a symlink (or a symlink to an app).
pub fn unlink_app(dest: &Path) -> String {
    let shown = dest.display();
    let inner = dest.join("Contents");
    let removed = match std::fs::symlink_metadata(dest) {
        Err(_) => return format!("app: {shown} is not there"),
        Ok(m) if m.is_symlink() => std::fs::remove_file(dest),
        Ok(m) if m.is_dir() && std::fs::symlink_metadata(&inner).is_ok_and(|c| c.is_symlink()) => {
            std::fs::remove_file(&inner).and_then(|()| std::fs::remove_dir(dest))
        }
        Ok(_) => return format!("app: left {shown} alone: it is not vxr's link"),
    };
    match removed {
        Ok(()) => format!("app: removed {shown}"),
        Err(e) => format!("app: could not remove {shown}: {e}"),
    }
}

/// What `--remove` leaves, because it is the user's: where each thing is and how to delete it.
fn left_behind(home: &Path) -> String {
    format!(
        "\nLeft in place, because they are yours. Delete them by hand if you want them gone:\n  \
         {scenes}  the scenes files mode serves (or the folder you gave with --root)\n  \
         {data}  the library, drafts and settings\n  \
         keychain items of the service \"vxr\"  API keys and the sign-in: Keychain Access, search for vxr\n\
         Then: brew uninstall vxr",
        scenes = home.join("vxr").display(),
        data = home.join("Library/Application Support/vxr").display(),
    )
}

/// The binary to point agents at. Homebrew runs it from `<prefix>/Cellar/<name>/<version>/…`,
/// which an upgrade removes, so that becomes `<prefix>/opt/<name>/…`, which follows upgrades.
pub fn stable_exe(exe: &Path) -> PathBuf {
    let parts: Vec<_> = exe.components().collect();
    match parts.iter().position(|c| c.as_os_str() == "Cellar") {
        Some(i) if i + 3 < parts.len() => {
            let mut stable: PathBuf = parts[..i].iter().collect();
            stable.push("opt");
            stable.push(parts[i + 1]);
            stable.extend(&parts[i + 3..]);
            stable
        }
        _ => exe.to_path_buf(),
    }
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(Into::into)
        .ok_or_else(|| "HOME is not set; give --root <dir>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homebrew_cellar_paths_become_opt_paths() {
        let exe = Path::new("/opt/homebrew/Cellar/vxr/0.1.0-pre.1/vxr.app/Contents/MacOS/vxr");
        assert_eq!(stable_exe(exe), Path::new("/opt/homebrew/opt/vxr/vxr.app/Contents/MacOS/vxr"));
        assert_eq!(stable_exe(Path::new("/opt/homebrew/bin/vxr")), Path::new("/opt/homebrew/bin/vxr"));
        assert_eq!(stable_exe(Path::new("/x/target/release/vxr")), Path::new("/x/target/release/vxr"));
    }

    /// A fake agent CLI that logs its arguments and knows the servers added before, so a second
    /// run replaces them.
    #[cfg(unix)]
    #[test]
    fn registers_both_servers_and_replaces_them_on_a_second_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("vxr-setup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("claude");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{d}/log'\nfor a; do [ \"$a\" = -- ] && break; name=$a; done\n\
                 case $2 in remove) [ -e '{d}/'\"$name\" ];; add) touch '{d}/'\"$name\";; esac\n",
                d = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let servers = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        let first = register(AGENTS[0], &fake, &servers).unwrap();
        assert_eq!(
            first,
            [
                "claude: added vxr: /opt/homebrew/bin/vxr mcp --root /home/user/vxr",
                "claude: added vxr-live: /opt/homebrew/bin/vxr mcp --live",
            ]
        );
        let second = register(AGENTS[0], &fake, &servers).unwrap();
        assert!(second.iter().all(|l| l.starts_with("claude: replaced vxr")), "{second:?}");
        let log = std::fs::read_to_string(dir.join("log")).unwrap();
        assert_eq!(
            log.lines().take(3).collect::<Vec<_>>(),
            [
                "mcp get vxr",
                "mcp remove --scope user vxr",
                "mcp add --scope user vxr -- /opt/homebrew/bin/vxr mcp --root /home/user/vxr",
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_the_app_and_replaces_only_its_own_link() {
        let dir = std::env::temp_dir().join(format!("vxr-setup-app-{}", std::process::id()));
        let contents = dir.join("opt/vxr/vxr.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let dest = dir.join("Applications/vxr.app");
        assert!(link_app(&contents, &dest).unwrap().starts_with("app: added"));
        assert!(dest.join("Contents/MacOS").is_dir());
        assert!(link_app(&contents, &dest).unwrap().starts_with("app: replaced"));
        assert_eq!(std::fs::read_link(dest.join("Contents")).unwrap(), contents);
        // A symlink to the app, as an earlier caveat might have made, is replaced too.
        std::fs::remove_file(dest.join("Contents")).unwrap();
        std::fs::remove_dir(&dest).unwrap();
        std::os::unix::fs::symlink(contents.parent().unwrap(), &dest).unwrap();
        assert!(link_app(&contents, &dest).unwrap().starts_with("app: replaced"));
        assert!(std::fs::symlink_metadata(&dest).unwrap().is_dir());
        // Another app is left alone.
        let other = dir.join("Applications/other.app");
        std::fs::create_dir_all(other.join("Contents")).unwrap();
        assert!(link_app(&contents, &other).unwrap_err().contains("move it away first"));
        assert!(std::fs::symlink_metadata(other.join("Contents")).unwrap().is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A fake agent CLI that keeps each added server's command and arguments in a file, shows
    /// them for `get` as `claude mcp get` does, and refuses to add a command that holds the
    /// text in the file `refuse`, if there is one.
    #[cfg(unix)]
    fn fake_with_get(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let fake = dir.join("claude");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nfor a; do [ \"$a\" = -- ] && break; name=$a; done\n\
                 case $2 in\n\
                 get) [ -e '{d}/'\"$name\" ] && {{ echo \"Command: $(sed -n 1p '{d}/'\"$name\")\"; echo \"Args: $(sed -n 2p '{d}/'\"$name\")\"; }};;\n\
                 remove) [ -e '{d}/'\"$name\" ] && rm '{d}/'\"$name\";;\n\
                 add) while [ \"$1\" != -- ]; do shift; done; shift;\n\
                 if [ -e '{d}/refuse' ] && echo \"$*\" | grep -q \"$(cat '{d}/refuse')\"; then echo 'no such binary' >&2; exit 1; fi\n\
                 echo \"$1\" > '{d}/'\"$name\"; shift; echo \"$*\" >> '{d}/'\"$name\";;\n\
                 esac\n",
                d = dir.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        fake
    }

    #[cfg(unix)]
    #[test]
    fn a_server_named_vxr_that_is_not_ours_is_never_replaced_or_removed() {
        let dir = std::env::temp_dir().join(format!("vxr-setup-foreign-{}", std::process::id()));
        let fake = fake_with_get(&dir);
        // Another tool took the name first.
        std::fs::write(dir.join("vxr"), "/usr/local/bin/voxel-router\nserve --stdio\n").unwrap();
        let servers = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        let err = register(AGENTS[0], &fake, &servers).unwrap_err();
        assert!(err.contains("not vxr's (/usr/local/bin/voxel-router serve --stdio)"), "{err}");
        assert!(err.contains("claude mcp remove --scope user vxr"), "{err}");
        let kept = std::fs::read_to_string(dir.join("vxr")).unwrap();
        assert_eq!(kept, "/usr/local/bin/voxel-router\nserve --stdio\n", "the other tool's entry is as it was");
        assert!(!dir.join("vxr-live").exists(), "nothing is added after the refusal");
        // --remove leaves it too, and says so.
        let lines = unregister(AGENTS[0], &fake);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("claude: left vxr alone: it is not vxr's"), "{lines:?}");
        assert_eq!(lines[1], "claude: vxr-live is not registered");
        assert!(dir.join("vxr").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_add_puts_the_entry_it_removed_back() {
        let dir = std::env::temp_dir().join(format!("vxr-setup-restore-{}", std::process::id()));
        let fake = fake_with_get(&dir);
        let old = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        register(AGENTS[0], &fake, &old).unwrap();
        // The agent refuses the new root.
        std::fs::write(dir.join("refuse"), "/home/user/scenes").unwrap();
        let new = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/scenes"));
        let err = register(AGENTS[0], &fake, &new).unwrap_err();
        assert!(err.contains("could not add vxr: no such binary"), "{err}");
        assert!(err.contains("the entry it had before is back in place"), "{err}");
        let kept = std::fs::read_to_string(dir.join("vxr")).unwrap();
        assert_eq!(kept, "/opt/homebrew/bin/vxr\nmcp --root /home/user/vxr\n", "files mode is as it was");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_takes_out_both_servers_the_command_and_the_app_link_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("vxr-setup-remove-{}", std::process::id()));
        let fake = fake_with_get(&dir);
        let both = servers(Path::new("/opt/homebrew/opt/vxr/vxr.app/Contents/MacOS/vxr"), Path::new("/home/user/vxr"));
        register(AGENTS[0], &fake, &both).unwrap();
        assert_eq!(unregister(AGENTS[0], &fake), ["claude: removed vxr", "claude: removed vxr-live"]);
        assert!(!dir.join("vxr").exists() && !dir.join("vxr-live").exists());
        // A second run has nothing to do, and an agent that is not installed is one line.
        assert_eq!(
            unregister(AGENTS[0], &fake),
            ["claude: vxr is not registered", "claude: vxr-live is not registered"]
        );
        assert_eq!(
            unregister(AGENTS[1], Path::new("/nonexistent/codex")),
            ["codex: not on the PATH, so nothing to remove"]
        );
        // /vxr: vxr's own file goes, a file the user wrote stays.
        let commands = dir.join("commands");
        assert!(crate::claude::install_command(&commands).unwrap().starts_with("claude: added /vxr"));
        assert!(crate::claude::remove_command(&commands).starts_with("claude: removed /vxr"));
        assert!(!commands.join(crate::claude::COMMAND_FILE).exists());
        assert_eq!(crate::claude::remove_command(&commands), "claude: /vxr is not installed");
        std::fs::write(commands.join(crate::claude::COMMAND_FILE), "my own command\n").unwrap();
        assert!(crate::claude::remove_command(&commands).contains("left"), "a user's file stays");
        assert!(commands.join(crate::claude::COMMAND_FILE).exists());
        // The Spotlight app: vxr's link goes, another app stays.
        let contents = dir.join("opt/vxr/vxr.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        let dest = dir.join("Applications/vxr.app");
        link_app(&contents, &dest).unwrap();
        assert!(unlink_app(&dest).starts_with("app: removed"));
        assert!(std::fs::symlink_metadata(&dest).is_err(), "the link folder is gone");
        assert!(contents.join("MacOS").is_dir(), "the app it pointed at is untouched");
        assert!(unlink_app(&dest).ends_with("is not there"));
        let other = dir.join("Applications/other.app");
        std::fs::create_dir_all(other.join("Contents")).unwrap();
        assert!(unlink_app(&other).contains("left"));
        assert!(other.join("Contents").is_dir());
        // What is the user's is named, not deleted.
        let left = left_behind(Path::new("/home/user"));
        assert!(left.contains("/home/user/vxr") && left.contains("keychain") && left.contains("brew uninstall vxr"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_scenes_folder_is_named_when_it_is_made() {
        let dir = std::env::temp_dir().join(format!("vxr-setup-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("vxr");
        let line = make_root(&root).unwrap().expect("a new folder is named");
        assert!(line.contains(&root.display().to_string()), "{line}");
        assert!(root.is_dir());
        assert_eq!(make_root(&root).unwrap(), None, "a folder that is there is not named again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_vxr_binary_run_as_mcp_is_ours() {
        assert!(is_ours("/opt/homebrew/opt/vxr/vxr.app/Contents/MacOS/vxr", "mcp --root /home/user/vxr"));
        assert!(is_ours("/opt/homebrew/bin/vxr", "mcp --live"));
        assert!(is_ours("vxr", "mcp --live --connect-at-start"));
        assert!(!is_ours("/usr/local/bin/voxel-router", "mcp --live"));
        assert!(!is_ours("/opt/homebrew/bin/vxr", "serve"));
        assert!(!is_ours("/opt/homebrew/bin/vxr", "mcp --lively"));
        assert_eq!(
            shown_entry("vxr:\n  Scope: User config\n  Command: /bin/vxr\n  Args: mcp --live\n"),
            Some(("/bin/vxr".into(), "mcp --live".into()))
        );
        assert_eq!(shown_entry("No MCP server found with name: vxr"), None);
    }

    #[test]
    fn a_missing_cli_says_how_to_add_by_hand() {
        let servers = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        let err = register(AGENTS[1], Path::new("/nonexistent/codex"), &servers).unwrap_err();
        assert!(err.contains("codex mcp add vxr -- /opt/homebrew/bin/vxr mcp --root /home/user/vxr"), "{err}");
    }
}
