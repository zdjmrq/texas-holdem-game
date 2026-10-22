//! Desktop service discovery, equivalent to main.js probePokerServer.
//!
//! The reference desktop application probes an already running local server
//! before deciding whether to reuse it (default port 3000) or to start its own
//! listener on a random port. The probe requires hello with
//! serverId == "texas-holdem-game", protocolVersion == 2 (numeric equality) and
//! an exact appVersion match, within 900 ms.

use std::time::Duration;

use poker_protocol::Hello;
use poker_protocol::constants::DESKTOP_PROBE_TIMEOUT_MS;

use crate::config::HelloPolicy;
use crate::error::ClientError;
use crate::transport;

/// Default probe timeout used by the desktop application (900 ms).
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(DESKTOP_PROBE_TIMEOUT_MS);

/// Probes a WebSocket endpoint with the reference desktop timeout and policy.
pub async fn probe_server(url: &str, expected_app_version: &str) -> Result<Hello, ClientError> {
    probe_server_with_timeout(url, PROBE_TIMEOUT, expected_app_version).await
}

/// Probes a WebSocket endpoint with an explicit timeout.
///
/// The timeout bounds the whole probe (connect + hello + close). A hello that
/// does not satisfy the desktop-probe policy is reported as
/// ClientError::Hello with the mismatching field.
pub async fn probe_server_with_timeout(
    url: &str,
    timeout: Duration,
    expected_app_version: &str,
) -> Result<Hello, ClientError> {
    let hello = tokio::time::timeout(timeout, async {
        let mut stream = transport::open_ws(url).await?;
        let hello = transport::read_hello(&mut stream).await?;
        transport::close_stream(stream).await;
        Ok::<Hello, ClientError>(hello)
    })
    .await
    .map_err(|_| transport::handshake_timeout_error(timeout))??;
    HelloPolicy::desktop_probe(expected_app_version)
        .validate(&hello)
        .map_err(ClientError::Hello)?;
    Ok(hello)
}
