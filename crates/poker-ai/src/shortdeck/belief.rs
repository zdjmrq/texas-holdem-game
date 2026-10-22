//! Per-hand short-deck opponent range belief (port of
//! `js/ai-shortdeck-stable.js` `OpponentBelief`, `stable:246-351`).
//!
//! Differences from the standard core (`docs/specs/ai-strategy.md` section 9.2):
//! * no `preflopSharpness` / `adaptiveRangeWeight` / public-stat shrinkage;
//! * pre-flop open-raise width is fixed to `0.24` (the standard core uses
//!   positional widths);
//! * the combo-weight sigmoid is fixed to `x15` and there is no observed
//!   percentile cutoff (the short-deck profile keeps `percentileWeight = 0`).

use std::collections::HashMap;

use poker_core::{Card, Variant};

use crate::input::{ObservedAction, Street};
use crate::math::BoardComboFeatures;

use super::features::{combo_board_features, public_action_likelihood};
use super::types::{FeatureKey, PublicActionEvent, clamp};

/// Pre-flop range snapshot carried into later streets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreflopSnapshot {
    /// Range width at the end of pre-flop.
    pub range_width: f64,
    /// Strength shift at the end of pre-flop.
    pub strength_shift: f64,
    /// Polarization at the end of pre-flop.
    pub polarization: f64,
    /// Capped flag at the end of pre-flop.
    pub capped: bool,
}

/// Estimated range belief for one opponent seat during a single hand.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OpponentBelief {
    /// Seat index.
    pub seat_id: usize,
    /// Estimated fraction of starting hands in range.
    pub range_width: f64,
    /// Strength shift applied to the range cutoff.
    pub strength_shift: f64,
    /// Polarization of the observed range.
    pub polarization: f64,
    /// Range appears capped (no strong hands).
    pub capped: bool,
    /// Opponent folded.
    pub folded: bool,
    /// Belief confidence.
    pub confidence: f64,
    /// Street of the last observed action.
    pub last_street: Street,
    /// Observed action history (bounded to 32 events).
    pub history: Vec<PublicActionEvent>,
    /// Pre-flop snapshot used after the board changes.
    pub preflop_snapshot: Option<PreflopSnapshot>,
    /// Estimated aggression rate from the model.
    pub aggression: f64,
    /// Weight of public-action likelihood in combo weights.
    pub evidence_weight: f64,
    /// Percentile-cutoff blend weight (always zero for short deck).
    pub percentile_weight: f64,
}

impl OpponentBelief {
    /// Creates a fresh belief with the original JS defaults.
    #[must_use]
    pub fn new(seat_id: usize) -> Self {
        Self {
            seat_id,
            range_width: 0.42,
            strength_shift: 0.0,
            polarization: 0.0,
            capped: false,
            folded: false,
            confidence: 0.12,
            last_street: Street::Preflop,
            history: Vec::new(),
            preflop_snapshot: None,
            aggression: 0.42,
            evidence_weight: 0.70,
            percentile_weight: 0.0,
        }
    }

    /// JS `beginHand()`: resets per-hand state.
    pub fn begin_hand(&mut self) {
        self.range_width = 0.42;
        self.strength_shift = 0.0;
        self.polarization = 0.0;
        self.capped = false;
        self.folded = false;
        self.confidence = 0.12;
        self.last_street = Street::Preflop;
        self.history.clear();
        self.preflop_snapshot = None;
    }

    /// JS `observe(event, model)` (`stable:273-318`).
    pub fn observe(&mut self, event: &ObservedAction, model_vpip: f64, model_aggression: f64) {
        let mut action = event.action.to_ascii_lowercase();
        if action == "allin" && !event.meta.aggressive {
            action = "call".to_owned();
        }
        let street = event.street;
        let fraction = clamp(event.meta.bet_fraction, 0.0, 3.0);
        self.last_street = street;
        if action.contains("fold") {
            self.folded = true;
            self.range_width = 0.0;
        } else if street == Street::Preflop {
            if action.contains("4bet") {
                self.range_width = self.range_width.min(0.055);
                self.strength_shift += 0.18;
                self.polarization = 0.24;
            } else if action.contains("3bet") {
                self.range_width = self.range_width.min(0.115);
                self.strength_shift += 0.12;
                self.polarization = 0.28;
            } else if action == "raise" || action == "bet" || action == "allin" {
                self.range_width = self.range_width.min(0.24);
                self.strength_shift += 0.075;
                self.polarization = 0.12;
            } else if action.contains("call") {
                self.range_width = self.range_width.min(0.34);
                self.strength_shift += 0.025;
                self.capped = true;
            } else if action == "check" {
                self.range_width = self.range_width.max(0.68);
                self.capped = true;
            }
        } else if action == "check" {
            self.strength_shift -= 0.025;
            self.capped = self.capped || fraction == 0.0;
        } else if action.contains("call") {
            self.range_width *= if fraction > 0.72 { 0.64 } else { 0.76 };
            self.strength_shift += if fraction > 0.72 { 0.075 } else { 0.035 };
            self.capped = true;
        } else if action.contains("raise") || action == "bet" || action == "allin" {
            self.range_width *= if fraction > 0.9 { 0.48 } else { 0.60 };
            self.strength_shift += if fraction > 0.9 { 0.13 } else { 0.085 };
            self.polarization = clamp(
                self.polarization + if fraction > 0.8 { 0.22 } else { 0.10 },
                0.0,
                0.8,
            );
            self.capped = false;
        }
        self.range_width += (model_vpip - 0.25) * 0.16;
        self.polarization += (model_aggression - 0.45).max(0.0) * 0.08;
        self.range_width = clamp(
            self.range_width,
            if self.folded { 0.0 } else { 0.025 },
            0.92,
        );
        self.strength_shift = clamp(self.strength_shift, -0.16, 0.30);
        self.polarization = clamp(self.polarization, 0.0, 0.85);
        self.confidence = clamp(self.confidence + 0.07, 0.12, 0.92);
        self.aggression = model_aggression;
        if street == Street::Preflop {
            self.preflop_snapshot = Some(PreflopSnapshot {
                range_width: self.range_width,
                strength_shift: self.strength_shift,
                polarization: self.polarization,
                capped: self.capped,
            });
        }
        self.history.push(PublicActionEvent {
            action,
            street,
            fraction,
            board: event.meta.community_cards.clone(),
            to_call: finite(event.meta.to_call_before),
            pot: finite(event.meta.pot_before),
        });
        if self.history.len() > 32 {
            self.history.remove(0);
        }
    }

