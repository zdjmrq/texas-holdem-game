//! Server-to-client protocol v2 messages.
//!
//! Every server message carries "protocolVersion": 2 (the reference server
//! prepends it in safeSend()). The DTOs expose the field explicitly so that
//! serialisation matches the reference wire format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cards::CardDto;
use crate::common::{
    ErrorMessage, LastHandResult, LegalActions, PublicPlayer, ReadyEntry, RequestId, RoomConfig,
};

/// The message types protocol v2 defines in the server-to-client direction.
pub const SERVER_MESSAGE_TYPES: [&str; 12] = [
    "hello",
    "room_created",
    "room_joined",
    "player_joined",
    "player_left",
    "host_changed",
    "game_starting",
    "game_state",
    "action_ack",
    "ready_status",
    "ack",
    "error",
];

/// A server-to-client protocol v2 message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // payloads are short-lived protocol values
pub enum ServerMessage {
    /// Sent immediately after the WebSocket opens; also the desktop service
    /// discovery payload.
    Hello(Hello),
    /// Sent to the client that created a room.
    RoomCreated(RoomInfo),
    /// Sent to a client that joined a room, or re-sent on resume/get_snapshot
    /// (resumed = true).
    RoomJoined(RoomInfo),
    /// Broadcast to the room when a player joins.
    PlayerJoined(LobbyUpdate),
    /// Broadcast to the room when a player leaves or its grace window expires.
    PlayerLeft(LobbyUpdate),
    /// Broadcast to the room when the host role moves.
    HostChanged(HostChanged),
    /// Sent to every connected human when the game starts.
    GameStarting(GameStarting),
    /// Authoritative game snapshot, personalised per recipient.
    GameState(GameState),
    /// Result of a player_action request (also replayed from the dedupe cache
    /// for duplicate request ids).
    ActionAck(ActionAck),
    /// Readiness snapshot after a settlement.
    ReadyStatus(ReadyStatus),
    /// Generic request acknowledgement (start_game, ready_for_next, leave_room).
    Ack(Ack),
    /// Request failure.
    Error(ErrorMessage),
}

impl ServerMessage {
    /// True when the type token belongs to protocol v2 server messages.
    pub fn is_known_type(message_type: &str) -> bool {
        SERVER_MESSAGE_TYPES.contains(&message_type)
    }

    /// Wire type token.
    pub fn message_type(&self) -> &'static str {
        match self {
            Self::Hello(_) => "hello",
            Self::RoomCreated(_) => "room_created",
            Self::RoomJoined(_) => "room_joined",
            Self::PlayerJoined(_) => "player_joined",
            Self::PlayerLeft(_) => "player_left",
            Self::HostChanged(_) => "host_changed",
            Self::GameStarting(_) => "game_starting",
            Self::GameState(_) => "game_state",
            Self::ActionAck(_) => "action_ack",
            Self::ReadyStatus(_) => "ready_status",
            Self::Ack(_) => "ack",
            Self::Error(_) => "error",
        }
    }

    /// Serialises to a wire JSON value.
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).expect("server message serialisation cannot fail")
    }

    /// Serialises to a wire JSON string.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_json_value())
            .expect("server message serialisation cannot fail")
    }
}

/// hello payload: service identity and version handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Service id, "texas-holdem-game".
    pub server_id: String,
    /// Human-readable service name ("德州扑克服务").
    ///
    /// The reference server always sends it, but the desktop service probe (and
    /// therefore a minimal hello payload) does not require it, so the DTO keeps
    /// it optional. It is omitted when absent on serialisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// Application version; the old desktop probe requires an exact match with
    /// its own version (1.5.8 for the reference release).
    pub app_version: String,
}

impl Hello {
    /// Reproduces the reference desktop client's service probe predicate:
    /// type == hello (guaranteed by the DTO), matching serverId, protocol 2 and
    /// an exact appVersion match.
    pub fn is_desktop_probe_compatible(&self, expected_app_version: &str) -> bool {
        self.server_id == crate::constants::SERVER_ID
            && self.protocol_version == crate::constants::PROTOCOL_VERSION
            && self.app_version == expected_app_version
    }
}

/// room_created / room_joined payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomInfo {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code ("1".."5").
    pub room_code: String,
    /// Assigned player id.
    pub player_id: String,
    /// Assigned seat index.
    pub seat_id: i64,
    /// True when this player is the host.
    pub is_host: bool,
    /// Current host player id (null only before any human is present).
    pub host_id: Option<String>,
    /// Token used by resume_session.
    pub resume_token: String,
    /// True for room_joined replies that re-attach a previous session.
    pub resumed: bool,
    /// Authoritative room configuration.
    pub room_config: RoomConfig,
    /// Public lobby player list.
    pub players: Vec<PublicPlayer>,
}

