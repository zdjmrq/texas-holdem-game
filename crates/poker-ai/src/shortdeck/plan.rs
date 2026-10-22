//! Per-hand short-deck plan (port of `js/ai-shortdeck-stable.js` `HandPlan`,
//! `stable:490-542`). The plan is recreated at every hand boundary; it feeds
//! the continuity bonus of each candidate and the next-plan trace summary.

use crate::input::DecisionContext;
use crate::math::Texture;

use super::types::{Candidate, Response, clamp};

/// One decision recorded in the plan history (JS `plan.history`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlanHistoryEntry {
    /// Street name.
    pub street: String,
    /// Action name.
    pub action: String,
    /// Chip amount.
    pub amount: u64,
    /// Plan intent at the time of the action.
    pub intent: String,
}

/// Per-hand plan state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HandPlan {
    /// Plan id (`plan-<handId>`).
    pub id: String,
    /// Hand id.
    pub hand_id: u64,
    /// Current intent label.
    pub intent: String,
    /// Seats targeted by the plan.
    pub target_seats: Vec<usize>,
    /// Maximum risk budget.
    pub max_risk: u64,
    /// Preferred bet sizes (raise total) once an aggressive action was chosen.
    pub preferred_sizes: Vec<u64>,
    /// Favorable runout labels.
    pub favorable_runouts: Vec<String>,
    /// Unfavorable runout labels.
    pub unfavorable_runouts: Vec<String>,
    /// Continue conditions.
    pub continue_conditions: Vec<String>,
    /// Abort conditions.
    pub abort_conditions: Vec<String>,
    /// Response-to-raise policy label.
    pub response_to_raise: String,
    /// Plan confidence.
    pub confidence: f64,
    /// Last street name.
    pub last_street: String,
    /// Plan history (JS `history`; unbounded in the original).
    pub history: Vec<PlanHistoryEntry>,
}

impl HandPlan {
    /// Creates the fresh per-hand plan (JS constructor).
    #[must_use]
    pub fn new(hand_id: u64) -> Self {
        Self {
            id: format!("plan-{hand_id}"),
            hand_id,
            intent: "observe".to_owned(),
            target_seats: Vec::new(),
            max_risk: 0,
            preferred_sizes: Vec::new(),
            favorable_runouts: Vec::new(),
            unfavorable_runouts: Vec::new(),
            continue_conditions: Vec::new(),
            abort_conditions: vec!["large_resistance".to_owned()],
            response_to_raise: "recalculate".to_owned(),
            confidence: 0.25,
            last_street: "preflop".to_owned(),
            history: Vec::new(),
        }
    }

    /// JS `continuityBonus(candidate, context, pot)` (`stable:507-517`).
    #[must_use]
    pub(crate) fn continuity_bonus(
        &self,
        candidate: &Candidate,
        context: &DecisionContext,
        pot: f64,
    ) -> f64 {
        let mut bonus = 0.0;
        let aggressive = candidate.action == "raise" || candidate.action == "allin";
        if self.intent == "value" && aggressive {
            bonus += pot * 0.025;
        }
        if self.intent == "semi_bluff"
            && aggressive
            && context.street != crate::input::Street::River
        {
            bonus += pot * 0.018;
        }
        if self.intent == "bluff" && aggressive && self.confidence > 0.55 {
            bonus += pot * 0.012;
        }
        if self.intent == "pot_control"
            && (candidate.action == "check" || candidate.action == "call")
        {
            bonus += pot * 0.020;
        }
        if self.intent == "trap" && context.to_call > 0.0 && aggressive {
            bonus += pot * 0.040;
        }
        if self
            .abort_conditions
            .iter()
            .any(|condition| condition == "large_resistance")
            && context.to_call > pot * 0.8
            && aggressive
        {
            bonus -= pot * 0.09;
        }
        bonus
    }

    /// JS `update(context, decision, equity, texture, response, profile)`
    /// (`stable:518-541`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        context: &DecisionContext,
        decision: &Candidate,
        equity: f64,
        texture: &Texture,
        response: Option<&Response>,
    ) {
        let aggressive = decision.action == "raise" || decision.action == "allin";
        if equity > 0.68 {
            self.intent = if aggressive { "value" } else { "trap" }.to_owned();
        } else if equity > 0.52 {
            self.intent = if aggressive {
                "thin_value"
            } else {
                "pot_control"
            }
            .to_owned();
        } else if texture.has_strong_draw
            && aggressive
            && context.street != crate::input::Street::River
        {
            self.intent = "semi_bluff".to_owned();
        } else if aggressive {
            self.intent = "bluff".to_owned();
        } else if context.to_call > 0.0 && decision.action == "call" {
            self.intent = "bluff_catch".to_owned();
        } else {
            self.intent = "pot_control".to_owned();
        }
        self.target_seats = context.active_opponent_seats.clone();
        let risk_factor = if self.intent == "value" {
            0.78
        } else if self.intent == "semi_bluff" {
            0.46
        } else {
            0.32
        };
        self.max_risk = amount_round(context.hero_stack * risk_factor);
        self.preferred_sizes = if aggressive {
            vec![amount_round(decision.amount)]
        } else {
            Vec::new()
        };
        self.favorable_runouts = if texture.has_flush_draw {
            vec!["flush_complete".to_owned(), "high_pressure_card".to_owned()]
        } else if texture.has_straight_draw {
            vec![
                "straight_complete".to_owned(),
                "high_pressure_card".to_owned(),
            ]
        } else {
            vec!["range_advantage_card".to_owned()]
        };
        self.unfavorable_runouts = if equity < 0.55 {
            vec![
                "blank_after_call".to_owned(),
                "opponent_range_strengthens".to_owned(),
            ]
        } else {
            vec!["four_to_straight".to_owned(), "four_to_flush".to_owned()]
        };
        self.continue_conditions = if self.intent == "bluff" {
            vec![
                "opponent_range_capped".to_owned(),
                "credible_pressure_card".to_owned(),
                "useful_blocker".to_owned(),
            ]
        } else {
            vec!["equity_holds".to_owned(), "price_is_reasonable".to_owned()]
        };
        self.abort_conditions = vec!["large_resistance".to_owned()];
        self.response_to_raise = if equity > 0.70 {
            "continue_or_reraise"
        } else if texture.has_strong_draw {
            "compare_price_and_equity"
        } else {
            "recalculate_or_fold"
        }
        .to_owned();
        let response_confidence = response.map_or(0.0, |response| response.confidence);
        self.confidence = clamp(
            0.35 + (equity - 0.5).abs() * 0.8 + response_confidence * 0.2,
            0.25,
            0.92,
        );
        self.last_street = context.street.as_str().to_owned();
        self.history.push(PlanHistoryEntry {
            street: context.street.as_str().to_owned(),
            action: decision.action.clone(),
            amount: amount_round(decision.amount),
            intent: self.intent.clone(),
        });
    }
}

/// JS `Math.round(x)` for a non-negative chip amount.
fn amount_round(value: f64) -> u64 {
    super::types::amount_u64(value)
}
