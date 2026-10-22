//! WebSocket client transport for the protocol v2 poker room server.
//!
//! This crate is the Rust equivalent of js/network.js (reference commit
//! e0c6c6f, v1.5.8): connection lifecycle, hello/version probing, terminal
//! close-code classification, exponential-backoff reconnect, resume-token
//! handling and typed message streaming. It owns no UI state and no game
//! rules; those live in poker-app and poker-engine.
//!
//! # Architecture
//!
//! One Client owns one WebSocket connection at a time:
//!
//! 1. Client::connect performs the TCP handshake, waits for the server hello
//!    frame and applies the configured HelloPolicy.
//! 2. Client::send writes typed protocol messages; calls are serialized
//!    because every write needs exclusive access to the client.
//! 3. Client::next_event returns lifecycle events and parsed ServerMessage
//!    values in wire order, and drives the automatic reconnect loop between
//!    calls.
//!
//! Typical application loop (a UI task that also needs to send commands should
//! run this loop in a dedicated task and forward commands through a channel):
//!
//! ~~~rust,no_run
//! # async fn example() -> Result<(), poker_client::ClientError> {
//! use poker_client::{Client, ClientConfig, ClientEvent};
//!
//! let mut client = Client::connect(ClientConfig::new("ws://127.0.0.1:3000")).await?;
//! while let Some(event) = client.next_event().await {
//!     match event {
//!         ClientEvent::Hello(hello) => println!("server {} v{}", hello.server_id, hello.app_version),
//!         ClientEvent::Message(message) => println!("message: {}", message.message_type()),
//!         ClientEvent::Disconnected { code, reason, .. } => {
//!             println!("disconnected: {code:?} {reason}");
//!         }
//!         ClientEvent::Closed => break,
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ~~~
//!
//! # Behaviour parity with js/network.js
//!
//! | Behaviour | Reference JS | This crate |
//! | --- | --- | --- |
//! | Terminal close codes | 1000, 1008, 1009, 1013, 4001 never auto-reconnect | same, via poker_protocol::is_terminal_close_code |
//! | Reconnect eligibility | only with resumeToken + roomCode (+ autoReconnect) | same: a stored SessionCredentials is required |
//! | Backoff | min(10000, 650 * 2^min(attempt,4)) + floor(rand * 350) ms | poker_protocol::reconnect_delay_ms with injectable jitter |
//! | Resume on open | onopen sends resume_session when a token exists | same (initial connect with restored session included) |
//! | Attempt counter reset | room_created / room_joined / game_state reset attempts | same |
//! | Unknown server message types | ignored by the switch default | ignored |
//! | Parse failures | console.error and continue | ClientEvent::ProtocolError (raw text preserved) |
//! | Outbound envelope | protocolVersion 2 on every message | ClientMessage::to_json_string adds it |
//! | Manual reconnect | disconnect(preserveSession) + 150 ms + connect | Client::reconnect (silent teardown, immediate retry) |
//! | Frame size limit | server enforces 16 KiB inbound | client rejects oversized outbound frames before writing |
//!
//! # Determinism
//!
//! Reconnect timing uses tokio time, so tokio::time::pause works in tests.
//! Request ids and jitter come from ClientHooks: FixedHooks makes both
//! deterministic, and FixedHooks::new(0, 0.5) reproduces the golden reconnect
//! delays captured in tests/golden/protocol/client-behaviour.json.
//!
//! # Known limitations
//!
//! * wss:// is rejected: this build has no TLS backend compiled (Phase 6
//!   desktop/LAN rooms use ws://; public deployments are a Phase 7 concern).
//! * The browser client has no handshake timeout; this client enforces one by
//!   default but can disable it with ClientConfig::without_handshake_timeout.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
pub mod config;
pub mod error;
pub mod event;
pub mod probe;
pub mod request_id;

mod transport;

pub use client::{ActionRequest, Client};
pub use config::{
    ClientConfig, ClientHooks, DEFAULT_HANDSHAKE_TIMEOUT, DEFAULT_SEND_TIMEOUT, FixedHooks,
    HelloMismatch, HelloPolicy, SessionCredentials, SystemHooks,
};
pub use error::ClientError;
pub use event::ClientEvent;
pub use poker_protocol::constants::PROTOCOL_VERSION;
pub use poker_protocol::{ActionType, RequestId, RoomOptionsInput, ServerMessage};
pub use probe::{PROBE_TIMEOUT, probe_server, probe_server_with_timeout};
pub use request_id::{RequestIdGenerator, RequestKind};
