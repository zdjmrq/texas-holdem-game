//! Post-flop candidate generation, response models and bounded mixing.
//!
//! Port of js/ai-core.js (responseForRaise / preflopResponse /
//! realizationFactor / riskPenalty / raiseTargets / buildPostflopCandidates /
//! selectCandidate). See docs/specs/ai-strategy.md sections 6.7-6.9.

use std::cmp::Ordering;
use std::collections::HashMap;

use poker_engine::ActionType;

use crate::belief::OpponentBelief;
use crate::equity::RangeEquity;
use crate::input::{DecisionContext, Street};
use crate::math::{self, Position, Texture};
use crate::model::BehavioralModel;
use crate::plan::HandPlan;
use crate::profile::AiProfile;
use crate::rng::AiRng;
use crate::self_image::SelfImage;

/// Opponent response model probabilities for one target size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResponseModel {
    /// Probability every opponent folds.
    pub all_fold: f64,
    /// Probability someone calls and nobody raises.
    pub any_call: f64,
    /// Probability someone raises.
    pub any_raise: f64,
    /// Expected callers conditional on a call without a raise.
    pub expected_callers: f64,
    /// Story credibility score.
    pub credibility: f64,
    /// Confidence of the estimate.
    pub confidence: f64,
    /// Bet size as a fraction of the pot.
    pub fraction: f64,
    /// Per-opponent fold probabilities.
    pub opponent_folds: Vec<(usize, f64)>,
    /// Per-opponent call probabilities.
    pub opponent_calls: Vec<(usize, f64)>,
    /// Per-opponent raise probabilities.
    pub opponent_raises: Vec<(usize, f64)>,
}

/// One evaluated action candidate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostflopCandidate {
    /// Action name (fold/check/call/raise/allin).
    pub action: String,
    /// Amount in chips.
    pub amount: u64,
    /// Chips invested by the action.
    pub invest: f64,
    /// Expected value.
    pub ev: f64,
    /// Risk penalty.
    pub risk: f64,
    /// Final utility (ev - risk + bonuses).
    pub utility: f64,
    /// Reason tag.
    pub reason: String,
    /// Conditional called equity (when computed).
    pub called_equity: Option<f64>,
    /// Bluff-catch classification.
    pub bluff_catch: bool,
    /// Plan continuity bonus.
    pub plan_bonus: f64,
    /// Terminal call classification (experimental; default None).
    pub terminal_call: Option<String>,
    /// Response model summary (raise candidates only).
    pub response: Option<ResponseModel>,
}

/// One read (belief + model) for an active opponent.
#[derive(Clone, Copy)]
pub struct Read<'a> {
    /// Seat.
    pub seat: usize,
    /// Per-hand belief.
    pub belief: &'a OpponentBelief,
    /// Cross-hand model.
    pub model: &'a BehavioralModel,
}

/// Target amount as a fraction of max(big blind, pot + to call).
#[must_use]
pub fn bet_fraction_for_target(context: &DecisionContext, target: f64) -> f64 {
    let invest = (target - context.hero_round_bet).max(0.0);
    invest / context.big_blind.max(context.pot + context.to_call)
}

