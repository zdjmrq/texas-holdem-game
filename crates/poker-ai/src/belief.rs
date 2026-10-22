//! Per-hand opponent range belief (port of js/ai-core.js OpponentBelief).
//!
//! Reset every hand, updated from public action events. Full formulas are in
//! docs/specs/ai-strategy.md section 4.1.

use std::collections::HashMap;

use poker_core::{Card, Variant};

use crate::input::ObservedAction;
use crate::math::{self, BoardComboFeatures, Position};
use crate::model::BehavioralModel;

/// Pre-flop range snapshot carried into later streets.
#[derive(Clone, Debug, PartialEq)]
pub struct PreflopSnapshot {
    /// Range width at the end of pre-flop.
    pub range_width: f64,
    /// Strength shift at the end of pre-flop.
    pub strength_shift: f64,
    /// Polarization at the end of pre-flop.
    pub polarization: f64,
    /// Capped flag at the end of pre-flop.
    pub capped: bool,
    /// Last pre-flop range action ("open"/"3bet"/"4bet").
    pub range_action: Option<String>,
    /// Opening position category when the seat opened.
    pub opening_position: String,
    /// Prior is unconditioned (no public evidence yet).
    pub unconditioned: bool,
}

impl Default for PreflopSnapshot {
    fn default() -> Self {
        Self {
            range_width: 1.0,
            strength_shift: 0.0,
            polarization: 0.0,
            capped: false,
            range_action: None,
            opening_position: "MP".to_owned(),
            unconditioned: true,
        }
    }
}

