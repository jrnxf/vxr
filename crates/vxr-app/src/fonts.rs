//! The UI fonts: Geist and Geist Mono 1.3.1, Inter 4.1 Regular for the symbols Geist lacks
//! (all OFL), and the Lucide 1.61.0 icon font (ISC). They are not in git: `crates/vxr-app/fetch-fonts` downloads them into
//! `crates/vxr-app/fonts/` (git-ignored) and checks their sha256.

#[cfg(not(target_arch = "wasm32"))]
use crate::ui::FontBytes;

/// The fonts folder: `VXR_FONTS` if set, else `Contents/Resources/fonts` when the binary runs
/// from `vxr.app` (a release, `crates/vxr-app/bundle`; also through a symlink to it, such as
/// Homebrew's `bin/vxr`), else `crates/vxr-app/fonts` in this checkout.
#[cfg(not(target_arch = "wasm32"))]
pub fn dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("VXR_FONTS") {
        return dir.into();
    }
    let bundled = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("Resources/fonts")))
        .filter(|dir| dir.is_dir());
    bundled.unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts"))
}

/// Read the seven font files from `dir`, or say which one is missing.
#[cfg(not(target_arch = "wasm32"))]
pub fn load(dir: &std::path::Path) -> Result<FontBytes, String> {
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{}: {e}", dir.join(n).display()));
    Ok(FontBytes {
        sans_regular: read("Geist-Regular.ttf")?,
        sans_medium: read("Geist-Medium.ttf")?,
        sans_semibold: read("Geist-SemiBold.ttf")?,
        mono_regular: read("GeistMono-Regular.ttf")?,
        mono_medium: read("GeistMono-Medium.ttf")?,
        symbols: read("Inter-Regular.ttf")?,
        icons: read("lucide.ttf")?,
    })
}
