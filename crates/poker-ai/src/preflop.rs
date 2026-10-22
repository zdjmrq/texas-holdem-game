//! Pre-flop decision pipeline (port of js/ai-core.js preflopDecision).

use std::collections::HashMap;

use poker_engine::ActionType;

use crate::belief::OpponentBelief;
use crate::equity::{RangeEquity, estimate_range_equity};
use crate::input::DecisionContext;
use crate::math::{self, Position, Texture};
use crate::model::BehavioralModel;
use crate::plan::HandPlan;
use crate::postflop::{PostflopCandidate, Read, build_postflop_candidates, select_candidate};
use crate::profile::AiProfile;
use crate::rng::AiRng;
use crate::self_image::SelfImage;

/// Result of the pre-flop decision stage.
#[derive(Clone, Debug, PartialEq)]
pub struct PreflopResult {
    /// Range equity estimate (includes response samples).
    pub equity: RangeEquity,
    /// Empty-board texture.
    pub texture: Texture,
    /// Filtered candidates (sorted by selection order before mixing).
    pub candidates: Vec<PostflopCandidate>,
    /// Selected candidate after bounded mixing.
    pub decision: PostflopCandidate,
}

/// Runs the pre-flop decision stage (production branch with evaluator).
#[must_use]
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::cast_precision_loss
)]
pub fn preflop_decision(
    profile: &AiProfile,
    context: &DecisionContext,
    beliefs: &HashMap<usize, OpponentBelief>,
    models: &HashMap<usize, BehavioralModel>,
    self_image: &SelfImage,
    plan: Option<&HandPlan>,
    spot_frequencies: &mut HashMap<String, f64>,
    rng: &mut AiRng,
    target_focus: bool,
) -> PreflopResult {
    let participants = profile.preflop_participation_weight > 0.0;
    let equity = estimate_range_equity(context, beliefs, models, rng, participants);
    let texture = math::analyze_texture(&[], &context.hole_cards, context.variant);
    let position = math::position_category(
        context.player_count,
        context.position_from_button,
        context.is_small_blind,
        context.is_big_blind,
    );
    let strength = if context.hole_cards.len() == 2 {
        math::starting_hand_strength(
            context.hole_cards[0],
            context.hole_cards[1],
            context.variant,
            position,
        )
    } else {
        0.0
    };
    let threshold = match position {
        Position::Late => 0.37,
        Position::Blind => 0.43,
        _ => 0.48,
    };
    let facing_raises = f64::from(context.preflop_raise_count) * 0.07;
    let style = (0.25 - profile.vpip) * 0.40 + (0.55 - profile.aggression) * 0.08;
    let reads: Vec<Read<'_>> = context
        .active_opponent_seats
        .iter()
        .filter_map(|seat| {
            let belief = beliefs.get(seat)?;
            let model = models.get(seat)?;
            Some(Read {
                seat: *seat,
                belief,
                model,
            })
        })
        .collect();
    let credible_steal = context.num_opponents == 1
        && reads.iter().any(|read| read.model.folds_too_much())
        && strength > 0.28;
    let cards = math::combo_features(
        context.hole_cards[0],
        context.hole_cards[1],
        context.variant,
    );
    let opening_width = if context.player_count == 2 {
        (0.55 + profile.vpip - 0.24).clamp(0.40, 0.85)
    } else {
        let multiplier = match position {
            Position::Early => 0.75,
            Position::Late => 1.4,
            Position::Blind => 0.95,
            Position::Middle => 1.0,
        };
        (profile.vpip * multiplier).clamp(0.10, 0.65)
    };
    let opening_cutoff = math::starting_range_cutoff(opening_width, context.variant);
    let opening_raise_cutoff = math::starting_range_cutoff(
        (opening_width * profile.pfr).clamp(0.025, 0.65),
        context.variant,
    );
    let price = context.to_call / context.big_blind;
    let raise_count = context.preflop_raise_count;
    let defense_width = if raise_count >= 3 {
        (0.04 + (profile.vpip - 0.24) * 0.10).clamp(0.025, 0.08)
    } else if raise_count >= 2 {
        ((if position == Position::Late {
            0.12
        } else {
            0.09
        }) + (profile.vpip - 0.24) * 0.15
            + (4.0 - price).max(0.0) * 0.015
            - (price - 5.0).max(0.0) * 0.005)
            .clamp(0.025, 0.20)
    } else {
        ((if context.is_big_blind {
            0.45
        } else if context.is_small_blind {
            0.20
        } else if position == Position::Late {
            0.23
        } else if position == Position::Early {
            0.18
        } else {
            0.20
        }) + (profile.vpip - 0.24) * 0.20
            + (2.5 - price).max(0.0) * 0.03)
            .clamp(0.10, 0.60)
    };
    let value_cutoff = math::starting_range_cutoff(
        ((if raise_count >= 2 { 0.012 } else { 0.025 }) + profile.three_bet_freq * 0.15)
            .clamp(0.02, 0.075),
        context.variant,
    );
    let mut candidates = build_postflop_candidates(
        context,
        &equity,
        &texture,
        &reads,
        profile,
        self_image,
        plan,
        target_focus,
        participants,
    );
    let mut filtered: Vec<PostflopCandidate> = candidates
        .drain(..)
        .filter(|candidate| {
            let action = candidate.action.as_str();
            if action == "fold" || action == "check" {
                return true;
            }
            if action == "call" || (action == "allin" && candidate.invest <= context.to_call) {
                if participants && raise_count > 0 {
                    return cards.strength
                        >= math::starting_range_cutoff(defense_width, context.variant);
                }
                if participants && raise_count == 0 && !context.is_big_blind {
                    let width = if context.is_small_blind {
                        (opening_width * 1.4).clamp(0.20, 0.65)
                    } else {
                        opening_width
                    };
                    return cards.strength >= math::starting_range_cutoff(width, context.variant);
                }
                return true;
            }
            if participants && raise_count == 0 && !credible_steal {
                return cards.strength >= opening_cutoff.max(opening_raise_cutoff);
            }
            if participants && raise_count > 0 {
                let value = cards.strength >= value_cutoff;
                let bluff = raise_count == 1
                    && position == Position::Late
                    && cards.suited
                    && (cards.ace_blocker || cards.king_blocker)
                    && candidate
                        .response
                        .as_ref()
                        .is_some_and(|response| response.all_fold > 0.55)
                    && profile.three_bet_freq >= 0.06;
                return value || bluff;
            }
            strength >= threshold + facing_raises + style || credible_steal
        })
        .collect();
    let uncertainty = equity.standard_error.max(0.05);
    let decision = select_candidate(
        &mut filtered,
        context,
        rng,
        uncertainty,
        profile,
        spot_frequencies,
    );
    PreflopResult {
        equity,
        texture,
        candidates: filtered,
        decision,
    }
}

/// True when the hero may raise in this context.
#[must_use]
pub fn can_raise(context: &DecisionContext) -> bool {
    context.can_raise && context.is_legal(ActionType::Raise)
}
