//! CJK font loading strategies for the spike.
//!
//! Three strategies are compared here (see the `--font-mode` flag):
//!
//! 1. SystemFallback - eframe 0.37 enables the `system_fonts` feature by default and installs
//!    a `SystemFontProvider` backed by `fontique`. Missing glyphs are resolved on demand through
//!    the OS font fallback (DirectWrite on Windows, fontconfig on Linux). No font is bundled.
//! 2. ExplicitSystem - look for a known system CJK font file, load it from disk and register it
//!    as the highest-priority proportional font. TTC collections carry several faces and the
//!    Simplified-Chinese face has a non-zero index (Noto Sans CJK SC = face #2).
//! 3. Bundled - load `assets/fonts/NotoSansSC-Regular.otf` if present. This is the path the
//!    release build should use (a subsetted font committed to `assets/fonts/`).
//!
//! `FontData::from_blob` keeps the loaded file behind an `Arc`, so the bytes are not copied
//! again when the font is registered with egui.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;

use crate::cli::FontMode;

/// A CJK font file that is expected to exist on a given OS installation.
#[derive(Debug, Clone, Copy)]
pub struct SystemFontCandidate {
    pub path: &'static str,
    pub face_index: u32,
    pub label: &'static str,
}

/// Known CJK fonts, most preferred first.
#[cfg(target_os = "windows")]
pub const SYSTEM_CJK_FONTS: &[SystemFontCandidate] = &[
    SystemFontCandidate {
        path: r"C:\Windows\Fonts\msyh.ttc",
        face_index: 0,
        label: "Microsoft YaHei",
    },
    SystemFontCandidate {
        path: r"C:\Windows\Fonts\msyhbd.ttc",
        face_index: 0,
        label: "Microsoft YaHei Bold",
    },
    SystemFontCandidate {
        path: r"C:\Windows\Fonts\simhei.ttf",
        face_index: 0,
        label: "SimHei",
    },
    SystemFontCandidate {
        path: r"C:\Windows\Fonts\simsun.ttc",
        face_index: 0,
        label: "SimSun",
    },
    SystemFontCandidate {
        path: r"C:\Windows\Fonts\Deng.ttf",
        face_index: 0,
        label: "DengXian",
    },
];

#[cfg(target_os = "linux")]
pub const SYSTEM_CJK_FONTS: &[SystemFontCandidate] = &[
    SystemFontCandidate {
        path: "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        face_index: 2, // 0=JP, 1=KR, 2=SC, 3=TC, 4=HK
        label: "Noto Sans CJK SC",
    },
    SystemFontCandidate {
        path: "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
        face_index: 2,
        label: "Noto Sans CJK SC Bold",
    },
    SystemFontCandidate {
        path: "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        face_index: 0,
        label: "WenQuanYi Zen Hei",
    },
    SystemFontCandidate {
        path: "/usr/share/fonts/truetype/arphic/uming.ttc",
        face_index: 0,
        label: "AR PL UMing",
    },
];

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub const SYSTEM_CJK_FONTS: &[SystemFontCandidate] = &[];

/// Font bytes loaded from disk, ready to be registered with egui.
#[derive(Debug, Clone)]
pub struct LoadedFont {
    pub label: String,
    pub path: PathBuf,
    pub face_index: u32,
    pub bytes: Vec<u8>,
}

/// Result of applying a font strategy to an egui Context.
#[derive(Debug, Clone)]
pub struct FontInstallReport {
    pub mode: FontMode,
    pub installed: bool,
    pub description: String,
    pub path: Option<PathBuf>,
    pub font_bytes: usize,
}

impl FontInstallReport {
    pub fn summary(&self) -> String {
        match &self.path {
            Some(path) => format!(
                "font-mode={} installed={} bytes={} path={} ({})",
                self.mode.label(),
                self.installed,
                self.font_bytes,
                path.display(),
                self.description
            ),
            None => format!(
                "font-mode={} installed={} ({})",
                self.mode.label(),
                self.installed,
                self.description
            ),
        }
    }
}

