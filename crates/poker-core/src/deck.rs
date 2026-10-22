//! Deck construction, shuffling and dealing.
//!
//! Card order matches the original JS decks (suits outer, ranks ascending) so
//! the same seeded Fisher-Yates shuffle produces the same deal in both
//! implementations. [`Deck::deal`] pops from the end of the card list, exactly
//! like the original JS Deck.deal.

use crate::card::Card;
use crate::error::DeckError;
use crate::rng::RandomSource;
use crate::variant::Variant;

/// A deck of cards in a fixed order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deck {
    cards: Vec<Card>,
}

impl Deck {
    /// Builds a fresh deck for the variant in the original JS order
    /// (suit-major, ranks ascending inside each suit).
    #[must_use]
    pub fn new(variant: Variant) -> Deck {
        let mut cards = Vec::with_capacity(variant.deck_size());
        for suit in crate::card::Suit::ALL {
            for &rank in variant.ranks() {
                cards.push(Card::new(suit, rank));
            }
        }
        Deck { cards }
    }

    /// Builds a standard 52-card deck.
    #[must_use]
    pub fn standard() -> Deck {
        Deck::new(Variant::Standard)
    }

    /// Builds a short 36-card deck.
    #[must_use]
    pub fn short_deck() -> Deck {
        Deck::new(Variant::ShortDeck)
    }

    /// Builds a deck from an explicit card order, rejecting duplicates and
    /// lists longer than the variant deck.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Duplicate`] or [`DeckError::TooManyCards`].
    pub fn from_cards(cards: Vec<Card>) -> Result<Deck, DeckError> {
        let maximum = usize::from(Card::COUNT);
        if cards.len() > maximum {
            return Err(DeckError::TooManyCards {
                supplied: cards.len(),
                maximum,
            });
        }
        let mut seen = 0u64;
        for card in &cards {
            let bit = card.bit();
            if seen & bit != 0 {
                return Err(DeckError::Duplicate(*card));
            }
            seen |= bit;
        }
        Ok(Deck { cards })
    }

    /// Cards currently in the deck, in order.
    #[must_use]
    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    /// Number of cards left.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// True when the deck is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// Deterministic Fisher-Yates shuffle, bit-compatible with the original JS
    /// shuffle when driven by the same RandomSource sequence.
    pub fn shuffle<R: RandomSource + ?Sized>(&mut self, rng: &mut R) {
        for i in (1..self.cards.len()).rev() {
            let j = (rng.next_f64() * ((i + 1) as f64)) as usize;
            self.cards.swap(i, j);
        }
    }

    /// Deals the next card (pops the last element, matching JS Deck.deal).
    pub fn deal(&mut self) -> Option<Card> {
        self.cards.pop()
    }

    /// Removes every copy of the given cards from the deck.
    pub fn remove_cards(&mut self, cards: &[Card]) {
        let mut mask = 0u64;
        for card in cards {
            mask |= card.bit();
        }
        self.cards.retain(|card| mask & card.bit() == 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_deck_has_52_distinct_cards_in_js_order() {
        let deck = Deck::standard();
        assert_eq!(deck.len(), 52);
        assert_eq!(
            deck.cards()[0],
            Card::new(crate::card::Suit::Spades, crate::card::Rank::Two)
        );
        assert_eq!(
            deck.cards()[12],
            Card::new(crate::card::Suit::Spades, crate::card::Rank::Ace)
        );
        assert_eq!(
            deck.cards()[13],
            Card::new(crate::card::Suit::Hearts, crate::card::Rank::Two)
        );
        assert_eq!(
            deck.cards()[51],
            Card::new(crate::card::Suit::Clubs, crate::card::Rank::Ace)
        );
        let mut seen = 0u64;
        for card in deck.cards() {
            assert_eq!(seen & card.bit(), 0);
            seen |= card.bit();
        }
    }

    #[test]
    fn short_deck_has_36_cards() {
        let deck = Deck::short_deck();
        assert_eq!(deck.len(), 36);
        assert!(deck.cards().iter().all(|card| card.rank_value() >= 6));
    }

    #[test]
    fn shuffle_is_deterministic_for_a_seeded_rng() {
        let mut a = Deck::standard();
        let mut b = Deck::standard();
        let mut rng_a = crate::rng::XorShift32Rng::from_seed("shuffle-seed");
        let mut rng_b = crate::rng::XorShift32Rng::from_seed("shuffle-seed");
        a.shuffle(&mut rng_a);
        b.shuffle(&mut rng_b);
        assert_eq!(a, b);
        assert_ne!(a.cards(), Deck::standard().cards());
    }

    #[test]
    fn deal_pops_from_the_end() {
        let mut deck = Deck::standard();
        assert_eq!(
            deck.deal(),
            Some(Card::new(crate::card::Suit::Clubs, crate::card::Rank::Ace))
        );
        assert_eq!(
            deck.deal(),
            Some(Card::new(crate::card::Suit::Clubs, crate::card::Rank::King))
        );
        assert_eq!(deck.len(), 50);
    }

    #[test]
    fn duplicate_cards_are_rejected() {
        let card = Card::new(crate::card::Suit::Spades, crate::card::Rank::Ace);
        let result = Deck::from_cards(vec![card, card]);
        assert_eq!(result, Err(DeckError::Duplicate(card)));
    }

    #[test]
    fn removing_known_cards_shrinks_the_deck() {
        let mut deck = Deck::standard();
        let card = Card::new(crate::card::Suit::Spades, crate::card::Rank::Ace);
        deck.remove_cards(&[card]);
        assert_eq!(deck.len(), 51);
        assert!(!deck.cards().contains(&card));
    }
}
