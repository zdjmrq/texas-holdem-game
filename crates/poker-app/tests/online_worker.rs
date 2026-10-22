use poker_app::online::{commands::OnlineCommand, view::OnlineView, worker::Worker};
use poker_client::ClientEvent;
use poker_protocol::ServerMessage;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

async fn until(worker: &Worker, view: &mut OnlineView, predicate: impl Fn(&OnlineView) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate(view) {
        while let Ok(event) = worker.rx.try_recv() {
            view.reduce(event, Instant::now());
        }
        assert!(Instant::now() < deadline, "timed out: {:?}", view);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
#[tokio::test]
async fn native_worker_connects_joins_starts_acts_and_disconnects() {
    let server = poker_server::spawn_server(poker_server::ServerOptions {
        port: 0,
        ai_factory: Arc::new(poker_server::StubAiFactory),
        ..Default::default()
    })
    .await
    .unwrap();
    let worker = Worker::new(eframe::egui::Context::default());
    let mut view = OnlineView::new(Some(server.url()));
    worker.send(OnlineCommand::Connect {
        url: server.url(),
        session: None,
    });
    until(&worker, &mut view, |v| v.status.is_connected()).await;
    worker.send(OnlineCommand::CreateRoom {
        name: "玩家".into(),
        options: serde_json::from_value(serde_json::json!({"maxPlayers":2,"aiDelay":800})).unwrap(),
    });
    until(&worker, &mut view, |v| v.room.is_some()).await;
    worker.send(OnlineCommand::StartGame {
        room_code: view.room_code().unwrap().into(),
    });
    until(&worker, &mut view, |v| v.game.is_some()).await;
    assert!(view.game.as_ref().unwrap().hand_id > 0);
    until(&worker, &mut view, |v| v.can_act()).await;
    let version = view.last_state_version;
    let action = if view.to_call() > 0 {
        poker_app::online::commands::OnlineAction::Call
    } else {
        poker_app::online::commands::OnlineAction::Check
    };
    worker.send(OnlineCommand::Action(view.action_request(action).unwrap()));
    until(&worker, &mut view, |v| v.last_state_version > version).await;
    worker.send(OnlineCommand::Disconnect);
    until(&worker, &mut view, |v| !v.status.is_connected()).await;
    worker.send(OnlineCommand::Shutdown);
    server.shutdown().await;
}
#[tokio::test]
async fn embedded_server_runtime_keeps_serving_after_url_is_published() {
    let mut server = poker_app::local_server::LocalServerHandle::start();
    let info = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(info) = server.poll() {
                break info.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut client = poker_client::Client::connect(poker_client::ClientConfig::new(info.url))
        .await
        .unwrap();
    client
        .create_room("玩家", Default::default())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(
                client.next_event().await,
                Some(ClientEvent::Message(ServerMessage::RoomCreated(_)))
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    client.disconnect().await;
    server.shutdown();
}