/// Estimated range belief for one opponent seat during a single hand.
#[derive(Clone, Debug, PartialEq)]
pub struct OpponentBelief {
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
    /// Last observed street name.
    pub last_street: String,
    /// Observed action history (bounded to 32 events).
    pub history: Vec<ObservedAction>,
    /// Pre-flop snapshot used after the board changes.
    pub preflop_snapshot: Option<PreflopSnapshot>,
    /// Estimated aggression rate.
    pub aggression: f64,
    /// Weight of public-action likelihood in combo weights.
    pub evidence_weight: f64,
    /// Percentile-cutoff blend weight.
    pub percentile_weight: f64,
    /// Pre-flop range sharpness gain.
    pub preflop_sharpness: f64,
    /// Shrinkage weight toward observed public pre-flop statistics.
    pub adaptive_range_weight: f64,
    /// Last pre-flop range action ("open"/"3bet"/"4bet").
    pub range_action: Option<String>,
    /// Opening position category when the seat opened.
    pub opening_position: String,
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
            last_street: "preflop".to_owned(),
            history: Vec::new(),
            preflop_snapshot: None,
            aggression: 0.42,
            evidence_weight: 0.70,
            percentile_weight: 0.0,
            preflop_sharpness: 0.0,
            adaptive_range_weight: 0.0,
            range_action: None,
            opening_position: "MP".to_owned(),
        }
    }

    /// Applies the profile-derived weights used by the standard core.
    pub fn configure(
        &mut self,
        evidence_weight: f64,
        percentile_weight: f64,
        preflop_sharpness: f64,
        adaptive_range_weight: f64,
    ) {
        self.evidence_weight = evidence_weight;
        self.percentile_weight = percentile_weight;
        self.preflop_sharpness = preflop_sharpness;
        self.adaptive_range_weight = adaptive_range_weight;
    }

    /// Resets per-hand state (BehavioralModel persists across hands).
    pub fn begin_hand(&mut self) {
        self.range_width = 0.42;
        self.strength_shift = 0.0;
        self.polarization = 0.0;
        self.capped = false;
        self.folded = false;
        self.confidence = 0.12;
        self.last_street = "preflop".to_owned();
        self.history.clear();
        self.preflop_snapshot = None;
        self.range_action = None;
        self.opening_position = "MP".to_owned();
    }

    /// Observes one public action (js/ai-core.js:306-359).
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn observe(&mut self, event: &ObservedAction, model: &BehavioralModel) {
        let mut action = event.action.to_ascii_lowercase();
        if action == "allin" && !event.meta.aggressive {
            action = "call".to_owned();
        }
        let street = event.street.as_str();
        if street == "preflop"
            && (event.meta.aggressive || action == "raise" || action == "bet" || action == "allin")
        {
            if event.meta.preflop_raise_count_before >= 2 {
                action = "4bet".to_owned();
            } else if event.meta.preflop_raise_count_before >= 1 {
                action = "3bet".to_owned();
            }
        }
        let fraction = event.meta.bet_fraction.clamp(0.0, 3.0);
        self.last_street = street.to_owned();
        if action.contains("fold") {
            self.folded = true;
            self.range_width = 0.0;
        } else if street == "preflop" {
            if action.contains("4bet") {
                self.range_width = self.range_width.min(0.055);
                self.strength_shift += 0.18;
                self.polarization = 0.24;
                self.capped = false;
                self.range_action = Some("4bet".to_owned());
            } else if action.contains("3bet") {
                self.range_width = self.range_width.min(0.115);
                self.strength_shift += 0.12;
                self.polarization = 0.28;
                self.capped = false;
                self.range_action = Some("3bet".to_owned());
            } else if action == "raise" || action == "bet" || action == "allin" {
                let position =
                    event
                        .meta
                        .position_from_button
                        .map_or(Position::Middle, |distance| {
                            math::position_category(
                                event.meta.player_count.unwrap_or(2),
                                distance,
                                event.meta.is_small_blind,
                                event.meta.is_big_blind,
                            )
                        });
                let width = match position {
                    Position::Early => 0.18,
                    Position::Late => 0.32,
                    _ => 0.24,
                };
                self.range_width = self.range_width.min(width);
                self.strength_shift += 0.075;
                self.polarization = 0.12;
                self.range_action = Some("open".to_owned());
                self.opening_position = position.as_str().to_owned();
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
            self.polarization =
                (self.polarization + if fraction > 0.8 { 0.22 } else { 0.10 }).clamp(0.0, 0.8);
            self.capped = false;
        }
        self.range_width += (model.vpip() - 0.25) * 0.16;
        self.polarization += (model.aggression_rate() - 0.45).max(0.0) * 0.08;
        self.range_width = self
            .range_width
            .clamp(if self.folded { 0.0 } else { 0.025 }, 0.92);
        self.strength_shift = self.strength_shift.clamp(-0.16, 0.30);
        self.polarization = self.polarization.clamp(0.0, 0.85);
        self.confidence = (self.confidence + 0.07).clamp(0.12, 0.92);
        self.aggression = model.aggression_rate();
        if street == "preflop" {
            self.preflop_snapshot = Some(PreflopSnapshot {
                range_width: self.range_width,
                strength_shift: self.strength_shift,
                polarization: self.polarization,
                capped: self.capped,
                range_action: self.range_action.clone(),
                opening_position: self.opening_position.clone(),
                unconditioned: false,
            });
        }
        self.history.push(event.clone());
        if self.history.len() > 32 {
            self.history.remove(0);
        }
    }

    /// Estimated strength of the believed range.
    #[must_use]
    pub fn estimated_strength(&self) -> f64 {
        (0.50 + self.strength_shift + (0.30 - self.range_width) * 0.34).clamp(0.18, 0.90)
    }

    /// Conditional weight of one concrete opponent combination.
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn combo_weight(
        &self,
        c1: Card,
        c2: Card,
        variant: Variant,
        model: Option<&BehavioralModel>,
        feature_cache: &mut HashMap<String, BoardComboFeatures>,
    ) -> f64 {
        if self.folded {
            return 0.0;
        }
        if self.history.is_empty()
            && (self.range_width - 0.42).abs() < f64::EPSILON
            && self.strength_shift == 0.0
            && self.polarization == 0.0
            && !self.capped
        {
            return 1.0;
        }
        let features = math::combo_features(c1, c2, variant);
        let has_board_history = self.last_street != "preflop"
            && self
                .history
                .iter()
                .any(|event| event.meta.community_cards.len() >= 3);
        let prior = if has_board_history {
            self.preflop_snapshot.clone().unwrap_or_default()
        } else {
            PreflopSnapshot {
                range_width: self.range_width,
                strength_shift: self.strength_shift,
                polarization: self.polarization,
                capped: self.capped,
                range_action: self.range_action.clone(),
                opening_position: self.opening_position.clone(),
                unconditioned: false,
            }
        };
        let observed = model.and_then(|model| {
            model.preflop_range_estimate(
                prior.range_action.as_deref(),
                &prior.opening_position,
                prior.range_width,
            )
        });
        let width = observed.map_or(prior.range_width, |(width, confidence)| {
            (prior.range_width
                + (width - prior.range_width) * confidence * self.adaptive_range_weight)
                .clamp(0.015, 0.65)
        });
        let cutoff = (1.0 - width + prior.strength_shift * 0.25).clamp(0.04, 0.95);
        let legacy_distance = features.strength + prior.strength_shift - cutoff;
        let distance = if self.percentile_weight > 0.0 {
            (1.0 - self.percentile_weight) * legacy_distance
                + self.percentile_weight
                    * (features.strength
                        - math::starting_range_cutoff(width, variant)
                        - prior.strength_shift * 0.25)
        } else {
            legacy_distance
        };
        let mut weight = if prior.unconditioned {
            1.0
        } else {
            1.0 / (1.0 + (-distance * (15.0 + self.preflop_sharpness * 45.0)).exp())
        };
        let blocker_bluff = features.suited
            && (features.ace_blocker || features.king_blocker)
            && features.strength > 0.24;
        if prior.polarization > 0.0 {
            if features.strength > 0.74 {
                weight *= 1.0 + prior.polarization * 0.9;
            } else if blocker_bluff {
                weight += prior.polarization * 0.22;
            } else if features.strength > 0.42 && features.strength < 0.64 {
                weight *= 1.0 - prior.polarization * 0.45;
            }
        }
        if prior.capped && features.strength > 0.82 {
            weight *= 0.42;
        }
        for event in &self.history {
            if event.street == crate::input::Street::Preflop
                || event.meta.community_cards.len() < 3
                || event.action.to_ascii_lowercase().contains("fold")
            {
                continue;
            }
            let variant_name = if variant.is_short_deck() {
                "shortdeck"
            } else {
                "standard"
            };
            let board_key = event
                .meta
                .community_cards
                .iter()
                .map(|card| format!("{}-{}", card.rank_value(), card.suit().as_str()))
                .collect::<Vec<_>>()
                .join(",");
            let hole_key = format!(
                "{}-{}-{}-{}",
                c1.rank_value(),
                c1.suit().as_str(),
                c2.rank_value(),
                c2.suit().as_str()
            );
            let key = format!("{variant_name}:{board_key}:{hole_key}");
            let board_features = feature_cache.entry(key).or_insert_with(|| {
                math::board_combo_features(c1, c2, &event.meta.community_cards, variant)
            });
            let likelihood = math::public_action_likelihood(
                board_features,
                &event.action,
                event.meta.to_call_before,
                event.meta.pot_before,
                event.meta.bet_fraction,
                self.aggression,
            );
            weight *= likelihood.powf(self.evidence_weight);
        }
        weight.max(0.0001)
    }
}
