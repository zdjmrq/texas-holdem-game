//! Optional interoperability smoke test against a real protocol v2 server.
//!
//! These tests are ignored by default; they need a running room server (the
//! reference JS server or the Rust poker-server) and never run in CI:
//!
//! ~~~text
//! # start the reference server on port 3000
//! node server/local-server.js
//! # then run
//! POKER_INTEROP_URL=ws://127.0.0.1:3000 \
//!   cargo test -p poker-client --offline --test interop -- --ignored --nocapture
//! ~~~
//!
//! The checklist mirrors docs/specs/protocol-v2.md section 14 and
//! docs/specs/client.md section 8.

use std::time::Duration;

use poker_client::{Client, ClientConfig, ClientEvent, RoomOptionsInput};
use poker_protocol::ServerMessage;
use serde_json::Value;

fn interop_url() -> Option<String> {
    std::env::var("POKER_INTEROP_URL").ok()
}

fn config(url: String) -> ClientConfig {
    ClientConfig::new(url).with_hello_policy(poker_client::HelloPolicy::compatible_server())
}

async fn next_matching(
    client: &mut Client,
    timeout: Duration,
    mut predicate: impl FnMut(&ServerMessage) -> bool,
) -> ServerMessage {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for server message");
        let event = tokio::time::timeout(remaining, client.next_event())
            .await
            .expect("timed out waiting for server event");
        match event {
            Some(ClientEvent::Message(message)) if predicate(&message) => return message,
            Some(ClientEvent::Message(_))
            | Some(ClientEvent::Connected { .. })
            | Some(ClientEvent::Hello(_))
            | Some(ClientEvent::Reconnecting { .. }) => {}
            Some(ClientEvent::Disconnected { .. }) | Some(ClientEvent::Closed) => {
                panic!("disconnected while waiting for a message")
            }
            Some(ClientEvent::ProtocolError { raw, error }) => {
                panic!("protocol error: {error} raw={raw}")
            }
            Some(ClientEvent::ReconnectFailed { last_error, .. }) => {
                panic!("reconnect failed: {last_error}")
            }
            None => panic!("event stream ended"),
        }
    }
}

#[tokio::test]
#[ignore = "requires a running protocol v2 server; set POKER_INTEROP_URL"]
async fn create_join_start_smoke_against_real_server() {
    let Some(url) = interop_url() else {
        eprintln!("POKER_INTEROP_URL is not set; skipping interop smoke test");
        return;
    };
    let options = RoomOptionsInput {
        max_players: Some(Value::from(2)),
        ..RoomOptionsInput::default()
    };
    let mut host = Client::connect(config(url.clone()))
        .await
        .expect("host connect");
    host.create_room("Host", options.clone())
        .await
        .expect("create_room");
    let room_created = next_matching(&mut host, Duration::from_secs(5), |message| {
        matches!(message, ServerMessage::RoomCreated(_))
    })
    .await;
    let ServerMessage::RoomCreated(room) = room_created else {
        unreachable!()
    };
    assert_eq!(room.room_code, "1");
    assert!(room.is_host);
    assert!(!room.resume_token.is_empty());
    host.remember_room(&room);

    let mut guest = Client::connect(config(url)).await.expect("guest connect");
    guest
        .join_room(room.room_code.clone(), "Guest", RoomOptionsInput::default())
        .await
        .expect("join_room");
    let room_joined = next_matching(&mut guest, Duration::from_secs(5), |message| {
        matches!(message, ServerMessage::RoomJoined(_))
    })
    .await;
    let ServerMessage::RoomJoined(joined) = room_joined else {
        unreachable!()
    };
    assert_eq!(joined.room_code, room.room_code);
    assert!(!joined.is_host);
    guest.remember_room(&joined);

    host.start_game(room.room_code.clone())
        .await
        .expect("start_game");
    next_matching(&mut host, Duration::from_secs(10), |message| {
        matches!(message, ServerMessage::GameStarting(_))
    })
    .await;
    let state = next_matching(&mut guest, Duration::from_secs(10), |message| {
        matches!(message, ServerMessage::GameState(_))
    })
    .await;
    let ServerMessage::GameState(state) = state else {
        unreachable!()
    };
    assert_eq!(state.room_code, room.room_code);
    assert!(
        !state.players.is_empty(),
        "game_state must list the seats after start"
    );
    assert_eq!(state.players.iter().filter(|p| p.is_human).count(), 2);

    host.disconnect().await;
    guest.disconnect().await;
}

#[tokio::test]
#[ignore = "requires a running protocol v2 server; set POKER_INTEROP_URL"]
async fn resume_smoke_against_real_server() {
    let Some(url) = interop_url() else {
        eprintln!("POKER_INTEROP_URL is not set; skipping interop resume test");
        return;
    };
    let mut client = Client::connect(config(url.clone())).await.expect("connect");
    client
        .create_room("Resumer", RoomOptionsInput::default())
        .await
        .expect("create_room");
    let room_created = next_matching(&mut client, Duration::from_secs(5), |message| {
        matches!(message, ServerMessage::RoomCreated(_))
    })
    .await;
    let ServerMessage::RoomCreated(room) = room_created else {
        unreachable!()
    };
    client.remember_room(&room);
    client.reconnect().await.expect("manual reconnect");
    let resumed = next_matching(
        &mut client,
        Duration::from_secs(10),
        |message| matches!(message, ServerMessage::RoomJoined(room) if room.resumed),
    )
    .await;
    let ServerMessage::RoomJoined(joined) = resumed else {
        unreachable!()
    };
    assert_eq!(joined.player_id, room.player_id);
    client.disconnect().await;
}
