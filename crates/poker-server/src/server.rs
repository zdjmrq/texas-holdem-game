//! Server state, axum transport and the protocol message handlers.
//!
//! A single std::sync::Mutex guards all server state. Handlers are synchronous:
//! they mutate rooms and push serialized frames into per-connection bounded
//! outbound queues, so no lock is ever held across an await. Timers (AI turn, next
//! hand, disconnect grace, empty-room TTL) are tokio tasks validated by
//! per-room generation tokens, which reproduces the reference server's
//! setTimeout + clearTimeout semantics without cancelled-callback races.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use poker_core::{RandomSource, RuntimeRng};
use poker_protocol as proto;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::ai::AiView;
use crate::metrics::{ServerMetrics, ServerMetricsSnapshot};
use crate::options::ServerOptions;
use crate::room::{Room, Seat, request_key};
use crate::wire;

/// Text of the "already in a room" join error.
const TEXT_ALREADY_IN_ROOM: &str = "请先离开当前房间";
/// Text of the invalid room-code error.
const TEXT_ROOM_CODE_RANGE: &str = "房间号只能是 1–5";
/// Text of the room-full error.
const TEXT_ROOM_FULL: &str = "房间已满";
/// Text of the room-already-started error.
const TEXT_ROOM_STARTED: &str = "房间已经开局，只能使用恢复令牌返回原座位";
/// Text of the resume-while-in-room error.
const TEXT_RESUME_ALREADY_IN_ROOM: &str = "请先离开当前房间再恢复其他座位";
/// Text of the expired resume token error.
const TEXT_RESUME_EXPIRED: &str = "恢复信息已失效";
/// Text of the missing-room resume error.
const TEXT_RESUME_ROOM_GONE: &str = "原房间已关闭";
/// Text of the host-only start error.
const TEXT_HOST_ONLY: &str = "只有房主可以开始游戏";
/// Text of the game-already-started error.
const TEXT_GAME_STARTED: &str = "游戏已经开始";
/// Text of the "someone is reconnecting" start error.
const TEXT_WAITING_RECONNECT: &str = "有玩家正在恢复连接，请稍候再开始";
/// Text of the not-enough-players start error.
const TEXT_NEED_TWO: &str = "至少需要两名可参赛玩家";
/// Text of the not-in-room error.
const TEXT_NOT_IN_ROOM: &str = "尚未进入房间";
/// Text of the game-not-started error.
const TEXT_GAME_NOT_STARTED: &str = "牌局尚未开始";
/// Text of the missing request-id action error.
const TEXT_ACTION_MISSING_REQUEST: &str = "动作缺少 requestId";
/// Text of the early-ready error.
const TEXT_HAND_NOT_ENDED: &str = "本手尚未结束";
/// Text of the stale-ready error.
const TEXT_OLD_HAND_READY: &str = "旧牌局的准备信号已忽略";
/// action_ack rejection text for a stale room.
const TEXT_STALE_ROOM: &str = "房间状态已过期";
/// action_ack rejection text for a stale hand.
const TEXT_STALE_HAND: &str = "这是一条旧牌局动作";
/// action_ack rejection text for a stale turn.
const TEXT_STALE_TURN: &str = "行动回合已经变化";
/// action_ack rejection text for a stale state version.
const TEXT_STALE_VERSION: &str = "牌局状态已更新，请重试";
/// action_ack success text.
const TEXT_ACTION_OK: &str = "行动已确认";
/// Text of the illegal-action rejection.
const TEXT_ILLEGAL_ACTION: &str = "当前不能执行这个行动";

/// Capacity of the per-connection outbound queue. A slow consumer that fills
/// this queue is disconnected instead of letting memory grow without bound.
const OUTBOUND_CAPACITY: usize = 256;

/// Metadata of one live WebSocket connection.
struct ConnectionMeta {
    ip: String,
    tx: mpsc::Sender<Message>,
    failed: Arc<AtomicBool>,
    alive: bool,
    metrics: Arc<ServerMetrics>,
    room_code: Option<String>,
    player_id: Option<String>,
    session_token: Option<String>,
    rate_started: Instant,
    message_count: u32,
}

/// A resume token -> (room, player) mapping.
struct SessionRef {
    room_code: String,
    player_id: String,
}

/// All mutable server state (guarded by one mutex; see module docs).
struct ServerState {
    options: ServerOptions,
    rooms: BTreeMap<String, Room>,
    sessions: HashMap<String, SessionRef>,
    connections: HashMap<u64, ConnectionMeta>,
    next_conn_id: u64,
    id_rng: RuntimeRng,
    next_joined_seq: u64,
    shutdown: bool,
    metrics: Arc<ServerMetrics>,
}

impl ServerState {
    fn new(options: ServerOptions) -> Self {
        let id_rng = RuntimeRng::from_optional_state(options.rng_seed.map(|seed| seed as u32));
        Self {
            options,
            rooms: BTreeMap::new(),
            sessions: HashMap::new(),
            connections: HashMap::new(),
            next_conn_id: 1,
            id_rng,
            next_joined_seq: 1,
            shutdown: false,
            metrics: Arc::new(ServerMetrics::default()),
        }
    }

    fn next_conn_id(&mut self) -> u64 {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        id
    }

    fn room_rng(&mut self) -> RuntimeRng {
        // Live decks must not share their stream with publicly observable IDs.
        let seed = self.options.rng_seed.map(|_| self.id_rng.next_u32());
        RuntimeRng::from_optional_state(seed)
    }
}

/// Shared handle used by axum handlers and timer tasks.
type SharedState = Arc<Mutex<ServerState>>;

fn lock(state: &SharedState) -> MutexGuard<'_, ServerState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Handle to a running server, suitable for embedding in the desktop app.
pub struct ServerHandle {
    local_addr: SocketAddr,
    app_version: String,
    state: SharedState,
    metrics: Arc<ServerMetrics>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    background: Vec<JoinHandle<()>>,
}

impl ServerHandle {
    /// Actual bound address (useful with port 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// appVersion reported in hello.
    #[must_use]
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    /// URL the desktop client / interop scripts should connect to.
    #[must_use]
    pub fn url(&self) -> String {
        format!("ws://{}", self.local_addr)
    }

    /// Point-in-time counter snapshot (connections, rooms, rejections, AI latency).
    #[must_use]
    pub fn metrics(&self) -> ServerMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Waits for the server to stop after signalling shutdown (same as
    /// shutdown() but named for embedding call sites).
    pub async fn join(self) {
        self.shutdown().await;
    }

    /// Stops accepting connections, closes live sockets and waits for the
    /// listener task to finish (aborting it if it does not stop in 2 seconds).
    pub async fn shutdown(mut self) {
        {
            let mut guard = lock(&self.state);
            guard.shutdown = true;
            for meta in guard.connections.values() {
                let _ = meta.tx.try_send(Message::Close(Some(CloseFrame {
                    code: proto::CLOSE_NORMAL,
                    reason: "server shutdown".into(),
                })));
            }
        }
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        for task in &self.background {
            task.abort();
        }
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
        info!("poker-server stopped");
    }
}

/// Starts the server on the configured port.
///
/// # Errors
///
/// Returns the underlying bind error (for example AddrInUse when the desktop
/// app should fall back to spawn_server_with_fallback).
pub async fn spawn_server(options: ServerOptions) -> std::io::Result<ServerHandle> {
    let options = options.sanitized();
    let app_version = options.app_version.clone();
    let host = options.host.clone();
    let port = options.port;
    let state: SharedState = Arc::new(Mutex::new(ServerState::new(options)));
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    let local_addr = listener.local_addr()?;
    let router = Router::new()
        .route("/", get(ws_handler))
        .with_state(state.clone());
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let service = router.into_make_service_with_connect_info::<SocketAddr>();
        let server = axum::serve(listener, service).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(error) = server.await {
            eprintln!("[poker-server] serve error: {error}");
        }
    });
    let heartbeat_interval = lock(&state).options.heartbeat_interval();
    let heartbeat = tokio::spawn(heartbeat_loop(state.clone(), heartbeat_interval));
    let metrics = lock(&state).metrics.clone();
    info!(%local_addr, %app_version, "poker-server started");
    Ok(ServerHandle {
        local_addr,
        app_version,
        state,
        metrics,
        shutdown: Some(shutdown_tx),
        task: Some(task),
        background: vec![heartbeat],
    })
}

/// Starts the server, falling back to an ephemeral port on AddrInUse.
///
/// This mirrors the reference desktop behaviour: try the preferred port
/// (3000), and when it is already bound let the room server pick a free one.
///
/// # Errors
///
/// Returns the bind error when the fallback attempt also fails.
pub async fn spawn_server_with_fallback(
    mut options: ServerOptions,
) -> std::io::Result<ServerHandle> {
    let preferred = options.port;
    match spawn_server(options.clone()).await {
        Ok(handle) => Ok(handle),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && preferred != 0 => {
            options.port = 0;
            spawn_server(options).await
        }
        Err(error) => Err(error),
    }
}

