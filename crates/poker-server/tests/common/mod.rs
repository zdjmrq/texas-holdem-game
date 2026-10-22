//! Shared WebSocket test client and payload helpers.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// Default wait budget for a single expected message.
pub const WAIT: Duration = Duration::from_secs(4);

#[derive(Default)]
struct Inbox {
    messages: Vec<Value>,
    closed: Option<(u16, String)>,
}

/// Test client that keeps a background reader so waits never miss messages.
pub struct TestClient {
    sink: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    inbox: Arc<Mutex<Inbox>>,
    notify: Arc<tokio::sync::Notify>,
    reader: tokio::task::JoinHandle<()>,
}

impl TestClient {
    /// Connects and waits for the WebSocket handshake.
    pub async fn connect(url: &str) -> Self {
        Self::connect_with_origin(url, None).await
    }

    /// Connects with an explicit Origin header.
    pub async fn connect_with_origin(url: &str, origin: Option<&str>) -> Self {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = url.into_client_request().expect("valid ws url");
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().expect("valid origin header"));
        }
        let (socket, _response) = connect_async(request).await.expect("websocket handshake");
        let (sink, mut stream) = socket.split();
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let notify = Arc::new(tokio::sync::Notify::new());
        let reader_inbox = inbox.clone();
        let reader_notify = notify.clone();
        let reader = tokio::spawn(async move {
            while let Some(incoming) = stream.next().await {
                match incoming {
                    Ok(Message::Text(text)) => {
                        if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
                            reader_inbox.lock().unwrap().messages.push(value);
                            reader_notify.notify_waiters();
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        let (code, reason) = frame.map_or((1000, String::new()), |frame| {
                            (u16::from(frame.code), frame.reason.to_string())
                        });
                        reader_inbox.lock().unwrap().closed = Some((code, reason));
                        reader_notify.notify_waiters();
                        break;
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
            reader_notify.notify_waiters();
        });
        Self {
            sink,
            inbox,
            notify,
            reader,
        }
    }

    /// Sends a JSON message, adding protocolVersion 2 when absent.
    pub async fn send(&mut self, mut message: Value) {
        if let Value::Object(map) = &mut message {
            map.entry("protocolVersion").or_insert_with(|| json!(2));
        }
        let text = serde_json::to_string(&message).expect("serialize test message");
        self.sink
            .send(Message::Text(text.into()))
            .await
            .expect("send");
    }

    /// Sends a raw text frame.
    pub async fn send_raw(&mut self, text: &str) {
        self.sink
            .send(Message::Text(text.to_owned().into()))
            .await
            .expect("send raw");
    }

    /// Snapshot of all messages received so far.
    pub fn messages(&self) -> Vec<Value> {
        self.inbox.lock().unwrap().messages.clone()
    }

    /// First received message matching the predicate (including old messages).
    pub fn find(&self, predicate: impl Fn(&Value) -> bool) -> Option<Value> {
        self.inbox
            .lock()
            .unwrap()
            .messages
            .iter()
            .find(|value| predicate(value))
            .cloned()
    }

    /// Number of messages received so far (used as a mark for wait_after).
    pub fn mark(&self) -> usize {
        self.inbox.lock().unwrap().messages.len()
    }

    /// Waits for a message matching the predicate, scanning from a mark.
    pub async fn wait_after(
        &self,
        mark: usize,
        predicate: impl Fn(&Value) -> bool,
        timeout: Duration,
    ) -> Value {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let inbox = self.inbox.lock().unwrap();
                if let Some(value) = inbox
                    .messages
                    .iter()
                    .skip(mark)
                    .find(|value| predicate(value))
                {
                    return value.clone();
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                let all = self.messages();
                panic!("timed out waiting for message after mark {mark}; received: {all:?}");
            }
            let _ = tokio::time::timeout(deadline - now, self.notify.notified()).await;
        }
    }

    /// Waits for a message of the given type.
    pub async fn wait_type(&self, message_type: &str) -> Value {
        let mark = self.mark();
        self.wait_after(mark, |value| value["type"] == json!(message_type), WAIT)
            .await
    }

    /// Waits for the socket to close and returns (code, reason).
    pub async fn wait_closed(&self, timeout: Duration) -> (u16, String) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(closed) = self.inbox.lock().unwrap().closed.clone() {
                return closed;
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                panic!(
                    "timed out waiting for close; messages: {:?}",
                    self.messages()
                );
            }
            let _ = tokio::time::timeout(deadline - now, self.notify.notified()).await;
        }
    }

    /// True when the socket has already closed.
    pub fn is_closed(&self) -> bool {
        self.inbox.lock().unwrap().closed.is_some()
    }

    /// Abruptly drops the connection (simulates a network drop for reconnect
    /// tests). The server sees a socket close without a close frame.
    pub fn force_disconnect(self) {
        self.reader.abort();
        drop(self.sink);
    }

    /// Graceful client-initiated close.
    pub async fn close(mut self) {
        let _ = futures_util::SinkExt::close(&mut self.sink).await;
        self.reader.abort();
    }
}

/// Room options payload used by create_room / join_room.
pub fn room_options(
    max_players: u32,
    is_short_deck: bool,
    starting_stack: u64,
    ai_delay: u64,
) -> Value {
    let _ = is_short_deck;
    let (small, big) = (40, 80);
    json!({
        "maxPlayers": max_players,
        "isShortDeck": is_short_deck,
        "startingStack": starting_stack,
        "smallBlind": small,
        "bigBlind": big,
        "ante": small,
        "minBet": big,
        "aiDelay": ai_delay,
    })
}

/// Merges room options into a base message object.
pub fn with_room_options(mut message: Value, options: Value) -> Value {
    if let (Value::Object(target), Value::Object(source)) = (&mut message, options) {
        for (key, value) in source {
            target.insert(key, value);
        }
    }
    message
}

/// Extracts the latest game_state matching the predicate.
pub fn latest_state(client: &TestClient, predicate: impl Fn(&Value) -> bool) -> Option<Value> {
    client
        .messages()
        .into_iter()
        .rev()
        .find(|value| value["type"] == json!("game_state") && predicate(value))
}
