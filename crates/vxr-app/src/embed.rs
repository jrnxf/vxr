//! The desktop side of the Embed command: where the viewer's files are, and the folder an
//! embed is written as. The folder's contents come from `vxr_core::files::embed_files`.

use std::path::{Path, PathBuf};

use crate::files::{EMBED_VIEWER, File, write_new};

/// The folder with the embed viewer's files: `VXR_EMBED` if set, else
/// `Contents/Resources/embed` when the binary runs from `vxr.app` (`crates/vxr-app/bundle`),
/// else `crates/vxr-embed/dist` in this checkout (`crates/vxr-embed/build` makes it).
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VXR_EMBED") {
        return dir.into();
    }
    let bundled = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/embed")))
        .filter(|dir| dir.is_dir());
    bundled.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../vxr-embed/dist"))
}

/// The viewer's files from `dir`, or what to do when one is not there.
pub fn viewer(dir: &Path) -> Result<Vec<File>, String> {
    EMBED_VIEWER
        .iter()
        .map(|name| {
            let bytes = std::fs::read(dir.join(name)).map_err(|_| {
                format!(
                    "this build has no embed viewer ({name} is missing). From a checkout, run crates/vxr-embed/build"
                )
            })?;
            Ok((name.to_string(), bytes))
        })
        .collect()
}

/// Write `files` into a new folder under `parent`: `name`, or `name-1` and so on when that
/// is taken, so an embed never replaces a folder already there. Returns the folder's name.
pub fn write_folder(parent: &Path, name: &str, files: &[File]) -> Result<String, String> {
    let taken = |e: &std::io::Error| e.kind() == std::io::ErrorKind::AlreadyExists;
    let mut made = None;
    for i in 0..1000 {
        let folder = if i == 0 { name.to_string() } else { format!("{name}-{i}") };
        match std::fs::create_dir(parent.join(&folder)) {
            Ok(()) => {
                made = Some(folder);
                break;
            }
            Err(e) if taken(&e) => {}
            Err(e) => return Err(format!("could not make the folder {folder}: {e}")),
        }
    }
    let folder = made.ok_or_else(|| format!("{name} and a thousand numbered folders are taken"))?;
    for (file, bytes) in files {
        write_new(&parent.join(&folder).join(file), bytes).map_err(|e| format!("could not write {file}: {e}"))?;
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catches: a second embed of the same scene replacing or mixing into the first folder.
    #[test]
    fn a_second_embed_gets_its_own_folder() {
        let parent = std::env::temp_dir().join(format!("vxr-embed-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).unwrap();
        let files = |tag: u8| vec![("index.html".to_string(), vec![tag]), ("a.vxrb".to_string(), vec![tag, tag])];
        assert_eq!(write_folder(&parent, "a-embed", &files(1)).unwrap(), "a-embed");
        assert_eq!(write_folder(&parent, "a-embed", &files(2)).unwrap(), "a-embed-1");
        let read = |f: &str| std::fs::read(parent.join(f)).unwrap();
        assert_eq!((read("a-embed/index.html"), read("a-embed/a.vxrb")), (vec![1], vec![1, 1]));
        assert_eq!((read("a-embed-1/index.html"), read("a-embed-1/a.vxrb")), (vec![2], vec![2, 2]));
        std::fs::remove_dir_all(&parent).unwrap();
    }
}
