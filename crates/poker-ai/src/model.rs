//! Cross-hand behavioural opponent model (port of js/ai-core.js
//! BehavioralModel, docs/specs/ai-strategy.md section 4.2).

use std::collections::{HashMap, HashSet};

use crate::input::{ObservedAction, ShowdownResult};

/// Recent-window size for hand-keyed observation sets.
pub const MODEL_HAND_LIMIT: usize = 256;

/// Per size-bucket facing-bet statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizeStat {
    /// Times the seat faced this size.
    pub faced: u32,
    /// Folds.
    pub folded: u32,
    /// Calls.
    pub called: u32,
    /// Raises.
    pub raised: u32,
}

/// Per pre-flop range bucket statistics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RangeStat {
    /// Times the seat faced this spot.
    pub faced: u32,
    /// Times it raised.
    pub raised: u32,
}

/// Public statistics for one opponent seat across hands.
#[derive(Clone, Debug, PartialEq)]
pub struct BehavioralModel {
    /// Seat index.
    pub seat_id: usize,
    /// Hand ids observed (bounded window).
    pub hands: HashSet<u64>,
    /// Hand ids where the seat voluntarily entered the pot.
    pub vpip_hands: HashSet<u64>,
    /// Hand ids where the seat raised pre-flop.
    pub pfr_hands: HashSet<u64>,
    /// Pre-flop sizing buckets.
    pub preflop_sizing: [SizeStat; 3],
    /// Post-flop sizing buckets.
    pub postflop_sizing: [SizeStat; 3],
    /// Pre-flop times facing a bet.
    pub preflop_faced: u32,
    /// Pre-flop folds facing a bet.
    pub preflop_folds: u32,
    /// Post-flop times facing a bet.
    pub postflop_faced: u32,
    /// Post-flop folds facing a bet.
    pub postflop_folds: u32,
    /// Post-flop calls facing a bet.
    pub postflop_calls: u32,
    /// Post-flop raises facing a bet.
    pub postflop_raises: u32,
    /// Showdowns observed.
    pub showdowns: u32,
    /// Shown bluffs.
    pub shown_bluffs: u32,
    /// Pre-flop range buckets (EP/MP/LP/BL/3bet/4bet).
    pub preflop_ranges: HashMap<String, RangeStat>,
}

impl BehavioralModel {
    /// Creates an empty model for a seat.
    #[must_use]
    pub fn new(seat_id: usize) -> Self {
        let mut preflop_ranges = HashMap::new();
        for key in ["EP", "MP", "LP", "BL", "3bet", "4bet"] {
            preflop_ranges.insert(key.to_owned(), RangeStat::default());
        }
        Self {
            seat_id,
            hands: HashSet::new(),
            vpip_hands: HashSet::new(),
            pfr_hands: HashSet::new(),
            preflop_sizing: [SizeStat::default(); 3],
            postflop_sizing: [SizeStat::default(); 3],
            preflop_faced: 0,
            preflop_folds: 0,
            postflop_faced: 0,
            postflop_folds: 0,
            postflop_calls: 0,
            postflop_raises: 0,
            showdowns: 0,
            shown_bluffs: 0,
            preflop_ranges,
        }
    }

    /// Records one observed public action (js/ai-core.js:426-468).
    pub fn observe(&mut self, event: &ObservedAction) {
        let action = event.action.to_ascii_lowercase();
        if action.contains("blind") || action.contains("ante") {
            return;
        }
        self.hands.insert(event.hand_id);
        let aggressive = event.meta.aggressive
            || action.contains("raise")
            || action == "bet"
            || (action == "allin" && event.meta.aggressive);
        let called = action.contains("call") || (action == "allin" && !aggressive);
        let folded = action.contains("fold");
        if event.street == crate::input::Street::Preflop {
            if let Some(position) = event.meta.position_from_button {
                let key = if event.meta.preflop_raise_count_before == 0 {
                    let category = crate::math::position_category(
                        event.meta.player_count.unwrap_or(2),
                        position,
                        event.meta.is_small_blind,
                        event.meta.is_big_blind,
                    );
                    Some(category.as_str().to_owned())
                } else if event.meta.preflop_raise_count_before == 1 {
                    Some("3bet".to_owned())
                } else if event.meta.preflop_raise_count_before == 2 {
                    Some("4bet".to_owned())
                } else {
                    None
                };
                if let Some(key) = key
                    && (event.meta.preflop_raise_count_before == 0
                        || event.meta.to_call_before > 0.0)
                    && let Some(stat) = self.preflop_ranges.get_mut(&key)
                {
                    stat.faced += 1;
                    if aggressive {
                        stat.raised += 1;
                    }
                }
            }
            if called || aggressive {
                self.vpip_hands.insert(event.hand_id);
            }
            if aggressive {
                self.pfr_hands.insert(event.hand_id);
            }
            if event.meta.to_call_before > 0.0 {
                self.preflop_faced += 1;
                if folded {
                    self.preflop_folds += 1;
                }
                let bucket = size_bucket(event.meta.bet_fraction);
                self.preflop_sizing[bucket].faced += 1;
                if folded {
                    self.preflop_sizing[bucket].folded += 1;
                }
            }
        } else if event.meta.to_call_before > 0.0 {
            self.postflop_faced += 1;
            if folded {
                self.postflop_folds += 1;
            } else if aggressive {
                self.postflop_raises += 1;
            } else if called {
                self.postflop_calls += 1;
            }
            let bucket = size_bucket(event.meta.bet_fraction);
            self.postflop_sizing[bucket].faced += 1;
            if folded {
                self.postflop_sizing[bucket].folded += 1;
            } else if aggressive {
                self.postflop_sizing[bucket].raised += 1;
            } else {
                self.postflop_sizing[bucket].called += 1;
            }
        } else if aggressive {
            self.postflop_raises += 1;
        }
        while self.hands.len() > MODEL_HAND_LIMIT {
            if let Some(oldest) = self.hands.iter().next().copied() {
                self.hands.remove(&oldest);
                self.vpip_hands.remove(&oldest);
                self.pfr_hands.remove(&oldest);
            } else {
                break;
            }
        }
    }