/// player_joined / player_left payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyUpdate {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code.
    pub room_code: String,
    /// Current host player id.
    pub host_id: Option<String>,
    /// Authoritative room configuration.
    pub room_config: RoomConfig,
    /// Public lobby player list.
    pub players: Vec<PublicPlayer>,
}

/// host_changed payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostChanged {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code.
    pub room_code: String,
    /// New host player id (null when no human remains).
    pub host_id: Option<String>,
    /// True for the recipient that became the host.
    pub is_host: bool,
    /// Room state version after the change.
    pub state_version: i64,
    /// Public lobby player list.
    pub players: Vec<PublicPlayer>,
}

/// game_starting payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStarting {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code.
    pub room_code: String,
    /// Total seats in the game (humans plus filled AI seats).
    pub player_count: i64,
    /// Number of human seats.
    pub human_count: i64,
    /// Number of AI seats.
    pub ai_count: i64,
    /// Authoritative room configuration.
    pub room_config: RoomConfig,
}

/// game_state payload: the authoritative table snapshot.
///
/// It is personalised per recipient: "isYourTurn", "yourCards", "yourStack" and
/// "yourBet" describe the recipient only. Opponents' hole cards never appear.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code.
    pub room_code: String,
    /// Monotonic room state version; stale messages must be ignored by clients.
    pub state_version: i64,
    /// Hand counter (increments on every deal).
    pub hand_id: i64,
    /// Decision counter (increments on every state emission and hand end).
    pub turn_id: i64,
    /// Current host player id.
    pub host_id: Option<String>,
    /// True when the recipient is the host.
    pub is_host: bool,
    /// Authoritative room configuration.
    pub room_config: RoomConfig,
    /// 6+ Hold'em variant flag.
    pub is_short_deck: bool,
    /// Current betting round or "idle" after a settlement.
    pub phase: crate::common::Phase,
    /// Pot size across all betting rounds.
    pub pot: i64,
    /// Community cards (0, 3, 4 or 5).
    pub community_cards: Vec<CardDto>,
    /// Public player list, sorted by seat id.
    pub players: Vec<PublicPlayer>,
    /// Dealer seat index.
    pub dealer_pos: i64,
    /// Small-blind seat index (-1 in short deck, which uses antes).
    pub sb_idx: i64,
    /// Big-blind seat index (-1 in short deck, which uses antes).
    pub bb_idx: i64,
    /// Seat index of the player to act (-1 when the hand is over).
    pub current_player_idx: i64,
    /// Current bet every player must match this round.
    pub current_bet: i64,
    /// Size of the last full raise this round.
    pub last_raise: i64,
    /// True when it is the recipient's turn to act.
    pub is_your_turn: bool,
    /// The recipient's hole cards.
    pub your_cards: Vec<CardDto>,
    /// The recipient's remaining stack.
    pub your_stack: i64,
    /// The recipient's committed chips this round.
    pub your_bet: i64,
    /// Legal actions for the recipient (empty when it is not the recipient's
    /// turn).
    pub legal_actions: LegalActions,
    /// Settlement of the previous hand while the table is idle, otherwise null.
    pub last_hand_result: Option<LastHandResult>,
}

/// action_ack payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionAck {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Echoed request id (also the dedupe key for retried actions).
    pub request_id: RequestId,
    /// True when the action was applied.
    pub accepted: bool,
    /// "行动已确认" on success, the rejection reason otherwise.
    pub message: String,
    /// Room code.
    pub room_code: String,
    /// Hand id the action was applied to.
    pub hand_id: i64,
    /// Decision id after the action.
    pub turn_id: i64,
    /// Room state version after the action.
    pub state_version: i64,
}

/// ready_status payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyStatus {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Room code.
    pub room_code: String,
    /// Hand id of the settlement being acknowledged.
    pub hand_id: i64,
    /// True when every active human is ready and the next hand is scheduled.
    pub all_ready: bool,
    /// Number of active humans that are not ready yet.
    pub waiting_for: i64,
    /// Per-human readiness entries.
    pub players: Vec<ReadyEntry>,
}

/// ack payload: generic request acknowledgement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Echoed request id.
    pub request_id: RequestId,
    /// True when the request was accepted.
    pub accepted: bool,
    /// Room code, or null when the connection is not in a room.
    pub room_code: Option<String>,
    /// Room state version after the operation.
    pub state_version: i64,
    /// Current hand id (0 before the first deal).
    pub hand_id: i64,
}
