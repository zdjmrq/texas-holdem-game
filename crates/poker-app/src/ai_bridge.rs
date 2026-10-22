//! Adapter between the app and the real poker-ai crate.
//!
//! The app owns one AiPolicy for the whole local table; this module keeps one
//! long-lived poker_ai::AIPlayer per AI seat (cross-hand behavioural models
//! persist) and maps the app's decision view into a
//! poker_ai::input::RawDecisionContext.
//!
//! Contract: docs/specs/ai-server-adapter.md. The adapter only ever sees public
//! table state plus the acting seat's own hole cards. If poker-ai ever returns
//! an action outside the engine's legal set, the deterministic policy is used as
//! a safety fallback so the table can never get stuck.

use std::collections::HashMap;

use poker_core::{Card, Variant};
use poker_engine::{Action, ActionType, LegalActions, Phase, Player};

use crate::ai::{AiPolicy, AiView, DeterministicAi};

/// Whether the real-ai backend is wired in (always true in this build).
#[must_use]
pub fn real_ai_feature_enabled() -> bool {
    true
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
    /// Seats at the table (including folded/busted seats).
    pub players: Vec<Player>,
    pub current_bet: u64,
    pub dealer_pos: i32,
    pub sb_idx: i32,
    pub bb_idx: i32,
    pub actions_this_round: u32,
    pub preflop_raise_count: u32,
    pub last_aggressor: i32,
    pub last_aggressor_street: Option<Phase>,
    pub preflop_aggressor: i32,
    pub flop_had_aggression: bool,
    pub starting_stack: u64,
    /// Big blind, or the short-deck minimum bet.
    pub big_blind: u64,
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
            players: view.players.to_vec(),
            current_bet: view.current_bet,
            dealer_pos: view.dealer_pos,
            sb_idx: view.sb_idx,
            bb_idx: view.bb_idx,
            actions_this_round: view.actions_this_round,
            preflop_raise_count: view.preflop_raise_count,
            last_aggressor: view.last_aggressor,
            last_aggressor_street: view.last_aggressor_street,
            preflop_aggressor: view.preflop_aggressor,
            flop_had_aggression: view.flop_had_aggression,
            starting_stack: view.starting_stack,
            big_blind: view.big_blind,
        }
    }
}

/// Implemented by the real poker-ai adapter.
pub trait PokerAiBackend: Send {
    fn name(&self) -> &str;
    /// Return None to delegate to the deterministic fallback.
    fn decide(&mut self, input: &BridgeInput) -> Option<Action>;
    fn observe(&mut self, _event: &poker_ai::ObservedAction) {}
    fn showdown(&mut self, _seat: usize, _result: &poker_ai::ShowdownResult) {}
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
    fn showdown(&mut self, seat: usize, result: &poker_ai::ShowdownResult) {
        if let Some(backend) = &mut self.backend {
            backend.showdown(seat, result);
        }
    }
    fn observe(&mut self, event: &poker_ai::ObservedAction) {
        if let Some(backend) = &mut self.backend {
            backend.observe(event);
        }
    }
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

/// Production backend: one poker-ai AIPlayer per seat, built lazily.
pub struct PokerAiBackendImpl {
    short_deck: bool,
    starting_stack: u64,
    brains: HashMap<usize, poker_ai::AIPlayer>,
}

impl PokerAiBackendImpl {
    /// Creates the backend for a standard or short-deck table.
    #[must_use]
    pub fn new(short_deck: bool, starting_stack: u64) -> Self {
        Self {
            short_deck,
            starting_stack,
            brains: HashMap::new(),
        }
    }

    fn brain(&mut self, seat: usize) -> &mut poker_ai::AIPlayer {
        let short_deck = self.short_deck;
        let starting_stack = self.starting_stack;
        self.brains.entry(seat).or_insert_with(|| {
            let variant = if short_deck {
                Variant::ShortDeck
            } else {
                Variant::Standard
            };
            let source = if short_deck {
                poker_ai::ProfileSource::ShortDeckLocalStyle
            } else {
                poker_ai::ProfileSource::StandardStyle
            };
            let styles = poker_ai::Style::ALL;
            let style = styles[seat % styles.len()];
            poker_ai::AIPlayer::new(
                &format!("AI{seat}"),
                style,
                starting_stack as f64,
                seat,
                variant,
                source,
            )
        })
    }
}

impl PokerAiBackend for PokerAiBackendImpl {
    fn showdown(&mut self, seat: usize, result: &poker_ai::ShowdownResult) {
        for brain in self.brains.values_mut() {
            brain.record_showdown(seat, result);
        }
    }
    fn observe(&mut self, event: &poker_ai::ObservedAction) {
        for seat in 0..10 {
            self.brain(seat).record_opponent_action(event);
        }
    }
    fn name(&self) -> &str {
        "poker-ai v1.5.8 port"
    }

