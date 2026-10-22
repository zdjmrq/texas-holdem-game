//! Short-deck pre-flop decision stage (port of
//! `js/ai-shortdeck-stable.js` `preflopDecision` production branch,
//! `stable:950-1019`).
//!
//! The short-deck core only implements the range-equity branch (an evaluator
//! always exists in the Rust port). The legacy no-evaluator heuristic branch is
//! intentionally not ported: it is unreachable in production, the adapter and
//! the golden corpus (`js/ai-shortdeck-stable.js:950-951`).

use crate::input::DecisionContext;
use crate::math::Position;
use crate::rng::AiRng;

use super::ShortDeckBrain;
use super::StageResult;
use super::equity::estimate_range_equity;
use super::features::{analyze_texture, hand_strength, position_category};
use super::postflop::{build_postflop_candidates, select_candidate};
use super::types::Candidate;

/// Runs the short-deck pre-flop stage.
pub(crate) fn preflop_decision(
    brain: &mut ShortDeckBrain,
    context: &DecisionContext,
    rng: &mut AiRng,
    reads: &[usize],
) -> StageResult {
    let equity = estimate_range_equity(context, &brain.beliefs, rng);
    let texture = analyze_texture(&[], &context.hole_cards, context.variant);
    let position = position_category(context);
    let strength = if context.hole_cards.len() >= 2 {
        hand_strength(
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
    #[allow(clippy::cast_precision_loss)]
    let facing_raises = f64::from(context.preflop_raise_count) * 0.07;
    let style = (0.25 - brain.profile.vpip) * 0.40 + (0.55 - brain.profile.aggression) * 0.08;
    let credible_steal = context.num_opponents == 1
        && reads.iter().any(|seat| {
            brain
                .model(*seat)
                .is_some_and(|model| model.folds_too_much())
        })
        && strength > 0.28;
    let unfiltered = build_postflop_candidates(brain, context, &equity, &texture, reads);
    let threshold_with_style = threshold + facing_raises + style;
    let mut candidates: Vec<Candidate> = unfiltered
        .iter()
        .filter(|candidate| {
            candidate.action == "fold"
                || candidate.action == "check"
                || candidate.action == "call"
                || (candidate.action == "allin" && candidate.invest <= context.to_call)
                || strength >= threshold_with_style
                || credible_steal
        })
        .cloned()
        .collect();
    let uncertainty = match equity.standard_error {
        Some(value) if value != 0.0 => value,
        _ => 0.05,
    };
    let decision = select_candidate(brain, &mut candidates, context, rng, uncertainty);
    StageResult {
        decision,
        equity,
        texture,
        candidates,
        raw_candidates: unfiltered,
    }
}
