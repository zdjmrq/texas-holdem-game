//! Public-range abstraction for the heads-up river subgame.
//!
//! Port of js/river-solver.js:5-25 (v1.5.8). The abstraction only ever sees
//! public information: five board cards, the full deck and the per-combination
//! public-range weights supplied by the caller. The hero's actual hole cards are
//! deliberately not an input here; they are only used after the solve to look up
//! the hero's private strength bucket.
//!
//! Six strength buckets are built from a weighted, score-sorted enumeration of
//! every available hole-card combination. Combinations with the same hand score
//! are never split across buckets, and exact card removal is applied to the
//! joint distribution so two private hands can never share a card.

use std::collections::BTreeMap;

use poker_core::{Card, Variant, evaluate};

use crate::AiError;

/// Number of private strength buckets (fixed at six in the original solver).
pub const BUCKETS: usize = 6;

/// One available private hole-card combination and its public-range weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Combo {
    /// The two hole cards.
    pub cards: [Card; 2],
    /// Bit mask of the two cards (fast exact card-removal check).
    pub mask: u64,
    /// Integer hand score from poker_core::evaluate for hole cards + board.
    pub score: u64,
    /// Public-range weights: [self, opponent].
    pub weights: [f64; 2],
    /// Private strength bucket assigned by the weighted score quantiles (0..=5).
    pub bucket: u8,
}

/// One joint private strength-bucket pair with positive probability mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deal {
    /// Bucket of [self, opponent] (0 = weakest, 5 = strongest).
    pub scores: [u8; 2],
    /// Probability of this bucket pair after exact card removal.
    pub weight: f64,
    /// Conditional average showdown sign (hero score - villain score).
    pub showdown_sign: f64,
}

/// Result of the public-range abstraction (ranges() in the original solver).
#[derive(Clone, Debug, PartialEq)]
pub struct RangeAbstraction {
    /// Joint bucket-pair distribution in ascending cell-index order.
    pub deals: Vec<Deal>,
    /// Hand score -> private strength bucket.
    pub bucket_by_score: BTreeMap<u64, u8>,
    /// Number of available private combinations (C(available, 2)).
    pub combinations: usize,
    /// Every available combination in deck order (kept for diagnostics).
    pub combos: Vec<Combo>,
}

/// Builds the six-bucket public-range abstraction.
///
/// The board must contain five distinct cards. The deck is the full deck in the
/// original JS order; the board is removed from it before the combinations are
/// enumerated. Weights holds one (self, opponent) pair per available
/// combination, in combination order (i outer, j inner over the remaining deck).
///
/// # Errors
///
/// Returns AiError::RiverInvalid for a malformed board, an invalid or
/// malformed weight list, an empty range, or when no compatible private deal
/// exists.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub fn ranges(
    board: &[Card; 5],
    deck: &[Card],
    weights: &[(f64, f64)],
) -> Result<RangeAbstraction, AiError> {
    let mut board_mask = 0u64;
    for card in board {
        board_mask |= card.bit();
    }
    if board_mask.count_ones() != 5 {
        return Err(AiError::RiverInvalid("five distinct public cards required"));
    }

    let available: Vec<Card> = deck
        .iter()
        .copied()
        .filter(|card| board_mask & card.bit() == 0)
        .collect();
    let expected = available
        .len()
        .checked_mul(available.len().saturating_sub(1))
        .map_or(0, |n| n / 2);
    if weights.len() != expected {
        return Err(AiError::RiverInvalid("public range weight count mismatch"));
    }

    let mut combos: Vec<Combo> = Vec::with_capacity(expected);
    let mut weight_index = 0usize;
    for (index, &first) in available.iter().enumerate() {
        for &second in &available[(index + 1)..] {
            let cards = [first, second];
            let (self_weight, opponent_weight) = weights[weight_index];
            weight_index += 1;
            if !self_weight.is_finite()
                || !opponent_weight.is_finite()
                || self_weight < 0.0
                || opponent_weight < 0.0
            {
                return Err(AiError::RiverInvalid("invalid public range"));
            }
            let mut seven = [Card::new(poker_core::Suit::Spades, poker_core::Rank::Two); 7];
            seven[0] = cards[0];
            seven[1] = cards[1];
            seven[2..].copy_from_slice(board);
            let score = evaluate(&seven, Variant::Standard).score();
            combos.push(Combo {
                cards,
                mask: cards[0].bit() | cards[1].bit(),
                score,
                weights: [self_weight, opponent_weight],
                bucket: 0,
            });
        }
    }

    // Weighted score quantiles. The original JS sorts a copy and keeps groups of
    // equal score in the same bucket, so no combination is ever split.
    let mut sorted: Vec<usize> = (0..combos.len()).collect();
    sorted.sort_by_key(|&index| combos[index].score);
    let mut mass = 0.0f64;
    for &index in &sorted {
        mass += combos[index].weights[0];
        mass += combos[index].weights[1];
    }
    if mass <= 0.0 || mass.is_nan() {
        return Err(AiError::RiverInvalid("empty range"));
    }
    let mut bucket_by_score: BTreeMap<u64, u8> = BTreeMap::new();
    let mut before = 0.0f64;
    let mut i = 0usize;
    while i < sorted.len() {
        let score = combos[sorted[i]].score;
        let mut j = i + 1;
        let mut group = combos[sorted[i]].weights[0] + combos[sorted[i]].weights[1];
        while j < sorted.len() && combos[sorted[j]].score == score {
            group += combos[sorted[j]].weights[0] + combos[sorted[j]].weights[1];
            j += 1;
        }
        let quantile = (before + group / 2.0) / mass * BUCKETS as f64;
        let bucket = quantile.floor().min((BUCKETS - 1) as f64) as u8;
        for &index in &sorted[i..j] {
            combos[index].bucket = bucket;
        }
        bucket_by_score.insert(score, bucket);
        before += group;
        i = j;
    }

    // Exact card removal on the joint public ranges. Do not remove the actual
    // hero's cards: that would leak them into the opponent's policy.
    let mut cells = [0.0f64; BUCKETS * BUCKETS];
    let mut signed = [0.0f64; BUCKETS * BUCKETS];
    let mut total = 0.0f64;
    for a in &combos {
        for b in &combos {
            if a.mask & b.mask != 0 {
                continue;
            }
            let weight = a.weights[0] * b.weights[1];
            let cell = usize::from(a.bucket) * BUCKETS + usize::from(b.bucket);
            cells[cell] += weight;
            let sign = match a.score.cmp(&b.score) {
                core::cmp::Ordering::Greater => 1.0,
                core::cmp::Ordering::Less => -1.0,
                core::cmp::Ordering::Equal => 0.0,
            };
            signed[cell] += weight * sign;
            total += weight;
        }
    }
    if total <= 0.0 || total.is_nan() {
        return Err(AiError::RiverInvalid("no compatible private deals"));
    }
    let mut deals = Vec::with_capacity(cells.len());
    for (index, &cell_weight) in cells.iter().enumerate() {
        if cell_weight > 0.0 {
            deals.push(Deal {
                scores: [(index / BUCKETS) as u8, (index % BUCKETS) as u8],
                weight: cell_weight / total,
                showdown_sign: signed[index] / cell_weight,
            });
        }
    }

    Ok(RangeAbstraction {
        deals,
        bucket_by_score,
        combinations: combos.len(),
        combos,
    })
}
