//! Events produced by crate::Client::next_event.

use poker_protocol::{Hello, ServerMessage, ServerParseError};

/// One lifecycle or protocol event produced by crate::Client.
///
/// The sequence for a connection is
/// Connected, Hello, zero or more Message/ProtocolError events, Disconnected,
/// then (Reconnecting, Connected, ... | ReconnectFailed | Closed). Every event
/// is delivered exactly once and in wire order.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // protocol payloads are short-lived; boxing would complicates consumers
pub enum ClientEvent {
    /// A WebSocket connection was established and the hello handshake
    /// succeeded.
    Connected {
        /// true for a connection created by the automatic reconnect loop or by
        /// crate::Client::reconnect; false for the first connection.
        reconnecting: bool,
    },
    /// The server hello frame (also available through crate::Client::hello).
    Hello(Hello),
    /// A typed server message.
    Message(ServerMessage),
    /// The connection is gone.
    Disconnected {
        /// WebSocket close code, or None for a transport failure / abnormal
        /// closure (the browser reports 1006 for these).
        code: Option<u16>,
        /// Close reason if the server sent one.
        reason: String,
        /// true when an automatic reconnect has been scheduled.
        will_reconnect: bool,
    },
    /// The reconnect loop scheduled another attempt after delay_ms.
    Reconnecting {
        /// Zero-based delay index used by the backoff formula.
        attempt: u32,
        /// Delay before the next attempt, in milliseconds (jitter included).
        delay_ms: u64,
    },
    /// The reconnect loop gave up (attempt cap reached).
    ReconnectFailed {
        /// Number of retry delays that were scheduled.
        attempts: u32,
        /// Last connection error.
        last_error: String,
    },
    /// A server frame could not be parsed. The connection stays open (the
    /// reference client logs the error and continues).
    ProtocolError {
        /// Raw frame text.
        raw: String,
        /// Parser error.
        error: ServerParseError,
    },
    /// Terminal event: no further events will be produced.
    Closed,
}

impl ClientEvent {
    /// True for ClientEvent::Closed.
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Returns the typed message when this is ClientEvent::Message.
    pub fn message(&self) -> Option<&ServerMessage> {
        match self {
            Self::Message(message) => Some(message),
            _ => None,
        }
    }
}
