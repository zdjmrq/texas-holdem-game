//! Player seat state inside a hand.

use poker_core::Card;

/// The kind of action recorded in a player's last-action field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionLabel {
    /// Fold.
    Fold,
    /// Check.
    Check,
    /// Call.
    Call,
    /// Volunary raise/bet.
    Raise,
    /// All-in.
    AllIn,
    /// Small blind post.
    SmallBlind,
    /// Big blind post.
    BigBlind,
    /// Ante post (short deck).
    Ante,
    /// Dealer extra ante post (short deck).
    DealerAnte,
}

impl ActionLabel {
    /// Lowercase label used by the JS implementation and the golden corpus.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ActionLabel::Fold => "fold",
            ActionLabel::Check => "check",
            ActionLabel::Call => "call",
            ActionLabel::Raise => "raise",
            ActionLabel::AllIn => "allin",
            ActionLabel::SmallBlind => "small blind",
            ActionLabel::BigBlind => "big blind",
            ActionLabel::Ante => "ante",
            ActionLabel::DealerAnte => "dealer ante",
        }
    }
}

/// A player's most recent action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastAction {
    /// What the player did.
    pub action: ActionLabel,
    /// Chips moved by that action.
    pub amount: u64,
}

/// One seat at the table.
///
/// The engine owns the in-hand state; room/personality data (avatars, AI
/// references, connection bookkeeping) lives in the layers above.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    /// Stable player id (for example "human" or "ai-3").
    pub id: String,
    /// Seat index at the table.
    pub seat_id: usize,
    /// Display name.
    pub name: String,
    /// True for the local human seat.
    pub is_human: bool,
    /// Connection state; a disconnected human seat is skipped when a hand starts.
    pub connected: bool,
    /// Player left the table; the seat is skipped when a hand starts.
    pub left: bool,
    /// Chip stack.
    pub stack: u64,
    /// Hole cards (empty before the hand starts).
    pub hole_cards: Vec<Card>,
    /// Total chips this player has put into the pot during the hand.
    pub chips_in_pot: u64,
    /// Chips committed during the current betting round.
    pub round_bet: u64,
    /// Folded this hand.
    pub folded: bool,
    /// All-in this hand.
    pub is_all_in: bool,
    /// Dealt into the current hand.
    pub dealt_in: bool,
    /// Most recent action within this hand.
    pub last_action: Option<LastAction>,
}

impl Player {
    /// Creates a seat with a fresh stack.
    #[must_use]
    pub fn new(id: impl Into<String>, seat_id: usize, name: impl Into<String>, stack: u64) -> Self {
        Self {
            id: id.into(),
            seat_id,
            name: name.into(),
            is_human: seat_id == 0,
            connected: true,
            left: false,
            stack,
            hole_cards: Vec::new(),
            chips_in_pot: 0,
            round_bet: 0,
            folded: false,
            is_all_in: false,
            dealt_in: false,
            last_action: None,
        }
    }

    /// Marks the seat as the local human player.
    #[must_use]
    pub const fn with_human(mut self, is_human: bool) -> Self {
        self.is_human = is_human;
        self
    }

    /// True when the player can be dealt into the next hand.
    #[must_use]
    pub fn active_for_new_hand(&self) -> bool {
        self.stack > 0 && !self.left && (!self.is_human || self.connected)
    }
}
