//! Texas Hold'em desktop application (egui/eframe).
//!
//! Native local and online games for standard hold'em and 6+ short deck,
//! with the production poker-ai decision pipeline and background equity via poker-prob,
//! persisted settings, background music and Windows shell integration.
//!
//! Linux/WSL is used for automated tests and screenshots; Windows x64 is the
//! acceptance platform (see docs/windows-smoke.md).
#![forbid(unsafe_code)]

pub mod ai;
pub mod ai_background;
pub mod ai_bridge;
pub mod analysis;
pub mod app;
pub mod assets;
pub mod audio;
pub mod cli;
pub mod focus;
pub mod fonts;
pub mod layout;
pub mod local_server;
pub mod online;
pub mod platform;
pub mod screenshot;
pub mod session;
pub mod settings;
pub mod shell;
pub mod table_ctrl;
pub mod theme;
pub mod ui;

pub use cli::{CliOptions, Scenario};
pub use settings::Settings;

/// Window / tray icon decoded from the bundled 32x32 RGBA PNG.
pub fn window_icon() -> Option<eframe::egui::IconData> {
    let image = image::load_from_memory(assets::ICON_PNG).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Run the application (or one of its headless audio probes).
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

    // Windows: named-mutex single-instance lock. Other platforms: no-op stub.
    let _instance_guard = match platform::ensure_single_instance("texas-holdem-rs") {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("[poker-app] not starting a second instance: {error}");
            return Ok(());
        }
    };

    let (settings, warning) = Settings::load();
    if let Some(warning) = &warning {
        eprintln!("[poker-app] settings: {warning}");
    }
    let frameless = cli.frameless || settings.custom_titlebar;
    let icon = window_icon();
    let initial_size = cli.window_size.unwrap_or((1280.0, 860.0));
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("德州扑克 Rust 重制版")
        .with_inner_size([initial_size.0, initial_size.1])
        .with_min_inner_size([900.0, 600.0])
        .with_decorations(!frameless)
        .with_resizable(true)
        .with_fullscreen(settings.fullscreen);
    if let Some(icon) = icon.clone() {
        viewport = viewport.with_icon(icon);
    }
    let native_options = eframe::NativeOptions {
        viewport,
        centered: true,
        system_font_fallback: true,
        ..Default::default()
    };

    eframe::run_native(
        "texas-holdem-rs",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(app::PokerApp::new(
                cc, cli, settings, warning, icon,
            )))
        }),
    )
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}
