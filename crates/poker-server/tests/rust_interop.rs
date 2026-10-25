//! Rust client (<->) Rust server interoperability: the same poker-client crate
//! the desktop app uses drives the real poker-server over a real WebSocket.
//!
//! This covers the full Phase 5 checklist end to end: create/join/start,
//! authoritative personalised game_state, player actions, showdown settlement,
//! ready + next hand, manual reconnect with resume_session and the 4001
//! "session resumed elsewhere" kick. It runs as a normal test (no external
//! process or Node dependency).

use std::time::Duration;

use poker_client::{
    ActionRequest, Client, ClientConfig, ClientEvent, HelloPolicy, RoomOptionsInput,
};
use poker_protocol::{ActionType, Phase, ServerMessage};
use poker_server::{ServerOptions, spawn_server};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(8);
const WAIT_HAND: Duration = Duration::from_secs(25);

fn config(url: &str) -> ClientConfig {
    ClientConfig::new(url).with_hello_policy(HelloPolicy::compatible_server())
}

async fn next_event(client: &mut Client, timeout: Duration) -> ClientEvent {
    tokio::time::timeout(timeout, client.next_event())
        .await
        .expect("timed out waiting for client event")
        .expect("client event stream ended")
}

async fn next_message(
    client: &mut Client,
    mut predicate: impl FnMut(&ServerMessage) -> bool,
    timeout: Duration,
) -> ServerMessage {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "timed out waiting for server message");
        match next_event(client, remaining).await {
            ClientEvent::Message(message) if predicate(&message) => return message,
            ClientEvent::Message(_)
            | ClientEvent::Connected { .. }
            | ClientEvent::Hello(_)
            | ClientEvent::Reconnecting { .. } => {}
            ClientEvent::Disconnected { code, reason, .. } => {
                panic!("disconnected while waiting for message: {code:?} {reason}")
            }
            ClientEvent::Closed => panic!("client closed while waiting for message"),
            ClientEvent::ProtocolError { raw, error } => {
                panic!("protocol error: {error} raw={raw}")
            }
            ClientEvent::ReconnectFailed { last_error, .. } => {
                panic!("reconnect failed: {last_error}")
            }
        }
    }
}

/// Drives one client through the current hand with check/call until settlement.
async fn drive_to_settlement(client: &mut Client, hand_id: i64) -> poker_protocol::GameState {
    let deadline = tokio::time::Instant::now() + WAIT_HAND;
    let mut previous_phase = None;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!remaining.is_zero(), "hand {hand_id} did not settle");
        match next_event(client, remaining).await {
            ClientEvent::Message(ServerMessage::GameState(state)) => {
                if matches!(state.phase, Phase::Flop | Phase::Turn | Phase::River)
                    && previous_phase != Some(state.phase)
                {
                    assert_eq!(
                        state.current_player_idx, state.bb_idx,
                        "heads-up: BB opens postflop"
                    );
                    assert!(
                        state
                            .players
                            .iter()
                            .all(|p| p.round_bet == 0 && p.last_action.is_none()),
                        "new street must not publish previous-street actions"
                    );
                }
                previous_phase = Some(state.phase);
                if state.phase == Phase::Idle
                    && state.last_hand_result.is_some()
                    && state.hand_id == hand_id
                {
                    return state;
                }
                if state.is_your_turn {
                    let legal = &state.legal_actions.actions;
                    let (action, amount) = if legal.contains(&ActionType::Check) {
                        (ActionType::Check, 0)
                    } else if legal.contains(&ActionType::Call) {
                        (ActionType::Call, state.legal_actions.to_call)
                    } else {
                        (ActionType::Fold, 0)
                    };
                    let request = ActionRequest::new(
                        state.room_code.clone(),
                        state.hand_id,
                        state.turn_id,
                        state.state_version,
                        action,
                    )
                    .with_amount(amount);
                    client.player_action(request).await.expect("player_action");
                }
            }
            ClientEvent::Message(ServerMessage::ActionAck(ack)) => {
                assert!(ack.accepted, "action rejected by the Rust server: {ack:?}");
            }
            ClientEvent::Disconnected { code, reason, .. } => {
                panic!("disconnected mid-hand: {code:?} {reason}")
            }
            ClientEvent::Closed => panic!("client closed mid-hand"),
            ClientEvent::ProtocolError { raw, error } => {
                panic!("protocol error: {error} raw={raw}")
            }
            _ => {}
        }
    }
}

