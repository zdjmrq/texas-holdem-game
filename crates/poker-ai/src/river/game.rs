//! Bounded heads-up river betting tree.
//!
//! Port of js/river-solver.js:26-41 (v1.5.8). The tree is finite and small:
//! at most one future raise, a public bet menu of roughly 1/3, 2/3 and 1 pot
//! plus the effective all-in, and two players with no internal chance nodes
//! (chance is fully expanded by the public-range abstraction before CFR runs).

use super::ranges::Deal;
use crate::AiError;

/// Terminal state of a river tree node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// One player folded; the other wins the contestable pot.
    Fold {
        /// Winning player index (0 = hero, 1 = villain).
        winner: u8,
    },
    /// Both players reached showdown.
    Showdown,
}

/// One node of the finite river betting tree.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// Private strength buckets of [hero, villain] for the chance deal.
    pub scores: [u8; 2],
    /// Conditional showdown sign of the chance deal (hero score - villain score).
    pub sign: f64,
    /// Acting player (0 = hero, 1 = villain).
    pub actor: u8,
    /// Chips committed on the current street by [hero, villain].
    pub paid: [f64; 2],
    /// Current highest street bet.
    pub bet: f64,
    /// Size of the last bet/raise increment.
    pub last_raise: f64,
    /// Number of raises since the last bet (bounded by the game maximum).
    pub raises: u32,
    /// Consecutive checks (2 ends the street).
    pub checks: u32,
    /// Public action history, for example ["check", "bet:200"].
    pub history: Vec<String>,
    /// Terminal marker; None while the node is still being bet.
    pub end: Option<End>,
}

/// The bounded heads-up river game definition.
///
/// Mirrors the original JS game() object: the caller supplies the pot and stack
/// geometry once, then drives the tree with actions/child/payoff.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverGame {
    /// Pot before the current street's bets.
    pub base: f64,
    /// Effective betting cap (deepest stack covered by the opponent).
    pub ceiling: f64,
    /// Current street round bets of [hero, villain] at the root.
    pub paid: [f64; 2],
    /// Current street bet to match at the root.
    pub current_bet: f64,
    /// Minimum raise-to total at the root.
    pub min_raise_to: f64,
    /// Minimum bet (big blind).
    pub min_bet: f64,
    /// Raise-grid unit (small blind / ante).
    pub unit: f64,
    /// Whether the other live seat already checked on this street.
    pub checked_before: bool,
    /// Whether the hero may make a voluntary raise at the root.
    pub can_raise: bool,
    /// Maximum future raises (1 in production).
    pub max_raises: u32,
    /// Chance deals (joint private strength-bucket pairs).
    pub deals: Vec<Deal>,
}

