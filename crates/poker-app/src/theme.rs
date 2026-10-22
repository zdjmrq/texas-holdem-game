//! Color and typography tokens from docs/specs/ui.md section 2.8.
//!
//! The original game is a dark theme; dark stays the default and a light theme
//! is offered as an accessibility/preference alternative. This is a
//! high-fidelity pass, not a pixel-perfect port.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};
use serde::{Deserialize, Serialize};

/// Display theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Original dark game theme.
    #[default]
    Dark,
    /// Light alternative.
    Light,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Dark, Theme::Light];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "深色（原版）",
            Self::Light => "浅色",
        }
    }

    #[must_use]
    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }

    /// Apply the theme to an egui context (visuals + text styles).
    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_visuals(self.visuals());
        ctx.all_styles_mut(apply_text_styles);
    }

    #[must_use]
    pub fn visuals(self) -> egui::Visuals {
        match self {
            Self::Dark => {
                let mut visuals = egui::Visuals::dark();
                visuals.panel_fill = DARK.panel_bg;
                visuals.window_fill = DARK.window_bg;
                visuals.extreme_bg_color = Color32::from_rgb(9, 13, 24);
                visuals.faint_bg_color = Color32::from_rgb(22, 26, 38);
                visuals.widgets.noninteractive.bg_fill = DARK.panel_bg;
                visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 35, 48);
                visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 52, 70);
                visuals.widgets.active.bg_fill = DARK.gold.gamma_multiply(0.55);
                visuals.selection.bg_fill = DARK.gold.gamma_multiply(0.45);
                visuals.selection.stroke = egui::Stroke::new(1.5, DARK.gold);
                visuals
            }
            Self::Light => {
                let mut visuals = egui::Visuals::light();
                visuals.panel_fill = LIGHT.panel_bg;
                visuals.window_fill = LIGHT.window_bg;
                visuals.extreme_bg_color = Color32::from_rgb(232, 235, 240);
                visuals.faint_bg_color = Color32::from_rgb(238, 241, 246);
                visuals.widgets.noninteractive.bg_fill = LIGHT.panel_bg;
                visuals.widgets.inactive.bg_fill = Color32::from_rgb(226, 230, 236);
                visuals.widgets.hovered.bg_fill = Color32::from_rgb(212, 218, 228);
                visuals.widgets.active.bg_fill = LIGHT.gold.gamma_multiply(0.6);
                visuals.selection.bg_fill = LIGHT.gold.gamma_multiply(0.5);
                visuals.selection.stroke = egui::Stroke::new(1.5, LIGHT.table_edge);
                visuals
            }
        }
    }
}

/// Font sizes from the ui.md typography table.
pub fn apply_text_styles(style: &mut egui::Style) {
    let mut styles = BTreeMap::new();
    styles.insert(
        TextStyle::Heading,
        FontId::new(24.0, FontFamily::Proportional),
    );
    styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
    styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );
    style.text_styles = styles;
}

/// The colour subset the table painter needs.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub page_bg: Color32,
    pub panel_bg: Color32,
    pub window_bg: Color32,
    pub felt_outer: Color32,
    pub felt_inner: Color32,
    pub table_edge: Color32,
    pub gold: Color32,
    pub theme_red: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub seat_bg: Color32,
    pub seat_human: Color32,
    pub seat_actor: Color32,
    pub seat_folded: Color32,
    pub overlay: Color32,
    pub win: Color32,
    pub lose: Color32,
    pub draw: Color32,
}

/// Dark palette from the ui.md token table.
pub static DARK: Palette = Palette {
    page_bg: Color32::from_rgb(0x0d, 0x0d, 0x1a),
    panel_bg: Color32::from_rgb(15, 19, 30),
    window_bg: Color32::from_rgb(22, 26, 40),
    felt_outer: Color32::from_rgb(0x1b, 0x5e, 0x20),
    felt_inner: Color32::from_rgb(0x0d, 0x3b, 0x0f),
    table_edge: Color32::from_rgb(0x5d, 0x40, 0x37),
    gold: Color32::from_rgb(0xff, 0xd7, 0x00),
    theme_red: Color32::from_rgb(0xe9, 0x45, 0x60),
    text_primary: Color32::from_rgb(0xe0, 0xe0, 0xe0),
    text_secondary: Color32::from_rgb(0xaa, 0xaa, 0xaa),
    seat_bg: Color32::from_rgb(44, 52, 60),
    seat_human: Color32::from_rgb(40, 72, 112),
    seat_actor: Color32::from_rgb(64, 84, 120),
    seat_folded: Color32::from_rgb(48, 52, 56),
    overlay: Color32::from_black_alpha(180),
    win: Color32::from_rgb(0x4c, 0xaf, 0x50),
    lose: Color32::from_rgb(0xf4, 0x43, 0x36),
    draw: Color32::from_rgb(0x21, 0x96, 0xf3),
};

/// Light palette derived from the same tokens (higher luminance backgrounds).
pub static LIGHT: Palette = Palette {
    page_bg: Color32::from_rgb(0xee, 0xf2, 0xf6),
    panel_bg: Color32::from_rgb(0xf7, 0xf9, 0xfb),
    window_bg: Color32::from_rgb(0xff, 0xff, 0xff),
    felt_outer: Color32::from_rgb(0x2e, 0x7d, 0x32),
    felt_inner: Color32::from_rgb(0x1b, 0x5e, 0x20),
    table_edge: Color32::from_rgb(0x8d, 0x6e, 0x63),
    gold: Color32::from_rgb(0xc9, 0x9b, 0x00),
    theme_red: Color32::from_rgb(0xd3, 0x2f, 0x4a),
    text_primary: Color32::from_rgb(0x21, 0x25, 0x2b),
    text_secondary: Color32::from_rgb(0x5f, 0x66, 0x70),
    seat_bg: Color32::from_rgb(0xe6, 0xea, 0xf0),
    seat_human: Color32::from_rgb(0xcb, 0xdd, 0xf0),
    seat_actor: Color32::from_rgb(0xff, 0xe9, 0xb0),
    seat_folded: Color32::from_rgb(0xd8, 0xdc, 0xe2),
    overlay: Color32::from_black_alpha(150),
    win: Color32::from_rgb(0x2e, 0x7d, 0x32),
    lose: Color32::from_rgb(0xc6, 0x28, 0x28),
    draw: Color32::from_rgb(0x15, 0x65, 0xc0),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_uses_original_page_colour() {
        assert_eq!(DARK.page_bg, Color32::from_rgb(0x0d, 0x0d, 0x1a));
        assert_eq!(Theme::Dark.palette().page_bg, DARK.page_bg);
    }

    #[test]
    fn light_theme_is_brighter_than_dark_theme() {
        let luminance = |color: Color32| {
            let [r, g, b, _] = color.to_array();
            0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)
        };
        assert!(luminance(LIGHT.page_bg) > luminance(DARK.page_bg) + 100.0);
        assert!(luminance(LIGHT.text_primary) < luminance(DARK.text_primary));
    }

    #[test]
    fn text_styles_cover_heading_body_and_small() {
        let mut style = egui::Style::default();
        apply_text_styles(&mut style);
        assert_eq!(
            style
                .text_styles
                .get(&TextStyle::Heading)
                .map(|font| font.size),
            Some(24.0)
        );
        assert_eq!(
            style
                .text_styles
                .get(&TextStyle::Body)
                .map(|font| font.size),
            Some(14.0)
        );
        assert_eq!(
            style
                .text_styles
                .get(&TextStyle::Small)
                .map(|font| font.size),
            Some(11.0)
        );
    }
}
