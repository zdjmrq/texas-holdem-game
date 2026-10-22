//! Settings persistence and sanitisation tests.

use std::path::PathBuf;

use poker_app::settings::{PlayMode, STACKS, Settings, VariantSetting, sanitize_name};

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("poker-app-test-{}-{name}.json", std::process::id()))
}

#[test]
fn settings_roundtrip_through_json() {
    let path = temp_path("roundtrip");
    let settings = Settings {
        player_name: "小明".to_owned(),
        players: 8,
        starting_stack: 20_000,
        small_blind: 50,
        big_blind: 100,
        ai_delay_ms: 1_800,
        music_enabled: true,
        music_volume: 0.7,
        play_mode: PlayMode::Shuffle,
        variant: VariantSetting::ShortDeck,
        ante: 80,
        custom_titlebar: true,
        ..Settings::default()
    };
    settings.save_to(&path).expect("save settings");
    let (loaded, warning) = Settings::load_from(&path);
    assert!(warning.is_none());
    assert_eq!(loaded, settings.clone().sanitized());
    let _ = std::fs::remove_file(path);
}

#[test]
fn missing_file_yields_defaults_without_warning() {
    let path = temp_path("missing-does-not-exist");
    let _ = std::fs::remove_file(&path);
    let (settings, warning) = Settings::load_from(&path);
    assert!(warning.is_none());
    assert_eq!(settings, Settings::default());
}

#[test]
fn corrupt_file_falls_back_to_defaults_with_warning() {
    let path = temp_path("corrupt");
    std::fs::write(&path, "{ this is not json").expect("write corrupt settings");
    let (settings, warning) = Settings::load_from(&path);
    assert_eq!(settings, Settings::default());
    assert!(warning.is_some(), "corrupt settings should warn");
    let _ = std::fs::remove_file(path);
}

#[test]
fn sanitize_clamps_out_of_range_values() {
    let settings = Settings {
        players: 99,
        music_volume: 4.0,
        ai_delay_ms: 999_999,
        starting_stack: 1_234,
        player_name: "一二三四五六七八九十".to_owned(),
        ..Settings::default()
    };
    let sanitized = settings.sanitized();
    assert!(sanitized.players <= 10);
    assert_eq!(sanitized.music_volume, 1.0);
    assert_eq!(sanitized.ai_delay_ms, 5_000);
    assert!(STACKS.contains(&sanitized.starting_stack));
    assert_eq!(sanitized.player_name.chars().count(), 8);
}

#[test]
fn short_deck_sanitize_caps_seats_at_eight() {
    let settings = Settings {
        variant: VariantSetting::ShortDeck,
        players: 10,
        ..Settings::default()
    };
    assert_eq!(settings.sanitized().players, 8);
}

#[test]
fn player_name_gets_default_when_empty() {
    assert_eq!(sanitize_name("   "), "玩家");
    assert_eq!(sanitize_name("  小 明  "), "小 明");
    assert_eq!(sanitize_name("abcdefghijk").chars().count(), 8);
}
