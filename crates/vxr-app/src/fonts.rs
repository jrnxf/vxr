//! The UI fonts: Inter 4.1, JetBrains Mono 2.304 (both OFL) and the Lucide 1.61.0 icon font
//! (ISC). They are not in git: `crates/vxr-app/fetch-fonts` downloads them into
//! `crates/vxr-app/fonts/` (git-ignored) and checks their sha256.

#[cfg(not(target_arch = "wasm32"))]
use crate::ui::FontBytes;

/// The fonts folder: `VXR_FONTS` if set, else `crates/vxr-app/fonts` in this checkout.
#[cfg(not(target_arch = "wasm32"))]
pub fn dir() -> std::path::PathBuf {
    std::env::var_os("VXR_FONTS")
        .map(Into::into)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts"))
}

/// Read the six font files from `dir`, or say which one is missing.
#[cfg(not(target_arch = "wasm32"))]
pub fn load(dir: &std::path::Path) -> Result<FontBytes, String> {
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{}: {e}", dir.join(n).display()));
    Ok(FontBytes {
        inter_regular: read("Inter-Regular.ttf")?,
        inter_medium: read("Inter-Medium.ttf")?,
        inter_semibold: read("Inter-SemiBold.ttf")?,
        mono_regular: read("JetBrainsMono-Regular.ttf")?,
        mono_medium: read("JetBrainsMono-Medium.ttf")?,
        icons: read("lucide.ttf")?,
    })
}
