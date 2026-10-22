//! Desktop application for `texas-holdem-rs` (egui/eframe).
//!
//! Phase 0 host: the crate currently contains a *risk spike* window that exercises the
//! Windows-facing UI stack before the real table UI is written:
//!
//! * CJK text rendering (system-font fallback and explicit TTC loading) and the IME
//!   input pipeline for a Chinese `TextEdit`;
//! * a continuously animated widget (progress bar + spinner, 60 fps repaint);
//! * mp3 playback through `rodio` + `symphonia` from an embedded asset;
//! * the frameless-window / custom-title-bar / tray-icon / taskbar-attention strategy,
//!   with Windows-only code isolated behind `#[cfg(windows)]`.
//!
//! Linux/WSLg is only used to prove that the window starts, renders and decodes audio.
//! Windows is the acceptance platform (IME, tray, taskbar flashing, DPI, GPU).
//!
//! See `docs/spikes/egui-spike.md` for the measured results and the open risks.
#![forbid(unsafe_code)]

pub mod app;
pub mod audio;
pub mod cli;
pub mod fonts;
pub mod platform;
pub mod screenshot;
pub mod shell;

pub use cli::{CliOptions, FontMode};

/// Window / tray icon decoded from the bundled 32x32 RGBA PNG.
pub fn window_icon() -> Option<eframe::egui::IconData> {
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/attention-icon.png");
    let image = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Run the spike application (or one of its headless probes).
///
/// Headless modes (`--audio-test`, `--audio-play`, `--help`) return before any window is
/// created, so they can run in CI / without a display.
pub fn run() -> eframe::Result {
    init_logging();
    let cli = CliOptions::parse(std::env::args().skip(1));

    if cli.help {
        println!("{}", CliOptions::usage());
        return Ok(());
    }
    if cli.audio_probe {
        audio::print_headless_probe();
        return Ok(());
    }
    if let Some(seconds) = cli.audio_play_secs {
        if let Err(error) = audio::play_headless_probe(seconds) {
            eprintln!("[poker-app] audio playback probe failed: {error}");
        }
        return Ok(());
    }

    // Windows: named-mutex single-instance lock. Other platforms: no-op stub for the spike.
    let _instance_guard = match platform::ensure_single_instance("texas-holdem-rs-spike") {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("[poker-app] not starting a second instance: {error}");
            return Ok(());
        }
    };

    let icon = window_icon();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("德州扑克 Rust 重制版 — egui Phase 0 Spike")
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([900.0, 600.0])
        .with_decorations(!cli.frameless)
        .with_resizable(true);
    if let Some(icon) = icon.clone() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        centered: true,
        // Load CJK/Arabic/... system fonts on demand (fontique: DirectWrite on Windows,
        // fontconfig on Linux). Can be switched off once a bundled font covers the UI.
        system_font_fallback: true,
        ..Default::default()
    };

    eframe::run_native(
        "poker-app-spike",
        native_options,
        Box::new(move |cc| Ok(Box::new(app::SpikeApp::new(cc, cli, icon)))),
    )
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}
