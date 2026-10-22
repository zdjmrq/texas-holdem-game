//! Process-local counters for operational visibility.
//!
//! These are intentionally simple atomics: the reference server is a small
//! single-process room server, so a metrics facade or exporter would be
//! overkill. ServerHandle::metrics() returns a snapshot; the example binary
//! logs it on shutdown and integrators can poll it.

use std::sync::atomic::{AtomicU64, Ordering};

/// Atomic counters shared by all tasks of one server instance.
#[derive(Debug, Default)]
pub(crate) struct ServerMetrics {
    pub(crate) connections_accepted: AtomicU64,
    pub(crate) connections_rejected_capacity: AtomicU64,
    pub(crate) connections_rejected_origin: AtomicU64,
    pub(crate) messages_received: AtomicU64,
    pub(crate) parse_errors: AtomicU64,
    pub(crate) rate_limited: AtomicU64,
    pub(crate) rooms_created: AtomicU64,
    pub(crate) rooms_closed: AtomicU64,
    pub(crate) resumes: AtomicU64,
    pub(crate) session_kicks: AtomicU64,
    pub(crate) actions_accepted: AtomicU64,
    pub(crate) actions_rejected: AtomicU64,
    pub(crate) slow_consumers: AtomicU64,
    pub(crate) heartbeat_timeouts: AtomicU64,
    pub(crate) ai_decisions: AtomicU64,
    pub(crate) ai_decision_total_ms: AtomicU64,
    pub(crate) ai_decision_max_ms: AtomicU64,
}

impl ServerMetrics {
    pub(crate) fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add(counter: &AtomicU64, value: u64) {
        counter.fetch_add(value, Ordering::Relaxed);
    }

    pub(crate) fn observe_max(counter: &AtomicU64, value: u64) {
        counter.fetch_max(value, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> ServerMetricsSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        ServerMetricsSnapshot {
            connections_accepted: load(&self.connections_accepted),
            connections_rejected_capacity: load(&self.connections_rejected_capacity),
            connections_rejected_origin: load(&self.connections_rejected_origin),
            messages_received: load(&self.messages_received),
            parse_errors: load(&self.parse_errors),
            rate_limited: load(&self.rate_limited),
            rooms_created: load(&self.rooms_created),
            rooms_closed: load(&self.rooms_closed),
            resumes: load(&self.resumes),
            session_kicks: load(&self.session_kicks),
            actions_accepted: load(&self.actions_accepted),
            actions_rejected: load(&self.actions_rejected),
            slow_consumers: load(&self.slow_consumers),
            heartbeat_timeouts: load(&self.heartbeat_timeouts),
            ai_decisions: load(&self.ai_decisions),
            ai_decision_total_ms: load(&self.ai_decision_total_ms),
            ai_decision_max_ms: load(&self.ai_decision_max_ms),
        }
    }
}

/// Point-in-time copy of ServerMetrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerMetricsSnapshot {
    /// Accepted WebSocket connections.
    pub connections_accepted: u64,
    /// Connections rejected by the connection/per-IP cap (close 1013).
    pub connections_rejected_capacity: u64,
    /// Upgrades rejected by the Origin allowlist (HTTP 401).
    pub connections_rejected_origin: u64,
    /// Text frames received.
    pub messages_received: u64,
    /// Frames that failed protocol parsing (INVALID_REQUEST replies).
    pub parse_errors: u64,
    /// Messages rejected by the per-connection rate limit (close 1008).
    pub rate_limited: u64,
    /// Rooms created.
    pub rooms_created: u64,
    /// Rooms removed (explicit cleanup or TTL).
    pub rooms_closed: u64,
    /// Successful resume_session requests.
    pub resumes: u64,
    /// Old sockets closed with 4001 because the session was resumed elsewhere.
    pub session_kicks: u64,
    /// Accepted player actions.
    pub actions_accepted: u64,
    /// Rejected player actions (stale ids, illegal action, raise grid).
    pub actions_rejected: u64,
    /// Connections closed by the bounded outbound queue (slow consumer).
    pub slow_consumers: u64,
    /// Connections dropped by the heartbeat because no pong arrived.
    pub heartbeat_timeouts: u64,
    /// AI decisions applied.
    pub ai_decisions: u64,
    /// Total AI decision wall time in milliseconds.
    pub ai_decision_total_ms: u64,
    /// Worst observed AI decision wall time in milliseconds.
    pub ai_decision_max_ms: u64,
}
