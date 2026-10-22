//! Commands sent from the UI thread to the online worker.

use poker_client::{ActionRequest, SessionCredentials};
use poker_protocol::RoomOptionsInput;

/// One player action chosen in the online table UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineAction {
    Fold,
    Check,
    Call,
    AllIn,
    /// Raise to a total amount (the wire amount for a raise is raise-to).
    RaiseTo(u64),
}

/// Commands consumed by the worker thread.
#[derive(Debug, Clone, PartialEq)]
pub enum OnlineCommand {
    /// Open the WebSocket and perform the hello handshake. The session (when
    /// present) is stored on the client so reconnects resume automatically.
    Connect {
        url: String,
        session: Option<SessionCredentials>,
    },
    /// User-initiated disconnect; the worker stays alive for a later connect.
    Disconnect,
    /// Manual reconnect that keeps the resume session.
    Reconnect,
    /// create_room (the UI keeps to "join creates the room" per ui.md, but the
    /// command stays available for the demo/test paths).
    CreateRoom {
        name: String,
        options: RoomOptionsInput,
    },
    /// join_room; joining a free room code creates the room (ui.md section 3.12).
    JoinRoom {
        code: String,
        name: String,
        options: RoomOptionsInput,
    },
    LeaveRoom {
        room_code: String,
    },
    StartGame {
        room_code: String,
    },
    Action(ActionRequest),
    ReadyNext {
        room_code: String,
        hand_id: i64,
    },
    /// Store refresh credentials discovered from room_created / room_joined.
    SetSession(SessionCredentials),
    ClearSession,
    /// Stop the worker (window close / back to home).
    Shutdown,
}
