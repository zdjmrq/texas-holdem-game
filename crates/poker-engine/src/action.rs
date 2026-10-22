//! Player actions and the legal action set for a seat.

use std::fmt;

/// A concrete player action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Fold.
    Fold,
    /// Check (only legal when there is nothing to call).
    Check,
    /// Call the outstanding amount (all-in when the stack is smaller).
    Call,
    /// Raise/bet to a total amount (including the call portion).
    Raise {
        /// Total chips committed this round after the raise.
        to: u64,
    },
    /// Commit the whole remaining stack.
    AllIn,
}

/// The action types used in legal-action lists and agent decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionType {
    /// Fold.
    Fold,
    /// Check.
    Check,
    /// Call.
    Call,
    /// Raise/bet.
    Raise,
    /// All-in.
    AllIn,
}

impl ActionType {
    /// Lowercase name used by the wire protocol and the original JS code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ActionType::Fold => "fold",
            ActionType::Check => "check",
            ActionType::Call => "call",
            ActionType::Raise => "raise",
            ActionType::AllIn => "allin",
        }
    }

    /// Parses an action-type name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<ActionType> {
        match name {
            "fold" => Some(ActionType::Fold),
            "check" => Some(ActionType::Check),
            "call" => Some(ActionType::Call),
            "raise" | "bet" => Some(ActionType::Raise),
            "allin" | "all-in" => Some(ActionType::AllIn),
            _ => None,
        }
    }
}

impl Action {
    /// The coarse action type.
    #[must_use]
    pub const fn action_type(self) -> ActionType {
        match self {
            Action::Fold => ActionType::Fold,
            Action::Check => ActionType::Check,
            Action::Call => ActionType::Call,
            Action::Raise { .. } => ActionType::Raise,
            Action::AllIn => ActionType::AllIn,
        }
    }

    /// Raise target for raise actions.
    #[must_use]
    pub const fn raise_to(self) -> Option<u64> {
        match self {
            Action::Raise { to } => Some(to),
            _ => None,
        }
    }
}

/// Everything a player is allowed to do at a decision point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegalActions {
    /// Allowed action types in the original JS order
    /// (check; or fold+call; then raise; then all-in).
    pub actions: Vec<ActionType>,
    /// Chips needed to call, capped at the player's stack.
    pub to_call: u64,
    /// Whether the player may make a voluntary raise.
    pub can_raise: bool,
    /// Smallest legal raise total (grid-aligned).
    pub min_raise_to: u64,
    /// Largest possible total commitment (current round bet + stack).
    pub max_raise_to: u64,
    /// Raise-grid unit (small blind or short-deck ante).
    pub wager_unit: u64,
}

impl LegalActions {
    /// True when the action type is currently legal.
    #[must_use]
    pub fn contains(&self, action: ActionType) -> bool {
        self.actions.contains(&action)
    }
}

/// Why an action was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActError {
    /// The action type is not in the legal action set for this seat.
    IllegalAction(ActionType),
    /// A raise target was not a multiple of the wager unit.
    RaiseNotIntegerMultiple {
        /// Offending total amount.
        amount: u64,
        /// Required grid unit.
        wager_unit: u64,
    },
    /// A raise target fell outside the legal range.
    RaiseOutOfRange {
        /// Offending total amount.
        amount: u64,
        /// Smallest legal total.
        minimum: u64,
        /// Largest legal total.
        maximum: u64,
    },
}

impl fmt::Display for ActError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActError::IllegalAction(_) => write!(f, "当前不能执行这个行动"),
            ActError::RaiseNotIntegerMultiple { wager_unit, .. } => {
                write!(
                    f,
                    "加注金额必须是 {wager_unit} 的整数倍；零头筹码可使用全下"
                )
            }
            ActError::RaiseOutOfRange { .. } => write!(f, "加注金额不在合法范围内"),
        }
    }
}

impl std::error::Error for ActError {}
