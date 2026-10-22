//! DTOs shared by both directions of the v2 protocol.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cards::RevealedCard;
use crate::error::ErrorCode;

/// Echoed request identifier.
///
/// The reference server treats the request id as opaque JSON and echoes it back
/// verbatim; the browser client always sends a string, but the DTO keeps the
/// raw value so a Rust server can reproduce the JS echo semantics exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub Value);

impl RequestId {
    /// Wraps a raw JSON value.
    pub fn new(value: impl Into<Value>) -> Self {
        Self(value.into())
    }

    /// Returns the string view for the normal browser-client shape.
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// Returns the raw JSON value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// JavaScript truthiness, matching the server's "requestId || null" echo.
    pub fn is_truthy(&self) -> bool {
        crate::js::truthy(&self.0)
    }

    pub(crate) fn from_wire(value: &Value) -> Option<Self> {
        crate::js::truthy(value).then(|| Self(value.clone()))
    }
}

impl From<&str> for RequestId {
    fn from(value: &str) -> Self {
        Self(Value::String(value.to_owned()))
    }
}

impl From<String> for RequestId {
    fn from(value: String) -> Self {
        Self(Value::String(value))
    }
}

/// Optional envelope fields shared by every client-to-server message.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientEnvelope {
    /// "protocolVersion": the client announces the protocol; the server accepts
    /// any value whose JavaScript Number() conversion equals 2 and rejects
    /// everything else with PROTOCOL_MISMATCH.
    #[serde(
        rename = "protocolVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub protocol_version: Option<Value>,
    /// Echoed by ack/action_ack/error. Optional because a malformed or hostile
    /// client message can omit it (the reference server then answers with
    /// requestId: null).
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
}

impl ClientEnvelope {
    /// Builds an envelope carrying the announced protocol version (2) and no
    /// request id.
    pub fn new() -> Self {
        Self {
            protocol_version: Some(Value::from(crate::constants::PROTOCOL_VERSION)),
            request_id: None,
        }
    }

    /// Builds an envelope with the announced protocol version and a request id.
    pub fn with_request_id(request_id: impl Into<RequestId>) -> Self {
        Self {
            protocol_version: Some(Value::from(crate::constants::PROTOCOL_VERSION)),
            request_id: Some(request_id.into()),
        }
    }
}

/// Action vocabulary used by legalActions and player_action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    /// fold
    Fold,
    /// check
    Check,
    /// call
    Call,
    /// raise
    Raise,
    /// allin (all-in; the reference wire token has no underscore)
    Allin,
}

impl ActionType {
    /// Wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fold => "fold",
            Self::Check => "check",
            Self::Call => "call",
            Self::Raise => "raise",
            Self::Allin => "allin",
        }
    }

    /// Lowercases like the reference server (String(action).toLowerCase()) and
    /// maps the legal vocabulary; unknown strings yield None.
    pub fn parse_js(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "fold" => Some(Self::Fold),
            "check" => Some(Self::Check),
            "call" => Some(Self::Call),
            "raise" => Some(Self::Raise),
            "allin" => Some(Self::Allin),
            _ => None,
        }
    }
}

/// Betting round / table phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// preflop
    Preflop,
    /// flop
    Flop,
    /// turn
    Turn,
    /// river
    River,
    /// showdown
    Showdown,
    /// idle
    Idle,
}

impl Phase {
    /// Wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preflop => "preflop",
            Self::Flop => "flop",
            Self::Turn => "turn",
            Self::River => "river",
            Self::Showdown => "showdown",
            Self::Idle => "idle",
        }
    }
}

/// Why the last hand ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandEndReason {
    /// showdown
    Showdown,
    /// fold
    Fold,
}

impl HandEndReason {
    /// Wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Showdown => "showdown",
            Self::Fold => "fold",
        }
    }
}

/// A recorded player action as broadcast in players[].lastAction.
///
/// The action token is a free string, not the legal-action enum: the reference
/// server records forced-bet labels too ("small blind", "big blind", "ante",
/// "dealer ante") alongside the legal actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerActionRecord {
    /// Action token, e.g. "fold", "check", "call", "raise", "allin",
    /// "small blind", "big blind", "ante", "dealer ante".
    pub action: String,
    /// Chips committed by this action (0 for fold/check).
    pub amount: i64,
}

