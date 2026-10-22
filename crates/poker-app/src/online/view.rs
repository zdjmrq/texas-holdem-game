//! Pure online view state machine.
//!
//! The view reduces worker events (poker-client lifecycle plus command
//! receipts) into the data the online screens render. It has no I/O and no
//! egui dependency, so it is unit-testable (docs/specs/online-ui-plan.md
//! section 7.1).

use std::time::Instant;

use poker_client::{ActionRequest, ClientEvent, SessionCredentials};
use poker_protocol::ErrorCode;
use poker_protocol::{
    GameState, Hello, LastHandResult, Phase, PublicPlayer, ReadyStatus, RoomInfo, ServerMessage,
};

use super::commands::OnlineAction;
use super::events::{CommandReceipt, PendingKind, WorkerEvent};
use super::mapping;
use super::text;

/// Which online screen is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnlineStage {
    #[default]
    Connect,
    Lobby,
    Table,
}

/// Connection status shown in headers and banners.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnStatus {
    #[default]
    Idle,
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
    },
    Closed,
}

impl ConnStatus {
    /// Label used by the online header/status bar.
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "未连接".to_owned(),
            Self::Connecting => "连接中…".to_owned(),
            Self::Connected => "已连接".to_owned(),
            Self::Reconnecting { attempt, .. } => text::reconnecting_attempt(attempt + 1),
            Self::Closed => "已断开".to_owned(),
        }
    }

    /// True only for an established connection.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// An in-flight request written to the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAction {
    pub request_id: String,
    pub kind: PendingKind,
    pub hand_id: i64,
    pub turn_id: i64,
    pub expected_state_version: i64,
    pub sent_at: Instant,
}

/// Side effects the app shell must apply (audio, attention, hints).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineEffect {
    PlayTurnPrompt,
    PlaySettlementPrompt,
    RequestAttention,
    Hint(String),
}

/// Everything the online screens render.
#[derive(Debug, Clone, Default)]
pub struct OnlineView {
    pub stage: OnlineStage,
    pub status: ConnStatus,
    pub server_url: Option<String>,
    pub local_server_url: Option<String>,
    pub hello: Option<Hello>,
    pub session: Option<SessionCredentials>,
    pub room: Option<RoomInfo>,
    pub players: Vec<PublicPlayer>,
    pub is_host: bool,
    pub host_id: Option<String>,
    pub game: Option<GameState>,
    pub last_state_version: i64,
    pub last_hand_id: i64,
    pub pending: Option<PendingAction>,
    pub ready: Option<ReadyStatus>,
    pub last_result: Option<LastHandResult>,
    pub settlement_open: bool,
    pub status_line: String,
    pub error_line: Option<String>,
    pub banner: Option<String>,
    pub kicked: bool,
    pub protocol_version_mismatch: bool,
    pub my_player_id: Option<String>,
    turn_prompt_key: Option<(String, i64, i64)>,
    settlement_prompt_key: Option<(String, i64)>,
}

impl OnlineView {
    /// Creates an empty view on the connect screen.
    pub fn new(server_url: Option<String>) -> Self {
        Self {
            server_url,
            stage: OnlineStage::Connect,
            ..Self::default()
        }
    }

    /// Stores the embedded server URL reported by the app shell.
    pub fn set_server_url(&mut self, url: String) {
        self.server_url = Some(url);
    }

    /// Current server URL, if known.
    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    /// The recipient player row of the latest snapshot.
    pub fn my_player(&self) -> Option<&PublicPlayer> {
        self.game
            .as_ref()
            .and_then(|game| mapping::my_player(game, self.my_player_id.as_deref()))
    }

    /// Room code of the joined room.
    pub fn room_code(&self) -> Option<&str> {
        self.room.as_ref().map(|room| room.room_code.as_str())
    }

