//! Connection lifecycle and message streaming.
//!
//! One Client instance owns one WebSocket connection at a time and exposes a
//! single ordered event stream plus serialized writes; this mirrors the
//! single-socket architecture of js/network.js.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use poker_protocol::c2s;
use poker_protocol::constants::{CLOSE_NORMAL, is_terminal_close_code, reconnect_delay_ms};
use poker_protocol::s2c::RoomInfo;
use poker_protocol::{
    ActionType, ClientEnvelope, ClientMessage, Hello, RequestId, RoomOptionsInput, ServerMessage,
    ServerParseError, parse_server_message,
};
use tokio_tungstenite::tungstenite::Message;

use crate::config::{ClientConfig, SessionCredentials};
use crate::error::ClientError;
use crate::event::ClientEvent;
use crate::request_id::{RequestIdGenerator, RequestKind};
use crate::transport::{self, WsStream, handshake_timeout_error};

/// A player_action request; the reference client sends all of these fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    /// Room code the client believes it is in.
    pub room_code: String,
    /// Hand id of the decision being answered.
    pub hand_id: i64,
    /// Turn/decision id of the decision being answered.
    pub turn_id: i64,
    /// Last stateVersion the caller observed.
    pub expected_state_version: i64,
    /// Chosen action.
    pub action: ActionType,
    /// Raise-to amount; 0 for non-raise actions.
    pub amount: i64,
}

impl ActionRequest {
    /// Builds an action request with amount 0 (fold/check/call/allin use the
    /// exact chip amount only for call/allin, see js/network.js).
    pub fn new(
        room_code: impl Into<String>,
        hand_id: i64,
        turn_id: i64,
        expected_state_version: i64,
        action: ActionType,
    ) -> Self {
        Self {
            room_code: room_code.into(),
            hand_id,
            turn_id,
            expected_state_version,
            action,
            amount: 0,
        }
    }

    /// Sets the raise-to amount.
    #[must_use]
    pub fn with_amount(mut self, amount: i64) -> Self {
        self.amount = amount;
        self
    }
}

/// The live WebSocket halves of a connection.
struct Connection {
    sink: SplitSink<WsStream, Message>,
    stream: SplitStream<WsStream>,
}

/// Connection state of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Socket open and hello handshake complete.
    Connected,
    /// Waiting to make another connection attempt.
    ReconnectPending { attempt: u32, delay_ms: u64 },
    /// No further events.
    Closed,
}

/// A protocol v2 WebSocket client (Rust equivalent of js/network.js).
///
/// See the crate documentation for the usage pattern and the exact mapping of
/// reference-client behaviours.
pub struct Client {
    config: ClientConfig,
    connection: Option<Connection>,
    state: State,
    hello: Option<Hello>,
    attempts: u32,
    ids: RequestIdGenerator,
    session: Option<SessionCredentials>,
    queue: VecDeque<ClientEvent>,
}

impl Client {
    /// Connects to the configured server and performs the hello handshake.
    ///
    /// If session credentials are already stored (for example restored from
    /// disk after an application restart), a resume_session frame is sent
    /// automatically before this function returns, matching the browser
    /// client's onopen behaviour.
    pub async fn connect(config: ClientConfig) -> Result<Self, ClientError> {
        let mut client = Self::new(config);
        client.establish(false).await?;
        Ok(client)
    }

