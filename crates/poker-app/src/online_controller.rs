//! Online connection abstraction for the app shell.
//!
//! The real controller (WebSocket client, reconnect/resume, room state) lands
//! with task-14 in src/online/**. app.rs only depends on this trait so the
//! online screens can be plugged in without touching local-game code. Until
//! then NoopOnlineController keeps the ONLINE placeholder honest: it reports
//! that the online client is not available yet.

use std::collections::VecDeque;
use std::time::Instant;

/// Connection lifecycle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    Failed {
        message: String,
    },
}

impl ConnectionState {
    pub fn label(&self) -> String {
        match self {
            Self::Disconnected => "未连接".to_owned(),
            Self::Connecting => "连接中…".to_owned(),
            Self::Connected => "已连接".to_owned(),
            Self::Reconnecting { attempt } => format!("重连中…（第 {attempt} 次）"),
            Self::Failed { message } => format!("连接失败：{message}"),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Room creation/join parameters (mirrors the local table settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomConfig {
    pub players: usize,
    pub starting_stack: u64,
    pub small_blind: u64,
    pub big_blind: u64,
    pub short_deck: bool,
}

/// Public room snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomInfo {
    pub code: String,
    pub host_name: String,
    pub players: Vec<String>,
    pub ready: Vec<bool>,
}

/// Asynchronous events the controller delivers to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineEvent {
    Connected { server_id: String },
    Disconnected { reason: String },
    RoomUpdated(Box<RoomInfo>),
}

/// UI requests sent to the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnlineRequest {
    Connect { url: String },
    Disconnect,
    CreateRoom { config: RoomConfig },
    JoinRoom { code: String },
    LeaveRoom,
    SetReady { ready: bool },
    StartGame,
    PlayerAction { action: String, amount: Option<u64> },
}

/// Controller interface consumed by app.rs (implemented by src/online/** later).
pub trait OnlineController: Send {
    fn name(&self) -> &'static str;
    /// Whether a real online client is wired (Noop returns false).
    fn is_available(&self) -> bool;
    fn state(&self) -> &ConnectionState;
    fn room(&self) -> Option<&RoomInfo>;
    /// Drive time-based work (reconnect backoff, timeouts). Called per frame.
    fn update(&mut self, now: Instant);
    fn handle(&mut self, request: OnlineRequest);
    fn poll_event(&mut self) -> Option<OnlineEvent>;
    /// One-line status shown in the online placeholder / status bar.
    fn status_text(&self) -> String;

    /// Update the server URL once the embedded local server has bound a port
    /// (the app spawns it asynchronously at startup). Default: no-op.
    fn set_server_url(&mut self, _url: String) {}

    /// Currently configured server URL, for display in the connection screen.
    fn server_url(&self) -> Option<String> {
        None
    }

    /// Stop background work on app exit. Default: no-op.
    fn shutdown(&mut self) {}
}

/// Placeholder controller: reports "not available" and rejects connect.
#[derive(Debug, Default)]
pub struct NoopOnlineController {
    state: ConnectionState,
    room: Option<RoomInfo>,
    events: VecDeque<OnlineEvent>,
    server_url: Option<String>,
}

impl NoopOnlineController {
    pub const MESSAGE: &'static str = "联机客户端尚未接入（poker-client 就绪后启用）";

    #[must_use]
    pub fn new() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            room: None,
            events: VecDeque::new(),
            server_url: None,
        }
    }
}

impl OnlineController for NoopOnlineController {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn is_available(&self) -> bool {
        false
    }

    fn state(&self) -> &ConnectionState {
        &self.state
    }

    fn room(&self) -> Option<&RoomInfo> {
        None
    }

    fn update(&mut self, _now: Instant) {}

    fn handle(&mut self, request: OnlineRequest) {
        match request {
            OnlineRequest::Connect { .. } => {
                self.state = ConnectionState::Failed {
                    message: Self::MESSAGE.to_owned(),
                };
                self.events.push_back(OnlineEvent::Disconnected {
                    reason: Self::MESSAGE.to_owned(),
                });
            }
            OnlineRequest::Disconnect => {
                self.state = ConnectionState::Disconnected;
                self.room = None;
            }
            _ => {
                self.events.push_back(OnlineEvent::Disconnected {
                    reason: "尚未接入联机客户端".to_owned(),
                });
            }
        }
    }

    fn poll_event(&mut self) -> Option<OnlineEvent> {
        self.events.pop_front()
    }

    fn status_text(&self) -> String {
        self.state.label()
    }

    fn set_server_url(&mut self, url: String) {
        self.server_url = Some(url);
    }

    fn server_url(&self) -> Option<String> {
        self.server_url.clone()
    }

    fn shutdown(&mut self) {
        self.state = ConnectionState::Disconnected;
        self.room = None;
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_controller_starts_disconnected_and_unavailable() {
        let controller = NoopOnlineController::new();
        assert!(!controller.is_available());
        assert_eq!(controller.state(), &ConnectionState::Disconnected);
        assert!(controller.room().is_none());
        assert_eq!(controller.status_text(), "未连接");
    }

    #[test]
    fn noop_rejects_connect_with_an_explicit_failure() {
        let mut controller = NoopOnlineController::new();
        controller.handle(OnlineRequest::Connect {
            url: "ws://127.0.0.1:3000".to_owned(),
        });
        assert!(matches!(controller.state(), ConnectionState::Failed { .. }));
        assert_eq!(
            controller.poll_event(),
            Some(OnlineEvent::Disconnected {
                reason: NoopOnlineController::MESSAGE.to_owned()
            })
        );
        assert!(controller.poll_event().is_none());
    }

    #[test]
    fn noop_disconnect_resets_state() {
        let mut controller = NoopOnlineController::new();
        controller.handle(OnlineRequest::Connect {
            url: "ws://127.0.0.1:3000".to_owned(),
        });
        controller.handle(OnlineRequest::Disconnect);
        assert_eq!(controller.state(), &ConnectionState::Disconnected);
        assert!(controller.room().is_none());
    }

    #[test]
    fn controller_is_usable_as_a_trait_object() {
        let mut controller: Box<dyn OnlineController> = Box::new(NoopOnlineController::new());
        assert_eq!(controller.name(), "noop");
        controller.update(Instant::now());
        controller.handle(OnlineRequest::JoinRoom {
            code: "ABC123".to_owned(),
        });
        assert!(controller.poll_event().is_some());
    }
}
