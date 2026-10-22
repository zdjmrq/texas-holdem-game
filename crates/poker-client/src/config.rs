//! Client configuration, hello handshake policy, session credentials and
//! deterministic hooks.
//!
//! The defaults reproduce js/network.js (reference commit e0c6c6f, v1.5.8):
//! the reference browser client ignores the contents of the server hello
//! frame, reconnects only while it still holds a resume token and uses a
//! min(10s, 650ms * 2^min(attempt, 4)) + jitter(0..350ms) backoff.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use poker_protocol::Hello;
use poker_protocol::constants::{MAX_PAYLOAD_BYTES, PROTOCOL_VERSION, SERVER_ID};

use crate::error::ClientError;

/// Default timeout for the TCP/WebSocket handshake plus the hello frame.
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default timeout for a single outbound frame write (None disables it).
pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// Credentials needed to re-attach a dropped session.
///
/// This is the Rust equivalent of the browser client sessionStorage record
/// under texas-holdem-network-session-v2 (js/network.js lines 6-68). The token
/// is issued by the server inside room_created / room_joined; this crate only
/// stores and returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCredentials {
    /// WebSocket URL the session was created against.
    pub server_url: String,
    /// Room code the client was attached to.
    pub room_code: String,
    /// Resume token issued by the server.
    pub resume_token: String,
    /// Player id, when known (diagnostics only; the token is authoritative).
    pub player_id: Option<String>,
}

impl SessionCredentials {
    /// Builds credentials from the three values the server hands out.
    pub fn new(
        server_url: impl Into<String>,
        room_code: impl Into<String>,
        resume_token: impl Into<String>,
    ) -> Self {
        Self {
            server_url: server_url.into(),
            room_code: room_code.into(),
            resume_token: resume_token.into(),
            player_id: None,
        }
    }

    /// Attaches a player id (diagnostics only).
    #[must_use]
    pub fn with_player_id(mut self, player_id: impl Into<String>) -> Self {
        self.player_id = Some(player_id.into());
        self
    }
}

/// What the client verifies in the server hello frame.
///
/// The reference browser client ignores the frame entirely; the desktop
/// service probe (main.js probePokerServer) requires serverId,
/// protocolVersion and an exact appVersion match. Both behaviours are
/// available: HelloPolicy::js_client (default) and HelloPolicy::desktop_probe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelloPolicy {
    /// When set, require this exact protocolVersion value (2).
    pub protocol_version: Option<u8>,
    /// When set, require this exact serverId value.
    pub server_id: Option<String>,
    /// When set, require this exact appVersion value (desktop probe).
    pub app_version: Option<String>,
}

impl HelloPolicy {
    /// No checks: matches the reference browser client behaviour.
    pub fn js_client() -> Self {
        Self::default()
    }

    /// Require protocolVersion == 2 and serverId == "texas-holdem-game",
    /// but accept any appVersion (compatibility window).
    pub fn compatible_server() -> Self {
        Self {
            protocol_version: Some(PROTOCOL_VERSION),
            server_id: Some(SERVER_ID.to_owned()),
            app_version: None,
        }
    }

    /// Exact desktop service-probe semantics of main.js probePokerServer:
    /// server id, protocol version and an exact application version match.
    pub fn desktop_probe(app_version: impl Into<String>) -> Self {
        Self {
            protocol_version: Some(PROTOCOL_VERSION),
            server_id: Some(SERVER_ID.to_owned()),
            app_version: Some(app_version.into()),
        }
    }

    /// Validates a received hello frame against this policy.
    pub fn validate(&self, hello: &Hello) -> Result<(), HelloMismatch> {
        if let Some(expected) = self.protocol_version
            && hello.protocol_version != expected
        {
            return Err(HelloMismatch {
                field: "protocolVersion",
                expected: expected.to_string(),
                actual: hello.protocol_version.to_string(),
            });
        }
        if let Some(expected) = &self.server_id
            && &hello.server_id != expected
        {
            return Err(HelloMismatch {
                field: "serverId",
                expected: expected.clone(),
                actual: hello.server_id.clone(),
            });
        }
        if let Some(expected) = &self.app_version
            && &hello.app_version != expected
        {
            return Err(HelloMismatch {
                field: "appVersion",
                expected: expected.clone(),
                actual: hello.app_version.clone(),
            });
        }
        Ok(())
    }
}

/// A hello field that did not match the configured HelloPolicy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloMismatch {
    /// Field name on the wire.
    pub field: &'static str,
    /// Value required by the policy.
    pub expected: String,
    /// Value actually reported by the server.
    pub actual: String,
}

impl fmt::Display for HelloMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "hello {} mismatch: expected {}, got {}",
            self.field, self.expected, self.actual
        )
    }
}

impl std::error::Error for HelloMismatch {}

/// Deterministic hooks used for request ids and reconnect jitter.
///
/// Production clients use SystemHooks; tests use FixedHooks so the behaviour
/// is reproducible. FixedHooks with jitter = 0.5 reproduces the golden
/// reconnect delays in tests/golden/protocol/client-behaviour.json.
pub trait ClientHooks: Send + Sync + fmt::Debug {
    /// Current wall clock in milliseconds (the reference client uses
    /// Date.now() when it builds request ids).
    fn now_millis(&self) -> u64;

    /// Jitter fraction in 0.0..1.0 used for reconnect delays.
    fn jitter_fraction(&self) -> f64;
}

/// Wall-clock hooks used by production clients.
#[derive(Debug, Default)]
pub struct SystemHooks {
    counter: AtomicU64,
}

impl SystemHooks {
    /// Creates wall-clock hooks.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClientHooks for SystemHooks {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn jitter_fraction(&self) -> f64 {
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos() as u64);
        let mixed = split_mix64(nanos ^ counter.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        mixed as f64 / (u64::MAX as f64)
    }
}

