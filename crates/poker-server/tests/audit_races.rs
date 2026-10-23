//! Adversarial regressions for lobby identity changes over real WebSockets.
mod common;

use common::*;
use poker_server::{ServerOptions, spawn_server};
use serde_json::json;

#[tokio::test]
async fn departed_lobby_seats_do_not_block_join_after_a_disconnect_timeout() {
    let handle = spawn_server(ServerOptions {
        port: 0,
        disconnect_grace_ms: 1500,
        ..Default::default()
    })
    .await
    .unwrap();
    let url = handle.url();
    let mut host = TestClient::connect(&url).await;
    host.send(with_room_options(
        json!({"type":"join_room", "roomCode":"1", "name":"Host"}),
        room_options(3, false, 20000, 0),
    ))
    .await;
    host.wait_type("room_created").await;
    let mut guest = TestClient::connect(&url).await;
    guest
        .send(json!({"type":"join_room", "roomCode":"1", "name":"Guest"}))
        .await;
    let guest_info = guest.wait_type("room_joined").await;
    let mut observer = TestClient::connect(&url).await;
    observer
        .send(json!({"type":"join_room", "roomCode":"1", "name":"Observer"}))
        .await;
    observer.wait_type("room_joined").await;

    let mark = observer.mark();
    guest.close().await;
    observer
        .wait_after(
            mark,
            |msg| {
                msg["type"] == "player_left"
                    && msg["players"].as_array().is_some_and(|players| {
                        players
                            .iter()
                            .any(|p| p["id"] == guest_info["playerId"] && p["connected"] == false)
                    })
            },
            WAIT,
        )
        .await;
    // This removes index zero while the guest's timer still holds index one.
    host.send(json!({"type":"leave_room", "roomCode":"1", "requestId":"leave"}))
        .await;
    let mark = observer.mark();
    observer
        .wait_after(
            mark,
            |msg| {
                msg["type"] == "player_left"
                    && msg["players"]
                        .as_array()
                        .is_some_and(|players| players.len() == 1)
            },
            WAIT,
        )
        .await;
    let mut replacement = TestClient::connect(&url).await;
    replacement
        .send(json!({"type":"join_room", "roomCode":"1", "name":"Replacement"}))
        .await;
    let joined = replacement.wait_type("room_joined").await;
    assert_eq!(joined["seatId"], 0);
    assert_eq!(joined["players"].as_array().unwrap().len(), 2);
    handle.shutdown().await;
}
