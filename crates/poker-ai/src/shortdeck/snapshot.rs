//! Unrounded decision snapshots for golden-corpus replay diagnostics.
//!
//! The JS corpus stores both a rounded trace and the raw (unrounded) values
//! captured by `oracle/golden-ai.js buildRawSnapshot`. Integration tests live
//! outside the crate, so this module exposes a plain public mirror of that raw
//! capture; production code never needs it.

use std::collections::BTreeMap;

use super::StageResult;
use super::types::{Candidate, RangeEquity};

/// Raw pre-legalize decision (JS `preLegalize`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShortDeckSnapshotDecision {
    /// Action name.
    pub action: String,
    /// Unrounded chip amount.
    pub amount: f64,
}

/// Raw per-opponent response probability (JS `response.opponents[i]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShortDeckSnapshotOpponent {
    /// Opponent seat.
    pub seat: usize,
    /// Fold probability.
    pub fold: f64,
    /// Call probability.
    pub call: f64,
    /// Raise probability.
    pub raise: f64,
}

/// Raw response model (JS `response`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShortDeckSnapshotResponse {
    /// Probability every opponent folds.
    pub all_fold: f64,
    /// Probability someone calls and nobody raises.
    pub any_call: f64,
    /// Probability someone raises.
    pub any_raise: f64,
    /// Expected callers given a call and no raise.
    pub expected_callers: f64,
    /// Story credibility.
    pub credibility: f64,
    /// Confidence.
    pub confidence: f64,
    /// Bet fraction.
    pub fraction: f64,
    /// Per-opponent probabilities.
    pub opponents: Vec<ShortDeckSnapshotOpponent>,
}

/// Raw candidate (JS `summarizeCandidates` entry).
#[derive(Clone, Debug, PartialEq)]
pub struct ShortDeckSnapshotCandidate {
    /// Action name.
    pub action: String,
    /// Unrounded chip amount.
    pub amount: f64,
    /// Chips invested.
    pub invest: f64,
    /// Expected value.
    pub ev: f64,
    /// Risk penalty.
    pub risk: f64,
    /// Final utility.
    pub utility: f64,
    /// Reason tag.
    pub reason: String,
    /// Conditional called equity (raise candidates only).
    pub called_equity: Option<f64>,
    /// Plan continuity bonus.
    pub plan_bonus: f64,
    /// Response model (raise candidates only).
    pub response: Option<ShortDeckSnapshotResponse>,
}

/// Raw range-equity estimate (JS `summarizeEquity`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShortDeckSnapshotEquity {
    /// Mean hero equity.
    pub mean: f64,
    /// Standard deviation.
    pub std_dev: f64,
    /// Standard error; `None` when the JS object has no `standardError` key.
    pub standard_error: Option<f64>,
    /// Range confidence; `None` when the JS object has no key.
    pub range_confidence: Option<f64>,
    /// Per-group subset equities; `None` when the JS object has no key.
    pub subset_equities: Option<BTreeMap<String, f64>>,
    /// Accepted samples.
    pub samples: usize,
    /// Sample-count confidence.
    pub confidence: f64,
    /// Exact river heads-up enumeration was used.
    pub exact: bool,
    /// Wall-clock brake fired.
    pub timed_out: bool,
    /// Sampler stalled.
    pub stalled: bool,
}

/// Raw decision snapshot (JS `buildRawSnapshot`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShortDeckSnapshot {
    /// Range equity estimate.
    pub equity: ShortDeckSnapshotEquity,
    /// Candidate list in the order captured by the JS oracle.
    pub candidates: Vec<ShortDeckSnapshotCandidate>,
    /// Pre-legalize selected action, when one was produced.
    pub pre_legalize: Option<ShortDeckSnapshotDecision>,
}

/// Converts an internal stage result into the public raw snapshot.
pub(super) fn capture(stage: &StageResult, prelegalize: &Candidate) -> ShortDeckSnapshot {
    ShortDeckSnapshot {
        equity: capture_equity(&stage.equity),
        candidates: stage.raw_candidates.iter().map(capture_candidate).collect(),
        pre_legalize: Some(ShortDeckSnapshotDecision {
            action: prelegalize.action.clone(),
            amount: prelegalize.amount,
        }),
    }
}

fn capture_equity(equity: &RangeEquity) -> ShortDeckSnapshotEquity {
    ShortDeckSnapshotEquity {
        mean: equity.mean,
        std_dev: equity.std_dev,
        standard_error: equity.standard_error,
        range_confidence: equity.range_confidence,
        subset_equities: equity.subset_equities.as_ref().map(|subset| {
            subset
                .iter()
                .map(|(key, value)| (key.clone(), *value))
                .collect()
        }),
        samples: equity.samples,
        confidence: equity.confidence,
        exact: equity.exact,
        timed_out: equity.timed_out,
        stalled: equity.stalled,
    }
}

fn capture_candidate(candidate: &Candidate) -> ShortDeckSnapshotCandidate {
    ShortDeckSnapshotCandidate {
        action: candidate.action.clone(),
        amount: candidate.amount,
        invest: candidate.invest,
        ev: candidate.ev,
        risk: candidate.risk,
        utility: candidate.utility,
        reason: candidate.reason.clone(),
        called_equity: candidate.called_equity,
        plan_bonus: candidate.plan_bonus,
        response: candidate
            .response
            .as_ref()
            .map(|response| ShortDeckSnapshotResponse {
                all_fold: response.all_fold,
                any_call: response.any_call,
                any_raise: response.any_raise,
                expected_callers: response.expected_callers,
                credibility: response.credibility,
                confidence: response.confidence,
                fraction: response.fraction,
                opponents: response
                    .opponents
                    .iter()
                    .map(|opponent| ShortDeckSnapshotOpponent {
                        seat: opponent.seat,
                        fold: opponent.fold,
                        call: opponent.call,
                        raise: opponent.raise,
                    })
                    .collect(),
            }),
    }
}
