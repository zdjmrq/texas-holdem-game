//! Connection lifecycle tests against a scripted tokio-tungstenite mock
//! server: hello handshake, terminal close codes, backoff sequence, resume.

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use poker_client::{Client, ClientConfig, ClientError, ClientEvent, HelloPolicy};
use poker_protocol::ServerMessage;
use support::{
    drain_recorded, events_until, golden_json, golden_lines, next_event, read_value, record,
    record_until_closed, send_close, send_hello, send_value, serve, test_config, test_session,
};

#[tokio::test]
async fn connects_and_exposes_hello() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    assert!(client.is_connected());
    assert_eq!(
        client.hello().map(|hello| hello.app_version.as_str()),
        Some("1.5.8")
    );
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Connected {
            reconnecting: false
        }
    ));
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Hello(_)
    ));
}

#[tokio::test]
async fn hello_policy_mismatch_fails_the_handshake() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let config = test_config(&server).with_hello_policy(HelloPolicy::desktop_probe("2.0.0"));
    let error = Client::connect(config).await.expect_err("mismatch");
    assert!(matches!(error, ClientError::Hello(_)), "{error:?}");
}

#[tokio::test]
async fn handshake_timeout_fires_when_hello_never_arrives() {
    let server = serve(|_index, mut ws, _sink| async move {
        // Accept the socket but never send hello.
        while read_value(&mut ws).await.is_some() {}
    })
    .await;
    let config =
        ClientConfig::new(server.url.clone()).with_handshake_timeout(Duration::from_millis(100));
    let error = Client::connect(config).await.expect_err("timeout");
    assert!(
        matches!(error, ClientError::HandshakeTimeout { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn terminal_close_codes_never_reconnect() {
    for event in golden_lines("close-events.jsonl") {
        let Some(code) = event["code"]
            .as_u64()
            .and_then(|code| u16::try_from(code).ok())
        else {
            continue; // the http-upgrade row has no ws close code
        };
        let reason = event["reason"].as_str().unwrap_or_default().to_owned();
        let reason_for_server = reason.clone();
        let server = serve(move |_index, mut ws, sink| {
            let reason = reason_for_server.clone();
            async move {
                send_hello(&mut ws).await;
                send_close(&mut ws, code, &reason).await;
                record_until_closed(&mut ws, &sink).await;
            }
        })
        .await;
        let mut client = Client::connect(test_config(&server))
            .await
            .expect("connect");
        client.set_session(test_session(&server));
        let events = events_until(&mut client, 8, ClientEvent::is_closed).await;
        let disconnected = events.iter().find_map(|event| match event {
            ClientEvent::Disconnected {
                code,
                reason,
                will_reconnect,
            } => Some((*code, reason.clone(), *will_reconnect)),
            _ => None,
        });
        let Some((received_code, received_reason, will_reconnect)) = disconnected else {
            panic!("missing Disconnected event: {events:?}");
        };
        assert_eq!(received_code, Some(code));
        assert_eq!(received_reason, reason);
        assert!(!will_reconnect, "terminal code {code} must not reconnect");
        assert!(client.next_event().await.is_none());
        assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn reconnect_backoff_matches_golden_sequence() {
    let server = serve(|index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        if index < 6 {
            // Returning drops the TCP connection: an abnormal (1006-like) close.
            return;
        }
        record_until_closed(&mut ws, &sink).await;
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    client.set_session(test_session(&server));

    let mut delays = Vec::new();
    let mut reconnected = false;
    for _ in 0..64 {
        match next_event(&mut client).await {
            ClientEvent::Reconnecting { attempt, delay_ms } => delays.push((attempt, delay_ms)),
            ClientEvent::Connected { reconnecting: true } => {
                // A connection that the server drops right after hello counts as
                // a failed attempt; only accept the successful one at the end.
                if delays.len() >= 6 {
                    reconnected = true;
                    break;
                }
            }
            ClientEvent::ReconnectFailed { .. } => panic!("unexpected give-up"),
            _ => {}
        }
    }
    assert!(reconnected, "expected a successful reconnect");
    let golden = golden_json("client-behaviour.json");
    let expected: Vec<(u32, u64)> = golden["reconnectDelaysMs"]
        .as_array()
        .expect("reconnectDelaysMs")
        .iter()
        .map(|entry| {
            (
                u32::try_from(entry["attempt"].as_u64().expect("attempt")).expect("attempt u32"),
                entry["delayMs"].as_u64().expect("delayMs"),
            )
        })
        .collect();
    assert_eq!(delays, expected);
    assert_eq!(server.connections.load(Ordering::SeqCst), 7);
}

#[tokio::test(start_paused = true)]
async fn reconnect_gives_up_after_max_attempts() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_hello(&mut ws).await;
        // Drop immediately: every retry fails.
    })
    .await;
    let config = test_config(&server).with_max_reconnect_attempts(Some(2));
    let mut client = Client::connect(config).await.expect("connect");
    client.set_session(test_session(&server));

    let events = events_until(&mut client, 32, ClientEvent::is_closed).await;
    let attempts: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            ClientEvent::Reconnecting { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, vec![0, 1]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::ReconnectFailed { attempts: 2, .. })),
        "{events:?}"
    );
    assert_eq!(server.connections.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn reconnect_requires_a_stored_session() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_hello(&mut ws).await;
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    // No session stored: the reference client does not reconnect either.
    let events = events_until(&mut client, 8, ClientEvent::is_closed).await;
    assert!(events.iter().any(|event| matches!(
        event,
        ClientEvent::Disconnected {
            will_reconnect: false,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ClientEvent::Reconnecting { .. }))
    );
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn auto_reconnect_can_be_disabled() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_hello(&mut ws).await;
    })
    .await;
    let config = test_config(&server).with_auto_reconnect(false);
    let mut client = Client::connect(config).await.expect("connect");
    client.set_session(test_session(&server));
    let events = events_until(&mut client, 8, ClientEvent::is_closed).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ClientEvent::Reconnecting { .. }))
    );
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn resume_session_is_sent_automatically_after_reconnect() {
    let server = serve(|index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        if index == 0 {
            return;
        }
        while let Some(frame) = read_value(&mut ws).await {
            if frame["type"].as_str() == Some("resume_session") {
                record(&sink, frame);
                send_value(&mut ws, &support::room_joined_json("1", true)).await;
                continue;
            }
            record(&sink, frame);
        }
    })
    .await;
    let mut server = server;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    client.set_session(test_session(&server));

    let events = events_until(&mut client, 32, |event| {
        matches!(
            event,
            ClientEvent::Message(ServerMessage::RoomJoined(room)) if room.resumed
        )
    })
    .await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ClientEvent::Connected { reconnecting: true }))
    );

    let frames = drain_recorded(&mut server, 1).await;
    let resume = &frames[0];
    assert_eq!(resume["type"].as_str(), Some("resume_session"));
    assert_eq!(resume["protocolVersion"], 2);
    assert_eq!(resume["roomCode"].as_str(), Some("1"));
    assert_eq!(
        resume["resumeToken"].as_str(),
        Some("resume-token-0123456789")
    );
    assert!(
        resume["requestId"]
            .as_str()
            .is_some_and(|request_id| request_id.starts_with("resume-")),
        "unexpected requestId: {}",
        resume["requestId"]
    );
}