/// Apply the requested font strategy. Safe to call once during app creation.
pub fn install(ctx: &egui::Context, mode: FontMode) -> FontInstallReport {
    match mode {
        FontMode::SystemFallback => FontInstallReport {
            mode,
            installed: true,
            description: "eframe system_font_fallback via fontique (DirectWrite on Windows, fontconfig on Linux); no font bundled".to_owned(),
            path: None,
            font_bytes: 0,
        },
        FontMode::ExplicitSystem => match load_system_font() {
            Some(font) => {
                let (font_bytes, path) = (font.bytes.len(), font.path.clone());
                add_cjk_font(ctx, &font);
                FontInstallReport {
                    mode,
                    installed: true,
                    description: format!(
                        "explicit system font {} (face #{})",
                        font.label, font.face_index
                    ),
                    path: Some(path),
                    font_bytes,
                }
            }
            None => FontInstallReport {
                mode,
                installed: false,
                description: "no system CJK font found; set POKER_APP_FONT or install fonts-noto-cjk".to_owned(),
                path: None,
                font_bytes: 0,
            },
        },
        FontMode::Bundled => match load_bundled_font() {
            Some(font) => {
                let (font_bytes, path) = (font.bytes.len(), font.path.clone());
                add_cjk_font(ctx, &font);
                FontInstallReport {
                    mode,
                    installed: true,
                    description: format!("bundled font {}", font.label),
                    path: Some(path),
                    font_bytes,
                }
            }
            None => FontInstallReport {
                mode,
                installed: false,
                description: format!(
                    "bundled font not found at {}; run the subsetting step or use --font-mode explicit",
                    bundled_font_path().display()
                ),
                path: Some(bundled_font_path()),
                font_bytes: 0,
            },
        },
    }
}

/// Register a CJK font as the preferred proportional font (and a monospace fallback).
pub fn add_cjk_font(ctx: &egui::Context, font: &LoadedFont) {
    // from_blob takes the shared bytes as-is; TTC files need an explicit face index.
    let data = egui::FontData::from_blob(Arc::new(font.bytes.clone()), font.face_index);
    let insert = egui::FontInsert::new(
        "poker-cjk",
        data,
        vec![
            egui::InsertFontFamily {
                family: egui::FontFamily::Proportional,
                priority: egui::FontPriority::Highest,
            },
            egui::InsertFontFamily {
                family: egui::FontFamily::Monospace,
                priority: egui::FontPriority::Lowest,
            },
        ],
    );
    ctx.add_font(insert);
}

/// Load the first available known system CJK font (or POKER_APP_FONT when set).
pub fn load_system_font() -> Option<LoadedFont> {
    if let Some(font) = load_font_from_env() {
        return Some(font);
    }
    for candidate in SYSTEM_CJK_FONTS {
        let path = Path::new(candidate.path);
        if !path.is_file() {
            continue;
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                return Some(LoadedFont {
                    label: candidate.label.to_owned(),
                    path: path.to_path_buf(),
                    face_index: candidate.face_index,
                    bytes,
                });
            }
            Err(error) => log::warn!("failed to read font {}: {error}", path.display()),
        }
    }
    None
}

fn load_font_from_env() -> Option<LoadedFont> {
    let path = PathBuf::from(std::env::var_os("POKER_APP_FONT")?);
    let face_index = std::env::var("POKER_APP_FONT_INDEX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let bytes = std::fs::read(&path).ok()?;
    Some(LoadedFont {
        label: "POKER_APP_FONT".to_owned(),
        path,
        face_index,
        bytes,
    })
}

/// Path the release build should put the subsetted Simplified-Chinese font at.
pub fn bundled_font_path() -> PathBuf {
    if let Some(path) = std::env::var_os("POKER_APP_BUNDLED_FONT") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/NotoSansSC-Regular.otf")
}

fn load_bundled_font() -> Option<LoadedFont> {
    let path = bundled_font_path();
    let bytes = std::fs::read(&path).ok()?;
    Some(LoadedFont {
        label: "bundled Noto Sans SC".to_owned(),
        path,
        face_index: 0,
        bytes,
    })
}
