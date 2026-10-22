//! Range-weighted equity estimation (port of js/ai-core.js
//! estimateRangeEquity, docs/specs/ai-strategy.md section 6.5).

use std::collections::{HashMap, HashSet};

use poker_core::{Card, Variant, evaluate};

use crate::belief::OpponentBelief;
use crate::input::DecisionContext;
use crate::math;
use crate::rng::AiRng;

/// Fixed sample schedule (js/ai-core.js:738-749).
pub const EMERGENCY_GRACE: f64 = 3.0;
/// River bonus samples.
pub const RIVER_BONUS: usize = 40;
/// Weighted pick retry limit (js/ai-core.js:651).
pub const WEIGHTED_PICK_ATTEMPTS: usize = 22;
/// Guard for random board completion.
pub const BOARD_GUARD: usize = 30;

/// Per-opponent sample relation: -1 lost, 0 tie, 1 won (relative to hero).
pub type Relation = i8;

/// One weighted opponent combination.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightedCombo {
    /// First card.
    pub c1: Card,
    /// Second card.
    pub c2: Card,
    /// Combination weight.
    pub weight: f64,
    /// Running cumulative weight (binary search friendly).
    pub cumulative: f64,
}

/// Weighted combination list for one seat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampler {
    /// Seat id.
    pub seat: usize,
    /// Combinations in deck order.
    pub combos: Vec<WeightedCombo>,
    /// Sum of weights.
    pub total_weight: f64,
}

/// Sampled response information collected during equity estimation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResponseSamples {
    /// Seat ids aligned with the samplers.
    pub seats: Vec<usize>,
    /// Per-sample per-seat relation values.
    pub relations: Vec<Vec<Relation>>,
    /// Per-sample per-seat pre-flop starting strengths (pre-flop only).
    pub strengths: Vec<Vec<f64>>,
    /// Per-sample per-seat board selection scores (post-flop only).
    pub selection_scores: Vec<Vec<f64>>,
    /// Per-sample statistical weights (exact river path only).
    pub weights: Vec<f64>,
}

/// Result of a range-equity estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeEquity {
    /// Mean hero equity share.
    pub mean: f64,
    /// Standard deviation of the sample.
    pub std_dev: f64,
    /// Standard error of the mean (0 for exact enumeration).
    pub standard_error: f64,
    /// Mean confidence of the opponent beliefs.
    pub range_confidence: f64,
    /// Equity per pot-contest group key (seat list joined by comma).
    pub subset_equities: Vec<(String, f64)>,
    /// Sample response information.
    pub response_samples: Option<ResponseSamples>,
    /// Number of samples (990 for the exact river heads-up enumeration).
    pub samples: usize,
    /// Confidence derived from the sample count.
    pub confidence: f64,
    /// Exact enumeration was used.
    pub exact: bool,
    /// Wall-clock safety brake fired (always false: Rust runs fixed samples).
    pub timed_out: bool,
    /// Sampler stalled with zero samples.
    pub stalled: bool,
}

impl Default for RangeEquity {
    fn default() -> Self {
        Self {
            mean: 0.5,
            std_dev: 0.22,
            standard_error: 0.0,
            range_confidence: 0.12,
            subset_equities: Vec::new(),
            response_samples: None,
            samples: 0,
            confidence: 0.05,
            exact: false,
            timed_out: false,
            stalled: false,
        }
    }
}

/// Default fixed sample count for a given opponent count.
#[must_use]
pub fn baseline_samples(num_opponents: usize, board_len: usize) -> usize {
    let baseline = if num_opponents >= 4 {
        240
    } else if num_opponents >= 2 {
        320
    } else {
        420
    };
    baseline + if board_len >= 5 { RIVER_BONUS } else { 0 }
}

fn card_index(card: Card) -> u64 {
    1u64 << card.index()
}

fn hand_score(cards: &[Card], variant: Variant) -> u64 {
    evaluate(cards, variant).score()
}

