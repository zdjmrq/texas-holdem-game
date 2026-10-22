//! AI seat abstraction and the deterministic stub strategy.
//!
//! The room server never embeds a concrete AI implementation. Each AI seat owns
//! a ServerAi brain created by an AiFactory; when it is that seat's turn the
//! room takes the brain out of the locked state, runs decide() on a blocking
//! thread pool, then reinserts it after re-validating the decision generation.
//! That keeps poker-ai (or any future implementation) replaceable without
//! touching the protocol/room layer.

use poker_core::Card;
use poker_engine::{Action, LegalActions, Phase};

/// Names used by the reference server for its AI seats, in seat order.
pub const AI_NAMES: [&str; 10] = [
    "Alex", "Blake", "Casey", "Drew", "Emma", "Finn", "Grace", "Hayes", "Ivy", "Jade",
];

/// Avatar emojis used by the reference server for its AI seats.
pub const AI_AVATARS: [&str; 10] = ["😎", "🤠", "🕶️", "🎩", "👑", "🦊", "🐺", "🦅", "🐯", "🐉"];

/// Immutable snapshot handed to an AI brain for one decision.
///
/// It contains only public information plus the seat's own hole cards, so the
/// same view can later be filled by the poker-ai adapter.
#[derive(Clone, Debug)]
pub struct AiView {
    /// Acting seat index.
    pub seat: usize,
    /// Number of seats at the table.
    pub seat_count: usize,
    /// Starting stack of the room (for stack-depth style features).
    pub starting_stack: u64,
    /// The acting seat's remaining stack.
    pub stack: u64,
    /// Total pot.
    pub pot: u64,
    /// Current bet to match.
    pub current_bet: u64,
    /// Chips required to call (capped at stack).
    pub to_call: u64,
    /// Smallest legal raise total.
    pub min_raise_to: u64,
    /// Largest legal raise total.
    pub max_raise_to: u64,
    /// Legal action set for the decision.
    pub legal_actions: LegalActions,
    /// Current betting phase.
    pub phase: Phase,
    /// Community cards.
    pub community_cards: Vec<Card>,
    /// The acting seat's hole cards.
    pub hole_cards: Vec<Card>,
    /// Stacks of the seats still in the hand (excluding the acting seat).
    pub opponent_stacks: Vec<u64>,
    /// Hand counter.
    pub hand_id: u64,
    /// Decision counter.
    pub turn_id: u64,
    /// Big blind / minimum bet for the variant.
    pub minimum_bet: u64,
    /// Raise grid unit for the variant.
    pub wager_unit: u64,
    /// True for 6+ short deck.
    pub is_short_deck: bool,
}

/// One AI decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiDecision {
    /// The engine action to apply.
    pub action: Action,
}

impl AiDecision {
    /// Wraps an engine action.
    pub fn new(action: Action) -> Self {
        Self { action }
    }
}

/// Pluggable AI brain for one server-side seat.
pub trait ServerAi: Send {
    /// Decides the next action for the given view. Returning None leaves the
    /// seat to the deterministic fallback (check/call/fold).
    fn decide(&mut self, view: &AiView) -> Option<AiDecision>;
}

/// Creates AI brains for filled seats.
pub trait AiFactory: Send + Sync {
    /// Creates a brain for a seat. starting_stack is the room starting stack.
    fn create(&self, seat_id: usize, starting_stack: u64) -> Box<dyn ServerAi>;
}

/// Factory for the deterministic stub strategy.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubAiFactory;

impl AiFactory for StubAiFactory {
    fn create(&self, _seat_id: usize, _starting_stack: u64) -> Box<dyn ServerAi> {
        Box::new(StubAi)
    }
}

/// Deterministic check/call stub used until poker-ai is wired in.
///
/// Strategy: check when possible, otherwise call whenever calling is legal,
/// otherwise fold. It never raises, which keeps integration tests fast and
/// fully deterministic while still exercising streets and showdowns.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubAi;

impl ServerAi for StubAi {
    fn decide(&mut self, view: &AiView) -> Option<AiDecision> {
        use poker_engine::ActionType;
        if view.legal_actions.contains(ActionType::Check) {
            return Some(AiDecision::new(Action::Check));
        }
        if view.legal_actions.contains(ActionType::Call) {
            return Some(AiDecision::new(Action::Call));
        }
        if view.legal_actions.contains(ActionType::Fold) {
            return Some(AiDecision::new(Action::Fold));
        }
        None
    }
}