/// Deterministic hooks for tests and reproducible tooling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedHooks {
    /// Value returned by ClientHooks::now_millis.
    pub now_millis: u64,
    /// Value returned by ClientHooks::jitter_fraction (clamped to 0..1).
    pub jitter: f64,
}

impl FixedHooks {
    /// Creates fixed hooks; jitter is clamped to 0.0..=1.0.
    pub fn new(now_millis: u64, jitter: f64) -> Self {
        Self {
            now_millis,
            jitter: jitter.clamp(0.0, 1.0),
        }
    }
}

impl ClientHooks for FixedHooks {
    fn now_millis(&self) -> u64 {
        self.now_millis
    }

    fn jitter_fraction(&self) -> f64 {
        self.jitter
    }
}

/// Configuration of a crate::Client.
///
/// Build it with ClientConfig::new and the with_* builders; every field
/// defaults to the reference browser client behaviour except that a
/// handshake timeout is enforced (the browser has none).
#[derive(Clone)]
pub struct ClientConfig {
    /// WebSocket URL (ws://host:port).
    pub url: String,
    /// Checks applied to the server hello frame.
    pub hello: HelloPolicy,
    /// Timeout for the handshake and the hello frame.
    ///
    /// None disables the timeout, matching the browser client (no timeout
    /// exists there). The default is DEFAULT_HANDSHAKE_TIMEOUT.
    pub handshake_timeout: Option<Duration>,
    /// Automatic reconnect after an abnormal close, when a session is stored.
    pub auto_reconnect: bool,
    /// Maximum number of retry delays before ReconnectFailed.
    ///
    /// None means unlimited, matching the browser client.
    pub max_reconnect_attempts: Option<u32>,
    /// Deterministic/random hooks for request ids and reconnect jitter.
    pub hooks: Arc<dyn ClientHooks>,
    /// Maximum outbound frame size; defaults to the server 16 KiB limit.
    pub max_frame_bytes: usize,
    /// Timeout for a single frame write (None disables it).
    pub send_timeout: Option<Duration>,
}

impl ClientConfig {
    /// Creates a configuration for url with reference-client defaults.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            hello: HelloPolicy::default(),
            handshake_timeout: Some(DEFAULT_HANDSHAKE_TIMEOUT),
            auto_reconnect: true,
            max_reconnect_attempts: None,
            hooks: Arc::new(SystemHooks::new()),
            max_frame_bytes: MAX_PAYLOAD_BYTES,
            send_timeout: Some(DEFAULT_SEND_TIMEOUT),
        }
    }

    /// Applies a different hello policy.
    #[must_use]
    pub fn with_hello_policy(mut self, hello: HelloPolicy) -> Self {
        self.hello = hello;
        self
    }

    /// Applies a handshake/hello timeout.
    #[must_use]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = Some(timeout);
        self
    }

    /// Disables the handshake/hello timeout (browser-client behaviour).
    #[must_use]
    pub fn without_handshake_timeout(mut self) -> Self {
        self.handshake_timeout = None;
        self
    }

    /// Enables or disables automatic reconnect.
    #[must_use]
    pub fn with_auto_reconnect(mut self, auto_reconnect: bool) -> Self {
        self.auto_reconnect = auto_reconnect;
        self
    }

    /// Caps the number of reconnect delays (None = unlimited).
    #[must_use]
    pub fn with_max_reconnect_attempts(mut self, attempts: Option<u32>) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }

    /// Applies deterministic hooks.
    #[must_use]
    pub fn with_hooks(mut self, hooks: Arc<dyn ClientHooks>) -> Self {
        self.hooks = hooks;
        self
    }

    /// Overrides the outbound frame size limit.
    #[must_use]
    pub fn with_max_frame_bytes(mut self, max_frame_bytes: usize) -> Self {
        self.max_frame_bytes = max_frame_bytes;
        self
    }

    /// Applies a per-frame write timeout.
    #[must_use]
    pub fn with_send_timeout(mut self, send_timeout: Option<Duration>) -> Self {
        self.send_timeout = send_timeout;
        self
    }

    pub(crate) fn jitter_fraction(&self) -> f64 {
        self.hooks.jitter_fraction().clamp(0.0, 1.0)
    }

    pub(crate) fn now_millis(&self) -> u64 {
        self.hooks.now_millis()
    }
}

impl fmt::Debug for ClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handshake_timeout = self
            .handshake_timeout
            .map_or_else(|| "none".to_owned(), |timeout| format!("{timeout:?}"));
        formatter
            .debug_struct("ClientConfig")
            .field("url", &self.url)
            .field("hello", &self.hello)
            .field("handshake_timeout", &handshake_timeout)
            .field("auto_reconnect", &self.auto_reconnect)
            .field("max_reconnect_attempts", &self.max_reconnect_attempts)
            .field("hooks", &self.hooks)
            .field("max_frame_bytes", &self.max_frame_bytes)
            .field("send_timeout", &self.send_timeout)
            .finish()
    }
}

/// Validates the WebSocket URL scheme.
///
/// The reference client accepts ws:// and wss:// (case-insensitive). This
/// build has no TLS backend compiled in, so wss:// is rejected explicitly
/// instead of failing deep inside the handshake.
pub(crate) fn validate_ws_url(url: &str) -> Result<(), ClientError> {
    let trimmed = url.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("ws://") {
        return Ok(());
    }
    if lowered.starts_with("wss://") {
        return Err(ClientError::UnsupportedScheme(trimmed.to_owned()));
    }
    Err(ClientError::InvalidUrl(trimmed.to_owned()))
}

fn split_mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut mixed = value;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^ (mixed >> 31)
}
