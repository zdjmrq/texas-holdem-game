//! Production adapter: observation routing and cross-decision state. Mirrors
//! the original AIPlayer (js/ai.js:341-1017) around the shared brain.

use std::collections::HashMap;

use poker_core::{Card, Variant, evaluate};
use poker_engine::ActionType;

use crate::belief::OpponentBelief;
use crate::equity::{RangeEquity, estimate_range_equity};
use crate::input::{DecisionContext, ObservedAction, ShowdownResult, Street};
use crate::math;
use crate::model::BehavioralModel;
use crate::plan::HandPlan;
use crate::postflop::{PostflopCandidate, Read, build_postflop_candidates, select_candidate};
use crate::preflop::preflop_decision;
use crate::profile::{AiProfile, ProfileSource, Style, profile_for};
use crate::rng::AiRng;
use crate::self_image::SelfImage;
use crate::shortdeck::ShortDeckBrain;
use crate::trace::{
    DecisionTrace, TraceCandidate, TraceChosen, TraceEquity, TraceOpponent, TraceParameters,
    TracePlan, TraceResponse, TraceSelfImage,
};
use crate::{Brain, DecisionWithTrace};

/// The original standard-mode brain (js/ai-core.js UnifiedPokerAI).
#[derive(Clone, Debug)]
pub struct StandardBrain {
    /// Effective profile.
    pub profile: AiProfile,
    /// Own seat.
    pub seat_id: usize,
    /// Style id (feeds the decision seed string).
    pub style: Style,
    /// Game variant (always Standard for this brain).
    pub variant: Variant,
    /// Per-hand opponent beliefs.
    pub beliefs: HashMap<usize, OpponentBelief>,
    /// Cross-hand behavioural models.
    pub models: HashMap<usize, BehavioralModel>,
    /// Own public image.
    pub self_image: SelfImage,
    /// Current hand plan.
    pub plan: Option<HandPlan>,
    /// Current hand id (detects hand boundaries).
    pub current_hand_id: Option<u64>,
    /// Spot mixing frequencies.
    pub spot_frequencies: HashMap<String, f64>,
    /// Public self belief used as the hero range for river CFR.
    pub public_self_belief: Option<OpponentBelief>,
    /// Public self behavioural model.
    pub public_self_model: Option<BehavioralModel>,
    /// Last river public action seen (for the checked-before flag).
    pub last_river_public_action: Option<(usize, String)>,
}

impl StandardBrain {
    /// Creates a standard brain.
    #[must_use]
    pub fn new(style: Style, seat_id: usize) -> Self {
        Self {
            profile: profile_for(style, Variant::Standard, ProfileSource::StandardStyle),
            seat_id,
            style,
            variant: Variant::Standard,
            beliefs: HashMap::new(),
            models: HashMap::new(),
            self_image: SelfImage::new(),
            plan: None,
            current_hand_id: None,
            spot_frequencies: HashMap::new(),
            public_self_belief: None,
            public_self_model: None,
            last_river_public_action: None,
        }
    }

    /// Ensures belief/model entries exist and applies the profile weights.
    pub fn ensure_opponent(&mut self, seat_id: usize) {
        self.beliefs
            .entry(seat_id)
            .or_insert_with(|| OpponentBelief::new(seat_id));
        self.models
            .entry(seat_id)
            .or_insert_with(|| BehavioralModel::new(seat_id));
        if let Some(belief) = self.beliefs.get_mut(&seat_id) {
            belief.configure(
                self.profile.range_evidence_weight,
                self.profile.range_percentile_weight,
                self.profile.preflop_range_sharpness,
                self.profile.observed_preflop_range_weight,
            );
        }
    }

    /// Seed string for the decision RNG.
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

