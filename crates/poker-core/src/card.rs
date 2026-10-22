//! Card representation.
//!
//! Internally a card is the dense index suit * 13 + (rank - 2) in 0..52, with
//! suits ordered spades, hearts, diamonds, clubs (the original JS order).
//! The original JS probability core used a suit * 15 + value encoding for
//! 5..7-card evaluation; [`Card::from_js_code`] and [`Card::to_js_code`]
//! convert to and from that encoding.

use std::fmt;
use std::str::FromStr;

use crate::error::CardParseError;

/// The four suits, ordered to match the original JS SUITS array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Suit {
    /// Spades.
    Spades = 0,
    /// Hearts.
    Hearts = 1,
    /// Diamonds.
    Diamonds = 2,
    /// Clubs.
    Clubs = 3,
}

impl Suit {
    /// All suits in the original JS order.
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];

    /// Lowercase English name used by the wire protocol, for example "spades".
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Suit::Spades => "spades",
            Suit::Hearts => "hearts",
            Suit::Diamonds => "diamonds",
            Suit::Clubs => "clubs",
        }
    }

    /// Unicode suit symbol.
    #[must_use]
    pub const fn symbol(self) -> char {
        match self {
            Suit::Spades => '♠',
            Suit::Hearts => '♥',
            Suit::Diamonds => '♦',
            Suit::Clubs => '♣',
        }
    }

    /// One-character suit code used by compact card text: s, h, d or c.
    #[must_use]
    pub const fn code(self) -> char {
        match self {
            Suit::Spades => 's',
            Suit::Hearts => 'h',
            Suit::Diamonds => 'd',
            Suit::Clubs => 'c',
        }
    }

    /// Parses a lowercase English suit name such as "spades".
    #[must_use]
    pub fn from_name(name: &str) -> Option<Suit> {
        match name {
            "spades" => Some(Suit::Spades),
            "hearts" => Some(Suit::Hearts),
            "diamonds" => Some(Suit::Diamonds),
            "clubs" => Some(Suit::Clubs),
            _ => None,
        }
    }

    /// Parses a one-character suit code (s, h, d, c) or a suit symbol.
    #[must_use]
    pub fn from_code(code: char) -> Option<Suit> {
        match code.to_ascii_lowercase() {
            's' | '♠' => Some(Suit::Spades),
            'h' | '♥' => Some(Suit::Hearts),
            'd' | '♦' => Some(Suit::Diamonds),
            'c' | '♣' => Some(Suit::Clubs),
            _ => None,
        }
    }
}

/// Card ranks Two..=Ace; the numeric value 2..=14 matches the original JS
/// VALUES map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Rank {
    /// 2
    Two = 2,
    /// 3
    Three = 3,
    /// 4
    Four = 4,
    /// 5
    Five = 5,
    /// 6
    Six = 6,
    /// 7
    Seven = 7,
    /// 8
    Eight = 8,
    /// 9
    Nine = 9,
    /// 10
    Ten = 10,
    /// Jack
    Jack = 11,
    /// Queen
    Queen = 12,
    /// King
    King = 13,
    /// Ace
    Ace = 14,
}

impl Rank {
    /// Rank from its poker value (2..=14), or None.
    #[must_use]
    pub const fn from_value(value: u8) -> Option<Rank> {
        match value {
            2 => Some(Rank::Two),
            3 => Some(Rank::Three),
            4 => Some(Rank::Four),
            5 => Some(Rank::Five),
            6 => Some(Rank::Six),
            7 => Some(Rank::Seven),
            8 => Some(Rank::Eight),
            9 => Some(Rank::Nine),
            10 => Some(Rank::Ten),
            11 => Some(Rank::Jack),
            12 => Some(Rank::Queen),
            13 => Some(Rank::King),
            14 => Some(Rank::Ace),
            _ => None,
        }
    }

    /// Poker value 2..=14 (ace high).
    #[must_use]
    pub const fn value(self) -> u8 {
        self as u8
    }

    /// Canonical rank token used by the protocol: 2..10 are digits, then J, Q, K, A.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        }
    }

    /// Parses a rank token: 2..9, T or 10, J, Q, K, A (case-insensitive).
    #[must_use]
    pub fn from_str_token(token: &str) -> Option<Rank> {
        match token.to_ascii_uppercase().as_str() {
            "2" => Some(Rank::Two),
            "3" => Some(Rank::Three),
            "4" => Some(Rank::Four),
            "5" => Some(Rank::Five),
            "6" => Some(Rank::Six),
            "7" => Some(Rank::Seven),
            "8" => Some(Rank::Eight),
            "9" => Some(Rank::Nine),
            "10" | "T" => Some(Rank::Ten),
            "J" => Some(Rank::Jack),
            "Q" => Some(Rank::Queen),
            "K" => Some(Rank::King),
            "A" => Some(Rank::Ace),
            _ => None,
        }
    }
}

/// A single playing card, stored as a dense index in 0..52.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card(u8);

