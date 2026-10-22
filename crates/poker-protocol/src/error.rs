//! Error codes, parse errors and server-message parsing.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::common::{ErrorMessage, RequestId};
use crate::constants::PROTOCOL_VERSION;
use crate::s2c::ServerMessage;

/// Wire token of the generic request error code.
pub const INVALID_REQUEST_CODE: &str = "INVALID_REQUEST";

/// Wire token of the protocol-version mismatch error code.
pub const PROTOCOL_MISMATCH_CODE: &str = "PROTOCOL_MISMATCH";

/// Wire token of the "room capacity full" error code.
pub const ROOM_LIMIT_CODE: &str = "ROOM_LIMIT";

/// Wire token of the "resume token no longer valid" error code.
pub const RESUME_EXPIRED_CODE: &str = "RESUME_EXPIRED";

/// Exact error text for an unparsable JSON payload.
pub const TEXT_INVALID_JSON: &str = "消息格式无效";

/// Exact error text for a payload without a string "type" field.
pub const TEXT_MISSING_TYPE: &str = "缺少消息类型";

/// Exact error text for an unknown message type.
pub const TEXT_UNKNOWN_TYPE: &str = "未知消息类型";

/// Prefix of the protocol-mismatch error text (the server version follows).
pub const TEXT_PROTOCOL_MISMATCH_PREFIX: &str = "协议版本不匹配（服务端 v";

/// Fallback text used by the reference server for unexpected handler failures.
pub const TEXT_REQUEST_FAILED: &str = "请求失败";

/// Builds the exact protocol-mismatch error text of the reference server.
pub fn protocol_mismatch_message() -> String {
    format!("{TEXT_PROTOCOL_MISMATCH_PREFIX}{PROTOCOL_VERSION}）")
}

/// Machine-readable error code carried by the "error" server message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Generic invalid request (the fallback code of the reference server).
    InvalidRequest,
    /// Client announced a protocol version other than 2.
    ProtocolMismatch,
    /// Room creation/join hit the room cap.
    RoomLimit,
    /// The resume token is unknown or its room is gone.
    ResumeExpired,
    /// A code this build does not know; preserved verbatim for forward
    /// compatibility.
    Unknown(String),
}

impl ErrorCode {
    /// Builds a code from its wire token.
    pub fn from_wire(token: &str) -> Self {
        match token {
            INVALID_REQUEST_CODE => Self::InvalidRequest,
            PROTOCOL_MISMATCH_CODE => Self::ProtocolMismatch,
            ROOM_LIMIT_CODE => Self::RoomLimit,
            RESUME_EXPIRED_CODE => Self::ResumeExpired,
            other => Self::Unknown(other.to_owned()),
        }
    }

    /// Wire token.
    pub fn as_str(&self) -> &str {
        match self {
            Self::InvalidRequest => INVALID_REQUEST_CODE,
            Self::ProtocolMismatch => PROTOCOL_MISMATCH_CODE,
            Self::RoomLimit => ROOM_LIMIT_CODE,
            Self::ResumeExpired => RESUME_EXPIRED_CODE,
            Self::Unknown(token) => token,
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        Ok(Self::from_wire(&token))
    }
}

/// Failure of client-to-server message parsing.
///
/// The first four variants map one-to-one to the reference server's validation
/// order (type check, protocol version check, dispatch) and error replies. The
/// last variant is the stricter typed-parser fallback for payloads that are not
/// valid protocol DTOs.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientParseError {
    /// The payload is not valid JSON.
    InvalidJson,
    /// The payload is not a JSON object with a string "type" field.
    MissingType {
        /// Request id echoed back (truthy values only), if the payload had one.
        request_id: Option<RequestId>,
    },
    /// The announced protocol version is not 2 (using JS Number() semantics).
    ProtocolMismatch {
        /// Request id echoed back, if any.
        request_id: Option<RequestId>,
    },
    /// The type string is not part of the protocol.
    UnknownType {
        /// Request id echoed back, if any.
        request_id: Option<RequestId>,
        /// The unknown type token.
        message_type: String,
    },
    /// The type is known but the payload does not match its DTO.
    InvalidShape {
        /// Request id echoed back, if any.
        request_id: Option<RequestId>,
        /// The known message type token.
        message_type: String,
        /// serde detail string (for logs, not sent to clients).
        detail: String,
    },
}