/// Contestable pot capped by the hero's commitment.
#[must_use]
pub fn contestable_pot(context: &DecisionContext, invest: f64) -> f64 {
    let Some(hero) = context
        .public_players
        .iter()
        .find(|player| player.seat == context.hero_seat)
    else {
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

/// Expected share of existing pots plus the hero's increment (layer aware).
#[must_use]
pub fn contestable_return(context: &DecisionContext, invest: f64, equity: &RangeEquity) -> f64 {
    let Some(hero) = context
        .public_players
        .iter()
        .find(|player| player.seat == context.hero_seat)
    else {
        return equity.mean * (context.pot + invest);
    };
    let cap = hero.committed + invest;
    let commitments: Vec<f64> = context
        .public_players
        .iter()
        .map(|player| {
            cap.min(
                player.committed
                    + if player.seat == hero.seat {
                        invest
                    } else {
                        0.0
                    },
            )
        })
        .collect();
    let mut levels: Vec<f64> = commitments
        .iter()
        .copied()
        .filter(|value| *value > 0.0)
        .collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut expected = 0.0;
    let mut previous = 0.0;
    for level in levels {
        let contributors = commitments.iter().filter(|value| **value >= level).count() as f64;
        let amount = (level - previous) * contributors;
        let mut seats: Vec<usize> = context
            .active_opponent_seats
            .iter()
            .copied()
            .filter(|seat| {
                context
                    .public_players
                    .iter()
                    .find(|player| player.seat == *seat)
                    .is_none_or(|player| !player.folded && player.committed + player.stack >= level)
            })
            .collect();
        seats.sort_unstable();
        let key = seats
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fraction = if seats.is_empty() {
            1.0
        } else {
            equity
                .subset_equities
                .iter()
                .find(|(group_key, _)| *group_key == key)
                .map_or(equity.mean, |(_, value)| *value)
        };
        expected += amount * fraction;
        previous = level;
    }
    expected
}

/// Additional calls weighted per seat and conditional on a call without raise.
#[must_use]
pub fn expected_additional_calls(
    context: &DecisionContext,
    target: f64,
    response: &ResponseModel,
) -> f64 {
    let needed = (target - context.current_bet).max(0.0);
    if context.public_players.is_empty() || response.opponent_folds.is_empty() {
        return needed * response.expected_callers;
    }
    let no_raise = 1.0 - response.any_raise;
    let any_call_given_no_raise = response.any_call / no_raise.max(1e-9);
    if any_call_given_no_raise < 1e-9 {
        return 0.0;
    }
    let mut sum = 0.0;
    for (index, (seat, fold)) in response.opponent_folds.iter().enumerate() {
        let Some(player) = context
            .public_players
            .iter()
            .find(|player| player.seat == *seat)
        else {
            continue;
        };
        if player.folded {
            continue;
        }
        let call = response
            .opponent_calls
            .get(index)
            .map_or(0.0, |(_, call)| *call);
        let capacity = player.stack.min((target - player.round_bet).max(0.0));
        if call + fold > 1e-9 {
            sum += capacity * call / (call + fold);
        }
    }
    sum / any_call_given_no_raise
}

/// Conditional called equity using response samples.
#[must_use]
pub fn conditional_called_equity(equity: &RangeEquity, response: &ResponseModel) -> f64 {
    let Some(samples) = equity.response_samples.as_ref() else {
        return equity.mean;
    };
    if samples.relations.is_empty() {
        return equity.mean;
    }
    let probabilities: Vec<f64> = samples
        .seats
        .iter()
        .map(|seat| {
            response
                .opponent_folds
                .iter()
                .find(|(candidate, _)| candidate == seat)
                .map_or(0.5, |(_, fold)| 1.0 - *fold)
        })
        .collect();
    let all_fold: f64 = probabilities.iter().map(|p| 1.0 - p).product();
    if 1.0 - all_fold < 1e-9 {
        return equity.mean;
    }
    let mut sum = 0.0;
    for relations in &samples.relations {
        let mut not_beaten = 1.0;
        let mut coefficients = vec![1.0_f64];
        for (index, relation) in relations.iter().enumerate() {
            let p = probabilities.get(index).copied().unwrap_or(0.0);
            if *relation == -1 {
                not_beaten *= 1.0 - p;
            } else if *relation == 0 {
                let mut next = vec![0.0_f64; coefficients.len() + 1];
                for (j, coefficient) in coefficients.iter().enumerate() {
                    next[j] += coefficient * (1.0 - p);
                    next[j + 1] += coefficient * p;
                }
                coefficients = next;
            }
        }
        let share = not_beaten
            * coefficients
                .iter()
                .enumerate()
                .map(|(index, coefficient)| coefficient / (index as f64 + 1.0))
                .sum::<f64>();
        sum += (share - all_fold) / (1.0 - all_fold);
    }
    (sum / samples.relations.len() as f64).clamp(0.0, 1.0)
}

/// Story credibility for aggressive actions.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn story_credibility(
    context: &DecisionContext,
    texture: &Texture,
    action: &str,
    equity: f64,
    reads: &[Read<'_>],
    profile: &AiProfile,
    self_image: &SelfImage,
    plan: Option<&HandPlan>,
) -> f64 {
    if action != "raise" && action != "allin" {
        return 0.5;
    }
    let image = self_image.snapshot();
    let mut score = 0.42;
    if context.is_previous_street_aggressor {
        score += 0.12;
    }
    if context.is_delayed_cbet_candidate && texture.pressure_card {
        score += 0.10;
    }
    if texture.monotone || texture.paired {
        score += 0.04;
    }
    score += texture.nut_blocker * 0.08;
    if equity > 0.62 || texture.has_strong_draw {
        score += 0.10;
    }
    if image.tightness > 0.58 {
        score += 0.10 * profile.image_awareness;
    }
    if image.aggression > 0.68 {
        score -= 0.07 * profile.image_awareness;
    }
    if plan.is_some_and(|plan| plan.intent == crate::plan::Intent::Bluff)
        && !texture.pressure_card
        && !texture.has_strong_draw
    {
        score -= 0.09;
    }
    if reads.iter().all(|read| read.belief.capped) {
        score += 0.08;
    }
    score.clamp(0.12, 0.90)
}

/// Response model for a raise target (js/ai-core.js:1330-1378).
#[must_use]
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn response_for_raise(
    context: &DecisionContext,
    target: f64,
    equity: f64,
    texture: &Texture,
    reads: &[Read<'_>],
    profile: &AiProfile,
    self_image: &SelfImage,
    plan: Option<&HandPlan>,
) -> ResponseModel {
    let fraction = bet_fraction_for_target(context, target);
    let credibility = story_credibility(
        context, texture, "raise", equity, reads, profile, self_image, plan,
    );
    let mut all_fold = 1.0;
    let mut no_raise = 1.0;
    let mut confidence = 0.0;
    let mut conditional_callers = 0.0;
    let mut conditional_all_fold = 1.0;
    let mut opponent_folds = Vec::with_capacity(reads.len());
    let mut opponent_calls = Vec::with_capacity(reads.len());
    let mut opponent_raises = Vec::with_capacity(reads.len());
    for read in reads {
        let player = context
            .public_players
            .iter()
            .find(|player| player.seat == read.seat);
        if player.is_some_and(|player| player.all_in || player.stack == 0.0) {
            opponent_folds.push((read.seat, 0.0));
            opponent_calls.push((read.seat, 1.0));
            opponent_raises.push((read.seat, 0.0));
            conditional_callers += 1.0;
            conditional_all_fold = 0.0;
            all_fold = 0.0;
            continue;
        }
        let mut fold = read.model.fold_rate_for(fraction, false);
        fold += (fraction - 0.55) * 0.13;
        fold += (credibility - 0.5) * 0.20;
        fold -= (read.belief.estimated_strength() - 0.5) * 0.32;
        if read.model.calls_too_much() {
            fold -= 0.13;
        }
        if read.model.folds_too_much() {
            fold += 0.11;
        }
        if context.num_opponents > 1 {
            fold -= 0.025;
        }
        fold = fold.clamp(0.04, 0.86);
        let can_raise = target < context.max_raise_to
            && player.is_none_or(|player| player.round_bet + player.stack > target);
        let raise = if can_raise {
            ((read.model.aggression_rate() - 0.30) * 0.18
                + (read.belief.estimated_strength() - 0.58).max(0.0) * 0.25
                + fraction * 0.025)
                .clamp(0.015, 0.26)
        } else {
            0.0
        };
        fold = fold.min(1.0 - raise);
        let call = 1.0 - fold - raise;
        opponent_folds.push((read.seat, fold));
        opponent_calls.push((read.seat, call));
        opponent_raises.push((read.seat, raise));
        all_fold *= fold;
        no_raise *= 1.0 - raise;
        conditional_callers += call / (1.0 - raise).max(1e-9);
        conditional_all_fold *= fold / (1.0 - raise).max(1e-9);
        confidence += read.model.confidence() * 0.55 + read.belief.confidence * 0.45;
    }
    ResponseModel {
        all_fold,
        any_raise: 1.0 - no_raise,
        any_call: (no_raise - all_fold).max(0.0),
        expected_callers: conditional_callers / (1.0 - conditional_all_fold).max(1e-9),
        credibility,
        confidence: if reads.is_empty() {
            0.2
        } else {
            confidence / reads.len() as f64
        },
        fraction,
        opponent_folds,
        opponent_calls,
        opponent_raises,
    }
}

fn fallback_response(
    context: &DecisionContext,
    target: f64,
    equity: f64,
    reads: &[Read<'_>],
    profile: &AiProfile,
) -> ResponseModel {
    let texture = Texture::default();
    let self_image = SelfImage::new();
    let mut response = response_for_raise(
        context,
        target,
        equity,
        &texture,
        reads,
        profile,
        &self_image,
        None,
    );
    response.fraction = bet_fraction_for_target(context, target);
    response
}

/// Pre-flop continuation response (js/ai-core.js:1292-1329).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn preflop_response(
    context: &DecisionContext,
    target: f64,
    mode: &str,
    reads: &[Read<'_>],
    equity: &RangeEquity,
    profile: &AiProfile,
) -> ResponseModel {
    let Some(samples) = equity.response_samples.as_ref() else {
        return fallback_response(context, target, equity.mean, reads, profile);
    };
    if samples.relations.is_empty() || samples.strengths.is_empty() {
        return fallback_response(context, target, equity.mean, reads, profile);
    }
    struct Plan {
        seat: usize,
        raise: f64,
        forced: bool,
        cutoff: Option<f64>,
    }
    let mut plans: Vec<Plan> = Vec::with_capacity(reads.len());
    for read in reads {
        let player = context
            .public_players
            .iter()
            .find(|player| player.seat == read.seat);
        let forced = player.is_some_and(|player| {
            player.all_in
                || player.stack == 0.0
                || (mode == "call" && player.round_bet >= context.current_bet)
        });
        let raised = read.belief.history.iter().any(|event| {
            event.street == Street::Preflop
                && (event.action.contains("bet")
                    || event.action == "raise"
                    || event.action == "allin")
        });
        let mut width = if context.variant.is_short_deck() {
            0.30
        } else {
            0.22
        };
        if context.preflop_raise_count > 0 {
            width = if mode == "call" {
                0.20
            } else if raised {
                0.13
            } else {
                0.095
            };
        }
        if context.preflop_raise_count >= 2 {
            width = if raised { 0.065 } else { 0.05 };
        }
        if let Some(player) = player
            && !raised
        {
            if player.is_big_blind {
                width *= 1.2;
            } else if player.is_small_blind {
                width *= 0.85;
            } else if let Some(distance) = player.position_from_button {
                match math::position_category(
                    context.player_count,
                    distance,
                    player.is_small_blind,
                    player.is_big_blind,
                ) {
                    Position::Late => width *= 1.15,
                    Position::Early => width *= 0.85,
                    _ => {}
                }
            }
        }
        let size = target / context.big_blind.max(context.current_bet);
        let raise_count = context.preflop_raise_count;
        width -= (size - if raise_count > 0 { 3.0 } else { 2.5 }).max(0.0) * 0.02;
        width += (read.model.vpip() - 0.27) * read.model.confidence() * 0.45;
        if read.model.calls_too_much() {
            width += 0.08;
        }
        let width = width.clamp(0.025, 0.55);
        let can_raise = target < context.max_raise_to
            && player.is_none_or(|player| player.round_bet + player.stack > target);
        let raise = if forced || !can_raise {
            0.0
        } else {
            (0.025 + (read.model.pfr() - 0.18).max(0.0) * read.model.confidence() * 0.25)
                .clamp(0.015, 0.12)
        };
        plans.push(Plan {
            seat: read.seat,
            raise,
            forced,
            cutoff: if forced {
                None
            } else {
                Some(math::starting_range_cutoff(width, context.variant))
            },
        });
    }
    let sample_count = samples.relations.len();
    let mut all_fold_given_no_raise = 0.0;
    let mut means = vec![0.0_f64; plans.len()];
    for sample in 0..sample_count {
        let probabilities: Vec<f64> = plans
            .iter()
            .map(|plan| {
                if plan.forced {
                    return 1.0;
                }
                let index = samples
                    .seats
                    .iter()
                    .position(|candidate| *candidate == plan.seat);
                let Some(index) = index else {
                    return 0.5;
                };
                let strength = samples
                    .strengths
                    .get(sample)
                    .and_then(|row| row.get(index))
                    .copied()
                    .unwrap_or(0.5);
                plan.cutoff.map_or(0.5, |cutoff| {
                    (1.0 / (1.0 + (-(strength - cutoff) * 24.0).exp())).clamp(0.005, 0.995)
                })
            })
            .collect();
        all_fold_given_no_raise += probabilities.iter().map(|p| 1.0 - p).product::<f64>();
        for (index, p) in probabilities.iter().enumerate() {
            means[index] += p;
        }
    }
    all_fold_given_no_raise /= sample_count as f64;
    for mean in &mut means {
        *mean /= sample_count as f64;
    }
    let no_raise: f64 = plans.iter().map(|plan| 1.0 - plan.raise).product();
    let any_call = no_raise * (1.0 - all_fold_given_no_raise);
    let expected_callers = means.iter().sum::<f64>() / (1.0 - all_fold_given_no_raise).max(1e-9);
    let opponent_folds: Vec<(usize, f64)> = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| (plan.seat, (1.0 - plan.raise) * (1.0 - means[index])))
        .collect();
    let opponent_calls: Vec<(usize, f64)> = plans
        .iter()
        .enumerate()
        .map(|(index, plan)| (plan.seat, (1.0 - plan.raise) * means[index]))
        .collect();
    let opponent_raises: Vec<(usize, f64)> =
        plans.iter().map(|plan| (plan.seat, plan.raise)).collect();
    ResponseModel {
        all_fold: no_raise * all_fold_given_no_raise,
        any_raise: 1.0 - no_raise,
        any_call,
        expected_callers,
        credibility: 0.5,
        confidence: reads
            .iter()
            .map(|read| read.model.confidence())
            .sum::<f64>()
            / reads.len().max(1) as f64,
        fraction: bet_fraction_for_target(context, target),
        opponent_folds,
        opponent_calls,
        opponent_raises,
    }
}

/// Realization factor (js/ai-core.js:1249-1266).
#[must_use]
pub fn realization_factor(
    context: &DecisionContext,
    texture: &Texture,
    reads: &[Read<'_>],
    _invest: f64,
) -> f64 {
    if context.street == Street::River {
        return 1.0;
    }
    let position = math::position_category(
        context.player_count,
        context.position_from_button,
        context.is_small_blind,
        context.is_big_blind,
    );
    let mut factor = 0.88;
    if position == Position::Late {
        factor += 0.10;
    }
    if context.num_opponents > 1 {
        factor -= ((context.num_opponents - 1) as f64 * 0.045).min(0.18);
    }
    if texture.has_strong_draw {
        factor += 0.06;
    }
    if texture.wetness > 0.55 && context.effective_stack > context.pot * 5.0 {
        factor -= 0.06;
    }
    if reads.iter().any(|read| read.model.passivity() > 0.62) {
        factor += 0.035;
    }
    factor.clamp(0.66, 1.12)
}

/// Risk penalty (js/ai-core.js:1452-1464).
#[must_use]
pub fn risk_penalty(
    context: &DecisionContext,
    invest: f64,
    equity: &RangeEquity,
    texture: &Texture,
    action: &str,
    profile: &AiProfile,
) -> f64 {
    if invest <= 0.0 {
        return 0.0;
    }
    let stack_ratio = invest / context.hero_stack.max(1.0);
    let deep_ratio = context.effective_stack / context.big_blind.max(context.pot);
    let uncertainty = ((equity
        .standard_error
        .max(equity.std_dev / (equity.samples.max(1) as f64).sqrt()))
        * 2.0
        + (1.0 - equity.range_confidence) * 0.12)
        .clamp(0.03, 0.35);
    let mut penalty = invest * stack_ratio * profile.risk_aversion * (0.18 + uncertainty * 0.42);
    if context.num_opponents > 1 {
        penalty *= 1.0 + (context.num_opponents - 1) as f64 * 0.10;
    }
    if deep_ratio > 5.0 && !texture.has_strong_draw && equity.mean < 0.62 {
        penalty += invest * 0.045;
    }
    if stack_ratio > 0.65 && equity.mean < 0.64 {
        penalty += invest * (stack_ratio - 0.65) * 0.35;
    }
    if action == "call" && texture.wetness > 0.62 && equity.mean < 0.52 {
        penalty += invest * 0.035;
    }
    penalty
}

/// Raise target menu (js/ai-core.js:1465-1498).
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
pub fn raise_targets(
    context: &DecisionContext,
    texture: &Texture,
    target_focus: bool,
    preflop_participants: bool,
) -> Vec<f64> {
    if !context.can_raise || context.max_raise_to <= context.current_bet {
        return Vec::new();
    }
    let pot_after_call = context
        .big_blind
        .max(contestable_pot(context, context.to_call) + context.to_call);
    let live: Vec<_> = context
        .public_players
        .iter()
        .filter(|player| !player.folded && player.seat != context.hero_seat)
        .collect();
    let ceiling = if live.is_empty() {
        context.max_raise_to
    } else {
        let max_live = live
            .iter()
            .map(|player| player.round_bet + player.stack)
            .fold(context.current_bet, f64::max);
        context.max_raise_to.min(max_live)
    };
    let fractions: [f64; 3] = if context.street == Street::River {
        [0.33, 0.68, 1.08]
    } else if texture.wetness > 0.52 {
        [0.52, 0.75, 0.98]
    } else {
        [0.30, 0.55, 0.76]
    };
    let mut targets: Vec<f64> = fractions
        .iter()
        .map(|fraction| {
            context.hero_round_bet + context.to_call + (pot_after_call * fraction).round()
        })
        .collect();
    if context.street == Street::Preflop {
        targets.clear();
        let base = context.big_blind.max(context.current_bet);
        let position = math::position_category(
            context.player_count,
            context.position_from_button,
            context.is_small_blind,
            context.is_big_blind,
        );
        let out_of_position = if position == Position::Late {
            0.0
        } else {
            0.35
        };
        let multipliers: [f64; 2] = if context.preflop_raise_count > 0 {
            [2.7 + out_of_position, 3.4 + out_of_position]
        } else {
            [2.25, 2.8]
        };
        for multiplier in multipliers {
            targets.push((base * multiplier).round());
        }
        targets.push((context.current_bet + pot_after_call * 0.75).round());
        let depth = context.effective_stack / context.big_blind;
        if target_focus && context.preflop_raise_count == 0 && depth > 15.0 && depth <= 40.0 {
            let limpers = live
                .iter()
                .filter(|player| !player.is_big_blind && player.round_bet >= context.current_bet)
                .count() as f64;
            targets.clear();
            targets.push((context.big_blind * (2.0 + limpers)).round());
            targets.push((context.big_blind * (2.5 + limpers)).round());
        }
    }
    let streets = math::streets_remaining(context.street);
    let geometric = ((1.0 + 2.0 * context.effective_stack / pot_after_call)
        .powf(1.0 / f64::from(streets))
        - 1.0)
        / 2.0;
    if (0.25..=1.5).contains(&geometric) {
        targets.push(context.current_bet + (pot_after_call * geometric).round());
    }
    let depth = context.effective_stack / context.big_blind;
    let focus_open = target_focus
        && context.street == Street::Preflop
        && context.preflop_raise_count == 0
        && depth > 15.0
        && depth <= 40.0;
    if !focus_open {
        targets.push(ceiling);
    }
    let deep_preflop = preflop_participants && depth > 40.0 && context.preflop_raise_count < 2;
    let sizing_cap = if deep_preflop {
        Some(context.min_raise_to.max(
            context.big_blind.max(context.current_bet)
                * if context.preflop_raise_count > 0 {
                    4.5
                } else {
                    4.0
                },
        ))
    } else {
        None
    };
    let unit = context.wager_unit.round() as u64;
    let minimum = context.min_raise_to.round() as u64;
    let maximum = ceiling.round() as u64;
    let mut out: Vec<f64> = Vec::new();
    for target in targets {
        let target = if target >= context.max_raise_to && ceiling == context.max_raise_to {
            context.max_raise_to
        } else {
            let raw = target.min(ceiling);
            match poker_engine::snap_raise(raw, minimum, maximum, unit) {
                Some(snapped) => snapped as f64,
                None => continue,
            }
        };
        if sizing_cap.is_some_and(|cap| target > cap) {
            continue;
        }
        if target > context.current_bet && target > context.hero_round_bet && !out.contains(&target)
        {
            out.push(target);
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

/// Builds post-flop candidates (js/ai-core.js:1499-1552).
#[must_use]
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn build_postflop_candidates(
    context: &DecisionContext,
    equity: &RangeEquity,
    texture: &Texture,
    reads: &[Read<'_>],
    profile: &AiProfile,
    self_image: &SelfImage,
    plan: Option<&HandPlan>,
    target_focus: bool,
    preflop_participants: bool,
) -> Vec<PostflopCandidate> {
    let mean = equity.mean;
    let realization = realization_factor(context, texture, reads, 0.0);
    let mut candidates = Vec::new();
    if context.to_call > 0.0 && context.is_legal(ActionType::Fold) {
        candidates.push(PostflopCandidate {
            action: "fold".to_owned(),
            reason: "preserve_stack".to_owned(),
            ..PostflopCandidate::default()
        });
    }
    if context.to_call <= 0.0 && context.can_check && context.is_legal(ActionType::Check) {
        let future = contestable_return(context, 0.0, equity) * realization;
        candidates.push(PostflopCandidate {
            action: "check".to_owned(),
            ev: future,
            utility: future,
            reason: "realize_equity_or_control_pot".to_owned(),
            ..PostflopCandidate::default()
        });
    }
    if context.to_call > 0.0
        && (context.is_legal(ActionType::Call) || context.is_legal(ActionType::AllIn))
    {
        let invest = context.to_call.min(context.hero_stack);
        let response = if preflop_participants {
            Some(preflop_response(
                context,
                context.current_bet,
                "call",
                reads,
                equity,
                profile,
            ))
        } else {
            None
        };
        let mut info = equity.clone();
        if let Some(response) = &response {
            info.mean = conditional_called_equity(equity, response);
        }
        let factor = realization_factor(context, texture, reads, invest);
        let extra = response.as_ref().map_or(0.0, |response| {
            expected_additional_calls(context, context.current_bet, response)
        });
        let payment = response.as_ref().map_or(0.0, |_| info.mean * extra);
        let called_ev = contestable_return(context, invest, &info) * factor + payment - invest;
        let ev = response.as_ref().map_or(called_ev, |response| {
            (1.0 - response.any_raise) * called_ev - response.any_raise * invest * 0.6
        });
        let risk = risk_penalty(context, invest, &info, texture, "call", profile);
        candidates.push(PostflopCandidate {
            action: if invest >= context.hero_stack {
                "allin".to_owned()
            } else {
                "call".to_owned()
            },
            amount: invest.round() as u64,
            invest,
            ev,
            risk,
            utility: ev - risk,
            reason: if response.is_some() {
                "price_and_participating_ranges".to_owned()
            } else {
                "price_and_range_equity".to_owned()
            },
            called_equity: Some(info.mean),
            bluff_catch: context.street == Street::River && mean < 0.52,
            ..PostflopCandidate::default()
        });
    }
    if context.is_legal(ActionType::Raise) || context.is_legal(ActionType::AllIn) {
        for target in raise_targets(context, texture, target_focus, preflop_participants) {
            let invest = target - context.hero_round_bet;
            let response = if preflop_participants {
                preflop_response(context, target, "raise", reads, equity, profile)
            } else {
                response_for_raise(
                    context, target, mean, texture, reads, profile, self_image, plan,
                )
            };
            let expected_call = expected_additional_calls(context, target, &response);
            let selection = ((response.fraction - 0.20) * profile.called_range_discount
                + (1.0 - response.all_fold - response.any_raise) * 0.025)
                .clamp(0.0, 0.16);
            let conditional = preflop_participants;
            let mut info = equity.clone();
            if conditional {
                info.mean = conditional_called_equity(equity, &response);
            }
            let base_equity = if conditional {
                info.mean
            } else {
                mean + profile.conditional_caller_equity_weight
                    * (conditional_called_equity(equity, &response) - mean)
            };
            let called_equity = if conditional {
                base_equity
            } else {
                (base_equity
                    - selection * (1.0 - base_equity)
                    - if response.expected_callers > 1.15 {
                        0.025
                    } else {
                        0.0
                    })
                .clamp(0.001, 0.995)
            };
            let factor = if preflop_participants {
                realization_factor(context, texture, reads, invest)
            } else {
                1.0
            };
            let payment = called_equity * expected_call;
            let called_ev = (contestable_pot(context, invest) + invest).min(
                contestable_return(context, invest, &info) * (called_equity / info.mean.max(0.001)),
            ) * factor
                + payment
                - invest;
            let reraised_ev = if called_equity > 0.68 {
                called_ev * 0.72
            } else {
                -invest * (0.44 + response.any_raise * 0.24)
            };
            let ev = response.all_fold * contestable_pot(context, invest)
                + response.any_call * called_ev
                + response.any_raise * reraised_ev;
            let called_info = RangeEquity {
                mean: called_equity,
                ..info.clone()
            };
            let risk = risk_penalty(
                context,
                invest,
                &called_info,
                texture,
                if target >= context.max_raise_to {
                    "allin"
                } else {
                    "raise"
                },
                profile,
            );
            let mut deception =
                (response.credibility - 0.5) * context.pot * 0.035 * profile.image_awareness;
            if mean < 0.32 && !texture.has_strong_draw {
                deception -= context.pot * 0.025;
            }
            if reads.iter().any(|read| read.model.calls_too_much()) && mean > 0.57 {
                deception += context.pot * 0.022 * profile.thin_value_preference;
            }
            let action = if target >= context.max_raise_to {
                "allin"
            } else {
                "raise"
            };
            let reason = if called_equity > 0.62 {
                "value_pressure"
            } else if texture.has_strong_draw {
                "semi_bluff_pressure"
            } else {
                "credible_range_pressure"
            };
            candidates.push(PostflopCandidate {
                action: action.to_owned(),
                amount: if target >= context.max_raise_to {
                    context.hero_stack.round() as u64
                } else {
                    target.round() as u64
                },
                invest,
                ev,
                risk,
                utility: ev - risk + deception,
                reason: reason.to_owned(),
                called_equity: Some(called_equity),
                response: Some(response),
                ..PostflopCandidate::default()
            });
        }
    }
    let pot = context.pot;
    if let Some(plan) = plan {
        for candidate in &mut candidates {
            if candidate.terminal_call.is_none() {
                candidate.plan_bonus = plan.continuity_bonus(
                    matches!(candidate.action.as_str(), "raise" | "allin"),
                    context.to_call,
                    pot,
                );
                candidate.utility += candidate.plan_bonus;
            }
        }
    }
    candidates
}

fn spot_key(context: &DecisionContext, candidate: &PostflopCandidate) -> String {
    let fraction = candidate.invest / context.big_blind.max(context.pot + context.to_call);
    let bucket = if fraction <= 0.38 {
        "small"
    } else if fraction <= 0.78 {
        "medium"
    } else {
        "large"
    };
    format!(
        "{}:{}:{}:{}",
        context.street.as_str(),
        if context.to_call > 0.0 {
            "facing"
        } else {
            "open"
        },
        candidate.action,
        bucket
    )
}

/// Bounded mixing selection (js/ai-core.js:1557-1599).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn select_candidate(
    candidates: &mut [PostflopCandidate],
    context: &DecisionContext,
    rng: &mut AiRng,
    uncertainty: f64,
    profile: &AiProfile,
    spot_frequencies: &mut HashMap<String, f64>,
) -> PostflopCandidate {
    if let Some(terminal) = candidates
        .iter()
        .find(|c| c.terminal_call.as_deref() == Some("allin"))
    {
        return candidates
            .iter()
            .filter(|c| std::ptr::eq(*c, terminal) || c.action == "fold")
            .max_by(|a, b| a.ev.partial_cmp(&b.ev).unwrap_or(Ordering::Equal))
            .cloned()
            .unwrap_or_else(|| terminal.clone());
    }
    let mut filtered: Vec<PostflopCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.terminal_call.is_none() || candidate.ev >= 0.0)
        .cloned()
        .collect();
    if filtered.is_empty() {
        return PostflopCandidate {
            action: if context.to_call > 0.0 {
                "fold"
            } else {
                "check"
            }
            .to_owned(),
            reason: "fallback".to_owned(),
            ..PostflopCandidate::default()
        };
    }
    filtered.sort_by(|a, b| b.utility.partial_cmp(&a.utility).unwrap_or(Ordering::Equal));
    let mut best = filtered[0].clone();
    if best.utility < 0.0
        && let Some(free) = filtered
            .iter()
            .find(|candidate| matches!(candidate.action.as_str(), "fold" | "check"))
    {
        best = free.clone();
    }
    let gap_limit = (context.big_blind * 0.18)
        .max(context.pot * (0.03 + profile.uncertainty_tolerance * 0.018));
    let best_utility = best.utility;
    let eligible: Vec<&PostflopCandidate> = filtered
        .iter()
        .filter(|candidate| {
            best_utility - candidate.utility <= gap_limit
                && (candidate.utility >= -gap_limit * 0.35
                    || matches!(candidate.action.as_str(), "fold" | "check"))
        })
        .collect();
    if eligible.len() <= 1 {
        return best;
    }
    let temperature = (context.big_blind * 0.10).max(context.pot * 0.012)
        * (0.75 + uncertainty * profile.uncertainty_tolerance);
    let mut weights = Vec::with_capacity(eligible.len());
    let mut total = 0.0;
    for candidate in &eligible {
        let key = spot_key(context, candidate);
        let used = spot_frequencies.get(&key).copied().unwrap_or(0.0);
        let balance = 1.0 / (1.0 + used * 0.18).sqrt();
        let mut preference = 1.0;
        let pressure_low = candidate.reason == "credible_range_pressure"
            && candidate.called_equity.unwrap_or(0.5) < 0.45;
        if pressure_low || candidate.reason == "preflop_blocker_3bet" {
            preference = (profile.bluff_freq / 0.10).clamp(0.3, 2.2);
        }
        if candidate.bluff_catch {
            preference = (profile.hero_call_freq / 0.15).clamp(0.6, 2.0);
        }
        let weight = ((candidate.utility - best_utility) / temperature.max(0.01)).exp()
            * balance
            * preference;
        weights.push(weight);
        total += weight;
    }
    let mut roll = rng.next_f64() * total;
    let mut chosen = best.clone();
    for (index, candidate) in eligible.iter().enumerate() {
        roll -= weights[index];
        if roll <= 0.0 {
            chosen = (*candidate).clone();
            break;
        }
    }
    let key = spot_key(context, &chosen);
    *spot_frequencies.entry(key).or_insert(0.0) += 1.0;
    chosen
}