async fn wait_hand(
    client: &mut Client,
    hand_id: i64,
    timeout: Duration,
) -> poker_protocol::GameState {
    let message = next_message(
        client,
        |message| matches!(message, ServerMessage::GameState(state) if state.hand_id == hand_id && state.phase != Phase::Idle),
        timeout,
    )
    .await;
    let ServerMessage::GameState(state) = message else {
        unreachable!()
    };
    state
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_client_full_flow_against_rust_server() {
    let handle = spawn_server(ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        disconnect_grace_ms: 1500,
        rng_seed: Some(2026),
        ..Default::default()
    })
    .await
    .expect("Rust server start");
    let url = handle.url();

    let mut host = Client::connect(config(&url)).await.expect("host connect");
    let mut guest = Client::connect(config(&url)).await.expect("guest connect");

    host.create_room(
        "RustHost",
        RoomOptionsInput {
            max_players: Some(json!(2)),
            ..RoomOptionsInput::default()
        },
    )
    .await
    .expect("create_room");
    let created = next_message(
        &mut host,
        |message| matches!(message, ServerMessage::RoomCreated(_)),
        WAIT,
    )
    .await;
    let ServerMessage::RoomCreated(room) = created else {
        unreachable!()
    };
    assert_eq!(room.room_code, "1");
    assert!(room.is_host);
    assert!(!room.resume_token.is_empty());
    host.remember_room(&room);

    guest
        .join_room(
            room.room_code.clone(),
            "RustGuest",
            RoomOptionsInput::default(),
        )
        .await
        .expect("join_room");
    let joined = next_message(
        &mut guest,
        |message| matches!(message, ServerMessage::RoomJoined(_)),
        WAIT,
    )
    .await;
    let ServerMessage::RoomJoined(joined) = joined else {
        unreachable!()
    };
    assert_eq!(joined.room_code, room.room_code);
    assert!(!joined.is_host);
    guest.remember_room(&joined);

    host.start_game(room.room_code.clone())
        .await
        .expect("start_game");
    next_message(
        &mut host,
        |message| matches!(message, ServerMessage::GameStarting(_)),
        WAIT,
    )
    .await;

    // Hand 1: both clients play check/call until showdown.
    let (host_settled, guest_settled) = tokio::join!(
        drive_to_settlement(&mut host, 1),
        drive_to_settlement(&mut guest, 1)
    );
    for settled in [&host_settled, &guest_settled] {
        let result = settled.last_hand_result.as_ref().expect("last hand result");
        assert_eq!(result.reason, poker_protocol::HandEndReason::Showdown);
        assert_eq!(result.pot, 160);
        assert_eq!(settled.your_cards.len(), 2);
        let total: i64 = settled.players.iter().map(|player| player.stack).sum();
        assert_eq!(total, 40_000);
    }
    assert_ne!(
        host_settled.your_cards, guest_settled.your_cards,
        "hole cards are private"
    );

    // Both ready -> hand 2 starts after the 250ms countdown.
    host.ready_for_next(room.room_code.clone(), 1)
        .await
        .expect("host ready");
    guest
        .ready_for_next(room.room_code.clone(), 1)
        .await
        .expect("guest ready");
    let (host_hand2, guest_hand2) = tokio::join!(
        wait_hand(&mut host, 2, WAIT_HAND),
        wait_hand(&mut guest, 2, WAIT_HAND)
    );
    assert_eq!(host_hand2.phase, Phase::Preflop);
    assert_eq!(guest_hand2.phase, Phase::Preflop);
    let cards_before_resume = host_hand2.your_cards.clone();

    // Manual reconnect keeps the session and re-attaches the same seat.
    host.reconnect().await.expect("manual reconnect");
    let resumed = next_message(
        &mut host,
        |message| matches!(message, ServerMessage::RoomJoined(room) if room.resumed),
        WAIT,
    )
    .await;
    let ServerMessage::RoomJoined(resumed) = resumed else {
        unreachable!()
    };
    assert_eq!(resumed.player_id, room.player_id);
    let snapshot = next_message(
        &mut host,
        |message| matches!(message, ServerMessage::GameState(state) if state.hand_id == 2),
        WAIT,
    )
    .await;
    let ServerMessage::GameState(snapshot) = snapshot else {
        unreachable!()
    };
    assert_eq!(
        snapshot.your_cards, cards_before_resume,
        "resume restores the hole cards"
    );

    // A second connection claiming the same token kicks the current one (4001).
    let session = host.session().cloned().expect("stored session");
    let mut kicker = Client::connect(config(&url)).await.expect("kicker connect");
    kicker.set_session(session);
    kicker.reconnect().await.expect("kicker resume");
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "expected the kicked connection to close"
        );
        match next_event(&mut host, remaining).await {
            ClientEvent::Disconnected { code, .. } => {
                assert_eq!(code, Some(4001), "session-resumed-elsewhere close code");
                break;
            }
            ClientEvent::Closed => panic!("kicked connection closed without a close code"),
            _ => {}
        }
    }

    // Kicker owns the seat now; leave cleanly and shut the server down.
    kicker
        .leave_room(room.room_code.clone())
        .await
        .expect("kicker leave");
    kicker.disconnect().await;
    guest
        .leave_room(room.room_code.clone())
        .await
        .expect("guest leave");
    guest.disconnect().await;
    let metrics = handle.metrics();
    assert!(metrics.rooms_created >= 1);
    assert!(metrics.resumes >= 1, "manual reconnect resume counted");
    assert!(metrics.session_kicks >= 1, "session kick counted");
    assert!(metrics.actions_accepted >= 6, "hand actions counted");
    assert_eq!(metrics.ai_decisions, 0, "no AI seat in a two-human room");
    handle.shutdown().await;
}
