//! End-to-end integration tests over real WebSocket connections.
//!
//! Scenario names mirror the reference tests/server.test.js coverage:
//! lobby lifecycle, two-human authoritative play, resume, limits, capacity,
//! Origin allowlist, oversized frames and the short-deck AI table.
#![allow(unused_variables, unused_mut)]

mod common;

use std::time::Duration;

use common::*;
use poker_server::{ServerOptions, spawn_server};
use serde_json::{Value, json};

const WAIT_LONG: Duration = Duration::from_secs(6);

fn test_options() -> ServerOptions {
    ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        ..Default::default()
    }
}

async fn start(options: ServerOptions) -> (poker_server::ServerHandle, String) {
    let handle = spawn_server(options).await.expect("server start");
    let url = handle.url();
    (handle, url)
}

fn last_state(client: &TestClient) -> Option<Value> {
    client
        .messages()
        .into_iter()
        .rev()
        .find(|value| value["type"] == json!("game_state"))
}

fn is_idle_settled(state: &Value) -> bool {
    state["phase"] == json!("idle") && !state["lastHandResult"].is_null()
}

async fn action_and_wait(client: &mut TestClient, state: &Value) {
    let legal = state["legalActions"]["actions"].clone();
    let has = |name: &str| {
        legal
            .as_array()
            .is_some_and(|actions| actions.iter().any(|value| value == name))
    };
    let (action, amount) = if has("check") {
        ("check", 0)
    } else if has("call") {
        (
            "call",
            state["legalActions"]["toCall"].as_i64().unwrap_or(0),
        )
    } else {
        ("fold", 0)
    };
    let request_id = format!("act-{}-{}", state["handId"], state["turnId"]);
    let mark = client.mark();
    client
        .send(json!({
            "type": "player_action",
            "requestId": request_id,
            "roomCode": state["roomCode"],
            "handId": state["handId"],
            "turnId": state["turnId"],
            "expectedStateVersion": state["stateVersion"],
            "action": action,
            "amount": amount,
        }))
        .await;
    let ack = client
        .wait_after(
            mark,
            |value| value["type"] == json!("action_ack") && value["requestId"] == json!(request_id),
            WAIT,
        )
        .await;
    assert_eq!(ack["accepted"], true, "action rejected: {ack}");
    tokio::time::sleep(Duration::from_millis(15)).await;
}

/// Drives the current hand to settlement with check/call for both humans.
async fn drive_hand(a: &mut TestClient, b: &mut TestClient) -> Value {
    let started = std::time::Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "hand did not finish; a={:?} b={:?}",
            a.messages(),
            b.messages()
        );
        let state_a = last_state(a);
        let state_b = last_state(b);
        if let Some(state) = state_a.as_ref().filter(|state| is_idle_settled(state)) {
            return state.clone();
        }
        if let Some(state) = state_b.as_ref().filter(|state| is_idle_settled(state)) {
            return state.clone();
        }
        if state_a
            .as_ref()
            .is_some_and(|state| state["isYourTurn"] == json!(true))
        {
            action_and_wait(a, state_a.as_ref().unwrap()).await;
            continue;
        }
        if state_b
            .as_ref()
            .is_some_and(|state| state["isYourTurn"] == json!(true))
        {
            action_and_wait(b, state_b.as_ref().unwrap()).await;
            continue;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

#[tokio::test]
async fn hello_and_config_whitelist() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        ..Default::default()
    })
    .await;
    let mut a = TestClient::connect(&url).await;
    let hello = a.wait_type("hello").await;
    assert_eq!(hello["protocolVersion"], json!(2));
    assert_eq!(hello["serverId"], json!("texas-holdem-game"));
    assert_eq!(hello["serverName"], json!("德州扑克服务"));
    assert_eq!(hello["appVersion"], json!("1.5.8"));
    let mark = a.mark();
    a.send(json!({
        "type": "create_room",
        "requestId": "create-1",
        "name": "Alice",
        "maxPlayers": 99,
        "startingStack": 12345,
        "smallBlind": 17,
        "bigBlind": 34,
        "aiDelay": 999999,
        "isShortDeck": false,
    }))
    .await;
    let created = a
        .wait_after(mark, |value| value["type"] == json!("room_created"), WAIT)
        .await;
    let config = &created["roomConfig"];
    assert_eq!(config["maxPlayers"], json!(10));
    assert_eq!(config["startingStack"], json!(20000));
    assert_eq!(config["smallBlind"], json!(40));
    assert_eq!(config["bigBlind"], json!(80));
    assert_eq!(config["ante"], json!(40));
    assert_eq!(config["minBet"], json!(80));
    assert_eq!(
        config["aiDelay"],
        json!(450),
        "invalid aiDelay falls back to the server option"
    );
    assert_eq!(config["isShortDeck"], json!(false));
    assert_eq!(created["isHost"], json!(true));
    assert_eq!(created["resumed"], json!(false));
    assert_eq!(created["resumeToken"].as_str().map(str::len), Some(24));
    handle.shutdown().await;
}

