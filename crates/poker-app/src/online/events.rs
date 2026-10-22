//! Events sent from the online worker back to the UI thread.

use poker_client::{ClientError, ClientEvent};

/// Bookkeeping kind of an in-flight request (used for the pending overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    CreateOrJoin,
    StartGame,
    Action,
    ReadyNext,
    LeaveRoom,
    Resume,
}

/// Receipt for a command whose request was written to the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReceipt {
    pub request_id: String,
    pub kind: PendingKind,
    pub hand_id: i64,
    pub turn_id: i64,
    pub expected_state_version: i64,
}

/// One item on the worker to UI channel.
///
/// The Client variant carries the full protocol event enum; it is larger than
/// the bookkeeping variants, but this channel crosses a worker thread at UI
/// frequency (a few messages per second), so boxing it would add allocation
/// and match noise without a measurable benefit.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum WorkerEvent {
    /// A poker-client lifecycle or protocol event.
    Client(ClientEvent),
    /// The initial Client::connect (or a reconnect) failed synchronously.
    ConnectFailed(ClientError),
    /// The embedded poker-server reported its listen URL.
    LocalServerUrl(String),
    /// A request was written; the view records it as pending.
    CommandSent(CommandReceipt),
    /// A request could not be written (not connected / oversized / closed).
    CommandFailed { kind: PendingKind, message: String },
    /// The worker thread stopped (shutdown or fatal error).
    WorkerClosed,
}
