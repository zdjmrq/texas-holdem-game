//! Frozen 6+ short-deck strategy (port of `js/ai-shortdeck-stable.js`, the
//! v1.5.6 stable core).
//!
//! The stable core is an independent branch copy of the standard AI: it keeps
//! the same decision pipeline shape (range equity -> legal candidate values ->
//! risk/response -> bounded mixing) but has its own position categories,
//! opponent belief updates, pre-flop candidate filter, trace shape and no river
//! CFR. `docs/specs/ai-strategy.md` section 9 lists every difference; constants
//! and line references in this module tree are relative to
//! `js/ai-shortdeck-stable.js`.
//!
//! Public interface (frozen by task-9): [`ShortDeckBrain::new`] plus the
//! [`crate::Brain`] implementation. The rest of the module tree mirrors the JS
//! private functions. Golden replay: `crates/poker-ai/tests/shortdeck_golden.rs`.

mod belief;
mod equity;
mod features;
mod model;
mod plan;
mod postflop;
mod preflop;
mod snapshot;
mod types;

use std::collections::HashMap;

use poker_engine::ActionType;

use crate::input::{DecisionContext, ObservedAction, ShowdownResult, Street};
use crate::profile::{AiProfile, Style};
use crate::rng::AiRng;
use crate::self_image::SelfImage;
use crate::trace::{
    DecisionTrace, TraceCandidate, TraceChosen, TraceEquity, TraceOpponent, TraceParameters,
    TracePlan, TraceResponse, TraceSelfImage,
};
use crate::{Brain, DecisionWithTrace};

use belief::OpponentBelief;
use model::BehavioralModel;
use plan::HandPlan;
use types::{Candidate, RangeEquity};

pub use snapshot::{
    ShortDeckSnapshot, ShortDeckSnapshotCandidate, ShortDeckSnapshotEquity,
    ShortDeckSnapshotOpponent, ShortDeckSnapshotResponse,
};

/// Short-deck decision core (v1.5.6 frozen policy).
#[derive(Clone, Debug)]
pub struct ShortDeckBrain {
    /// Effective short-deck profile.
    pub profile: AiProfile,
    /// Personality id.
    pub style: Style,
    /// Own seat.
    pub seat_id: usize,
    /// Current hand id.
    pub current_hand_id: Option<u64>,
    /// Current hand plan.
    plan: Option<HandPlan>,
    /// Per-hand opponent beliefs.
    beliefs: HashMap<usize, OpponentBelief>,
    /// Cross-hand behavioural models.
    models: HashMap<usize, BehavioralModel>,
    /// Own public image.
    self_image: SelfImage,
    /// Spot mixing frequencies (bounded by decision count).
    spot_frequencies: HashMap<String, f64>,
    /// Trace of the last decision.
    last_trace: Option<DecisionTrace>,
    /// Unrounded snapshot of the last decision (golden-corpus diagnostics).
    last_snapshot: Option<ShortDeckSnapshot>,
}

/// Intermediate result of one pre-flop or post-flop stage.
struct StageResult {
    /// Selected action before legalize.
    decision: Candidate,
    /// Range equity estimate.
    equity: RangeEquity,
    /// Board texture.
    texture: crate::math::Texture,
    /// Candidates consumed by the trace (sorted in place like the JS array).
    candidates: Vec<Candidate>,
    /// Raw candidate capture: the unfiltered builder output for pre-flop (the
    /// JS capture wraps `buildPostflopCandidates`), the sorted candidate list
    /// for post-flop.
    raw_candidates: Vec<Candidate>,
}

impl ShortDeckBrain {
    /// Creates the short-deck brain with an already resolved profile.
    #[must_use]
    pub fn new(profile: AiProfile, style: Style, seat_id: usize) -> Self {
        Self {
            profile,
            style,
            seat_id,
            current_hand_id: None,
            plan: None,
            beliefs: HashMap::new(),
            models: HashMap::new(),
            self_image: SelfImage::new(),
            spot_frequencies: HashMap::new(),
            last_trace: None,
            last_snapshot: None,
        }
    }

    /// Seed text for the decision RNG
    /// (JS `${raw.seed ?? 'table'}:${handId}:${seatId}:${decisionId}:${style}`).
    #[must_use]
    pub fn seed_text(&self, context: &DecisionContext) -> String {
        let table_seed = if context.seed.is_empty() {
            "table"
        } else {
            context.seed.as_str()
        };
        format!(
            "{table_seed}:{}:{}:{}:{}",
            context.hand_id,
            self.seat_id,
            context.decision_id,
            self.style.as_str()
        )
    }

    /// Unrounded snapshot of the last decision, for golden-corpus replay
    /// diagnostics only. It deliberately mirrors the JS oracle's raw capture
    /// (`golden-ai.js buildRawSnapshot`).
    #[must_use]
    pub fn last_snapshot(&self) -> Option<&ShortDeckSnapshot> {
        self.last_snapshot.as_ref()
    }

