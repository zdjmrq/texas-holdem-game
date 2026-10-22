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
    /// Complete public decision context plus this seat's own cards.
    pub context: poker_ai::input::RawDecisionContext,
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
    /// Observe an accepted public action before the next decision.
    fn observe(&mut self, _event: &poker_ai::input::ObservedAction) {}
    /// Record only publicly revealed showdown evidence.
    fn showdown(&mut self, _seat: usize, _result: &poker_ai::ShowdownResult) {}
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

/// Production v1.5.8 AI factory; each seat retains its own behavioural model.
#[derive(Clone, Copy, Debug, Default)]
pub struct PokerAiFactory;
impl AiFactory for PokerAiFactory {
    fn create(&self, seat_id: usize, starting_stack: u64) -> Box<dyn ServerAi> {
        Box::new(PokerAi {
            seat: seat_id,
            starting_stack,
            brain: None,
            pending: Vec::new(),
        })
    }
}
struct PokerAi {
    seat: usize,
    starting_stack: u64,
    brain: Option<(bool, poker_ai::AIPlayer)>,
    pending: Vec<poker_ai::input::ObservedAction>,
}
impl ServerAi for PokerAi {
    fn showdown(&mut self, seat: usize, result: &poker_ai::ShowdownResult) {
        if let Some((_, brain)) = &mut self.brain {
            brain.record_showdown(seat, result);
        }
    }
    fn decide(&mut self, view: &AiView) -> Option<AiDecision> {
        if self
            .brain
            .as_ref()
            .is_none_or(|(short, _)| *short != view.is_short_deck)
        {
            let variant = if view.is_short_deck {
                poker_core::Variant::ShortDeck
            } else {
                poker_core::Variant::Standard
            };
            let source = if view.is_short_deck {
                poker_ai::ProfileSource::ShortDeckLocalStyle
            } else {
                poker_ai::ProfileSource::StandardStyle
            };
            self.brain = Some((
                view.is_short_deck,
                poker_ai::AIPlayer::new(
                    AI_NAMES[self.seat % 10],
                    poker_ai::Style::ALL[self.seat % 6],
                    self.starting_stack as f64,
                    self.seat,
                    variant,
                    source,
                ),
            ));
        }
        let brain = &mut self.brain.as_mut()?.1;
        for event in self
            .pending
            .drain(..)
            .filter(|event| event.hand_id == view.hand_id)
        {
            brain.record_opponent_action(&event);
        }
        brain.set_hole_cards(&view.hole_cards);
        let mut raw = view.context.clone();
        poker_ai::search::deepen(&mut raw);
        let d = brain.decide(&raw.normalize());
        use poker_engine::{Action, ActionType};
        Some(AiDecision::new(match d.action {
            ActionType::Fold => Action::Fold,
            ActionType::Check => Action::Check,
            ActionType::Call => Action::Call,
            ActionType::Raise => Action::Raise { to: d.amount },
            ActionType::AllIn => Action::AllIn,
        }))
    }
    fn observe(&mut self, event: &poker_ai::input::ObservedAction) {
        if let Some((_, brain)) = &mut self.brain {
            brain.record_opponent_action(event);
        } else {
            self.pending.retain(|old| old.hand_id == event.hand_id);
            self.pending.push(event.clone());
        }
    }
}

/// Build the AI contract without exposing other players' hole cards or the deck.
pub fn decision_context(
    game: &poker_engine::Game,
    seat: usize,
) -> poker_ai::input::RawDecisionContext {
    use serde_json::json;
    let legal = game.legal_actions(seat);
    let hero = &game.players()[seat];
    let active: Vec<_> = game
        .players()
        .iter()
        .enumerate()
        .filter(|(i, p)| *i != seat && p.dealt_in && !p.folded)
        .map(|(i, _)| i)
        .collect();
    let n = game.players().len() as i32;
    let position = |i: usize| {
        if game.dealer_pos() < 0 {
            None
        } else {
            Some((i as i32 - game.dealer_pos() + n) % n)
        }
    };
    let cards = |cs: &[poker_core::Card]| {
        cs.iter()
            .map(|c| json!({"suit":c.suit().as_str(),"rank":c.rank().as_str()}))
            .collect::<Vec<_>>()
    };
    let public:Vec<_>=game.players().iter().enumerate().map(|(i,p)|json!({"seat":i,"stack":p.stack,"committed":p.chips_in_pot,"roundBet":p.round_bet,"positionFromButton":position(i),"isSmallBlind":game.sb_idx()==i as i32,"isBigBlind":game.bb_idx()==i as i32,"folded":p.folded,"allIn":p.is_all_in})).collect();
    let variant = if game.is_short_deck() {
        "shortdeck"
    } else {
        "standard"
    };
    let previous = match game.phase() {
        Phase::Flop => Some(Phase::Preflop),
        Phase::Turn => Some(Phase::Flop),
        Phase::River => Some(Phase::Turn),
        _ => None,
    };
    serde_json::from_value(json!({
        "handId":game.hand_id(),"decisionId":poker_engine::rules::decision_identity(game.config().variant,game.phase().as_str(),game.actions_this_round(),seat),
        "seed":format!("poker-table:{variant}"),"variant":variant,"phase":game.phase().as_str(),"heroSeat":seat,
        "holeCards":cards(&hero.hole_cards),"communityCards":cards(game.community_cards()),"pot":game.pot(),"currentBet":game.current_bet(),"toCall":legal.to_call,
        "heroRoundBet":hero.round_bet,"heroStack":hero.stack,"effectiveStack":active.iter().map(|i|game.players()[*i].stack).max().unwrap_or(hero.stack).min(hero.stack),
        "minRaiseTo":legal.min_raise_to,"maxRaiseTo":legal.max_raise_to,"canRaise":legal.can_raise,"canCheck":legal.contains(poker_engine::ActionType::Check),
        "legalActions":legal.actions.iter().map(|a|a.as_str()).collect::<Vec<_>>(),"activeOpponentSeats":active,"tablePlayerCount":game.players().len(),"startingStack":game.config().starting_stack,
        "playerCount":game.players().iter().filter(|p|p.dealt_in).count(),"positionFromButton":position(seat),"isSmallBlind":game.sb_idx()==seat as i32,"isBigBlind":game.bb_idx()==seat as i32,
        "preflopRaiseCount":game.preflop_raise_count(),"isPreviousStreetAggressor":game.last_aggressor()==seat as i32 && previous.is_some() && game.last_aggressor_street()==previous,
        "isDelayedCBetCandidate":game.phase()==Phase::Turn && game.preflop_aggressor()==seat as i32 && !game.flop_had_aggression(),
        "bigBlind":game.minimum_bet(),"wagerUnit":game.wager_unit(),"publicPlayers":public
    })).expect("internally constructed AI context")
}