    /// Records a showdown observation.
    pub fn observe_showdown(&mut self, result: &ShowdownResult) {
        self.showdowns = self.showdowns.saturating_add(1);
        if result.was_bluff {
            self.shown_bluffs = self.shown_bluffs.saturating_add(1);
        }
    }

    /// Bayesian posterior helper.
    #[must_use]
    pub fn posterior(
        successes: f64,
        observations: f64,
        prior_mean: f64,
        prior_strength: f64,
    ) -> f64 {
        (successes + prior_mean * prior_strength) / (observations + prior_strength).max(1.0)
    }

    /// Observed VPIP prior.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn vpip(&self) -> f64 {
        Self::posterior(
            self.vpip_hands.len() as f64,
            self.hands.len() as f64,
            0.27,
            10.0,
        )
    }

    /// Observed PFR prior.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn pfr(&self) -> f64 {
        Self::posterior(
            self.pfr_hands.len() as f64,
            self.hands.len() as f64,
            0.18,
            10.0,
        )
    }

    /// Confidence of the model estimate.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn confidence(&self) -> f64 {
        let count = self.hands.len() as f64;
        (count / (count + 10.0)).clamp(0.0, 0.9)
    }

    /// Post-flop aggression rate.
    #[must_use]
    pub fn aggression_rate(&self) -> f64 {
        let actions = f64::from(self.postflop_calls + self.postflop_raises);
        Self::posterior(f64::from(self.postflop_raises), actions, 0.42, 7.0)
    }

    /// Passivity complement of the aggression rate.
    #[must_use]
    pub fn passivity(&self) -> f64 {
        1.0 - self.aggression_rate()
    }

    /// Fold rate for a bet size, optionally using the pre-flop sizing stats.
    #[must_use]
    pub fn fold_rate_for(&self, fraction: f64, preflop: bool) -> f64 {
        let bucket = size_bucket(fraction);
        let stat = if preflop {
            self.preflop_sizing[bucket]
        } else {
            self.postflop_sizing[bucket]
        };
        let all = if preflop {
            Self::posterior(
                f64::from(self.preflop_folds),
                f64::from(self.preflop_faced),
                0.62,
                6.0,
            )
        } else {
            Self::posterior(
                f64::from(self.postflop_folds),
                f64::from(self.postflop_faced),
                0.40,
                6.0,
            )
        };
        let sized = Self::posterior(f64::from(stat.folded), f64::from(stat.faced), all, 7.0);
        sized.clamp(0.08, 0.82)
    }

    /// Pre-flop range width estimate from public statistics.
    #[must_use]
    pub fn preflop_range_estimate(
        &self,
        range_action: Option<&str>,
        position: &str,
        prior_width: f64,
    ) -> Option<(f64, f64)> {
        let key = match range_action {
            Some("open") => position,
            Some(action @ ("3bet" | "4bet")) => action,
            _ => return None,
        };
        let stat = self.preflop_ranges.get(key)?;
        let width = Self::posterior(
            f64::from(stat.raised),
            f64::from(stat.faced),
            prior_width,
            10.0,
        );
        let confidence = f64::from(stat.faced) / (f64::from(stat.faced) + 16.0);
        Some((width, confidence))
    }

    /// True when the seat folds too much.
    #[must_use]
    pub fn folds_too_much(&self) -> bool {
        self.postflop_faced >= 4 && self.fold_rate_for(0.6, false) > 0.56
    }

    /// True when the seat calls too much.
    #[must_use]
    pub fn calls_too_much(&self) -> bool {
        self.postflop_faced >= 4 && self.fold_rate_for(0.6, false) < 0.27 && self.passivity() > 0.52
    }

    /// Observed showdown bluff rate.
    #[must_use]
    pub fn bluff_show_rate(&self) -> f64 {
        Self::posterior(
            f64::from(self.shown_bluffs),
            f64::from(self.showdowns),
            0.24,
            8.0,
        )
    }
}

/// Size bucket index for a bet fraction (0=small, 1=medium, 2=large).
#[must_use]
pub fn size_bucket(fraction: f64) -> usize {
    if fraction <= 0.38 {
        0
    } else if fraction <= 0.78 {
        1
    } else {
        2
    }
}
