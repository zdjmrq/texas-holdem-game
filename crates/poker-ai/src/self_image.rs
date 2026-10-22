//! Self-image tracking (port of js/ai-core.js SelfImage): what a public
//! observer could infer from this seat's own actions.

use std::collections::HashSet;

use crate::input::ShowdownResult;
use crate::model::MODEL_HAND_LIMIT;

/// One recorded self action.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedAction {
    /// Hand id.
    pub hand_id: u64,
    /// Street name.
    pub street: String,
    /// Action name.
    pub action: String,
    /// Amount.
    pub amount: u64,
    /// Aggressive action.
    pub aggressive: bool,
    /// Plan intent at the time.
    pub intent: String,
}

/// Public image snapshot (JS SelfImage.snapshot()).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelfImageSnapshot {
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

/// Tracks the hero's own public image across hands.
#[derive(Clone, Debug, PartialEq)]
pub struct SelfImage {
    /// Hand ids tracked (bounded).
    pub hands: HashSet<u64>,
    /// Recent actions (bounded to 80).
    pub actions: Vec<RecordedAction>,
    /// Showdowns observed.
    pub showdowns: u32,
    /// Shown bluffs.
    pub shown_bluffs: u32,
    /// Shown traps.
    pub shown_traps: u32,
}

impl Default for SelfImage {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfImage {
    /// Creates an empty self image.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hands: HashSet::new(),
            actions: Vec::new(),
            showdowns: 0,
            shown_bluffs: 0,
            shown_traps: 0,
        }
    }

    /// Records one self action.
    pub fn record(&mut self, hand_id: u64, street: &str, action: &str, amount: u64, intent: &str) {
        self.hands.insert(hand_id);
        if self.hands.len() > MODEL_HAND_LIMIT
            && let Some(oldest) = self.hands.iter().next().copied()
        {
            self.hands.remove(&oldest);
        }
        self.actions.push(RecordedAction {
            hand_id,
            street: street.to_owned(),
            action: action.to_owned(),
            amount,
            aggressive: matches!(action, "raise" | "allin"),
            intent: intent.to_owned(),
        });
        if self.actions.len() > 80 {
            self.actions.remove(0);
        }
    }

    /// Records a showdown observation.
    pub fn observe_showdown(&mut self, result: &ShowdownResult) {
        self.showdowns = self.showdowns.saturating_add(1);
        if result.was_bluff {
            self.shown_bluffs = self.shown_bluffs.saturating_add(1);
        }
        if result.was_trap {
            self.shown_traps = self.shown_traps.saturating_add(1);
        }
    }

    /// Public image snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SelfImageSnapshot {
        let recent = if self.actions.len() > 28 {
            &self.actions[self.actions.len() - 28..]
        } else {
            &self.actions[..]
        };
        let voluntary = recent
            .iter()
            .filter(|action| !matches!(action.action.as_str(), "fold" | "check"))
            .count() as f64;
        let aggressive = recent.iter().filter(|action| action.aggressive).count() as f64;
        let river_actions: Vec<&RecordedAction> = recent
            .iter()
            .filter(|action| action.street == "river")
            .collect();
        let river_aggressive = river_actions
            .iter()
            .filter(|action| action.aggressive)
            .count() as f64;
        SelfImageSnapshot {
            tightness: clamp01(
                1.0 - (voluntary + 3.0) / (recent.len() as f64 + 12.0),
                0.05,
                0.95,
            ),
            aggression: clamp01((aggressive + 3.2) / (recent.len() as f64 + 8.0), 0.05, 0.95),
            river_aggression: clamp01(
                (river_aggressive + 0.5) / (river_actions.len() as f64 + 4.0),
                0.0,
                1.0,
            ),
            shown_bluff_rate: (f64::from(self.shown_bluffs) + 1.6)
                / (f64::from(self.showdowns) + 7.0),
            recently_bluffed: self
                .actions
                .iter()
                .rev()
                .take(10)
                .any(|action| action.intent == "bluff"),
        }
    }
}

fn clamp01(value: f64, low: f64, high: f64) -> f64 {
    high.min(low.max(value))
}
