//! Authoritative WebSocket room server for protocol v2 (tokio + axum).
//!
//! This crate is the Rust port of the reference project's
//! server/local-server.js + server/server-game.js. It serves the original
//! JS desktop client (see docs/specs/protocol-v2.md) and exposes a small
//! embedding API for the Rust desktop app:
//!
//! ```text
//! let options = poker_server::ServerOptions { port: 0, ..Default::default() };
//! let handle = poker_server::spawn_server(options).await?;
//! println!("listening on {}", handle.local_addr());
//! handle.shutdown().await;
//! ```
//!
//! # Responsibilities
//!
//! * protocol v2 framing, validation and error replies (poker-protocol DTOs);
//! * room lifecycle: create/join/leave/start/ready/actions/resume/snapshot,
//!   host migration, AI seat filling, disconnect grace, empty-room TTL;
//! * authoritative game state driven by poker-engine (the engine owns all
//!   betting/settlement rules; this crate only adapts it to the wire protocol);
//! * AI seats through the ServerAi trait, computed off the async reactor in a
//!   serial per-room spawn_blocking queue;
//! * transport hardening: Origin allowlist, payload limit, per-connection
//!   rate limit, connection/per-IP caps and the reference close codes.
//!
//! # Game authority
//!
//! poker-engine is the single source of truth for betting, pots and
//! settlement. The room layer mirrors the engine seat state after each change
//! so lobby messages and game snapshots share one representation. Hidden
//! information never leaves the server: game_state messages are personalised
//! per recipient and only reveal hole cards inside lastHandResult at showdown.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Several room handlers mirror the reference server's nested guard structure
// (already in a room / has a game / is host); collapsing those ifs into
// let-chains would obscure the correspondence with local-server.js.
#![allow(clippy::collapsible_if)]

pub mod ai;
pub mod options;
mod room;
mod server;
mod wire;

pub use ai::{AiDecision, AiFactory, AiView, ServerAi, StubAi, StubAiFactory};
pub use options::ServerOptions;
pub use server::{ServerHandle, probe_server, spawn_server, spawn_server_with_fallback};