    /// Ensures a belief/model pair exists for a seat and refreshes the profile
    /// weights consumed by belief combo weights (JS `ensureOpponent`).
    fn ensure_opponent(&mut self, seat_id: usize) {
        let range_evidence_weight = self.profile.range_evidence_weight;
        let percentile_weight = self.profile.range_percentile_weight;
        let belief = self
            .beliefs
            .entry(seat_id)
            .or_insert_with(|| OpponentBelief::new(seat_id));
        belief.evidence_weight = range_evidence_weight;
        belief.percentile_weight = percentile_weight;
        self.models
            .entry(seat_id)
            .or_insert_with(|| BehavioralModel::new(seat_id));
    }

    /// Accessor used by the sibling pipeline modules.
    #[must_use]
    pub(crate) fn model(&self, seat: usize) -> Option<&BehavioralModel> {
        self.models.get(&seat)
    }

    /// Accessor used by the sibling pipeline modules.
    #[must_use]
    pub(crate) fn belief(&self, seat: usize) -> Option<&OpponentBelief> {
        self.beliefs.get(&seat)
    }

    /// JS `activeReads(context)`: ensures a belief/model entry for every live
    /// opponent and returns the seat list in context order.
    fn active_reads(&mut self, context: &DecisionContext) -> Vec<usize> {
        let seats = context.active_opponent_seats.clone();
        for seat in &seats {
            self.ensure_opponent(*seat);
        }
        seats
    }

    /// Runs the post-flop stage (JS `decide` else-branch).
    fn postflop_stage(
        &mut self,
        context: &DecisionContext,
        rng: &mut AiRng,
        reads: &[usize],
    ) -> StageResult {
        let equity = equity::estimate_range_equity(context, &self.beliefs, rng);
        let texture = features::analyze_texture(
            &context.community_cards,
            &context.hole_cards,
            context.variant,
        );
        let mut candidates =
            postflop::build_postflop_candidates(self, context, &equity, &texture, reads);
        let uncertainty = equity
            .standard_error
            .unwrap_or(equity.std_dev * (1.0 - equity.confidence));
        let decision = postflop::select_candidate(self, &mut candidates, context, rng, uncertainty);
        let raw_candidates = candidates.clone();
        StageResult {
            decision,
            equity,
            texture,
            candidates,
            raw_candidates,
        }
    }

    /// Builds the JS `lastTrace` object (short-deck stable shape: parameters
    /// carry only variant/rangeEvidenceWeight/calledRangeDiscount and there are
    /// no river CFR fields).
    fn build_trace(
        &self,
        context: &DecisionContext,
        stage: &StageResult,
        chosen: &Candidate,
        intent: &str,
        rng_calls: u64,
    ) -> DecisionTrace {
        let image = self.self_image.snapshot();
        let opponents: Vec<TraceOpponent> = context
            .active_opponent_seats
            .iter()
            .map(|seat| {
                let belief = self.belief(*seat);
                let model = self.model(*seat);
                let range_width = belief.map_or(0.0, |belief| belief.range_width);
                let estimated_strength = belief.map_or(0.0, OpponentBelief::estimated_strength);
                let confidence = (belief.map_or(0.0, |belief| belief.confidence)
                    + model.map_or(0.0, BehavioralModel::confidence))
                    / 2.0;
                TraceOpponent {
                    seat: *seat,
                    range_width: types::fixed(range_width, 3),
                    estimated_strength: types::fixed(estimated_strength, 3),
                    confidence: types::fixed(confidence, 3),
                    folds_too_much: model.is_some_and(BehavioralModel::folds_too_much),
                    calls_too_much: model.is_some_and(BehavioralModel::calls_too_much),
                }
            })
            .collect();
        let mut candidates: Vec<TraceCandidate> = stage
            .candidates
            .iter()
            .map(|candidate| TraceCandidate {
                action: candidate.action.clone(),
                amount: types::amount_u64(candidate.amount),
                ev: types::fixed(candidate.ev, 2),
                risk: types::fixed(candidate.risk, 2),
                utility: types::fixed(candidate.utility, 2),
                reason: candidate.reason.clone(),
                terminal_call: None,
                called_equity: None,
                response: candidate.response.as_ref().map(|response| TraceResponse {
                    all_fold: response.all_fold,
                    any_call: response.any_call,
                    any_raise: response.any_raise,
                    expected_callers: response.expected_callers,
                }),
            })
            .collect();
        candidates.sort_by(|a, b| b.utility.total_cmp(&a.utility));
        let parameters = TraceParameters {
            variant: "shortdeck".to_owned(),
            range_evidence_weight: self.profile.range_evidence_weight,
            called_range_discount: self.profile.called_range_discount,
            ..TraceParameters::default()
        };
        let equity = &stage.equity;
        DecisionTrace {
            strategy_version: "unified-v2".to_owned(),
            parameters,
            hand_id: context.hand_id,
            decision_id: context.decision_id.clone(),
            intent: intent.to_owned(),
            equity: TraceEquity {
                mean: types::fixed(equity.mean, 4),
                std_dev: types::fixed(equity.std_dev, 4),
                confidence: types::fixed(equity.confidence, 4),
                samples: equity.samples as u64,
                timed_out: equity.timed_out,
                standard_error: types::fixed(equity.standard_error.unwrap_or(0.0), 4),
                range_confidence: types::fixed(equity.range_confidence.unwrap_or(0.12), 4),
                exact: equity.exact,
            },
            pot_odds: types::fixed(
                context.to_call / (context.pot + context.to_call).max(1.0),
                4,
            ),
            self_image: TraceSelfImage {
                tightness: image.tightness,
                aggression: image.aggression,
                river_aggression: image.river_aggression,
                shown_bluff_rate: image.shown_bluff_rate,
                recently_bluffed: image.recently_bluffed,
            },
            opponents,
            candidates,
            river_cfr_enabled: false,
            river_solver: None,
            chosen: TraceChosen {
                action: chosen.action.clone(),
                amount: types::amount_u64(chosen.amount),
                utility: types::fixed(chosen.utility, 2),
            },
            next_plan: self.plan.as_ref().map_or_else(
                || TracePlan {
                    id: format!("plan-{}", context.hand_id),
                    ..TracePlan::default()
                },
                |plan| TracePlan {
                    id: plan.id.clone(),
                    intent: plan.intent.clone(),
                    max_risk: plan.max_risk,
                    continue_conditions: plan.continue_conditions.clone(),
                    abort_conditions: plan.abort_conditions.clone(),
                    response_to_raise: plan.response_to_raise.clone(),
                },
            ),
            rng_calls,
        }
    }
}

