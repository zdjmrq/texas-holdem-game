//! Per-hand plan state (port of js/ai-core.js HandPlan).

/// Intent labels used by the original JS plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// No strong read yet.
    Observe,
    /// Value bet.
    Value,
    /// Trap / slow play.
    Trap,
    /// Thin value bet.
    ThinValue,
    /// Pot control.
    PotControl,
    /// Semi-bluff.
    SemiBluff,
    /// Bluff.
    Bluff,
    /// Bluff catch.
    BluffCatch,
}

impl Intent {
    /// Lowercase intent name used in the trace.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Intent::Observe => "observe",
            Intent::Value => "value",
            Intent::Trap => "trap",
            Intent::ThinValue => "thin_value",
            Intent::PotControl => "pot_control",
            Intent::SemiBluff => "semi_bluff",
            Intent::Bluff => "bluff",
            Intent::BluffCatch => "bluff_catch",
        }
    }
}

/// Per-hand plan (recreated for every hand).
#[derive(Clone, Debug, PartialEq)]
pub struct HandPlan {
    /// Plan id (plan-&lt;handId&gt;).
    pub id: String,
    /// Hand id.
    pub hand_id: u64,
    /// Current intent.
    pub intent: Intent,
    /// Seats targeted by the plan.
    pub target_seats: Vec<usize>,
    /// Maximum risk budget.
    pub max_risk: u64,
    /// Preferred sizes.
    pub preferred_sizes: Vec<u64>,
    /// Favorable runouts.
    pub favorable_runouts: Vec<String>,
    /// Unfavorable runouts.
    pub unfavorable_runouts: Vec<String>,
    /// Continue conditions.
    pub continue_conditions: Vec<String>,
    /// Abort conditions.
    pub abort_conditions: Vec<String>,
    /// Response-to-raise policy.
    pub response_to_raise: String,
    /// Plan confidence.
    pub confidence: f64,
    /// Last street name.
    pub last_street: String,
}

impl HandPlan {
    /// Creates the fresh per-hand plan.
    #[must_use]
    pub fn new(hand_id: u64) -> Self {
        Self {
            id: format!("plan-{hand_id}"),
            hand_id,
            intent: Intent::Observe,
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
        }
    }

    /// Plan continuity bonus for a candidate (js/ai-core.js:561-571).
    #[must_use]
    pub fn continuity_bonus(&self, aggressive: bool, to_call: f64, pot: f64) -> f64 {
        let mut bonus = 0.0;
        if self.intent == Intent::Value && aggressive {
            bonus += pot * 0.025;
        }
        if self.intent == Intent::SemiBluff && aggressive {
            bonus += pot * 0.018;
        }
        if self.intent == Intent::Bluff && aggressive && self.confidence > 0.55 {
            bonus += pot * 0.012;
        }
        if self.intent == Intent::PotControl && !aggressive {
            bonus += pot * 0.020;
        }
        if self.intent == Intent::Trap && to_call > 0.0 && aggressive {
            bonus += pot * 0.040;
        }
        if self
            .abort_conditions
            .iter()
            .any(|condition| condition == "large_resistance")
            && to_call > pot * 0.8
            && aggressive
        {
            bonus -= pot * 0.09;
        }
        bonus
    }

    /// Updates the plan after a decision (js/ai-core.js:572-595).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        street: crate::input::Street,
        aggressive: bool,
        action: &str,
        equity: f64,
        has_strong_draw: bool,
        has_flush_draw: bool,
        has_straight_draw: bool,
        to_call: f64,
        hero_stack: f64,
        active_seats: &[usize],
        response_confidence: f64,
        amount: u64,
    ) {
        self.intent = if equity > 0.68 {
            if aggressive {
                Intent::Value
            } else {
                Intent::Trap
            }
        } else if equity > 0.52 {
            if aggressive {
                Intent::ThinValue
            } else {
                Intent::PotControl
            }
        } else if has_strong_draw && aggressive && street != crate::input::Street::River {
            Intent::SemiBluff
        } else if aggressive {
            Intent::Bluff
        } else if to_call > 0.0 && action == "call" {
            Intent::BluffCatch
        } else {
            Intent::PotControl
        };
        self.target_seats = active_seats.to_vec();
        self.max_risk = (hero_stack
            * match self.intent {
                Intent::Value => 0.78,
                Intent::SemiBluff => 0.46,
                _ => 0.32,
            })
        .round() as u64;
        self.preferred_sizes = if aggressive { vec![amount] } else { Vec::new() };
        self.favorable_runouts = if has_flush_draw {
            vec!["flush_complete".to_owned(), "high_pressure_card".to_owned()]
        } else if has_straight_draw {
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
        self.continue_conditions = if self.intent == Intent::Bluff {
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
            "continue_or_reraise".to_owned()
        } else if has_strong_draw {
            "compare_price_and_equity".to_owned()
        } else {
            "recalculate_or_fold".to_owned()
        };
        self.confidence =
            (0.35 + (equity - 0.5).abs() * 0.8 + response_confidence * 0.2).clamp(0.25, 0.92);
        self.last_street = street.as_str().to_owned();
    }
}