    fn decide(&mut self, input: &BridgeInput) -> Option<Action> {
        let brain = self.brain(input.seat);
        brain.set_hole_cards(&input.hole_cards);
        let mut context = build_raw_context(input).normalize();
        poker_ai::search::deepen_context(&mut context);
        let decision = brain.decide(&context);
        Some(match decision.action {
            ActionType::Fold => Action::Fold,
            ActionType::Check => Action::Check,
            ActionType::Call => Action::Call,
            ActionType::Raise => {
                if decision.amount == 0 {
                    Action::Call
                } else {
                    Action::Raise {
                        to: decision.amount,
                    }
                }
            }
            ActionType::AllIn => Action::AllIn,
        })
    }
}

fn poker_ai_legal_action(action: ActionType) -> poker_ai::input::LegalAction {
    match action {
        ActionType::Fold => poker_ai::input::LegalAction::Fold,
        ActionType::Check => poker_ai::input::LegalAction::Check,
        ActionType::Call => poker_ai::input::LegalAction::Call,
        ActionType::Raise => poker_ai::input::LegalAction::Raise,
        ActionType::AllIn => poker_ai::input::LegalAction::AllIn,
    }
}

fn street(phase: Phase) -> Option<poker_ai::input::Street> {
    match phase {
        Phase::Preflop => Some(poker_ai::input::Street::Preflop),
        Phase::Flop => Some(poker_ai::input::Street::Flop),
        Phase::Turn => Some(poker_ai::input::Street::Turn),
        Phase::River => Some(poker_ai::input::Street::River),
        Phase::Idle | Phase::Showdown => None,
    }
}

fn position_from_button(input: &BridgeInput, seat: usize) -> Option<i32> {
    let n = input.players.len() as i32;
    if n == 0 || input.dealer_pos < 0 {
        return None;
    }
    let mut seats: Vec<usize> = input
        .players
        .iter()
        .enumerate()
        .filter(|(_, player)| player.dealt_in)
        .map(|(index, _)| index)
        .collect();
    seats.sort_by_key(|candidate| (*candidate as i32 - input.dealer_pos + n) % n);
    seats
        .iter()
        .position(|candidate| *candidate == seat)
        .map(|index| i32::try_from(index).unwrap_or(0))
}

#[allow(clippy::too_many_lines)]
fn build_raw_context(input: &BridgeInput) -> poker_ai::input::RawDecisionContext {
    use poker_ai::input::{PublicPlayer, RawDecisionContext, RawLegalActions, SeatRef};

    let seat = input.seat;
    let own_round_bet = input.players.get(seat).map_or(0, |player| player.round_bet);
    let in_hand: Vec<usize> = input
        .players
        .iter()
        .enumerate()
        .filter(|(_, player)| !player.folded && player.dealt_in)
        .map(|(index, _)| index)
        .collect();
    let opponents: Vec<usize> = in_hand
        .iter()
        .copied()
        .filter(|candidate| *candidate != seat)
        .collect();
    let num_opponents = opponents.len();
    let effective_stack = opponents
        .iter()
        .map(|candidate| {
            input.players[*candidate]
                .stack
                .saturating_add(input.players[*candidate].round_bet)
        })
        .max()
        .map_or(input.stack, |max_opponent| input.stack.min(max_opponent));
    let previous_street = match input.phase {
        Phase::Flop => Some(Phase::Preflop),
        Phase::Turn => Some(Phase::Flop),
        Phase::River => Some(Phase::Turn),
        _ => None,
    };
    let is_previous_street_aggressor = input.last_aggressor == i32::try_from(seat).unwrap_or(-1)
        && input.last_aggressor_street.is_some()
        && input.last_aggressor_street == previous_street;
    let is_delayed_cbet_candidate = input.phase == Phase::Turn
        && input.preflop_aggressor == i32::try_from(seat).unwrap_or(-1)
        && !input.flop_had_aggression;
    let public_players: Vec<PublicPlayer> = input
        .players
        .iter()
        .enumerate()
        .map(|(index, player)| PublicPlayer {
            seat: index,
            stack: player.stack as f64,
            committed: player.chips_in_pot as f64,
            round_bet: player.round_bet as f64,
            position_from_button: position_from_button(input, index),
            is_small_blind: input.sb_idx == i32::try_from(index).unwrap_or(-1),
            is_big_blind: input.bb_idx == i32::try_from(index).unwrap_or(-1),
            folded: player.folded,
            all_in: player.is_all_in,
        })
        .collect();
    let active_opponents: Vec<SeatRef> = opponents
        .iter()
        .map(|candidate| SeatRef::Number(*candidate))
        .collect();
    let variant_name = if input.variant.is_short_deck() {
        "shortdeck"
    } else {
        "standard"
    };
    RawDecisionContext {
        hand_id: input.hand_id,
        decision_id: poker_engine::rules::decision_identity(
            input.variant,
            input.phase.as_str(),
            input.actions_this_round,
            seat,
        ),
        seed: Some(format!("poker-table:{variant_name}")),
        variant: Some(variant_name.to_owned()),
        is_short_deck: input.variant.is_short_deck(),
        street: street(input.phase),
        phase: Some(input.phase.as_str().to_owned()),
        hero_seat: Some(seat),
        current_player_index: Some(seat),
        hole_cards: input.hole_cards.clone(),
        community_cards: input.community_cards.clone(),
        pot: input.pot as f64,
        current_bet: input.current_bet as f64,
        to_call: Some(input.to_call as f64),
        your_bet: Some(own_round_bet as f64),
        hero_round_bet: Some(own_round_bet as f64),
        stack: Some(input.stack as f64),
        hero_stack: Some(input.stack as f64),
        effective_stack: Some(effective_stack as f64),
        min_raise_to: Some(input.legal.min_raise_to as f64),
        max_raise_to: Some(input.legal.max_raise_to as f64),
        can_raise: Some(input.legal.can_raise),
        can_check: Some(input.legal.contains(ActionType::Check)),
        legal_actions: Some(RawLegalActions::List(
            input
                .legal
                .actions
                .iter()
                .copied()
                .map(poker_ai_legal_action)
                .collect(),
        )),
        active_opponent_seats: active_opponents,
        num_opponents: Some(num_opponents),
        num_opponents_active: Some(num_opponents),
        player_count: Some(in_hand.len().max(2)),
        table_player_count: Some(input.player_count),
        starting_stack: Some(input.starting_stack as f64),
        position_from_button: position_from_button(input, seat),
        is_small_blind: input.sb_idx == i32::try_from(seat).unwrap_or(-1),
        is_big_blind: input.bb_idx == i32::try_from(seat).unwrap_or(-1),
        preflop_raise_count: Some(input.preflop_raise_count),
        is_previous_street_aggressor,
        is_delayed_cbet_candidate,
        big_blind: Some(input.big_blind as f64),
        wager_unit: Some(input.legal.wager_unit as f64),
        time_budget_ms: Some(if input.phase == Phase::River || num_opponents >= 4 {
            150.0
        } else {
            80.0
        }),
        public_players,
        ..RawDecisionContext::default()
    }
}

/// Policy factory used by TableSession: the real poker-ai backend for both
/// variants, with the deterministic policy as an illegal-action safety net.
#[must_use]
pub fn create_policy(seed: &str, short_deck: bool, starting_stack: u64) -> Box<dyn AiPolicy> {
    let _ = seed;
    Box::new(crate::ai_background::BackgroundAi::new(Box::new(
        PokerAiBridge::with_backend(Box::new(PokerAiBackendImpl::new(
            short_deck,
            starting_stack,
        ))),
    )))
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
            current_bet: 100,
            dealer_pos: 0,
            sb_idx: 1,
            bb_idx: 2,
            actions_this_round: 2,
            preflop_raise_count: 1,
            last_aggressor: 2,
            last_aggressor_street: Some(Phase::Preflop),
            preflop_aggressor: 2,
            flop_had_aggression: false,
            starting_stack: 5_000,
            big_blind: 100,
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
    fn real_backend_is_compiled_in() {
        assert!(real_ai_feature_enabled());
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
        let mut policy = create_policy("seed", false, 5_000);
        let action = policy.decide(&view(&hole, &board, &legal, &players));
        assert!(legal.contains(action.action_type()));
        let _ = crate::ai::fallback_action(&legal);
    }
}
