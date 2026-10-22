//! Result and helper types for the frozen short-deck decision core.
//!
//! Field names and shapes mirror the original `js/ai-shortdeck-stable.js`
//! objects (`candidate`, `response`, `equityInfo`, `combo`) so the golden
//! corpus replay can compare unrounded values field by field. This module is
//! private to `crate::shortdeck`; the public surface is [super::ShortDeckBrain].

use std::collections::HashMap;

use poker_core::Card;

/// JS `Math.max(low, Math.min(high, Number(value) || 0))` for finite inputs.
///
/// NaN is coerced to zero by the original helper, so it clamps to `low`.
#[must_use]
pub(crate) fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value.is_nan() {
        return low;
    }
    high.min(low.max(value))
}

/// JS `Number(value.toFixed(digits))`.
///
/// The corpus rounds trace values with `toFixed`; this reproduces the common
/// case (round half away from zero on the scaled value). The golden test also
/// applies a tolerance, so only exact binary ties could differ.
#[must_use]
pub(crate) fn fixed(value: f64, digits: i32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let scale = 10_f64.powi(digits);
    (value * scale).round() / scale
}

/// Converts a chip amount to the engine's integer chip unit.
#[must_use]
pub(crate) fn amount_u64(amount: f64) -> u64 {
    if !amount.is_finite() || amount <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rounded = amount.round() as u64;
    rounded
}

/// One live opponent seat, or the synthetic `unknown-i` seat of the JS
/// fallback when no active opponent seat list is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Seat {
    /// A real seat index.
    Known(usize),
    /// Synthetic seat used by `estimateRangeEquity` when the context carries no
    /// active opponent seats.
    Unknown(usize),
}

/// One weighted starting combination (JS combo object with a running prefix
/// weight used by the binary search in `weightedPick`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Combo {
    /// First card.
    pub c1: Card,
    /// Second card.
    pub c2: Card,
    /// Belief weight.
    pub weight: f64,
    /// Running sum of weights up to and including this combo.
    pub cumulative: f64,
}

/// One seat's weighted combination sampler.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sampler {
    /// Owner seat.
    pub seat: Seat,
    /// Combinations in deck order.
    pub combos: Vec<Combo>,
}

/// Cache key for `boardComboFeatures` results (JS featureCache key).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FeatureKey {
    /// Community cards at the moment of the observed action.
    pub board: Vec<Card>,
    /// First hole card of the concrete combination.
    pub c1: Card,
    /// Second hole card of the concrete combination.
    pub c2: Card,
}

/// One observed public action as stored in the belief history. The original
/// stable core reduces `event.meta` into this flat record before replaying it
/// through `publicActionLikelihood`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PublicActionEvent {
    /// Lowercase action name (fold/check/call/raise/bet/allin/...).
    pub action: String,
    /// Street of the action.
    pub street: crate::input::Street,
    /// Clamped bet fraction (`meta.betFraction`).
    pub fraction: f64,
    /// Community cards at the moment of the action.
    pub board: Vec<Card>,
    /// Chips needed to call before the action (`meta.toCallBefore`).
    pub to_call: f64,
    /// Pot before the action (`meta.potBefore`).
    pub pot: f64,
}

/// One opponent entry in a response model (JS `response.opponents[i]`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ResponseOpponent {
    /// Opponent seat.
    pub seat: usize,
    /// Probability the opponent folds.
    pub fold: f64,
    /// Probability the opponent calls.
    pub call: f64,
    /// Probability the opponent raises.
    pub raise: f64,
}

/// Opponent response model for one raise target (JS `responseForRaise` output).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Response {
    /// Probability every live opponent folds.
    pub all_fold: f64,
    /// Probability at least one opponent calls and none raises.
    pub any_call: f64,
    /// Probability at least one opponent raises.
    pub any_raise: f64,
    /// Expected callers given that someone calls and nobody raises.
    pub expected_callers: f64,
    /// Story credibility of the aggressive line.
    pub credibility: f64,
    /// Confidence of the per-opponent estimates.
    pub confidence: f64,
    /// Bet size over `max(bigBlind, pot + toCall)`.
    pub fraction: f64,
    /// Per-opponent fold/call/raise probabilities.
    pub opponents: Vec<ResponseOpponent>,
}

/// One evaluated action candidate (JS `candidate` object).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Candidate {
    /// Action name (fold/check/call/raise/allin).
    pub action: String,
    /// Unrounded chip amount produced by the candidate builder.
    pub amount: f64,
    /// Chips invested by the action (raise total minus the round bet).
    pub invest: f64,
    /// Expected value.
    pub ev: f64,
    /// Risk penalty.
    pub risk: f64,
    /// Final utility (ev - risk + plan bonus + deception).
    pub utility: f64,
    /// Candidate reason tag.
    pub reason: String,
    /// Conditional called equity (raise candidates only).
    pub called_equity: Option<f64>,
    /// Response model of a raise candidate.
    pub response: Option<Response>,
    /// Plan continuity bonus already folded into `utility`.
    pub plan_bonus: f64,
}

/// Range equity estimate (JS `estimateRangeEquity` return value).
///
/// `Option` fields reproduce the JS object keys that are missing on some
/// return paths (the exact river path omits `subsetEquities`, the "no sampler"
/// path omits almost everything).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RangeEquity {
    /// Mean hero equity.
    pub mean: f64,
    /// Standard deviation of the sampled outcome.
    pub std_dev: f64,
    /// Standard error of the mean; `None` when JS leaves it `undefined`.
    pub standard_error: Option<f64>,
    /// Confidence of the opponent range beliefs; `None` when `undefined`.
    pub range_confidence: Option<f64>,
    /// Equity per pot-contest group key; `None` when JS has no key.
    pub subset_equities: Option<HashMap<String, f64>>,
    /// Number of accepted samples.
    pub samples: usize,
    /// Sample-count confidence.
    pub confidence: f64,
    /// Exact river heads-up enumeration was used.
    pub exact: bool,
    /// Wall-clock safety brake fired (always false under the virtual clock).
    pub timed_out: bool,
    /// The sampler stalled with zero samples.
    pub stalled: bool,
}

impl RangeEquity {
    /// The JS early return when no seat can be sampled
    /// (`{mean:.5, stdDev:.22, samples:0, confidence:.05, timedOut:false}`).
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self {
            mean: 0.5,
            std_dev: 0.22,
            standard_error: None,
            range_confidence: None,
            subset_equities: None,
            samples: 0,
            confidence: 0.05,
            exact: false,
            timed_out: false,
            stalled: false,
        }
    }
}

/// Bet-size bucket used by the opponent model statistics and spot mixing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeBucket {
    /// Small size.
    Small,
    /// Medium size.
    Medium,
    /// Large size.
    Large,
}

impl SizeBucket {
    /// JS `sizeBucket(fraction)`.
    #[must_use]
    pub(crate) fn of(fraction: f64) -> SizeBucket {
        let fraction = if fraction.is_finite() { fraction } else { 0.0 };
        if fraction <= 0.38 {
            SizeBucket::Small
        } else if fraction <= 0.78 {
            SizeBucket::Medium
        } else {
            SizeBucket::Large
        }
    }

    /// Array index used by the model sizing tables.
    #[must_use]
    pub(crate) const fn index(self) -> usize {
        match self {
            SizeBucket::Small => 0,
            SizeBucket::Medium => 1,
            SizeBucket::Large => 2,
        }
    }
}
