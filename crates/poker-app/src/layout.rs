//! Window layout tiers, toolbar hover zone and small-window table sizing.
//!
//! Ported from docs/specs/ui.md sections 2.6-2.7:
//! * <=760 logical px high: compact spacing;
//! * <=650 logical px high: tiny mode, the toolbar detaches into a hover zone
//!   in front of the content;
//! * the table keeps a minimum height so seats never get clipped, and the page
//!   scrolls vertically when the window is shorter.

/// Height breakpoint for the compact tier.
pub const COMPACT_HEIGHT: f32 = 760.0;
/// Height breakpoint for the tiny (detached toolbar) tier.
pub const TINY_HEIGHT: f32 = 650.0;
/// The hover zone is 85% of the toolbar height (JS: round(header.offsetHeight * 0.85)).
pub const HEADER_HOVER_RATIO: f32 = 0.85;
/// Fallback hover zone height from the original CSS var default (64px).
pub const HEADER_FALLBACK_HEIGHT: f32 = 64.0;
/// Toolbar height in normal mode.
pub const TOP_BAR_HEIGHT_NORMAL: f32 = 40.0;
/// Toolbar height in compact mode.
pub const TOP_BAR_HEIGHT_COMPACT: f32 = 30.0;
/// Minimum table canvas height in normal mode.
pub const MIN_TABLE_HEIGHT_NORMAL: f32 = 560.0;
/// Minimum table canvas height in compact mode.
pub const MIN_TABLE_HEIGHT_COMPACT: f32 = 520.0;
/// Minimum table canvas height in tiny mode (ui.md 2.7: 480).
pub const MIN_TABLE_HEIGHT_TINY: f32 = 480.0;

/// Size tier derived from the logical content height (never DPI-scaled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTier {
    Normal,
    Compact,
    Tiny,
}

impl WindowTier {
    #[must_use]
    pub fn from_height(logical_height: f32) -> Self {
        if logical_height <= TINY_HEIGHT {
            Self::Tiny
        } else if logical_height <= COMPACT_HEIGHT {
            Self::Compact
        } else {
            Self::Normal
        }
    }

    /// Uniform scale for seat/card sizes (compact UI shrinks the table a bit).
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Compact => 0.92,
            Self::Tiny => 0.84,
        }
    }

    #[must_use]
    pub fn top_bar_height(self) -> f32 {
        match self {
            Self::Normal => TOP_BAR_HEIGHT_NORMAL,
            _ => TOP_BAR_HEIGHT_COMPACT,
        }
    }

    #[must_use]
    pub fn min_table_height(self) -> f32 {
        match self {
            Self::Normal => MIN_TABLE_HEIGHT_NORMAL,
            Self::Compact => MIN_TABLE_HEIGHT_COMPACT,
            Self::Tiny => MIN_TABLE_HEIGHT_TINY,
        }
    }

    /// In tiny mode the toolbar detaches into the hover zone (ui.md 2.6).
    #[must_use]
    pub fn toolbar_detached(self) -> bool {
        self == Self::Tiny
    }
}

/// Height of the top hover zone: 85% of the toolbar height, fallback 64px.
#[must_use]
pub fn header_hover_height(top_bar_height: f32) -> f32 {
    if top_bar_height <= 0.0 {
        HEADER_FALLBACK_HEIGHT * HEADER_HOVER_RATIO
    } else {
        (top_bar_height / HEADER_HOVER_RATIO).max(top_bar_height)
    }
}

/// Whether the detached toolbar should be visible this frame.
///
/// Mirrors the CSS :has() rules from ui.md 2.6: visible while the pointer is
/// in the hover zone, while the pointer is over the toolbar itself, or while
/// keyboard focus is inside the toolbar region.
#[must_use]
pub fn detached_toolbar_visible(
    pointer_in_hover_zone: bool,
    pointer_in_toolbar: bool,
    keyboard_focus_in_toolbar: bool,
    pinned: bool,
) -> bool {
    pointer_in_hover_zone || pointer_in_toolbar || keyboard_focus_in_toolbar || pinned
}

/// Whether the table content needs vertical scrolling at the given height.
#[must_use]
pub fn needs_table_scroll(tier: WindowTier, available_height: f32) -> bool {
    available_height < tier.min_table_height()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_tiers_follow_ui_spec_breakpoints() {
        assert_eq!(WindowTier::from_height(900.0), WindowTier::Normal);
        assert_eq!(WindowTier::from_height(761.0), WindowTier::Normal);
        assert_eq!(WindowTier::from_height(760.0), WindowTier::Compact);
        assert_eq!(WindowTier::from_height(700.0), WindowTier::Compact);
        assert_eq!(WindowTier::from_height(650.0), WindowTier::Tiny);
        assert_eq!(WindowTier::from_height(600.0), WindowTier::Tiny);
    }

    #[test]
    fn hover_zone_is_eighty_five_percent_of_toolbar() {
        assert!((header_hover_height(40.0) - 40.0 / 0.85).abs() < 0.001);
        assert!((header_hover_height(64.0) - 64.0 / 0.85).abs() < 0.001);
        assert!((header_hover_height(0.0) - 64.0 * 0.85).abs() < 0.001);
    }

    #[test]
    fn detached_toolbar_visibility_matches_css_rules() {
        assert!(!detached_toolbar_visible(false, false, false, false));
        assert!(detached_toolbar_visible(true, false, false, false));
        assert!(detached_toolbar_visible(false, true, false, false));
        assert!(detached_toolbar_visible(false, false, true, false));
        assert!(detached_toolbar_visible(false, false, false, true));
    }

    #[test]
    fn small_window_table_scroll_and_scale() {
        assert!(!needs_table_scroll(WindowTier::Normal, 700.0));
        assert!(needs_table_scroll(WindowTier::Tiny, 470.0));
        assert!(WindowTier::Tiny.min_table_height() < WindowTier::Normal.min_table_height());
        assert!(WindowTier::Tiny.scale() < WindowTier::Compact.scale());
        assert!(WindowTier::Tiny.toolbar_detached());
        assert!(!WindowTier::Compact.toolbar_detached());
    }
}
