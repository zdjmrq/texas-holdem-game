//! Short-deck behavioural opponent model (port of `js/ai-shortdeck-stable.js`
//! `BehavioralModel`, `stable:360-446`).
//!
//! Compared with the standard core this model has no `preflopRanges` tracking
//! (3-bet/4-bet statistics); every other statistic and formula is identical.
//! The hand window is bounded to [`MODEL_HAND_LIMIT`] hands in insertion order.

use std::collections::{HashSet, VecDeque};

use crate::input::{ObservedAction, ShowdownResult, Street};

use super::types::{SizeBucket, clamp};

/// Recent-window size for hand-keyed observation sets (JS `MODEL_HAND_LIMIT`).
pub(crate) const MODEL_HAND_LIMIT: usize = 256;

/// Post-flop sizing statistics bucket (JS `sizing.small/medium/large`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PostflopSizing {
    faced: u32,
    folded: u32,
    called: u32,
    raised: u32,
}

/// Pre-flop sizing statistics bucket (JS `preflopSizing.small/medium/large`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PreflopSizing {
    faced: u32,
    folded: u32,
}

/// Cross-hand public statistics for one opponent seat.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BehavioralModel {
    /// Seat index.
    pub seat_id: usize,
    hands: VecDeque<u64>,
    hand_set: HashSet<u64>,
    vpip_hands: HashSet<u64>,
    pfr_hands: HashSet<u64>,
    preflop_opportunities: u32,
    postflop_faced: u32,
    postflop_folds: u32,
    postflop_calls: u32,
    postflop_raises: u32,
    showdowns: u32,
    shown_bluffs: u32,
    sizing: [PostflopSizing; 3],
    preflop_sizing: [PreflopSizing; 3],
    preflop_faced: u32,
    preflop_folds: u32,
}

impl BehavioralModel {
    /// Creates an empty model for a seat (JS constructor defaults).
    #[must_use]
    pub fn new(seat_id: usize) -> Self {
        Self {
            seat_id,
            hands: VecDeque::new(),
            hand_set: HashSet::new(),
            vpip_hands: HashSet::new(),
            pfr_hands: HashSet::new(),
            preflop_opportunities: 0,
            postflop_faced: 0,
            postflop_folds: 0,
            postflop_calls: 0,
            postflop_raises: 0,
            showdowns: 0,
            shown_bluffs: 0,
            sizing: [PostflopSizing::default(); 3],
            preflop_sizing: [PreflopSizing::default(); 3],
            preflop_faced: 0,
            preflop_folds: 0,
        }
    }

    /// JS `observe(event)` (`stable:382-419`).
    pub fn observe(&mut self, event: &ObservedAction) {
        let action = event.action.to_ascii_lowercase();
        if action.contains("blind") || action.contains("ante") {
            return;
        }
        let hand_id = event.hand_id;
        if self.hand_set.insert(hand_id) {
            self.hands.push_back(hand_id);
        }
        let meta = &event.meta;
        // JS: meta.aggressive === true || action.includes('raise') ||
        // action === 'bet' || (action === 'allin' && meta.aggressive !== false).
        // The corpus action metadata always records a boolean, so an all-in with
        // aggressive=false (an all-in call) is not an aggressive action.
        let aggressive = meta.aggressive || action.contains("raise") || action == "bet";
        let called = action.contains("call") || (action == "allin" && !aggressive);
        let folded = action.contains("fold");
        if event.street == Street::Preflop {
            #[allow(clippy::cast_possible_truncation)]
            let hand_count = self.hand_count() as u32;
            self.preflop_opportunities = self.preflop_opportunities.max(hand_count);
            if called || aggressive {
                self.vpip_hands.insert(hand_id);
            }
            if aggressive {
                self.pfr_hands.insert(hand_id);
            }
            if finite(meta.to_call_before) > 0.0 {
                self.preflop_faced += 1;
                if folded {
                    self.preflop_folds += 1;
                }
                let bucket = SizeBucket::of(meta.bet_fraction).index();
                let stat = &mut self.preflop_sizing[bucket];
                stat.faced += 1;
                if folded {
                    stat.folded += 1;
                }
            }
        } else if finite(meta.to_call_before) > 0.0 {
            self.postflop_faced += 1;
            if folded {
                self.postflop_folds += 1;
            } else if aggressive {
                self.postflop_raises += 1;
            } else if called {
                self.postflop_calls += 1;
            }
            let bucket = SizeBucket::of(meta.bet_fraction).index();
            let stat = &mut self.sizing[bucket];
            stat.faced += 1;
            if folded {
                stat.folded += 1;
            } else if aggressive {
                stat.raised += 1;
            } else {
                stat.called += 1;
            }
        } else if aggressive {
            self.postflop_raises += 1;
        }
        // Keep numerator and denominator in the same bounded recent window.
        while self.hands.len() > MODEL_HAND_LIMIT {
            if let Some(oldest) = self.hands.pop_front() {
                self.hand_set.remove(&oldest);
                self.vpip_hands.remove(&oldest);
                self.pfr_hands.remove(&oldest);
            } else {
                break;
            }
        }
    }

