//! Phase 5 compatibility gates: the Rust server is driven by the reference JS
//! protocol client, and a Rust protocol client drives the reference JS server.
//!
//! These tests need Node plus the read-only reference checkout (with its ws
//! dependency). They skip with a printed notice when that environment is not
//! available; the acceptance run always exercises both directions.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use poker_protocol as proto;
use poker_server::{ServerOptions, spawn_server};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn reference_repo() -> PathBuf {
    std::env::var("REFERENCE_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("../reference/texas-holdem-game"))
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn js_client_ready(script: &Path) -> Option<PathBuf> {
    let reference = reference_repo();
    let ws = reference.join("node_modules/ws");
    if !script.exists() || !ws.exists() || !node_available() {
        eprintln!(
            "[interop] skipping: need node and reference repo with ws ({})",
            reference.display()
        );
        return None;
    }
    Some(reference)
}

fn read_transcript(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).expect("transcript file");
    serde_json::from_str(&text).expect("transcript json")
}

#[tokio::test]
async fn reference_js_client_drives_rust_server() {
    let workspace = workspace_root();
    let script = workspace.join("oracle/interop-js-client.js");
    let Some(reference) = js_client_ready(&script) else {
        return;
    };
    let transcript_path = workspace.join("target/interop-js-transcript.json");
    let _ = std::fs::remove_file(&transcript_path);
    let handle = spawn_server(ServerOptions {
        port: 0,
        ai_delay_ms: 0,
        disconnect_grace_ms: 400,
        rng_seed: Some(2026),
        ..Default::default()
    })
    .await
    .expect("rust server start");
    let url = handle.url();
    let run = tokio::task::spawn_blocking(move || {
        Command::new("node")
            .arg(&script)
            .arg("--url")
            .arg(&url)
            .arg("--transcript")
            .arg(&transcript_path)
            .env("REFERENCE_REPO", &reference)
            .output()
            .expect("run interop js client")
    })
    .await
    .expect("join interop task");
    assert!(
        run.status.success(),
        "JS interop client failed (status {})\nstdout: {}\nstderr: {}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let transcript = read_transcript(&workspace.join("target/interop-js-transcript.json"));
    assert_eq!(transcript["ok"], json!(true), "transcript: {transcript}");
    let checks = transcript["checks"].as_array().expect("checks");
    assert!(checks.len() >= 10, "too few interop checks: {checks:?}");
    let labels: Vec<&str> = checks
        .iter()
        .filter_map(|check| check["label"].as_str())
        .collect();
    for expected in [
        "hello",
        "create_room",
        "game_started",
        "hand_1_settled",
        "resume_session",
        "resume_kicks_old_socket",
        "host_migration",
        "ai_refill",
        "leave_room",
    ] {
        assert!(
            labels.contains(&expected),
            "missing interop check {expected}"
        );
    }
    eprintln!(
        "[interop] JS client -> Rust server checks: {}",
        checks.len()
    );
    eprintln!(
        "[interop] transcript: {}",
        workspace
            .join("target/interop-js-transcript.json")
            .display()
    );
    handle.shutdown().await;
}

struct JsServerProcess {
    child: Child,
}

impl Drop for JsServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_reference_js_server(script: &Path, reference: &Path) -> Option<(JsServerProcess, String)> {
    let mut child = Command::new("node")
        .arg(script)
        .env("REFERENCE_REPO", reference)
        .env("POKER_PORT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let parsed: Value = serde_json::from_str(&line).ok()?;
    let url = parsed["url"].as_str()?.to_owned();
    Some((JsServerProcess { child }, url))
}

struct ProtocolClient {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl ProtocolClient {
    async fn connect(url: &str) -> Self {
        let (socket, _response) = connect_async(url).await.expect("connect reference server");
        Self { socket }
    }

    async fn send(&mut self, message: &proto::ClientMessage) {
        self.socket
            .send(Message::Text(message.to_json_string().into()))
            .await
            .expect("send client message");
    }

    async fn wait_for(
        &mut self,
        predicate: impl Fn(&proto::ServerMessage) -> bool,
        timeout: Duration,
    ) -> proto::ServerMessage {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            assert!(
                now < deadline,
                "timeout waiting for reference-server message"
            );
            let remaining = deadline - now;
            let incoming = tokio::time::timeout(remaining, self.socket.next())
                .await
                .expect("reference server timeout")
                .expect("reference server socket")
                .expect("reference server frame");
            if let Message::Text(text) = incoming {
                let message =
                    proto::parse_server_message(text.as_str()).expect("v2 server message");
                if predicate(&message) {
                    return message;
                }
            }
        }
    }

    /// Waits for the next frame of any type (used to pump the interleaved
    /// per-player snapshots of the reference server).
    async fn next_message(&mut self, timeout: Duration) -> proto::ServerMessage {
        self.wait_for(|_| true, timeout).await
    }
}

fn envelope(request_id: &str) -> proto::ClientEnvelope {
    proto::ClientEnvelope::with_request_id(request_id)
}

fn room_options(max_players: i64) -> proto::RoomOptionsInput {
    proto::RoomOptionsInput {
        max_players: Some(json!(max_players)),
        is_short_deck: Some(json!(false)),
        starting_stack: Some(json!(20000)),
        small_blind: Some(json!(40)),
        big_blind: Some(json!(80)),
        ante: Some(json!(40)),
        min_bet: Some(json!(80)),
        ai_delay: Some(json!(999999)),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_protocol_client_drives_reference_js_server() {
    let workspace = workspace_root();
    let script = workspace.join("oracle/interop-rust-client.js");
    let Some(reference) = js_client_ready(&script) else {
        return;
    };
    let Some((_process, url)) = start_reference_js_server(&script, &reference) else {
        eprintln!("[interop] skipping: reference JS server did not start");
        return;
    };
    assert!(
        poker_server::probe_server(&url, "1.5.8", Duration::from_secs(3)).await,
        "probe accepts the reference server"
    );

    let mut client = ProtocolClient::connect(&url).await;
    let hello = client
        .wait_for(
            |message| matches!(message, proto::ServerMessage::Hello(_)),
            Duration::from_secs(3),
        )
        .await;
    let proto::ServerMessage::Hello(hello) = hello else {
        panic!("hello expected")
    };
    assert!(
        hello.is_desktop_probe_compatible("1.5.8"),
        "hello probe fields: {hello:?}"
    );

    client
        .send(&proto::ClientMessage::CreateRoom(proto::CreateRoom {
            envelope: envelope("rust-create-1"),
            name: Some("Rust-Client".to_owned()),
            options: room_options(3),
        }))
        .await;
    let created = client
        .wait_for(
            |message| matches!(message, proto::ServerMessage::RoomCreated(_)),
            Duration::from_secs(3),
        )
        .await;
    let proto::ServerMessage::RoomCreated(created) = created else {
        panic!("room_created expected")
    };
    assert_eq!(created.room_config.max_players, 3);
    assert_eq!(
        created.room_config.ai_delay, 0,
        "invalid aiDelay falls back to the JS server option"
    );
    let room_code = created.room_code.clone();

    client
        .send(&proto::ClientMessage::StartGame(proto::StartGame {
            envelope: envelope("rust-start-1"),
            room_code: Some(room_code.clone()),
        }))
        .await;
    let starting = client
        .wait_for(
            |message| matches!(message, proto::ServerMessage::GameStarting(_)),
            Duration::from_secs(3),
        )
        .await;
    let proto::ServerMessage::GameStarting(starting) = starting else {
        panic!("game_starting expected")
    };
    assert_eq!(starting.human_count, 1);
    assert_eq!(starting.ai_count, 2);

    // Drive the hand: pump every frame, act on our turn, ignore the interleaved
    // acks/snapshots until the settlement snapshot arrives.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut handled_turn = -1_i64;
    let settled = loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "hand did not settle on the reference server"
        );
        let message = client.next_message(Duration::from_secs(10)).await;
        match message {
            proto::ServerMessage::ActionAck(ack) => {
                assert!(ack.accepted, "action rejected by reference server: {ack:?}")
            }
            proto::ServerMessage::GameState(state) => {
                assert!(
                    state.players.iter().all(|player| player.id != "leak"),
                    "unexpected player id"
                );
                if state.phase == proto::Phase::Idle && state.last_hand_result.is_some() {
                    break state;
                }
                if state.is_your_turn && state.turn_id != handled_turn {
                    handled_turn = state.turn_id;
                    let legal = &state.legal_actions.actions;
                    let (action, amount) = if legal.contains(&proto::ActionType::Check) {
                        ("check".to_owned(), 0)
                    } else if legal.contains(&proto::ActionType::Call)
                        && state.your_stack.saturating_sub(state.legal_actions.to_call)
                            >= state.room_config.big_blind.saturating_mul(10)
                    {
                        // This protocol flow must also test ready/next-hand.
                        // The reference AI uses live randomness: calling a shove
                        // can bust the sole human, legitimately ending the game
                        // and making a next-hand assertion impossible. Keep a
                        // reserve; all-in settlement is covered by engine tests.
                        ("call".to_owned(), state.legal_actions.to_call)
                    } else {
                        ("fold".to_owned(), 0)
                    };
                    client
                        .send(&proto::ClientMessage::PlayerAction(proto::PlayerAction {
                            envelope: envelope(&format!(
                                "rust-act-{}-{}",
                                state.hand_id, state.turn_id
                            )),
                            room_code: Some(room_code.clone()),
                            hand_id: Some(state.hand_id),
                            turn_id: Some(state.turn_id),
                            expected_state_version: Some(state.state_version),
                            action: Some(action),
                            amount,
                        }))
                        .await;
                }
            }
            _ => {}
        }
    };
    let result = settled.last_hand_result.expect("settlement");
    assert!(result.pot > 0 || result.reason == proto::HandEndReason::Fold);
    eprintln!(
        "[interop] Rust client vs reference server: hand {} settled by {}",
        settled.hand_id,
        result.reason.as_str()
    );

    let hand_id = settled.hand_id;
    client
        .send(&proto::ClientMessage::ReadyForNext(proto::ReadyForNext {
            envelope: envelope("rust-ready-1"),
            room_code: Some(room_code.clone()),
            hand_id: Some(hand_id),
        }))
        .await;
    let next_hand = client
        .wait_for(
            |message| match message {
                proto::ServerMessage::GameState(state) => {
                    state.hand_id > hand_id && state.phase != proto::Phase::Idle
                }
                _ => false,
            },
            Duration::from_secs(10),
        )
        .await;
    let proto::ServerMessage::GameState(next_hand) = next_hand else {
        panic!("next hand expected")
    };
    assert_eq!(next_hand.hand_id, hand_id + 1);

    client
        .send(&proto::ClientMessage::LeaveRoom(proto::LeaveRoom {
            envelope: envelope("rust-leave-1"),
            room_code: Some(room_code),
        }))
        .await;
    let ack = client
        .wait_for(
            |message| matches!(message, proto::ServerMessage::Ack(_)),
            Duration::from_secs(3),
        )
        .await;
    let proto::ServerMessage::Ack(ack) = ack else {
        panic!("ack expected")
    };
    assert!(ack.accepted);
    eprintln!("[interop] Rust client -> reference JS server: all steps passed");
}
