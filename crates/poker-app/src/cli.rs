//! Command line options for the spike binary.
//!
//! Hand-rolled parsing on purpose: the spike should not pull in clap before the real app
//! architecture (and its CLI surface) is decided.

use std::path::PathBuf;

/// How the spike provides CJK glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontMode {
    /// Use eframe's on-demand system font fallback (fontique: DirectWrite / fontconfig).
    SystemFallback,
    /// Explicitly load a known system CJK font file (handles TTC face indices).
    ExplicitSystem,
    /// Load a font file from assets/fonts/ (the packaged-font path used by the release build).
    Bundled,
}

impl FontMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system-fallback" | "system" | "fallback" => Some(Self::SystemFallback),
            "explicit" | "explicit-system" | "system-file" => Some(Self::ExplicitSystem),
            "bundled" | "bundle" => Some(Self::Bundled),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SystemFallback => "system-fallback",
            Self::ExplicitSystem => "explicit-system",
            Self::Bundled => "bundled",
        }
    }
}

/// Parsed command line for the Phase 0 spike.
#[derive(Debug, Clone)]
pub struct CliOptions {
    /// Save a PNG screenshot of the window to this path and exit.
    pub screenshot: Option<PathBuf>,
    /// Frame index at which the screenshot is requested (default: 30, about 0.5 s at 60 fps).
    pub screenshot_frame: u64,
    pub font_mode: FontMode,
    /// Start with an undecorated window and the custom title bar.
    pub frameless: bool,
    /// Start playing the embedded background track on launch.
    pub auto_play: bool,
    /// Headless: decode the embedded mp3 and list audio devices, then exit.
    pub audio_probe: bool,
    /// Headless: play the embedded mp3 for N seconds on the default device, then exit.
    pub audio_play_secs: Option<f32>,
    pub help: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            screenshot: None,
            screenshot_frame: 30,
            font_mode: FontMode::SystemFallback,
            frameless: false,
            auto_play: false,
            audio_probe: false,
            audio_play_secs: None,
            help: false,
        }
    }
}

impl CliOptions {
    pub fn usage() -> &'static str {
        "\
poker-app - Phase 0 egui risk spike

USAGE:
    poker-app [OPTIONS]

OPTIONS:
    --screenshot <PATH>       Render a few frames, save a PNG, then exit
    --screenshot-frame <N>    Frame index used for the screenshot (default: 30)
    --font-mode <MODE>        system-fallback | explicit | bundled   (default: system-fallback)
    --frameless               Undecorated window + custom title bar / edge resize
    --auto-play               Start the embedded 01.mp3 on launch
    --audio-test              Headless: decode mp3 + print audio device info, then exit
    --audio-play [SECONDS]    Headless: play the embedded mp3 for N seconds (default: 2)
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
                "--font-mode" => {
                    if let Some(value) = args.next() {
                        match FontMode::parse(&value) {
                            Some(mode) => options.font_mode = mode,
                            None => eprintln!(
                                "[poker-app] unknown --font-mode {value:?}, keeping default"
                            ),
                        }
                    }
                }
                "--frameless" => options.frameless = true,
                "--auto-play" => options.auto_play = true,
                "--audio-test" => options.audio_probe = true,
                "--audio-play" => {
                    // The duration is optional: "--audio-play 3" or plain "--audio-play".
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
                    } else if let Some(value) = other.strip_prefix("--font-mode=") {
                        match FontMode::parse(value) {
                            Some(mode) => options.font_mode = mode,
                            None => eprintln!("[poker-app] unknown --font-mode {value:?}"),
                        }
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
