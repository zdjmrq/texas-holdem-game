//! Server options, environment-variable loading and range clamping.
//!
//! Defaults and ranges are pinned by docs/specs/protocol-v2.md and by the
//! reference implementation (server/local-server.js constructor). The
//! POKER_* environment variables are read by from_env(); embedding callers
//! (the desktop app) build options explicitly.

use std::sync::Arc;
use std::time::Duration;

use poker_protocol as proto;
use serde_json::Value;

use crate::ai::{AiFactory, StubAiFactory};

/// Options of the authoritative room server.
///
/// All numeric options are clamped to the reference ranges by
/// ServerOptions::sanitized(); spawn_server calls it automatically.
#[derive(Clone)]
pub struct ServerOptions {
    /// Bind address, default 127.0.0.1.
    pub host: String,
    /// Bind port, default 3000; 0 picks a free port (used by embedding and tests).
    pub port: u16,
    /// Fallback AI delay when a room sends an invalid aiDelay, 0..=10000, default 450.
    pub ai_delay_ms: u32,
    /// Reconnect grace window, 100..=60000 ms, default 1800.
    pub disconnect_grace_ms: u32,
    /// Maximum number of rooms, 1..=5, default 3.
    pub max_rooms: u32,
    /// Maximum concurrent connections, 4..=100, default 24.
    pub max_connections: u32,
    /// Maximum concurrent connections per IP, 2..=30, default 20.
    pub max_connections_per_ip: u32,
    /// Rate-limit window, 1000..=60000 ms, default 10000.
    pub rate_window_ms: u32,
    /// Messages allowed per window per connection, 20..=500, default 80.
    pub max_messages_per_window: u32,
    /// Trust the X-Forwarded-For header (POKER_TRUST_PROXY=1).
    pub trust_proxy: bool,
    /// Exact-match Origin allowlist.
    pub allowed_origins: Vec<String>,
    /// appVersion reported in hello. The original v1.5.8 desktop probe
    /// requires an exact match, so the default is the compatibility version.
    pub app_version: String,
    /// Seed for the table RNG (dealer choice and shuffles). None uses a
    /// time/process derived seed; tests and interop runs set a fixed seed.
    pub rng_seed: Option<u64>,
    /// Empty-room TTL for rooms that have dealt a hand. Defaults to the
    /// reference constant 15000 ms; tests override it to keep runs fast.
    pub empty_room_ttl_ms: u32,
    /// AI seat factory. Defaults to the deterministic check/call stub.
    pub ai_factory: Arc<dyn AiFactory>,
}

impl std::fmt::Debug for ServerOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerOptions")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("ai_delay_ms", &self.ai_delay_ms)
            .field("disconnect_grace_ms", &self.disconnect_grace_ms)
            .field("max_rooms", &self.max_rooms)
            .field("max_connections", &self.max_connections)
            .field("max_connections_per_ip", &self.max_connections_per_ip)
            .field("rate_window_ms", &self.rate_window_ms)
            .field("max_messages_per_window", &self.max_messages_per_window)
            .field("trust_proxy", &self.trust_proxy)
            .field("allowed_origins", &self.allowed_origins)
            .field("app_version", &self.app_version)
            .field("rng_seed", &self.rng_seed)
            .field("empty_room_ttl_ms", &self.empty_room_ttl_ms)
            .finish_non_exhaustive()
    }
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            host: proto::DEFAULT_HOST.to_owned(),
            port: proto::DEFAULT_PORT,
            ai_delay_ms: proto::SERVER_AI_DELAY_DEFAULT_MS,
            disconnect_grace_ms: proto::DISCONNECT_GRACE_DEFAULT_MS,
            max_rooms: proto::MAX_ROOMS_DEFAULT,
            max_connections: proto::MAX_CONNECTIONS_DEFAULT,
            max_connections_per_ip: proto::MAX_CONNECTIONS_PER_IP_DEFAULT,
            rate_window_ms: proto::RATE_WINDOW_DEFAULT_MS,
            max_messages_per_window: proto::MAX_MESSAGES_PER_WINDOW_DEFAULT,
            trust_proxy: false,
            allowed_origins: Vec::new(),
            app_version: proto::COMPAT_APP_VERSION.to_owned(),
            rng_seed: None,
            empty_room_ttl_ms: u32::try_from(proto::EMPTY_ROOM_TTL_MS).unwrap_or(15_000),
            ai_factory: Arc::new(StubAiFactory),
        }
    }
}

fn env_value(name: &str) -> Option<Value> {
    std::env::var(name).ok().map(Value::String)
}

fn bounded(value: Option<&Value>, fallback: f64, minimum: f64, maximum: f64) -> f64 {
    proto::bounded_number(value, fallback, minimum, maximum, true)
}

fn bounded_u32(value: Option<&Value>, fallback: f64, minimum: f64, maximum: f64) -> u32 {
    bounded(value, fallback, minimum, maximum) as u32
}

