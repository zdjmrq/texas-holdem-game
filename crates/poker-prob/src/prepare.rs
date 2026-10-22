//! Input validation and shared preparation for exact and Monte Carlo equity.

use poker_core::{Card, Deck, Variant};

use crate::ProbError;

/// Validated inputs for an equity calculation.
///
/// Mirrors the object returned by the original JS `prepare(options)` helper:
/// the variant, the two hole cards, the community cards, the remaining deck in
/// canonical order and the clamped opponent count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    variant: Variant,
    player: [Card; 2],
    board: Vec<Card>,
    deck: Vec<Card>,
    opponents: usize,
}

impl Prepared {
    /// The game variant.
    #[must_use]
    pub const fn variant(&self) -> Variant {
        self.variant
    }

    /// The two hole cards in the order they were supplied.
    #[must_use]
    pub const fn player(&self) -> [Card; 2] {
        self.player
    }

    /// The community cards in the order they were supplied.
    #[must_use]
    pub fn board(&self) -> &[Card] {
        &self.board
    }

    /// Remaining deck in the original deck order (suit-major, ranks
    /// ascending), with every known card removed.
    #[must_use]
    pub fn remaining_deck(&self) -> &[Card] {
        &self.deck
    }

    /// Opponent count after clamping to `1..=7` (short deck) or `1..=9`
    /// (standard).
    #[must_use]
    pub const fn opponents(&self) -> usize {
        self.opponents
    }

    /// Number of community cards still to come (`0..=5`).
    #[must_use]
    pub fn missing_board_cards(&self) -> usize {
        5 - self.board.len()
    }
}

/// Validates the calculation inputs and builds the remaining deck.
///
/// Mirrors `prepare()` in `js/probability-core.js` exactly:
///
/// * exactly two hole cards and at most five board cards are required;
/// * every known card must be distinct;
/// * the opponent count is clamped to the variant maximum
///   (`Math.max(1, Math.min(isShortDeck ? 7 : 9, numOpponents))`);
/// * the remaining deck keeps the canonical suit-major/rank-ascending order
///   and must cover the board runout plus two cards per opponent.
///
/// # Errors
///
/// Returns [`ProbError::WrongHoleCardCount`],
/// [`ProbError::TooManyCommunityCards`], [`ProbError::DuplicateKnownCard`] or
/// [`ProbError::TooManyOpponents`] when the inputs cannot be analysed.
pub fn prepare(
    player: &[Card],
    board: &[Card],
    num_opponents: usize,
    variant: Variant,
) -> Result<Prepared, ProbError> {
    if player.len() != 2 {
        return Err(ProbError::WrongHoleCardCount {
            provided: player.len(),
        });
    }
    if board.len() > 5 {
        return Err(ProbError::TooManyCommunityCards {
            provided: board.len(),
        });
    }

    let mut known_mask = 0u64;
    for card in player.iter().chain(board) {
        known_mask |= card.bit();
    }
    if known_mask.count_ones() as usize != player.len() + board.len() {
        return Err(ProbError::DuplicateKnownCard);
    }

    let maximum_opponents = if variant.is_short_deck() { 7 } else { 9 };
    let opponents = num_opponents.clamp(1, maximum_opponents);

    let canonical_deck = Deck::new(variant);
    let mut deck = Vec::with_capacity(variant.deck_size());
    for &card in canonical_deck.cards() {
        if known_mask & card.bit() == 0 {
            deck.push(card);
        }
    }

    let needed = (5 - board.len()) + opponents * 2;
    if deck.len() < needed {
        return Err(ProbError::TooManyOpponents {
            opponents,
            remaining: deck.len(),
            needed,
        });
    }

    let hole_cards = [player[0], player[1]];
    Ok(Prepared {
        variant,
        player: hole_cards,
        board: board.to_vec(),
        deck,
        opponents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::Deck;

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| token.parse().expect("card"))
            .collect()
    }

    #[test]
    fn rejects_wrong_hole_card_count() {
        let error = prepare(&cards("As"), &[], 1, Variant::Standard).unwrap_err();
        assert_eq!(error, ProbError::WrongHoleCardCount { provided: 1 });
        let error = prepare(&cards("As Kh Qd"), &[], 1, Variant::Standard).unwrap_err();
        assert_eq!(error, ProbError::WrongHoleCardCount { provided: 3 });
    }

    #[test]
    fn rejects_too_many_board_cards() {
        let error = prepare(
            &cards("As Kh"),
            &cards("2c 3c 4c 5c 6c 7c"),
            1,
            Variant::Standard,
        )
        .unwrap_err();
        assert_eq!(error, ProbError::TooManyCommunityCards { provided: 6 });
    }

    #[test]
    fn rejects_duplicate_known_cards() {
        let error = prepare(&cards("As As"), &[], 1, Variant::Standard).unwrap_err();
        assert_eq!(error, ProbError::DuplicateKnownCard);
        let error = prepare(&cards("As Kh"), &cards("As 2c 3c"), 1, Variant::Standard).unwrap_err();
        assert_eq!(error, ProbError::DuplicateKnownCard);
    }

    #[test]
    fn clamps_opponent_count_like_js() {
        let none = prepare(&cards("As Kh"), &[], 0, Variant::Standard).unwrap();
        assert_eq!(none.opponents(), 1);
        let many = prepare(&cards("As Kh"), &[], 99, Variant::Standard).unwrap();
        assert_eq!(many.opponents(), 9);
        let short = prepare(&cards("As Kh"), &[], 99, Variant::ShortDeck).unwrap();
        assert_eq!(short.opponents(), 7);
    }

    #[test]
    fn remaining_deck_keeps_canonical_order_without_known_cards() {
        let prepared = prepare(&cards("As Kh"), &cards("2c 3c 4c"), 2, Variant::Standard).unwrap();
        let known = cards("As Kh 2c 3c 4c");
        let expected: Vec<Card> = Deck::new(Variant::Standard)
            .cards()
            .iter()
            .copied()
            .filter(|card| !known.contains(card))
            .collect();
        assert_eq!(prepared.remaining_deck(), expected.as_slice());
        assert_eq!(prepared.board().len(), 3);
        assert_eq!(prepared.missing_board_cards(), 2);
        assert_eq!(prepared.player()[0], cards("As")[0]);
    }

    #[test]
    fn short_deck_never_contains_low_ranks() {
        let prepared = prepare(&cards("As Kh"), &[], 1, Variant::ShortDeck).unwrap();
        assert_eq!(prepared.remaining_deck().len(), 34);
        assert!(
            prepared
                .remaining_deck()
                .iter()
                .all(|card| card.rank_value() >= 6)
        );
    }
}