/// Probes an existing server with the reference desktop hello handshake.
///
/// Returns true when a hello arrives within the timeout, its serverId is
/// "texas-holdem-game", its protocolVersion is 2 and its appVersion equals the
/// expected version exactly (the v1.5.8 desktop probe contract).
pub async fn probe_server(url: &str, expected_app_version: &str, timeout: Duration) -> bool {
    let probe = async {
        let Ok((mut socket, _response)) = tokio_tungstenite::connect_async(url).await else {
            return false;
        };
        loop {
            match socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let Ok(proto::ServerMessage::Hello(hello)) =
                        proto::parse_server_message(text.as_str())
                    else {
                        return false;
                    };
                    return hello.is_desktop_probe_compatible(expected_app_version);
                }
                Some(Ok(_)) => continue,
                _ => return false,
            }
        }
    };
    tokio::time::timeout(timeout, probe).await.unwrap_or(false)
}

/// One heartbeat tick: ping every live connection and drop peers that did not
/// answer the previous ping (same semantics as local-server.js pingConnections).
async fn heartbeat_loop(state: SharedState, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick completes immediately; skip it so the first ping goes out
    // one interval after the connection was accepted.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let mut guard = lock(&state);
        if guard.shutdown {
            return;
        }
        let mut timed_out = Vec::new();
        for (conn_id, meta) in &mut guard.connections {
            if !meta.alive {
                timed_out.push(*conn_id);
                continue;
            }
            meta.alive = false;
            if meta.tx.try_send(Message::Ping(Default::default())).is_err() {
                timed_out.push(*conn_id);
            }
        }
        for conn_id in timed_out {
            if let Some(meta) = guard.connections.remove(&conn_id) {
                meta.failed.store(true, Ordering::Relaxed);
                ServerMetrics::inc(&meta.metrics.heartbeat_timeouts);
                // Wake the writer task so it observes the failed flag and drops
                // the socket; the frame itself may not be delivered.
                let _ = meta.tx.try_send(Message::Close(Some(CloseFrame {
                    code: proto::CLOSE_NORMAL,
                    reason: "heartbeat timeout".into(),
                })));
                warn!(conn_id, "heartbeat timeout; connection dropped");
            }
        }
    }
}

/// Marks a connection alive after a pong control frame.
fn mark_alive(state: &SharedState, conn_id: u64) {
    let mut guard = lock(state);
    if let Some(meta) = guard.connections.get_mut(&conn_id) {
        meta.alive = true;
    }
}

/// axum WebSocket upgrade handler: enforces the Origin allowlist before the
/// upgrade and records the client IP.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let trust_proxy = lock(&state).options.trust_proxy;
    let allowed_origins = lock(&state).options.allowed_origins.clone();
    let origin = headers.get("origin").and_then(|value| value.to_str().ok());
    if !origin_allowed(origin, &allowed_origins) {
        let metrics = lock(&state).metrics.clone();
        ServerMetrics::inc(&metrics.connections_rejected_origin);
        warn!(
            origin = origin.unwrap_or(""),
            "connection rejected: origin not allowed"
        );
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let ip = client_ip(&headers, peer, trust_proxy);
    // The app-level guard enforces the reference 16 KiB payload limit and owns
    // the 1009 close frame. The transport limit is raised so oversized frames
    // reach on_text() instead of being dropped by the ws library without a
    // protocol close.
    ws.max_message_size(256 * 1024)
        .max_frame_size(256 * 1024)
        .on_upgrade(move |socket| handle_connection(socket, state, ip))
}

fn origin_allowed(origin: Option<&str>, allowed_origins: &[String]) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    if origin == "null" || origin.starts_with("file://") {
        return true;
    }
    if allowed_origins.iter().any(|allowed| allowed == origin) {
        return true;
    }
    let Ok(parsed) = url_hostname(origin) else {
        return false;
    };
    matches!(parsed.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn url_hostname(origin: &str) -> Result<String, ()> {
    let rest = match origin.split_once("://") {
        Some((_scheme, rest)) => rest,
        None => return Err(()),
    };
    let authority = rest.split(['/', '?', '#']).next().ok_or(())?;
    let authority = authority.rsplit('@').next().ok_or(())?;
    if authority.is_empty() {
        return Err(());
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']').ok_or(())?;
        return Ok(rest[..end].to_owned());
    }
    let host = authority.split(':').next().ok_or(())?;
    if host.is_empty() {
        return Err(());
    }
    Ok(host.to_owned())
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = forwarded.split(',').next() {
                let first = first.trim();
                if !first.is_empty() {
                    return first.to_owned();
                }
            }
        }
    }
    peer.ip().to_string()
}

/// One live connection: register, writer task, reader loop, disconnect cleanup.
async fn handle_connection(socket: WebSocket, state: SharedState, ip: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Message>(OUTBOUND_CAPACITY);
    let failed = Arc::new(AtomicBool::new(false));
    enum Admission {
        Accepted(u64),
        Rejected(u16, &'static str),
    }
    let admission = {
        let mut guard = lock(&state);
        if guard.shutdown {
            Admission::Rejected(proto::CLOSE_TRY_AGAIN_LATER, "server shutting down")
        } else {
            let ip_count = guard
                .connections
                .values()
                .filter(|meta| meta.ip == ip)
                .count();
            let capacity_reached = guard.connections.len()
                >= guard.options.max_connections as usize
                || ip_count >= guard.options.max_connections_per_ip as usize;
            if capacity_reached {
                ServerMetrics::inc(&guard.metrics.connections_rejected_capacity);
                warn!(%ip, "connection rejected: capacity reached");
                Admission::Rejected(proto::CLOSE_TRY_AGAIN_LATER, proto::CLOSE_REASON_CAPACITY)
            } else {
                let metrics = guard.metrics.clone();
                let conn_id = guard.next_conn_id();
                guard.connections.insert(
                    conn_id,
                    ConnectionMeta {
                        ip: ip.clone(),
                        tx: tx.clone(),
                        failed: failed.clone(),
                        alive: true,
                        metrics: metrics.clone(),
                        room_code: None,
                        player_id: None,
                        session_token: None,
                        rate_started: Instant::now(),
                        message_count: 0,
                    },
                );
                ServerMetrics::inc(&metrics.connections_accepted);
                info!(conn_id, %ip, "connection accepted");
                Admission::Accepted(conn_id)
            }
        }
    };
    let conn_id = match admission {
        Admission::Accepted(conn_id) => conn_id,
        Admission::Rejected(code, reason) => {
            let _ = sink
                .send(Message::Close(Some(CloseFrame {
                    code,
                    reason: reason.into(),
                })))
                .await;
            let _ = sink.close().await;
            return;
        }
    };
    let app_version = lock(&state).options.app_version.clone();
    let hello = proto::ServerMessage::Hello(proto::Hello {
        protocol_version: proto::PROTOCOL_VERSION,
        server_id: proto::SERVER_ID.to_owned(),
        server_name: Some(proto::SERVER_NAME.to_owned()),
        app_version,
    });
    let _ = tx.try_send(Message::Text(hello.to_json_string().into()));
    let writer_failed = failed.clone();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if writer_failed.load(Ordering::Relaxed) {
                break;
            }
            let is_close = matches!(message, Message::Close(_));
            if sink.send(message).await.is_err() {
                break;
            }
            if is_close {
                break;
            }
        }
        let _ = sink.close().await;
    });
    loop {
        let incoming = stream.next().await;
        let Some(incoming) = incoming else {
            break;
        };
        match incoming {
            Ok(Message::Text(text)) => on_text(&state, conn_id, text.as_str()),
            Ok(Message::Binary(bytes)) => match std::str::from_utf8(&bytes) {
                Ok(text) => on_text(&state, conn_id, text),
                Err(_) => send_error(
                    &lock(&state).connections,
                    conn_id,
                    None,
                    proto::ErrorCode::InvalidRequest,
                    proto::error::TEXT_INVALID_JSON,
                ),
            },
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) => {}
            Ok(Message::Pong(_)) => mark_alive(&state, conn_id),
            Err(error) => {
                debug!(conn_id, %error, "connection read error");
                break;
            }
        }
    }
    writer.abort();
    {
        let mut guard = lock(&state);
        guard.on_disconnect(&state, conn_id);
    }
    debug!(conn_id, "connection closed");
}

