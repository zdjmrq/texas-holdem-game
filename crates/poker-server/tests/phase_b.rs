//! Phase B coverage: heartbeat ping/pong, bounded outbound slow consumers and
//! the process-local metrics counters.

mod common;

use std::time::Duration;

use common::{TestClient, WAIT};
use futures_util::{SinkExt, StreamExt};
use poker_server::{ServerOptions, spawn_server};
use serde_json::json;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

async fn next_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout: Duration,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    tokio::time::timeout(timeout, socket.next())
        .await
        .ok()
        .flatten()
}

#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_answers_are_required() {
    let handle = spawn_server(ServerOptions {
        port: 0,
        heartbeat_interval_ms: 60,
        ..Default::default()
    })
    .await
    .expect("server");
    let url = handle.url();

    // Active client: answers pings explicitly and stays connected.
    let (mut active, _) = connect_async(&url).await.expect("active connect");
    let mut saw_hello = false;
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_millis(350) {
        match next_frame(&mut active, Duration::from_millis(40)).await {
            Some(Ok(Message::Ping(payload))) => {
                active.send(Message::Pong(payload)).await.expect("pong");
            }
            Some(Ok(Message::Text(_))) => saw_hello = true,
            Some(Ok(_)) => {}
            Some(Err(_)) | None => break,
        }
    }
    assert!(saw_hello, "hello frame expected");
    active
        .send(Message::Text(
            serde_json::to_string(&json!({"protocolVersion":2,"type":"get_snapshot"}))
                .unwrap()
                .into(),
        ))
        .await
        .expect("active client send");
    let reply = next_frame(&mut active, Duration::from_secs(2))
        .await
        .expect("active reply")
        .expect("frame");
    assert!(
        matches!(reply, Message::Text(_)),
        "active connection must survive heartbeats"
    );

    // Idle client: never reads or pongs, so the second heartbeat tick drops it.
    let (mut idle, _) = connect_async(&url).await.expect("idle connect");
    let hello = next_frame(&mut idle, Duration::from_secs(2))
        .await
        .expect("idle hello")
        .expect("frame");
    assert!(matches!(hello, Message::Text(_)));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(_)) = idle.next().await {}
    })
    .await;
    assert!(
        closed.is_ok(),
        "idle connection should be terminated by the heartbeat"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while handle.metrics().heartbeat_timeouts == 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        handle.metrics().heartbeat_timeouts >= 1,
        "heartbeat timeout counter"
    );
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_count_rejections_and_parse_errors() {
    let handle = spawn_server(ServerOptions {
        port: 0,
        max_rooms: 1,
        max_connections: 8,
        max_connections_per_ip: 2,
        max_messages_per_window: 20,
        rate_window_ms: 60_000,
        ..Default::default()
    })
    .await
    .expect("server");
    let url = handle.url();
    let mut a = TestClient::connect(&url).await;
    a.wait_type("hello").await;
    let b = TestClient::connect(&url).await;
    b.wait_type("hello").await;

    // Capacity rejection (per-IP cap 2): third connection gets 1013.
    let third = TestClient::connect(&url).await;
    let (code, reason) = third.wait_closed(WAIT).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "server capacity reached");

    // Parse errors are counted and answered.
    let mark = a.mark();
    a.send_raw("not-json{").await;
    a.wait_after(
        mark,
        |value| value["type"] == json!("error") && value["message"] == json!("消息格式无效"),
        WAIT,
    )
    .await;
    assert!(handle.metrics().parse_errors >= 1);

    // Rooms created / closed.
    a.send(with_json_room_options()).await;
    a.wait_type("room_created").await;
    assert_eq!(handle.metrics().rooms_created, 1);
    let mark = a.mark();
    a.send(json!({"type": "leave_room", "requestId": "leave-1", "roomCode": "1"}))
        .await;
    a.wait_after(
        mark,
        |value| value["type"] == json!("ack") && value["requestId"] == json!("leave-1"),
        WAIT,
    )
    .await;
    assert_eq!(handle.metrics().rooms_closed, 1);
    assert!(handle.metrics().connections_accepted >= 2);
    assert!(handle.metrics().connections_rejected_capacity >= 1);
    handle.shutdown().await;
}

fn with_json_room_options() -> serde_json::Value {
    json!({
        "type": "create_room",
        "requestId": "create-1",
        "name": "Metrics",
        "maxPlayers": 2,
        "isShortDeck": false,
        "startingStack": 20000,
        "smallBlind": 40,
        "bigBlind": 80,
        "ante": 40,
        "minBet": 80,
        "aiDelay": 800,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn origin_rejection_counter() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let handle = spawn_server(ServerOptions {
        port: 0,
        allowed_origins: vec!["https://cards.example".to_owned()],
        ..Default::default()
    })
    .await
    .expect("server");
    let url = handle.url();
    let mut request = url.as_str().into_client_request().expect("request");
    request.headers_mut().insert(
        "Origin",
        "https://untrusted.example".parse().expect("origin"),
    );
    let rejected = connect_async(request).await;
    assert!(
        rejected.is_err(),
        "origin rejection must fail the handshake"
    );
    assert_eq!(handle.metrics().connections_rejected_origin, 1);
    handle.shutdown().await;
}
