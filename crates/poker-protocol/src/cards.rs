//! Wire representation of playing cards.
//!
//! The reference protocol sends cards as {"rank": "A", "suit": "hearts"}.
//! Ranks use the JS deck tokens "2".."10", "J", "Q", "K", "A" (the ten is the
//! two-character string "10"); suits are the full lowercase names "spades",
//! "hearts", "diamonds", "clubs". Showdown results additionally carry the
//! numeric value (2..=14).

use serde::{Deserialize, Serialize};

/// Standard rank tokens in deck order.
pub const RANKS: [&str; 13] = [
    "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
];

/// Suit names in the reference deck order.
pub const SUITS: [&str; 4] = ["spades", "hearts", "diamonds", "clubs"];
/// A face-up card as broadcast in game_state (communityCards, yourCards).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardDto {
    /// Rank token: "2".."10", "J", "Q", "K", "A".
    pub rank: String,
    /// Suit name: "spades", "hearts", "diamonds" or "clubs".
    pub suit: String,
}

impl CardDto {
    /// Builds a card from its wire tokens.
    pub fn new(rank: impl Into<String>, suit: impl Into<String>) -> Self {
        Self {
            rank: rank.into(),
            suit: suit.into(),
        }
    }

    /// Creates a card from its numeric rank value (2..=14) and suit name.
    pub fn from_value(value: u8, suit: &str) -> Option<Self> {
        let rank = match value {
            2..=10 => value.to_string(),
            11 => "J".to_owned(),
            12 => "Q".to_owned(),
            13 => "K".to_owned(),
            14 => "A".to_owned(),
            _ => return None,
        };
        Some(Self::new(rank, suit))
    }

    /// Returns the numeric rank value (2..=14) for a valid wire token.
    pub fn rank_value(&self) -> Option<u8> {
        match self.rank.as_str() {
            "2" => Some(2),
            "3" => Some(3),
            "4" => Some(4),
            "5" => Some(5),
            "6" => Some(6),
            "7" => Some(7),
            "8" => Some(8),
            "9" => Some(9),
            "10" => Some(10),
            "J" => Some(11),
            "Q" => Some(12),
            "K" => Some(13),
            "A" => Some(14),
            _ => None,
        }
    }

    /// True when both tokens are part of the reference deck vocabulary.
    pub fn is_valid(&self) -> bool {
        self.rank_value().is_some() && SUITS.contains(&self.suit.as_str())
    }
}
/// A revealed showdown card. The reference server clones hole cards with their
/// numeric value for lastHandResult.results[].cards; the value key is absent on
/// other card objects (hence the optional field).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevealedCard {
    /// Rank token as in CardDto.
    pub rank: String,
    /// Suit name as in CardDto.
    pub suit: String,
    /// Numeric rank value (2..=14); present on showdown-revealed cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<i64>,
}

impl RevealedCard {
    /// Builds a revealed card from tokens.
    pub fn new(rank: impl Into<String>, suit: impl Into<String>, value: Option<i64>) -> Self {
        Self {
            rank: rank.into(),
            suit: suit.into(),
            value,
        }
    }

    /// Converts to the plain card shape used by communityCards and yourCards.
    pub fn card(&self) -> CardDto {
        CardDto::new(self.rank.clone(), self.suit.clone())
    }
}