impl Brain for ShortDeckBrain {
    fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace {
        if self.current_hand_id != Some(context.hand_id) || self.plan.is_none() {
            self.begin_hand(context.hand_id);
        }
        let seed = self.seed_text(context);
        let mut rng = AiRng::from_seed(&seed);
        let reads = self.active_reads(context);
        let stage = if context.street == Street::Preflop {
            preflop::preflop_decision(self, context, &mut rng, &reads)
        } else {
            self.postflop_stage(context, &mut rng, &reads)
        };
        let prelegalize = stage.decision.clone();
        let chosen = postflop::legalize(&stage.decision, context);
        let response = chosen.response.clone();
        // The stable profile keeps conditionalCallerEquityWeight at zero, so the
        // plan equity is always the plain range-equity mean.
        let plan_equity = if self.profile.conditional_caller_equity_weight > 0.0 {
            chosen.called_equity.unwrap_or(stage.equity.mean)
        } else {
            stage.equity.mean
        };
        if let Some(plan) = self.plan.as_mut() {
            plan.update(
                context,
                &chosen,
                plan_equity,
                &stage.texture,
                response.as_ref(),
            );
        }
        let intent = self
            .plan
            .as_ref()
            .map_or_else(|| "observe".to_owned(), |plan| plan.intent.clone());
        let amount = types::amount_u64(chosen.amount);
        let action = ActionType::from_name(&chosen.action).unwrap_or({
            if context.to_call > 0.0 {
                ActionType::Fold
            } else {
                ActionType::Check
            }
        });
        self.self_image.record(
            context.hand_id,
            context.street.as_str(),
            &chosen.action,
            amount,
            &intent,
        );
        let trace = self.build_trace(context, &stage, &chosen, &intent, rng.calls());
        self.last_trace = Some(trace.clone());
        self.last_snapshot = Some(snapshot::capture(&stage, &prelegalize));
        DecisionWithTrace {
            action,
            amount,
            trace,
        }
    }

    fn observe_action(&mut self, event: &ObservedAction) {
        if event.seat_id == self.seat_id {
            // The stable core ignores its own public actions.
            return;
        }
        if self.current_hand_id != Some(event.hand_id) {
            self.begin_hand(event.hand_id);
        }
        self.ensure_opponent(event.seat_id);
        let model_read = {
            let model = self
                .models
                .get_mut(&event.seat_id)
                .expect("model ensured before observe");
            model.observe(event);
            (model.vpip(), model.aggression_rate())
        };
        let belief = self
            .beliefs
            .get_mut(&event.seat_id)
            .expect("belief ensured before observe");
        belief.observe(event, model_read.0, model_read.1);
    }

    fn observe_showdown(&mut self, seat: usize, result: &ShowdownResult) {
        if seat == self.seat_id {
            self.self_image.observe_showdown(result);
        } else {
            self.ensure_opponent(seat);
            if let Some(model) = self.models.get_mut(&seat) {
                model.observe_showdown(result);
            }
        }
    }

    fn begin_hand(&mut self, hand_id: u64) {
        self.current_hand_id = Some(hand_id);
        self.plan = Some(HandPlan::new(hand_id));
        for belief in self.beliefs.values_mut() {
            belief.begin_hand();
        }
    }
}
