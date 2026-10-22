//! Request-id generation matching js/network.js lines 72-75.
//!
//! Reference format: prefix-base36(Date.now())-base36(sequence), with prefix
//! from {action, create, join, leave, ready, resume, start} and the sequence
//! starting at 1. The server treats the value as an opaque string and only
//! echoes it back.

use poker_protocol::RequestId;

/// The request-id prefixes the reference browser client uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestKind {
    /// player_action.
    Action,
    /// create_room.
    Create,
    /// join_room.
    Join,
    /// leave_room.
    Leave,
    /// ready_for_next.
    Ready,
    /// resume_session.
    Resume,
    /// start_game.
    Start,
}

impl RequestKind {
    /// All kinds, in the order of the golden corpus list.
    pub const ALL: [Self; 7] = [
        Self::Action,
        Self::Create,
        Self::Join,
        Self::Leave,
        Self::Ready,
        Self::Resume,
        Self::Start,
    ];

    /// Prefix used in the generated request id.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Create => "create",
            Self::Join => "join",
            Self::Leave => "leave",
            Self::Ready => "ready",
            Self::Resume => "resume",
            Self::Start => "start",
        }
    }
}

/// Monotonic request-id generator.
#[derive(Debug, Clone, Default)]
pub struct RequestIdGenerator {
    sequence: u64,
}

impl RequestIdGenerator {
    /// Creates a generator whose first id uses sequence 1.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the next request id for kind at the given wall-clock time.
    pub fn next(&mut self, kind: RequestKind, now_millis: u64) -> RequestId {
        self.sequence = self.sequence.saturating_add(1);
        RequestId::from(format!(
            "{}-{}-{}",
            kind.prefix(),
            base36(now_millis),
            base36(self.sequence)
        ))
    }
}

/// Lower-case base-36 encoding of a non-negative integer (JavaScript
/// Number.prototype.toString(36) shape).
pub fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_owned();
    }
    let mut buffer = [0u8; 13];
    let mut index = buffer.len();
    while value > 0 {
        index -= 1;
        buffer[index] = DIGITS[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8_lossy(&buffer[index..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base36_matches_javascript() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(1_700_000_000_000), "loyw3v28");
    }

    #[test]
    fn request_ids_use_reference_format() {
        let mut generator = RequestIdGenerator::new();
        let first = generator.next(RequestKind::Action, 1_700_000_000_000);
        assert_eq!(
            first.as_str(),
            Some("action-loyw3v28-1"),
            "prefix + base36(now) + base36(seq)"
        );
        let second = generator.next(RequestKind::Resume, 1_700_000_000_000);
        assert_eq!(second.as_str(), Some("resume-loyw3v28-2"));
    }

    #[test]
    fn all_prefixes_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in RequestKind::ALL {
            assert!(seen.insert(kind.prefix()), "duplicate prefix");
        }
        assert_eq!(seen.len(), 7);
    }
}