#[tokio::test]
async fn lobby_lifecycle_and_error_texts() {
    let (handle, url) = start(test_options()).await;
    let mut a = TestClient::connect(&url).await;
    a.wait_type("hello").await;
    let mut b = TestClient::connect(&url).await;
    b.wait_type("hello").await;
    let mut outsider = TestClient::connect(&url).await;
    outsider.wait_type("hello").await;

    let mark = a.mark();
    a.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "Alice"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let created = a
        .wait_after(mark, |value| value["type"] == json!("room_created"), WAIT)
        .await;
    assert_eq!(created["roomCode"], json!("1"));
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-1", "roomCode": "1", "name": "Bob"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let joined = b
        .wait_after(mark, |value| value["type"] == json!("room_joined"), WAIT)
        .await;
    assert_eq!(joined["isHost"], json!(false));
    assert_eq!(joined["resumed"], json!(false));
    a.wait_after(
        0,
        |value| {
            value["type"] == json!("player_joined")
                && value["players"].as_array().map_or(0, Vec::len) == 2
        },
        WAIT,
    )
    .await;

    // Already-in-room join.
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-again", "roomCode": "1", "name": "Bob"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let error = b
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("join-again"),
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("INVALID_REQUEST"));
    assert_eq!(error["message"], json!("请先离开当前房间"));

    // Non-host start.
    let mark = b.mark();
    b.send(json!({"type": "start_game", "requestId": "not-host", "roomCode": "1"}))
        .await;
    let error = b
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("not-host"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("只有房主可以开始游戏"));

    // Unknown type, missing type, invalid JSON.
    let mark = a.mark();
    a.send(json!({"type": "does_not_exist", "requestId": "unknown-1"}))
        .await;
    let error = a
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("unknown-1"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("未知消息类型"));
    let mark = a.mark();
    a.send_raw("{\"requestId\":\"missing-type\"}").await;
    let error = a
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("missing-type"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("缺少消息类型"));
    let mark = a.mark();
    a.send_raw("not-json{").await;
    let error = a
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["message"] == json!("消息格式无效"),
            WAIT,
        )
        .await;
    assert_eq!(error["requestId"], Value::Null);

    // Protocol mismatch wins over dispatch.
    let mark = a.mark();
    a.send(json!({"protocolVersion": 3, "type": "get_snapshot", "requestId": "mismatch"} ))
        .await;
    let error = a
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("mismatch"),
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("PROTOCOL_MISMATCH"));
    assert_eq!(error["message"], json!("协议版本不匹配（服务端 v2）"));

    // get_snapshot in the lobby returns room_joined(resumed=true).
    let mark = a.mark();
    a.send(json!({"type": "get_snapshot"})).await;
    let snapshot = a
        .wait_after(
            mark,
            |value| value["type"] == json!("room_joined") && value["resumed"] == json!(true),
            WAIT,
        )
        .await;
    assert_eq!(snapshot["playerId"], created["playerId"]);

    // Invalid room code, room full and resume expiry.
    let mark = outsider.mark();
    outsider
        .send(with_room_options(
            json!({"type": "join_room", "requestId": "join-bad", "roomCode": "9", "name": "Carol"}),
            room_options(2, false, 20000, 800),
        ))
        .await;
    let error = outsider
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("join-bad"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("房间号只能是 1–5"));
    let mark = outsider.mark();
    outsider.send(with_room_options(json!({"type": "join_room", "requestId": "join-full", "roomCode": "1", "name": "Carol"}), room_options(2, false, 20000, 800))).await;
    let error = outsider
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("join-full"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("房间已满"));
    let mark = outsider.mark();
    outsider.send(json!({"type": "resume_session", "requestId": "resume-bad", "roomCode": "1", "resumeToken": "bogus"})).await;
    let error = outsider
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("resume-bad"),
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("RESUME_EXPIRED"));
    assert_eq!(error["message"], json!("恢复信息已失效"));
    let mark = outsider.mark();
    outsider
        .send(json!({"type": "leave_room", "requestId": "leave-outside"}))
        .await;
    outsider.send(json!({"type": "get_snapshot"})).await;
    let error = outsider
        .wait_after(mark, |value| value["type"] == json!("error"), WAIT)
        .await;
    assert_eq!(error["message"], json!("尚未进入房间"));
    handle.shutdown().await;
}