impl RiverGame {
    /// Builds the game from raw pot/stack geometry.
    ///
    /// # Errors
    ///
    /// Returns AiError::RiverInvalid when the pot or stack geometry is not a
    /// supported heads-up river spot (same checks as the original JS game()).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pot: f64,
        paid: [f64; 2],
        stacks: [f64; 2],
        current_bet: f64,
        min_raise_to: f64,
        min_bet: f64,
        unit: f64,
        checked_before: bool,
        can_raise: bool,
        deals: Vec<Deal>,
    ) -> Result<Self, AiError> {
        let base = pot - paid[0] - paid[1];
        let ceiling = (stacks[0] + paid[0]).min(stacks[1] + paid[1]);
        let paid_max = paid[0].max(paid[1]);
        if base <= 0.0
            || paid[0] < 0.0
            || paid[1] < 0.0
            || stacks[0] <= 0.0
            || stacks[1] <= 0.0
            || current_bet != paid_max
            || current_bet > ceiling
        {
            return Err(AiError::RiverInvalid("unsupported river pot or stacks"));
        }
        Ok(Self {
            base,
            ceiling,
            paid,
            current_bet,
            min_raise_to,
            min_bet,
            unit,
            checked_before,
            can_raise,
            max_raises: 1,
            deals,
        })
    }

    /// Initial states paired with their chance-deal probabilities, in deal order.
    #[must_use]
    pub fn initial_states(&self) -> Vec<(State, f64)> {
        self.deals
            .iter()
            .map(|deal| {
                let state = State {
                    scores: deal.scores,
                    sign: deal.showdown_sign,
                    actor: 0,
                    paid: self.paid,
                    bet: self.current_bet,
                    last_raise: if self.current_bet != 0.0 {
                        self.min_raise_to - self.current_bet
                    } else {
                        self.min_bet
                    },
                    raises: 0,
                    checks: u32::from(self.checked_before),
                    history: Vec::new(),
                    end: None,
                };
                (state, deal.weight)
            })
            .collect()
    }

    /// True when the node is terminal.
    #[must_use]
    pub fn terminal(&self, state: &State) -> bool {
        state.end.is_some()
    }

    /// Acting player of a node.
    #[must_use]
    pub fn player(&self, state: &State) -> u8 {
        state.actor
    }

    /// Information-set key: acting player, own bucket and public action history.
    #[must_use]
    pub fn key(&self, state: &State, player: u8) -> String {
        format!(
            "{}:{}:{}",
            player,
            state.scores[usize::from(player)],
            state.history.join("|")
        )
    }

    /// Legal action menu for a node, in the original JS order.
    #[must_use]
    pub fn actions(&self, state: &State) -> Vec<String> {
        let actor = usize::from(state.actor);
        let call = state.bet - state.paid[actor];
        let mut menu = if call != 0.0 {
            vec!["fold".to_owned(), "call".to_owned()]
        } else {
            vec!["check".to_owned()]
        };
        if state.bet < self.ceiling
            && (!state.history.is_empty() || self.can_raise)
            && (call == 0.0 || state.raises < self.max_raises)
        {
            let minimum = if call != 0.0 {
                state.bet + state.last_raise
            } else {
                self.min_bet
            };
            let after_call = self.base + state.paid[0] + state.paid[1] + call;
            let mut targets = [0.33f64, 0.67, 1.0]
                .iter()
                .map(|fraction| state.bet + (after_call * fraction / self.unit).round() * self.unit)
                .collect::<Vec<f64>>();
            targets.push(self.ceiling);
            let minimum_target = (minimum / self.unit).ceil() * self.unit;
            let mut clamped = targets
                .iter()
                .map(|target| self.ceiling.min(minimum_target.max(*target)))
                .collect::<Vec<f64>>();
            clamped.sort_by(f64::total_cmp);
            clamped.dedup();
            let prefix = if call != 0.0 { "raise" } else { "bet" };
            for target in clamped {
                if target > state.bet && (target == self.ceiling || target % self.unit == 0.0) {
                    menu.push(format!("{prefix}:{}", format_number(target)));
                }
            }
        }
        menu
    }

    /// Applies one action and returns the child node.
    #[must_use]
    pub fn child(&self, state: &State, action: &str) -> State {
        let mut next = state.clone();
        next.actor = 1 - state.actor;
        next.history.push(action.to_owned());
        let actor = usize::from(state.actor);
        if action == "fold" {
            next.end = Some(End::Fold {
                winner: 1 - state.actor,
            });
        } else if action == "call" {
            next.paid[actor] = state.bet;
            next.end = Some(End::Showdown);
        } else if action == "check" {
            next.checks = state.checks + 1;
            if next.checks == 2 {
                next.end = Some(End::Showdown);
            }
        } else {
            let target = action
                .split_once(':')
                .and_then(|(_, suffix)| suffix.parse::<f64>().ok())
                .expect("compiled river action carries a numeric target");
            next.paid[actor] = target;
            next.last_raise = target - state.bet;
            next.raises = state.raises + u32::from(state.bet > 0.0);
            next.bet = target;
            next.checks = 0;
        }
        next
    }

    /// Terminal payoff of a node, zero-sum and from the hero's point of view.
    ///
    /// Folds pay half the pre-street pot plus the opponent's current street
    /// commitment; showdowns pay the conditional showdown sign times half the
    /// pre-street pot plus the hero's current street commitment. The final
    /// solve() step adds the remaining offset to report a net-chip EV.
    #[must_use]
    pub fn payoff(&self, state: &State) -> f64 {
        match state.end {
            Some(End::Fold { winner: 0 }) => self.base / 2.0 + state.paid[1],
            Some(End::Fold { .. }) => -(self.base / 2.0 + state.paid[0]),
            Some(End::Showdown) | None => state.sign * (self.base / 2.0 + state.paid[0]),
        }
    }
}

/// Converts a chip quantity to the original JS Number string form.
///
/// The original action strings interpolate JS numbers, which never show a
/// trailing .0 for integral values. Rust's Display for f64 behaves the same way
/// for the bounded chip quantities used here.
fn format_number(value: f64) -> String {
    format!("{value}")
}
