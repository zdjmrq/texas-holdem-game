//! Standalone room server binary.
//!
//! Starts the authoritative protocol v2 server from POKER_* environment
//! variables (ServerOptions::from_env), prints the actual listening URL
//! (including the OS-assigned port when POKER_PORT=0) and shuts down gracefully
//! on Ctrl-C.
//!
//! Manual/interop use:
//!
//!     POKER_PORT=0 ./target/debug/examples/serve
//!     POKER_INTEROP_URL=ws://127.0.0.1:PORT \
//!       cargo test -p poker-client --test interop -- --ignored --nocapture
//!
//! The one-shot wrapper oracle/interop-rust-rust.sh starts this binary and runs
//! the poker-client interop suite against it.

use std::io::Write;
use std::time::Duration;

use poker_server::{ServerOptions, spawn_server_with_fallback};

fn log_level() -> tracing::Level {
    match std::env::var("POKER_LOG")
        .ok()
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("trace") => tracing::Level::TRACE,
        Some("debug") => tracing::Level::DEBUG,
        Some("warn") => tracing::Level::WARN,
        Some("error") => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(log_level())
        .with_target(true)
        .init();

    let options = ServerOptions::from_env();
    let requested = format!("{}:{}", options.host, options.port);
    let handle = spawn_server_with_fallback(options).await?;
    println!("poker-server listening on {}", handle.url());
    println!(
        "poker-server hello serverId=texas-holdem-game protocolVersion=2 appVersion={} (requested {requested})",
        handle.app_version()
    );
    let _ = std::io::stdout().flush();
    tracing::info!(url = %handle.url(), "poker-server started");

    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                tracing::warn!(%error, "ctrl-c handler failed");
            }
        }
        _ = tokio::time::sleep(Duration::from_secs(u64::MAX)) => {}
    }
    println!("poker-server shutting down");
    let _ = std::io::stdout().flush();
    handle.shutdown().await;
    std::process::exit(0);
}