impl ServerOptions {
    /// Loads the POKER_* environment variables on top of the defaults.
    pub fn from_env() -> Self {
        let mut options = Self::default();
        if let Ok(host) = std::env::var(proto::ENV_POKER_HOST) {
            if !host.is_empty() {
                options.host = host;
            }
        }
        if let Ok(port) = std::env::var(proto::ENV_POKER_PORT) {
            let parsed =
                proto::bounded_number(Some(&Value::String(port)), 3000.0, 0.0, 65_535.0, true);
            options.port = parsed as u16;
        }
        options.max_rooms = bounded_u32(
            env_value(proto::ENV_POKER_MAX_ROOMS).as_ref(),
            f64::from(proto::MAX_ROOMS_DEFAULT),
            f64::from(proto::MAX_ROOMS_MIN),
            f64::from(proto::MAX_ROOMS_MAX),
        );
        options.max_connections = bounded_u32(
            env_value(proto::ENV_POKER_MAX_CONNECTIONS).as_ref(),
            f64::from(proto::MAX_CONNECTIONS_DEFAULT),
            f64::from(proto::MAX_CONNECTIONS_MIN),
            f64::from(proto::MAX_CONNECTIONS_MAX),
        );
        options.max_connections_per_ip = bounded_u32(
            env_value(proto::ENV_POKER_MAX_CONNECTIONS_PER_IP).as_ref(),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_DEFAULT),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_MIN),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_MAX),
        );
        if let Ok(origins) = std::env::var(proto::ENV_POKER_ALLOWED_ORIGINS) {
            options.allowed_origins = origins
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect();
        }
        if let Ok(trust) = std::env::var(proto::ENV_POKER_TRUST_PROXY) {
            options.trust_proxy = trust == "1";
        }
        if let Ok(version) = std::env::var("POKER_APP_VERSION") {
            if !version.is_empty() {
                options.app_version = version;
            }
        }
        options
    }

    /// Clamps every numeric option to the reference ranges.
    pub fn sanitized(mut self) -> Self {
        self.ai_delay_ms = bounded_u32(
            Some(&Value::from(self.ai_delay_ms)),
            f64::from(proto::SERVER_AI_DELAY_DEFAULT_MS),
            f64::from(proto::SERVER_AI_DELAY_MIN_MS),
            f64::from(proto::SERVER_AI_DELAY_MAX_MS),
        );
        self.disconnect_grace_ms = bounded_u32(
            Some(&Value::from(self.disconnect_grace_ms)),
            f64::from(proto::DISCONNECT_GRACE_DEFAULT_MS),
            f64::from(proto::DISCONNECT_GRACE_MIN_MS),
            f64::from(proto::DISCONNECT_GRACE_MAX_MS),
        );
        self.max_rooms = bounded_u32(
            Some(&Value::from(self.max_rooms)),
            f64::from(proto::MAX_ROOMS_DEFAULT),
            f64::from(proto::MAX_ROOMS_MIN),
            f64::from(proto::MAX_ROOMS_MAX),
        );
        self.max_connections = bounded_u32(
            Some(&Value::from(self.max_connections)),
            f64::from(proto::MAX_CONNECTIONS_DEFAULT),
            f64::from(proto::MAX_CONNECTIONS_MIN),
            f64::from(proto::MAX_CONNECTIONS_MAX),
        );
        self.max_connections_per_ip = bounded_u32(
            Some(&Value::from(self.max_connections_per_ip)),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_DEFAULT),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_MIN),
            f64::from(proto::MAX_CONNECTIONS_PER_IP_MAX),
        );
        self.rate_window_ms = bounded_u32(
            Some(&Value::from(self.rate_window_ms)),
            f64::from(proto::RATE_WINDOW_DEFAULT_MS),
            f64::from(proto::RATE_WINDOW_MIN_MS),
            f64::from(proto::RATE_WINDOW_MAX_MS),
        );
        self.max_messages_per_window = bounded_u32(
            Some(&Value::from(self.max_messages_per_window)),
            f64::from(proto::MAX_MESSAGES_PER_WINDOW_DEFAULT),
            f64::from(proto::MAX_MESSAGES_PER_WINDOW_MIN),
            f64::from(proto::MAX_MESSAGES_PER_WINDOW_MAX),
        );
        self.empty_room_ttl_ms = self.empty_room_ttl_ms.max(50);
        self.allowed_origins = self
            .allowed_origins
            .iter()
            .map(|origin| origin.trim().to_owned())
            .filter(|origin| !origin.is_empty())
            .collect();
        self
    }

    /// Window duration of the per-connection rate limiter.
    pub(crate) fn rate_window(&self) -> Duration {
        Duration::from_millis(u64::from(self.rate_window_ms))
    }

    /// Reconnect grace window.
    pub(crate) fn disconnect_grace(&self) -> Duration {
        Duration::from_millis(u64::from(self.disconnect_grace_ms))
    }

    /// Empty-room TTL.
    pub(crate) fn empty_room_ttl(&self) -> Duration {
        Duration::from_millis(u64::from(self.empty_room_ttl_ms))
    }
}
