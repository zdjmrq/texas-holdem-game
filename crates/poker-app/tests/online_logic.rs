//! Pure-logic tests for the online module (no sockets, no egui).
//!
//! Golden protocol payloads come from tests/golden/protocol/s2c-messages.jsonl
//! so the DTO shapes are the ones a real server sends.

use std::time::Instant;

use poker_app::online::commands::OnlineAction;
use poker_app::online::events::{CommandReceipt, PendingKind, WorkerEvent};
use poker_app::online::mapping;
use poker_app::online::text;
use poker_app::online::view::{ConnStatus, OnlineEffect, OnlineStage, OnlineView};
use poker_client::{ClientError, ClientEvent, SessionCredentials};
use poker_protocol::{ServerMessage, parse_server_message};
use serde_json::Value;

fn golden_messages() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/protocol/s2c-messages.jsonl"
    );
    let text = std::fs::read_to_string(path).expect("golden corpus");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("golden json"))
        .map(|line| line["message"].clone())
        .collect()
}

fn golden(type_name: &str) -> ServerMessage {
    let value = golden_messages()
        .into_iter()
        .find(|message| message["type"].as_str() == Some(type_name))
        .unwrap_or_else(|| panic!("golden message {type_name}"));
    parse_server_message(&value.to_string()).expect("golden parse")
}

fn view_with_room() -> (OnlineView, ServerMessage) {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    let message = golden("room_joined");
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(message.clone())),
        Instant::now(),
    );
    (view, message)
}

fn game_state_message() -> ServerMessage {
    golden("game_state")
}

fn receipt(kind: PendingKind, request_id: &str) -> CommandReceipt {
    CommandReceipt {
        request_id: request_id.to_owned(),
        kind,
        hand_id: 1,
        turn_id: 2,
        expected_state_version: 5,
    }
}

#[test]
fn text_helpers_match_reference_wording() {
    assert_eq!(text::chips(20_000), "20,000");
    assert_eq!(text::chips(5_000), "5,000");
    assert_eq!(text::chips(0), "0");
    assert_eq!(text::close_notice(Some(4001)), text::KICKED);
    assert_eq!(text::close_notice(Some(1000)), "已断开连接");
    assert_eq!(text::close_notice(None), "连接中断");
    assert_eq!(
        text::connect_error(&ClientError::InvalidUrl("http://x".to_owned())),
        "服务器地址必须以 ws:// 或 wss:// 开头"
    );
    assert_eq!(text::NEXT_HAND, "下一手 →");
    assert!(text::waiting_ready(2).contains('2'));
}

#[test]
fn mapping_converts_cards_and_rejects_bad_tokens() {
    let ace = mapping::card_from_dto(&poker_protocol::CardDto::new("A", "spades")).expect("ace");
    assert_eq!(ace.rank(), poker_core::Rank::Ace);
    assert_eq!(ace.suit(), poker_core::Suit::Spades);
    assert!(mapping::card_from_dto(&poker_protocol::CardDto::new("1", "spades")).is_none());
    assert!(mapping::card_from_dto(&poker_protocol::CardDto::new("A", "stars")).is_none());
}

#[test]
fn mapping_legal_actions_fall_back_to_room_config_unit() {
    let config = poker_protocol::RoomConfig {
        max_players: 2,
        is_short_deck: true,
        starting_stack: 20_000,
        small_blind: 40,
        big_blind: 80,
        ante: 40,
        min_bet: 80,
        ai_delay: 800,
    };
    let legal = poker_protocol::LegalActions {
        actions: vec![poker_protocol::ActionType::Fold],
        to_call: 80,
        can_raise: true,
        min_raise_to: 160,
        max_raise_to: 400,
        wager_unit: None,
    };
    let engine = mapping::legal_from_wire(&legal, &config);
    assert_eq!(engine.wager_unit, 40, "short deck falls back to the ante");
    assert_eq!(engine.to_call, 80);
    assert!(engine.contains(poker_engine::ActionType::Fold));
}

#[test]
fn mapping_hand_names_follow_variant_order() {
    // Rank 5 is flush in standard and full house in short deck.
    assert_eq!(mapping::hand_name(false, 5), "同花");
    assert_eq!(mapping::hand_name(true, 5), "葫芦");
    assert_eq!(mapping::hand_name(false, 9), "皇家同花顺");
    assert_eq!(mapping::hand_name(false, 42), "-");
}

