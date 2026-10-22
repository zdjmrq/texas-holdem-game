//! Server-message streaming tests: golden corpus parse, protocol errors,
//! unknown-type tolerance and in-order delivery.

mod support;

use poker_client::{Client, ClientError, ClientEvent};
use poker_protocol::ServerMessage;
use serde_json::json;
use support::{
    golden_message_lines, next_event, read_value, send_hello, send_text, send_value, serve,
    test_config,
};

#[tokio::test]
async fn streams_every_golden_server_message_in_order() {
    let golden: Vec<serde_json::Value> = golden_message_lines("s2c-messages.jsonl")
        .into_iter()
        .filter(|frame| frame["type"].as_str() != Some("hello"))
        .collect();
    let expected: Vec<ServerMessage> = golden
        .iter()
        .map(|frame| {
            poker_protocol::parse_server_message_value(frame.clone()).expect("golden parse")
        })
        .collect();
    let frames = golden.clone();
    let server = serve(move |_index, mut ws, _sink| {
        let frames = frames.clone();
        async move {
            send_hello(&mut ws).await;
            for frame in &frames {
                send_value(&mut ws, frame).await;
            }
            while read_value(&mut ws).await.is_some() {}
        }
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    let mut received = Vec::new();
    while received.len() < expected.len() {
        if let ClientEvent::Message(message) = next_event(&mut client).await {
            received.push(message);
        }
    }
    assert_eq!(received.len(), expected.len());
    assert_eq!(received, expected, "event stream must preserve wire order");
}

#[tokio::test]
async fn protocol_errors_keep_the_stream_alive_and_unknown_types_are_ignored() {
    let invalid_shape = serde_json::to_string(&json!({"type": "hello"})).expect("json");
    let invalid_shape_for_assert = invalid_shape.clone();
    let server = serve(move |_index, mut ws, _sink| {
        let invalid_shape = invalid_shape.clone();
        async move {
            send_hello(&mut ws).await;
            send_text(&mut ws, "not json").await;
            send_text(&mut ws, &invalid_shape).await;
            send_value(&mut ws, &json!({"type": "player_disconnected"})).await;
            send_value(
                &mut ws,
                &json!({
                    "type": "ack",
                    "protocolVersion": 2,
                    "requestId": "req-1",
                    "accepted": true,
                    "roomCode": null,
                    "stateVersion": 3,
                    "handId": 0,
                }),
            )
            .await;
            while read_value(&mut ws).await.is_some() {}
        }
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Connected { .. }
    ));
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Hello(_)
    ));
    match next_event(&mut client).await {
        ClientEvent::ProtocolError { raw, .. } => assert_eq!(raw, "not json"),
        other => panic!("unexpected event: {other:?}"),
    }
    match next_event(&mut client).await {
        ClientEvent::ProtocolError { raw, .. } => assert_eq!(raw, invalid_shape_for_assert),
        other => panic!("unexpected event: {other:?}"),
    }
    // The unknown player_disconnected type is ignored (js/network.js default).
    match next_event(&mut client).await {
        ClientEvent::Message(ServerMessage::Ack(ack)) => {
            assert_eq!(ack.request_id.as_str(), Some("req-1"));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn connect_requires_hello_as_the_first_frame() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_value(
            &mut ws,
            &json!({
                "type": "ack",
                "protocolVersion": 2,
                "accepted": true,
                "roomCode": null,
                "stateVersion": 0,
                "handId": 0,
            }),
        )
        .await;
        while read_value(&mut ws).await.is_some() {}
    })
    .await;
    let error = Client::connect(test_config(&server))
        .await
        .expect_err("non-hello first frame");
    assert!(matches!(error, ClientError::Protocol { .. }), "{error:?}");
}

#[tokio::test]
async fn error_messages_are_typed_with_reference_text() {
    let error_frame = golden_message_lines("s2c-messages.jsonl")
        .into_iter()
        .find(|frame| {
            frame["type"].as_str() == Some("error")
                && frame["code"].as_str() == Some("RESUME_EXPIRED")
        })
        .expect("golden error message");
    let expected_message = error_frame["message"].as_str().expect("message").to_owned();
    let frame = error_frame.clone();
    let server = serve(move |_index, mut ws, _sink| {
        let frame = frame.clone();
        async move {
            send_hello(&mut ws).await;
            send_value(&mut ws, &frame).await;
            while read_value(&mut ws).await.is_some() {}
        }
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    let mut found = None;
    for _ in 0..8 {
        if let ClientEvent::Message(ServerMessage::Error(error)) = next_event(&mut client).await {
            found = Some(error);
            break;
        }
    }
    let error = found.expect("error message");
    assert_eq!(error.code.as_str(), "RESUME_EXPIRED");
    assert_eq!(error.message, expected_message);
}

#[tokio::test]
async fn game_state_dedupe_is_left_to_the_caller() {
    // js/network.js filters stale stateVersion values in the UI layer; the
    // transport must not drop anything.
    let states: Vec<serde_json::Value> = golden_message_lines("s2c-messages.jsonl")
        .into_iter()
        .filter(|frame| frame["type"].as_str() == Some("game_state"))
        .take(2)
        .collect();
    assert_eq!(states.len(), 2);
    let frames = states.clone();
    let server = serve(move |_index, mut ws, _sink| {
        let frames = frames.clone();
        async move {
            send_hello(&mut ws).await;
            // Send the newer message first on purpose.
            for frame in frames.iter().rev() {
                send_value(&mut ws, frame).await;
            }
            while read_value(&mut ws).await.is_some() {}
        }
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    let mut versions = Vec::new();
    for _ in 0..8 {
        if let ClientEvent::Message(ServerMessage::GameState(state)) = next_event(&mut client).await
        {
            versions.push(state.state_version);
            if versions.len() == 2 {
                break;
            }
        }
    }
    let expected: Vec<i64> = states
        .iter()
        .rev()
        .map(|frame| frame["stateVersion"].as_i64().expect("stateVersion"))
        .collect();
    assert_eq!(versions, expected);
}