    /// JS `comboWeight(c1, c2, variant, featureCache)` (`stable:319-347`).
    pub(crate) fn combo_weight(
        &self,
        c1: Card,
        c2: Card,
        variant: Variant,
        feature_cache: &mut HashMap<FeatureKey, BoardComboFeatures>,
    ) -> f64 {
        if self.folded {
            return 0.0;
        }
        // An unacted seat has no evidence of an opening range; assuming a top
        // 42% range for it would collapse preflop equity at a full table.
        if self.history.is_empty()
            && self.range_width == 0.42
            && self.strength_shift == 0.0
            && self.polarization == 0.0
            && !self.capped
        {
            return 1.0;
        }
        let features = crate::math::combo_features(c1, c2, variant);
        let use_snapshot =
            self.last_street != Street::Preflop && self.history.iter().any(|e| e.board.len() >= 3);
        let (
            prior_range_width,
            prior_strength_shift,
            prior_polarization,
            prior_capped,
            unconditioned,
        ) = if use_snapshot {
            match self.preflop_snapshot {
                Some(snapshot) => (
                    snapshot.range_width,
                    snapshot.strength_shift,
                    snapshot.polarization,
                    snapshot.capped,
                    false,
                ),
                None => (1.0, 0.0, 0.0, false, true),
            }
        } else {
            (
                self.range_width,
                self.strength_shift,
                self.polarization,
                self.capped,
                false,
            )
        };
        let cutoff = clamp(
            1.0 - prior_range_width + prior_strength_shift * 0.25,
            0.04,
            0.95,
        );
        let legacy_distance = features.strength + prior_strength_shift - cutoff;
        let distance = if self.percentile_weight != 0.0 {
            let percentile_distance = features.strength
                - crate::math::starting_range_cutoff(prior_range_width, variant)
                - prior_strength_shift * 0.25;
            (1.0 - self.percentile_weight) * legacy_distance
                + self.percentile_weight * percentile_distance
        } else {
            legacy_distance
        };
        let mut weight = if unconditioned {
            1.0
        } else {
            1.0 / (1.0 + (-distance * 15.0).exp())
        };
        let blocker_bluff = features.suited
            && (features.ace_blocker || features.king_blocker)
            && features.strength > 0.24;
        if prior_polarization > 0.0 {
            if features.strength > 0.74 {
                weight *= 1.0 + prior_polarization * 0.9;
            } else if blocker_bluff {
                weight += prior_polarization * 0.22;
            } else if features.strength > 0.42 && features.strength < 0.64 {
                weight *= 1.0 - prior_polarization * 0.45;
            }
        }
        if prior_capped && features.strength > 0.82 {
            weight *= 0.42;
        }
        for event in &self.history {
            if event.street == Street::Preflop
                || event.board.len() < 3
                || event.action.contains("fold")
            {
                continue;
            }
            let key = FeatureKey {
                board: event.board.clone(),
                c1,
                c2,
            };
            let board_features = match feature_cache.get(&key) {
                Some(cached) => *cached,
                None => {
                    let computed = combo_board_features(c1, c2, &event.board, variant);
                    feature_cache.insert(key, computed);
                    computed
                }
            };
            weight *= public_action_likelihood(&board_features, event, self.aggression)
                .powf(self.evidence_weight);
        }
        weight.max(0.0001)
    }

    /// JS `estimatedStrength()` (`stable:348-350`).
    #[must_use]
    pub fn estimated_strength(&self) -> f64 {
        clamp(
            0.50 + self.strength_shift + (0.30 - self.range_width) * 0.34,
            0.18,
            0.90,
        )
    }
}

/// JS `finite(value, fallback = 0)` for an already-typed f64.
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
