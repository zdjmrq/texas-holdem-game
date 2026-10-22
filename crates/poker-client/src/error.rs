//! Error type shared by the client, its handshake helpers and tests.

use std::fmt;

use poker_protocol::ServerParseError;

use crate::config::HelloMismatch;

/// Failure of a crate::Client operation.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// The URL is not a WebSocket URL.
    ///
    /// The reference client shows a Chinese hint for this case
    /// (js/network.js lines 83-86: server address must start with ws:// or
    /// wss://).
    InvalidUrl(String),
    /// The URL uses wss://, but this build has no TLS backend compiled in.
    ///
    /// Phase 6 desktop clients talk to a local/LAN ws:// room server; public
    /// deployments (which need wss://) are a Phase 7 concern.
    UnsupportedScheme(String),
    /// The TCP/WebSocket handshake or a socket operation failed.
    WebSocket(String),
    /// The server did not send hello within the configured handshake timeout.
    HandshakeTimeout {
        /// Timeout that expired, in milliseconds.
        timeout_ms: u64,
    },
    /// The hello payload did not match the configured policy.
    Hello(HelloMismatch),
    /// send was called while not connected.
    ///
    /// The reference client send() returns false in this case
    /// (js/network.js lines 255-261).
    NotConnected,
    /// Writing a frame failed; the connection is unusable afterwards.
    SendFailed(String),
    /// The serialized message exceeds the protocol 16 KiB frame limit.
    MessageTooLarge {
        /// Serialized frame length in bytes.
        len: usize,
        /// Configured limit.
        max: usize,
    },
    /// A resume operation was requested without stored session credentials.
    NoSession,
    /// A server frame could not be parsed; the raw text is preserved for
    /// diagnostics.
    Protocol {
        /// Raw frame text.
        raw: String,
        /// Parser error.
        error: ServerParseError,
    },
    /// The client is closed (intentional disconnect or terminal close).
    Closed,
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(url) => write!(formatter, "invalid WebSocket URL: {url}"),
            Self::UnsupportedScheme(url) => {
                write!(formatter, "wss:// is not supported by this build: {url}")
            }
            Self::WebSocket(detail) => write!(formatter, "WebSocket error: {detail}"),
            Self::HandshakeTimeout { timeout_ms } => {
                write!(formatter, "handshake timed out after {timeout_ms} ms")
            }
            Self::Hello(mismatch) => write!(formatter, "{mismatch}"),
            Self::NotConnected => formatter.write_str("client is not connected"),
            Self::SendFailed(detail) => write!(formatter, "failed to send frame: {detail}"),
            Self::MessageTooLarge { len, max } => {
                write!(
                    formatter,
                    "frame of {len} bytes exceeds the {max} byte limit"
                )
            }
            Self::NoSession => formatter.write_str("no resume session stored"),
            Self::Protocol { error, .. } => write!(formatter, "invalid server message: {error}"),
            Self::Closed => formatter.write_str("client is closed"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hello(mismatch) => Some(mismatch),
            Self::Protocol { error, .. } => Some(error),
            _ => None,
        }
    }
}
