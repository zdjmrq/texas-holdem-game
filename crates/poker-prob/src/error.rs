//! Error type returned by preparation and equity calculations.

use std::fmt;

/// Errors produced while preparing or enumerating an equity calculation.
///
/// The variants mirror the `throw new Error(...)` branches of
/// `js/probability-core.js`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbError {
    /// The player must supply exactly two hole cards.
    WrongHoleCardCount {
        /// Number of hole cards that were supplied.
        provided: usize,
    },
    /// At most five community cards are supported.
    TooManyCommunityCards {
        /// Number of community cards that were supplied.
        provided: usize,
    },
    /// A card appears more than once across the hole and community cards.
    DuplicateKnownCard,
    /// The remaining deck cannot cover the board runout plus every opponent.
    TooManyOpponents {
        /// Number of opponents after clamping to the variant maximum.
        opponents: usize,
        /// Number of cards left in the deck after removing the known cards.
        remaining: usize,
        /// Number of cards the calculation needs from the deck.
        needed: usize,
    },
    /// Exact heads-up enumeration only exists for a turn or river board.
    ExactRequiresTurnOrRiver {
        /// Number of community cards in the prepared state.
        board_cards: usize,
    },
}

impl fmt::Display for ProbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbError::WrongHoleCardCount { provided } => {
                write!(f, "Texas Hold'em needs two hole cards (got {provided})")
            }
            ProbError::TooManyCommunityCards { provided } => {
                write!(
                    f,
                    "Texas Hold'em needs at most five board cards (got {provided})"
                )
            }
            ProbError::DuplicateKnownCard => write!(f, "Duplicate known card"),
            ProbError::TooManyOpponents {
                opponents,
                remaining,
                needed,
            } => write!(
                f,
                "Too many opponents for remaining cards: {opponents} opponents need {needed} cards, only {remaining} left"
            ),
            ProbError::ExactRequiresTurnOrRiver { board_cards } => write!(
                f,
                "Exact heads-up enumeration needs four or five board cards (got {board_cards})"
            ),
        }
    }
}

impl std::error::Error for ProbError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_stable() {
        assert_eq!(
            ProbError::WrongHoleCardCount { provided: 1 }.to_string(),
            "Texas Hold'em needs two hole cards (got 1)"
        );
        assert_eq!(
            ProbError::TooManyCommunityCards { provided: 6 }.to_string(),
            "Texas Hold'em needs at most five board cards (got 6)"
        );
        assert_eq!(
            ProbError::DuplicateKnownCard.to_string(),
            "Duplicate known card"
        );
        assert_eq!(
            ProbError::TooManyOpponents {
                opponents: 9,
                remaining: 5,
                needed: 18
            }
            .to_string(),
            "Too many opponents for remaining cards: 9 opponents need 18 cards, only 5 left"
        );
        assert_eq!(
            ProbError::ExactRequiresTurnOrRiver { board_cards: 3 }.to_string(),
            "Exact heads-up enumeration needs four or five board cards (got 3)"
        );
    }
}