/// Handles one text frame: rate limit, size guard, parse, dispatch.
fn on_text(state: &SharedState, conn_id: u64, text: &str) {
    let violation = {
        let mut guard = lock(state);
        if guard.shutdown {
            return;
        }
        let rate_window = guard.options.rate_window();
        let max_messages = guard.options.max_messages_per_window;
        let metrics = guard.metrics.clone();
        ServerMetrics::inc(&metrics.messages_received);
        let now = Instant::now();
        let Some(meta) = guard.connections.get_mut(&conn_id) else {
            return;
        };
        if now.duration_since(meta.rate_started) >= rate_window {
            meta.rate_started = now;
            meta.message_count = 0;
        }
        meta.message_count += 1;
        let over_rate = meta.message_count > max_messages;
        let tx = meta.tx.clone();
        if over_rate {
            ServerMetrics::inc(&metrics.rate_limited);
            warn!(conn_id, "connection rate limit exceeded");
            guard.connections.remove(&conn_id);
            Some((
                tx,
                proto::CLOSE_POLICY_VIOLATION,
                proto::CLOSE_REASON_RATE_LIMIT,
            ))
        } else if text.len() > proto::MAX_PAYLOAD_BYTES {
            guard.connections.remove(&conn_id);
            Some((tx, proto::CLOSE_MESSAGE_TOO_BIG, "message too large"))
        } else {
            None
        }
    };
    if let Some((tx, code, reason)) = violation {
        let _ = tx.try_send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })));
        return;
    }
    let parsed = {
        let guard = lock(state);
        match proto::parse_client_message(text) {
            Ok(message) => message,
            Err(error) => {
                ServerMetrics::inc(&guard.metrics.parse_errors);
                debug!(conn_id, error = %error, "message parse failed");
                let reply = proto::ServerMessage::Error(error.to_server_error());
                send_message(&guard.connections, conn_id, &reply);
                return;
            }
        }
    };
    let mut guard = lock(state);
    guard.dispatch(state, conn_id, parsed);
}

// ---------------------------------------------------------------------------
// Serialization / broadcast helpers (take split borrows of ServerState fields).
// ---------------------------------------------------------------------------

/// Pushes a frame into the bounded outbound queue.
///
/// A full queue means the peer is not draining its socket (slow consumer). The
/// connection is marked failed so the writer task closes it instead of letting
/// the queue or memory grow without bound. The reference server has no such
/// path (ws buffers locally); this is documented in docs/specs/server-plan.md.
fn try_send_frame(meta: &ConnectionMeta, message: Message) {
    if meta.tx.try_send(message).is_err() {
        if !meta.failed.swap(true, Ordering::Relaxed) {
            ServerMetrics::inc(&meta.metrics.slow_consumers);
            warn!("connection outbound queue full; closing slow consumer");
        }
    }
}

fn send_message(
    connections: &HashMap<u64, ConnectionMeta>,
    conn_id: u64,
    message: &proto::ServerMessage,
) {
    if let Some(meta) = connections.get(&conn_id) {
        try_send_frame(meta, Message::Text(message.to_json_string().into()));
    }
}

fn send_text(connections: &HashMap<u64, ConnectionMeta>, conn_id: u64, text: String) {
    if let Some(meta) = connections.get(&conn_id) {
        try_send_frame(meta, Message::Text(text.into()));
    }
}

fn send_error(
    connections: &HashMap<u64, ConnectionMeta>,
    conn_id: u64,
    request_id: Option<proto::RequestId>,
    code: proto::ErrorCode,
    text: &str,
) {
    let reply = proto::ServerMessage::Error(proto::ErrorMessage::new(request_id, code, text));
    send_message(connections, conn_id, &reply);
}

fn send_ack(
    connections: &HashMap<u64, ConnectionMeta>,
    conn_id: u64,
    request_id: &proto::RequestId,
    accepted: bool,
    room: Option<&Room>,
) {
    let reply = proto::ServerMessage::Ack(wire::ack(request_id, accepted, room));
    send_message(connections, conn_id, &reply);
}

/// Sends a message to every connected human in the room.
fn broadcast_message(
    room: &Room,
    connections: &HashMap<u64, ConnectionMeta>,
    message: &proto::ServerMessage,
) {
    for seat in &room.seats {
        if seat.is_human && seat.connected {
            if let Some(conn_id) = seat.conn_id {
                send_message(connections, conn_id, message);
            }
        }
    }
}

/// Sends each connected human its personalised game snapshot.
fn broadcast_game(room: &Room, connections: &HashMap<u64, ConnectionMeta>) {
    if room.game.is_none() {
        return;
    }
    for (index, seat) in room.seats.iter().enumerate() {
        if !(seat.is_human && seat.connected) {
            continue;
        }
        let Some(conn_id) = seat.conn_id else {
            continue;
        };
        if let Some(snapshot) = wire::game_state(room, index) {
            send_message(
                connections,
                conn_id,
                &proto::ServerMessage::GameState(snapshot),
            );
        }
    }
}

fn broadcast_lobby(room: &Room, connections: &HashMap<u64, ConnectionMeta>) {
    let message = proto::ServerMessage::PlayerJoined(wire::lobby_update(room));
    broadcast_message(room, connections, &message);
}

fn broadcast_player_left(room: &Room, connections: &HashMap<u64, ConnectionMeta>) {
    let message = proto::ServerMessage::PlayerLeft(wire::lobby_update(room));
    broadcast_message(room, connections, &message);
}

fn broadcast_ready(room: &Room, connections: &HashMap<u64, ConnectionMeta>, all_ready: bool) {
    let message = proto::ServerMessage::ReadyStatus(wire::ready_status(room, all_ready));
    broadcast_message(room, connections, &message);
}

