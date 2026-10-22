//! Request construction tests: request ids, outbound envelope, frame limits
//! and send failures.

mod support;

use std::time::Duration;

use poker_client::{
    ActionRequest, Client, ClientError, ClientEvent, RequestKind, SessionCredentials,
};
use poker_protocol::{ActionType, RoomOptionsInput};
use support::{
    drain_recorded, golden_message_lines, next_event, read_value, record_until_closed, send_hello,
    serve, test_config, test_session,
};

#[tokio::test]
async fn convenience_requests_match_reference_wire_shape() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut server = server;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    client.set_session(test_session(&server));

    let create = client
        .create_room("玩家", RoomOptionsInput::default())
        .await
        .expect("create_room");
    let join = client
        .join_room("1", "玩家", RoomOptionsInput::default())
        .await
        .expect("join_room");
    let leave = client.leave_room("1").await.expect("leave_room");
    let start = client.start_game("1").await.expect("start_game");
    let action = client
        .player_action(ActionRequest::new("1", 3, 7, 42, ActionType::Raise).with_amount(200))
        .await
        .expect("player_action");
    let ready = client.ready_for_next("1", 3).await.expect("ready_for_next");
    client.get_snapshot().await.expect("get_snapshot");
    let resume = client.resume_session().await.expect("resume_session");

    let frames = drain_recorded(&mut server, 8).await;
    let expected: Vec<(&str, Option<&poker_protocol::RequestId>)> = vec![
        ("create_room", Some(&create)),
        ("join_room", Some(&join)),
        ("leave_room", Some(&leave)),
        ("start_game", Some(&start)),
        ("player_action", Some(&action)),
        ("ready_for_next", Some(&ready)),
        ("get_snapshot", None),
        ("resume_session", Some(&resume)),
    ];
    assert_eq!(frames.len(), expected.len());
    for (frame, (message_type, request_id)) in frames.iter().zip(expected) {
        assert_eq!(frame["type"].as_str(), Some(message_type));
        assert_eq!(frame["protocolVersion"], 2);
        match request_id {
            Some(request_id) => assert_eq!(
                frame["requestId"].as_str(),
                request_id.as_str(),
                "requestId echo for {message_type}"
            ),
            None => assert!(
                frame.get("requestId").is_none(),
                "get_snapshot must not carry a requestId (js/network.js never sends it)"
            ),
        }
    }
    assert_eq!(frames[0]["name"].as_str(), Some("玩家"));
    assert_eq!(frames[1]["roomCode"].as_str(), Some("1"));
    assert_eq!(frames[3]["roomCode"].as_str(), Some("1"));
    assert_eq!(frames[4]["handId"].as_i64(), Some(3));
    assert_eq!(frames[4]["turnId"].as_i64(), Some(7));
    assert_eq!(frames[4]["expectedStateVersion"].as_i64(), Some(42));
    assert_eq!(frames[4]["action"].as_str(), Some("raise"));
    assert_eq!(frames[4]["amount"].as_i64(), Some(200));
    assert_eq!(frames[5]["handId"].as_i64(), Some(3));
    assert_eq!(
        frames[7]["resumeToken"].as_str(),
        Some("resume-token-0123456789")
    );
    assert!(
        frames[7]["requestId"]
            .as_str()
            .is_some_and(|id| id.starts_with("resume-")),
        "unexpected resume request id: {}",
        frames[7]["requestId"]
    );
    // request ids are monotonic per generator: sequence order follows the calls.
    let request_ids: Vec<&str> = frames
        .iter()
        .filter_map(|frame| frame["requestId"].as_str())
        .collect();
    let sequences: Vec<&str> = request_ids
        .iter()
        .map(|id| id.rsplit('-').next().expect("sequence"))
        .collect();
    assert_eq!(sequences, vec!["1", "2", "3", "4", "5", "6", "7"]);
}

#[tokio::test]
async fn oversized_frames_are_rejected_before_writing() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut server = server;
    let config = test_config(&server).with_max_frame_bytes(64);
    let mut client = Client::connect(config).await.expect("connect");
    let long_name = "x".repeat(200);
    let error = client
        .create_room(long_name, RoomOptionsInput::default())
        .await
        .expect_err("frame too large");
    assert!(
        matches!(error, ClientError::MessageTooLarge { .. }),
        "{error:?}"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), server.received.recv())
            .await
            .is_err(),
        "nothing must be written for an oversized frame"
    );
}

#[tokio::test]
async fn sends_after_a_terminal_close_report_not_connected() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        support::send_close(&mut ws, 1013, "server capacity reached").await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    // Drain until the terminal disconnect is reported.
    for _ in 0..8 {
        if matches!(next_event(&mut client).await, ClientEvent::Closed) {
            break;
        }
    }
    let error = client.resume_session().await.expect_err("no session");
    assert!(matches!(error, ClientError::NoSession), "{error:?}");
    let error = client.get_snapshot().await.expect_err("not connected");
    assert!(matches!(error, ClientError::NotConnected), "{error:?}");
}

#[tokio::test]
async fn c2s_golden_payloads_roundtrip_through_the_client() {
    let mut sendable = Vec::new();
    for frame in golden_message_lines("c2s-messages.jsonl") {
        let Ok(message) = poker_protocol::parse_client_message(&frame.to_string()) else {
            continue; // unknown_type_here is deliberately invalid
        };
        sendable.push((frame, message));
    }
    assert!(!sendable.is_empty(), "no sendable golden messages");
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut server = server;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    for (_, message) in &sendable {
        client.send(message.clone()).await.expect("send golden");
    }
    let frames = drain_recorded(&mut server, sendable.len()).await;
    for ((golden, _), sent) in sendable.iter().zip(frames.iter()) {
        assert_eq!(sent["type"], golden["type"]);
        assert_eq!(sent["protocolVersion"], 2);
        let golden_request_id = golden["requestId"].as_str();
        if let Some(golden_request_id) = golden_request_id {
            assert_eq!(sent["requestId"].as_str(), Some(golden_request_id));
        }
        // Every field the reference payload carries must survive the roundtrip
        // with the same JSON value (keys are unordered).
        if let (Some(golden_map), Some(sent_map)) = (golden.as_object(), sent.as_object()) {
            for (key, value) in golden_map {
                if key == "protocolVersion" || key == "requestId" {
                    continue;
                }
                assert_eq!(
                    sent_map.get(key),
                    Some(value),
                    "field {key} changed for {}",
                    golden["type"]
                );
            }
        }
    }
}

#[tokio::test]
async fn disconnect_keeps_the_session_until_cleared() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_hello(&mut ws).await;
        while read_value(&mut ws).await.is_some() {}
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    client.set_session(SessionCredentials::new(server.url.clone(), "2", "token-2"));
    client.disconnect().await;
    assert!(client.session().is_some());
    assert_eq!(
        client.clear_session().map(|s| s.room_code),
        Some("2".to_owned())
    );
    assert!(client.session().is_none());
}

#[tokio::test]
async fn request_id_generator_is_exposed() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    let first = client.new_request_id(RequestKind::Action);
    let second = client.new_request_id(RequestKind::Join);
    assert!(
        first
            .as_str()
            .is_some_and(|id| id.starts_with("action-") && id.ends_with("-1")),
        "unexpected id: {first:?}"
    );
    assert!(
        second
            .as_str()
            .is_some_and(|id| id.starts_with("join-") && id.ends_with("-2")),
        "unexpected id: {second:?}"
    );
}