#[test]
fn mapping_wire_action_semantics_match_protocol() {
    assert_eq!(
        mapping::wire_action(OnlineAction::Fold, 80, 1000),
        (poker_protocol::ActionType::Fold, 0)
    );
    assert_eq!(
        mapping::wire_action(OnlineAction::Call, 80, 1000),
        (poker_protocol::ActionType::Call, 80)
    );
    assert_eq!(
        mapping::wire_action(OnlineAction::AllIn, 80, 1000),
        (poker_protocol::ActionType::Allin, 1000)
    );
    assert_eq!(
        mapping::wire_action(OnlineAction::RaiseTo(240), 80, 1000),
        (poker_protocol::ActionType::Raise, 240)
    );
}

#[test]
fn room_joined_enters_lobby_and_records_the_session() {
    let (view, message) = view_with_room();
    let ServerMessage::RoomJoined(room) = message else {
        panic!("golden room_joined");
    };
    assert_eq!(view.stage, OnlineStage::Lobby);
    assert_eq!(view.room_code(), Some(room.room_code.as_str()));
    assert_eq!(view.my_player_id.as_deref(), Some(room.player_id.as_str()));
    let session = view.session.expect("session");
    assert_eq!(session.room_code, room.room_code);
    assert_eq!(session.resume_token, room.resume_token);
    assert!(view.status_line.contains("已进入房间"));
    assert_eq!(view.players.len(), room.players.len());
}

#[test]
fn game_state_is_deduped_by_version_and_hand() {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    let message = game_state_message();
    let ServerMessage::GameState(first) = message.clone() else {
        panic!("game_state");
    };
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(message)),
        Instant::now(),
    );
    assert_eq!(view.stage, OnlineStage::Table);
    let first_version = view.last_state_version;
    // Replaying the same snapshot must not change state (js/network.js dedupe).
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(game_state_message())),
        Instant::now(),
    );
    assert_eq!(view.last_state_version, first_version);
    assert_eq!(view.hand_id(), first.hand_id);
}

#[test]
fn pending_action_clears_on_ack_and_on_newer_state() {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    let ack = golden("action_ack");
    let ServerMessage::ActionAck(ack) = ack else {
        panic!("action_ack");
    };
    let request_id = mapping::request_id_key(&ack.request_id);
    view.reduce(
        WorkerEvent::CommandSent(receipt(PendingKind::Action, &request_id)),
        Instant::now(),
    );
    assert!(view.pending.is_some());
    assert!(!view.can_act());
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(ServerMessage::ActionAck(ack))),
        Instant::now(),
    );
    assert!(view.pending.is_none(), "matching action_ack clears pending");

    // A newer authoritative snapshot also clears a stale pending request.
    let mut stale = receipt(PendingKind::Action, "action-2");
    stale.expected_state_version = 0;
    view.reduce(WorkerEvent::CommandSent(stale), Instant::now());
    assert!(view.pending.is_some());
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(game_state_message())),
        Instant::now(),
    );
    assert!(view.pending.is_none(), "newer game_state clears pending");
}

#[test]
fn reservation_blocks_duplicate_input_and_send_failure_unlocks_it() {
    let (mut view, _) = view_with_room();
    view.status = ConnStatus::Connected;
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(game_state_message())),
        Instant::now(),
    );
    let game = view.game.as_mut().unwrap();
    game.is_your_turn = true;
    game.legal_actions.actions = vec![poker_protocol::ActionType::Check];
    assert!(view.can_act());
    view.reserve_command(PendingKind::Action, Instant::now());
    assert!(!view.can_act(), "block before worker receipt arrives");
    view.reduce(
        WorkerEvent::CommandFailed {
            kind: PendingKind::Action,
            message: "closed".into(),
        },
        Instant::now(),
    );
    assert!(view.can_act(), "retry immediately after transport failure");
    assert_eq!(view.error_line.as_deref(), Some("closed"));
}

#[test]
fn matching_server_error_unlocks_pending_command() {
    let mut view = OnlineView::default();
    let ServerMessage::Error(mut error) = golden("error") else {
        panic!("error")
    };
    error.code = poker_protocol::ErrorCode::InvalidRequest;
    error.request_id = Some("rejected".into());
    view.reduce(
        WorkerEvent::CommandSent(receipt(PendingKind::Action, "rejected")),
        Instant::now(),
    );
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(ServerMessage::Error(error))),
        Instant::now(),
    );
    assert!(view.pending.is_none());
    assert!(view.error_line.is_some());
}