fn broadcast_host_changed(room: &Room, connections: &HashMap<u64, ConnectionMeta>) {
    for (index, seat) in room.seats.iter().enumerate() {
        if !seat.is_human || !seat.connected {
            continue;
        }
        let Some(conn_id) = seat.conn_id else {
            continue;
        };
        if let Some(payload) = wire::host_changed(room, index) {
            send_message(
                connections,
                conn_id,
                &proto::ServerMessage::HostChanged(payload),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch.
// ---------------------------------------------------------------------------

impl ServerState {
    fn dispatch(&mut self, shared: &SharedState, conn_id: u64, message: proto::ClientMessage) {
        match message {
            proto::ClientMessage::CreateRoom(payload) => self.create_room(shared, conn_id, payload),
            proto::ClientMessage::JoinRoom(payload) => self.join_room(shared, conn_id, payload),
            proto::ClientMessage::LeaveRoom(payload) => self.leave_room(shared, conn_id, payload),
            proto::ClientMessage::StartGame(payload) => self.start_game(shared, conn_id, payload),
            proto::ClientMessage::PlayerAction(payload) => {
                self.player_action(shared, conn_id, payload)
            }
            proto::ClientMessage::ReadyForNext(payload) => {
                self.ready_for_next(shared, conn_id, payload)
            }
            proto::ClientMessage::ResumeSession(payload) => {
                self.resume_session(shared, conn_id, payload)
            }
            proto::ClientMessage::GetSnapshot(payload) => self.get_snapshot(conn_id, payload),
        }
    }

    fn request_id_of(envelope: &proto::ClientEnvelope) -> Option<proto::RequestId> {
        envelope.request_id.clone()
    }

    /// Creates a room on the first free code, then performs a normal join.
    fn create_room(&mut self, shared: &SharedState, conn_id: u64, payload: proto::CreateRoom) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some(meta) = self.connections.get(&conn_id) else {
            return;
        };
        if meta.room_code.is_some() {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ALREADY_IN_ROOM,
            );
            return;
        }
        let free = proto::ROOM_CODES
            .iter()
            .find(|code| !self.rooms.contains_key(**code))
            .map(|code| (*code).to_owned());
        let Some(code) = free else {
            let text = format!("房间已满（最多 {} 个）", self.options.max_rooms);
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                &text,
            );
            return;
        };
        if self.rooms.len() >= self.options.max_rooms as usize {
            let text = format!("房间已满（最多 {} 个）", self.options.max_rooms);
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                &text,
            );
            return;
        }
        self.join_internal(
            shared,
            conn_id,
            code,
            payload.name,
            payload.options,
            request_id,
            false,
        );
    }

    fn join_room(&mut self, shared: &SharedState, conn_id: u64, payload: proto::JoinRoom) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some(meta) = self.connections.get(&conn_id) else {
            return;
        };
        if meta.room_code.is_some() {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ALREADY_IN_ROOM,
            );
            return;
        }
        let code = payload.room_code.unwrap_or_default().trim().to_owned();
        if !is_valid_room_code(&code) {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ROOM_CODE_RANGE,
            );
            return;
        }
        self.join_internal(
            shared,
            conn_id,
            code,
            payload.name,
            payload.options,
            request_id,
            true,
        );
    }

    /// Shared join path used by both create_room and join_room.
    #[allow(clippy::too_many_arguments)]
    fn join_internal(
        &mut self,
        _shared: &SharedState,
        conn_id: u64,
        code: String,
        name: Option<String>,
        options: proto::RoomOptionsInput,
        request_id: Option<proto::RequestId>,
        room_limit_code: bool,
    ) {
        let created = !self.rooms.contains_key(&code);
        if created && self.rooms.len() >= self.options.max_rooms as usize {
            let text = format!("房间已满（最多 {} 个）", self.options.max_rooms);
            let error_code = if room_limit_code {
                proto::ErrorCode::RoomLimit
            } else {
                proto::ErrorCode::InvalidRequest
            };
            send_error(&self.connections, conn_id, request_id, error_code, &text);
            return;
        }
        if created {
            let config = options.sanitize(f64::from(self.options.ai_delay_ms));
            let rng = self.room_rng();
            self.rooms
                .insert(code.clone(), Room::new(code.clone(), config, rng));
            ServerMetrics::inc(&self.metrics.rooms_created);
            info!(room_code = %code, "room created");
        }
        let player_id = generate_player_id(&mut self.id_rng);
        let resume_token = generate_token(&mut self.id_rng);
        let joined_at = {
            let seq = self.next_joined_seq;
            self.next_joined_seq += 1;
            seq
        };
        let room_name = proto::normalize_name(name.as_deref().unwrap_or(""));
        let ServerState {
            rooms,
            sessions,
            connections,
            ..
        } = self;
        let Some(room) = rooms.get_mut(&code) else {
            return;
        };
        if room.game.is_some() {
            send_error(
                connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ROOM_STARTED,
            );
            return;
        }
        let humans = room
            .seats
            .iter()
            .filter(|seat| seat.is_human && !seat.left)
            .count();
        if humans >= usize::try_from(room.config.max_players).unwrap_or(10) {
            send_error(
                connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ROOM_FULL,
            );
            return;
        }
        let Some(seat_id) = room.free_seat_id() else {
            send_error(
                connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_ROOM_FULL,
            );
            return;
        };
        let starting_stack = u64::try_from(room.config.starting_stack).unwrap_or(0);
        let seat = Seat::human(
            player_id.clone(),
            seat_id,
            room_name,
            starting_stack,
            resume_token.clone(),
            joined_at,
            conn_id,
        );
        room.insert_seat(seat);
        if room.host_id.is_none() {
            room.host_id = Some(player_id.clone());
        }
        sessions.insert(
            resume_token.clone(),
            SessionRef {
                room_code: code.clone(),
                player_id: player_id.clone(),
            },
        );
        if let Some(meta) = connections.get_mut(&conn_id) {
            meta.room_code = Some(code.clone());
            meta.player_id = Some(player_id.clone());
            meta.session_token = Some(resume_token.clone());
        }
        room.state_version += 1;
        let index = room.seat_index(&player_id).unwrap_or(0);
        let Some(info) = wire::room_info(room, index, false) else {
            return;
        };
        let message = if created {
            proto::ServerMessage::RoomCreated(info)
        } else {
            proto::ServerMessage::RoomJoined(info)
        };
        send_message(connections, conn_id, &message);
        broadcast_lobby(room, connections);
    }

    fn start_game(&mut self, shared: &SharedState, conn_id: u64, payload: proto::StartGame) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some((code, player_id)) = self.room_player_of(conn_id) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_NOT_IN_ROOM,
            );
            return;
        };
        {
            let Some(room) = self.rooms.get(&code) else {
                return;
            };
            if room.host_id.as_deref() != Some(player_id.as_str()) {
                send_error(
                    &self.connections,
                    conn_id,
                    request_id.clone(),
                    proto::ErrorCode::InvalidRequest,
                    TEXT_HOST_ONLY,
                );
                return;
            }
            if room.game.is_some() {
                send_error(
                    &self.connections,
                    conn_id,
                    request_id.clone(),
                    proto::ErrorCode::InvalidRequest,
                    TEXT_GAME_STARTED,
                );
                return;
            }
            if room
                .seats
                .iter()
                .any(|seat| seat.is_human && !seat.left && !seat.connected)
            {
                send_error(
                    &self.connections,
                    conn_id,
                    request_id.clone(),
                    proto::ErrorCode::InvalidRequest,
                    TEXT_WAITING_RECONNECT,
                );
                return;
            }
        }
        let factory = self.options.ai_factory.clone();
        let mut game = {
            let Some(room) = self.rooms.get_mut(&code) else {
                return;
            };
            let starting_stack = u64::try_from(room.config.starting_stack).unwrap_or(0);
            let max_players = usize::try_from(room.config.max_players).unwrap_or(10);
            room.seats
                .retain(|seat| !seat.left && (seat.is_human || seat.stack > 0));
            let occupied: std::collections::BTreeSet<usize> =
                room.seats.iter().map(|seat| seat.seat_id).collect();
            for seat_id in 0..max_players {
                if !occupied.contains(&seat_id) {
                    room.insert_seat(Seat::ai(seat_id, starting_stack, factory.as_ref()));
                }
            }
            room.seats.retain(|seat| seat.seat_id < max_players);
            let engine_players: Vec<poker_engine::Player> =
                room.seats.iter().map(Seat::to_engine_player).collect();
            poker_engine::Game::new(room.engine_config.clone(), engine_players)
        };
        let started = {
            let Some(room) = self.rooms.get_mut(&code) else {
                return;
            };
            let started = game.start_hand(&mut room.rng);
            if started {
                room.game = Some(game);
                room.sync_from_game();
                let message = proto::ServerMessage::GameStarting(wire::game_starting(room));
                broadcast_message(room, &self.connections, &message);
            }
            started
        };
        if !started {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_NEED_TWO,
            );
            return;
        }
        self.after_game_change(shared, &code);
        if let Some(request_id) = request_id.as_ref() {
            if let Some(room) = self.rooms.get(&code) {
                send_ack(&self.connections, conn_id, request_id, true, Some(room));
            }
        }
    }

    fn leave_room(&mut self, shared: &SharedState, conn_id: u64, payload: proto::LeaveRoom) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some((code, player_id)) = self.room_player_of(conn_id) else {
            return;
        };
        self.detach_connection(conn_id);
        {
            let ServerState {
                rooms, sessions, ..
            } = self;
            let Some(room) = rooms.get_mut(&code) else {
                return;
            };
            let Some(index) = room.seat_index(&player_id) else {
                return;
            };
            room.seats[index].left = true;
            room.seats[index].connected = false;
            room.seats[index].conn_id = None;
            room.seats[index].disconnect_token = None;
            let token = room.seats[index].resume_token.clone();
            if !token.is_empty() {
                sessions.remove(&token);
            }
            if room.game.is_none() {
                room.seats.remove(index);
            }
        }
        if let Some(index) = self
            .rooms
            .get(&code)
            .and_then(|room| room.seat_index(&player_id))
        {
            self.fold_disconnected(shared, &code, index);
        }
        self.migrate_host(shared, &code, true);
        {
            let ServerState {
                rooms, connections, ..
            } = self;
            let Some(room) = rooms.get_mut(&code) else {
                return;
            };
            room.state_version += 1;
            broadcast_player_left(room, connections);
            broadcast_game(room, connections);
            if let Some(request_id) = request_id.as_ref() {
                send_ack(connections, conn_id, request_id, true, Some(room));
            }
        }
        self.try_start_next_hand(shared, &code);
        self.cleanup_room(shared, &code);
    }

    fn room_player_of(&self, conn_id: u64) -> Option<(String, String)> {
        let meta = self.connections.get(&conn_id)?;
        Some((meta.room_code.clone()?, meta.player_id.clone()?))
    }

    fn detach_connection(&mut self, conn_id: u64) {
        if let Some(meta) = self.connections.get_mut(&conn_id) {
            meta.room_code = None;
            meta.player_id = None;
            meta.session_token = None;
        }
    }

    fn get_snapshot(&mut self, conn_id: u64, payload: proto::GetSnapshot) {
        let _ = payload;
        let Some((code, player_id)) = self.room_player_of(conn_id) else {
            send_error(
                &self.connections,
                conn_id,
                None,
                proto::ErrorCode::InvalidRequest,
                TEXT_NOT_IN_ROOM,
            );
            return;
        };
        let ServerState {
            rooms, connections, ..
        } = self;
        let Some(room) = rooms.get_mut(&code) else {
            return;
        };
        let Some(index) = room.seat_index(&player_id) else {
            return;
        };
        let token = room.seats[index].resume_token.clone();
        if let Some(info) = wire::room_info(room, index, true) {
            send_message(
                connections,
                conn_id,
                &proto::ServerMessage::RoomJoined(info),
            );
        }
        if room.game.is_some() {
            if let Some(snapshot) = wire::game_state(room, index) {
                send_message(
                    connections,
                    conn_id,
                    &proto::ServerMessage::GameState(snapshot),
                );
            }
        }
        let _ = token;
    }
}

fn is_valid_room_code(code: &str) -> bool {
    proto::ROOM_CODES.contains(&code)
}

fn generate_player_id(rng: &mut impl RandomSource) -> String {
    let a = rng.next_u32();
    let b = rng.next_u32();
    let c = rng.next_u32();
    let d = rng.next_u32();
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:08x}{:04x}",
        a,
        (b >> 16) & 0xffff,
        b & 0x0fff,
        c & 0x0fff,
        c,
        d & 0xffff
    )
}

fn generate_token(rng: &mut impl RandomSource) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut token = String::with_capacity(24);
    for _ in 0..6 {
        let word = rng.next_u32();
        let mut value = word;
        for _ in 0..4 {
            token.push(char::from(ALPHABET[(value & 0x3f) as usize]));
            value >>= 6;
        }
    }
    token
}

