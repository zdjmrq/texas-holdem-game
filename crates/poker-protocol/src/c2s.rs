//! Client-to-server protocol v2 messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{ClientEnvelope, RequestId};
use crate::config::RoomOptionsInput;
use crate::constants::PROTOCOL_VERSION;
use crate::error::ClientParseError;
use crate::js;

/// The message types the reference server dispatches.
pub const CLIENT_MESSAGE_TYPES: [&str; 8] = [
    "create_room",
    "join_room",
    "leave_room",
    "start_game",
    "player_action",
    "ready_for_next",
    "resume_session",
    "get_snapshot",
];

/// Parses one client-to-server message from JSON text.
///
/// Applies the reference server's validation order and error texts: JSON
/// syntax, "type" presence/shape, protocol-version announcement, known type.
/// Shape errors inside a known type are reported as InvalidShape (the JS server
/// would coerce undefined fields instead; see the crate docs).
pub fn parse_client_message(input: &str) -> Result<ClientMessage, ClientParseError> {
    let value: Value = serde_json::from_str(input).map_err(|_| ClientParseError::InvalidJson)?;
    parse_client_message_value(value)
}

/// Parses one client-to-server message from an already-parsed JSON value.
pub fn parse_client_message_value(value: Value) -> Result<ClientMessage, ClientParseError> {
    let request_id = value.get("requestId").and_then(RequestId::from_wire);
    let Some(message_type) = value.get("type").and_then(Value::as_str).map(str::to_owned) else {
        return Err(ClientParseError::MissingType { request_id });
    };
    if let Some(announced) = value.get("protocolVersion") {
        if js::number(Some(announced)) != Some(f64::from(PROTOCOL_VERSION)) {
            return Err(ClientParseError::ProtocolMismatch { request_id });
        }
    }
    if !is_known_client_type(&message_type) {
        return Err(ClientParseError::UnknownType {
            request_id,
            message_type,
        });
    }
    serde_json::from_value(value).map_err(|error| ClientParseError::InvalidShape {
        request_id,
        message_type,
        detail: error.to_string(),
    })
}

/// True when the type token belongs to protocol v2 client messages.
pub fn is_known_client_type(message_type: &str) -> bool {
    CLIENT_MESSAGE_TYPES.contains(&message_type)
}

/// A client-to-server protocol v2 message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Create a room (the server picks the first free room code).
    CreateRoom(CreateRoom),
    /// Join an existing room, creating it when the code is free.
    JoinRoom(JoinRoom),
    /// Leave the current room.
    LeaveRoom(LeaveRoom),
    /// Host-only: fill empty seats with AI and deal the first hand.
    StartGame(StartGame),
    /// Submit a betting action for the current decision.
    PlayerAction(PlayerAction),
    /// Signal readiness for the next hand after a settlement.
    ReadyForNext(ReadyForNext),
    /// Re-attach a dropped connection with a resume token.
    ResumeSession(ResumeSession),
    /// Ask for an authoritative resync (undocumented in the UI but supported by
    /// the reference server).
    GetSnapshot(GetSnapshot),
}

impl ClientMessage {
    /// Wire type token.
    pub fn message_type(&self) -> &'static str {
        match self {
            Self::CreateRoom(_) => "create_room",
            Self::JoinRoom(_) => "join_room",
            Self::LeaveRoom(_) => "leave_room",
            Self::StartGame(_) => "start_game",
            Self::PlayerAction(_) => "player_action",
            Self::ReadyForNext(_) => "ready_for_next",
            Self::ResumeSession(_) => "resume_session",
            Self::GetSnapshot(_) => "get_snapshot",
        }
    }

    /// Optional envelope fields (protocol announcement and request id).
    pub fn envelope(&self) -> &ClientEnvelope {
        match self {
            Self::CreateRoom(message) => &message.envelope,
            Self::JoinRoom(message) => &message.envelope,
            Self::LeaveRoom(message) => &message.envelope,
            Self::StartGame(message) => &message.envelope,
            Self::PlayerAction(message) => &message.envelope,
            Self::ReadyForNext(message) => &message.envelope,
            Self::ResumeSession(message) => &message.envelope,
            Self::GetSnapshot(message) => &message.envelope,
        }
    }

    /// Request id of this message, when present.
    pub fn request_id(&self) -> Option<&RequestId> {
        self.envelope().request_id.as_ref()
    }

    /// Serialises to a wire JSON value, announcing protocol version 2 when the
    /// parsed message did not carry one.
    pub fn to_json_value(&self) -> Value {
        let mut value =
            serde_json::to_value(self).expect("client message serialisation cannot fail");
        if let Value::Object(map) = &mut value {
            map.entry("protocolVersion")
                .or_insert(Value::from(PROTOCOL_VERSION));
        }
        value
    }

    /// Serialises to a wire JSON string.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_json_value())
            .expect("client message serialisation cannot fail")
    }
}

/// create_room payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CreateRoom {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Requested player name (normalised server-side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Requested room options (sanitised server-side).
    #[serde(flatten)]
    pub options: RoomOptionsInput,
}

/// join_room payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JoinRoom {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code to join (empty/absent when the caller wants a room_created).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
    /// Requested player name (normalised server-side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Room options used only when this join creates the room.
    #[serde(flatten)]
    pub options: RoomOptionsInput,
}

/// leave_room payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LeaveRoom {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code (informational; the connection's room is authoritative).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
}

/// start_game payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StartGame {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code (informational; the connection's room is authoritative).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
}

/// player_action payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerAction {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code the client believes it is in (stale messages are rejected).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
    /// Hand id of the decision the client is answering.
    #[serde(rename = "handId", default, skip_serializing_if = "Option::is_none")]
    pub hand_id: Option<i64>,
    /// Decision (turn) id of the decision the client is answering.
    #[serde(rename = "turnId", default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<i64>,
    /// Last stateVersion the client saw.
    #[serde(
        rename = "expectedStateVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_state_version: Option<i64>,
    /// Action token (the server lowercases it before validation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Raise-to amount for raises; the client sends 0 otherwise.
    #[serde(default)]
    pub amount: i64,
}

/// ready_for_next payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReadyForNext {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code (informational; the connection's room is authoritative).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
    /// Hand id the client is signalling readiness for.
    #[serde(rename = "handId", default, skip_serializing_if = "Option::is_none")]
    pub hand_id: Option<i64>,
}

/// resume_session payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumeSession {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
    /// Room code the client believes it is in (informational).
    #[serde(rename = "roomCode", default, skip_serializing_if = "Option::is_none")]
    pub room_code: Option<String>,
    /// Resume token issued by room_created / room_joined.
    #[serde(
        rename = "resumeToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub resume_token: Option<String>,
}

/// get_snapshot payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GetSnapshot {
    /// Optional envelope fields.
    #[serde(flatten)]
    pub envelope: ClientEnvelope,
}