#[test]
fn switching_servers_discards_old_room_game_and_resume_credentials() {
    let (mut view, _) = view_with_room();
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(game_state_message())),
        Instant::now(),
    );
    let old_session = view.session.clone();
    assert!(old_session.is_some());
    view.prepare_connection("ws://other:4000".into(), old_session);
    assert!(view.session.is_none());
    assert!(view.room.is_none());
    assert!(view.game.is_none());
    assert!(!view.can_act());
    assert_eq!(view.stage, OnlineStage::Connect);
    assert_eq!(view.status, ConnStatus::Connecting);
    let session = SessionCredentials::new("ws://other:4000", "2", "resume");
    view.prepare_connection("ws://other:4000".into(), Some(session.clone()));
    assert_eq!(view.session, Some(session));
}

#[test]
fn kicked_connection_returns_to_connect_screen() {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    view.session = Some(SessionCredentials::new("ws://x", "1", "token"));
    view.reduce(
        WorkerEvent::Client(ClientEvent::Disconnected {
            code: Some(4001),
            reason: "session resumed elsewhere".to_owned(),
            will_reconnect: false,
        }),
        Instant::now(),
    );
    assert!(view.kicked);
    assert_eq!(view.stage, OnlineStage::Connect);
    assert!(view.session.is_none());
    assert_eq!(view.status_line, text::KICKED);
}

#[test]
fn reconnect_chain_sets_banners_and_resume_clears_them() {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    view.reduce(
        WorkerEvent::Client(ClientEvent::Disconnected {
            code: None,
            reason: "connection reset".to_owned(),
            will_reconnect: true,
        }),
        Instant::now(),
    );
    assert!(matches!(view.status, ConnStatus::Reconnecting { .. }));
    view.reduce(
        WorkerEvent::Client(ClientEvent::Reconnecting {
            attempt: 0,
            delay_ms: 825,
        }),
        Instant::now(),
    );
    assert!(
        view.banner
            .as_deref()
            .is_some_and(|banner| banner.contains('1'))
    );
    view.reduce(
        WorkerEvent::Client(ClientEvent::Connected { reconnecting: true }),
        Instant::now(),
    );
    assert_eq!(view.status, ConnStatus::Connected);
    assert_eq!(view.banner.as_deref(), Some(text::RECONNECTED_RESUMING));
    let resumed = golden("room_joined");
    let ServerMessage::RoomJoined(mut room) = resumed else {
        panic!("room_joined");
    };
    room.resumed = true;
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(ServerMessage::RoomJoined(room))),
        Instant::now(),
    );
    assert!(view.banner.is_none());
}

#[test]
fn resume_expired_clears_the_session() {
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    view.session = Some(SessionCredentials::new("ws://x", "1", "token"));
    let error = golden("error");
    let ServerMessage::Error(mut payload) = error else {
        panic!("error");
    };
    payload.code = poker_protocol::ErrorCode::ResumeExpired;
    payload.message = "恢复信息已失效".to_owned();
    view.reduce(
        WorkerEvent::Client(ClientEvent::Message(ServerMessage::Error(payload))),
        Instant::now(),
    );
    assert!(view.session.is_none());
    assert_eq!(view.stage, OnlineStage::Connect);
    assert_eq!(view.status_line, "恢复信息已失效");
    assert!(view.banner.is_some());
}

#[test]
fn settlement_snapshot_opens_the_overlay_and_prompts_once() {
    let game = golden_messages().into_iter().find(|message| {
        message["type"].as_str() == Some("game_state")
            && message["phase"].as_str() == Some("idle")
            && !message["lastHandResult"].is_null()
    });
    let Some(game) = game else {
        return; // golden corpus always has one; skip defensively
    };
    let message = parse_server_message(&game.to_string()).expect("settlement state");
    let mut view = OnlineView::new(Some("ws://127.0.0.1:3000".to_owned()));
    let effects = view.reduce(
        WorkerEvent::Client(ClientEvent::Message(message)),
        Instant::now(),
    );
    assert!(view.settlement_open);
    assert!(effects.contains(&OnlineEffect::PlaySettlementPrompt));
    let settlement = view.settlement().expect("settlement view");
    assert!(settlement.rows.iter().any(|row| row.is_winner));
    assert!(!view.next_hand_label().is_empty());
}