// ---------------------------------------------------------------------------
// Game/lifecycle handlers.
// ---------------------------------------------------------------------------

impl ServerState {
    fn player_action(&mut self, shared: &SharedState, conn_id: u64, payload: proto::PlayerAction) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some((code, player_id)) = self.room_player_of(conn_id) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_GAME_NOT_STARTED,
            );
            return;
        };
        let Some(room) = self.rooms.get(&code) else {
            return;
        };
        let Some(seat_index) = room.seat_index(&player_id) else {
            return;
        };
        if room.game.is_none() {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_GAME_NOT_STARTED,
            );
            return;
        }
        let Some(request_id) = request_id else {
            send_error(
                &self.connections,
                conn_id,
                None,
                proto::ErrorCode::InvalidRequest,
                TEXT_ACTION_MISSING_REQUEST,
            );
            return;
        };
        // Duplicate request id: replay the cached action_ack.
        let key = request_key(&request_id);
        if let Some(cached) = self
            .rooms
            .get(&code)
            .and_then(|room| room.seats.get(seat_index))
            .and_then(|seat| seat.processed.get(&key).map(str::to_owned))
        {
            send_text(&self.connections, conn_id, cached);
            return;
        }
        let Some(room) = self.rooms.get(&code) else {
            return;
        };
        let game = room.game.as_ref().expect("checked above");
        if let Some(room_code) = payload.room_code.as_deref() {
            if room_code != room.code {
                self.reject_action(conn_id, seat_index, &code, &request_id, KEY_TEXT_STALE_ROOM);
                return;
            }
        }
        if payload.hand_id != Some(room.hand_id()) {
            self.reject_action(conn_id, seat_index, &code, &request_id, KEY_TEXT_STALE_HAND);
            return;
        }
        if payload.turn_id != Some(room.turn_id()) {
            self.reject_action(conn_id, seat_index, &code, &request_id, KEY_TEXT_STALE_TURN);
            return;
        }
        if payload.expected_state_version != Some(room.state_version) {
            self.reject_action(
                conn_id,
                seat_index,
                &code,
                &request_id,
                KEY_TEXT_STALE_VERSION,
            );
            return;
        }
        let raw_action = payload.action.clone().unwrap_or_default().to_lowercase();
        let legal = game.legal_actions(seat_index);
        let Some(action_type) = poker_engine::ActionType::from_name(&raw_action) else {
            self.reject_action(conn_id, seat_index, &code, &request_id, KEY_TEXT_ILLEGAL);
            return;
        };
        if !legal.contains(action_type) {
            self.reject_action(conn_id, seat_index, &code, &request_id, KEY_TEXT_ILLEGAL);
            return;
        }
        let amount = u64::try_from(payload.amount.max(0)).unwrap_or(0);
        let action = match action_type {
            poker_engine::ActionType::Fold => poker_engine::Action::Fold,
            poker_engine::ActionType::Check => poker_engine::Action::Check,
            poker_engine::ActionType::Call => poker_engine::Action::Call,
            poker_engine::ActionType::Raise => poker_engine::Action::Raise { to: amount },
            poker_engine::ActionType::AllIn => poker_engine::Action::AllIn,
        };
        match self.game_act(&code, seat_index, action) {
            Err(message) => {
                self.reject_action_text(conn_id, seat_index, &code, &request_id, message);
            }
            Ok(()) => {
                self.after_game_change(shared, &code);
                self.finish_action_ack(
                    conn_id,
                    seat_index,
                    &code,
                    &request_id,
                    true,
                    TEXT_ACTION_OK,
                );
            }
        }
    }

    fn game_act(
        &mut self,
        code: &str,
        seat_index: usize,
        action: poker_engine::Action,
    ) -> Result<(), String> {
        let Some(room) = self.rooms.get_mut(code) else {
            return Err(TEXT_ILLEGAL_ACTION.to_owned());
        };
        apply_observed_action(room, seat_index, action).map_err(|error| error.to_string())
    }

    fn reject_action(
        &mut self,
        conn_id: u64,
        seat_index: usize,
        code: &str,
        request_id: &proto::RequestId,
        text: &str,
    ) {
        self.reject_action_text(conn_id, seat_index, code, request_id, text.to_owned());
    }

    fn reject_action_text(
        &mut self,
        conn_id: u64,
        seat_index: usize,
        code: &str,
        request_id: &proto::RequestId,
        text: String,
    ) {
        self.finish_action_ack(conn_id, seat_index, code, request_id, false, &text);
    }

    /// Builds, caches and sends the action_ack; on rejection also re-sends the
    /// authoritative game_state to the actor (reference behaviour).
    fn finish_action_ack(
        &mut self,
        conn_id: u64,
        seat_index: usize,
        code: &str,
        request_id: &proto::RequestId,
        accepted: bool,
        text: &str,
    ) {
        let Some(room) = self.rooms.get(code) else {
            return;
        };
        let reply = proto::ServerMessage::ActionAck(wire::action_ack(
            request_id,
            accepted,
            text.to_owned(),
            room,
        ));
        if accepted {
            ServerMetrics::inc(&self.metrics.actions_accepted);
        } else {
            ServerMetrics::inc(&self.metrics.actions_rejected);
            debug!(room_code = %room.code, message = %text, "action rejected");
        }
        let text = reply.to_json_string();
        if let Some(room) = self.rooms.get_mut(code) {
            if let Some(seat) = room.seats.get_mut(seat_index) {
                seat.processed.insert(request_key(request_id), text.clone());
            }
        }
        send_text(&self.connections, conn_id, text);
        if !accepted {
            if let Some(room) = self.rooms.get(code) {
                if let Some(snapshot) = wire::game_state(room, seat_index) {
                    send_message(
                        &self.connections,
                        conn_id,
                        &proto::ServerMessage::GameState(snapshot),
                    );
                }
            }
        }
    }

    fn ready_for_next(&mut self, shared: &SharedState, conn_id: u64, payload: proto::ReadyForNext) {
        let request_id = Self::request_id_of(&payload.envelope);
        let Some((code, player_id)) = self.room_player_of(conn_id) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_GAME_NOT_STARTED,
            );
            return;
        };
        let Some(room) = self.rooms.get(&code) else {
            return;
        };
        if room.game.is_none() {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_GAME_NOT_STARTED,
            );
            return;
        }
        if payload.hand_id != Some(room.hand_id()) {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_OLD_HAND_READY,
            );
            return;
        }
        if !room.idle_after_hand() {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_HAND_NOT_ENDED,
            );
            return;
        }
        if let Some(room) = self.rooms.get_mut(&code) {
            room.ready.insert(player_id);
        }
        if let Some(request_id) = request_id.as_ref() {
            let Some(room) = self.rooms.get(&code) else {
                return;
            };
            send_ack(&self.connections, conn_id, request_id, true, Some(room));
        }
        let all_ready = {
            let Some(room) = self.rooms.get(&code) else {
                return;
            };
            let humans: Vec<usize> = room
                .seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| seat.is_human && seat.connected && !seat.left && seat.stack > 0)
                .map(|(index, _)| index)
                .collect();
            !humans.is_empty()
                && humans
                    .iter()
                    .all(|index| room.ready.contains(&room.seats[*index].id))
        };
        let scheduled = all_ready && self.try_start_next_hand(shared, &code);
        if let Some(room) = self.rooms.get(&code) {
            broadcast_ready(room, &self.connections, scheduled);
        }
    }

    fn resume_session(
        &mut self,
        shared: &SharedState,
        conn_id: u64,
        payload: proto::ResumeSession,
    ) {
        let request_id = Self::request_id_of(&payload.envelope);
        if self
            .connections
            .get(&conn_id)
            .is_some_and(|meta| meta.room_code.is_some())
        {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::InvalidRequest,
                TEXT_RESUME_ALREADY_IN_ROOM,
            );
            return;
        }
        let token = payload.resume_token.unwrap_or_default();
        let Some(session) = self.sessions.get(&token) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::ResumeExpired,
                TEXT_RESUME_EXPIRED,
            );
            return;
        };
        let (code, player_id) = (session.room_code.clone(), session.player_id.clone());
        let Some(room) = self.rooms.get(&code) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::ResumeExpired,
                TEXT_RESUME_ROOM_GONE,
            );
            return;
        };
        let Some(seat_index) = room.seat_index(&player_id) else {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::ResumeExpired,
                TEXT_RESUME_ROOM_GONE,
            );
            return;
        };
        if room.seats[seat_index].left {
            send_error(
                &self.connections,
                conn_id,
                request_id,
                proto::ErrorCode::ResumeExpired,
                TEXT_RESUME_ROOM_GONE,
            );
            return;
        }
        let old_conn = room.seats[seat_index].conn_id;
        if let Some(old_conn) = old_conn {
            if old_conn != conn_id {
                if let Some(meta) = self.connections.remove(&old_conn) {
                    let _ = meta.tx.try_send(Message::Close(Some(CloseFrame {
                        code: proto::CLOSE_SESSION_REPLACED,
                        reason: proto::CLOSE_REASON_SESSION_REPLACED.into(),
                    })));
                    ServerMetrics::inc(&meta.metrics.session_kicks);
                    info!(room_code = %code, "session resumed elsewhere; old socket kicked");
                }
            }
        }
        ServerMetrics::inc(&self.metrics.resumes);
        info!(room_code = %code, "session resumed");
        if let Some(room) = self.rooms.get_mut(&code) {
            room.cleanup_token = None;
            let seat = &mut room.seats[seat_index];
            seat.connected = true;
            seat.conn_id = Some(conn_id);
            seat.disconnect_token = None;
        }
        if let Some(meta) = self.connections.get_mut(&conn_id) {
            meta.room_code = Some(code.clone());
            meta.player_id = Some(player_id);
            meta.session_token = Some(token);
        }
        {
            let ServerState {
                rooms, connections, ..
            } = self;
            let Some(room) = rooms.get_mut(&code) else {
                return;
            };
            room.state_version += 1;
            if let Some(info) = wire::room_info(room, seat_index, true) {
                send_message(
                    connections,
                    conn_id,
                    &proto::ServerMessage::RoomJoined(info),
                );
            }
            broadcast_lobby(room, connections);
            broadcast_game(room, connections);
        }
        let idle_after_hand = self.rooms.get(&code).is_some_and(Room::idle_after_hand);
        if idle_after_hand {
            let scheduled = self.try_start_next_hand(shared, &code);
            if let Some(room) = self.rooms.get(&code) {
                broadcast_ready(room, &self.connections, scheduled);
            }
        }
    }

    /// Called after every authoritative game mutation: bumps stateVersion,
    /// mirrors engine seats, broadcasts snapshots, fires hand-end readiness and
    /// schedules the next AI turn.
    fn after_game_change(&mut self, shared: &SharedState, code: &str) {
        let ServerState {
            rooms,
            connections,
            options,
            ..
        } = self;
        let Some(room) = rooms.get_mut(code) else {
            return;
        };
        room.sync_from_game();
        room.state_version += 1;
        broadcast_game(room, connections);
        if room.idle_after_hand() {
            room.ready.clear();
            broadcast_ready(room, connections, false);
            room.ai_token = None;
            return;
        }
        let current = room
            .game
            .as_ref()
            .map_or(-1, poker_engine::Game::current_player_idx);
        let ai_seat = usize::try_from(current)
            .ok()
            .and_then(|index| room.seats.get(index).map(|seat| (index, !seat.is_human)));
        match ai_seat {
            Some((_index, true)) => {
                let delay = u64::try_from(room.config.ai_delay).unwrap_or(0);
                let token = room.alloc_token();
                room.ai_token = Some(token);
                let _ = options;
                spawn_ai_timer(shared, code.to_owned(), token, Duration::from_millis(delay));
            }
            _ => room.ai_token = None,
        }
    }

    fn migrate_host(&mut self, _shared: &SharedState, code: &str, immediate: bool) -> bool {
        let (old_host, new_host) = {
            let Some(room) = self.rooms.get(code) else {
                return false;
            };
            let current = room
                .host_id
                .as_ref()
                .and_then(|id| room.seats.iter().find(|seat| &seat.id == id));
            if let Some(seat) = current {
                if seat.connected && !seat.left {
                    return false;
                }
            }
            let _ = immediate;
            (room.host_id.clone(), room.next_host())
        };
        if old_host == new_host {
            return false;
        }
        let changed = {
            let Some(room) = self.rooms.get_mut(code) else {
                return false;
            };
            room.host_id = new_host;
            room.state_version += 1;
            true
        };
        if changed {
            let ServerState {
                rooms, connections, ..
            } = self;
            if let Some(room) = rooms.get(code) {
                broadcast_host_changed(room, connections);
                broadcast_game(room, connections);
            }
        }
        changed
    }

    /// Folds a disconnected/left seat: finishes the hand when it was the last
    /// live player or the current actor, otherwise just re-broadcasts.
    fn fold_disconnected(&mut self, shared: &SharedState, code: &str, seat_index: usize) -> bool {
        {
            let Some(room) = self.rooms.get_mut(code) else {
                return false;
            };
            let Some(game) = room.game.as_ref() else {
                return false;
            };
            if game.phase() == poker_engine::Phase::Idle {
                return false;
            }
            let Some(seat) = room.seats.get(seat_index) else {
                return false;
            };
            if !seat.dealt_in || seat.folded || seat.is_all_in {
                return false;
            }
            let player_id = seat.id.clone();
            room.seats[seat_index].folded = true;
            room.seats[seat_index].last_action = Some(poker_engine::LastAction {
                action: poker_engine::ActionLabel::Fold,
                amount: 0,
            });
            let Some(game) = room.game.as_mut() else {
                return false;
            };
            if let Some(player) = game
                .players_mut()
                .iter_mut()
                .find(|player| player.id == player_id)
            {
                player.folded = true;
                player.last_action = Some(poker_engine::LastAction {
                    action: poker_engine::ActionLabel::Fold,
                    amount: 0,
                });
            }
            // Decided after the engine fold: as in server-game.js, finishing by
            // fold happens when one seat remains, and advancing happens when the
            // folded seat was the one to act.
            let in_hand = game.in_hand().len();
            let current = game.current_player_idx();
            if in_hand <= 1 || current == i32::try_from(seat_index).unwrap_or(-1) {
                game.advance_after_action();
            }
        }
        self.after_game_change(shared, code);
        true
    }

    /// Replaces left human seats with AI seats between hands.
    fn refill_vacated_seats(&mut self, code: &str) {
        let factory = self.options.ai_factory.clone();
        let ServerState {
            rooms, sessions, ..
        } = self;
        let Some(room) = rooms.get_mut(code) else {
            return;
        };
        {
            let Some(game) = room.game.as_ref() else {
                return;
            };
            if game.phase() != poker_engine::Phase::Idle {
                return;
            }
        }
        let starting_stack = u64::try_from(room.config.starting_stack).unwrap_or(0);
        for index in 0..room.seats.len() {
            if !room.seats[index].is_human || !room.seats[index].left {
                continue;
            }
            let token = room.seats[index].resume_token.clone();
            if !token.is_empty() {
                sessions.remove(&token);
            }
            let seat_id = room.seats[index].seat_id;
            room.seats[index] = Seat::ai(seat_id, starting_stack, factory.as_ref());
        }
        let players: Vec<poker_engine::Player> =
            room.seats.iter().map(Seat::to_engine_player).collect();
        if let Some(game) = room.game.as_mut() {
            game.set_players(players);
        }
    }

    fn try_start_next_hand(&mut self, shared: &SharedState, code: &str) -> bool {
        {
            let Some(room) = self.rooms.get(code) else {
                return false;
            };
            if room.next_hand_token.is_some() {
                return true;
            }
            if !room.idle_after_hand() {
                return false;
            }
            let humans: Vec<usize> = room
                .seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| seat.is_human && seat.connected && !seat.left && seat.stack > 0)
                .map(|(index, _)| index)
                .collect();
            if humans.is_empty()
                || !humans
                    .iter()
                    .all(|index| room.ready.contains(&room.seats[*index].id))
            {
                return false;
            }
        }
        self.refill_vacated_seats(code);
        let active = self.rooms.get(code).map_or(0, Room::active_for_new_hand);
        if active < 2 {
            return false;
        }
        let token = {
            let Some(room) = self.rooms.get_mut(code) else {
                return false;
            };
            let token = room.alloc_token();
            room.next_hand_token = Some(token);
            token
        };
        spawn_next_hand_timer(shared, code.to_owned(), token);
        true
    }

    fn on_next_hand_deadline(&mut self, shared: &SharedState, code: &str, token: u64) {
        {
            let Some(room) = self.rooms.get_mut(code) else {
                return;
            };
            if room.next_hand_token != Some(token) {
                return;
            }
            room.next_hand_token = None;
            if !room.idle_after_hand() {
                return;
            }
        }
        let all_ready = {
            let Some(room) = self.rooms.get(code) else {
                return;
            };
            let humans: Vec<usize> = room
                .seats
                .iter()
                .enumerate()
                .filter(|(_, seat)| seat.is_human && seat.connected && !seat.left && seat.stack > 0)
                .map(|(index, _)| index)
                .collect();
            !humans.is_empty()
                && humans
                    .iter()
                    .all(|index| room.ready.contains(&room.seats[*index].id))
        };
        if !all_ready {
            if let Some(room) = self.rooms.get(code) {
                broadcast_ready(room, &self.connections, false);
            }
            return;
        }
        self.refill_vacated_seats(code);
        let active = self.rooms.get(code).map_or(0, Room::active_for_new_hand);
        if active < 2 {
            if let Some(room) = self.rooms.get(code) {
                broadcast_ready(room, &self.connections, false);
            }
            return;
        }
        let started = {
            let Some(room) = self.rooms.get_mut(code) else {
                return;
            };
            room.ready.clear();
            let Some(game) = room.game.as_mut() else {
                return;
            };
            game.start_hand(&mut room.rng)
        };
        if !started {
            if let Some(room) = self.rooms.get(code) {
                broadcast_ready(room, &self.connections, false);
            }
            return;
        }
        self.after_game_change(shared, code);
    }

    fn cleanup_room(&mut self, shared: &SharedState, code: &str) {
        let (has_connected, has_pending_reconnect, has_game, already) = {
            let Some(room) = self.rooms.get(code) else {
                return;
            };
            let has_connected = room
                .seats
                .iter()
                .any(|seat| seat.is_human && seat.connected && !seat.left);
            // A socket that just dropped during its disconnect grace window can
            // still resume. Keeping the game-less room alive until that timer
            // fires makes lobby reconnect/resume work; the reference JS server
            // deletes such rooms immediately, which loses the resume race.
            let has_pending_reconnect = room.seats.iter().any(|seat| {
                seat.is_human && !seat.connected && !seat.left && seat.disconnect_token.is_some()
            });
            (
                has_connected,
                has_pending_reconnect,
                room.game.is_some(),
                room.cleanup_token.is_some(),
            )
        };
        if has_connected || has_pending_reconnect {
            if let Some(room) = self.rooms.get_mut(code) {
                room.cleanup_token = None;
            }
            return;
        }
        if !has_game {
            let Some(room) = self.rooms.remove(code) else {
                return;
            };
            ServerMetrics::inc(&self.metrics.rooms_closed);
            info!(room_code = %code, "room closed (never started)");
            for seat in &room.seats {
                if !seat.resume_token.is_empty() {
                    self.sessions.remove(&seat.resume_token);
                }
            }
            return;
        }
        if already {
            return;
        }
        let token = {
            let Some(room) = self.rooms.get_mut(code) else {
                return;
            };
            let token = room.alloc_token();
            room.cleanup_token = Some(token);
            token
        };
        let ttl = self.options.empty_room_ttl();
        spawn_cleanup_timer(shared, code.to_owned(), token, ttl);
    }

    fn on_cleanup_deadline(&mut self, _shared: &SharedState, code: &str, token: u64) {
        let removable = {
            let Some(room) = self.rooms.get_mut(code) else {
                return;
            };
            if room.cleanup_token != Some(token) {
                return;
            }
            let has_connected = room
                .seats
                .iter()
                .any(|seat| seat.is_human && seat.connected && !seat.left);
            let has_pending_reconnect = room.seats.iter().any(|seat| {
                seat.is_human && !seat.connected && !seat.left && seat.disconnect_token.is_some()
            });
            !has_connected && !has_pending_reconnect
        };
        if !removable {
            return;
        }
        let Some(room) = self.rooms.remove(code) else {
            return;
        };
        ServerMetrics::inc(&self.metrics.rooms_closed);
        info!(room_code = %code, "room closed (empty TTL)");
        for seat in &room.seats {
            if !seat.resume_token.is_empty() {
                self.sessions.remove(&seat.resume_token);
            }
        }
    }

    fn on_disconnect(&mut self, shared: &SharedState, conn_id: u64) {
        let Some(meta) = self.connections.remove(&conn_id) else {
            return;
        };
        let (Some(code), Some(player_id)) = (meta.room_code.clone(), meta.player_id.clone()) else {
            return;
        };
        let grace = self.options.disconnect_grace();
        let (seat_index, token) = {
            let ServerState {
                rooms, connections, ..
            } = self;
            let Some(room) = rooms.get_mut(&code) else {
                return;
            };
            let Some(seat_index) = room.seat_index(&player_id) else {
                return;
            };
            if room.seats[seat_index].conn_id != Some(conn_id) {
                return;
            }
            room.seats[seat_index].connected = false;
            room.seats[seat_index].conn_id = None;
            room.state_version += 1;
            broadcast_player_left(room, connections);
            broadcast_game(room, connections);
            let token = room.alloc_token();
            room.seats[seat_index].disconnect_token = Some(token);
            (seat_index, token)
        };
        spawn_disconnect_timer(shared, code.clone(), player_id, seat_index, token, grace);
        self.cleanup_room(shared, &code);
    }

    fn on_disconnect_deadline(
        &mut self,
        shared: &SharedState,
        code: &str,
        player_id: &str,
        seat_index: usize,
        token: u64,
    ) {
        {
            let ServerState {
                rooms, sessions, ..
            } = self;
            let Some(room) = rooms.get_mut(code) else {
                return;
            };
            let Some(current_index) = room.seat_index(player_id) else {
                return;
            };
            if current_index != seat_index {
                return;
            }
            let seat = &room.seats[seat_index];
            if seat.disconnect_token != Some(token) || seat.connected || seat.left {
                return;
            }
            let resume_token = seat.resume_token.clone();
            room.seats[seat_index].disconnect_token = None;
            room.seats[seat_index].left = true;
            room.seats[seat_index].connected = false;
            if !resume_token.is_empty() {
                sessions.remove(&resume_token);
            }
        }
        self.fold_disconnected(shared, code, seat_index);
        self.migrate_host(shared, code, false);
        {
            let ServerState {
                rooms, connections, ..
            } = self;
            let Some(room) = rooms.get_mut(code) else {
                return;
            };
            room.state_version += 1;
            broadcast_player_left(room, connections);
            broadcast_game(room, connections);
        }
        self.try_start_next_hand(shared, code);
        self.cleanup_room(shared, code);
    }
}

