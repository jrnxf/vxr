//! The vxr desktop editor.
//!
//! - `vxr [scene.vox]` opens a window, with the scene if one is given.
//! - `vxr --shot <dir> [scene.vox]` runs headless: it drives the editor through every tool,
//!   saves and reopens the result, and writes `app-1x.png` and `app-2x.png` into `<dir>`.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod shot;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--shot") => match args.get(1) {
            Some(dir) => shot::run(dir.as_ref(), args.get(2).map(|s| s.as_ref())),
            None => Err("usage: vxr --shot <dir> [scene.vox]".into()),
        },
        Some(flag) if flag.starts_with('-') => Err(format!("unknown flag {flag}; usage: vxr [scene.vox]")),
        path => native::run(path.map(Into::into)),
    };
    if let Err(e) = result {
        eprintln!("vxr: {e}");
        std::process::exit(1);
    }
}

// The browser build is a library entry point (not yet written); the binary does nothing there.
#[cfg(target_arch = "wasm32")]
fn main() {}
