//! Reference (pre-table) evaluator, kept for equivalence testing and as the
//! fallback path for hands with fewer than seven cards.
//!
//! This is the original implementation: a direct 5-7 card evaluator using rank
//! histograms and per-suit rank arrays. It is intentionally straightforward and
//! mirrors js/probability-core.js line by line. The fast seven-card path in
//! `eval::fast` is validated against this implementation exhaustively (all
//! seven-card rank patterns, all flush masks) and with random differential
//! tests; the public API always goes through `eval::evaluate`.

use crate::card::Card;
use crate::variant::Variant;

use super::{EvaluatedHand, HandCategory, MAX_EVALUATED_CARDS, finish};

const KICKER_SCALE: [u64; 5] = [100_000_000, 1_000_000, 10_000, 100, 1];

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

/// Reference implementation of the best-five-from-five-to-seven-card evaluator.
///
/// Mirrors js/probability-core.js evaluateCodes() exactly, including the
/// short-deck flush/full-house ordering and wheel rules.
#[inline(always)]
pub(super) fn evaluate(cards: &[Card], variant: Variant) -> EvaluatedHand {
    debug_assert!(cards.len() <= MAX_EVALUATED_CARDS);
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