#[tokio::test(start_paused = true)]
async fn manual_reconnect_emits_reconnecting_connected_and_resumes() {
    let server = serve(|_index, mut ws, sink| async move {
        send_hello(&mut ws).await;
        while let Some(frame) = read_value(&mut ws).await {
            record(&sink, frame);
        }
    })
    .await;
    let mut server = server;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    // Drain the events of the initial connection.
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Connected {
            reconnecting: false
        }
    ));
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Hello(_)
    ));
    client.set_session(test_session(&server));

    client.reconnect().await.expect("manual reconnect");
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Connected { reconnecting: true }
    ));
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Hello(_)
    ));
    let frames = drain_recorded(&mut server, 1).await;
    assert_eq!(frames[0]["type"].as_str(), Some("resume_session"));
    assert_eq!(server.connections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn disconnect_emits_terminal_events() {
    let server = serve(|_index, mut ws, _sink| async move {
        send_hello(&mut ws).await;
        while read_value(&mut ws).await.is_some() {}
    })
    .await;
    let mut client = Client::connect(test_config(&server))
        .await
        .expect("connect");
    // Drain the initial connection events first.
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Connected { .. }
    ));
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Hello(_)
    ));
    client.disconnect().await;
    assert!(matches!(
        next_event(&mut client).await,
        ClientEvent::Disconnected {
            code: Some(1000),
            will_reconnect: false,
            ..
        }
    ));
    assert!(matches!(next_event(&mut client).await, ClientEvent::Closed));
    assert!(client.next_event().await.is_none());
    let error = client.get_snapshot().await.expect_err("not connected");
    assert!(matches!(error, ClientError::NotConnected), "{error:?}");
}

#[tokio::test]
async fn rejects_invalid_and_tls_urls() {
    let error = Client::connect(ClientConfig::new("http://127.0.0.1:1"))
        .await
        .expect_err("invalid url");
    assert!(matches!(error, ClientError::InvalidUrl(_)), "{error:?}");
    let error = Client::connect(ClientConfig::new("wss://127.0.0.1:1"))
        .await
        .expect_err("tls url");
    assert!(
        matches!(error, ClientError::UnsupportedScheme(_)),
        "{error:?}"
    );
}