impl Card {
    /// Total number of cards in a standard deck.
    pub const COUNT: u8 = 52;

    /// Builds a card from a suit and a rank.
    #[must_use]
    pub const fn new(suit: Suit, rank: Rank) -> Card {
        Card(suit as u8 * 13 + (rank as u8 - 2))
    }

    /// Builds a card from its dense index (0..52), or None when out of range.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Card> {
        if index < Card::COUNT {
            Some(Card(index))
        } else {
            None
        }
    }

    /// Dense card index (0..52).
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The card's suit.
    #[must_use]
    pub const fn suit(self) -> Suit {
        match self.0 / 13 {
            0 => Suit::Spades,
            1 => Suit::Hearts,
            2 => Suit::Diamonds,
            _ => Suit::Clubs,
        }
    }

    /// The card's rank.
    #[must_use]
    pub const fn rank(self) -> Rank {
        match self.0 % 13 + 2 {
            2 => Rank::Two,
            3 => Rank::Three,
            4 => Rank::Four,
            5 => Rank::Five,
            6 => Rank::Six,
            7 => Rank::Seven,
            8 => Rank::Eight,
            9 => Rank::Nine,
            10 => Rank::Ten,
            11 => Rank::Jack,
            12 => Rank::Queen,
            13 => Rank::King,
            _ => Rank::Ace,
        }
    }

    /// Poker rank value 2..=14.
    #[must_use]
    pub const fn rank_value(self) -> u8 {
        self.0 % 13 + 2
    }

    /// Converts from the original JS evaluator encoding suit * 15 + value.
    #[must_use]
    pub const fn from_js_code(code: u8) -> Option<Card> {
        let value = code % 15;
        let suit = code / 15;
        if value < 2 || value > 14 || suit > 3 {
            return None;
        }
        Some(Card(suit * 13 + (value - 2)))
    }

    /// Converts to the original JS evaluator encoding suit * 15 + value.
    #[must_use]
    pub const fn to_js_code(self) -> u8 {
        self.suit() as u8 * 15 + self.rank_value()
    }

    /// Bit for this card in a standard 52-bit deck mask.
    #[must_use]
    pub const fn bit(self) -> u64 {
        1u64 << self.0
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank().as_str(), self.suit().symbol())
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank().as_str(), self.suit().code())
    }
}

impl FromStr for Card {
    type Err = CardParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let mut chars = trimmed.chars();
        let suit_char = chars.next_back().ok_or(CardParseError::Empty)?;
        let suit = Suit::from_code(suit_char).ok_or(CardParseError::Suit(suit_char))?;
        let rank_token: String = chars.collect();
        let rank = Rank::from_str_token(&rank_token)
            .ok_or_else(|| CardParseError::Rank(rank_token.clone()))?;
        Ok(Card::new(suit, rank))
    }
}

/// Parses a compact card string such as "As" or "10h".
///
/// # Errors
///
/// Returns [`CardParseError`] when the rank or suit is invalid.
pub fn parse_card(text: &str) -> Result<Card, CardParseError> {
    text.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_roundtrips_its_dense_index() {
        for index in 0..Card::COUNT {
            let card = Card::from_index(index).expect("valid index");
            assert_eq!(card.index(), index);
            assert_eq!(Card::new(card.suit(), card.rank()), card);
        }
        assert!(Card::from_index(52).is_none());
    }

    #[test]
    fn parses_compact_card_text() {
        assert_eq!(
            "As".parse::<Card>().expect("As"),
            Card::new(Suit::Spades, Rank::Ace)
        );
        assert_eq!(
            "10h".parse::<Card>().expect("10h"),
            Card::new(Suit::Hearts, Rank::Ten)
        );
        assert_eq!(
            "Td".parse::<Card>().expect("Td"),
            Card::new(Suit::Diamonds, Rank::Ten)
        );
        assert_eq!(
            "2c".parse::<Card>().expect("2c"),
            Card::new(Suit::Clubs, Rank::Two)
        );
        assert_eq!(
            "A♠".parse::<Card>().expect("A♠"),
            Card::new(Suit::Spades, Rank::Ace)
        );
        assert!("1x".parse::<Card>().is_err());
        assert!("".parse::<Card>().is_err());
    }

    #[test]
    fn js_encoding_roundtrips() {
        for code in 0..60 {
            if let Some(card) = Card::from_js_code(code) {
                assert_eq!(card.to_js_code(), code);
            }
        }
        assert!(Card::from_js_code(15).is_none());
        assert!(Card::from_js_code(31).is_none());
        assert!(Card::from_js_code(60).is_none());
        assert_eq!(Card::from_js_code(3), Card::from_index(1));
    }

    #[test]
    fn debug_uses_compact_notation() {
        let card = Card::new(Suit::Hearts, Rank::Ten);
        assert_eq!(format!("{card:?}"), "10h");
        assert_eq!(format!("{card}"), "10♥");
    }
}
