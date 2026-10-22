//! Deterministic scripted policies for simulation and smoke tests.

use poker_core::{Card, RandomSource};
use poker_engine::{Action, LegalActions, Phase, Player};

/// Which scripted policy a seat uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyKind {
    /// Uniform random choice among legal actions.
    Random,
    /// Check or call; fold only when facing a bet that cannot be called.
    CheckCall,
    /// Raise most of the time when allowed.
    Aggressive,
}

impl PolicyKind {
    /// Policy name used on the command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PolicyKind::Random => "random",
            PolicyKind::CheckCall => "checkcall",
            PolicyKind::Aggressive => "aggressive",
        }
    }

    /// Parses a policy name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<PolicyKind> {
        match name {
            "random" => Some(PolicyKind::Random),
            "checkcall" => Some(PolicyKind::CheckCall),
            "aggressive" => Some(PolicyKind::Aggressive),
            _ => None,
        }
    }
}

/// Read-only decision context handed to a policy.
pub struct AgentView<'a> {
    /// Seat index to act.
    pub seat: usize,
    /// Legal actions for the seat.
    pub legal: &'a LegalActions,
    /// Current pot size.
    pub pot: u64,
    /// Current phase.
    pub phase: Phase,
    /// All seats (public state only for scripted policies).
    pub players: &'a [Player],
    /// Community cards.
    pub community: &'a [Card],
}

/// Picks a legal raise total on the wager grid.
fn pick_raise_target(legal: &LegalActions, rng: &mut dyn RandomSource) -> u64 {
    let unit = legal.wager_unit.max(1);
    let low = legal.min_raise_to;
    let high = legal.max_raise_to;
    if low > high {
        return low;
    }
    let steps = (high - low) / unit;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let offset = (rng.next_f64() * (steps as f64 + 1.0)) as u64;
    (low + offset.min(steps) * unit).min(high)
}

/// Builds the concrete action for a legal action type.
fn action_for(
    action_type: poker_engine::ActionType,
    legal: &LegalActions,
    rng: &mut dyn RandomSource,
) -> Action {
    match action_type {
        poker_engine::ActionType::Fold => Action::Fold,
        poker_engine::ActionType::Check => Action::Check,
        poker_engine::ActionType::Call => Action::Call,
        poker_engine::ActionType::Raise => Action::Raise {
            to: pick_raise_target(legal, rng),
        },
        poker_engine::ActionType::AllIn => Action::AllIn,
    }
}

/// A scripted policy agent.
#[derive(Clone, Copy, Debug)]
pub struct PolicyAgent {
    kind: PolicyKind,
}

impl PolicyAgent {
    /// Creates an agent for the given policy.
    #[must_use]
    pub const fn new(kind: PolicyKind) -> Self {
        Self { kind }
    }

    /// Chooses an action for the current decision point.
    #[must_use]
    pub fn decide(&self, view: &AgentView<'_>, rng: &mut dyn RandomSource) -> Action {
        let legal = view.legal;
        let actions = &legal.actions;
        if actions.is_empty() {
            return Action::Fold;
        }
        match self.kind {
            PolicyKind::Random => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let index = (rng.next_f64() * actions.len() as f64) as usize;
                action_for(actions[index.min(actions.len() - 1)], legal, rng)
            }
            PolicyKind::CheckCall => {
                if legal.contains(poker_engine::ActionType::Check) {
                    Action::Check
                } else if legal.contains(poker_engine::ActionType::Call) {
                    Action::Call
                } else if legal.contains(poker_engine::ActionType::Fold) {
                    Action::Fold
                } else {
                    action_for(actions[0], legal, rng)
                }
            }
            PolicyKind::Aggressive => {
                let raise_roll = rng.next_f64();
                if legal.contains(poker_engine::ActionType::Raise) && raise_roll < 0.6 {
                    action_for(poker_engine::ActionType::Raise, legal, rng)
                } else if legal.contains(poker_engine::ActionType::Check) {
                    Action::Check
                } else if legal.contains(poker_engine::ActionType::Call) {
                    Action::Call
                } else if legal.contains(poker_engine::ActionType::Fold) {
                    Action::Fold
                } else {
                    action_for(actions[0], legal, rng)
                }
            }
        }
    }
}
