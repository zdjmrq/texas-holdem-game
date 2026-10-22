//! Embedded local poker server for the desktop app (Lead decision A).
//!
//! The app spawns the authoritative room server in a background thread with a
//! current-thread tokio runtime, so "double click and play online on this PC"
//! works without a separate process. The bound URL is published to the UI (the
//! connection screen shows it); shutdown happens on app exit.
//!
//! Port selection mirrors the reference desktop behaviour: prefer POKER_PORT
//! when set, otherwise ask the server for an ephemeral port (fallback to a free
//! one when the preferred port is taken).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use poker_server::{ServerHandle, ServerOptions, spawn_server_with_fallback};

/// Published information about the running embedded server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// ws:// URL the client should connect to.
    pub url: String,
    /// appVersion reported in the hello handshake.
    pub app_version: String,
}

/// Handle to the background server thread.
pub struct LocalServerHandle {
    info_rx: Receiver<Result<ServerInfo, String>>,
    shutdown_tx: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    shutdown_requested: bool,
}

impl LocalServerHandle {
    /// Start the embedded server on its own thread. Non-blocking.
    #[must_use]
    pub fn start() -> Self {
        let (info_tx, info_rx) = channel::<Result<ServerInfo, String>>();
        let (shutdown_tx, shutdown_rx) = channel::<()>();
        let thread = thread::Builder::new()
            .name("poker-local-server".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = info_tx.send(Err(format!("创建服务端 runtime 失败：{error}")));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let mut options = ServerOptions::from_env();
                    // Embedding default: ephemeral port unless POKER_PORT is set.
                    if std::env::var("POKER_PORT").is_err() {
                        options.port = 0;
                    }
                    match spawn_server_with_fallback(options).await {
                        Ok(handle) => {
                            publish(&info_tx, &handle);
                            // Block until the app asks us to stop.
                            let _ = shutdown_rx.recv();
                            handle.shutdown().await;
                            log::info!("embedded poker server stopped");
                        }
                        Err(error) => {
                            let _ = info_tx.send(Err(format!("本地联机服务启动失败：{error}")));
                        }
                    }
                });
            })
            .ok();
        Self {
            info_rx,
            shutdown_tx: Some(shutdown_tx),
            thread,
            shutdown_requested: false,
        }
    }

    /// Poll the server URL once it is bound (or the startup error).
    pub fn poll(&self) -> Option<Result<ServerInfo, String>> {
        self.info_rx.try_recv().ok()
    }

    /// Signal the server to stop and wait for the thread (bounded by the
    /// server's own 2 s graceful-shutdown timeout).
    pub fn shutdown(&mut self) {
        self.shutdown_requested = true;
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.shutdown_requested && self.thread.is_some()
    }
}

fn publish(info_tx: &Sender<Result<ServerInfo, String>>, handle: &ServerHandle) {
    let info = ServerInfo {
        url: handle.url(),
        app_version: handle.app_version().to_owned(),
    };
    log::info!(
        "embedded poker server listening on {} (appVersion {})",
        info.url,
        info.app_version
    );
    let _ = info_tx.send(Ok(info));
}

impl Drop for LocalServerHandle {
    fn drop(&mut self) {
        if !self.shutdown_requested {
            self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_server_starts_on_an_ephemeral_port_and_stops() {
        let mut server = LocalServerHandle::start();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let info = loop {
            if let Some(result) = server.poll() {
                break result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "embedded server did not publish its URL in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let info = info.expect("embedded server should start");
        assert!(
            info.url.starts_with("ws://127.0.0.1:"),
            "unexpected url: {}",
            info.url
        );
        assert!(!info.app_version.is_empty());
        server.shutdown();
        assert!(!server.is_running());
    }
}
