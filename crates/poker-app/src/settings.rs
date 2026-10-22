//! Persisted application settings.
//!
//! Stored as JSON under the per-user config directory. Loading is tolerant:
//! a missing file yields defaults; a corrupt file yields defaults plus a warning
//! string that the app shows in the status bar.

use std::path::Path;
use std::time::Duration;

use poker_core::Variant;
use poker_engine::GameConfig;
use serde::{Deserialize, Serialize};

use crate::assets;
use crate::theme::Theme;

/// Starting stack choices (mirrors the original room configuration whitelist).
pub const STACKS: [u64; 5] = [5_000, 10_000, 20_000, 50_000, 100_000];
/// Standard blind pairs.
pub const STANDARD_BLINDS: [(u64, u64); 6] = [
    (10, 20),
    (20, 40),
    (40, 80),
    (50, 100),
    (100, 200),
    (200, 400),
];
/// Short-deck ante choices (minimum bet is always twice the ante).
pub const SHORT_DECK_ANTES: [u64; 4] = [20, 40, 80, 100];
/// AI action delay choices in milliseconds.
pub const AI_DELAYS_MS: [u64; 3] = [800, 1800, 3200];
/// Maximum seats (standard variant; short deck is capped lower by the engine).
pub const MAX_PLAYERS: usize = 10;
/// The local human always occupies seat 0; the rest are AI.
pub const MAX_AI_PLAYERS: usize = MAX_PLAYERS - 1;

/// Game variant as persisted in settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantSetting {
    Standard,
    ShortDeck,
}

impl VariantSetting {
    pub fn to_variant(self) -> Variant {
        match self {
            Self::Standard => Variant::Standard,
            Self::ShortDeck => Variant::ShortDeck,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "标准德州",
            Self::ShortDeck => "短牌 6+",
        }
    }
}

/// Music playlist mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    Sequential,
    Shuffle,
    RepeatOne,
}

impl PlayMode {
    pub const ALL: [PlayMode; 3] = [PlayMode::Sequential, PlayMode::Shuffle, PlayMode::RepeatOne];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序播放",
            Self::Shuffle => "随机播放",
            Self::RepeatOne => "单曲循环",
        }
    }
}

/// All persisted user settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub player_name: String,
    pub variant: VariantSetting,
    pub players: usize,
    pub starting_stack: u64,
    pub small_blind: u64,
    pub big_blind: u64,
    pub ante: u64,
    pub ai_delay_ms: u64,
    pub music_enabled: bool,
    pub music_volume: f32,
    pub play_mode: PlayMode,
    pub sound_enabled: bool,
    pub fullscreen: bool,
    pub custom_titlebar: bool,
    pub show_analysis: bool,
    pub hide_hole_cards: bool,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            player_name: "玩家".to_owned(),
            variant: VariantSetting::Standard,
            players: 10,
            starting_stack: 20_000,
            small_blind: 40,
            big_blind: 80,
            ante: 40,
            ai_delay_ms: 1800,
            music_enabled: false,
            music_volume: 0.35,
            play_mode: PlayMode::Sequential,
            sound_enabled: true,
            fullscreen: false,
            custom_titlebar: false,
            show_analysis: true,
            hide_hole_cards: false,
            theme: Theme::Dark,
        }
    }
}

impl Settings {
    /// Clamp every field into a supported range (also repairs hand-edited files).
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.player_name = sanitize_name(&self.player_name);
        self.players = self.players.clamp(2, MAX_PLAYERS);
        if self.variant == VariantSetting::ShortDeck {
            // The 36-card deck supports at most 8 seats (engine config cap).
            self.players = self.players.min(8);
        }
        self.starting_stack = nearest_u64(self.starting_stack, &STACKS);
        let blinds = nearest_blinds(self.small_blind, self.big_blind);
        self.small_blind = blinds.0;
        self.big_blind = blinds.1;
        self.ante = nearest_u64(self.ante, &SHORT_DECK_ANTES);
        self.ai_delay_ms = self.ai_delay_ms.min(5_000);
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
        self
    }

    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&assets::settings_path())
    }

    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(settings) => (settings.sanitized(), None),
                Err(error) => (
                    Self::default(),
                    Some(format!("settings.json 解析失败，已使用默认设置：{error}")),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(error) => (
                Self::default(),
                Some(format!("settings.json 读取失败，已使用默认设置：{error}")),
            ),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&assets::settings_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create config dir {}: {error}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|error| format!("serialize settings: {error}"))?;
        std::fs::write(path, json)
            .map_err(|error| format!("write settings {}: {error}", path.display()))
    }

    pub fn variant(&self) -> Variant {
        self.variant.to_variant()
    }

    pub fn ai_delay(&self) -> Duration {
        Duration::from_millis(self.ai_delay_ms)
    }

    pub fn is_short_deck(&self) -> bool {
        self.variant == VariantSetting::ShortDeck
    }

    /// The engine table configuration described by these settings.
    #[must_use]
    pub fn game_config(&self) -> GameConfig {
        if self.is_short_deck() {
            GameConfig::short_deck(self.starting_stack, self.ante, self.ante.saturating_mul(2))
                .with_max_players(8)
        } else {
            GameConfig::standard(self.starting_stack, self.small_blind, self.big_blind)
                .with_max_players(MAX_PLAYERS)
        }
    }

    /// Short label such as "100/200" or "前注 40".
    pub fn stakes_label(&self) -> String {
        if self.is_short_deck() {
            format!("前注 {}", self.ante)
        } else {
            format!("{}/{}", self.small_blind, self.big_blind)
        }
    }

    pub fn ai_delay_label(&self) -> String {
        if self.ai_delay_ms == 0 {
            "立即".to_owned()
        } else {
            format!("{} ms", self.ai_delay_ms)
        }
    }
}

/// Trim a player name to at most 8 characters (the original room rule) and fall
/// back to the default name when empty.
pub fn sanitize_name(name: &str) -> String {
    let trimmed = name.trim();
    let truncated: String = trimmed.chars().take(8).collect();
    if truncated.is_empty() {
        "玩家".to_owned()
    } else {
        truncated
    }
}

fn nearest_u64(value: u64, options: &[u64]) -> u64 {
    options
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(value))
        .unwrap_or(value)
}

fn nearest_blinds(small: u64, big: u64) -> (u64, u64) {
    STANDARD_BLINDS
        .iter()
        .copied()
        .min_by_key(|(candidate_small, candidate_big)| {
            candidate_small.abs_diff(small) + candidate_big.abs_diff(big)
        })
        .unwrap_or((100, 200))
}
