//! Standard and short-deck hand evaluation.
//!
//! # Score contract
//!
//! The original JS project compares hands by an integer score:
//!
//! score = rankIndex * 10^10 + v0 * 10^8 + v1 * 10^6 + v2 * 10^4 + v3 * 10^2 + v4
//!
//! where rankIndex is the variant-specific hand-type index (short deck swaps
//! flush and full house) and v0..v4 are the tie-break card values, highest
//! first. Every term fits exactly in an f64 (values stay below 2^53), so the
//! Rust and JS implementations can be compared bit-for-bit. This module returns
//! the identical rank index and score as js/probability-core.js evaluate(),
//! which was verified exhaustively for all five-card combinations in both
//! variants.
//!
//! Hands with fewer than five cards use the same partial-hand rules as
//! probability-core (made trips/quads/pairs plus ordered kickers; no suited
//! bonus, unlike the older js/cards.js display helper).
//!
//! # Implementation
//!
//! Seven-card hands use the table-driven [`fast`] path (rank histogram lookup
//! plus flush-mask tables). Every other size (including partial hands) uses the
//! straightforward [`reference`] implementation. Both paths share the same
//! score assembly, and the fast path is validated exhaustively against the
//! reference implementation: every one of the 49,205 5/6/7-card rank patterns
//! and every one of the 8,192 flush masks is compared field by field, plus a
//! 100,000-hand-per-variant random differential test.

mod fast;
mod reference;
mod tables;

use crate::card::Card;
use crate::variant::Variant;

/// Maximum number of cards accepted by [`evaluate`] (two hole cards plus five
/// board cards).
pub const MAX_EVALUATED_CARDS: usize = 7;

const RANK_SCALE: u64 = 10_000_000_000;
const KICKER_SCALE: [u64; 5] = [100_000_000, 1_000_000, 10_000, 100, 1];

/// Semantic hand category, independent of the variant's internal rank index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandCategory {
    /// High card.
    HighCard = 0,
    /// One pair.
    Pair = 1,
    /// Two pair.
    TwoPair = 2,
    /// Three of a kind.
    ThreeOfAKind = 3,
    /// Straight.
    Straight = 4,
    /// Flush (rank index differs between variants).
    Flush = 5,
    /// Full house (rank index differs between variants).
    FullHouse = 6,
    /// Four of a kind.
    FourOfAKind = 7,
    /// Straight flush (non-royal).
    StraightFlush = 8,
    /// Royal flush.
    RoyalFlush = 9,
}

impl HandCategory {
    /// All categories in standard-deck strength order.
    pub const ALL: [HandCategory; 10] = [
        HandCategory::HighCard,
        HandCategory::Pair,
        HandCategory::TwoPair,
        HandCategory::ThreeOfAKind,
        HandCategory::Straight,
        HandCategory::Flush,
        HandCategory::FullHouse,
        HandCategory::FourOfAKind,
        HandCategory::StraightFlush,
        HandCategory::RoyalFlush,
    ];
}

/// The result of evaluating a hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EvaluatedHand {
    rank_index: u8,
    score: u64,
    category: HandCategory,
    name: &'static str,
}

impl EvaluatedHand {
    /// Variant-specific hand-type index, matching the original JS rank field.
    #[must_use]
    pub const fn rank_index(self) -> u8 {
        self.rank_index
    }

    /// Integer comparison score, identical to the original JS score field.
    #[must_use]
    pub const fn score(self) -> u64 {
        self.score
    }

    /// Semantic hand category.
    #[must_use]
    pub const fn category(self) -> HandCategory {
        self.category
    }

    /// Chinese display name, for example "同花" or "皇家同花顺".
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// True when this hand ranks strictly higher; only meaningful within one variant.
    #[must_use]
    pub const fn beats(self, other: EvaluatedHand) -> bool {
        self.score > other.score
    }

    /// Compares two hands of the same variant: 1 when self wins, -1 when other
    /// wins, 0 on a tie. Matches the original JS compareHands helpers.
    #[must_use]
    pub const fn compare(self, other: EvaluatedHand) -> i8 {
        if self.score > other.score {
            1
        } else if self.score < other.score {
            -1
        } else {
            0
        }
    }
}

impl PartialOrd for EvaluatedHand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EvaluatedHand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.score.cmp(&other.score)
    }
}

fn hand_rank_index(variant: Variant, category: HandCategory) -> u8 {
    match (variant, category) {
        (Variant::ShortDeck, HandCategory::Flush) => 6,
        (Variant::ShortDeck, HandCategory::FullHouse) => 5,
        _ => category as u8,
    }
}

