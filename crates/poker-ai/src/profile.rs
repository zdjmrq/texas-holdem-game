//! AI personalities and effective profile resolution.
//!
//! Port of the original JS configuration stack:
//!
//! 1. `DEFAULT_PROFILE` (js/ai-core.js:25-60)
//! 2. `PROFILE_OVERRIDES[style]` (js/ai-core.js:62-69)
//! 3. `VARIANT_PROFILE_OVERRIDES[variant]` (js/ai-core.js:72-75)
//! 4. adapter profile (`STYLE_CONFIGS` or `SD_STYLE_CONFIGS`)
//!
//! Only the seventh profile field group (riverCfrWeight=1 for standard) is
//! enabled by default among the v1.5.8 experimental candidates; every other
//! experimental weight defaults to zero and must stay disabled.

use poker_core::Variant;

/// The six personalities shipped by the original game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    /// Tight-aggressive.
    Tag,
    /// Loose-aggressive.
    Lag,
    /// Balanced.
    Solid,
    /// Hyper-aggressive.
    Maniac,
    /// Tight-passive.
    Tagfish,
    /// Calling station.
    CallingStation,
}

impl Style {
    /// All six styles in the original JS order.
    pub const ALL: [Style; 6] = [
        Style::Tag,
        Style::Lag,
        Style::Solid,
        Style::Maniac,
        Style::Tagfish,
        Style::CallingStation,
    ];

    /// Original JS style id (for example "TAG" or "CALLING_STATION").
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Style::Tag => "TAG",
            Style::Lag => "LAG",
            Style::Solid => "SOLID",
            Style::Maniac => "MANIAC",
            Style::Tagfish => "TAGFISH",
            Style::CallingStation => "CALLING_STATION",
        }
    }

    /// Parses an original JS style id.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Style> {
        match value {
            "TAG" => Some(Style::Tag),
            "LAG" => Some(Style::Lag),
            "SOLID" => Some(Style::Solid),
            "MANIAC" => Some(Style::Maniac),
            "TAGFISH" => Some(Style::Tagfish),
            "CALLING_STATION" | "CALLING" => Some(Style::CallingStation),
            _ => None,
        }
    }
}

/// Where an adapter profile table comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileSource {
    /// Standard `STYLE_CONFIGS` (local standard games and server short-deck).
    StandardStyle,
    /// Short-deck local `SD_STYLE_CONFIGS` (js/shortdeck.js:290-333).
    ShortDeckLocalStyle,
    /// Server short-deck path also uses `STYLE_CONFIGS` (known v1.5.8 fork).
    ShortDeckServerStyle,
}

/// The full effective AI profile (union of DEFAULT_PROFILE and adapter keys).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiProfile {
    /// Voluntarily put money in pot prior (adapter override).
    pub vpip: f64,
    /// Pre-flop raise frequency ratio from the adapter style table.
    pub pfr: f64,
    /// Aggression preference.
    pub aggression: f64,
    /// Three-bet preference.
    pub three_bet_freq: f64,
    /// Bluff preference.
    pub bluff_freq: f64,
    /// Continuation-bet frequency (legacy path only).
    pub cb_freq: f64,
    /// Double-barrel frequency (legacy path only).
    pub double_barrel: f64,
    /// Check-raise frequency (legacy path only).
    pub check_raise_freq: f64,
    /// Risk aversion used by riskPenalty.
    pub risk_aversion: f64,
    /// Trap preference (legacy path only).
    pub trap_preference: f64,
    /// Thin-value preference used by deception.
    pub thin_value_preference: f64,
    /// Adaptation speed (legacy path only).
    pub adaptation_speed: f64,
    /// Uncertainty tolerance used by mixing.
    pub uncertainty_tolerance: f64,
    /// Self-image awareness.
    pub image_awareness: f64,
    /// Patience (legacy path only).
    pub patience: f64,
    /// Weight of public-action likelihood in belief weights.
    pub range_evidence_weight: f64,
    /// Discount applied to called-range equity and sizing.
    pub called_range_discount: f64,
    /// Pre-flop response blend weight (experimental, default 0).
    pub preflop_response_weight: f64,
    /// Percentile-cutoff belief blend (standard variant sets 1).
    pub range_percentile_weight: f64,
    /// Conditional caller equity weight (experimental, default 0).
    pub conditional_caller_equity_weight: f64,
    /// Pre-flop participation gating (standard variant sets 1).
    pub preflop_participation_weight: f64,
    /// Belief sigmoid sharpness gain (standard variant sets 1).
    pub preflop_range_sharpness: f64,
    /// Observed pre-flop public-stat shrinkage weight (standard sets 0.65).
    pub observed_preflop_range_weight: f64,
    /// Hero-call frequency preference.
    pub hero_call_freq: f64,
    /// Rejected broad-pressure experiment (default 0).
    pub professional_weight: f64,
    /// Rejected post-flop selection experiment (default 0).
    pub postflop_selection_weight: f64,
    /// Rejected joint-call-return experiment (default 0).
    pub joint_call_return_weight: f64,
    /// Rejected competence experiment (default 0).
    pub competence_weight: f64,
    /// Rejected sampling-precision experiment (default 0).
    pub target_precision_weight: f64,
    /// Rejected conditional-risk experiment (default 0).
    pub conditional_risk_weight: f64,
    /// Rejected terminal-realization experiment (default 0).
    pub terminal_realization_weight: f64,
    /// Rejected ranked-continuation experiment (default 0).
    pub ranked_continuation_weight: f64,
    /// Rejected terminal-call experiment (default 0).
    pub terminal_call_weight: f64,
    /// Rejected closing-river-call experiment (default 0).
    pub river_call_weight: f64,
    /// River CFR switch; standard variant profile enables it (1).
    pub river_cfr_weight: f64,
}

