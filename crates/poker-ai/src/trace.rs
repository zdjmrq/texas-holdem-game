//! Decision trace structures mirroring the original JS `lastTrace`.
//!
//! The golden corpus stores the JS trace (without `elapsedMs`) and the Rust
//! replay compares these fields. Production can keep a reduced trace; the
//! default here is the full diagnostic shape.

use crate::input::RiverTrace;

/// Parameters block of the trace (profile values active for this decision).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceParameters {
    /// Variant name.
    pub variant: String,
    /// Strategy focus name (`ten-player-25bb` / `general`).
    pub strategy_focus: String,
    /// Range evidence weight.
    pub range_evidence_weight: f64,
    /// Called range discount.
    pub called_range_discount: f64,
    /// Pre-flop participation weight.
    pub pre_flop_participation_weight: f64,
    /// Pre-flop range sharpness.
    pub pre_flop_range_sharpness: f64,
    /// Observed pre-flop range weight.
    pub observed_pre_flop_range_weight: f64,
    /// Rejected professional experiment weight.
    pub professional_weight: f64,
    /// Post-flop selection weight.
    pub post_flop_selection_weight: f64,
    /// Joint call return weight.
    pub joint_call_return_weight: f64,
    /// Competence experiment weight.
    pub competence_weight: f64,
    /// Target precision weight.
    pub target_precision_weight: f64,
    /// Conditional risk weight.
    pub conditional_risk_weight: f64,
    /// Terminal realization weight.
    pub terminal_realization_weight: f64,
    /// Ranked continuation weight.
    pub ranked_continuation_weight: f64,
    /// Terminal call weight.
    pub terminal_call_weight: f64,
    /// River call weight.
    pub river_call_weight: f64,
}

/// Equity block of the trace (JS rounds these to 4 decimals).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceEquity {
    /// Mean range equity.
    pub mean: f64,
    /// Standard deviation of the sampled outcome.
    pub std_dev: f64,
    /// Confidence in the sample count.
    pub confidence: f64,
    /// Number of samples (990 for the exact river heads-up path).
    pub samples: u64,
    /// The wall-clock safety brake fired.
    pub timed_out: bool,
    /// Standard error of the mean.
    pub standard_error: f64,
    /// Confidence of the opponent range beliefs.
    pub range_confidence: f64,
    /// Exact enumeration was used.
    pub exact: bool,
}

/// Candidate response summary in the trace.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraceResponse {
    /// Probability every live opponent folds.
    pub all_fold: f64,
    /// Probability at least one opponent calls and none raises.
    pub any_call: f64,
    /// Probability at least one opponent raises.
    pub any_raise: f64,
    /// Expected callers given a call and no raise.
    pub expected_callers: f64,
}

/// One candidate action in the trace (JS rounds ev/risk/utility to 2 decimals).
#[derive(Clone, Debug, PartialEq)]
pub struct TraceCandidate {
    /// Action name (fold/check/call/raise/allin).
    pub action: String,
    /// Amount in chips.
    pub amount: u64,
    /// Expected value.
    pub ev: f64,
    /// Risk penalty.
    pub risk: f64,
    /// Final utility (ev - risk + bonuses).
    pub utility: f64,
    /// Candidate reason tag.
    pub reason: String,
    /// Terminal call classification (experimental, usually absent).
    pub terminal_call: Option<String>,
    /// Conditional called equity (when computed).
    pub called_equity: Option<f64>,
    /// Response model summary.
    pub response: Option<TraceResponse>,
}

impl Default for TraceCandidate {
    fn default() -> Self {
        Self {
            action: String::new(),
            amount: 0,
            ev: 0.0,
            risk: 0.0,
            utility: 0.0,
            reason: String::new(),
            terminal_call: None,
            called_equity: None,
            response: None,
        }
    }
}

/// Self-image snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraceSelfImage {
    /// Perceived tightness.
    pub tightness: f64,
    /// Perceived aggression.
    pub aggression: f64,
    /// Perceived river aggression.
    pub river_aggression: f64,
    /// Shown bluff rate.
    pub shown_bluff_rate: f64,
    /// Recently bluffed.
    pub recently_bluffed: bool,
}

/// Opponent summary in the trace.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraceOpponent {
    /// Seat index.
    pub seat: usize,
    /// Estimated range width.
    pub range_width: f64,
    /// Estimated range strength.
    pub estimated_strength: f64,
    /// Observation confidence.
    pub confidence: f64,
    /// Opponent folds too much.
    pub folds_too_much: bool,
    /// Opponent calls too much.
    pub calls_too_much: bool,
}

/// Next-hand plan summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TracePlan {
    /// Plan id (plan-<handId>).
    pub id: String,
    /// Intent label.
    pub intent: String,
    /// Maximum risk budget.
    pub max_risk: u64,
    /// Continue conditions.
    pub continue_conditions: Vec<String>,
    /// Abort conditions.
    pub abort_conditions: Vec<String>,
    /// Response-to-raise policy label.
    pub response_to_raise: String,
}

/// Chosen action summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceChosen {
    /// Action name.
    pub action: String,
    /// Amount in chips.
    pub amount: u64,
    /// Utility of the chosen action.
    pub utility: f64,
}

/// Full decision trace (mirror of the JS `lastTrace` object).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecisionTrace {
    /// Strategy version string (`unified-v2`).
    pub strategy_version: String,
    /// Active profile parameters.
    pub parameters: TraceParameters,
    /// Hand id.
    pub hand_id: u64,
    /// Decision id.
    pub decision_id: String,
    /// Current plan intent.
    pub intent: String,
    /// Range equity estimate.
    pub equity: TraceEquity,
    /// Pot odds of the call (toCall / max(1, pot + toCall)).
    pub pot_odds: f64,
    /// Self-image snapshot.
    pub self_image: TraceSelfImage,
    /// Opponent summaries (active reads order).
    pub opponents: Vec<TraceOpponent>,
    /// Candidate actions sorted by utility descending.
    pub candidates: Vec<TraceCandidate>,
    /// River CFR enabled by profile and focus.
    pub river_cfr_enabled: bool,
    /// River solver trace when it drove the decision.
    pub river_solver: Option<RiverTrace>,
    /// Chosen action summary.
    pub chosen: TraceChosen,
    /// Next-hand plan summary.
    pub next_plan: TracePlan,
    /// Number of AI RNG calls consumed by this decision (golden corpus check).
    pub rng_calls: u64,
}
