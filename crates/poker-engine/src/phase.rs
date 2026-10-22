//! Betting phases of a hand.

/// The phase of a hand, using the same string names as the original JS code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// No hand in progress.
    Idle,
    /// Pre-flop betting.
    Preflop,
    /// Flop betting (three community cards).
    Flop,
    /// Turn betting (four community cards).
    Turn,
    /// River betting (five community cards).
    River,
    /// Showdown / hand settlement.
    Showdown,
}

impl Phase {
    /// Lowercase phase name used by the wire protocol and golden corpus.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Preflop => "preflop",
            Phase::Flop => "flop",
            Phase::Turn => "turn",
            Phase::River => "river",
            Phase::Showdown => "showdown",
        }
    }

    /// Parses a phase name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Phase> {
        match name {
            "idle" => Some(Phase::Idle),
            "preflop" => Some(Phase::Preflop),
            "flop" => Some(Phase::Flop),
            "turn" => Some(Phase::Turn),
            "river" => Some(Phase::River),
            "showdown" => Some(Phase::Showdown),
            _ => None,
        }
    }

    /// True while a hand is in progress.
    #[must_use]
    pub const fn is_active(self) -> bool {
        !matches!(self, Phase::Idle | Phase::Showdown)
    }
}