impl Default for AiProfile {
    fn default() -> Self {
        Self {
            vpip: 0.25,
            pfr: 0.75,
            aggression: 0.55,
            three_bet_freq: 0.08,
            bluff_freq: 0.10,
            cb_freq: 0.58,
            double_barrel: 0.46,
            check_raise_freq: 0.10,
            risk_aversion: 0.52,
            trap_preference: 0.18,
            thin_value_preference: 0.42,
            adaptation_speed: 0.55,
            uncertainty_tolerance: 0.42,
            image_awareness: 0.58,
            patience: 0.55,
            range_evidence_weight: 0.70,
            called_range_discount: 0.055,
            preflop_response_weight: 0.0,
            range_percentile_weight: 0.0,
            conditional_caller_equity_weight: 0.0,
            preflop_participation_weight: 0.0,
            preflop_range_sharpness: 0.0,
            observed_preflop_range_weight: 0.0,
            hero_call_freq: 0.15,
            professional_weight: 0.0,
            postflop_selection_weight: 0.0,
            joint_call_return_weight: 0.0,
            competence_weight: 0.0,
            target_precision_weight: 0.0,
            conditional_risk_weight: 0.0,
            terminal_realization_weight: 0.0,
            ranked_continuation_weight: 0.0,
            terminal_call_weight: 0.0,
            river_call_weight: 0.0,
            river_cfr_weight: 0.0,
        }
    }
}

