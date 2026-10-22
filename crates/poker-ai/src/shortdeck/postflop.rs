//! Short-deck post-flop pipeline: candidate generation, opponent responses,
//! risk penalties, raise targeting, bounded mixing and legalization.
//!
//! Ports `js/ai-shortdeck-stable.js`:
//! * `realizationFactor` `stable:1020-1030`
//! * `storyCredibility` `stable:1031-1045`
//! * `responseForRaise` `stable:1046-1093`
//! * `riskPenalty` `stable:1094-1106`
//! * `raiseTargets` `stable:1107-1133`
//! * `buildPostflopCandidates` `stable:1134-1175`
//! * `spotKey` / `selectCandidate` `stable:1176-1212`
//! * `legalize` `stable:1213-1239`

use poker_engine::ActionType;

use crate::input::{DecisionContext, Street};
use crate::math::{Position, Texture};
use crate::profile::AiProfile;

use super::ShortDeckBrain;
use super::equity::{
    conditional_called_equity, contestable_pot, contestable_return, expected_additional_calls,
};
use super::features::position_category;
use super::types::{Candidate, RangeEquity, Response, ResponseOpponent, SizeBucket, clamp};

/// JS `rules.snapRaise(amount, minimum, maximum, unit)` with float inputs.
///
/// The original helper is `Math.max(bounds.min, Math.min(bounds.max,
/// Math.round(amount / unit) * unit))`; amounts in the AI remain floats until
/// the engine adapter rounds them, so the float version is kept here.
#[must_use]
fn snap_raise_js(amount: f64, minimum: f64, maximum: f64, unit: f64) -> Option<f64> {
    let unit = if unit.is_finite() {
        unit.max(1.0).floor()
    } else {
        1.0
    };
    let min = (minimum / unit).ceil() * unit;
    let max = (maximum / unit).floor() * unit;
    if min > max || !amount.is_finite() {
        return None;
    }
    Some(((amount / unit).round() * unit).clamp(min, max))
}

/// JS `betFractionForTarget(context, target)` (`stable:812-815`).
#[must_use]
fn bet_fraction_for_target(context: &DecisionContext, target: f64) -> f64 {
    let invest = (target - context.hero_round_bet).max(0.0);
    invest / context.big_blind.max(context.pot + context.to_call)
}

/// JS `realizationFactor(context, texture, reads)` (`stable:1020-1030`).
#[must_use]
pub(crate) fn realization_factor(
    brain: &ShortDeckBrain,
    context: &DecisionContext,
    texture: &Texture,
    reads: &[usize],
) -> f64 {
    if context.street == Street::River {
        return 1.0;
    }
    let mut factor = 0.88;
    if position_category(context) == Position::Late {
        factor += 0.10;
    }
    if context.num_opponents > 1 {
        #[allow(clippy::cast_precision_loss)]
        {
            factor -= ((context.num_opponents - 1) as f64 * 0.045).min(0.18);
        }
    }
    if texture.has_strong_draw {
        factor += 0.06;
    }
    if texture.wetness > 0.55 && context.effective_stack > context.pot * 5.0 {
        factor -= 0.06;
    }
    if reads.iter().any(|seat| {
        brain
            .model(*seat)
            .is_some_and(|model| model.passivity() > 0.62)
    }) {
        factor += 0.035;
    }
    clamp(factor, 0.66, 1.12)
}

/// JS `storyCredibility(context, texture, action, equity, reads)`
/// (`stable:1031-1045`).
#[must_use]
pub(crate) fn story_credibility(
    brain: &ShortDeckBrain,
    context: &DecisionContext,
    texture: &Texture,
    action: &str,
    equity: f64,
    reads: &[usize],
) -> f64 {
    if action != "raise" && action != "allin" {
        return 0.5;
    }
    let image = brain.self_image.snapshot();
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
        score += 0.10 * brain.profile.image_awareness;
    }
    if image.aggression > 0.68 {
        score -= 0.07 * brain.profile.image_awareness;
    }
    if brain
        .plan
        .as_ref()
        .is_some_and(|plan| plan.intent == "bluff")
        && !texture.pressure_card
        && !texture.has_strong_draw
    {
        score -= 0.09;
    }
    if reads
        .iter()
        .all(|seat| brain.belief(*seat).is_some_and(|belief| belief.capped))
    {
        score += 0.08;
    }
    clamp(score, 0.12, 0.90)
}