fn build_sampler(
    deck: &[Card],
    seat: usize,
    belief: Option<&OpponentBelief>,
    model: Option<&crate::model::BehavioralModel>,
    variant: Variant,
    feature_cache: &mut HashMap<String, math::BoardComboFeatures>,
) -> Sampler {
    let mut combos = Vec::with_capacity(deck.len() * (deck.len() - 1) / 2);
    let mut total_weight = 0.0;
    for i in 0..deck.len() {
        for j in (i + 1)..deck.len() {
            let weight = belief.map_or(1.0, |belief| {
                belief.combo_weight(deck[i], deck[j], variant, model, feature_cache)
            });
            if weight <= 0.0 {
                continue;
            }
            total_weight += weight;
            combos.push(WeightedCombo {
                c1: deck[i],
                c2: deck[j],
                weight,
                cumulative: total_weight,
            });
        }
    }
    Sampler {
        seat,
        combos,
        total_weight: total_weight.max(1.0),
    }
}

fn weighted_pick(sampler: &Sampler, rng: &mut AiRng, used: u64) -> Option<WeightedCombo> {
    if sampler.combos.is_empty() {
        return None;
    }
    for _ in 0..WEIGHTED_PICK_ATTEMPTS {
        let target = rng.next_f64() * sampler.total_weight;
        let mut low = 0usize;
        let mut high = sampler.combos.len() - 1;
        while low < high {
            let mid = (low + high) / 2;
            if sampler.combos[mid].cumulative < target {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let combo = sampler.combos[low];
        if used & card_index(combo.c1) == 0 && used & card_index(combo.c2) == 0 {
            return Some(combo);
        }
    }
    let mut total = 0.0;
    for combo in &sampler.combos {
        if used & card_index(combo.c1) == 0 && used & card_index(combo.c2) == 0 {
            total += combo.weight;
        }
    }
    let mut roll = rng.next_f64() * total;
    for combo in &sampler.combos {
        if used & card_index(combo.c1) != 0 || used & card_index(combo.c2) != 0 {
            continue;
        }
        roll -= combo.weight;
        if roll <= 0.0 {
            return Some(*combo);
        }
    }
    None
}

/// Pot-contest groups: opponent seat lists that can still compete for one pot
/// layer (port of js/ai-core.js potContestGroups).
#[must_use]
pub fn pot_contest_groups(context: &DecisionContext) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for player in &context.public_players {
        for level in [player.committed, player.committed + 1.0] {
            let mut seats: Vec<usize> = context
                .active_opponent_seats
                .iter()
                .copied()
                .filter(|seat| {
                    context
                        .public_players
                        .iter()
                        .find(|candidate| candidate.seat == *seat)
                        .is_none_or(|candidate| {
                            !candidate.folded && candidate.committed + candidate.stack >= level
                        })
                })
                .collect();
            seats.sort_unstable();
            let key = seats
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            if !groups.iter().any(|(existing, _)| *existing == key) {
                groups.push((key, seats));
            }
        }
    }
    groups
}

/// Exact river heads-up enumeration (js/ai-core.js:720-737).
#[allow(clippy::cast_precision_loss)]
fn exact_river(context: &DecisionContext, samplers: &[Sampler]) -> RangeEquity {
    let hole = &context.hole_cards;
    let board = &context.community_cards;
    let mut hero_cards = hole.clone();
    hero_cards.extend_from_slice(board);
    let hero = hand_score(&hero_cards, context.variant);
    let mut sum = 0.0;
    let mut squares = 0.0;
    let mut total = 0.0;
    let mut relations = Vec::new();
    let mut weights = Vec::new();
    let mut selection_scores = Vec::new();
    for combo in &samplers[0].combos {
        let mut cards = vec![combo.c1, combo.c2];
        cards.extend_from_slice(board);
        let other = hand_score(&cards, context.variant);
        let outcome = if hero > other {
            1.0
        } else if hero == other {
            0.5
        } else {
            0.0
        };
        sum += outcome * combo.weight;
        squares += outcome * outcome * combo.weight;
        total += combo.weight;
        relations.push(vec![if hero > other {
            1
        } else if hero == other {
            0
        } else {
            -1
        }]);
        weights.push(combo.weight);
        let features = math::board_combo_features(combo.c1, combo.c2, board, context.variant);
        selection_scores.push(vec![
            (features.value + features.draw * 0.24 + features.nut_blocker * 0.03).clamp(0.0, 1.0),
        ]);
    }
    let mean = if total > 0.0 { sum / total } else { 0.5 };
    let variance = (squares / total.max(1.0) - mean * mean).max(0.0);
    RangeEquity {
        mean,
        std_dev: variance.sqrt(),
        standard_error: 0.0,
        range_confidence: 0.12,
        subset_equities: Vec::new(),
        response_samples: Some(ResponseSamples {
            seats: vec![samplers[0].seat],
            relations,
            strengths: Vec::new(),
            selection_scores,
            weights,
        }),
        samples: samplers[0].combos.len(),
        confidence: 1.0,
        exact: true,
        timed_out: false,
        stalled: false,
    }
}

/// Estimates hero range equity against the current opponent beliefs.
///
/// # Panics
///
/// Panics only when the poker-core evaluator is called with more than seven
/// cards, which cannot happen for a hold'em board.
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn estimate_range_equity(
    context: &DecisionContext,
    beliefs: &HashMap<usize, OpponentBelief>,
    models: &HashMap<usize, crate::model::BehavioralModel>,
    rng: &mut AiRng,
    want_response_samples: bool,
) -> RangeEquity {
    let mut known: u64 = 0;
    for card in context
        .hole_cards
        .iter()
        .chain(context.community_cards.iter())
    {
        known |= card_index(*card);
    }
    let deck: Vec<Card> = math::deck_cards(context.variant)
        .into_iter()
        .filter(|card| known & card_index(*card) == 0)
        .collect();
    let seats: Vec<usize> = if context.active_opponent_seats.is_empty() {
        (0..context.num_opponents)
            .map(|index| context.player_count + index)
            .collect()
    } else {
        context.active_opponent_seats.clone()
    };
    let mut feature_cache: HashMap<String, math::BoardComboFeatures> = HashMap::new();
    let samplers: Vec<Sampler> = seats
        .iter()
        .map(|seat| {
            build_sampler(
                &deck,
                *seat,
                beliefs.get(seat),
                models.get(seat),
                context.variant,
                &mut feature_cache,
            )
        })
        .filter(|sampler| !sampler.combos.is_empty())
        .collect();
    if samplers.is_empty() {
        return RangeEquity::default();
    }
    let mut range_confidence = 0.0;
    for seat in &seats {
        range_confidence += beliefs.get(seat).map_or(0.12, |belief| belief.confidence);
    }
    range_confidence /= seats.len() as f64;

    if context.community_cards.len() == 5 && samplers.len() == 1 {
        let mut result = exact_river(context, &samplers);
        result.range_confidence = range_confidence;
        return result;
    }

    let max_samples = if context.mc_samples > 0.0 {
        (context.mc_samples as usize).max(120)
    } else {
        baseline_samples(context.num_opponents, context.community_cards.len()).max(120)
    };

    let contest_groups: Vec<(String, Vec<usize>)> = pot_contest_groups(context)
        .into_iter()
        .filter(|(_, seats)| !seats.is_empty() && seats.len() < samplers.len())
        .collect();
    let mut group_sums = vec![0.0_f64; contest_groups.len()];
    let mut response_samples = if want_response_samples {
        Some(ResponseSamples {
            seats: samplers.iter().map(|sampler| sampler.seat).collect(),
            ..ResponseSamples::default()
        })
    } else {
        None
    };
    let mut samples = 0usize;
    let mut sum = 0.0;
    let mut sum_squares = 0.0;
    let mut stalled = false;
    let mut attempt = 0usize;
    while samples < max_samples {
        attempt += 1;
        let mut used = known;
        let mut opponent_cards: Vec<(usize, Card, Card)> = Vec::new();
        let mut order: Vec<usize> = (0..samplers.len()).collect();
        for j in (1..order.len()).rev() {
            let k = rng.int(j + 1);
            order.swap(j, k);
        }
        let mut valid = true;
        for index in order {
            let sampler = &samplers[index];
            match weighted_pick(sampler, rng, used) {
                Some(combo) => {
                    used |= card_index(combo.c1) | card_index(combo.c2);
                    opponent_cards.push((sampler.seat, combo.c1, combo.c2));
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            if samples == 0 && attempt >= 5 {
                stalled = true;
                break;
            }
            continue;
        }
        let mut board = context.community_cards.clone();
        while board.len() < 5 {
            let mut card: Option<Card> = None;
            for _ in 0..BOARD_GUARD {
                let candidate = deck[rng.int(deck.len())];
                if used & card_index(candidate) == 0 {
                    card = Some(candidate);
                    break;
                }
            }
            match card {
                Some(card) => {
                    used |= card_index(card);
                    board.push(card);
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let mut hero_cards = context.hole_cards.clone();
        hero_cards.extend_from_slice(&board);
        let hero = hand_score(&hero_cards, context.variant);
        let mut beaten = false;
        let mut ties = 0usize;
        let mut relations: Vec<Relation> = Vec::with_capacity(opponent_cards.len());
        let mut scores: HashMap<usize, u64> = HashMap::new();
        for (seat, c1, c2) in &opponent_cards {
            let mut cards = vec![*c1, *c2];
            cards.extend_from_slice(&board);
            let score = hand_score(&cards, context.variant);
            scores.insert(*seat, score);
            relations.push(if score > hero {
                -1
            } else if score == hero {
                ties += 1;
                0
            } else {
                1
            });
            if score > hero {
                beaten = true;
            }
        }
        for (index, (_, group_seats)) in contest_groups.iter().enumerate() {
            let mut loses = false;
            let mut split = 0usize;
            for seat in group_seats {
                if let Some(score) = scores.get(seat) {
                    if *score > hero {
                        loses = true;
                    } else if *score == hero {
                        split += 1;
                    }
                }
            }
            group_sums[index] += if loses {
                0.0
            } else {
                1.0 / (split as f64 + 1.0)
            };
        }
        let outcome = if beaten {
            0.0
        } else {
            1.0 / (ties as f64 + 1.0)
        };
        if let Some(samples_info) = response_samples.as_mut() {
            samples_info.relations.push(relations);
            if context.street == crate::input::Street::Preflop {
                samples_info.strengths.push(
                    opponent_cards
                        .iter()
                        .map(|(_, c1, c2)| {
                            math::starting_hand_strength(
                                *c1,
                                *c2,
                                context.variant,
                                math::Position::Middle,
                            )
                        })
                        .collect(),
                );
            } else {
                samples_info.selection_scores.push(
                    opponent_cards
                        .iter()
                        .map(|(_, c1, c2)| {
                            let features =
                                math::board_combo_features(*c1, *c2, &board, context.variant);
                            (features.value + features.draw * 0.24 + features.nut_blocker * 0.03)
                                .clamp(0.0, 1.0)
                        })
                        .collect(),
                );
            }
        }
        sum += outcome;
        sum_squares += outcome * outcome;
        samples += 1;
    }
    let mean = if samples > 0 {
        sum / samples as f64
    } else {
        0.5
    };
    let variance = if samples > 0 {
        (sum_squares / samples as f64 - mean * mean).max(0.0)
    } else {
        0.0625
    };
    RangeEquity {
        mean: mean.clamp(0.005, 0.995),
        std_dev: variance.sqrt(),
        standard_error: (variance / samples.max(1) as f64).sqrt(),
        range_confidence,
        subset_equities: contest_groups
            .iter()
            .enumerate()
            .map(|(index, (key, _))| {
                (
                    key.clone(),
                    if samples > 0 {
                        group_sums[index] / samples as f64
                    } else {
                        0.5
                    },
                )
            })
            .collect(),
        response_samples,
        samples,
        confidence: (samples as f64 / 160.0_f64.max(max_samples as f64)).clamp(0.08, 0.96),
        exact: false,
        timed_out: false,
        stalled,
    }
}

/// Returns the exact enumerations size for a river heads-up board.
#[must_use]
pub fn exact_river_samples(deck_len: usize) -> usize {
    deck_len * deck_len.saturating_sub(1) / 2
}

/// Removes duplicate cards from a list (debug helper).
#[must_use]
pub fn distinct_cards(cards: &[Card]) -> bool {
    let mut seen: HashSet<u8> = HashSet::new();
    cards.iter().all(|card| seen.insert(card.index()))
}
