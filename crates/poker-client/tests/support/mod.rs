//! Shared helpers for the poker-client integration tests: a scripted
//! tokio-tungstenite mock server and loaders for the golden protocol corpus.
#![allow(dead_code)]

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{WebSocketStream, accept_async};

use poker_client::{Client, ClientConfig, ClientEvent, FixedHooks, SessionCredentials};

/// Server side of one mock WebSocket connection.
pub type ServerWs = WebSocketStream<TcpStream>;

/// Scripted mock server.
///
/// Connections are handled strictly one at a time (accept, run the behaviour,
/// then accept the next one), which is enough for every reconnect scenario and
/// avoids task leaks between assertions.
pub struct TestServer {
    /// ws:// URL clients should connect to.
    pub url: String,
    /// Number of accepted TCP connections so far.
    pub connections: Arc<AtomicUsize>,
    /// Every client text frame recorded by the behaviours.
    pub received: mpsc::UnboundedReceiver<Value>,
    handle: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Starts a mock server; behaviour receives (connection index, socket, frame sink).
pub async fn serve<F, Fut>(mut behaviour: F) -> TestServer
where
    F: FnMut(usize, ServerWs, mpsc::UnboundedSender<Value>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let address = listener.local_addr().expect("mock server address");
    let (frame_tx, frame_rx) = mpsc::unbounded_channel();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    let handle = tokio::spawn(async move {
        loop {
            let Ok((tcp, _peer)) = listener.accept().await else {
                break;
            };
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let Ok(ws) = accept_async(tcp).await else {
                continue;
            };
            // Drop the HTTP response value; accept_async returns it.
            behaviour(index, ws, frame_tx.clone()).await;
        }
    });
    TestServer {
        url: format!("ws://{address}"),
        connections,
        received: frame_rx,
        handle,
    }
}

/// Absolute path of a golden protocol corpus file.
pub fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/protocol")
        .join(name)
}

/// Reads one golden JSON document.
pub fn golden_json(name: &str) -> Value {
    let text = std::fs::read_to_string(golden_path(name)).expect("read golden json");
    serde_json::from_str(&text).expect("parse golden json")
}

/// Reads a golden JSONL corpus into values.
pub fn golden_lines(name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(golden_path(name)).expect("read golden jsonl");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse golden line"))
        .collect()
}

/// Reads a wrapped golden message corpus (s2c/c2s jsonl) and returns the
/// inner \"message\" objects.
pub fn golden_message_lines(name: &str) -> Vec<Value> {
    golden_lines(name)
        .into_iter()
        .map(|entry| entry["message"].clone())
        .collect()
}

/// The reference server hello payload.
pub fn hello_json(app_version: &str) -> Value {
    json!({
        "type": "hello",
        "protocolVersion": 2,
        "serverId": "texas-holdem-game",
        "serverName": "德州扑克服务",
        "appVersion": app_version,
    })
}

/// Sends a JSON value as one text frame.
pub async fn send_value(ws: &mut ServerWs, value: &Value) {
    let text = serde_json::to_string(value).expect("serialize frame");
    ws.send(Message::text(text)).await.expect("send frame");
}

/// Sends the reference hello frame.
pub async fn send_hello(ws: &mut ServerWs) {
    send_value(ws, &json!(hello_json("1.5.8"))).await;
}

/// Sends a raw text frame.
pub async fn send_text(ws: &mut ServerWs, text: &str) {
    ws.send(Message::text(text.to_owned()))
        .await
        .expect("send text frame");
}

/// Sends a close frame with a code and reason.
pub async fn send_close(ws: &mut ServerWs, code: u16, reason: &str) {
    let frame = CloseFrame {
        code: CloseCode::from(code),
        reason: reason.to_owned().into(),
    };
    let _ = ws.send(Message::Close(Some(frame))).await;
}

/// Reads the next client JSON frame, ignoring control frames.
pub async fn read_value(ws: &mut ServerWs) -> Option<Value> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(text.as_str()).ok();
            }
            Some(Ok(
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_),
            )) => {}
            Some(Ok(Message::Close(_))) | None => return None,
            Some(Err(_)) => return None,
        }
    }
}

/// Reads frames until the client closes, recording them into the sink.
pub async fn record_until_closed(ws: &mut ServerWs, sink: &mpsc::UnboundedSender<Value>) {
    while let Some(value) = read_value(ws).await {
        let _ = sink.send(value);
    }
}

/// Records one frame.
pub fn record(sink: &mpsc::UnboundedSender<Value>, value: Value) {
    let _ = sink.send(value);
}

/// Deterministic client configuration for the mock server.
pub fn test_config(server: &TestServer) -> ClientConfig {
    ClientConfig::new(server.url.clone())
        .with_hooks(Arc::new(FixedHooks::new(1_700_000_000_000, 0.5)))
        .without_handshake_timeout()
}

/// Session credentials for the mock server.
pub fn test_session(server: &TestServer) -> SessionCredentials {
    SessionCredentials::new(server.url.clone(), "1", "resume-token-0123456789")
        .with_player_id("player-1")
}

/// Awaits the next client event, panicking when the stream ended unexpectedly.
pub async fn next_event(client: &mut Client) -> ClientEvent {
    client.next_event().await.expect("event stream ended")
}

/// Awaits events until the predicate matches (inclusive) or the limit is hit.
pub async fn events_until<F>(
    client: &mut Client,
    limit: usize,
    mut predicate: F,
) -> Vec<ClientEvent>
where
    F: FnMut(&ClientEvent) -> bool,
{
    let mut events = Vec::new();
    for _ in 0..limit {
        let event = next_event(client).await;
        let matched = predicate(&event);
        events.push(event);
        if matched {
            return events;
        }
    }
    panic!("predicate did not match within {limit} events: {events:?}");
}

/// Waits for one recorded client frame with a timeout.
pub async fn next_recorded(server: &mut TestServer, timeout: Duration) -> Value {
    tokio::time::timeout(timeout, server.received.recv())
        .await
        .expect("recorded client frame timed out")
        .expect("recorded channel closed")
}

/// Collects all currently available recorded frames (bounded by a short wait).
pub async fn drain_recorded(server: &mut TestServer, expected: usize) -> Vec<Value> {
    let mut frames = Vec::new();
    while frames.len() < expected {
        let value = tokio::time::timeout(Duration::from_secs(2), server.received.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out after {} frames", frames.len()))
            .expect("recorded channel closed");
        frames.push(value);
    }
    frames
}

/// A minimal valid room_joined message taken from the golden corpus.
pub fn room_joined_json(room_code: &str, resumed: bool) -> Value {
    let lines = golden_message_lines("s2c-messages.jsonl");
    let mut message = lines
        .into_iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("room_joined"))
        .expect("golden room_joined message");
    if let Value::Object(map) = &mut message {
        map.insert("roomCode".to_owned(), Value::from(room_code));
        map.insert("resumed".to_owned(), Value::Bool(resumed));
    }
    message
}
