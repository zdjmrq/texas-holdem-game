//! Replaceable AI policy interface and the temporary deterministic policy.
//!
//! The real implementation will live in poker-ai; the app only depends on the
//! AiPolicy trait so the stub can be swapped without touching session/UI code.
//! The stub is deterministic: the same public state always yields the same
//! action, which keeps screenshots and tests reproducible.

use poker_core::{Card, Variant, evaluate};
use poker_engine::{Action, ActionType, LegalActions, Phase, Player};

/// Everything a policy may observe: public table state plus its own hole cards.
pub struct AiView<'a> {
    pub seat: usize,
    pub phase: Phase,
    pub hand_id: u64,
    pub turn_id: u64,
    pub hole_cards: &'a [Card],
    pub community_cards: &'a [Card],
    pub pot: u64,
    pub to_call: u64,
    pub stack: u64,
    pub legal: &'a LegalActions,
    pub variant: Variant,
    pub players: &'a [Player],
}

/// A pluggable opponent policy.
pub trait AiPolicy: Send {
    fn name(&self) -> &str;
    /// Decide an action. Implementations must return a legal action.
    fn decide(&mut self, view: &AiView<'_>) -> Action;
}

/// Deterministic temporary policy (placeholder until poker-ai is ready).
#[derive(Debug, Default)]
pub struct DeterministicAi;

impl AiPolicy for DeterministicAi {
    fn name(&self) -> &str {
        "确定性策略桩（临时，poker-ai 就绪后替换）"
    }

    fn decide(&mut self, view: &AiView<'_>) -> Action {
        let legal = view.legal;
        if legal.actions.is_empty() {
            return Action::Fold;
        }
        let strength = hand_strength(view);
        let roll = deterministic_roll(view);
        let can_raise =
            legal.contains(ActionType::Raise) && legal.min_raise_to < legal.max_raise_to;
        let raise_action = || Action::Raise {
            to: raise_target(legal),
        };

        // Short stacks and very strong hands are willing to commit everything.
        if legal.contains(ActionType::AllIn) {
            let short_stack = view.to_call > 0
                && view.stack <= view.to_call.saturating_mul(2)
                && strength >= 3
                && roll < 60;
            let monster = strength >= 6 && roll < 35;
            if short_stack || monster {
                return Action::AllIn;
            }
        }

        if view.to_call == 0 {
            if can_raise && (strength >= 4 || roll < 12) {
                return raise_action();
            }
            if legal.contains(ActionType::Check) {
                return Action::Check;
            }
            return fallback_action(legal);
        }

        // Facing a bet.
        let expensive = view.to_call.saturating_mul(4) > view.stack.max(1);
        if expensive && strength <= 1 && roll < 75 {
            return legal_action_or(legal, Action::Fold);
        }
        if can_raise && ((strength >= 5 && roll < 55) || (strength >= 7 && roll < 85)) {
            return raise_action();
        }
        legal_action_or(legal, Action::Call)
    }
}

/// Strength bucket derived from the evaluator rank index (0..=9).
fn hand_strength(view: &AiView<'_>) -> u8 {
    let mut cards = view.hole_cards.to_vec();
    cards.extend_from_slice(view.community_cards);
    if cards.len() < 2 || cards.len() > 7 {
        return 0;
    }
    evaluate(&cards, view.variant).rank_index()
}

/// Stable pseudo-random roll in 0..100 derived from the decision identity.
fn deterministic_roll(view: &AiView<'_>) -> u32 {
    let mut hash: u64 = 1_469_598_103_934_665_603;
    for byte in view.hand_id.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    for byte in view.turn_id.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash ^= view.seat as u64;
    hash = hash.wrapping_mul(1_099_511_628_211);
    for card in view.hole_cards {
        hash ^= u64::from(card.index()) + 1;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    (hash % 100) as u32
}

/// A raise target: one grid step above the minimum, clamped and snapped.
fn raise_target(legal: &LegalActions) -> u64 {
    let candidate = legal
        .min_raise_to
        .saturating_add(legal.wager_unit)
        .min(legal.max_raise_to);
    poker_engine::snap_raise(
        candidate as f64,
        legal.min_raise_to,
        legal.max_raise_to,
        legal.wager_unit,
    )
    .unwrap_or(legal.min_raise_to)
}

/// Fallback action that is always legal at a decision point.
#[must_use]
pub fn fallback_action(legal: &LegalActions) -> Action {
    if legal.contains(ActionType::Check) {
        Action::Check
    } else if legal.contains(ActionType::Call) {
        Action::Call
    } else {
        Action::Fold
    }
}

fn legal_action_or(legal: &LegalActions, action: Action) -> Action {
    if legal.contains(action.action_type()) {
        action
    } else {
        fallback_action(legal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::Variant;
    use poker_engine::{ActionType, Phase, Player};

    fn players() -> Vec<Player> {
        (0..3)
            .map(|seat| Player::new(format!("p{seat}"), seat, format!("P{seat}"), 5_000))
            .collect()
    }

    fn legal(actions: Vec<ActionType>, min: u64, max: u64, unit: u64) -> LegalActions {
        let can_raise = actions.contains(&ActionType::Raise);
        LegalActions {
            actions,
            to_call: 100,
            can_raise,
            min_raise_to: min,
            max_raise_to: max,
            wager_unit: unit,
        }
    }

    fn view<'a>(
        hole: &'a [Card],
        board: &'a [Card],
        legal: &'a LegalActions,
        players: &'a [Player],
    ) -> AiView<'a> {
        AiView {
            seat: 1,
            phase: Phase::Flop,
            hand_id: 7,
            turn_id: 3,
            hole_cards: hole,
            community_cards: board,
            pot: 1_000,
            to_call: 100,
            stack: 4_900,
            legal,
            variant: Variant::Standard,
            players,
        }
    }

    #[test]
    fn deterministic_policy_is_stable_for_the_same_state() {
        let hole: Vec<Card> = ["As", "Ah"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["2c", "7d", "9s"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = legal(
            vec![
                ActionType::Fold,
                ActionType::Call,
                ActionType::Raise,
                ActionType::AllIn,
            ],
            200,
            4_900,
            50,
        );
        let players = players();
        let mut policy = DeterministicAi;
        let first = policy.decide(&view(&hole, &board, &legal, &players));
        let second = policy.decide(&view(&hole, &board, &legal, &players));
        assert_eq!(first, second, "stub must be deterministic");
        assert!(legal.contains(first.action_type()));
    }

    #[test]
    fn weak_hand_folds_to_a_large_bet_sometimes() {
        let hole: Vec<Card> = ["2c", "7d"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["9s", "Jh", "Qd"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = legal(vec![ActionType::Fold, ActionType::Call], 400, 400, 50);
        let players = players();
        let mut policy = DeterministicAi;
        let mut folds = 0;
        for turn in 0..20_u64 {
            let mut test_view = view(&hole, &board, &legal, &players);
            test_view.turn_id = turn;
            test_view.to_call = 2_000;
            if policy.decide(&test_view) == Action::Fold {
                folds += 1;
            }
        }
        assert!(
            folds > 0,
            "stub should sometimes fold weak hands to big bets"
        );
    }

    #[test]
    fn fallback_action_is_always_legal() {
        let check = legal(vec![ActionType::Check], 0, 0, 1);
        assert_eq!(fallback_action(&check), Action::Check);
        let call = legal(vec![ActionType::Call], 1, 1, 1);
        assert_eq!(fallback_action(&call), Action::Call);
        let fold = legal(vec![ActionType::Fold], 1, 1, 1);
        assert_eq!(fallback_action(&fold), Action::Fold);
    }
}