const KEY_TEXT_STALE_ROOM: &str = TEXT_STALE_ROOM;
const KEY_TEXT_STALE_HAND: &str = TEXT_STALE_HAND;
const KEY_TEXT_STALE_TURN: &str = TEXT_STALE_TURN;
const KEY_TEXT_STALE_VERSION: &str = TEXT_STALE_VERSION;
const KEY_TEXT_ILLEGAL: &str = TEXT_ILLEGAL_ACTION;

// ---------------------------------------------------------------------------
// AI decision task, timer tasks and AI view construction.
// ---------------------------------------------------------------------------

fn build_ai_view(room: &Room, seat_index: usize) -> Option<AiView> {
    let game = room.game.as_ref()?;
    let seat = room.seats.get(seat_index)?;
    let legal = game.legal_actions(seat_index);
    let opponent_stacks = game
        .in_hand()
        .into_iter()
        .filter(|index| *index != seat_index)
        .map(|index| game.players()[index].stack)
        .collect();
    Some(AiView {
        seat: seat_index,
        context: crate::ai::decision_context(game, seat_index),
        seat_count: room.seats.len(),
        starting_stack: u64::try_from(room.config.starting_stack).unwrap_or(0),
        stack: seat.stack,
        pot: game.pot(),
        current_bet: game.current_bet(),
        to_call: legal.to_call,
        min_raise_to: legal.min_raise_to,
        max_raise_to: legal.max_raise_to,
        phase: game.phase(),
        community_cards: game.community_cards().to_vec(),
        hole_cards: seat.hole_cards.clone(),
        opponent_stacks,
        hand_id: game.hand_id(),
        turn_id: game.turn_id(),
        minimum_bet: game.minimum_bet(),
        wager_unit: game.wager_unit(),
        is_short_deck: game.is_short_deck(),
        legal_actions: legal,
    })
}

