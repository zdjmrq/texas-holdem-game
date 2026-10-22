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

/// Returns the high card of the best straight in the rank histogram, or 0 when
/// there is none. Mirrors js/probability-core.js straightHigh().
fn straight_high(counts: &[u8; 15], variant: Variant) -> u8 {
    let lowest_high: u8 = if variant.is_short_deck() { 10 } else { 6 };
    let mut high = 14u8;
    while high >= lowest_high {
        if (0..5).all(|offset| counts[usize::from(high - offset)] > 0) {
            return high;
        }
        high -= 1;
    }
    if variant.is_short_deck() {
        // Short-deck wheel: A-6-7-8-9, nine high.
        if counts[14] > 0 && counts[9] > 0 && counts[8] > 0 && counts[7] > 0 && counts[6] > 0 {
            return 9;
        }
    } else if counts[14] > 0 && counts[5] > 0 && counts[4] > 0 && counts[3] > 0 && counts[2] > 0 {
        // Standard wheel: A-2-3-4-5, five high.
        return 5;
    }
    0
}

fn evaluate_partial(cards: &[Card], variant: Variant) -> EvaluatedHand {
    let mut counts = [0u8; 15];
    for card in cards {
        counts[usize::from(card.rank_value())] += 1;
    }
    let mut groups: Vec<(u8, u8)> = Vec::with_capacity(4);
    for value in (2..=14u8).rev() {
        let count = counts[usize::from(value)];
        if count > 0 {
            groups.push((value, count));
        }
    }
    groups.sort_by(|left, right| right.1.cmp(&left.1).then(right.0.cmp(&left.0)));
    let Some(&(top_value, top_count)) = groups.first() else {
        return finish(variant, HandCategory::HighCard, &[]);
    };
    let mut values = [0u8; 5];
    let mut len = 0usize;
    let push = |value: u8, values: &mut [u8; 5], len: &mut usize| {
        if *len < values.len() {
            values[*len] = value;
            *len += 1;
        }
    };
    match top_count {
        4 => {
            push(top_value, &mut values, &mut len);
            for group in &groups[1..] {
                push(group.0, &mut values, &mut len);
            }
            finish(variant, HandCategory::FourOfAKind, &values[..len])
        }
        3 => {
            if groups.get(1).is_some_and(|group| group.1 >= 2) {
                finish(variant, HandCategory::FullHouse, &[top_value, groups[1].0])
            } else {
                push(top_value, &mut values, &mut len);
                for group in &groups[1..] {
                    push(group.0, &mut values, &mut len);
                }
                finish(variant, HandCategory::ThreeOfAKind, &values[..len])
            }
        }
        2 => {
            if groups.get(1).is_some_and(|group| group.1 == 2) {
                push(groups[0].0, &mut values, &mut len);
                push(groups[1].0, &mut values, &mut len);
                for group in &groups[2..] {
                    push(group.0, &mut values, &mut len);
                }
                finish(variant, HandCategory::TwoPair, &values[..len])
            } else {
                push(top_value, &mut values, &mut len);
                for group in &groups[1..] {
                    push(group.0, &mut values, &mut len);
                }
                finish(variant, HandCategory::Pair, &values[..len])
            }
        }
        _ => {
            for group in &groups {
                push(group.0, &mut values, &mut len);
            }
            finish(variant, HandCategory::HighCard, &values[..len])
        }
    }
}