impl ClientParseError {
    /// Error code the server should answer with.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ProtocolMismatch { .. } => ErrorCode::ProtocolMismatch,
            Self::InvalidJson
            | Self::MissingType { .. }
            | Self::UnknownType { .. }
            | Self::InvalidShape { .. } => ErrorCode::InvalidRequest,
        }
    }

    /// Request id echoed back in the error message.
    pub fn request_id(&self) -> Option<&RequestId> {
        match self {
            Self::InvalidJson => None,
            Self::MissingType { request_id }
            | Self::ProtocolMismatch { request_id }
            | Self::UnknownType { request_id, .. }
            | Self::InvalidShape { request_id, .. } => request_id.as_ref(),
        }
    }

    /// Exact human-readable text of the reference server reply.
    pub fn message_text(&self) -> String {
        match self {
            Self::InvalidJson => TEXT_INVALID_JSON.to_owned(),
            Self::MissingType { .. } => TEXT_MISSING_TYPE.to_owned(),
            Self::ProtocolMismatch { .. } => protocol_mismatch_message(),
            Self::UnknownType { .. } => TEXT_UNKNOWN_TYPE.to_owned(),
            // The reference server never produces a parse-shape error: it
            // coerces undefined fields and answers from the handler. The Rust
            // server answers the generic JSON-format text instead.
            Self::InvalidShape { .. } => TEXT_INVALID_JSON.to_owned(),
        }
    }

    /// Builds the "error" DTO a server should send for this failure.
    pub fn to_server_error(&self) -> ErrorMessage {
        ErrorMessage::new(self.request_id().cloned(), self.code(), self.message_text())
    }
}

impl std::fmt::Display for ClientParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("invalid JSON payload"),
            Self::MissingType { .. } => formatter.write_str("missing message type"),
            Self::ProtocolMismatch { .. } => formatter.write_str("protocol version mismatch"),
            Self::UnknownType { message_type, .. } => {
                write!(formatter, "unknown message type: {message_type}")
            }
            Self::InvalidShape {
                message_type,
                detail,
                ..
            } => {
                write!(formatter, "invalid {message_type} payload: {detail}")
            }
        }
    }
}

impl std::error::Error for ClientParseError {}

/// Failure of server-to-client message parsing (used by Rust clients).
#[derive(Debug, Clone, PartialEq)]
pub enum ServerParseError {
    /// The payload is not valid JSON.
    InvalidJson,
    /// The payload is not a JSON object.
    NotAnObject,
    /// The payload has no string "type" field.
    MissingType,
    /// The message type is not part of protocol v2.
    UnknownType(String),
    /// The type is known but the payload does not match its DTO.
    InvalidShape {
        /// The known message type token.
        message_type: String,
        /// serde detail string.
        detail: String,
    },
}

impl std::fmt::Display for ServerParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("invalid JSON payload"),
            Self::NotAnObject => formatter.write_str("server message is not an object"),
            Self::MissingType => formatter.write_str("missing server message type"),
            Self::UnknownType(message_type) => {
                write!(formatter, "unknown server message type: {message_type}")
            }
            Self::InvalidShape {
                message_type,
                detail,
            } => {
                write!(formatter, "invalid {message_type} payload: {detail}")
            }
        }
    }
}

impl std::error::Error for ServerParseError {}

/// Parses one server-to-client message from JSON text.
pub fn parse_server_message(input: &str) -> Result<ServerMessage, ServerParseError> {
    let value: Value = serde_json::from_str(input).map_err(|_| ServerParseError::InvalidJson)?;
    parse_server_message_value(value)
}

/// Parses one server-to-client message from a JSON value.
pub fn parse_server_message_value(value: Value) -> Result<ServerMessage, ServerParseError> {
    let object = value.as_object().ok_or(ServerParseError::NotAnObject)?;
    let message_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ServerParseError::MissingType)?
        .to_owned();
    if !ServerMessage::is_known_type(&message_type) {
        return Err(ServerParseError::UnknownType(message_type));
    }
    serde_json::from_value(value).map_err(|error| ServerParseError::InvalidShape {
        message_type,
        detail: error.to_string(),
    })
}
