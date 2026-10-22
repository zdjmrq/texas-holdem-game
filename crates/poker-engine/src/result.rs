//! Hand settlement result, mirroring the server's `lastHandResult` shape.

use poker_core::Card;

/// Why the hand ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandEndReason {
    /// Showdown with side pots.
    Showdown,
    /// Everyone else folded.
    Fold,
}

impl HandEndReason {
    /// Lowercase reason string used by the original JS code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            HandEndReason::Showdown => "showdown",
            HandEndReason::Fold => "fold",
        }
    }
}

/// Public identity of a player (mirrors the JS publicIdentity helper).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerIdentity {
    /// Stable player id.
    pub id: String,
    /// Seat index.
    pub seat_id: usize,
    /// Display name.
    pub name: String,
    /// True for the local human seat.
    pub is_human: bool,
}

/// One player's showdown result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandPlayerResult {
    /// Player identity.
    pub identity: PlayerIdentity,
    /// Hole cards (empty for a fold result).
    pub cards: Vec<Card>,
    /// Variant hand-rank index at showdown (0 for a fold result).
    pub hand_rank: u8,
}

/// Full result of a settled hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandResult {
    /// Showdown or fold.
    pub reason: HandEndReason,
    /// Inherited variant flag from the original JS result.
    pub is_short_deck: bool,
    /// Total pot that was settled.
    pub pot: u64,
    /// Strongest hand-rank index at showdown (0 for a fold result).
    pub hand_rank: u8,
    /// Winning players in first-win order.
    pub winners: Vec<PlayerIdentity>,
    /// Chips won per player id, in winner-discovery order.
    pub winner_amounts: Vec<(String, u64)>,
    /// Per-player results (showdown: players in hand; fold: dealt-in players).
    pub results: Vec<HandPlayerResult>,
}
