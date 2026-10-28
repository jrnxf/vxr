//! `vxr setup claude|codex [--root <dir>]`: registers vxr's two MCP servers with a terminal
//! agent through the agent's own CLI, for every project of the user:
//!
//! - `vxr`: files mode, `vxr mcp --root <dir>`, on `~/vxr` unless `--root` names another folder
//!   (made if missing).
//! - `vxr-live`: live mode, `vxr mcp --live`, the scene open in the desktop app.
//!
//! Each server is removed first and added again, so a second run leaves the same setup (with
//! the new root, if one is given) and says it replaced them.
//!
//! `vxr setup app` makes `~/Applications/vxr.app` for Spotlight: a folder whose `Contents` links
//! to the `vxr.app` this binary runs from (Homebrew's `opt` path, so it follows upgrades).
//! Spotlight indexes that as an app; it skips a symlink to the app and Homebrew's folders.

use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: vxr setup claude|codex [--root <dir>] | vxr setup app";

const HELP: &str = "usage: vxr setup claude|codex [--root <dir>]
       vxr setup app

claude, codex: registers vxr's MCP servers with the agent, for every project:
  vxr       files mode on the .vox files under --root (default ~/vxr)
  vxr-live  live mode, the scene open in the vxr window
app: adds ~/Applications/vxr.app, so Spotlight opens vxr";

/// A terminal agent's CLI: how it removes and adds a stdio MCP server for every project.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Agent {
    pub name: &'static str,
    remove: &'static [&'static str],
    add: &'static [&'static str],
}

pub const AGENTS: [Agent; 2] = [
    Agent { name: "claude", remove: &["mcp", "remove", "--scope", "user"], add: &["mcp", "add", "--scope", "user"] },
    Agent { name: "codex", remove: &["mcp", "remove"], add: &["mcp", "add"] },
];

/// One server to register: its name and the command the agent runs.
#[derive(Debug, PartialEq)]
pub struct Server {
    pub name: &'static str,
    pub command: Vec<String>,
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
    let root = match root {
        Some(r) => std::path::absolute(&r).map_err(|e| format!("{}: {e}", r.display()))?,
        None => home()?.join("vxr"),
    };
    std::fs::create_dir_all(&root).map_err(|e| format!("could not make {}: {e}", root.display()))?;
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

/// Remove and add each server with the agent's CLI `program`, and say what was done.
pub fn register(agent: Agent, program: &Path, servers: &[Server]) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for server in servers {
        let removed = Command::new(program)
            .args(agent.remove)
            .arg(server.name)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => format!(
                    "{} is not on the PATH; install it, or add the server by hand: {} {} {} -- {}",
                    agent.name,
                    agent.name,
                    agent.add.join(" "),
                    server.name,
                    server.command.join(" ")
                ),
                _ => format!("could not run {}: {e}", agent.name),
            })?
            .status
            .success();
        let added = Command::new(program)
            .args(agent.add)
            .arg(server.name)
            .arg("--")
            .args(&server.command)
            .output()
            .map_err(|e| format!("could not run {}: {e}", agent.name))?;
        if !added.status.success() {
            let why = String::from_utf8_lossy(&added.stderr);
            return Err(format!("{} could not add {}: {}", agent.name, server.name, why.trim()));
        }
        let verb = if removed { "replaced" } else { "added" };
        done.push(format!("{}: {verb} {}: {}", agent.name, server.name, server.command.join(" ")));
    }
    Ok(done)
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
            log.lines().take(2).collect::<Vec<_>>(),
            [
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

    #[test]
    fn a_missing_cli_says_how_to_add_by_hand() {
        let servers = servers(Path::new("/opt/homebrew/bin/vxr"), Path::new("/home/user/vxr"));
        let err = register(AGENTS[1], Path::new("/nonexistent/codex"), &servers).unwrap_err();
        assert!(err.contains("codex mcp add vxr -- /opt/homebrew/bin/vxr mcp --root /home/user/vxr"), "{err}");
    }
}
