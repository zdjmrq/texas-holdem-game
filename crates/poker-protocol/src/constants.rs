//! Protocol constants pinned by the reference JavaScript implementation.
//!
//! Values are captured by oracle/golden-protocol.js into
//! tests/golden/protocol/server-constants.json and client-behaviour.json; the
//! golden test asserts they stay in sync.

/// Wire protocol version carried by every message.
pub const PROTOCOL_VERSION: u8 = 2;

/// Value of the hello message "serverId" field.
pub const SERVER_ID: &str = "texas-holdem-game";

/// Value of the hello message "serverName" field.
pub const SERVER_NAME: &str = "德州扑克服务";

/// Application version reported by the reference server. The old desktop
/// client's service probe requires an exact match, so a drop-in Rust server
/// must report the compatibility version until the client is updated.
pub const COMPAT_APP_VERSION: &str = "1.5.8";

/// Default listen port of the desktop-embedded local server.
pub const DEFAULT_PORT: u16 = 3000;

/// Default bind address of the desktop-embedded local server.
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Empty room TTL after the last human disconnects (rooms that never dealt a
/// hand are removed immediately).
pub const EMPTY_ROOM_TTL_MS: u64 = 15_000;

/// Maximum normalised player-name length in Unicode scalar values.
pub const MAX_NAME_LENGTH: usize = 8;

/// Maximum raw player-name length in UTF-16 code units before NFKC.
pub const MAX_RAW_NAME_LENGTH: usize = 256;

/// Maximum WebSocket payload accepted by the reference server.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

/// WebSocket ping interval.
pub const HEARTBEAT_INTERVAL_MS: u64 = 15_000;

/// Delay between "all players ready" and the next deal.
pub const NEXT_HAND_DELAY_MS: u64 = 250;

/// Server-side cap on the AI decision queue delay default (option fallback).
pub const SERVER_AI_DELAY_DEFAULT_MS: u32 = 450;

/// Lower bound of the server AI delay option.
pub const SERVER_AI_DELAY_MIN_MS: u32 = 0;

/// Upper bound of the server AI delay option.
pub const SERVER_AI_DELAY_MAX_MS: u32 = 10_000;

/// Default reconnect grace window before a disconnected player is folded.
pub const DISCONNECT_GRACE_DEFAULT_MS: u32 = 1_800;

/// Lower bound of the reconnect grace window.
pub const DISCONNECT_GRACE_MIN_MS: u32 = 100;

/// Upper bound of the reconnect grace window.
pub const DISCONNECT_GRACE_MAX_MS: u32 = 60_000;

/// Room codes the server hands out.
pub const ROOM_CODES: [&str; 5] = ["1", "2", "3", "4", "5"];

/// Starting-stack whitelist.
pub const STACK_LEVELS: [i64; 5] = [5_000, 10_000, 20_000, 50_000, 100_000];

/// Blind-level whitelist as (small blind, big blind).
pub const BLIND_LEVELS: [(i64, i64); 6] = [
    (10, 20),
    (20, 40),
    (40, 80),
    (50, 100),
    (100, 200),
    (200, 400),
];

/// AI delay whitelist in milliseconds.
pub const AI_DELAY_LEVELS: [i64; 3] = [800, 1_800, 3_200];

/// Maximum number of rooms (option default).
pub const MAX_ROOMS_DEFAULT: u32 = 3;

/// Lower bound of the room cap.
pub const MAX_ROOMS_MIN: u32 = 1;

/// Upper bound of the room cap.
pub const MAX_ROOMS_MAX: u32 = 5;

/// Maximum number of connections (option default).
pub const MAX_CONNECTIONS_DEFAULT: u32 = 24;

/// Lower bound of the connection cap.
pub const MAX_CONNECTIONS_MIN: u32 = 4;

/// Upper bound of the connection cap.
pub const MAX_CONNECTIONS_MAX: u32 = 100;

/// Maximum number of connections per IP (option default).
pub const MAX_CONNECTIONS_PER_IP_DEFAULT: u32 = 20;

/// Lower bound of the per-IP connection cap.
pub const MAX_CONNECTIONS_PER_IP_MIN: u32 = 2;

/// Upper bound of the per-IP connection cap.
pub const MAX_CONNECTIONS_PER_IP_MAX: u32 = 30;

/// Rate-limit window (option default).
pub const RATE_WINDOW_DEFAULT_MS: u32 = 10_000;

/// Lower bound of the rate-limit window.
pub const RATE_WINDOW_MIN_MS: u32 = 1_000;

/// Upper bound of the rate-limit window.
pub const RATE_WINDOW_MAX_MS: u32 = 60_000;

/// Messages allowed per rate-limit window (option default).
pub const MAX_MESSAGES_PER_WINDOW_DEFAULT: u32 = 80;

/// Lower bound of the per-window message cap.
pub const MAX_MESSAGES_PER_WINDOW_MIN: u32 = 20;

/// Upper bound of the per-window message cap.
pub const MAX_MESSAGES_PER_WINDOW_MAX: u32 = 500;

/// WebSocket close code for "capacity reached / try again later".
pub const CLOSE_TRY_AGAIN_LATER: u16 = 1013;

/// Close code used by the reference server when the message rate is exceeded.
pub const CLOSE_POLICY_VIOLATION: u16 = 1008;

/// Close code used for oversized WebSocket frames.
pub const CLOSE_MESSAGE_TOO_BIG: u16 = 1009;

/// Close code used when the same resume token is claimed elsewhere.
pub const CLOSE_SESSION_REPLACED: u16 = 4001;

/// Normal client-initiated close code.
pub const CLOSE_NORMAL: u16 = 1000;

/// Reason string when the server is at capacity.
pub const CLOSE_REASON_CAPACITY: &str = "server capacity reached";

/// Reason string when the per-window message rate is exceeded.
pub const CLOSE_REASON_RATE_LIMIT: &str = "message rate exceeded";

/// Reason string carried to the old socket when a session is resumed elsewhere.
pub const CLOSE_REASON_SESSION_REPLACED: &str = "session resumed elsewhere";

/// HTTP status returned by the ws upgrade rejection for a disallowed Origin.
pub const ORIGIN_REJECTED_HTTP_STATUS: u16 = 401;

/// Environment variable for the listen host.
pub const ENV_POKER_HOST: &str = "POKER_HOST";

/// Environment variable for the listen port.
pub const ENV_POKER_PORT: &str = "POKER_PORT";

/// Environment variable for the room cap.
pub const ENV_POKER_MAX_ROOMS: &str = "POKER_MAX_ROOMS";

/// Environment variable for the connection cap.
pub const ENV_POKER_MAX_CONNECTIONS: &str = "POKER_MAX_CONNECTIONS";

/// Environment variable for the per-IP connection cap.
pub const ENV_POKER_MAX_CONNECTIONS_PER_IP: &str = "POKER_MAX_CONNECTIONS_PER_IP";

/// Environment variable for the comma-separated Origin allowlist.
pub const ENV_POKER_ALLOWED_ORIGINS: &str = "POKER_ALLOWED_ORIGINS";

/// Environment variable to trust the X-Forwarded-For header ("1" enables it).
pub const ENV_POKER_TRUST_PROXY: &str = "POKER_TRUST_PROXY";

/// sessionStorage key of the browser client's reconnect session.
pub const SESSION_STORAGE_KEY: &str = "texas-holdem-network-session-v2";

/// Reconnect backoff base delay.
pub const RECONNECT_BASE_DELAY_MS: u64 = 650;

/// Reconnect backoff cap.
pub const RECONNECT_MAX_DELAY_MS: u64 = 10_000;

/// Number of backoff doublings before the cap applies.
pub const RECONNECT_MAX_DOUBLINGS: u32 = 4;

/// Random jitter added to the reconnect backoff (0..=jitter-1 ms).
pub const RECONNECT_JITTER_MS: u64 = 350;

/// Close codes after which the browser client must not auto-reconnect.
pub const TERMINAL_CLOSE_CODES: [u16; 5] = [
    CLOSE_NORMAL,
    CLOSE_POLICY_VIOLATION,
    CLOSE_MESSAGE_TOO_BIG,
    CLOSE_TRY_AGAIN_LATER,
    CLOSE_SESSION_REPLACED,
];

/// Request-id prefixes the browser client generates.
pub const REQUEST_ID_PREFIXES: [&str; 7] = [
    "action", "create", "join", "leave", "ready", "resume", "start",
];

/// Timeout of the desktop client's service probe in milliseconds.
pub const DESKTOP_PROBE_TIMEOUT_MS: u64 = 900;

/// Returns the protocol version (used as a serde default for the envelope field).
pub fn default_protocol_version() -> u8 {
    PROTOCOL_VERSION
}

/// True when the browser client treats a close code as terminal (no reconnect).
pub fn is_terminal_close_code(code: u16) -> bool {
    TERMINAL_CLOSE_CODES.contains(&code)
}

/// Reconnect delay of the browser client: min(cap, base * 2^min(attempt, 4))
/// plus floor(jitter * 350ms). Passing a jitter of 0.5 reproduces the golden
/// corpus values.
pub fn reconnect_delay_ms(attempt: u32, jitter: f64) -> u64 {
    let exponent = attempt.min(RECONNECT_MAX_DOUBLINGS);
    let base = RECONNECT_BASE_DELAY_MS
        .saturating_mul(1u64 << exponent)
        .min(RECONNECT_MAX_DELAY_MS);
    let jitter = (jitter.clamp(0.0, 1.0) * RECONNECT_JITTER_MS as f64).floor() as u64;
    base + jitter
}