    fn reads(&self, context: &DecisionContext) -> Vec<Read<'_>> {
        context
            .active_opponent_seats
            .iter()
            .filter_map(|seat| {
                let belief = self.beliefs.get(seat)?;
                let model = self.models.get(seat)?;
                Some(Read {
                    seat: *seat,
                    belief,
                    model,
                })
            })
            .collect()
    }

    fn observe_self(&mut self, event: &ObservedAction) {
        if self.public_self_belief.is_none() {
            self.public_self_belief = Some(OpponentBelief::new(self.seat_id));
            self.public_self_model = Some(BehavioralModel::new(self.seat_id));
        }
        let (Some(model), Some(belief)) = (
            self.public_self_model.as_mut(),
            self.public_self_belief.as_mut(),
        ) else {
            return;
        };
        model.observe(event);
        belief.observe(event, model);
        if event.street == Street::River {
            self.last_river_public_action = Some((event.seat_id, event.action.clone()));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn decide_pipeline(
        &mut self,
        context: &DecisionContext,
    ) -> (
        PostflopCandidate,
        Vec<PostflopCandidate>,
        RangeEquity,
        math::Texture,
        DecisionTrace,
    ) {
        let target_focus = context.strategy_focus() == crate::input::StrategyFocus::TenPlayer25Bb;
        let mut rng = AiRng::from_seed(&self.seed_text(context));
        for seat in &context.active_opponent_seats {
            self.ensure_opponent(*seat);
        }
        let mut river_trace = None;
        let (candidates, chosen, equity, texture) = if context.street == Street::Preflop {
            let result = preflop_decision(
                &self.profile,
                context,
                &self.beliefs,
                &self.models,
                &self.self_image,
                self.plan.as_ref(),
                &mut self.spot_frequencies,
                &mut rng,
                target_focus,
            );
            (
                result.candidates,
                result.decision,
                result.equity,
                result.texture,
            )
        } else {
            let equity =
                estimate_range_equity(context, &self.beliefs, &self.models, &mut rng, false);
            let texture = math::analyze_texture(
                &context.community_cards,
                &context.hole_cards,
                context.variant,
            );
            let reads = self.reads(context);
            let mut candidates = build_postflop_candidates(
                context,
                &equity,
                &texture,
                &reads,
                &self.profile,
                &self.self_image,
                self.plan.as_ref(),
                target_focus,
                false,
            );
            let uncertainty = if equity.standard_error > 0.0 {
                equity.standard_error
            } else {
                equity.std_dev * (1.0 - equity.confidence)
            };
            let chosen = select_candidate(
                &mut candidates,
                context,
                &mut rng,
                uncertainty,
                &self.profile,
                &mut self.spot_frequencies,
            );
            (candidates, chosen, equity, texture)
        };
        let mut chosen = chosen;
        let mut candidates = candidates;
        if let Some((river_decision, trace)) = self.try_solve_river(context, &mut rng) {
            chosen = PostflopCandidate {
                action: action_name(river_decision.action).to_owned(),
                amount: river_decision.amount.round().max(0.0) as u64,
                invest: (river_decision.amount - context.hero_round_bet).max(0.0),
                ev: river_decision.ev,
                utility: river_decision.ev,
                reason: "public-range-river-cfr".to_owned(),
                ..PostflopCandidate::default()
            };
            candidates = vec![chosen.clone()];
            river_trace = Some(trace);
        }
        let (action, amount) = self.legalize_decision(context, &chosen);
        let chosen_name = action_name(action);
        let plan_equity = chosen.called_equity.unwrap_or(equity.mean);
        let response_confidence = chosen
            .response
            .as_ref()
            .map_or(0.0, |response| response.confidence);
        if let Some(plan) = self.plan.as_mut() {
            plan.update(
                context.street,
                matches!(action, ActionType::Raise | ActionType::AllIn),
                chosen_name,
                plan_equity,
                texture.has_strong_draw,
                texture.has_flush_draw,
                texture.has_straight_draw,
                context.to_call,
                context.hero_stack,
                &context.active_opponent_seats,
                response_confidence,
                amount,
            );
        }
        let intent = self
            .plan
            .as_ref()
            .map_or("observe".to_owned(), |plan| plan.intent.as_str().to_owned());
        self.self_image.record(
            context.hand_id,
            context.street.as_str(),
            chosen_name,
            amount,
            &intent,
        );
        let trace = self.build_trace(
            context,
            &chosen,
            &candidates,
            &equity,
            &texture,
            chosen_name,
            amount,
            rng.calls(),
            river_trace,
            intent,
        );
        (chosen, candidates, equity, texture, trace)
    }

    fn legalize_decision(
        &self,
        context: &DecisionContext,
        candidate: &PostflopCandidate,
    ) -> (ActionType, u64) {
        let action = match candidate.action.as_str() {
            "fold" => ActionType::Fold,
            "check" => ActionType::Check,
            "call" => ActionType::Call,
            "raise" => ActionType::Raise,
            "allin" => ActionType::AllIn,
            _ => {
                if context.to_call > 0.0 {
                    ActionType::Fold
                } else {
                    ActionType::Check
                }
            }
        };
        AIPlayer::legalize(context, action, candidate.amount as f64)
    }

    #[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
    fn build_trace(
        &self,
        context: &DecisionContext,
        chosen: &PostflopCandidate,
        candidates: &[PostflopCandidate],
        equity: &RangeEquity,
        texture: &math::Texture,
        action_name: &str,
        amount: u64,
        rng_calls: u64,
        river_trace: Option<crate::input::RiverTrace>,
        intent: String,
    ) -> DecisionTrace {
        let _ = texture;
        let focus = context.strategy_focus();
        let profile = &self.profile;
        let image = self.self_image.snapshot();
        let reads = self.reads(context);
        let mut trace_candidates: Vec<TraceCandidate> = candidates
            .iter()
            .map(|candidate| TraceCandidate {
                action: candidate.action.clone(),
                amount: candidate.amount,
                ev: round2(candidate.ev),
                risk: round2(candidate.risk),
                utility: round2(candidate.utility),
                reason: candidate.reason.clone(),
                terminal_call: candidate.terminal_call.clone(),
                called_equity: candidate.called_equity.map(round4),
                response: candidate.response.as_ref().map(|response| TraceResponse {
                    all_fold: round4(response.all_fold),
                    any_call: round4(response.any_call),
                    any_raise: round4(response.any_raise),
                    expected_callers: round3(response.expected_callers),
                }),
            })
            .collect();
        trace_candidates.sort_by(|a, b| {
            b.utility
                .partial_cmp(&a.utility)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let plan = self
            .plan
            .clone()
            .unwrap_or_else(|| HandPlan::new(context.hand_id));
        DecisionTrace {
            strategy_version: "unified-v2".to_owned(),
            parameters: TraceParameters {
                variant: if context.variant.is_short_deck() {
                    "shortdeck"
                } else {
                    "standard"
                }
                .to_owned(),
                strategy_focus: focus.as_str().to_owned(),
                range_evidence_weight: profile.range_evidence_weight,
                called_range_discount: profile.called_range_discount,
                pre_flop_participation_weight: profile.preflop_participation_weight,
                pre_flop_range_sharpness: profile.preflop_range_sharpness,
                observed_pre_flop_range_weight: profile.observed_preflop_range_weight,
                professional_weight: profile.professional_weight,
                post_flop_selection_weight: profile.postflop_selection_weight,
                joint_call_return_weight: profile.joint_call_return_weight,
                competence_weight: profile.competence_weight,
                target_precision_weight: profile.target_precision_weight,
                conditional_risk_weight: profile.conditional_risk_weight,
                terminal_realization_weight: profile.terminal_realization_weight,
                ranked_continuation_weight: profile.ranked_continuation_weight,
                terminal_call_weight: profile.terminal_call_weight,
                river_call_weight: profile.river_call_weight,
            },
            hand_id: context.hand_id,
            decision_id: context.decision_id.clone(),
            intent,
            equity: TraceEquity {
                mean: round4(equity.mean),
                std_dev: round4(equity.std_dev),
                confidence: round4(equity.confidence),
                samples: equity.samples as u64,
                timed_out: equity.timed_out,
                standard_error: round4(equity.standard_error),
                range_confidence: round4(equity.range_confidence),
                exact: equity.exact,
            },
            pot_odds: round4(context.to_call / (context.pot + context.to_call).max(1.0)),
            self_image: TraceSelfImage {
                tightness: image.tightness,
                aggression: image.aggression,
                river_aggression: image.river_aggression,
                shown_bluff_rate: image.shown_bluff_rate,
                recently_bluffed: image.recently_bluffed,
            },
            opponents: reads
                .iter()
                .map(|read| TraceOpponent {
                    seat: read.seat,
                    range_width: round3(read.belief.range_width),
                    estimated_strength: round3(read.belief.estimated_strength()),
                    confidence: round3((read.belief.confidence + read.model.confidence()) / 2.0),
                    folds_too_much: read.model.folds_too_much(),
                    calls_too_much: read.model.calls_too_much(),
                })
                .collect(),
            candidates: trace_candidates,
            river_cfr_enabled: context.variant == Variant::Standard
                && profile.river_cfr_weight > 0.0,
            river_solver: river_trace,
            chosen: TraceChosen {
                action: action_name.to_owned(),
                amount,
                utility: round2(chosen.utility),
            },
            next_plan: TracePlan {
                id: plan.id.clone(),
                intent: plan.intent.as_str().to_owned(),
                max_risk: plan.max_risk,
                continue_conditions: plan.continue_conditions.clone(),
                abort_conditions: plan.abort_conditions.clone(),
                response_to_raise: plan.response_to_raise.clone(),
            },
            rng_calls,
        }
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn try_solve_river(
        &mut self,
        context: &DecisionContext,
        rng: &mut AiRng,
    ) -> Option<(crate::input::RiverDecision, crate::input::RiverTrace)> {
        if !crate::river::is_available()
            || self.profile.river_cfr_weight <= 0.0
            || context.strategy_focus() != crate::input::StrategyFocus::TenPlayer25Bb
            || context.street != Street::River
            || context.active_opponent_seats.len() != 1
            || context.hole_cards.len() != 2
            || context.community_cards.len() != 5
        {
            return None;
        }
        let hero = *context
            .public_players
            .iter()
            .find(|player| player.seat == context.hero_seat)?;
        let villain_seat = context.active_opponent_seats[0];
        let villain = *context
            .public_players
            .iter()
            .find(|player| player.seat == villain_seat)?;
        if hero.all_in || villain.all_in || hero.stack <= context.to_call || villain.stack <= 0.0 {
            return None;
        }
        if context
            .public_players
            .iter()
            .any(|player| !player.folded && player.seat != hero.seat && player.seat != villain.seat)
        {
            return None;
        }
        if (hero.committed - hero.round_bet - (villain.committed - villain.round_bet)).abs() > 1e-9
        {
            return None;
        }
        if (crate::postflop::contestable_pot(context, context.to_call) - context.pot).abs() > 1e-9
            || context.pot - hero.round_bet - villain.round_bet <= 0.0
        {
            return None;
        }
        if self.public_self_belief.is_none() {
            self.public_self_belief = Some(OpponentBelief::new(self.seat_id));
            self.public_self_model = Some(BehavioralModel::new(self.seat_id));
        }
        if let Some(belief) = self.public_self_belief.as_mut() {
            belief.configure(
                self.profile.range_evidence_weight,
                self.profile.range_percentile_weight,
                self.profile.preflop_range_sharpness,
                self.profile.observed_preflop_range_weight,
            );
        }
        let opponent = self.beliefs.get(&villain_seat)?;
        let self_belief = self.public_self_belief.as_ref()?;
        let self_model = self.public_self_model.as_ref();
        let board: [Card; 5] = [
            context.community_cards[0],
            context.community_cards[1],
            context.community_cards[2],
            context.community_cards[3],
            context.community_cards[4],
        ];
        let deck: Vec<Card> = math::deck_cards(Variant::Standard)
            .into_iter()
            .filter(|card| !board.contains(card))
            .collect();
        let mut feature_cache: HashMap<String, math::BoardComboFeatures> = HashMap::new();
        let mut weights = Vec::with_capacity(deck.len() * (deck.len() - 1) / 2);
        for i in 0..deck.len() {
            for j in (i + 1)..deck.len() {
                let self_weight = self_belief.combo_weight(
                    deck[i],
                    deck[j],
                    Variant::Standard,
                    self_model,
                    &mut feature_cache,
                );
                let opponent_weight = opponent.combo_weight(
                    deck[i],
                    deck[j],
                    Variant::Standard,
                    self.models.get(&villain_seat),
                    &mut feature_cache,
                );
                weights.push((self_weight, opponent_weight));
            }
        }
        let order = |player: &crate::input::PublicPlayer| -> i32 {
            if player.position_from_button == Some(0) {
                context.player_count as i32
            } else {
                player.position_from_button.unwrap_or(i32::MAX)
            }
        };
        let checked_before = context.current_bet == 0.0
            && if let Some((seat, action)) = &self.last_river_public_action {
                *seat == villain_seat && action == "check"
            } else {
                hero.position_from_button.is_some()
                    && villain.position_from_button.is_some()
                    && order(&hero) > order(&villain)
            };
        let request = crate::input::RiverRequest {
            board,
            deck,
            pot: context.pot,
            paid: [hero.round_bet, villain.round_bet],
            stacks: [hero.stack, villain.stack],
            current_bet: context.current_bet,
            min_raise_to: context.min_raise_to,
            min_bet: context.big_blind,
            unit: context.wager_unit,
            checked_before,
            can_raise: context.can_raise,
            iterations: 800,
            weights,
        };
        let solution = crate::river::solve(&request).ok()?;
        let score = evaluate(
            &[
                context.hole_cards[0],
                context.hole_cards[1],
                board[0],
                board[1],
                board[2],
                board[3],
                board[4],
            ],
            Variant::Standard,
        )
        .score();
        let roll = rng.next_f64();
        let decision = crate::river::select_decision(&solution, context, score, roll).ok()?;
        let trace = decision.trace.clone();
        Some((decision, trace))
    }
}

fn action_name(action: ActionType) -> &'static str {
    match action {
        ActionType::Fold => "fold",
        ActionType::Check => "check",
        ActionType::Call => "call",
        ActionType::Raise => "raise",
        ActionType::AllIn => "allin",
    }
}

impl Brain for StandardBrain {
    fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace {
        if self.current_hand_id != Some(context.hand_id) || self.plan.is_none() {
            self.begin_hand(context.hand_id);
        }
        let (_chosen, _candidates, _equity, _texture, trace) = self.decide_pipeline(context);
        let action = match trace.chosen.action.as_str() {
            "fold" => ActionType::Fold,
            "check" => ActionType::Check,
            "call" => ActionType::Call,
            "raise" => ActionType::Raise,
            "allin" => ActionType::AllIn,
            _ => ActionType::Fold,
        };
        DecisionWithTrace {
            action,
            amount: trace.chosen.amount,
            trace,
        }
    }

    fn observe_action(&mut self, event: &ObservedAction) {
        // JS observeAction: a new hand id on an observation starts the hand
        // before the event is folded into beliefs/models.
        if self.current_hand_id != Some(event.hand_id) {
            self.begin_hand(event.hand_id);
        }
        if event.seat_id == self.seat_id {
            self.observe_self(event);
            return;
        }
        self.ensure_opponent(event.seat_id);
        if let Some(model) = self.models.get_mut(&event.seat_id) {
            model.observe(event);
        }
        if let (Some(belief), Some(model)) = (
            self.beliefs.get_mut(&event.seat_id),
            self.models.get(&event.seat_id),
        ) {
            belief.observe(event, model);
        }
        if event.street == Street::River {
            self.last_river_public_action = Some((event.seat_id, event.action.clone()));
        }
    }

    fn observe_showdown(&mut self, seat: usize, result: &ShowdownResult) {
        if seat == self.seat_id {
            self.self_image.observe_showdown(result);
        } else if let Some(model) = self.models.get_mut(&seat) {
            model.observe_showdown(result);
        }
    }

    fn begin_hand(&mut self, hand_id: u64) {
        self.current_hand_id = Some(hand_id);
        self.plan = Some(HandPlan::new(hand_id));
        for belief in self.beliefs.values_mut() {
            belief.begin_hand();
            belief.configure(
                self.profile.range_evidence_weight,
                self.profile.range_percentile_weight,
                self.profile.preflop_range_sharpness,
                self.profile.observed_preflop_range_weight,
            );
        }
        if let Some(belief) = self.public_self_belief.as_mut() {
            belief.begin_hand();
        }
        self.last_river_public_action = None;
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}
fn round4(value: f64) -> f64 {
    (value * 10000.0).round() / 10000.0
}

/// Production AI player adapter (js/ai.js AIPlayer).
#[derive(Clone, Debug)]
pub struct AIPlayer {
    /// Display name (identity only, never used by the strategy).
    pub name: String,
    /// Personality id.
    pub style: Style,
    /// Effective profile.
    pub profile: AiProfile,
    /// Seat index.
    pub seat_id: usize,
    /// Variant.
    pub variant: Variant,
    /// Own hole cards (set by the engine before each decision).
    pub hole_cards: Vec<Card>,
    /// Remaining stack.
    pub stack: f64,
    /// Chips committed to the pot over the hand.
    pub chips_in_pot: f64,
    /// Seat index alias used by the original adapter.
    pub position: usize,
    /// Dealt-in player count.
    pub num_players: usize,
    /// Big blind (short deck: minimum bet).
    pub big_blind: f64,
    /// Short-deck flag.
    pub is_short_deck: bool,
    /// Decision engine (standard or short-deck).
    pub brain: BrainImpl,
    /// Trace of the last decision.
    pub last_trace: Option<DecisionTrace>,
}

/// Concrete brain variants (enum instead of trait object keeps Clone).
#[derive(Clone, Debug)]
pub enum BrainImpl {
    /// Standard unified core.
    Standard(Box<StandardBrain>),
    /// Frozen short-deck core.
    ShortDeck(Box<ShortDeckBrain>),
}

impl BrainImpl {
    fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace {
        match self {
            BrainImpl::Standard(brain) => brain.decide(context),
            BrainImpl::ShortDeck(brain) => brain.decide(context),
        }
    }

    fn observe_action(&mut self, event: &ObservedAction) {
        match self {
            BrainImpl::Standard(brain) => brain.observe_action(event),
            BrainImpl::ShortDeck(brain) => brain.observe_action(event),
        }
    }

    fn observe_showdown(&mut self, seat: usize, result: &ShowdownResult) {
        match self {
            BrainImpl::Standard(brain) => brain.observe_showdown(seat, result),
            BrainImpl::ShortDeck(brain) => brain.observe_showdown(seat, result),
        }
    }
}

impl AIPlayer {
    /// Creates an AI player for one of the three original runtime paths.
    #[must_use]
    pub fn new(
        name: &str,
        style: Style,
        starting_stack: f64,
        seat_id: usize,
        variant: Variant,
        source: ProfileSource,
    ) -> Self {
        let profile = profile_for(style, variant, source);
        let brain = if variant == Variant::ShortDeck {
            BrainImpl::ShortDeck(Box::new(ShortDeckBrain::new(profile, style, seat_id)))
        } else {
            BrainImpl::Standard(Box::new(StandardBrain::new(style, seat_id)))
        };
        Self {
            name: name.to_owned(),
            style,
            profile,
            seat_id,
            variant,
            hole_cards: Vec::new(),
            stack: starting_stack,
            chips_in_pot: 0.0,
            position: seat_id,
            num_players: 6,
            big_blind: 80.0,
            is_short_deck: variant == Variant::ShortDeck,
            brain,
            last_trace: None,
        }
    }

    /// Sets the hole cards before a decision.
    pub fn set_hole_cards(&mut self, cards: &[Card]) {
        self.hole_cards = cards.to_vec();
    }

    /// Last decision trace, when a decision has been made.
    #[must_use]
    pub fn last_trace(&self) -> Option<&DecisionTrace> {
        self.last_trace.as_ref()
    }

    /// Decides an action for the current public context.
    pub fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace {
        let mut joined = context.clone();
        if joined.hole_cards.is_empty() {
            joined.hole_cards = self.hole_cards.clone();
        }
        let decision = self.brain.decide(&joined);
        self.last_trace = Some(decision.trace.clone());
        decision
    }

    /// Records one public action (routes self events like the JS adapter).
    pub fn record_opponent_action(&mut self, event: &ObservedAction) {
        self.brain.observe_action(event);
    }

    /// Records a showdown result.
    pub fn record_showdown(&mut self, seat: usize, result: &ShowdownResult) {
        self.brain.observe_showdown(seat, result);
    }

    /// Starts a hand explicitly (also triggered by decide/observe internally).
    pub fn begin_hand(&mut self, hand_id: u64) {
        match &mut self.brain {
            BrainImpl::Standard(brain) => brain.begin_hand(hand_id),
            BrainImpl::ShortDeck(brain) => brain.begin_hand(hand_id),
        }
    }

    /// Legalizes a decision against the context (port of js/ai-core.js legalize).
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn legalize(
        context: &DecisionContext,
        mut action: ActionType,
        mut amount: f64,
    ) -> (ActionType, u64) {
        if !context.is_legal(action) {
            if action == ActionType::AllIn
                && context.is_legal(ActionType::Raise)
                && context.max_raise_to > context.current_bet
            {
                action = ActionType::Raise;
                amount = context.max_raise_to;
            } else if context.to_call > 0.0 && context.is_legal(ActionType::Call) {
                action = ActionType::Call;
                amount = context.to_call.min(context.hero_stack);
            } else if context.is_legal(ActionType::Check) {
                action = ActionType::Check;
                amount = 0.0;
            } else {
                action = ActionType::Fold;
                amount = 0.0;
            }
        }
        let mut amount = amount.max(0.0).round() as u64;
        if action == ActionType::Call {
            amount = context.to_call.min(context.hero_stack).round() as u64;
        }
        if action == ActionType::Raise {
            let minimum = context.min_raise_to.round() as u64;
            let maximum = context.max_raise_to.round() as u64;
            amount = amount.clamp(minimum, maximum.max(minimum));
            if amount >= maximum && context.is_legal(ActionType::AllIn) {
                action = ActionType::AllIn;
                amount = context.hero_stack.round() as u64;
            } else if let Some(snapped) = poker_engine::snap_raise(
                amount as f64,
                minimum,
                maximum,
                context.wager_unit.round() as u64,
            ) {
                amount = snapped;
            }
        }
        if action == ActionType::AllIn {
            amount = context.hero_stack.round() as u64;
        }
        if matches!(action, ActionType::Fold | ActionType::Check) {
            amount = 0;
        }
        (action, amount)
    }
}
