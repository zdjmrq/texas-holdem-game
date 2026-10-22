//! CJK font installation.
//!
//! The app embeds a GB2312 subset of Noto Sans SC (3.1 MB, OFL) so Chinese text
//! renders identically on every Windows machine without depending on system
//! fonts. eframe's system font fallback stays enabled for rare glyphs/emoji.

use std::sync::Arc;

use eframe::egui;

use crate::assets;

/// Result of the font setup, shown in the settings screen.
#[derive(Debug, Clone)]
pub struct FontReport {
    pub bundled: bool,
    pub bytes: usize,
    pub description: String,
}

impl FontReport {
    pub fn summary(&self) -> String {
        if self.bundled {
            format!(
                "内置 Noto Sans SC GB2312 子集（{} 字节）+ 系统字体回退",
                self.bytes
            )
        } else {
            self.description.clone()
        }
    }
}

/// Register the bundled CJK font as the highest-priority proportional family.
pub fn install(ctx: &egui::Context) -> FontReport {
    let bytes = assets::BUNDLED_FONT_OTF;
    let data = egui::FontData::from_blob(Arc::new(bytes) as Arc<dyn AsRef<[u8]> + Send + Sync>, 0);
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
    FontReport {
        bundled: true,
        bytes: bytes.len(),
        description: "内置中文字体".to_owned(),
    }
}

/// Byte length of the embedded font (used by tests and the about panel).
#[must_use]
pub fn bundled_font_len() -> usize {
    assets::BUNDLED_FONT_OTF.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_is_a_real_cff_opentype_file() {
        let bytes = assets::BUNDLED_FONT_OTF;
        assert!(bytes.len() > 1_000_000, "font subset looks too small");
        assert_eq!(&bytes[0..4], b"OTTO", "expected an OpenType CFF font");
    }

    #[test]
    fn bundled_font_len_matches_asset() {
        assert_eq!(bundled_font_len(), assets::BUNDLED_FONT_OTF.len());
    }
}