fn finish(variant: Variant, category: HandCategory, values: &[u8]) -> EvaluatedHand {
    let rank_index = hand_rank_index(variant, category);
    let mut score = u64::from(rank_index) * RANK_SCALE;
    for (index, value) in values.iter().take(5).enumerate() {
        score += u64::from(*value) * KICKER_SCALE[index];
    }
    EvaluatedHand {
        rank_index,
        score,
        category,
        name: variant.hand_type_names()[usize::from(rank_index)],
    }
}

/// Evaluates the best five-card hand from at most seven cards.
///
/// Complete 5/6/7-card hands use the table-driven fast path; partial hands (including
/// partial 2/3/4-card hands) use the reference implementation. Both produce
/// bit-identical `rank_index`, `score` and `name`.
///
/// # Panics
///
/// Panics when more than [`MAX_EVALUATED_CARDS`] cards are supplied. Callers in
/// this code base only ever pass two hole cards plus at most five board cards.
#[must_use]
#[inline]
pub fn evaluate(cards: &[Card], variant: Variant) -> EvaluatedHand {
    assert!(
        cards.len() <= MAX_EVALUATED_CARDS,
        "evaluator supports at most {MAX_EVALUATED_CARDS} cards, got {}",
        cards.len()
    );
    if cards.len() >= 5 {
        fast::evaluate_complete(cards, variant)
    } else {
        reference::evaluate(cards, variant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, Rank, Suit, parse_card};
    use crate::deck::Deck;
    use crate::rng::XorShift32Rng;

    fn hand(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| parse_card(token).expect("card"))
            .collect()
    }

    fn rank_of(cards: &str, variant: Variant) -> (u8, u64, &'static str) {
        let evaluated = evaluate(&hand(cards), variant);
        (evaluated.rank_index(), evaluated.score(), evaluated.name())
    }

    fn assert_fast_matches_reference(cards: &[Card], variant: Variant) {
        let fast = evaluate(cards, variant);
        let reference_evaluated = reference::evaluate(cards, variant);
        assert_eq!(
            fast, reference_evaluated,
            "fast/reference mismatch for {cards:?} ({variant:?})"
        );
    }

    /// Builds a concrete (duplicate-free, flush-free) seven-card hand from a
    /// rank histogram. Cycling suits over the cards keeps every suit count at
    /// most two, so no flush interferes with the rank-pattern table.
    fn synthetic_hand_from_counts(counts: &[u8; tables::RANK_COUNT]) -> Vec<Card> {
        let mut hand = Vec::with_capacity(MAX_EVALUATED_CARDS);
        let mut suit_cursor = 0usize;
        for (offset, &count) in counts.iter().enumerate() {
            let rank = Rank::from_value(offset as u8 + 2).expect("rank");
            for _ in 0..count {
                hand.push(Card::new(Suit::ALL[suit_cursor % 4], rank));
                suit_cursor += 1;
            }
        }
        assert!((5..=MAX_EVALUATED_CARDS).contains(&hand.len()));
        hand
    }

    fn counts_from_key(key: u64) -> [u8; tables::RANK_COUNT] {
        let mut counts = [0u8; tables::RANK_COUNT];
        for (offset, slot) in counts.iter_mut().enumerate() {
            *slot = ((key >> (offset * 4)) & 0xF) as u8;
        }
        counts
    }

    /// Builds a duplicate-free seven-card hand whose spade mask equals `mask`.
    fn flush_hand_from_mask(mask: u16) -> Vec<Card> {
        let mut hand = Vec::with_capacity(MAX_EVALUATED_CARDS);
        for bit in 0..13u8 {
            if mask & (1u16 << bit) != 0 {
                let rank = Rank::from_value(bit + 2).expect("rank");
                hand.push(Card::new(Suit::Spades, rank));
            }
        }
        let fillers = [
            (Suit::Hearts, Rank::Ace),
            (Suit::Diamonds, Rank::King),
            (Suit::Clubs, Rank::Queen),
        ];
        for &(suit, rank) in fillers.iter().take(MAX_EVALUATED_CARDS - hand.len()) {
            hand.push(Card::new(suit, rank));
        }
        assert_eq!(hand.len(), MAX_EVALUATED_CARDS);
        hand
    }

    #[test]
    fn standard_hand_categories_and_scores() {
        let royal = rank_of("As Ks Qs Js Ts", Variant::Standard);
        assert_eq!(royal.0, 9);
        assert_eq!(royal.1, 90_000_000_000);
        assert_eq!(royal.2, "皇家同花顺");

        let straight_flush = rank_of("9h 8h 7h 6h 5h", Variant::Standard);
        assert_eq!(straight_flush.0, 8);
        assert_eq!(straight_flush.1, 80_000_000_000 + 9 * 100_000_000);

        let quads = rank_of("As Ah Ad Ac Kd", Variant::Standard);
        assert_eq!(quads.0, 7);
        assert_eq!(quads.1, 70_000_000_000 + 14 * 100_000_000 + 13 * 1_000_000);

        let full_house = rank_of("Ks Kh Kd Qs Qh", Variant::Standard);
        assert_eq!(full_house.0, 6);
        assert_eq!(
            full_house.1,
            60_000_000_000 + 13 * 100_000_000 + 12 * 1_000_000
        );

        let flush = rank_of("As Qs Ts 8s 6s", Variant::Standard);
        assert_eq!(flush.0, 5);
        assert_eq!(
            flush.1,
            50_000_000_000 + 14 * 100_000_000 + 12 * 1_000_000 + 10 * 10_000 + 8 * 100 + 6
        );

        let straight = rank_of("9h 8d 7c 6s 5h", Variant::Standard);
        assert_eq!(straight.0, 4);
        assert_eq!(straight.1, 40_000_000_000 + 9 * 100_000_000);

        let trips = rank_of("As Ah Ad Kd Qc", Variant::Standard);
        assert_eq!(trips.0, 3);
        assert_eq!(
            trips.1,
            30_000_000_000 + 14 * 100_000_000 + 13 * 1_000_000 + 12 * 10_000
        );

        let two_pair = rank_of("As Ah Kd Kc Qc", Variant::Standard);
        assert_eq!(two_pair.0, 2);
        assert_eq!(
            two_pair.1,
            20_000_000_000 + 14 * 100_000_000 + 13 * 1_000_000 + 12 * 10_000
        );

        let pair = rank_of("As Ah Kd Qc Jc", Variant::Standard);
        assert_eq!(pair.0, 1);
        assert_eq!(
            pair.1,
            10_000_000_000 + 14 * 100_000_000 + 13 * 1_000_000 + 12 * 10_000 + 11 * 100
        );

        let high = rank_of("As Kh Qd Jc 9c", Variant::Standard);
        assert_eq!(high.0, 0);
        assert_eq!(
            high.1,
            14 * 100_000_000 + 13 * 1_000_000 + 12 * 10_000 + 11 * 100 + 9
        );
    }

    #[test]
    fn standard_wheel_is_five_high_straight() {
        let wheel = rank_of("As 2h 3d 4c 5h", Variant::Standard);
        assert_eq!(wheel.0, 4);
        assert_eq!(wheel.1, 40_000_000_000 + 5 * 100_000_000);
        // A-6-7-8-9 is not a straight in standard.
        assert_eq!(rank_of("As 6h 7d 8c 9h", Variant::Standard).0, 0);
    }

    #[test]
    fn short_deck_flips_flush_and_full_house() {
        let flush = rank_of("As Js 9s 7s 6s", Variant::ShortDeck);
        let full_house = rank_of("Ks Kh Kd Qs Qh", Variant::ShortDeck);
        assert_eq!(flush.0, 6);
        assert_eq!(full_house.0, 5);
        assert!(
            evaluate(&hand("As Js 9s 7s 6s"), Variant::ShortDeck)
                .beats(evaluate(&hand("Ks Kh Kd Qs Qh"), Variant::ShortDeck))
        );
        assert!(
            evaluate(&hand("Ks Kh Kd Qs Qh"), Variant::Standard)
                .beats(evaluate(&hand("As Js 9s 7s 6s"), Variant::Standard))
        );
    }

    #[test]
    fn short_deck_wheel_and_straights() {
        let wheel = rank_of("As 6h 7d 8c 9h", Variant::ShortDeck);
        assert_eq!(wheel.0, 4);
        assert_eq!(wheel.1, 40_000_000_000 + 9 * 100_000_000);
        // Ten-high straight.
        assert_eq!(
            rank_of("6s 7h 8d 9c Th", Variant::ShortDeck).1,
            40_000_000_000 + 10 * 100_000_000
        );
        // A-2-3-4-5 is out of the short deck's rank range, so the shortest
        // wheel is the A-6-7-8-9 one checked above.
        assert_eq!(rank_of("As 6h 7d 8c 9h", Variant::ShortDeck).2, "顺子");
    }

    #[test]
    fn royals_and_straight_flushes_are_identified_in_both_variants() {
        assert_eq!(rank_of("Ts Js Qs Ks As", Variant::ShortDeck).0, 9);
        assert_eq!(rank_of("Th 9h 8h 7h 6h", Variant::ShortDeck).0, 8);
        assert_eq!(rank_of("9h 8h 7h 6h 5h", Variant::Standard).0, 8);
    }

    #[test]
    fn seven_card_hands_pick_the_best_five() {
        // Board pairs the board but the player has quads: pick quads.
        let evaluated = evaluate(&hand("As Ah Ad Ac 2d Kd Qd"), Variant::Standard);
        assert_eq!(evaluated.category(), HandCategory::FourOfAKind);
        // Two pair on board, flush in hand.
        let evaluated = evaluate(&hand("As Qs 9s 5s 2s Kd Kh"), Variant::Standard);
        assert_eq!(evaluated.category(), HandCategory::Flush);
        // Quads plus pair still quads.
        let evaluated = evaluate(&hand("As Ah Ad Ac Ks Kd 2c"), Variant::Standard);
        assert_eq!(evaluated.category(), HandCategory::FourOfAKind);
    }

    #[test]
    fn partial_hands_match_probability_core_semantics() {
        let pocket_pair = evaluate(&hand("As Ah"), Variant::Standard);
        assert_eq!(pocket_pair.rank_index(), 1);
        assert_eq!(pocket_pair.score(), 10_000_000_000 + 14 * 100_000_000);
        // probability-core has no suited preflop bonus.
        let suited = evaluate(&hand("As Ks"), Variant::Standard);
        let offsuit = evaluate(&hand("As Kh"), Variant::Standard);
        assert_eq!(suited.score(), offsuit.score());
        let empty: Vec<Card> = Vec::new();
        assert_eq!(evaluate(&empty, Variant::Standard).score(), 0);
    }

    #[test]
    fn tie_scores_compare_equal() {
        let first = evaluate(&hand("As Kh Qd Jc 9c"), Variant::Standard);
        let second = evaluate(&hand("Ah Ks Qc Jd 9d"), Variant::Standard);
        assert_eq!(first.score(), second.score());
        assert_eq!(first.compare(second), 0);
    }

    #[test]
    fn fast_path_matches_reference_for_every_rank_pattern() {
        let keys = tables::pattern_keys();
        assert_eq!(keys.len(), 73_775, "seven-card rank patterns");
        for key in keys {
            let cards = synthetic_hand_from_counts(&counts_from_key(key));
            for variant in Variant::ALL {
                assert_fast_matches_reference(&cards, variant);
            }
        }
    }

    #[test]
    fn fast_path_matches_reference_for_every_flush_mask() {
        let mut tested = 0usize;
        for mask in 0..tables::MASK_COUNT {
            let mask = mask as u16;
            let flush_cards = mask.count_ones();
            // A seven-card hand can only have 5, 6 or 7 cards in its flush suit.
            if !(5..=MAX_EVALUATED_CARDS as u32).contains(&flush_cards) {
                continue;
            }
            tested += 1;
            let cards = flush_hand_from_mask(mask);
            for variant in Variant::ALL {
                assert_fast_matches_reference(&cards, variant);
            }
        }
        // C(13,5) + C(13,6) + C(13,7) = 1287 + 1716 + 1716.
        assert_eq!(tested, 4_719);
    }

    #[test]
    fn fast_path_matches_reference_on_random_hands() {
        let mut rng = XorShift32Rng::from_seed("fast-eval-differential");
        for variant in Variant::ALL {
            let canonical = Deck::new(variant);
            for sample in 0..100_000usize {
                let mut deck = canonical.clone();
                deck.shuffle(&mut rng);
                let card_count = 5 + sample % 3;
                let cards = &deck.cards()[..card_count];
                assert_fast_matches_reference(cards, variant);
            }
        }
    }

    #[test]
    fn fast_path_matches_reference_on_short_deck_special_hands() {
        let cases = [
            ("As Js 9s 7s 6s Ks Kh", Variant::ShortDeck),
            ("As 6h 7d 8c 9h Kd Qd", Variant::ShortDeck),
            ("As 6s 7s 8s 9s Kd Qd", Variant::ShortDeck),
            ("Ts Js Qs Ks As 9d 8c", Variant::ShortDeck),
            ("As Ad Ah Ks Kd Kc Qd", Variant::ShortDeck),
            ("As Ad Ah Ac Ks Kd Qd", Variant::ShortDeck),
            ("As Ks Qs Js 9s Kd Qd", Variant::ShortDeck),
            ("As Ad Ah Ks Kd Kc Qd", Variant::Standard),
            ("As Ad Ah Ac Ks Kd Qd", Variant::Standard),
            ("As 2h 3d 4c 5h Kd Qd", Variant::Standard),
            ("As 6h 7d 8c 9h Kd Qd", Variant::Standard),
        ];
        for (text, variant) in cases {
            assert_fast_matches_reference(&hand(text), variant);
        }
    }
}