    /// True when it is the recipient's decision and no request is pending.
    pub fn can_act(&self) -> bool {
        self.status.is_connected()
            && self.pending.is_none()
            && self
                .game
                .as_ref()
                .is_some_and(|game| game.is_your_turn && !game.legal_actions.actions.is_empty())
    }

    /// Chips needed to call in the current snapshot.
    pub fn to_call(&self) -> u64 {
        self.game
            .as_ref()
            .map_or(0, |game| mapping::chips(game.legal_actions.to_call))
    }

    /// Recipient stack in the current snapshot.
    pub fn my_stack(&self) -> u64 {
        self.game
            .as_ref()
            .map_or(0, |game| mapping::chips(game.your_stack))
    }

    /// Current hand id (0 before the first snapshot).
    pub fn hand_id(&self) -> i64 {
        self.game.as_ref().map_or(0, |game| game.hand_id)
    }

    /// Current turn/decision id.
    pub fn turn_id(&self) -> i64 {
        self.game.as_ref().map_or(0, |game| game.turn_id)
    }

    /// Settlement overlay model, if the latest snapshot carries a result.
    pub fn settlement(&self) -> Option<mapping::SettlementView> {
        let result = self.last_result.as_ref()?;
        let board = self
            .game
            .as_ref()
            .map_or(&[][..], |game| game.community_cards.as_slice());
        Some(mapping::settlement_view(
            u64::try_from(self.hand_id()).unwrap_or(0),
            result,
            board,
        ))
    }

    /// "Next hand" button label for the settlement overlay (ui.md section 5.6).
    pub fn next_hand_label(&self) -> String {
        if let Some(ready) = &self.ready {
            if ready.all_ready {
                return text::STARTING_NEXT_HAND.to_owned();
            }
            let me_ready = ready.players.iter().any(|player| {
                self.my_player_id
                    .as_deref()
                    .is_some_and(|id| player.id == id && player.ready)
            });
            if me_ready {
                return text::waiting_ready(u32::try_from(ready.waiting_for).unwrap_or(0));
            }
        }
        text::NEXT_HAND.to_owned()
    }

    /// Builds the player_action request for the current snapshot.
    pub fn action_request(&self, action: OnlineAction) -> Option<ActionRequest> {
        let game = self.game.as_ref()?;
        let (wire_action, amount) = mapping::wire_action(action, self.to_call(), self.my_stack());
        let room_code = game.room_code.clone();
        Some(
            ActionRequest::new(
                room_code,
                game.hand_id,
                game.turn_id,
                game.state_version,
                wire_action,
            )
            .with_amount(amount),
        )
    }

    /// One-line status for the app shell/status bar.
    pub fn status_text(&self) -> String {
        self.error_line
            .clone()
            .unwrap_or_else(|| self.status.label())
    }

    /// Reduces one worker event into the view, returning side effects.
    pub fn reduce(&mut self, event: WorkerEvent, now: Instant) -> Vec<OnlineEffect> {
        let mut effects = Vec::new();
        match event {
            WorkerEvent::LocalServerUrl(url) => {
                self.local_server_url = Some(url.clone());
                let _ = self.server_url.get_or_insert(url);
            }
            WorkerEvent::ConnectFailed(error) => {
                self.status = ConnStatus::Closed;
                self.stage = OnlineStage::Connect;
                self.status_line = text::connect_error(&error);
            }
            WorkerEvent::CommandSent(receipt) => self.begin_command(receipt, now),
            WorkerEvent::CommandFailed { message, .. } => {
                self.error_line = Some(message);
            }
            WorkerEvent::WorkerClosed => {
                self.status = ConnStatus::Closed;
            }
            WorkerEvent::Client(event) => self.reduce_client(event, &mut effects),
        }
        effects
    }