    /// JS `observeShowdown(result)`.
    pub fn observe_showdown(&mut self, result: &ShowdownResult) {
        self.showdowns += 1;
        if result.was_bluff {
            self.shown_bluffs += 1;
        }
    }

    /// Bayesian posterior helper (JS `posterior`).
    #[must_use]
    pub fn posterior(
        successes: f64,
        observations: f64,
        prior_mean: f64,
        prior_strength: f64,
    ) -> f64 {
        (successes + prior_mean * prior_strength) / (observations + prior_strength).max(1.0)
    }

    /// JS `get handCount()`.
    #[must_use]
    pub fn hand_count(&self) -> usize {
        self.hands.len()
    }

    /// JS `get confidence()`: `clamp(handCount / (handCount + 10), 0, 0.9)`.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let count = self.hand_count() as f64;
        clamp(count / (count + 10.0), 0.0, 0.9)
    }

    /// JS `get vpip()`.
    #[must_use]
    pub fn vpip(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        Self::posterior(
            self.vpip_hands.len() as f64,
            self.hands.len() as f64,
            0.27,
            10.0,
        )
    }

    /// JS `get pfr()`.
    #[must_use]
    #[allow(dead_code)]
    pub fn pfr(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        Self::posterior(
            self.pfr_hands.len() as f64,
            self.hands.len() as f64,
            0.18,
            10.0,
        )
    }

    /// JS `get aggressionRate()`.
    #[must_use]
    pub fn aggression_rate(&self) -> f64 {
        let actions = f64::from(self.postflop_calls + self.postflop_raises);
        Self::posterior(f64::from(self.postflop_raises), actions, 0.42, 7.0)
    }

    /// JS `get passivity()`.
    #[must_use]
    pub fn passivity(&self) -> f64 {
        1.0 - self.aggression_rate()
    }

    /// JS `foldRateFor(fraction, street = 'postflop')` (`stable:436-442`).
    #[must_use]
    pub fn fold_rate_for(&self, fraction: f64, street: Street) -> f64 {
        let preflop = street == Street::Preflop;
        let bucket = SizeBucket::of(fraction).index();
        let (stat_folded, stat_faced) = if preflop {
            (
                self.preflop_sizing[bucket].folded,
                self.preflop_sizing[bucket].faced,
            )
        } else {
            (self.sizing[bucket].folded, self.sizing[bucket].faced)
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
        let sized = Self::posterior(f64::from(stat_folded), f64::from(stat_faced), all, 7.0);
        clamp(sized, 0.08, 0.82)
    }

    /// JS `get bluffShowRate()` (kept for parity; unused by the stable core).
    #[must_use]
    #[allow(dead_code)]
    pub fn bluff_show_rate(&self) -> f64 {
        Self::posterior(
            f64::from(self.shown_bluffs),
            f64::from(self.showdowns),
            0.24,
            8.0,
        )
    }

    /// JS `get callsTooMuch()`.
    #[must_use]
    pub fn calls_too_much(&self) -> bool {
        self.postflop_faced >= 4
            && self.fold_rate_for(0.6, Street::Flop) < 0.27
            && self.passivity() > 0.52
    }

    /// JS `get foldsTooMuch()`.
    #[must_use]
    pub fn folds_too_much(&self) -> bool {
        self.postflop_faced >= 4 && self.fold_rate_for(0.6, Street::Flop) > 0.56
    }
}

/// JS `finite(value, fallback = 0)` for an already-typed f64.
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}
