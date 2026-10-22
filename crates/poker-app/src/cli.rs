//! Command line options for the desktop app.

use std::path::PathBuf;

/// Deterministic demo scenario used by screenshot automation and manual smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Home screen with the default settings.
    Home,
    /// Table screen, pre-flop, waiting for the human (or AI) to act.
    Table,
    /// Table screen after the flop has been dealt.
    TableFlop,
    /// Table screen for the 6+ short-deck variant.
    TableShortDeck,
    /// Table screen with the settlement dialog of a finished hand.
    Settlement,
    /// Settings screen.
    Settings,
}

impl Scenario {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "home" => Some(Self::Home),
            "table" | "preflop" => Some(Self::Table),
            "table-flop" | "flop" => Some(Self::TableFlop),
            "table-shortdeck" | "shortdeck" => Some(Self::TableShortDeck),
            "settlement" | "result" => Some(Self::Settlement),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Table => "table",
            Self::TableFlop => "table-flop",
            Self::TableShortDeck => "table-shortdeck",
            Self::Settlement => "settlement",
            Self::Settings => "settings",
        }
    }
}

/// Parsed command line.
#[derive(Debug, Clone)]
pub struct CliOptions {
    /// Render a few frames, save a PNG and exit.
    pub screenshot: Option<PathBuf>,
    /// Frame index at which the screenshot is requested.
    pub screenshot_frame: u64,
    /// Start with an undecorated window (custom title bar).
    pub frameless: bool,
    /// Low-memory mode placeholder (Phase 7 will reduce animations / preloads).
    pub low_memory: bool,
    /// Do not initialise audio at all.
    pub no_audio: bool,
    /// Jump directly into a deterministic demo scenario.
    pub scenario: Option<Scenario>,
    /// Start with the human hole cards hidden (screenshot / smoke testing).
    pub hide_cards: bool,
    /// Open the raise panel once the human is to act (screenshot / smoke testing).
    pub open_raise: bool,
    /// Seed for the deterministic demo scenario.
    pub seed: Option<String>,
    /// Headless: decode the bundled mp3 and print audio device info, then exit.
    pub audio_probe: bool,
    /// Headless: play the bundled mp3 for N seconds on the default device, then exit.
    pub audio_play_secs: Option<f32>,
    pub help: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            screenshot: None,
            screenshot_frame: 60,
            frameless: false,
            low_memory: false,
            no_audio: false,
            scenario: None,
            hide_cards: false,
            open_raise: false,
            seed: None,
            audio_probe: false,
            audio_play_secs: None,
            help: false,
        }
    }
}

impl CliOptions {
    /// The seed used by deterministic scenarios.
    pub fn scenario_seed(&self) -> String {
        self.seed
            .clone()
            .unwrap_or_else(|| "poker-app-demo".to_owned())
    }

    pub fn usage() -> &'static str {
        "\
poker-app - Texas Hold'em desktop app (egui/eframe)

USAGE:
    poker-app [OPTIONS]

OPTIONS:
    --screenshot <PATH>       Render frames, save a PNG and exit (screenshot mode)
    --screenshot-frame <N>    Frame index used for the screenshot (default: 60)
    --scenario <NAME>         Deterministic demo state: home | table | table-flop | table-shortdeck | settlement | settings
    --seed <SEED>             Deck/AI seed for deterministic scenarios (default: poker-app-demo)
    --hide-cards              Start with the human hole cards hidden
    --open-raise              Open the raise panel once it is the human's turn
    --frameless               Start with the custom title bar (undecorated window)
    --low-memory              Low-memory mode placeholder (Phase 7)
    --no-audio                Do not initialise audio
    --audio-test              Headless: decode mp3 + audio device info, then exit
    --audio-play [SECONDS]    Headless: play the bundled mp3 for N seconds (default: 2)
    -h, --help                Print this help"
    }

    /// Parse arguments (without the program name).
    pub fn parse<I>(args: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
                "--screenshot-frame" => {
                    options.screenshot_frame = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(options.screenshot_frame);
                }
                "--scenario" => {
                    if let Some(value) = args.next() {
                        match Scenario::parse(&value) {
                            Some(scenario) => options.scenario = Some(scenario),
                            None => eprintln!("[poker-app] unknown --scenario {value:?}"),
                        }
                    }
                }
                "--seed" => options.seed = args.next(),
                "--hide-cards" => options.hide_cards = true,
                "--open-raise" => options.open_raise = true,
                "--frameless" => options.frameless = true,
                "--low-memory" => options.low_memory = true,
                "--no-audio" => options.no_audio = true,
                "--audio-test" => options.audio_probe = true,
                "--audio-play" => {
                    let requested = args.peek().and_then(|value| value.parse::<f32>().ok());
                    match requested {
                        Some(seconds) => {
                            let _ = args.next();
                            options.audio_play_secs = Some(seconds);
                        }
                        None => options.audio_play_secs = Some(2.0),
                    }
                }
                "--help" | "-h" => options.help = true,
                other => {
                    if let Some(value) = other.strip_prefix("--screenshot=") {
                        options.screenshot = Some(PathBuf::from(value));
                    } else if let Some(value) = other.strip_prefix("--scenario=") {
                        match Scenario::parse(value) {
                            Some(scenario) => options.scenario = Some(scenario),
                            None => eprintln!("[poker-app] unknown --scenario {value:?}"),
                        }
                    } else if let Some(value) = other.strip_prefix("--seed=") {
                        options.seed = Some(value.to_owned());
                    } else if let Some(value) = other.strip_prefix("--screenshot-frame=") {
                        options.screenshot_frame =
                            value.parse().unwrap_or(options.screenshot_frame);
                    } else {
                        eprintln!("[poker-app] ignoring unknown argument: {other}");
                    }
                }
            }
        }
        options
    }
}