fn fallback_action(game: &poker_engine::Game, seat_index: usize) -> Option<poker_engine::Action> {
    use poker_engine::ActionType;
    let legal = game.legal_actions(seat_index);
    if legal.contains(ActionType::Check) {
        return Some(poker_engine::Action::Check);
    }
    if legal.contains(ActionType::Call) {
        return Some(poker_engine::Action::Call);
    }
    if legal.contains(ActionType::Fold) {
        return Some(poker_engine::Action::Fold);
    }
    None
}

fn spawn_ai_timer(shared: &SharedState, code: String, token: u64, delay: Duration) {
    let shared = shared.clone();
    tokio::spawn(async move {
        if !delay.is_zero() {
            sleep(delay).await;
        }
        run_ai_turn(shared, code, token).await;
    });
}

/// Runs one AI decision off the async reactor and applies it after revalidation.
async fn run_ai_turn(shared: SharedState, code: String, token: u64) {
    let prepared = {
        let mut guard = lock(&shared);
        if guard.shutdown {
            return;
        }
        let Some(room) = guard.rooms.get_mut(&code) else {
            return;
        };
        if room.ai_token != Some(token) {
            return;
        }
        room.ai_token = None;
        let Some(game) = room.game.as_ref() else {
            return;
        };
        if game.phase() == poker_engine::Phase::Idle {
            return;
        }
        let current = game.current_player_idx();
        let Ok(seat_index) = usize::try_from(current) else {
            return;
        };
        let Some(seat) = room.seats.get_mut(seat_index) else {
            return;
        };
        if seat.is_human {
            return;
        }
        let Some(brain) = seat.ai.take() else { return };
        let Some(view) = build_ai_view(room, seat_index) else {
            if let Some(seat) = room.seats.get_mut(seat_index) {
                seat.ai = Some(brain);
            }
            return;
        };
        let hand_id = game.hand_id();
        let turn_id = game.turn_id();
        (seat_index, brain, view, hand_id, turn_id)
    };
    let (seat_index, brain, view, hand_id, turn_id) = prepared;
    let decision_started = Instant::now();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut brain = brain;
        let decision = brain.decide(&view);
        (brain, decision)
    })
    .await;
    let Ok((brain, decision)) = outcome else {
        return;
    };
    let mut guard = lock(&shared);
    if guard.shutdown {
        return;
    }
    let applied = {
        let ServerState { rooms, .. } = &mut *guard;
        let Some(room) = rooms.get_mut(&code) else {
            return;
        };
        let Some(seat) = room.seats.get_mut(seat_index) else {
            return;
        };
        if seat.ai.is_some() {
            return;
        }
        seat.ai = Some(brain);
        let Some(game) = room.game.as_mut() else {
            return;
        };
        if game.hand_id() != hand_id
            || game.turn_id() != turn_id
            || game.current_player_idx() != i32::try_from(seat_index).unwrap_or(-1)
            || game.phase() == poker_engine::Phase::Idle
        {
            return;
        }
        let action = decision
            .map(|decision| decision.action)
            .or_else(|| fallback_action(game, seat_index));
        let Some(action) = action else { return };
        if apply_observed_action(room, seat_index, action).is_ok() {
            true
        } else if let Some(fallback) = room
            .game
            .as_ref()
            .and_then(|game| fallback_action(game, seat_index))
        {
            apply_observed_action(room, seat_index, fallback).is_ok()
        } else {
            false
        }
    };
    if applied {
        let elapsed_ms = u64::try_from(decision_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let metrics = guard.metrics.clone();
        ServerMetrics::inc(&metrics.ai_decisions);
        ServerMetrics::add(&metrics.ai_decision_total_ms, elapsed_ms);
        ServerMetrics::observe_max(&metrics.ai_decision_max_ms, elapsed_ms);
        debug!(decision_ms = elapsed_ms, "ai decision applied");
        guard.after_game_change(&shared, &code);
    }
}