/// Partial profile override (JS spread semantics: present keys only).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileOverrides {
    /// Voluntarily put money in pot prior.
    pub vpip: Option<f64>,
    /// Pre-flop raise ratio (adapter table only).
    pub pfr: Option<f64>,
    /// Aggression preference.
    pub aggression: Option<f64>,
    /// Three-bet preference.
    pub three_bet_freq: Option<f64>,
    /// Bluff preference.
    pub bluff_freq: Option<f64>,
    /// Continuation-bet frequency.
    pub cb_freq: Option<f64>,
    /// Double-barrel frequency.
    pub double_barrel: Option<f64>,
    /// Check-raise frequency.
    pub check_raise_freq: Option<f64>,
    /// Risk aversion.
    pub risk_aversion: Option<f64>,
    /// Trap preference.
    pub trap_preference: Option<f64>,
    /// Thin-value preference.
    pub thin_value_preference: Option<f64>,
    /// Adaptation speed.
    pub adaptation_speed: Option<f64>,
    /// Uncertainty tolerance.
    pub uncertainty_tolerance: Option<f64>,
    /// Image awareness.
    pub image_awareness: Option<f64>,
    /// Patience.
    pub patience: Option<f64>,
    /// Range evidence weight.
    pub range_evidence_weight: Option<f64>,
    /// Called range discount.
    pub called_range_discount: Option<f64>,
    /// Pre-flop response weight.
    pub preflop_response_weight: Option<f64>,
    /// Range percentile weight.
    pub range_percentile_weight: Option<f64>,
    /// Conditional caller equity weight.
    pub conditional_caller_equity_weight: Option<f64>,
    /// Pre-flop participation weight.
    pub preflop_participation_weight: Option<f64>,
    /// Pre-flop range sharpness.
    pub preflop_range_sharpness: Option<f64>,
    /// Observed pre-flop range weight.
    pub observed_preflop_range_weight: Option<f64>,
    /// Hero call frequency.
    pub hero_call_freq: Option<f64>,
    /// Professional weight.
    pub professional_weight: Option<f64>,
    /// Post-flop selection weight.
    pub postflop_selection_weight: Option<f64>,
    /// Joint call return weight.
    pub joint_call_return_weight: Option<f64>,
    /// Competence weight.
    pub competence_weight: Option<f64>,
    /// Target precision weight.
    pub target_precision_weight: Option<f64>,
    /// Conditional risk weight.
    pub conditional_risk_weight: Option<f64>,
    /// Terminal realization weight.
    pub terminal_realization_weight: Option<f64>,
    /// Ranked continuation weight.
    pub ranked_continuation_weight: Option<f64>,
    /// Terminal call weight.
    pub terminal_call_weight: Option<f64>,
    /// River call weight.
    pub river_call_weight: Option<f64>,
    /// River CFR weight.
    pub river_cfr_weight: Option<f64>,
    /// Fold-to-raise preference (adapter table, legacy only).
    pub fold_to_raise: Option<f64>,
    /// Fold-to-3-bet preference (adapter table, legacy only).
    pub fold_to_3bet: Option<f64>,
    /// Raise ratio (short-deck adapter table, legacy only).
    pub raise_ratio: Option<f64>,
    /// Float frequency (adapter table, legacy only).
    pub float_freq: Option<f64>,
}

impl ProfileOverrides {
    fn apply(self, profile: &mut AiProfile) {
        macro_rules! apply {
            ($($field:ident),* $(,)?) => {
                $(
                    if let Some(value) = self.$field {
                        profile.$field = value;
                    }
                )*
            };
        }
        apply!(
            vpip,
            pfr,
            aggression,
            three_bet_freq,
            bluff_freq,
            cb_freq,
            double_barrel,
            check_raise_freq,
            risk_aversion,
            trap_preference,
            thin_value_preference,
            adaptation_speed,
            uncertainty_tolerance,
            image_awareness,
            patience,
            range_evidence_weight,
            called_range_discount,
            preflop_response_weight,
            range_percentile_weight,
            conditional_caller_equity_weight,
            preflop_participation_weight,
            preflop_range_sharpness,
            observed_preflop_range_weight,
            hero_call_freq,
            professional_weight,
            postflop_selection_weight,
            joint_call_return_weight,
            competence_weight,
            target_precision_weight,
            conditional_risk_weight,
            terminal_realization_weight,
            ranked_continuation_weight,
            terminal_call_weight,
            river_call_weight,
            river_cfr_weight,
        );
    }
}

const fn some(value: f64) -> Option<f64> {
    Some(value)
}

/// `PROFILE_OVERRIDES[style]` (js/ai-core.js:62-69).
#[must_use]
pub fn style_overrides(style: Style) -> ProfileOverrides {
    match style {
        Style::Tag => ProfileOverrides {
            risk_aversion: some(0.67),
            aggression: some(0.50),
            bluff_freq: some(0.07),
            trap_preference: some(0.18),
            adaptation_speed: some(0.48),
            patience: some(0.72),
            ..ProfileOverrides::default()
        },
        Style::Lag => ProfileOverrides {
            risk_aversion: some(0.38),
            aggression: some(0.70),
            bluff_freq: some(0.15),
            thin_value_preference: some(0.55),
            uncertainty_tolerance: some(0.58),
            image_awareness: some(0.68),
            ..ProfileOverrides::default()
        },
        Style::Solid => ProfileOverrides {
            risk_aversion: some(0.54),
            aggression: some(0.56),
            bluff_freq: some(0.10),
            adaptation_speed: some(0.62),
            image_awareness: some(0.66),
            ..ProfileOverrides::default()
        },
        Style::Maniac => ProfileOverrides {
            risk_aversion: some(0.28),
            aggression: some(0.78),
            bluff_freq: some(0.19),
            uncertainty_tolerance: some(0.68),
            image_awareness: some(0.48),
            patience: some(0.28),
            ..ProfileOverrides::default()
        },
        Style::Tagfish => ProfileOverrides {
            risk_aversion: some(0.76),
            aggression: some(0.36),
            bluff_freq: some(0.045),
            trap_preference: some(0.12),
            adaptation_speed: some(0.38),
            patience: some(0.78),
            ..ProfileOverrides::default()
        },
        Style::CallingStation => ProfileOverrides {
            risk_aversion: some(0.46),
            aggression: some(0.25),
            bluff_freq: some(0.035),
            thin_value_preference: some(0.28),
            uncertainty_tolerance: some(0.52),
            patience: some(0.66),
            ..ProfileOverrides::default()
        },
    }
}

