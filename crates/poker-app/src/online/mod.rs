//! Online (WebSocket room) UI and client integration.
//!
//! This module is the Rust port of the online screens described in
//! docs/specs/ui.md section 5 (connection, lobby, room table, reconnect and
//! settlement), wired to the poker-client WebSocket client and to the
//! embedded poker-server room service.
//!
//! Layout:
//! * commands/events: UI to worker commands and worker to UI events.
//! * view: pure state machine that reduces events into an OnlineView.
//! * mapping: protocol DTO to display/engine types (cards, legal actions,
//!   settlement rows, request ids).
//! * text: every user-facing Chinese string for the online screens.
//! * worker: background thread with a current-thread tokio runtime that owns
//!   poker_client::Client.
//! * controller: OnlineClient, the UI-facing facade. It implements the
//!   crate::online_controller::OnlineController trait used by app.rs.
//! * session_store: resume credentials persisted next to the app config.
//! * demo: fake OnlineView data for screenshots/tests without a server.
//! * ui: egui screens (connect, lobby, room table, settlement, status).
//!
//! The module deliberately keeps all online-only code here so app.rs only
//! needs the small mount points listed in docs/specs/online-ui-plan.md.

pub mod commands;
pub mod events;
pub mod mapping;
pub mod text;
pub mod view;

pub use commands::{OnlineAction, OnlineCommand};
pub use events::{CommandReceipt, PendingKind, WorkerEvent};
pub use view::{ConnStatus, OnlineEffect, OnlineStage, OnlineView, PendingAction};

pub mod native;
pub mod worker;

mod session_store;