fn spawn_next_hand_timer(shared: &SharedState, code: String, token: u64) {
    let shared = shared.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(proto::NEXT_HAND_DELAY_MS)).await;
        let mut guard = lock(&shared);
        if guard.shutdown {
            return;
        }
        guard.on_next_hand_deadline(&shared, &code, token);
    });
}

fn spawn_disconnect_timer(
    shared: &SharedState,
    code: String,
    player_id: String,
    seat_index: usize,
    token: u64,
    grace: Duration,
) {
    let shared = shared.clone();
    tokio::spawn(async move {
        sleep(grace).await;
        let mut guard = lock(&shared);
        if guard.shutdown {
            return;
        }
        guard.on_disconnect_deadline(&shared, &code, &player_id, seat_index, token);
    });
}

fn spawn_cleanup_timer(shared: &SharedState, code: String, token: u64, ttl: Duration) {
    let shared = shared.clone();
    tokio::spawn(async move {
        sleep(ttl).await;
        let mut guard = lock(&shared);
        if guard.shutdown {
            return;
        }
        guard.on_cleanup_deadline(&shared, &code, token);
    });
}

fn apply_observed_action(
    room: &mut Room,
    seat: usize,
    action: poker_engine::Action,
) -> Result<(), poker_engine::ActError> {
    let game = room.game.as_mut().expect("active game");
    let event = poker_ai::host::act(game, seat, action)?;
    let revealed = if game.phase() == poker_engine::Phase::Idle {
        poker_ai::host::showdown(game)
    } else {
        Vec::new()
    };
    for seat in &mut room.seats {
        if let Some(brain) = &mut seat.ai {
            brain.observe(&event);
            for (seat, result) in &revealed {
                brain.showdown(*seat, result);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_servers_and_rooms_have_independent_random_streams() {
        let mut a = ServerState::new(ServerOptions::default());
        let mut b = ServerState::new(ServerOptions::default());
        assert_ne!(generate_token(&mut a.id_rng), generate_token(&mut b.id_rng));
        let mut room_a = a.room_rng();
        let mut room_b = a.room_rng();
        let first: Vec<_> = (0..16).map(|_| room_a.next_u32()).collect();
        let second: Vec<_> = (0..16).map(|_| room_b.next_u32()).collect();
        assert_ne!(first, second);
    }

    #[test]
    fn zero_seed_server_remains_reproducible_without_repeating_ids() {
        let options = ServerOptions {
            rng_seed: Some(0),
            ..Default::default()
        };
        let mut a = ServerState::new(options.clone());
        let mut b = ServerState::new(options);
        let first = generate_player_id(&mut a.id_rng);
        assert_eq!(first, generate_player_id(&mut b.id_rng));
        let next = generate_player_id(&mut a.id_rng);
        assert_ne!(first, next);
        assert_eq!(next, generate_player_id(&mut b.id_rng));
    }

    #[test]
    fn origin_allowlist_matches_reference_semantics() {
        let allowed = vec!["https://cards.example".to_owned()];
        assert!(origin_allowed(None, &allowed));
        assert!(origin_allowed(Some("null"), &allowed));
        assert!(origin_allowed(Some("file:///app/index.html"), &allowed));
        assert!(origin_allowed(Some("https://cards.example"), &allowed));
        assert!(origin_allowed(Some("http://localhost:3000"), &allowed));
        assert!(origin_allowed(Some("http://127.0.0.1:3000"), &allowed));
        assert!(origin_allowed(Some("http://[::1]:3000"), &allowed));
        assert!(!origin_allowed(Some("https://untrusted.example"), &allowed));
        assert!(!origin_allowed(Some("not a url"), &allowed));
        assert!(!origin_allowed(
            Some("https://evil.localhost.evil"),
            &allowed
        ));
    }

    #[test]
    fn client_ip_honours_trust_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.5, 10.0.0.1".parse().expect("header"),
        );
        let peer: SocketAddr = "127.0.0.1:5000".parse().expect("peer");
        assert_eq!(client_ip(&headers, peer, true), "203.0.113.5");
        assert_eq!(client_ip(&headers, peer, false), "127.0.0.1");
        let empty = HeaderMap::new();
        assert_eq!(client_ip(&empty, peer, true), "127.0.0.1");
    }

    #[test]
    fn generated_tokens_and_player_ids_match_the_wire_shape() {
        let mut rng = RuntimeRng::from_optional_state(Some(0x1234_5678));
        let token = generate_token(&mut rng);
        assert_eq!(token.len(), 24);
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        let id = generate_player_id(&mut rng);
        assert_eq!(id.len(), 36);
        assert_eq!(&id[8..9], "-");
        assert_eq!(&id[13..14], "-");
        assert_eq!(&id[14..15], "4");
        assert_eq!(&id[18..19], "-");
        assert_eq!(&id[19..20], "8");
        assert_eq!(&id[23..24], "-");
    }

    #[test]
    fn room_option_clamping_follows_reference_ranges() {
        let options = ServerOptions {
            ai_delay_ms: 99_999,
            max_rooms: 0,
            max_connections: 1_000,
            max_messages_per_window: 10,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(options.ai_delay_ms, proto::SERVER_AI_DELAY_MAX_MS);
        assert_eq!(options.max_rooms, proto::MAX_ROOMS_MIN);
        assert_eq!(options.max_connections, proto::MAX_CONNECTIONS_MAX);
        assert_eq!(
            options.max_messages_per_window,
            proto::MAX_MESSAGES_PER_WINDOW_MIN
        );
    }

    #[test]
    fn outbound_overflow_marks_slow_consumer() {
        let metrics = Arc::new(ServerMetrics::default());
        let (tx, _rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("warmup".into()))
            .expect("first frame fits the test queue");
        let failed = Arc::new(AtomicBool::new(false));
        let meta = ConnectionMeta {
            ip: "127.0.0.1".to_owned(),
            tx,
            failed: failed.clone(),
            alive: true,
            metrics: metrics.clone(),
            room_code: None,
            player_id: None,
            session_token: None,
            rate_started: Instant::now(),
            message_count: 0,
        };
        let connections = HashMap::from([(1u64, meta)]);
        let hello = proto::ServerMessage::Hello(proto::Hello {
            protocol_version: proto::PROTOCOL_VERSION,
            server_id: proto::SERVER_ID.to_owned(),
            server_name: None,
            app_version: proto::COMPAT_APP_VERSION.to_owned(),
        });
        send_message(&connections, 1, &hello);
        assert!(
            failed.load(Ordering::Relaxed),
            "slow consumer must be flagged"
        );
        assert_eq!(metrics.snapshot().slow_consumers, 1);
    }
}
