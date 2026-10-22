//! `poker-app` binary entry point for the Phase 0 egui spike.
//!
//! On Windows release builds this is a GUI subsystem binary (no console window); keep the
//! debug build attached to the console so logs stay visible while testing.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = poker_app::run() {
        eprintln!("[poker-app] fatal: {error}");
        std::process::exit(1);
    }
}