    fn reduce_client(&mut self, event: ClientEvent, effects: &mut Vec<OnlineEffect>) {
        match event {
            ClientEvent::Connected { reconnecting } => {
                self.status = ConnStatus::Connected;
                self.banner = reconnecting.then(|| text::RECONNECTED_RESUMING.to_owned());
                if reconnecting {
                    self.error_line = None;
                }
            }
            ClientEvent::Hello(hello) => {
                self.hello = Some(hello);
            }
            ClientEvent::Message(message) => self.apply_message(message, effects),
            ClientEvent::Disconnected {
                code,
                reason,
                will_reconnect,
            } => {
                if will_reconnect {
                    self.status = ConnStatus::Reconnecting {
                        attempt: 0,
                        delay_ms: 0,
                    };
                    self.banner = Some(reason);
                } else {
                    self.status = ConnStatus::Closed;
                    self.stage = OnlineStage::Connect;
                    if code == Some(4001) {
                        self.kicked = true;
                        self.session = None;
                    }
                    self.status_line = text::close_notice(code);
                    self.clear_room();
                }
            }
            ClientEvent::Reconnecting { attempt, delay_ms } => {
                self.status = ConnStatus::Reconnecting { attempt, delay_ms };
                self.banner = Some(text::link_lost(delay_ms.div_ceil(1000).max(1)));
            }
            ClientEvent::ReconnectFailed {
                attempts,
                last_error,
            } => {
                self.status = ConnStatus::Closed;
                self.stage = OnlineStage::Connect;
                self.status_line = text::RECONNECT_FAILED.to_owned();
                self.error_line = Some(format!("{last_error}（{attempts} 次）"));
                self.clear_room();
            }
            ClientEvent::ProtocolError { raw, error } => {
                self.error_line = Some(format!("协议错误：{error}（{raw}）"));
            }
            ClientEvent::Closed => {
                self.status = ConnStatus::Closed;
                if self.stage != OnlineStage::Connect {
                    self.stage = OnlineStage::Connect;
                    self.clear_room();
                }
            }
        }
    }

