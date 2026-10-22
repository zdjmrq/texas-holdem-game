//! Adapter layer between the app and the real poker-ai crate.
//!
//! poker-ai is intentionally NOT a dependency yet: Phase 4 is still being
//! migrated in parallel and must not break the default app build. The shape of
//! the integration is fixed here so wiring the real crate later is a localised
//! change:
//!
//! 1. add `poker-ai = { workspace = true, optional = true }` to Cargo.toml and
//!    map feature real-ai to it (`real-ai = ["dep:poker-ai"]`);
//! 2. implement PokerAiBackend for a thin adapter that maps BridgeInput into
//!    the poker-ai decision input and maps its chosen action back to Action;
//! 3. return PokerAiBridge::with_backend(...) from create_policy when the
//!    feature is enabled. Until then the deterministic policy is the fallback.
//!
//! The feature is a no-op today, so `cargo build` / `cargo test` (default
//! features) stay green.

use poker_core::{Card, Variant};
use poker_engine::{Action, ActionType, LegalActions, Phase};

use crate::ai::{AiPolicy, AiView, DeterministicAi};

/// Feature flag: whether the real-ai integration is compiled in.
#[must_use]
pub fn real_ai_feature_enabled() -> bool {
    cfg!(feature = "real-ai")
}

/// Owned copy of the decision-time state, shaped for a future poker-ai adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeInput {
    pub seat: usize,
    pub phase: Phase,
    pub hand_id: u64,
    pub turn_id: u64,
    pub hole_cards: Vec<Card>,
    pub community_cards: Vec<Card>,
    pub pot: u64,
    pub to_call: u64,
    pub stack: u64,
    pub legal: LegalActions,
    pub variant: Variant,
    pub player_count: usize,
}

impl BridgeInput {
    #[must_use]
    pub fn from_view(view: &AiView<'_>) -> Self {
        Self {
            seat: view.seat,
            phase: view.phase,
            hand_id: view.hand_id,
            turn_id: view.turn_id,
            hole_cards: view.hole_cards.to_vec(),
            community_cards: view.community_cards.to_vec(),
            pot: view.pot,
            to_call: view.to_call,
            stack: view.stack,
            legal: view.legal.clone(),
            variant: view.variant,
            player_count: view.players.len(),
        }
    }
}

/// Implemented by the real poker-ai adapter.
pub trait PokerAiBackend: Send {
    fn name(&self) -> &str;
    /// Return None to delegate to the deterministic fallback.
    fn decide(&mut self, input: &BridgeInput) -> Option<Action>;
}

/// AiPolicy adapter: delegates to a backend when present, otherwise (or on an
/// illegal/absent decision) falls back to the deterministic policy.
pub struct PokerAiBridge {
    backend: Option<Box<dyn PokerAiBackend>>,
    fallback: DeterministicAi,
}

impl PokerAiBridge {
    /// Bridge without a real backend (skeleton state).
    #[must_use]
    pub fn without_backend() -> Self {
        Self {
            backend: None,
            fallback: DeterministicAi,
        }
    }

    /// Bridge wrapping a real poker-ai adapter.
    #[must_use]
    pub fn with_backend(backend: Box<dyn PokerAiBackend>) -> Self {
        Self {
            backend: Some(backend),
            fallback: DeterministicAi,
        }
    }

    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }
}

impl AiPolicy for PokerAiBridge {
    fn name(&self) -> &str {
        match &self.backend {
            Some(backend) => backend.name(),
            None => "PokerAiBridge（确定性回退）",
        }
    }

    fn decide(&mut self, view: &AiView<'_>) -> Action {
        if let Some(backend) = &mut self.backend {
            let input = BridgeInput::from_view(view);
            if let Some(action) = backend.decide(&input) {
                if is_legal(action, view.legal) {
                    return action;
                }
                log::warn!(
                    "poker-ai backend returned an illegal action {:?}; falling back",
                    action
                );
            }
        }
        self.fallback.decide(view)
    }
}

fn is_legal(action: Action, legal: &LegalActions) -> bool {
    if !legal.contains(action.action_type()) {
        return false;
    }
    if let Action::Raise { to } = action {
        return to >= legal.min_raise_to && to <= legal.max_raise_to;
    }
    if action.action_type() == ActionType::Call && legal.to_call == 0 {
        return false;
    }
    true
}