/// Evaluates the best five-card hand from at most seven cards.
///
/// # Panics
///
/// Panics when more than [`MAX_EVALUATED_CARDS`] cards are supplied. Callers in
/// this code base only ever pass two hole cards plus at most five board cards.
#[must_use]
pub fn evaluate(cards: &[Card], variant: Variant) -> EvaluatedHand {
    assert!(
        cards.len() <= MAX_EVALUATED_CARDS,
        "evaluator supports at most {MAX_EVALUATED_CARDS} cards, got {}",
        cards.len()
    );
    if cards.len() < 5 {
        return evaluate_partial(cards, variant);
    }

    let mut counts = [0u8; 15];
    let mut suit_values = [[0u8; MAX_EVALUATED_CARDS]; 4];
    let mut suit_lens = [0usize; 4];
    for card in cards {
        let value = card.rank_value();
        counts[usize::from(value)] += 1;
        let suit = card.suit() as usize;
        suit_values[suit][suit_lens[suit]] = value;
        suit_lens[suit] += 1;
    }

    // Straight flush and flush detection, including the five-card flush top.
    let mut flush_values: Option<[u8; 5]> = None;
    let mut flush_top_score = 0u64;
    for (&length, values) in suit_lens.iter().zip(suit_values.iter()) {
        if length < 5 {
            continue;
        }
        let mut sorted = *values;
        sorted[..length].sort_unstable_by(|left, right| right.cmp(left));
        let mut flush_counts = [0u8; 15];
        for &value in &sorted[..length] {
            flush_counts[usize::from(value)] += 1;
        }
        let high = straight_high(&flush_counts, variant);
        if high != 0 {
            let category = if high == 14 {
                HandCategory::RoyalFlush
            } else {
                HandCategory::StraightFlush
            };
            return if high == 14 {
                finish(variant, category, &[])
            } else {
                finish(variant, category, &[high])
            };
        }
        let top = [sorted[0], sorted[1], sorted[2], sorted[3], sorted[4]];
        let top_score = top
            .iter()
            .enumerate()
            .map(|(index, &value)| u64::from(value) * KICKER_SCALE[index])
            .sum();
        if flush_values.is_none() || top_score > flush_top_score {
            flush_values = Some(top);
            flush_top_score = top_score;
        }
    }

    // Rank multiplicities in descending rank order.
    let mut ordered = [0u8; MAX_EVALUATED_CARDS];
    let mut ordered_len = 0usize;
    let mut trips = [0u8; 3];
    let mut trips_len = 0usize;
    let mut pairs = [0u8; 4];
    let mut pairs_len = 0usize;
    let mut quads = 0u8;
    for value in (2..=14u8).rev() {
        let count = counts[usize::from(value)];
        if count == 0 {
            continue;
        }
        ordered[ordered_len] = value;
        ordered_len += 1;
        if count == 4 {
            quads = value;
        }
        if count >= 3 {
            trips[trips_len] = value;
            trips_len += 1;
        }
        if count >= 2 {
            pairs[pairs_len] = value;
            pairs_len += 1;
        }
    }

    if quads != 0 {
        let kicker = ordered[..ordered_len]
            .iter()
            .copied()
            .find(|&value| value != quads)
            .unwrap_or(0);
        return finish(variant, HandCategory::FourOfAKind, &[quads, kicker]);
    }

    let full_house = (|| {
        let trip = *trips[..trips_len].first()?;
        let pair = pairs[..pairs_len]
            .iter()
            .copied()
            .find(|&value| value != trip)?;
        Some((trip, pair))
    })();

    // Short deck ranks flush above full house; the original JS evaluator checks
    // them in that order too.
    if variant.is_short_deck() {
        if let Some(values) = flush_values {
            return finish(variant, HandCategory::Flush, &values);
        }
        if let Some((trip, pair)) = full_house {
            return finish(variant, HandCategory::FullHouse, &[trip, pair]);
        }
    } else {
        if let Some((trip, pair)) = full_house {
            return finish(variant, HandCategory::FullHouse, &[trip, pair]);
        }
        if let Some(values) = flush_values {
            return finish(variant, HandCategory::Flush, &values);
        }
    }

    let high = straight_high(&counts, variant);
    if high != 0 {
        return finish(variant, HandCategory::Straight, &[high]);
    }

    if trips_len > 0 {
        let trip = trips[0];
        let mut values = [trip, 0, 0];
        let mut len = 1usize;
        for &value in &ordered[..ordered_len] {
            if value != trip {
                values[len] = value;
                len += 1;
                if len == 3 {
                    break;
                }
            }
        }
        return finish(variant, HandCategory::ThreeOfAKind, &values[..len]);
    }

    if pairs_len >= 2 {
        let first = pairs[0];
        let second = pairs[1];
        let kicker = ordered[..ordered_len]
            .iter()
            .copied()
            .find(|&value| value != first && value != second)
            .unwrap_or(0);
        return finish(variant, HandCategory::TwoPair, &[first, second, kicker]);
    }

    if pairs_len == 1 {
        let pair = pairs[0];
        let mut values = [pair, 0, 0, 0, 0];
        let mut len = 1usize;
        for &value in &ordered[..ordered_len] {
            if value != pair {
                values[len] = value;
                len += 1;
                // The original JS pair branch keeps exactly three kickers.
                if len == 4 {
                    break;
                }
            }
        }
        return finish(variant, HandCategory::Pair, &values[..len]);
    }

    let top_five = [ordered[0], ordered[1], ordered[2], ordered[3], ordered[4]];
    finish(variant, HandCategory::HighCard, &top_five)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Card, parse_card};

    fn hand(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| parse_card(token).expect("card"))
            .collect()
    }

    fn rank_of(cards: &str, variant: Variant) -> (u8, u64, &'static str) {
        let evaluated = evaluate(&hand(cards), variant);
        (evaluated.rank_index(), evaluated.score(), evaluated.name())
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
}
