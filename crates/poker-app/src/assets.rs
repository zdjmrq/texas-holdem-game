//! Runtime asset and config directory resolution.
//!
//! The portable Windows build ships assets next to the executable; during
//! development the same files are found relative to the workspace root. All
//! lookups are read-only and fall back gracefully so the app still starts (with
//! reduced features) if a package is incomplete.

use std::path::{Path, PathBuf};

/// Icon PNG embedded in the binary (window icon and Windows tray icon).
pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/attention-icon.png");

/// Bundled GB2312 subset of Noto Sans SC (generated from the system Noto CJK
/// TTC, see assets/fonts/README.md). 3.1 MB, OFL licensed.
pub const BUNDLED_FONT_OTF: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular.otf");

/// Optional embedded first music track, enabled with the embedded-music feature.
#[cfg(feature = "embedded-music")]
pub const EMBEDDED_MUSIC_01: &[u8] = include_bytes!("../../../assets/music/01.mp3");

/// Possible asset roots, most specific first.
pub fn asset_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.join("assets"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("assets"));
    }
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    roots
}

/// First existing path for a relative asset path.
pub fn asset_path(relative: impl AsRef<Path>) -> Option<PathBuf> {
    let relative = relative.as_ref();
    asset_roots()
        .into_iter()
        .map(|root| root.join(relative))
        .find(|candidate| candidate.exists())
}

/// All mp3 tracks under assets/music, sorted by file name.
pub fn music_tracks() -> Vec<PathBuf> {
    for root in asset_roots() {
        let music_dir = root.join("music");
        let Ok(entries) = std::fs::read_dir(&music_dir) else {
            continue;
        };
        let mut tracks: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
            })
            .collect();
        tracks.sort();
        if !tracks.is_empty() {
            return tracks;
        }
    }
    Vec::new()
}

/// Application config directory (settings.json lives here).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("POKER_APP_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("texas-holdem-rs");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("texas-holdem-rs");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("texas-holdem-rs");
    }
    PathBuf::from(".").join(".texas-holdem-rs")
}

/// Path of the persisted settings JSON.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}