/// Policy factory used by TableSession. Enables the real backend when the
/// feature is compiled in and an adapter exists; otherwise deterministic.
#[must_use]
pub fn create_policy(seed: &str, short_deck: bool) -> Box<dyn AiPolicy> {
    let _ = (seed, short_deck);
    if real_ai_feature_enabled() {
        log::warn!(
            "real-ai feature enabled but the poker-ai adapter is not wired yet (Phase 4 in progress); using the deterministic fallback"
        );
    }
    Box::new(PokerAiBridge::without_backend())
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::Variant;
    use poker_engine::{LegalActions, Phase, Player};

    fn players() -> Vec<Player> {
        (0..3)
            .map(|seat| Player::new(format!("p{seat}"), seat, format!("P{seat}"), 5_000))
            .collect()
    }

    fn legal() -> LegalActions {
        LegalActions {
            actions: vec![ActionType::Fold, ActionType::Call, ActionType::Raise],
            to_call: 100,
            can_raise: true,
            min_raise_to: 200,
            max_raise_to: 4_900,
            wager_unit: 50,
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
            hand_id: 3,
            turn_id: 2,
            hole_cards: hole,
            community_cards: board,
            pot: 800,
            to_call: 100,
            stack: 4_900,
            legal,
            variant: Variant::Standard,
            players,
        }
    }

    struct FakeBackend {
        calls: usize,
        action: Option<Action>,
    }

    impl PokerAiBackend for FakeBackend {
        fn name(&self) -> &str {
            "fake-poker-ai"
        }

        fn decide(&mut self, input: &BridgeInput) -> Option<Action> {
            self.calls += 1;
            assert_eq!(input.hole_cards.len(), 2);
            assert_eq!(input.player_count, 3);
            self.action
        }
    }

    #[test]
    fn feature_is_disabled_by_default() {
        assert!(!real_ai_feature_enabled());
    }

    #[test]
    fn bridge_without_backend_falls_back_and_stays_legal() {
        let hole: Vec<Card> = ["As", "Ah"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["2c", "7d", "9s"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = legal();
        let players = players();
        let mut bridge = PokerAiBridge::without_backend();
        assert!(!bridge.has_backend());
        let action = bridge.decide(&view(&hole, &board, &legal, &players));
        assert!(legal.contains(action.action_type()));
        assert!(bridge.name().contains("回退"));
    }

    #[test]
    fn bridge_uses_backend_action_when_legal() {
        let hole: Vec<Card> = ["As", "Ah"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["2c", "7d", "9s"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = legal();
        let players = players();
        let backend = Box::new(FakeBackend {
            calls: 0,
            action: Some(Action::Fold),
        });
        let mut bridge = PokerAiBridge::with_backend(backend);
        assert!(bridge.has_backend());
        let action = bridge.decide(&view(&hole, &board, &legal, &players));
        assert_eq!(action, Action::Fold);
    }

    #[test]
    fn bridge_rejects_illegal_backend_action_and_falls_back() {
        let hole: Vec<Card> = ["2c", "7d"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["9s", "Jh", "Qd"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = LegalActions {
            actions: vec![ActionType::Fold, ActionType::Call],
            to_call: 100,
            can_raise: false,
            min_raise_to: 0,
            max_raise_to: 0,
            wager_unit: 50,
        };
        let players = players();
        let backend = Box::new(FakeBackend {
            calls: 0,
            action: Some(Action::Raise { to: 400 }),
        });
        let mut bridge = PokerAiBridge::with_backend(backend);
        let action = bridge.decide(&view(&hole, &board, &legal, &players));
        assert!(
            legal.contains(action.action_type()),
            "must fall back to a legal action"
        );
        assert_ne!(action.action_type(), ActionType::Raise);
    }

    #[test]
    fn create_policy_returns_a_working_policy() {
        let hole: Vec<Card> = ["As", "Kh"].iter().map(|c| c.parse().unwrap()).collect();
        let board: Vec<Card> = ["Qh", "Jh", "2c"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let legal = legal();
        let players = players();
        let mut policy = create_policy("seed", false);
        let action = policy.decide(&view(&hole, &board, &legal, &players));
        assert!(legal.contains(action.action_type()));
        let _ = crate::ai::fallback_action(&legal);
    }
}