    fn apply_message(&mut self, message: ServerMessage, effects: &mut Vec<OnlineEffect>) {
        match message {
            ServerMessage::RoomCreated(room) => self.apply_room_info(room, false),
            ServerMessage::RoomJoined(room) => {
                let resumed = room.resumed;
                self.apply_room_info(room, resumed);
            }
            ServerMessage::PlayerJoined(update) | ServerMessage::PlayerLeft(update) => {
                self.players = update.players;
                self.host_id = update.host_id;
                self.is_host = self
                    .my_player_id
                    .as_deref()
                    .is_some_and(|id| self.host_id.as_deref() == Some(id));
            }
            ServerMessage::HostChanged(changed) => {
                self.host_id = changed.host_id;
                self.is_host = changed.is_host;
                self.players = changed.players;
                self.banner = Some(if changed.is_host {
                    text::HOST_CHANGED_YOU.to_owned()
                } else {
                    text::HOST_CHANGED.to_owned()
                });
            }
            ServerMessage::GameStarting(starting) => {
                self.stage = OnlineStage::Table;
                self.banner = Some(text::game_starting(
                    starting.player_count,
                    starting.human_count,
                    starting.ai_count,
                ));
            }
            ServerMessage::GameState(state) => self.apply_game_state(state, effects),
            ServerMessage::ActionAck(ack) => {
                let request_id = mapping::request_id_key(&ack.request_id);
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.request_id == request_id)
                {
                    self.pending = None;
                }
                if ack.accepted {
                    self.status_line = text::ACTION_ACK_OK.to_owned();
                } else {
                    self.error_line = Some(ack.message);
                }
            }
            ServerMessage::ReadyStatus(ready) => {
                if ready.all_ready {
                    self.settlement_open = false;
                    self.status_line = text::ALL_READY.to_owned();
                }
                self.ready = Some(ready);
            }
            ServerMessage::Ack(ack) => {
                let request_id = mapping::request_id_key(&ack.request_id);
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.request_id == request_id)
                {
                    self.pending = None;
                }
            }
            ServerMessage::Error(error) => self.apply_error(error),
            ServerMessage::Hello(hello) => self.hello = Some(hello),
        }
    }

    fn apply_room_info(&mut self, room: RoomInfo, resumed: bool) {
        self.pending = None;
        self.error_line = None;
        self.players = room.players.clone();
        self.is_host = room.is_host;
        self.host_id = room.host_id.clone();
        self.status_line = text::room_joined(&room.room_code, room.is_host);
        let server_url = self.server_url.clone().unwrap_or_default();
        self.session = Some(SessionCredentials::new(
            server_url,
            room.room_code.clone(),
            room.resume_token.clone(),
        ));
        self.my_player_id = Some(room.player_id.clone());
        self.room = Some(room);
        if resumed {
            self.banner = None;
        }
        if self.stage != OnlineStage::Table {
            self.stage = OnlineStage::Lobby;
        }
    }

    fn apply_game_state(&mut self, state: GameState, effects: &mut Vec<OnlineEffect>) {
        if self.game.is_some()
            && (state.state_version <= self.last_state_version || state.hand_id < self.last_hand_id)
        {
            return;
        }
        let pending_cleared = self
            .pending
            .as_ref()
            .is_some_and(|pending| state.state_version > pending.expected_state_version);
        if pending_cleared {
            self.pending = None;
        }
        if self
            .game
            .as_ref()
            .is_some_and(|previous| previous.hand_id != state.hand_id)
        {
            self.ready = None;
        }
        self.last_state_version = state.state_version;
        self.last_hand_id = state.hand_id;
        self.players = state.players.clone();
        self.host_id = state.host_id.clone();
        self.is_host = state.is_host;
        self.stage = OnlineStage::Table;
        self.banner = None;
        self.last_result = state.last_hand_result.clone();
        self.settlement_open = state.phase == Phase::Idle && state.last_hand_result.is_some();
        self.game = Some(state);

        if self.can_act() {
            let key = (
                self.room_code().unwrap_or_default().to_owned(),
                self.hand_id(),
                self.turn_id(),
            );
            if self.turn_prompt_key.as_ref() != Some(&key) {
                self.turn_prompt_key = Some(key);
                effects.push(OnlineEffect::PlayTurnPrompt);
                effects.push(OnlineEffect::RequestAttention);
            }
        }
        if self.settlement_open {
            let key = (
                self.room_code().unwrap_or_default().to_owned(),
                self.hand_id(),
            );
            if self.settlement_prompt_key.as_ref() != Some(&key) {
                self.settlement_prompt_key = Some(key);
                effects.push(OnlineEffect::PlaySettlementPrompt);
            }
        }
    }

    fn apply_error(&mut self, error: poker_protocol::ErrorMessage) {
        match error.code {
            ErrorCode::ResumeExpired => {
                self.session = None;
                self.stage = OnlineStage::Connect;
                self.banner = Some(text::RESUME_EXPIRED_HINT.to_owned());
                self.status_line = error.message;
                self.clear_room();
            }
            ErrorCode::ProtocolMismatch => {
                self.protocol_version_mismatch = true;
                self.stage = OnlineStage::Connect;
                self.status_line = error.message;
                self.clear_room();
            }
            _ => {
                self.error_line = Some(error.message);
            }
        }
    }

    fn begin_command(&mut self, receipt: CommandReceipt, now: Instant) {
        self.pending = Some(PendingAction {
            request_id: receipt.request_id,
            kind: receipt.kind,
            hand_id: receipt.hand_id,
            turn_id: receipt.turn_id,
            expected_state_version: receipt.expected_state_version,
            sent_at: now,
        });
    }

    fn clear_room(&mut self) {
        self.room = None;
        self.players.clear();
        self.game = None;
        self.ready = None;
        self.last_result = None;
        self.settlement_open = false;
        self.last_state_version = 0;
        self.last_hand_id = 0;
        self.pending = None;
        self.my_player_id = None;
        self.turn_prompt_key = None;
        self.settlement_prompt_key = None;
    }
}
