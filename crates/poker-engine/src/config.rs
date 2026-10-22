//! Table configuration for both variants.

use poker_core::Variant;

/// Static configuration of a table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameConfig {
    /// Game variant (standard 52-card blinds or 36-card short-deck antes).
    pub variant: Variant,
    /// Starting stack for a fresh seat.
    pub starting_stack: u64,
    /// Small blind (standard variant only; also the standard raise grid unit).
    pub small_blind: u64,
    /// Big blind (standard variant only; also the standard minimum bet).
    pub big_blind: u64,
    /// Per-player ante (short deck only).
    pub ante: u64,
    /// Minimum bet/raise for short deck (usually 2x ante).
    pub min_bet: u64,
    /// Maximum number of seats at the table.
    pub max_players: usize,
}

impl GameConfig {
    /// Standard no-limit hold'em configuration.
    #[must_use]
    pub const fn standard(starting_stack: u64, small_blind: u64, big_blind: u64) -> Self {
        Self {
            variant: Variant::Standard,
            starting_stack,
            small_blind,
            big_blind,
            ante: 0,
            min_bet: big_blind,
            max_players: 10,
        }
    }

    /// 6+ short-deck configuration (antes, no blinds).
    #[must_use]
    pub const fn short_deck(starting_stack: u64, ante: u64, min_bet: u64) -> Self {
        Self {
            variant: Variant::ShortDeck,
            starting_stack,
            small_blind: 0,
            big_blind: min_bet,
            ante,
            min_bet,
            max_players: 8,
        }
    }

    /// True for the 36-card short-deck variant.
    #[must_use]
    pub const fn is_short_deck(&self) -> bool {
        self.variant.is_short_deck()
    }

    /// The unit that bounds voluntary bets/raises: big blind, or short-deck ante.
    #[must_use]
    pub const fn minimum_bet(&self) -> u64 {
        if self.variant.is_short_deck() {
            self.min_bet
        } else {
            self.big_blind
        }
    }

    /// Raise-grid unit: small blind (standard) or ante (short deck).
    #[must_use]
    pub const fn wager_unit(&self) -> u64 {
        if self.variant.is_short_deck() {
            self.ante
        } else {
            self.small_blind
        }
    }

    /// Sets the seat cap.
    #[must_use]
    pub const fn with_max_players(mut self, max_players: usize) -> Self {
        self.max_players = max_players;
        self
    }
}