#[tokio::test]
async fn two_humans_authoritative_private_and_cross_hand() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        disconnect_grace_ms: 1500,
        ..Default::default()
    })
    .await;
    let mut a = TestClient::connect(&url).await;
    let hello = a.wait_type("hello").await;
    assert_eq!(hello["appVersion"], json!("1.5.8"));
    let mut b = TestClient::connect(&url).await;
    b.wait_type("hello").await;

    let mark = a.mark();
    a.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "Host"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let created = a
        .wait_after(mark, |value| value["type"] == json!("room_created"), WAIT)
        .await;
    let host_player_id = created["playerId"].clone();
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-1", "roomCode": "1", "name": "Guest"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let joined_b = b
        .wait_after(mark, |value| value["type"] == json!("room_joined"), WAIT)
        .await;
    let guest_player_id = joined_b["playerId"].clone();

    let mark_a = a.mark();
    let mark_b = b.mark();
    a.send(json!({"type": "start_game", "requestId": "start-1", "roomCode": "1"}))
        .await;
    let starting = a
        .wait_after(
            mark_a,
            |value| value["type"] == json!("game_starting"),
            WAIT,
        )
        .await;
    assert_eq!(starting["playerCount"], json!(2));
    assert_eq!(starting["humanCount"], json!(2));
    assert_eq!(starting["aiCount"], json!(0));
    let ack = a
        .wait_after(
            mark_a,
            |value| value["type"] == json!("ack") && value["requestId"] == json!("start-1"),
            WAIT,
        )
        .await;
    assert_eq!(ack["accepted"], json!(true));
    let state_a = a
        .wait_after(
            mark_a,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    let state_b = b
        .wait_after(
            mark_b,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    assert_eq!(state_a["stateVersion"], state_b["stateVersion"]);
    assert_eq!(state_a["yourCards"].as_array().map(Vec::len), Some(2));
    assert_eq!(state_b["yourCards"].as_array().map(Vec::len), Some(2));
    assert_ne!(state_a["yourCards"], state_b["yourCards"]);
    for player in state_a["players"].as_array().unwrap() {
        assert!(
            player.get("holeCards").is_none(),
            "hidden cards leaked: {player}"
        );
    }

    // Stale action negotiation produces action_ack rejections with exact texts.
    let (actor, actor_state) = if state_a["isYourTurn"] == json!(true) {
        (&mut a, state_a.clone())
    } else {
        (&mut b, state_b.clone())
    };
    let stale_cases = [
        ("stale-room", json!({"roomCode": "2"}), "房间状态已过期"),
        (
            "old-hand",
            json!({"handId": actor_state["handId"].as_i64().unwrap() - 1}),
            "这是一条旧牌局动作",
        ),
        (
            "old-turn",
            json!({"turnId": actor_state["turnId"].as_i64().unwrap() - 1}),
            "行动回合已经变化",
        ),
        (
            "stale-version",
            json!({"expectedStateVersion": actor_state["stateVersion"].as_i64().unwrap() - 1}),
            "牌局状态已更新，请重试",
        ),
        (
            "off-grid",
            json!({"action": "raise", "amount": 201}),
            "加注金额必须是 40 的整数倍；零头筹码可使用全下",
        ),
        (
            "out-of-range",
            json!({"action": "raise", "amount": 120}),
            "加注金额不在合法范围内",
        ),
        (
            "illegal-check",
            json!({"action": "check", "amount": 0}),
            "当前不能执行这个行动",
        ),
    ];
    for (name, overrides, expected) in stale_cases {
        let request_id = format!("reject-{name}");
        let mut payload = json!({
            "type": "player_action",
            "requestId": request_id,
            "roomCode": actor_state["roomCode"],
            "handId": actor_state["handId"],
            "turnId": actor_state["turnId"],
            "expectedStateVersion": actor_state["stateVersion"],
            "action": "fold",
            "amount": 0,
        });
        if let (Some(target), Some(source)) = (payload.as_object_mut(), overrides.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        let mark = actor.mark();
        actor.send(payload).await;
        let ack = actor
            .wait_after(
                mark,
                |value| {
                    value["type"] == json!("action_ack") && value["requestId"] == json!(request_id)
                },
                WAIT,
            )
            .await;
        assert_eq!(ack["accepted"], json!(false), "{name}");
        assert_eq!(ack["message"], json!(expected), "{name}");
    }

    // Hand 1: both humans check/call to showdown.
    let settled = drive_hand(&mut a, &mut b).await;
    assert_eq!(settled["lastHandResult"]["reason"], json!("showdown"));
    assert_eq!(settled["lastHandResult"]["pot"], json!(160));
    assert_eq!(
        settled["lastHandResult"]["results"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let revealed = settled["lastHandResult"]["results"][0]["cards"][0]["value"].as_i64();
    assert!(revealed.is_some(), "showdown cards carry numeric value");
    assert_eq!(settled["pot"], json!(0));
    let total: i64 = settled["players"]
        .as_array()
        .unwrap()
        .iter()
        .map(|player| player["stack"].as_i64().unwrap())
        .sum();
    assert_eq!(total, 40000);

    // Both ready -> hand 2 after the 250ms countdown.
    let mark_a = a.mark();
    let mark_b = b.mark();
    a.send(
        json!({"type": "ready_for_next", "requestId": "ready-a-1", "roomCode": "1", "handId": 1}),
    )
    .await;
    b.send(
        json!({"type": "ready_for_next", "requestId": "ready-b-1", "roomCode": "1", "handId": 1}),
    )
    .await;
    a.wait_after(
        mark_a,
        |value| value["type"] == json!("ready_status") && value["allReady"] == json!(true),
        WAIT,
    )
    .await;
    a.wait_after(
        mark_a,
        |value| {
            value["type"] == json!("game_state")
                && value["handId"] == json!(2)
                && value["phase"] == json!("preflop")
        },
        WAIT_LONG,
    )
    .await;
    b.wait_after(
        mark_b,
        |value| value["type"] == json!("game_state") && value["handId"] == json!(2),
        WAIT_LONG,
    )
    .await;

    // Duplicate request id replays the accepted action_ack (hand 2 folds).
    let fold_actor = if last_state(&a).unwrap()["isYourTurn"] == json!(true) {
        &mut a
    } else {
        &mut b
    };
    let turn_state = last_state(fold_actor).unwrap();
    assert_eq!(turn_state["isYourTurn"], json!(true));
    let payload = json!({
        "type": "player_action",
        "requestId": "duplicate-fold",
        "roomCode": "1",
        "handId": turn_state["handId"],
        "turnId": turn_state["turnId"],
        "expectedStateVersion": turn_state["stateVersion"],
        "action": "fold",
        "amount": 0,
    });
    let mark = fold_actor.mark();
    fold_actor.send(payload.clone()).await;
    fold_actor.send(payload).await;
    let acks = {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let acks: Vec<Value> = fold_actor
                .messages()
                .into_iter()
                .filter(|value| {
                    value["type"] == json!("action_ack")
                        && value["requestId"] == json!("duplicate-fold")
                })
                .collect();
            if acks.len() >= 2 {
                break acks;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "duplicate ack missing"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    assert!(acks.iter().all(|ack| ack["accepted"] == json!(true)));
    let idle_fold = a
        .wait_after(
            0,
            |value| {
                value["type"] == json!("game_state")
                    && value["phase"] == json!("idle")
                    && value["handId"] == json!(2)
            },
            WAIT_LONG,
        )
        .await;
    assert_eq!(idle_fold["lastHandResult"]["reason"], json!("fold"));

    // Ready for hand 3, then the host leaves mid-hand: guest becomes host and
    // the vacated seat is refilled with an AI for hand 4.
    let mark_a = a.mark();
    let mark_b = b.mark();
    a.send(
        json!({"type": "ready_for_next", "requestId": "ready-a-2", "roomCode": "1", "handId": 2}),
    )
    .await;
    b.send(
        json!({"type": "ready_for_next", "requestId": "ready-b-2", "roomCode": "1", "handId": 2}),
    )
    .await;
    a.wait_after(
        mark_a,
        |value| {
            value["type"] == json!("game_state")
                && value["handId"] == json!(3)
                && value["phase"] != json!("idle")
        },
        WAIT_LONG,
    )
    .await;
    let mark_b = b.mark();
    a.send(json!({"type": "leave_room", "requestId": "leave-host", "roomCode": "1"}))
        .await;
    let host_changed = b
        .wait_after(
            mark_b,
            |value| value["type"] == json!("host_changed") && value["isHost"] == json!(true),
            WAIT,
        )
        .await;
    assert_eq!(host_changed["hostId"], guest_player_id);
    assert_ne!(host_changed["hostId"], host_player_id);
    b.wait_after(mark_b, |value| value["type"] == json!("player_left"), WAIT)
        .await;
    let mark_b = b.mark();
    b.send(json!({"type": "ready_for_next", "requestId": "ready-after-host", "roomCode": "1", "handId": 3})).await;
    let hand4 = b
        .wait_after(
            mark_b,
            |value| {
                value["type"] == json!("game_state")
                    && value["handId"] == json!(4)
                    && value["phase"] != json!("idle")
            },
            WAIT_LONG,
        )
        .await;
    let ai_seats = hand4["players"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|player| player["isHuman"] == json!(false))
        .count();
    assert_eq!(ai_seats, 1, "vacated seat filled by AI: {hand4}");
    let mark_b = b.mark();
    b.send(json!({"type": "leave_room", "requestId": "leave-guest", "roomCode": "1"}))
        .await;
    let ack = b
        .wait_after(
            mark_b,
            |value| value["type"] == json!("ack") && value["requestId"] == json!("leave-guest"),
            WAIT,
        )
        .await;
    assert_eq!(ack["accepted"], json!(true));
    handle.shutdown().await;
}

#[tokio::test]
async fn resume_session_and_disconnect_grace() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        disconnect_grace_ms: 250,
        empty_room_ttl_ms: 300,
        ..Default::default()
    })
    .await;
    let mut a = TestClient::connect(&url).await;
    a.wait_type("hello").await;
    let mut b = TestClient::connect(&url).await;
    b.wait_type("hello").await;
    a.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "D"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    a.wait_type("room_created").await;
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-1", "roomCode": "1", "name": "E"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    let joined_b = b
        .wait_after(mark, |value| value["type"] == json!("room_joined"), WAIT)
        .await;
    let token = joined_b["resumeToken"].as_str().unwrap().to_owned();
    let player_id = joined_b["playerId"].clone();
    let mark = a.mark();
    a.send(json!({"type": "start_game", "requestId": "start-1", "roomCode": "1"}))
        .await;
    let state = a
        .wait_after(
            mark,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    let _ = state;
    let starting_cards = last_state(&b).expect("guest snapshot")["yourCards"].clone();
    let _ = player_id;

    // Abrupt disconnect, then resume from a fresh socket within the grace window.
    let mark = a.mark();
    b.force_disconnect();
    a.wait_after(mark, |value| value["type"] == json!("player_left"), WAIT)
        .await;
    let mut b2 = TestClient::connect(&url).await;
    b2.wait_type("hello").await;
    let mark = b2.mark();
    b2.send(json!({"type": "resume_session", "requestId": "resume-1", "roomCode": "1", "resumeToken": token})).await;
    let resumed = b2
        .wait_after(
            mark,
            |value| value["type"] == json!("room_joined") && value["resumed"] == json!(true),
            WAIT,
        )
        .await;
    assert_eq!(resumed["playerId"], player_id);
    let resumed_state = b2
        .wait_after(
            mark,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    assert_eq!(
        resumed_state["yourCards"], starting_cards,
        "resume restores hole cards"
    );

    // Resuming the same token elsewhere kicks the current socket with 4001.
    let mut b3 = TestClient::connect(&url).await;
    b3.wait_type("hello").await;
    let mark = b3.mark();
    b3.send(json!({"type": "resume_session", "requestId": "resume-2", "roomCode": "1", "resumeToken": token})).await;
    let (code, reason) = b2.wait_closed(WAIT).await;
    assert_eq!(code, 4001);
    assert_eq!(reason, "session resumed elsewhere");
    b3.wait_after(
        mark,
        |value| value["type"] == json!("room_joined") && value["resumed"] == json!(true),
        WAIT,
    )
    .await;

    // Grace expiry folds the disconnected seat and invalidates the token.
    b3.force_disconnect();
    let mark = a.mark();
    let idle = a
        .wait_after(
            mark,
            |value| {
                value["type"] == json!("game_state")
                    && value["phase"] == json!("idle")
                    && !value["lastHandResult"].is_null()
            },
            WAIT_LONG,
        )
        .await;
    assert_eq!(idle["lastHandResult"]["reason"], json!("fold"));
    let mut b4 = TestClient::connect(&url).await;
    b4.wait_type("hello").await;
    let mark = b4.mark();
    b4.send(json!({"type": "resume_session", "requestId": "resume-3", "roomCode": "1", "resumeToken": token})).await;
    let error = b4
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("resume-3"),
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("RESUME_EXPIRED"));
    assert_eq!(error["message"], json!("恢复信息已失效"));
    handle.shutdown().await;
}

#[tokio::test]
async fn missing_request_id_action_is_an_error() {
    let (handle, url) = start(test_options()).await;
    let mut a = TestClient::connect(&url).await;
    a.wait_type("hello").await;
    let mut b = TestClient::connect(&url).await;
    b.wait_type("hello").await;
    a.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "H"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    a.wait_type("room_created").await;
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-1", "roomCode": "1", "name": "G"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    b.wait_after(mark, |value| value["type"] == json!("room_joined"), WAIT)
        .await;
    let mark = a.mark();
    a.send(json!({"type": "start_game", "requestId": "start-1", "roomCode": "1"}))
        .await;
    let state = a
        .wait_after(
            mark,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    let actor = if state["isYourTurn"] == json!(true) {
        &mut a
    } else {
        &mut b
    };
    let state = last_state(actor).unwrap();
    let mark = actor.mark();
    actor
        .send(json!({
            "type": "player_action",
            "roomCode": "1",
            "handId": state["handId"],
            "turnId": state["turnId"],
            "expectedStateVersion": state["stateVersion"],
            "action": "fold",
            "amount": 0,
        }))
        .await;
    let error = actor
        .wait_after(
            mark,
            |value| {
                value["type"] == json!("error") && value["message"] == json!("动作缺少 requestId")
            },
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("INVALID_REQUEST"));
    assert_eq!(error["requestId"], Value::Null);
    handle.shutdown().await;
}

#[tokio::test]
async fn room_limits_and_message_throttling() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        max_rooms: 1,
        max_connections: 8,
        max_connections_per_ip: 8,
        max_messages_per_window: 20,
        rate_window_ms: 60000,
        ..Default::default()
    })
    .await;
    let mut first = TestClient::connect(&url).await;
    first.wait_type("hello").await;
    first
        .send(with_room_options(
            json!({"type": "create_room", "requestId": "create-1", "name": "One"}),
            room_options(2, false, 20000, 800),
        ))
        .await;
    first.wait_type("room_created").await;
    let mut second = TestClient::connect(&url).await;
    second.wait_type("hello").await;
    let mark = second.mark();
    second
        .send(with_room_options(
            json!({"type": "create_room", "requestId": "create-limit", "name": "Two"}),
            room_options(2, false, 20000, 800),
        ))
        .await;
    let error = second
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("create-limit"),
            WAIT,
        )
        .await;
    assert_eq!(error["message"], json!("房间已满（最多 1 个）"));
    let mark = second.mark();
    second
        .send(with_room_options(
            json!({"type": "join_room", "requestId": "join-limit", "roomCode": "2", "name": "Two"}),
            room_options(2, false, 20000, 800),
        ))
        .await;
    let error = second
        .wait_after(
            mark,
            |value| value["type"] == json!("error") && value["requestId"] == json!("join-limit"),
            WAIT,
        )
        .await;
    assert_eq!(error["code"], json!("ROOM_LIMIT"));
    assert_eq!(error["message"], json!("房间已满（最多 1 个）"));

    // 21 messages within one window close the socket with 1008.
    for index in 0..25 {
        if second.is_closed() {
            break;
        }
        let _ = index;
        let _ = second
            .send_raw("{\"protocolVersion\":2,\"type\":\"unknown_type\",\"requestId\":\"flood\"}")
            .await;
    }
    let (code, reason) = second.wait_closed(WAIT).await;
    assert_eq!(code, 1008);
    assert!(reason.contains("rate"), "unexpected reason: {reason}");
    handle.shutdown().await;
}

#[tokio::test]
async fn connection_capacity_close_code() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        max_connections: 8,
        max_connections_per_ip: 2,
        ..Default::default()
    })
    .await;
    let mut first = TestClient::connect(&url).await;
    first.wait_type("hello").await;
    let mut second = TestClient::connect(&url).await;
    second.wait_type("hello").await;
    let third = TestClient::connect(&url).await;
    let (code, reason) = third.wait_closed(WAIT).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "server capacity reached");
    handle.shutdown().await;
}