/// Public per-player view included in lobby and game messages.
///
/// Opponents' hole cards are never part of this view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicPlayer {
    /// Stable player id (UUID in the reference server).
    pub id: String,
    /// Seat index in the room's sorted player list.
    pub seat_id: i64,
    /// Normalised player name.
    pub name: String,
    /// Avatar string (the reference server uses a single emoji).
    pub avatar: String,
    /// True for human players, false for server AI seats.
    pub is_human: bool,
    /// WebSocket connection state (false during the reconnect grace window).
    pub connected: bool,
    /// Remaining chips.
    pub stack: i64,
    /// Chips committed to the pot in the current hand.
    pub chips_in_pot: i64,
    /// Chips committed in the current betting round.
    pub round_bet: i64,
    /// True once the player folded.
    pub folded: bool,
    /// True once the player is all-in.
    pub is_all_in: bool,
    /// Last recorded action, or null before the player acted.
    pub last_action: Option<PlayerActionRecord>,
}

/// Seat identity used inside lastHandResult winners/results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIdentity {
    /// Player id.
    pub id: String,
    /// Seat index.
    pub seat_id: i64,
    /// Normalised player name.
    pub name: String,
    /// True for human players.
    pub is_human: bool,
}

/// Authoritative room configuration produced by the config whitelist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomConfig {
    /// Seat count, clamped to 2..=10 (2..=8 for short deck).
    pub max_players: i64,
    /// 6+ Hold'em variant flag.
    pub is_short_deck: bool,
    /// Starting stack; one of the stack levels.
    pub starting_stack: i64,
    /// Small blind; one of the blind levels.
    pub small_blind: i64,
    /// Big blind; one of the blind levels.
    pub big_blind: i64,
    /// Ante for short deck; mirrors small_blind.
    pub ante: i64,
    /// Minimum bet for short deck; mirrors big_blind.
    pub min_bet: i64,
    /// AI action delay in milliseconds.
    pub ai_delay: i64,
}

/// Legal action snapshot for the player whose turn it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegalActions {
    /// Legal action vocabulary for this decision (empty when it is not your
    /// turn).
    pub actions: Vec<ActionType>,
    /// Chips required to call, already clamped to the player's stack.
    pub to_call: i64,
    /// Whether a raise is currently allowed.
    pub can_raise: bool,
    /// Smallest legal raise-to amount (grid aligned unless it is an all-in).
    pub min_raise_to: i64,
    /// Largest legal raise-to amount (own bet + remaining stack).
    pub max_raise_to: i64,
    /// Raise grid unit (small blind for standard, ante for short deck).
    ///
    /// Absent when it is not the recipient's turn: the reference server's
    /// legalActions() early-return path omits the field entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wager_unit: Option<i64>,
}

/// Readiness entry of the ready_status message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyEntry {
    /// Player id.
    pub id: String,
    /// Ready for the next hand flag.
    pub ready: bool,
}

/// A revealed hand-result row (one per player dealt into the hand).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandResultEntry {
    /// Player id.
    pub id: String,
    /// Seat index.
    pub seat_id: i64,
    /// Normalised player name.
    pub name: String,
    /// True for human players.
    pub is_human: bool,
    /// Revealed hole cards (empty for fold endings).
    pub cards: Vec<RevealedCard>,
    /// Hand rank category index.
    pub hand_rank: i64,
}

/// Settlement payload of the hand that just ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastHandResult {
    /// "showdown" or "fold".
    pub reason: HandEndReason,
    /// Variant flag, copied from the game.
    pub is_short_deck: bool,
    /// Pot that was settled (before the awards).
    pub pot: i64,
    /// Strongest hand rank in the showdown (0 for fold endings).
    pub hand_rank: i64,
    /// Winning seats in left-of-dealer order.
    pub winners: Vec<PublicIdentity>,
    /// Awarded chips per winner id.
    pub winner_amounts: BTreeMap<String, i64>,
    /// One row per player dealt into the hand.
    pub results: Vec<HandResultEntry>,
}

/// The error payload of the server-to-client "error" message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorMessage {
    /// Always 2 on the wire.
    #[serde(
        rename = "protocolVersion",
        default = "crate::constants::default_protocol_version"
    )]
    pub protocol_version: u8,
    /// Echoed request id, or null for protocol-level failures.
    #[serde(default)]
    pub request_id: Option<RequestId>,
    /// Machine-readable error code.
    pub code: ErrorCode,
    /// Human-readable (Chinese) error text, exactly as the JS server sends it.
    pub message: String,
}

impl ErrorMessage {
    /// Builds an error payload with the protocol version envelope field set.
    pub fn new(request_id: Option<RequestId>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: crate::constants::PROTOCOL_VERSION,
            request_id,
            code,
            message: message.into(),
        }
    }
}
