//! `poker-app` binary entry point for the Phase 0 egui spike.

fn main() {
    if let Err(error) = poker_app::run() {
        eprintln!("[poker-app] fatal: {error}");
        std::process::exit(1);
    }
}