#[tokio::test]
async fn origin_allowlist_rejects_before_upgrade() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let (handle, url) = start(ServerOptions {
        port: 0,
        allowed_origins: vec!["https://cards.example".to_owned()],
        ..Default::default()
    })
    .await;
    let mut request = url.as_str().into_client_request().expect("request");
    request.headers_mut().insert(
        "Origin",
        "https://untrusted.example".parse().expect("header"),
    );
    let rejected = tokio_tungstenite::connect_async(request).await;
    assert!(
        rejected.is_err(),
        "disallowed origin must fail the handshake"
    );
    let mut allowed = TestClient::connect_with_origin(&url, Some("https://cards.example")).await;
    allowed.wait_type("hello").await;
    handle.shutdown().await;
}

#[tokio::test]
async fn oversized_frame_closes_with_1009() {
    let (handle, url) = start(test_options()).await;
    let mut client = TestClient::connect(&url).await;
    client.wait_type("hello").await;
    client.send_raw(&"x".repeat(20 * 1024)).await;
    let (code, _reason) = client.wait_closed(WAIT).await;
    assert_eq!(code, 1009);
    handle.shutdown().await;
}

#[tokio::test]
async fn short_deck_ai_table_plays_a_turn() {
    let (handle, url) = start(test_options()).await;
    let mut host = TestClient::connect(&url).await;
    host.wait_type("hello").await;
    host.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "Shorty"}),
        room_options(3, true, 20000, 800),
    ))
    .await;
    let created = host.wait_type("room_created").await;
    assert_eq!(created["roomConfig"]["isShortDeck"], json!(true));
    assert_eq!(created["roomConfig"]["aiDelay"], json!(800));
    let mark = host.mark();
    host.send(json!({"type": "start_game", "requestId": "start-1", "roomCode": "1"}))
        .await;
    let starting = host
        .wait_after(mark, |value| value["type"] == json!("game_starting"), WAIT)
        .await;
    assert_eq!(starting["humanCount"], json!(1));
    assert_eq!(starting["aiCount"], json!(2));
    let initial = host
        .wait_after(
            mark,
            |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
            WAIT,
        )
        .await;
    assert_eq!(initial["isShortDeck"], json!(true));
    assert_eq!(initial["players"].as_array().map(Vec::len), Some(3));
    let mut initial = initial;
    if initial["isYourTurn"] == json!(true) {
        action_and_wait(&mut host, &initial).await;
        initial = last_state(&host).unwrap();
    }
    let start_turn = initial["turnId"].as_i64().unwrap();
    let ai_state = {
        let deadline = tokio::time::Instant::now() + WAIT_LONG;
        loop {
            if let Some(state) = last_state(&host) {
                if state["turnId"].as_i64().unwrap_or(0) > start_turn
                    && state["players"].as_array().is_some_and(|players| {
                        players.iter().any(|player| {
                            player["isHuman"] == json!(false)
                                && ["fold", "check", "call", "raise", "allin"].contains(
                                    &player["lastAction"]["action"].as_str().unwrap_or(""),
                                )
                        })
                    })
                {
                    break state;
                }
                if state["isYourTurn"] == json!(true) {
                    action_and_wait(&mut host, &state).await;
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "AI seat never acted; messages: {:?}",
                host.messages()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    assert_eq!(ai_state["phase"], json!("preflop"));
    let mark = host.mark();
    host.send(json!({"type": "leave_room", "requestId": "leave-1", "roomCode": "1"}))
        .await;
    host.wait_after(
        mark,
        |value| value["type"] == json!("ack") && value["requestId"] == json!("leave-1"),
        WAIT,
    )
    .await;
    handle.shutdown().await;
}

#[tokio::test]
async fn empty_room_ttl_frees_the_slot() {
    let (handle, url) = start(ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        disconnect_grace_ms: 100,
        empty_room_ttl_ms: 300,
        ..Default::default()
    })
    .await;
    let mut a = TestClient::connect(&url).await;
    a.wait_type("hello").await;
    let mut b = TestClient::connect(&url).await;
    b.wait_type("hello").await;
    a.send(with_room_options(
        json!({"type": "create_room", "requestId": "create-1", "name": "A"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    a.wait_type("room_created").await;
    let mark = b.mark();
    b.send(with_room_options(
        json!({"type": "join_room", "requestId": "join-1", "roomCode": "1", "name": "B"}),
        room_options(2, false, 20000, 800),
    ))
    .await;
    b.wait_after(mark, |value| value["type"] == json!("room_joined"), WAIT)
        .await;
    let mark = a.mark();
    a.send(json!({"type": "start_game", "requestId": "start-1", "roomCode": "1"}))
        .await;
    a.wait_after(
        mark,
        |value| value["type"] == json!("game_state") && value["handId"] == json!(1),
        WAIT,
    )
    .await;
    a.force_disconnect();
    b.force_disconnect();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let mut fresh = TestClient::connect(&url).await;
    fresh.wait_type("hello").await;
    let mark = fresh.mark();
    fresh
        .send(with_room_options(
            json!({"type": "create_room", "requestId": "create-new", "name": "Fresh"}),
            room_options(2, false, 20000, 800),
        ))
        .await;
    let created = fresh
        .wait_after(mark, |value| value["type"] == json!("room_created"), WAIT)
        .await;
    assert_eq!(
        created["roomCode"],
        json!("1"),
        "expired room slot must be reusable"
    );
    handle.shutdown().await;
}