    fn new(config: ClientConfig) -> Self {
        Self {
            config,
            connection: None,
            state: State::Closed,
            hello: None,
            attempts: 0,
            ids: RequestIdGenerator::new(),
            session: None,
            queue: VecDeque::new(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Returns the most recent hello frame, if any.
    pub fn hello(&self) -> Option<&Hello> {
        self.hello.as_ref()
    }

    /// True while a connection is open and the handshake succeeded.
    pub fn is_connected(&self) -> bool {
        matches!(self.state, State::Connected) && self.connection.is_some()
    }

    /// Returns the stored resume session, if any.
    pub fn session(&self) -> Option<&SessionCredentials> {
        self.session.as_ref()
    }

    /// Stores resume credentials used by the automatic reconnect loop.
    pub fn set_session(&mut self, session: SessionCredentials) {
        self.session = Some(session);
    }

    /// Removes the stored resume session and returns it.
    pub fn clear_session(&mut self) -> Option<SessionCredentials> {
        self.session.take()
    }

    /// Stores the room identity reported by room_created / room_joined.
    ///
    /// The caller remains responsible for persistence (the browser client
    /// writes sessionStorage); this helper only fills in the client-side
    /// session cache.
    pub fn remember_room(&mut self, room: &RoomInfo) -> SessionCredentials {
        let session = SessionCredentials {
            server_url: self.config.url.clone(),
            room_code: room.room_code.clone(),
            resume_token: room.resume_token.clone(),
            player_id: Some(room.player_id.clone()),
        };
        self.session = Some(session.clone());
        session
    }

    /// Generates the next request id for the given request kind.
    pub fn new_request_id(&mut self, kind: RequestKind) -> RequestId {
        let now = self.config.now_millis();
        self.ids.next(kind, now)
    }

    /// Writes one client message. Frames are written in call order.
    ///
    /// Fails with ClientError::NotConnected when no connection is open (the
    /// reference client send() returns false in that case).
    pub async fn send(&mut self, message: ClientMessage) -> Result<(), ClientError> {
        if !matches!(self.state, State::Connected) {
            return Err(ClientError::NotConnected);
        }
        let Some(connection) = self.connection.as_mut() else {
            return Err(ClientError::NotConnected);
        };
        let text = message.to_json_string();
        let max = self.config.max_frame_bytes;
        if text.len() > max {
            return Err(ClientError::MessageTooLarge {
                len: text.len(),
                max,
            });
        }
        let send_timeout = self.config.send_timeout;
        let result = match send_timeout {
            Some(timeout) => {
                tokio::time::timeout(timeout, connection.sink.send(Message::text(text)))
                    .await
                    .map_or_else(
                        |_| Err("timed out while writing frame".to_owned()),
                        |outcome| outcome.map_err(|error| error.to_string()),
                    )
            }
            None => connection
                .sink
                .send(Message::text(text))
                .await
                .map_err(|error| error.to_string()),
        };
        match result {
            Ok(()) => Ok(()),
            Err(detail) => {
                self.handle_connection_lost(None, format!("write failed: {detail}"));
                Err(ClientError::SendFailed(detail))
            }
        }
    }

    /// create_room: asks the server for a room (it picks the first free code).
    pub async fn create_room(
        &mut self,
        name: impl Into<String>,
        options: RoomOptionsInput,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Create);
        let message = ClientMessage::CreateRoom(c2s::CreateRoom {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            name: Some(name.into()),
            options,
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// join_room: joins (or creates, when the code is free) a room.
    pub async fn join_room(
        &mut self,
        room_code: impl Into<String>,
        name: impl Into<String>,
        options: RoomOptionsInput,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Join);
        let message = ClientMessage::JoinRoom(c2s::JoinRoom {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(room_code.into()),
            name: Some(name.into()),
            options,
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// leave_room.
    pub async fn leave_room(
        &mut self,
        room_code: impl Into<String>,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Leave);
        let message = ClientMessage::LeaveRoom(c2s::LeaveRoom {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(room_code.into()),
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// start_game (host only).
    pub async fn start_game(
        &mut self,
        room_code: impl Into<String>,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Start);
        let message = ClientMessage::StartGame(c2s::StartGame {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(room_code.into()),
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// player_action for the current decision.
    pub async fn player_action(
        &mut self,
        request: ActionRequest,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Action);
        let message = ClientMessage::PlayerAction(c2s::PlayerAction {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(request.room_code),
            hand_id: Some(request.hand_id),
            turn_id: Some(request.turn_id),
            expected_state_version: Some(request.expected_state_version),
            action: Some(request.action.as_str().to_owned()),
            amount: request.amount,
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// ready_for_next after a settlement.
    pub async fn ready_for_next(
        &mut self,
        room_code: impl Into<String>,
        hand_id: i64,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Ready);
        let message = ClientMessage::ReadyForNext(c2s::ReadyForNext {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(room_code.into()),
            hand_id: Some(hand_id),
        });
        self.send(message).await?;
        Ok(request_id)
    }

    /// resume_session using the stored session credentials.
    pub async fn resume_session(&mut self) -> Result<RequestId, ClientError> {
        let Some(session) = self.session.clone() else {
            return Err(ClientError::NoSession);
        };
        self.send_resume_session(&session).await
    }

    /// get_snapshot: asks the server to replay room_joined(resumed: true) and
    /// the latest game_state.
    ///
    /// The reference client never sends this message, so it carries no
    /// requestId (the spec documents the field as optional for this request).
    pub async fn get_snapshot(&mut self) -> Result<(), ClientError> {
        let message = ClientMessage::GetSnapshot(c2s::GetSnapshot {
            envelope: ClientEnvelope::new(),
        });
        self.send(message).await
    }

    /// Returns the next lifecycle/protocol event, or None once the client is
    /// permanently closed (terminal close, intentional disconnect or a failed
    /// reconnect with no attempts left).
    ///
    /// Automatic reconnects happen inside this future: a Reconnecting event is
    /// returned first, then the next call sleeps for the backoff delay and
    /// attempts the connection.
    pub async fn next_event(&mut self) -> Option<ClientEvent> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Some(event);
            }
            match self.state {
                State::Closed => return None,
                State::ReconnectPending { delay_ms, .. } => {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    match self.establish(true).await {
                        Ok(()) => {}
                        Err(error) => self.on_reconnect_failure(error),
                    }
                }
                State::Connected => {
                    let incoming = self.read_incoming().await;
                    match incoming {
                        Incoming::Text(raw) => self.on_text_frame(raw),
                        Incoming::Closed(code, reason) => self.handle_connection_lost(code, reason),
                        Incoming::Gone(reason) => self.handle_connection_lost(None, reason),
                        Incoming::Skip => {}
                    }
                }
            }
        }
    }

    /// Manual reconnect: closes the current socket silently and connects again.
    ///
    /// This mirrors network.reconnectToServer(): the resume session is kept,
    /// the reconnect is marked as reconnecting, and a failed attempt schedules
    /// the normal backoff loop while still returning the immediate error.
    pub async fn reconnect(&mut self) -> Result<(), ClientError> {
        self.teardown_socket().await;
        self.attempts = 0;
        match self.establish(true).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let can_resume = self.config.auto_reconnect && self.session.is_some();
                self.queue.push_back(ClientEvent::Disconnected {
                    code: None,
                    reason: error.to_string(),
                    will_reconnect: can_resume,
                });
                if can_resume {
                    self.schedule_retry(error.to_string());
                } else {
                    self.state = State::Closed;
                    self.queue.push_back(ClientEvent::Closed);
                }
                Err(error)
            }
        }
    }

    /// Intentional disconnect (the reference client's disconnect()).
    ///
    /// Keeps the stored session so a later reconnect() can resume; call
    /// clear_session when the user leaves the room for good.
    pub async fn disconnect(&mut self) {
        self.teardown_socket().await;
        self.state = State::Closed;
        self.queue.push_back(ClientEvent::Disconnected {
            code: Some(CLOSE_NORMAL),
            reason: "client disconnect".to_owned(),
            will_reconnect: false,
        });
        self.queue.push_back(ClientEvent::Closed);
    }

    async fn establish(&mut self, reconnecting: bool) -> Result<(), ClientError> {
        let result = self.establish_inner(reconnecting).await;
        if result.is_err() {
            self.connection = None;
            self.state = State::Closed;
        }
        result
    }

    async fn establish_inner(&mut self, reconnecting: bool) -> Result<(), ClientError> {
        let handshake_timeout = self.config.handshake_timeout;
        let mut stream = match handshake_timeout {
            Some(timeout) => tokio::time::timeout(timeout, transport::open_ws(&self.config.url))
                .await
                .map_err(|_| handshake_timeout_error(timeout))??,
            None => transport::open_ws(&self.config.url).await?,
        };
        let hello = match handshake_timeout {
            Some(timeout) => tokio::time::timeout(timeout, transport::read_hello(&mut stream))
                .await
                .map_err(|_| handshake_timeout_error(timeout))??,
            None => transport::read_hello(&mut stream).await?,
        };
        if let Err(error) = self.config.hello.validate(&hello) {
            transport::close_stream(stream).await;
            return Err(ClientError::Hello(error));
        }
        let (sink, stream) = stream.split();
        self.connection = Some(Connection { sink, stream });
        self.hello = Some(hello.clone());
        self.state = State::Connected;
        if let Some(session) = self.session.clone() {
            self.send_resume_session(&session).await?;
        }
        self.queue
            .push_back(ClientEvent::Connected { reconnecting });
        self.queue.push_back(ClientEvent::Hello(hello));
        Ok(())
    }

    async fn send_resume_session(
        &mut self,
        session: &SessionCredentials,
    ) -> Result<RequestId, ClientError> {
        let request_id = self.new_request_id(RequestKind::Resume);
        let message = ClientMessage::ResumeSession(c2s::ResumeSession {
            envelope: ClientEnvelope::with_request_id(request_id.clone()),
            room_code: Some(session.room_code.clone()),
            resume_token: Some(session.resume_token.clone()),
        });
        self.send(message).await?;
        Ok(request_id)
    }

    async fn read_incoming(&mut self) -> Incoming {
        let Some(connection) = self.connection.as_mut() else {
            return Incoming::Gone("connection missing".to_owned());
        };
        match connection.stream.next().await {
            Some(Ok(Message::Text(text))) => Incoming::Text(text.as_str().to_owned()),
            Some(Ok(Message::Close(frame))) => match frame {
                Some(frame) => Incoming::Closed(
                    Some(u16::from(frame.code)),
                    frame.reason.as_str().to_owned(),
                ),
                None => Incoming::Closed(None, String::new()),
            },
            Some(Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) | Message::Binary(_),
            )) => Incoming::Skip,
            Some(Err(error)) => Incoming::Gone(error.to_string()),
            None => Incoming::Gone("connection closed by peer".to_owned()),
        }
    }

    fn on_text_frame(&mut self, raw: String) {
        match parse_server_message(&raw) {
            Ok(ServerMessage::Hello(hello)) => {
                self.hello = Some(hello.clone());
                self.queue.push_back(ClientEvent::Hello(hello));
            }
            Ok(message) => {
                if matches!(
                    message,
                    ServerMessage::RoomCreated(_)
                        | ServerMessage::RoomJoined(_)
                        | ServerMessage::GameState(_)
                ) {
                    // js/network.js resets reconnectAttempts in these handlers.
                    self.attempts = 0;
                }
                self.queue.push_back(ClientEvent::Message(message));
            }
            Err(ServerParseError::UnknownType(_)) => {
                // js/network.js ignores message types it does not know.
            }
            Err(error) => {
                self.queue
                    .push_back(ClientEvent::ProtocolError { raw, error });
            }
        }
    }

    fn handle_connection_lost(&mut self, code: Option<u16>, reason: String) {
        self.connection = None;
        let terminal = code.is_some_and(is_terminal_close_code);
        let can_resume = self.config.auto_reconnect && self.session.is_some() && !terminal;
        self.queue.push_back(ClientEvent::Disconnected {
            code,
            reason: reason.clone(),
            will_reconnect: can_resume,
        });
        if !can_resume {
            self.state = State::Closed;
            self.queue.push_back(ClientEvent::Closed);
            return;
        }
        self.schedule_retry(reason);
    }

    fn on_reconnect_failure(&mut self, error: ClientError) {
        let reason = error.to_string();
        self.connection = None;
        self.queue.push_back(ClientEvent::Disconnected {
            code: None,
            reason: reason.clone(),
            will_reconnect: true,
        });
        self.schedule_retry(reason);
    }

    /// Schedules the next retry delay, or closes the client when the attempt
    /// cap is reached. The delay uses the current attempt index, exactly like
    /// js/network.js (which computes the delay and then increments attempts).
    fn schedule_retry(&mut self, last_error: String) {
        let attempt = self.attempts;
        if let Some(max) = self.config.max_reconnect_attempts
            && attempt >= max
        {
            self.state = State::Closed;
            self.queue.push_back(ClientEvent::ReconnectFailed {
                attempts: attempt,
                last_error,
            });
            self.queue.push_back(ClientEvent::Closed);
            return;
        }
        let delay_ms = reconnect_delay_ms(attempt, self.config.jitter_fraction());
        self.attempts = attempt.saturating_add(1);
        self.state = State::ReconnectPending { attempt, delay_ms };
        self.queue
            .push_back(ClientEvent::Reconnecting { attempt, delay_ms });
    }

    async fn teardown_socket(&mut self) {
        self.state = State::Closed;
        if let Some(mut connection) = self.connection.take() {
            let _ = tokio::time::timeout(
                Duration::from_millis(200),
                connection.sink.send(Message::Close(None)),
            )
            .await;
            let _ = tokio::time::timeout(Duration::from_millis(200), connection.sink.close()).await;
        }
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("config", &self.config)
            .field("state", &self.state)
            .field("connected", &self.is_connected())
            .field("session", &self.session)
            .field("queued_events", &self.queue.len())
            .finish()
    }
}

/// Result of one read from the socket.
enum Incoming {
    Text(String),
    Closed(Option<u16>, String),
    Gone(String),
    Skip,
}
