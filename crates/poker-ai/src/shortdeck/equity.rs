//! Range-weighted equity sampling and pot-contest helpers for the frozen
//! short-deck core (ports of `js/ai-shortdeck-stable.js`: `estimateRangeEquity`
//! `stable:638-756`, `potContestGroups` `stable:869-882`, `contestablePot`
//! `stable:817-823`, `contestableReturn` `stable:883-901`,
//! `expectedAdditionalCalls` `stable:827-841`, `conditionalCalledEquity`
//! `stable:842-868`).
//!
//! The wall-clock safety brake of the original sampler is intentionally absent:
//! the golden corpus is generated with a virtual clock (`performance.now() = 0`)
//! and the spec requires fixed sample counts, so the emergency brake can never
//! fire during replay. The sample schedule, RNG consumption order and stall
//! detection are reproduced exactly.

use std::collections::{HashMap, HashSet};

use poker_core::{Card, Variant, evaluate};

use crate::input::DecisionContext;
use crate::rng::AiRng;

use super::belief::OpponentBelief;
use super::types::{Combo, FeatureKey, RangeEquity, Sampler, Seat, clamp};

/// One pot-contest subset group (JS `{key, seats, sum}`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ContestGroup {
    /// Seat list key (sorted string seat ids joined by comma).
    pub key: String,
    /// Seats contributing to this group.
    pub seats: Vec<usize>,
    /// Running sum of per-sample hero shares.
    pub sum: f64,
}

/// JS `handScore(result)`: the evaluator score as a comparison number.
#[must_use]
fn hand_score(cards: &[Card], variant: Variant) -> u64 {
    evaluate(cards, variant).score()
}

