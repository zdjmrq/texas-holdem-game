//! CLI parsing tests (no window is created).

use poker_app::cli::{CliOptions, Scenario};

fn parse(args: &[&str]) -> CliOptions {
    CliOptions::parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[test]
fn defaults_are_interactive() {
    let options = parse(&[]);
    assert!(options.screenshot.is_none());
    assert_eq!(options.screenshot_frame, 60);
    assert!(options.scenario.is_none());
    assert!(!options.frameless);
    assert!(!options.low_memory);
    assert!(!options.no_audio);
    assert!(!options.help);
}

#[test]
fn parses_screenshot_and_scenario() {
    let options = parse(&[
        "--screenshot",
        "/tmp/table.png",
        "--screenshot-frame",
        "90",
        "--scenario",
        "table-flop",
        "--seed",
        "demo-seed",
    ]);
    assert_eq!(
        options.screenshot.as_deref(),
        Some(std::path::Path::new("/tmp/table.png"))
    );
    assert_eq!(options.screenshot_frame, 90);
    assert_eq!(options.scenario, Some(Scenario::TableFlop));
    assert_eq!(options.seed.as_deref(), Some("demo-seed"));
}

#[test]
fn scenario_aliases_parse() {
    assert_eq!(Scenario::parse("home"), Some(Scenario::Home));
    assert_eq!(Scenario::parse("preflop"), Some(Scenario::Table));
    assert_eq!(Scenario::parse("flop"), Some(Scenario::TableFlop));
    assert_eq!(Scenario::parse("settlement"), Some(Scenario::Settlement));
    assert_eq!(Scenario::parse("settings"), Some(Scenario::Settings));
    assert_eq!(Scenario::parse("nonsense"), None);
}

#[test]
fn parses_equals_forms_and_optional_audio_play() {
    let options = parse(&[
        "--screenshot=/tmp/a.png",
        "--scenario=settlement",
        "--seed=abc",
    ]);
    assert_eq!(
        options.screenshot.as_deref(),
        Some(std::path::Path::new("/tmp/a.png"))
    );
    assert_eq!(options.scenario, Some(Scenario::Settlement));
    assert_eq!(options.seed.as_deref(), Some("abc"));

    let options = parse(&["--audio-play", "1.5"]);
    assert_eq!(options.audio_play_secs, Some(1.5));
    let options = parse(&["--audio-play"]);
    assert_eq!(options.audio_play_secs, Some(2.0));
}

#[test]
fn parses_toggles_and_unknown_arguments_are_ignored() {
    let options = parse(&["--frameless", "--low-memory", "--no-audio", "--wat"]);
    assert!(options.frameless);
    assert!(options.low_memory);
    assert!(options.no_audio);
}

#[test]
fn scenario_seed_defaults_to_demo_seed() {
    let options = parse(&[]);
    assert_eq!(options.scenario_seed(), "poker-app-demo");
}
