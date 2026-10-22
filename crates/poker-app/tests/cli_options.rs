//! CLI parsing and font-strategy checks (no window is created).

use poker_app::cli::{CliOptions, FontMode};

fn parse(args: &[&str]) -> CliOptions {
    CliOptions::parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[test]
fn defaults_are_interactive_system_font_mode() {
    let options = parse(&[]);
    assert_eq!(options.font_mode, FontMode::SystemFallback);
    assert!(options.screenshot.is_none());
    assert!(!options.frameless);
    assert!(!options.auto_play);
    assert!(!options.audio_probe);
    assert!(options.audio_play_secs.is_none());
    assert!(!options.help);
}

#[test]
fn parses_spike_flags() {
    let options = parse(&[
        "--screenshot",
        "/tmp/shot.png",
        "--font-mode",
        "explicit",
        "--frameless",
        "--auto-play",
        "--screenshot-frame",
        "12",
    ]);
    assert_eq!(
        options.screenshot.as_deref(),
        Some(std::path::Path::new("/tmp/shot.png"))
    );
    assert_eq!(options.font_mode, FontMode::ExplicitSystem);
    assert!(options.frameless);
    assert!(options.auto_play);
    assert_eq!(options.screenshot_frame, 12);
}

#[test]
fn parses_equals_form_and_optional_audio_play_duration() {
    let options = parse(&["--screenshot=/tmp/a.png", "--font-mode=bundled"]);
    assert_eq!(
        options.screenshot.as_deref(),
        Some(std::path::Path::new("/tmp/a.png"))
    );
    assert_eq!(options.font_mode, FontMode::Bundled);

    let options = parse(&["--audio-play", "1.5"]);
    assert_eq!(options.audio_play_secs, Some(1.5));

    let options = parse(&["--audio-play"]);
    assert_eq!(options.audio_play_secs, Some(2.0));
}

#[test]
fn font_mode_aliases_parse() {
    for alias in ["system", "system-fallback", "fallback"] {
        assert_eq!(FontMode::parse(alias), Some(FontMode::SystemFallback));
    }
    for alias in ["explicit", "explicit-system", "system-file"] {
        assert_eq!(FontMode::parse(alias), Some(FontMode::ExplicitSystem));
    }
    for alias in ["bundled", "bundle"] {
        assert_eq!(FontMode::parse(alias), Some(FontMode::Bundled));
    }
    assert_eq!(FontMode::parse("nonsense"), None);
}