/// JS `responseForRaise(context, target, equity, texture, reads)`
/// (`stable:1046-1093`).
#[must_use]
pub(crate) fn response_for_raise(
    brain: &ShortDeckBrain,
    context: &DecisionContext,
    target: f64,
    equity: f64,
    texture: &Texture,
    reads: &[usize],
) -> Response {
    let fraction = bet_fraction_for_target(context, target);
    let credibility = story_credibility(brain, context, texture, "raise", equity, reads);
    let mut all_fold = 1.0;
    let mut no_raise = 1.0;
    let mut confidence = 0.0;
    let mut conditional_callers = 0.0;
    let mut conditional_all_fold = 1.0;
    let mut opponents: Vec<ResponseOpponent> = Vec::new();
    for seat in reads {
        let seat = *seat;
        let player = context
            .public_players
            .iter()
            .find(|player| player.seat == seat);
        if player.is_some_and(|player| player.all_in || player.stack == 0.0) {
            opponents.push(ResponseOpponent {
                seat,
                fold: 0.0,
                call: 1.0,
                raise: 0.0,
            });
            conditional_callers += 1.0;
            conditional_all_fold = 0.0;
            all_fold = 0.0;
            continue;
        }
        let Some(model) = brain.model(seat) else {
            continue;
        };
        let Some(belief) = brain.belief(seat) else {
            continue;
        };
        let mut fold = model.fold_rate_for(fraction, Street::Flop);
        if context.street == Street::Preflop {
            fold += brain.profile.preflop_response_weight
                * (model.fold_rate_for(fraction, Street::Preflop) - fold);
        }
        fold += (fraction - 0.55) * 0.13;
        fold += (credibility - 0.5) * 0.20;
        fold -= (belief.estimated_strength() - 0.5) * 0.32;
        if model.calls_too_much() {
            fold -= 0.13;
        }
        if model.folds_too_much() {
            fold += 0.11;
        }
        if context.num_opponents > 1 {
            fold -= 0.025;
        }
        fold = clamp(fold, 0.04, 0.86);
        let raise = clamp(
            (model.aggression_rate() - 0.30) * 0.18
                + (belief.estimated_strength() - 0.58).max(0.0) * 0.25
                + fraction * 0.025,
            0.015,
            0.26,
        );
        // Each opponent chooses exactly one of fold/call/raise.
        fold = fold.min(1.0 - raise);
        let call = 1.0 - fold - raise;
        opponents.push(ResponseOpponent {
            seat,
            fold,
            call,
            raise,
        });
        all_fold *= fold;
        no_raise *= 1.0 - raise;
        conditional_callers += call / (1.0 - raise);
        conditional_all_fold *= fold / (1.0 - raise);
        confidence += model.confidence() * 0.55 + belief.confidence * 0.45;
    }
    Response {
        all_fold,
        any_raise: 1.0 - no_raise,
        any_call: (no_raise - all_fold).max(0.0),
        expected_callers: conditional_callers / (1.0 - conditional_all_fold).max(1e-9),
        credibility,
        confidence: if reads.is_empty() {
            0.2
        } else {
            #[allow(clippy::cast_precision_loss)]
            {
                confidence / reads.len() as f64
            }
        },
        fraction,
        opponents,
    }
}