/// `VARIANT_PROFILE_OVERRIDES[variant]` (js/ai-core.js:72-75).
///
/// The short-deck stable core has no variant overrides, so this returns an
/// empty override set for `Variant::ShortDeck`.
#[must_use]
pub fn variant_overrides(variant: Variant) -> ProfileOverrides {
    match variant {
        Variant::Standard => ProfileOverrides {
            range_evidence_weight: some(0.45),
            called_range_discount: some(0.08),
            preflop_participation_weight: some(1.0),
            range_percentile_weight: some(1.0),
            preflop_range_sharpness: some(1.0),
            observed_preflop_range_weight: some(0.65),
            river_cfr_weight: some(1.0),
            ..ProfileOverrides::default()
        },
        Variant::ShortDeck => ProfileOverrides::default(),
    }
}

/// Adapter `STYLE_CONFIGS[style]` (js/ai.js:59-66).
#[must_use]
pub fn standard_style_config(style: Style) -> ProfileOverrides {
    let mut overrides = ProfileOverrides {
        fold_to_raise: None,
        fold_to_3bet: None,
        raise_ratio: None,
        float_freq: None,
        ..ProfileOverrides::default()
    };
    let (
        vpip,
        pfr,
        aggression,
        three_bet_freq,
        bluff_freq,
        fold_to_3bet,
        cb_freq,
        double_barrel,
        float_freq,
        check_raise_freq,
        hero_call_freq,
    ) = match style {
        Style::Tag => (
            0.20, 0.85, 0.60, 0.06, 0.08, 0.55, 0.60, 0.45, 0.15, 0.08, 0.10,
        ),
        Style::Lag => (
            0.30, 0.80, 0.65, 0.12, 0.14, 0.40, 0.70, 0.55, 0.25, 0.12, 0.20,
        ),
        Style::Solid => (
            0.24, 0.75, 0.55, 0.08, 0.10, 0.48, 0.55, 0.48, 0.18, 0.10, 0.15,
        ),
        Style::Maniac => (
            0.42, 0.70, 0.75, 0.18, 0.22, 0.28, 0.72, 0.60, 0.30, 0.18, 0.25,
        ),
        Style::Tagfish => (
            0.18, 0.55, 0.35, 0.03, 0.04, 0.50, 0.40, 0.25, 0.08, 0.04, 0.12,
        ),
        Style::CallingStation => (
            0.40, 0.15, 0.22, 0.02, 0.03, 0.15, 0.25, 0.12, 0.05, 0.03, 0.30,
        ),
    };
    overrides.vpip = some(vpip);
    overrides.pfr = some(pfr);
    overrides.aggression = some(aggression);
    overrides.three_bet_freq = some(three_bet_freq);
    overrides.bluff_freq = some(bluff_freq);
    overrides.fold_to_3bet = some(fold_to_3bet);
    overrides.cb_freq = some(cb_freq);
    overrides.double_barrel = some(double_barrel);
    overrides.float_freq = some(float_freq);
    overrides.check_raise_freq = some(check_raise_freq);
    overrides.hero_call_freq = some(hero_call_freq);
    overrides
}

