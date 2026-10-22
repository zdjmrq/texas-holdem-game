//! Small error types shared by the core crate.

use std::fmt;

use crate::card::Card;

/// Error returned when parsing compact card text such as "As" or "10h".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardParseError {
    /// The input was empty.
    Empty,
    /// The suit token was not recognized.
    Suit(char),
    /// The rank token was not recognized.
    Rank(String),
}

impl fmt::Display for CardParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardParseError::Empty => write!(f, "empty card text"),
            CardParseError::Suit(c) => write!(f, "invalid suit: {c}"),
            CardParseError::Rank(r) => write!(f, "invalid rank: {r}"),
        }
    }
}

impl std::error::Error for CardParseError {}

/// Error returned when building a deck from an explicit card list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckError {
    /// The same card appeared more than once.
    Duplicate(Card),
    /// The deck contains more cards than the variant allows.
    TooManyCards {
        /// Number of cards supplied.
        supplied: usize,
        /// Maximum number of cards for the variant.
        maximum: usize,
    },
}

impl fmt::Display for DeckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeckError::Duplicate(card) => write!(f, "duplicate card: {card}"),
            DeckError::TooManyCards { supplied, maximum } => {
                write!(f, "too many cards: {supplied} > {maximum}")
            }
        }
    }
}

impl std::error::Error for DeckError {}