/// JS `riskPenalty(context, invest, equityInfo, texture, action)`
/// (`stable:1094-1106`).
#[must_use]
pub(crate) fn risk_penalty(
    profile: &AiProfile,
    context: &DecisionContext,
    invest: f64,
    equity: &RangeEquity,
    texture: &Texture,
    action: &str,
) -> f64 {
    if invest <= 0.0 {
        return 0.0;
    }
    let stack_ratio = invest / context.hero_stack.max(1.0);
    let deep_ratio = context.effective_stack / context.big_blind.max(context.pot);
    let standard_error = equity
        .standard_error
        .unwrap_or_else(|| equity.std_dev / (equity.samples.max(1) as f64).sqrt());
    let range_confidence = equity.range_confidence.unwrap_or(0.3);
    let uncertainty = clamp(
        standard_error * 2.0 + (1.0 - range_confidence) * 0.12,
        0.03,
        0.35,
    );
    let mut penalty = invest * stack_ratio * profile.risk_aversion * (0.18 + uncertainty * 0.42);
    if context.num_opponents > 1 {
        #[allow(clippy::cast_precision_loss)]
        {
            penalty *= 1.0 + (context.num_opponents - 1) as f64 * 0.10;
        }
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

/// JS `raiseTargets(context, texture)` (`stable:1107-1133`).
#[must_use]
pub(crate) fn raise_targets(context: &DecisionContext, texture: &Texture) -> Vec<f64> {
    if !context.can_raise || context.max_raise_to <= context.current_bet {
        return Vec::new();
    }
    let pot_after_call =
        (contestable_pot(context, context.to_call) + context.to_call).max(context.big_blind);
    let live: Vec<&crate::input::PublicPlayer> = context
        .public_players
        .iter()
        .filter(|player| !player.folded && player.seat != context.hero_seat)
        .collect();
    let ceiling = if live.is_empty() {
        context.max_raise_to
    } else {
        let mut max_stack: f64 = context.current_bet;
        for player in &live {
            max_stack = max_stack.max(player.round_bet + player.stack);
        }
        context.max_raise_to.min(max_stack)
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
        let out_of_position = if position_category(context) == Position::Late {
            0.0
        } else {
            0.35
        };
        let multipliers: [f64; 2] = if context.preflop_raise_count > 0 {
            [2.7 + out_of_position, 3.4 + out_of_position]
        } else {
            [2.25, 2.8]
        };
        targets.extend(
            multipliers
                .iter()
                .map(|multiplier| (base * multiplier).round()),
        );
        targets.push((context.current_bet + pot_after_call * 0.75).round());
    }
    let streets = match context.street {
        Street::Flop => 3.0_f64,
        Street::Turn => 2.0,
        _ => 1.0,
    };
    let geometric =
        ((1.0 + 2.0 * context.effective_stack / pot_after_call).powf(1.0 / streets) - 1.0) / 2.0;
    if (0.25..=1.5).contains(&geometric) {
        targets.push(context.current_bet + (pot_after_call * geometric).round());
    }
    targets.push(ceiling);
    let mut snapped: Vec<f64> = Vec::new();
    for target in targets {
        let mapped = if target >= context.max_raise_to && ceiling == context.max_raise_to {
            Some(context.max_raise_to)
        } else {
            snap_raise_js(
                target.min(ceiling),
                context.min_raise_to,
                ceiling,
                context.wager_unit,
            )
        };
        if let Some(value) = mapped
            && !snapped.contains(&value)
        {
            snapped.push(value);
        }
    }
    let mut result: Vec<f64> = snapped
        .into_iter()
        .filter(|target| *target > context.current_bet && *target > context.hero_round_bet)
        .collect();
    result.sort_by(f64::total_cmp);
    result
}

/// Builds post-flop candidates (JS `buildPostflopCandidates`,
/// `stable:1134-1175`).
#[must_use]
pub(crate) fn build_postflop_candidates(
    brain: &ShortDeckBrain,
    context: &DecisionContext,
    equity_info: &RangeEquity,
    texture: &Texture,
    reads: &[usize],
) -> Vec<Candidate> {
    let equity = equity_info.mean;
    let realization = realization_factor(brain, context, texture, reads);
    let mut candidates: Vec<Candidate> = Vec::new();
    if context.to_call > 0.0 && context.is_legal(ActionType::Fold) {
        candidates.push(Candidate {
            action: "fold".to_owned(),
            reason: "preserve_stack".to_owned(),
            ..Candidate::default()
        });
    }
    if context.to_call == 0.0 && context.can_check && context.is_legal(ActionType::Check) {
        let future = contestable_return(context, 0.0, equity_info) * realization;
        candidates.push(Candidate {
            action: "check".to_owned(),
            ev: future,
            utility: future,
            reason: "realize_equity_or_control_pot".to_owned(),
            ..Candidate::default()
        });
    }
    if context.to_call > 0.0
        && (context.is_legal(ActionType::Call) || context.is_legal(ActionType::AllIn))
    {
        let invest = context.to_call.min(context.hero_stack);
        let ev = contestable_return(context, invest, equity_info) * realization - invest;
        let risk = risk_penalty(
            &brain.profile,
            context,
            invest,
            equity_info,
            texture,
            "call",
        );
        candidates.push(Candidate {
            action: if invest >= context.hero_stack {
                "allin".to_owned()
            } else {
                "call".to_owned()
            },
            amount: invest,
            invest,
            ev,
            risk,
            utility: ev - risk,
            reason: "price_and_range_equity".to_owned(),
            ..Candidate::default()
        });
    }
    if context.is_legal(ActionType::Raise) || context.is_legal(ActionType::AllIn) {
        for target in raise_targets(context, texture) {
            let invest = target - context.hero_round_bet;
            let response = response_for_raise(brain, context, target, equity, texture, reads);
            let expected_call = expected_additional_calls(context, target, &response);
            let selection = clamp(
                (response.fraction - 0.20) * brain.profile.called_range_discount
                    + (1.0 - response.all_fold - response.any_raise) * 0.025,
                0.0,
                0.16,
            );
            let subset_equity = conditional_called_equity(equity_info, &response);
            let base_equity =
                equity + brain.profile.conditional_caller_equity_weight * (subset_equity - equity);
            let called_equity = clamp(
                base_equity
                    - selection * (1.0 - base_equity)
                    - if response.expected_callers > 1.15 {
                        0.025
                    } else {
                        0.0
                    },
                0.001,
                0.995,
            );
            let called_ev = (contestable_pot(context, invest) + invest).min(
                contestable_return(context, invest, equity_info)
                    * (called_equity / equity.max(0.001)),
            ) + called_equity * expected_call
                - invest;
            let reraised_ev = if equity > 0.68 {
                called_ev * 0.72
            } else {
                -invest * (0.44 + response.any_raise * 0.24)
            };
            let ev = response.all_fold * contestable_pot(context, invest)
                + response.any_call * called_ev
                + response.any_raise * reraised_ev;
            let action = if target >= context.max_raise_to {
                "allin"
            } else {
                "raise"
            };
            let risk = risk_penalty(
                &brain.profile,
                context,
                invest,
                equity_info,
                texture,
                action,
            );
            let mut deception =
                (response.credibility - 0.5) * context.pot * 0.035 * brain.profile.image_awareness;
            if equity < 0.32 && !texture.has_strong_draw {
                deception -= context.pot * 0.025;
            }
            if equity > 0.57
                && reads.iter().any(|seat| {
                    brain
                        .model(*seat)
                        .is_some_and(|model| model.calls_too_much())
                })
            {
                deception += context.pot * 0.022 * brain.profile.thin_value_preference;
            }
            let reason = if called_equity > 0.62 {
                "value_pressure"
            } else if texture.has_strong_draw {
                "semi_bluff_pressure"
            } else {
                "credible_range_pressure"
            };
            candidates.push(Candidate {
                action: action.to_owned(),
                amount: if action == "allin" {
                    context.hero_stack
                } else {
                    target
                },
                invest,
                ev,
                risk,
                utility: ev - risk + deception,
                reason: reason.to_owned(),
                called_equity: Some(called_equity),
                response: Some(response),
                plan_bonus: 0.0,
            });
        }
    }
    for candidate in &mut candidates {
        candidate.plan_bonus = brain.plan.as_ref().map_or(0.0, |plan| {
            plan.continuity_bonus(candidate, context, context.pot)
        });
        candidate.utility += candidate.plan_bonus;
    }
    candidates
}

/// JS `spotKey(context, candidate)` (`stable:1176-1179`).
#[must_use]
pub(crate) fn spot_key(context: &DecisionContext, candidate: &Candidate) -> String {
    let fraction = candidate.invest / context.big_blind.max(context.pot + context.to_call);
    let facing = if context.to_call > 0.0 {
        "facing"
    } else {
        "open"
    };
    let bucket = match SizeBucket::of(fraction) {
        SizeBucket::Small => "small",
        SizeBucket::Medium => "medium",
        SizeBucket::Large => "large",
    };
    format!(
        "{}:{facing}:{}:{bucket}",
        context.street.as_str(),
        candidate.action
    )
}

/// JS `selectCandidate(candidates, context, rng, uncertainty = 0.3)`
/// (`stable:1180-1212`).
///
/// Sorts the candidate slice in place (the caller keeps the sorted slice for
/// the trace, exactly like the JS reference array) and returns the selected
/// candidate.
pub(crate) fn select_candidate(
    brain: &mut ShortDeckBrain,
    candidates: &mut [Candidate],
    context: &DecisionContext,
    rng: &mut crate::rng::AiRng,
    uncertainty: f64,
) -> Candidate {
    if candidates.is_empty() {
        return Candidate {
            action: if context.to_call > 0.0 {
                "fold".to_owned()
            } else {
                "check".to_owned()
            },
            reason: "fallback".to_owned(),
            ..Candidate::default()
        };
    }
    candidates.sort_by(|a, b| b.utility.total_cmp(&a.utility));
    let mut best_index = 0_usize;
    if candidates[0].utility < 0.0
        && let Some(index) = candidates
            .iter()
            .position(|candidate| candidate.action == "fold" || candidate.action == "check")
    {
        best_index = index;
    }
    let best_utility = candidates[best_index].utility;
    let gap_limit = (context.big_blind * 0.18)
        .max(context.pot * (0.03 + brain.profile.uncertainty_tolerance * 0.018));
    let eligible: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| {
            best_utility - candidate.utility <= gap_limit
                && (candidate.utility >= -gap_limit * 0.35
                    || candidate.action == "fold"
                    || candidate.action == "check")
        })
        .map(|(index, _)| index)
        .collect();
    if eligible.len() <= 1 {
        return candidates[best_index].clone();
    }
    let temperature = (context.big_blind * 0.10).max(context.pot * 0.012)
        * (0.75 + uncertainty * brain.profile.uncertainty_tolerance);
    let mut weights: Vec<f64> = Vec::with_capacity(eligible.len());
    let mut total = 0.0;
    for index in &eligible {
        let key = spot_key(context, &candidates[*index]);
        let used = brain.spot_frequencies.get(&key).copied().unwrap_or(0.0);
        let balance = 1.0 / (1.0 + used * 0.18).sqrt();
        let weight =
            ((candidates[*index].utility - best_utility) / temperature.max(0.01)).exp() * balance;
        weights.push(weight);
        total += weight;
    }
    let roll = rng.next_f64() * total;
    let mut chosen_index = best_index;
    let mut remaining = roll;
    for (position, index) in eligible.iter().enumerate() {
        remaining -= weights[position];
        if remaining <= 0.0 {
            chosen_index = *index;
            break;
        }
    }
    let key = spot_key(context, &candidates[chosen_index]);
    let count = brain.spot_frequencies.get(&key).copied().unwrap_or(0.0);
    brain.spot_frequencies.insert(key, count + 1.0);
    candidates[chosen_index].clone()
}

/// JS `legalize(decision, context)` (`stable:1213-1239`).
#[must_use]
pub(crate) fn legalize(decision: &Candidate, context: &DecisionContext) -> Candidate {
    let has =
        |name: &str| ActionType::from_name(name).is_some_and(|action| context.is_legal(action));
    let mut action = if decision.action.is_empty() {
        if context.to_call > 0.0 {
            "fold"
        } else {
            "check"
        }
        .to_owned()
    } else {
        decision.action.clone()
    };
    let mut amount = decision.amount.max(0.0).round();
    if !has(&action) {
        if action == "allin" && has("raise") && context.max_raise_to > context.current_bet {
            action = "raise".to_owned();
            amount = context.max_raise_to;
        } else if context.to_call > 0.0 && has("call") {
            action = "call".to_owned();
            amount = context.to_call.min(context.hero_stack);
        } else if has("check") {
            action = "check".to_owned();
            amount = 0.0;
        } else {
            action = "fold".to_owned();
            amount = 0.0;
        }
    }
    if action == "call" {
        amount = context.to_call.min(context.hero_stack);
    }
    if action == "raise" {
        amount = amount.max(context.min_raise_to).min(context.max_raise_to);
        if amount >= context.max_raise_to && has("allin") {
            action = "allin".to_owned();
            amount = context.hero_stack;
        } else {
            match snap_raise_js(
                amount,
                context.min_raise_to,
                context.max_raise_to,
                context.wager_unit,
            ) {
                None => {
                    action = if has("call") {
                        "call"
                    } else if has("check") {
                        "check"
                    } else {
                        "fold"
                    }
                    .to_owned();
                    amount = if action == "call" {
                        context.to_call.min(context.hero_stack)
                    } else {
                        0.0
                    };
                }
                Some(snapped) => amount = snapped,
            }
        }
    }
    if action == "allin" {
        amount = context.hero_stack;
    }
    if action == "fold" || action == "check" {
        amount = 0.0;
    }
    let mut result = decision.clone();
    result.action = action;
    result.amount = amount;
    result
}