/// Adapter `SD_STYLE_CONFIGS[style]` (js/shortdeck.js:290-333).
#[must_use]
pub fn short_deck_style_config(style: Style) -> ProfileOverrides {
    let (
        vpip,
        pfr,
        aggression,
        bluff_freq,
        fold_to_raise,
        fold_to_3bet,
        raise_ratio,
        cb_freq,
        three_bet_freq,
        double_barrel,
        float_freq,
        check_raise_freq,
        hero_call_freq,
    ) = match style {
        Style::Tag => (
            0.20, 0.82, 0.55, 0.07, 0.55, 0.55, 2.2, 0.50, 0.05, 0.42, 0.14, 0.07, 0.10,
        ),
        Style::Lag => (
            0.30, 0.78, 0.50, 0.12, 0.38, 0.42, 2.5, 0.45, 0.11, 0.50, 0.24, 0.11, 0.20,
        ),
        Style::Tagfish => (
            0.18, 0.50, 0.35, 0.04, 0.45, 0.52, 1.8, 0.30, 0.03, 0.22, 0.07, 0.04, 0.12,
        ),
        Style::Maniac => (
            0.38, 0.68, 0.60, 0.20, 0.25, 0.30, 3.0, 0.50, 0.17, 0.58, 0.28, 0.17, 0.25,
        ),
        Style::CallingStation => (
            0.38, 0.14, 0.20, 0.03, 0.10, 0.18, 1.3, 0.20, 0.02, 0.10, 0.05, 0.03, 0.30,
        ),
        Style::Solid => (
            0.23, 0.72, 0.45, 0.08, 0.45, 0.48, 2.0, 0.40, 0.07, 0.42, 0.16, 0.09, 0.15,
        ),
    };
    let mut overrides = ProfileOverrides {
        vpip: some(vpip),
        pfr: some(pfr),
        aggression: some(aggression),
        bluff_freq: some(bluff_freq),
        fold_to_raise: some(fold_to_raise),
        fold_to_3bet: some(fold_to_3bet),
        raise_ratio: some(raise_ratio),
        cb_freq: some(cb_freq),
        three_bet_freq: some(three_bet_freq),
        double_barrel: some(double_barrel),
        float_freq: some(float_freq),
        check_raise_freq: some(check_raise_freq),
        hero_call_freq: some(hero_call_freq),
        ..ProfileOverrides::default()
    };
    overrides.preflop_response_weight = None;
    overrides
}

/// Resolves the effective profile exactly like the JS constructor:
/// DEFAULT -> style overrides -> variant overrides -> adapter profile.
#[must_use]
pub fn effective_profile(style: Style, variant: Variant, adapter: ProfileOverrides) -> AiProfile {
    let mut profile = AiProfile::default();
    style_overrides(style).apply(&mut profile);
    variant_overrides(variant).apply(&mut profile);
    adapter.apply(&mut profile);
    profile
}

/// Resolves a profile for one of the three original runtime paths.
#[must_use]
pub fn profile_for(style: Style, variant: Variant, source: ProfileSource) -> AiProfile {
    let adapter = match source {
        ProfileSource::StandardStyle | ProfileSource::ShortDeckServerStyle => {
            standard_style_config(style)
        }
        ProfileSource::ShortDeckLocalStyle => short_deck_style_config(style),
    };
    effective_profile(style, variant, adapter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_profile_enables_only_river_cfr() {
        let profile = profile_for(
            Style::Solid,
            Variant::Standard,
            ProfileSource::StandardStyle,
        );
        assert_eq!(profile.range_evidence_weight, 0.45);
        assert_eq!(profile.called_range_discount, 0.08);
        assert_eq!(profile.preflop_participation_weight, 1.0);
        assert_eq!(profile.range_percentile_weight, 1.0);
        assert_eq!(profile.river_cfr_weight, 1.0);
        assert_eq!(profile.professional_weight, 0.0);
        assert_eq!(profile.competence_weight, 0.0);
        assert_eq!(profile.target_precision_weight, 0.0);
    }

    #[test]
    fn short_deck_profile_uses_adapter_table() {
        let local = profile_for(
            Style::Solid,
            Variant::ShortDeck,
            ProfileSource::ShortDeckLocalStyle,
        );
        assert_eq!(local.vpip, 0.23);
        assert_eq!(local.aggression, 0.45);
        assert_eq!(local.river_cfr_weight, 0.0);
        let server = profile_for(
            Style::Solid,
            Variant::ShortDeck,
            ProfileSource::ShortDeckServerStyle,
        );
        assert_eq!(server.vpip, 0.24);
        assert_eq!(server.aggression, 0.55);
    }
}