/// JS `weightedPick(combos, rng, used)` (`stable:595-616`).
///
/// Up to 22 binary-search attempts (one `next()` each) and a linear-scan
/// fallback that consumes one more `next()`.
#[must_use]
pub(crate) fn weighted_pick(
    combos: &[Combo],
    rng: &mut AiRng,
    used: &HashSet<Card>,
) -> Option<(Card, Card)> {
    if combos.is_empty() {
        return None;
    }
    let total_weight = combos.last().map_or(0.0, |combo| combo.cumulative);
    for _attempt in 0..22 {
        let target = rng.next_f64() * total_weight;
        let mut low = 0_usize;
        let mut high = combos.len() - 1;
        while low < high {
            let mid = (low + high) / 2;
            if combos[mid].cumulative < target {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let combo = &combos[low];
        if !used.contains(&combo.c1) && !used.contains(&combo.c2) {
            return Some((combo.c1, combo.c2));
        }
    }
    let mut total = 0.0;
    for combo in combos {
        if !used.contains(&combo.c1) && !used.contains(&combo.c2) {
            total += combo.weight;
        }
    }
    let mut roll = rng.next_f64() * total;
    for combo in combos {
        if !used.contains(&combo.c1) && !used.contains(&combo.c2) {
            roll -= combo.weight;
            if roll <= 0.0 {
                return Some((combo.c1, combo.c2));
            }
        }
    }
    None
}

/// JS `buildWeightedCombos(deck, belief, variant, featureCache)`
/// (`stable:618-631`).
#[must_use]
pub(crate) fn build_weighted_combos(
    deck: &[Card],
    belief: Option<&OpponentBelief>,
    variant: Variant,
    feature_cache: &mut HashMap<FeatureKey, crate::math::BoardComboFeatures>,
) -> Vec<Combo> {
    let mut combos = Vec::with_capacity(deck.len() * deck.len() / 2);
    let mut total_weight = 0.0;
    for i in 0..deck.len() {
        for j in (i + 1)..deck.len() {
            let weight = belief.map_or(1.0, |belief| {
                belief.combo_weight(deck[i], deck[j], variant, feature_cache)
            });
            if weight <= 0.0 {
                continue;
            }
            total_weight += weight;
            combos.push(Combo {
                c1: deck[i],
                c2: deck[j],
                weight,
                cumulative: total_weight,
            });
        }
    }
    combos
}

/// JS `estimateRangeEquity(context, beliefs, rng)` (`stable:638-756`).
pub(crate) fn estimate_range_equity(
    context: &DecisionContext,
    beliefs: &HashMap<usize, OpponentBelief>,
    rng: &mut AiRng,
) -> RangeEquity {
    let variant = context.variant;
    let mut known: HashSet<Card> = HashSet::new();
    known.extend(context.hole_cards.iter().copied());
    known.extend(context.community_cards.iter().copied());
    let deck: Vec<Card> = crate::math::deck_cards(variant)
        .into_iter()
        .filter(|card| !known.contains(card))
        .collect();
    let seats: Vec<Seat> = if context.active_opponent_seats.is_empty() {
        (0..context.num_opponents).map(Seat::Unknown).collect()
    } else {
        context
            .active_opponent_seats
            .iter()
            .copied()
            .map(Seat::Known)
            .collect()
    };
    let mut feature_cache: HashMap<FeatureKey, crate::math::BoardComboFeatures> = HashMap::new();
    let mut samplers: Vec<Sampler> = Vec::new();
    for seat in &seats {
        let belief = match seat {
            Seat::Known(seat_id) => beliefs.get(seat_id),
            Seat::Unknown(_) => None,
        };
        let combos = build_weighted_combos(&deck, belief, variant, &mut feature_cache);
        if !combos.is_empty() {
            samplers.push(Sampler {
                seat: *seat,
                combos,
            });
        }
    }
    if samplers.is_empty() {
        return RangeEquity::empty();
    }
    let range_confidence = if seats.is_empty() {
        0.0
    } else {
        let sum: f64 = seats
            .iter()
            .map(|seat| match seat {
                Seat::Known(seat_id) => beliefs
                    .get(seat_id)
                    .map_or(0.12, |belief| belief.confidence),
                Seat::Unknown(_) => 0.12,
            })
            .sum();
        #[allow(clippy::cast_precision_loss)]
        {
            sum / seats.len() as f64
        }
    };
    let mut subset_groups: Vec<ContestGroup> = pot_contest_groups(context)
        .into_iter()
        .filter(|group| !group.seats.is_empty() && group.seats.len() < samplers.len())
        .collect();
    // Exact river heads-up enumeration (no RNG).
    if context.community_cards.len() == 5 && samplers.len() == 1 {
        let mut hero_cards = context.hole_cards.clone();
        hero_cards.extend(context.community_cards.iter().copied());
        let hero = hand_score(&hero_cards, variant);
        let mut sum = 0.0;
        let mut squares = 0.0;
        let mut total = 0.0;
        for combo in &samplers[0].combos {
            let mut cards = vec![combo.c1, combo.c2];
            cards.extend(context.community_cards.iter().copied());
            let other = hand_score(&cards, variant);
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
        }
        let mean = sum / total;
        return RangeEquity {
            mean,
            std_dev: (squares / total - mean * mean).max(0.0).sqrt(),
            standard_error: Some(0.0),
            range_confidence: Some(range_confidence),
            subset_equities: None,
            samples: samplers[0].combos.len(),
            confidence: 1.0,
            exact: true,
            timed_out: false,
            stalled: false,
        };
    }
    let budget = clamp(context.time_budget_ms, 20.0, 180.0);
    let _ = budget;
    let baseline_samples = if context.num_opponents >= 4 {
        240_usize
    } else if context.num_opponents >= 2 {
        320
    } else {
        420
    };
    let river_bonus = if context.community_cards.len() >= 5 {
        40_usize
    } else {
        0
    };
    // JS: Math.max(120, Math.floor(context.mcSamples || baseline + riverBonus)).
    // A negative override is truthy in JS and floors below the 120 floor.
    #[allow(clippy::cast_possible_truncation)]
    let configured_samples = if context.mc_samples != 0.0 {
        context.mc_samples.floor() as i64
    } else {
        (baseline_samples + river_bonus) as i64
    };
    let max_samples = 120_i64.max(configured_samples) as usize;
    let mut samples = 0_usize;
    let mut sum = 0.0_f64;
    let mut sum_squares = 0.0_f64;
    let mut stalled = false;
    let mut attempt = 0_usize;
    while samples < max_samples {
        // Emergency brake: virtual clock (0) never reaches budget * 3.
        let mut used = known.clone();
        let mut opponent_cards: Vec<(Seat, Card, Card)> = Vec::new();
        let mut valid = true;
        let mut order: Vec<usize> = (0..samplers.len()).collect();
        for j in (1..order.len()).rev() {
            let k = rng.int(j + 1);
            order.swap(j, k);
        }
        for index in order {
            match weighted_pick(&samplers[index].combos, rng, &used) {
                Some((c1, c2)) => {
                    used.insert(c1);
                    used.insert(c2);
                    opponent_cards.push((samplers[index].seat, c1, c2));
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
            attempt += 1;
            continue;
        }
        let mut board = context.community_cards.clone();
        while board.len() < 5 {
            let mut card: Option<Card> = None;
            for _guard in 0..30 {
                let candidate = deck[rng.int(deck.len())];
                if !used.contains(&candidate) {
                    card = Some(candidate);
                }
                if card.is_some() {
                    break;
                }
            }
            match card {
                Some(card) => {
                    used.insert(card);
                    board.push(card);
                }
                None => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            attempt += 1;
            continue;
        }
        let mut hero_cards = context.hole_cards.clone();
        hero_cards.extend(board.iter().copied());
        let hero = hand_score(&hero_cards, variant);
        let use_scores = !subset_groups.is_empty();
        let mut scores: HashMap<Seat, u64> = HashMap::new();
        let mut beaten = false;
        let mut ties = 0_u32;
        for (seat, c1, c2) in &opponent_cards {
            let mut cards = vec![*c1, *c2];
            cards.extend(board.iter().copied());
            let score = hand_score(&cards, variant);
            if use_scores {
                scores.insert(*seat, score);
            }
            if score > hero {
                beaten = true;
                if !use_scores {
                    break;
                }
            }
            if score == hero {
                ties += 1;
            }
        }
        for group in &mut subset_groups {
            let mut loses = false;
            let mut split = 0_u32;
            for seat in &group.seats {
                if let Some(score) = scores.get(&Seat::Known(*seat)) {
                    if *score > hero {
                        loses = true;
                    } else if *score == hero {
                        split += 1;
                    }
                }
            }
            group.sum += if loses {
                0.0
            } else {
                1.0 / (f64::from(split) + 1.0)
            };
        }
        let outcome = if beaten {
            0.0
        } else {
            1.0 / (f64::from(ties) + 1.0)
        };
        sum += outcome;
        sum_squares += outcome * outcome;
        samples += 1;
        attempt += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let sample_count = samples as f64;
    let mean = if samples > 0 { sum / sample_count } else { 0.5 };
    let variance = if samples > 0 {
        (sum_squares / sample_count - mean * mean).max(0.0)
    } else {
        0.0625
    };
    let mut subset_equities: HashMap<String, f64> = HashMap::new();
    for group in &subset_groups {
        subset_equities.insert(
            group.key.clone(),
            if samples > 0 {
                group.sum / sample_count
            } else {
                0.5
            },
        );
    }
    #[allow(clippy::cast_precision_loss)]
    let confidence = clamp(
        sample_count / (160_usize.max(max_samples) as f64),
        0.08,
        0.96,
    );
    RangeEquity {
        mean: clamp(mean, 0.005, 0.995),
        std_dev: variance.sqrt(),
        standard_error: Some((variance / (samples.max(1) as f64)).sqrt()),
        range_confidence: Some(range_confidence),
        subset_equities: Some(subset_equities),
        samples,
        confidence,
        exact: false,
        timed_out: false,
        stalled,
    }
}

/// JS `opponentGroupKey(seats)`: sorted string seat ids joined by comma.
#[must_use]
pub(crate) fn opponent_group_key(seats: &[usize]) -> String {
    let mut keys: Vec<String> = seats.iter().map(usize::to_string).collect();
    keys.sort();
    keys.join(",")
}

/// Finds a public player row by seat.
#[must_use]
fn find_player(context: &DecisionContext, seat: usize) -> Option<&crate::input::PublicPlayer> {
    context
        .public_players
        .iter()
        .find(|player| player.seat == seat)
}

/// JS `potContestGroups(context)` (`stable:869-882`).
#[must_use]
pub(crate) fn pot_contest_groups(context: &DecisionContext) -> Vec<ContestGroup> {
    let mut groups: Vec<ContestGroup> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for player in &context.public_players {
        for level in [player.committed, player.committed + 1.0] {
            let seats: Vec<usize> = context
                .active_opponent_seats
                .iter()
                .copied()
                .filter(|seat| match find_player(context, *seat) {
                    None => true,
                    Some(opponent) => {
                        !opponent.folded && opponent.committed + opponent.stack >= level
                    }
                })
                .collect();
            let key = opponent_group_key(&seats);
            if let std::collections::hash_map::Entry::Vacant(entry) = index.entry(key.clone()) {
                entry.insert(groups.len());
                groups.push(ContestGroup {
                    key,
                    seats,
                    sum: 0.0,
                });
            }
        }
    }
    groups
}

/// JS `contestablePot(context, invest = 0)` (`stable:817-823`).
#[must_use]
pub(crate) fn contestable_pot(context: &DecisionContext, invest: f64) -> f64 {
    if context.public_players.is_empty() {
        return context.pot;
    }
    let Some(hero) = find_player(context, context.hero_seat) else {
        return context.pot;
    };
    let cap = hero.committed + invest;
    let total: f64 = context
        .public_players
        .iter()
        .map(|player| player.committed.min(cap))
        .sum();
    context.pot.min(total)
}

/// JS `contestableReturn(context, invest, equityInfo)` (`stable:883-901`).
#[must_use]
pub(crate) fn contestable_return(
    context: &DecisionContext,
    invest: f64,
    equity: &RangeEquity,
) -> f64 {
    let Some(hero) = find_player(context, context.hero_seat) else {
        return equity.mean * (context.pot + invest);
    };
    let cap = hero.committed + invest;
    let commitments: Vec<f64> = context
        .public_players
        .iter()
        .map(|player| {
            let own_invest = if player.seat == hero.seat {
                invest
            } else {
                0.0
            };
            cap.min(player.committed + own_invest)
        })
        .collect();
    let mut levels: Vec<f64> = commitments
        .iter()
        .copied()
        .filter(|amount| *amount > 0.0)
        .collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut expected = 0.0;
    let mut previous = 0.0;
    for level in levels {
        #[allow(clippy::cast_precision_loss)]
        let contributors = commitments
            .iter()
            .filter(|amount| **amount >= level)
            .count() as f64;
        let amount = (level - previous) * contributors;
        let seats: Vec<usize> = context
            .active_opponent_seats
            .iter()
            .copied()
            .filter(|seat| match find_player(context, *seat) {
                None => true,
                Some(opponent) => !opponent.folded && opponent.committed + opponent.stack >= level,
            })
            .collect();
        let group_equity = if seats.is_empty() {
            1.0
        } else {
            let key = opponent_group_key(&seats);
            let from_subsets = equity
                .subset_equities
                .as_ref()
                .and_then(|subsets| subsets.get(&key).copied());
            from_subsets.unwrap_or(equity.mean)
        };
        expected += amount * group_equity;
        previous = level;
    }
    expected
}

/// JS `expectedAdditionalCalls(context, target, response)` (`stable:827-841`).
#[must_use]
pub(crate) fn expected_additional_calls(
    context: &DecisionContext,
    target: f64,
    response: &super::types::Response,
) -> f64 {
    let needed = (target - context.current_bet).max(0.0);
    if context.public_players.is_empty() || response.opponents.is_empty() {
        return needed * response.expected_callers;
    }
    let no_raise = 1.0 - response.any_raise;
    let any_call_given_no_raise = response.any_call / no_raise.max(1e-9);
    if any_call_given_no_raise < 1e-9 {
        return 0.0;
    }
    let mut sum = 0.0;
    for opponent in &response.opponents {
        let player = find_player(context, opponent.seat);
        if player.is_some_and(|player| player.folded) {
            continue;
        }
        let capacity = player.map_or(needed, |player| {
            needed.min((player.stack - (context.current_bet - player.round_bet).max(0.0)).max(0.0))
        });
        sum += capacity * opponent.call / (opponent.call + opponent.fold).max(1e-9);
    }
    sum / any_call_given_no_raise
}

/// JS `conditionalCalledEquity(equityInfo, response)` (`stable:842-868`).
///
/// The stable short-deck core never records response samples
/// (`conditionalCallerEquityWeight = 0`), so this always reduces to the plain
/// equity mean. The full integration is intentionally not ported: promoting it
/// would require the `responseSamples` relation matrix that the frozen
/// short-deck profile disables (`docs/specs/ai-strategy.md` section 9.2).
#[must_use]
pub(crate) fn conditional_called_equity(
    equity: &RangeEquity,
    _response: &super::types::Response,
) -> f64 {
    equity.mean
}
