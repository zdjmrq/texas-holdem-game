//! Decision context, observation events and river-solver request/response
//! types shared by the AI modules.
//!
//! This file is frozen for task-10 (river/CFR) and task-11 (short-deck): both
//! may read it but must not modify it. Every field mirrors the original JS
//! `normalizedContext` (docs/specs/ai-strategy.md section 5) so the golden
//! corpus can be replayed without translation.

use std::collections::BTreeMap;

use poker_core::{Card, Rank, Suit, Variant};
use poker_engine::ActionType;
use serde::Deserialize;

/// Betting street / phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Street {
    /// Pre-flop.
    Preflop,
    /// Flop.
    Flop,
    /// Turn.
    Turn,
    /// River.
    River,
}

impl Street {
    /// Original JS lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Street::Preflop => "preflop",
            Street::Flop => "flop",
            Street::Turn => "turn",
            Street::River => "river",
        }
    }

    /// Parses an original JS phase/street name.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Street> {
        match value {
            "preflop" => Some(Street::Preflop),
            "flop" => Some(Street::Flop),
            "turn" => Some(Street::Turn),
            "river" => Some(Street::River),
            _ => None,
        }
    }

    /// Derives the street from the number of community cards, like normalizedContext.
    #[must_use]
    pub fn from_community_count(count: usize) -> Street {
        match count {
            0 => Street::Preflop,
            3 => Street::Flop,
            4 => Street::Turn,
            _ => Street::River,
        }
    }
}

/// One public seat snapshot (folded players are included, as in the JS context).
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PublicPlayer {
    /// Seat index.
    pub seat: usize,
    /// Remaining stack.
    pub stack: f64,
    /// Total chips committed to the pot over the hand.
    pub committed: f64,
    /// Chips committed on the current street.
    pub round_bet: f64,
    /// Distance from the button (optional; server short-deck may omit it).
    pub position_from_button: Option<i32>,
    /// Small blind flag.
    pub is_small_blind: bool,
    /// Big blind flag.
    pub is_big_blind: bool,
    /// Folded flag.
    pub folded: bool,
    /// All-in flag.
    pub all_in: bool,
}

/// Legal action name as it appears in the original JS context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LegalAction {
    /// Fold.
    Fold,
    /// Check.
    Check,
    /// Call.
    Call,
    /// Raise/bet.
    #[serde(alias = "bet")]
    Raise,
    /// All-in.
    AllIn,
}

impl LegalAction {
    /// Converts to the engine action type.
    #[must_use]
    pub const fn to_action_type(self) -> ActionType {
        match self {
            LegalAction::Fold => ActionType::Fold,
            LegalAction::Check => ActionType::Check,
            LegalAction::Call => ActionType::Call,
            LegalAction::Raise => ActionType::Raise,
            LegalAction::AllIn => ActionType::AllIn,
        }
    }
}

/// A normalised decision context (pure public information + own hole cards).
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionContext {
    /// Hand identifier.
    pub hand_id: u64,
    /// Decision identifier (feeds the RNG seed).
    pub decision_id: String,
    /// Table-level seed string (`poker-table:standard` / `poker-table:shortdeck`).
    pub seed: String,
    /// Game variant.
    pub variant: Variant,
    /// Current street.
    pub street: Street,
    /// Acting seat.
    pub hero_seat: usize,
    /// Own two hole cards.
    pub hole_cards: Vec<Card>,
    /// Public community cards.
    pub community_cards: Vec<Card>,
    /// Pot size.
    pub pot: f64,
    /// Current street's highest bet.
    pub current_bet: f64,
    /// Amount needed to call (already capped to the hero stack by the adapter).
    pub to_call: f64,
    /// Hero chips committed on the current street.
    pub hero_round_bet: f64,
    /// Hero remaining stack.
    pub hero_stack: f64,
    /// Effective stack versus the deepest live opponent.
    pub effective_stack: f64,
    /// Smallest legal raise-to total.
    pub min_raise_to: f64,
    /// Largest legal raise-to total.
    pub max_raise_to: f64,
    /// Whether a voluntary raise is legal.
    pub can_raise: bool,
    /// Whether check is legal.
    pub can_check: bool,
    /// Legal action types.
    pub legal_actions: Vec<ActionType>,
    /// Live opponent seats.
    pub active_opponent_seats: Vec<usize>,
    /// Number of live opponents (at least 1).
    pub num_opponents: usize,
    /// Number of players dealt into the hand.
    pub player_count: usize,
    /// Total seats at the table (used for the ten-player strategy focus).
    pub table_player_count: usize,
    /// Starting stack of the table.
    pub starting_stack: f64,
    /// Distance from the button.
    pub position_from_button: i32,
    /// Hero is the small blind.
    pub is_small_blind: bool,
    /// Hero is the big blind.
    pub is_big_blind: bool,
    /// Number of raises before this decision (pre-flop).
    pub preflop_raise_count: u32,
    /// Hero was the last aggressor on the previous street.
    pub is_previous_street_aggressor: bool,
    /// Hero is a delayed continuation-bet candidate.
    pub is_delayed_cbet_candidate: bool,
    /// Big blind (short deck: minimum bet).
    pub big_blind: f64,
    /// Raise-grid unit (small blind / ante).
    pub wager_unit: f64,
    /// Wall-clock budget in milliseconds (safety brake only).
    pub time_budget_ms: f64,
    /// Fixed Monte Carlo samples override (0 = use defaults).
    pub mc_samples: f64,
    /// River CFR iteration override (800 preserves reference behaviour).
    pub river_iterations: u32,
    /// Public seat snapshots (all seats, including folded ones).
    pub public_players: Vec<PublicPlayer>,
}

impl DecisionContext {
    /// True when the legal action list contains the action.
    #[must_use]
    pub fn is_legal(&self, action: ActionType) -> bool {
        self.legal_actions.contains(&action)
    }

    /// True for the ten-player 25 BB strategy focus
    /// (standard, 10 seats, 5000 stack, 100/200 blinds, unit 100).
    #[must_use]
    pub fn strategy_focus(&self) -> StrategyFocus {
        if self.variant == Variant::Standard
            && self.table_player_count == 10
            && self.starting_stack == 5000.0
            && self.big_blind == 200.0
            && self.wager_unit == 100.0
        {
            StrategyFocus::TenPlayer25Bb
        } else {
            StrategyFocus::General
        }
    }
}

/// The two original strategy-focus regimes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StrategyFocus {
    /// Standard ten-player table, 5000 chips, 100/200 blinds.
    TenPlayer25Bb,
    /// Every other configuration.
    General,
}

impl StrategyFocus {
    /// Original JS focus name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            StrategyFocus::TenPlayer25Bb => "ten-player-25bb",
            StrategyFocus::General => "general",
        }
    }
}

/// One public action observed by an AI (adapter level).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedAction {
    /// Seat that acted.
    pub seat_id: usize,
    /// Lowercase action name (fold/check/call/raise/bet/allin/blind/ante/...).
    pub action: String,
    /// Street of the action.
    pub street: Street,
    /// Hand identifier.
    pub hand_id: u64,
    /// Action metadata (buildActionMeta output).
    #[serde(default)]
    pub meta: ObservedActionMeta,
}

/// Action metadata consumed by opponent models (port of `buildActionMeta`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObservedActionMeta {
    /// Amount needed to call before the action.
    pub to_call_before: f64,
    /// Chips added by the action.
    pub additional_paid: f64,
    /// Total round bet after the action.
    pub raise_to: f64,
    /// Table current bet before the action.
    pub current_bet_before: f64,
    /// Pot before the action.
    pub pot_before: f64,
    /// Pot after the action.
    pub pot_after: f64,
    /// Minimum bet unit.
    pub minimum_bet: f64,
    /// Whether the action increased the current bet.
    pub aggressive: bool,
    /// Bet size divided by max(1, minimum bet, pot before).
    pub bet_fraction: f64,
    /// Number of pre-flop raises before the action.
    pub preflop_raise_count_before: u32,
    /// Actor's distance from the button.
    pub position_from_button: Option<i32>,
    /// Number of players dealt into the hand.
    pub player_count: Option<usize>,
    /// Actor is the small blind.
    pub is_small_blind: bool,
    /// Actor is the big blind.
    pub is_big_blind: bool,
    /// Community cards at the moment of the action.
    #[serde(default, deserialize_with = "card_vec::deserialize")]
    pub community_cards: Vec<Card>,
    /// Flop c-bet opportunity.
    pub is_cbet_opportunity: bool,
    /// Facing a flop c-bet.
    pub faced_cbet: bool,
    /// Facing a pre-flop 3-bet or later.
    pub faced_3bet: bool,
}

/// Showdown information revealed to the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowdownResult {
    /// This seat won at least one pot.
    pub won: bool,
    /// Revealed hand rank index.
    pub hand_rank: u8,
    /// The revealed hand was a bluff.
    pub was_bluff: bool,
    /// The revealed hand was a trap.
    pub was_trap: bool,
}

/// Request passed to the river CFR solver (built by the standard brain).
#[derive(Clone, Debug, PartialEq)]
pub struct RiverRequest {
    /// Five public cards.
    pub board: [Card; 5],
    /// Standard 52-card deck (solver removes the board itself).
    pub deck: Vec<Card>,
    /// Pot size (including both players' current street bets).
    pub pot: f64,
    /// Both players' current street round bets: [hero, villain].
    pub paid: [f64; 2],
    /// Both players' remaining stacks: [hero, villain].
    pub stacks: [f64; 2],
    /// Current street bet to match.
    pub current_bet: f64,
    /// Minimum raise-to total.
    pub min_raise_to: f64,
    /// Minimum bet (big blind).
    pub min_bet: f64,
    /// Raise-grid unit.
    pub unit: f64,
    /// Villain checked before hero acts on the river.
    pub checked_before: bool,
    /// Hero may raise.
    pub can_raise: bool,
    /// CFR iterations (800 in production).
    pub iterations: u32,
    /// Public-range weights for every available combination in deck order:
    /// (self weight, opponent weight). Length must equal C(available, 2).
    pub weights: Vec<(f64, f64)>,
}

/// One information-set node of the solved river policy.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverPolicyNode {
    /// Acting player (0 = hero, 1 = villain).
    pub player: u8,
    /// Action menu, for example ["check", "bet:300", "raise:900"].
    pub actions: Vec<String>,
    /// Average-strategy probabilities aligned with actions.
    pub probabilities: Vec<f64>,
}

/// Full solution returned by the river CFR solver.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverSolution {
    /// Information-set key ("player:bucket:history") -> policy node.
    pub policy: BTreeMap<String, RiverPolicyNode>,
    /// Information-set key -> action values from the average policy.
    pub root_values: BTreeMap<String, Vec<f64>>,
    /// Hero hand score -> private strength bucket (0..=5).
    pub bucket_by_score: BTreeMap<u64, u8>,
    /// CFR iterations performed.
    pub iterations: u32,
    /// Number of information sets in the compiled tree.
    pub information_sets: u32,
    /// Number of tree nodes in the compiled tree.
    pub tree_nodes: u32,
    /// Scope description copied from the JS solver.
    pub scope: &'static str,
}

/// Trace emitted when the river solver drives the decision.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverTrace {
    /// CFR iterations.
    pub iterations: u32,
    /// Compiled information sets.
    pub information_sets: u32,
    /// Compiled tree nodes.
    pub tree_nodes: u32,
    /// Hero private strength bucket.
    pub bucket: u8,
    /// Villain checked before hero acted.
    pub checked_before: bool,
    /// Policy node used for sampling.
    pub policy: RiverPolicyNode,
    /// Action values of the root node.
    pub action_values: Vec<f64>,
    /// Scope description.
    pub scope: String,
}

/// Final river decision (before the adapter's legalize step).
#[derive(Clone, Debug, PartialEq)]
pub struct RiverDecision {
    /// Selected action type.
    pub action: ActionType,
    /// Selected amount (solver units; caller legalizes).
    pub amount: f64,
    /// Expected value of the selected action.
    pub ev: f64,
    /// Diagnostic trace.
    pub trace: RiverTrace,
}

/// Raw corpus/context JSON shape (deserialised from `tests/golden/ai/*.jsonl`).
///
/// Field names and defaults mirror the original JS `gameState` (camelCase) and
/// the JS `normalizedContext` fallbacks.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawDecisionContext {
    /// Hand id.
    pub hand_id: u64,
    /// Decision id (string or number in the corpus).
    #[serde(deserialize_with = "de_decision_id")]
    pub decision_id: String,
    /// Table seed string.
    pub seed: Option<String>,
    /// Variant name ("standard" / "shortdeck").
    pub variant: Option<String>,
    /// Short-deck flag.
    pub is_short_deck: bool,
    /// Explicit street (optional).
    pub street: Option<Street>,
    /// Explicit phase name (optional).
    pub phase: Option<String>,
    /// Hero seat.
    pub hero_seat: Option<usize>,
    /// Current player index.
    pub current_player_index: Option<usize>,
    /// Own hole cards.
    #[serde(deserialize_with = "card_vec::deserialize")]
    pub hole_cards: Vec<Card>,
    /// Public cards.
    #[serde(deserialize_with = "card_vec::deserialize")]
    pub community_cards: Vec<Card>,
    /// Pot.
    pub pot: f64,
    /// Current bet.
    pub current_bet: f64,
    /// Chips to call.
    pub to_call: Option<f64>,
    /// Hero current street bet (JS name yourBet).
    pub your_bet: Option<f64>,
    /// Hero current street bet (alternate name).
    pub hero_round_bet: Option<f64>,
    /// Hero stack.
    pub stack: Option<f64>,
    /// Hero stack (alternate name).
    pub hero_stack: Option<f64>,
    /// Effective stack.
    pub effective_stack: Option<f64>,
    /// Minimum raise-to.
    pub min_raise_to: Option<f64>,
    /// Maximum raise-to.
    pub max_raise_to: Option<f64>,
    /// Whether raising is legal.
    pub can_raise: Option<bool>,
    /// Whether checking is legal.
    pub can_check: Option<bool>,
    /// Legal actions (array or object form).
    pub legal_actions: Option<RawLegalActions>,
    /// Live opponent seats (numbers or objects).
    pub active_opponent_seats: Vec<SeatRef>,
    /// Number of live opponents.
    pub num_opponents: Option<usize>,
    /// Number of live opponents (alternate name).
    pub num_opponents_active: Option<usize>,
    /// Dealt-in player count.
    pub player_count: Option<usize>,
    /// Total table seats.
    pub table_player_count: Option<usize>,
    /// Starting stack.
    pub starting_stack: Option<f64>,
    /// Distance from the button.
    pub position_from_button: Option<i32>,
    /// Hero is small blind.
    pub is_small_blind: bool,
    /// Hero is big blind.
    pub is_big_blind: bool,
    /// Pre-flop raise count.
    pub preflop_raise_count: Option<u32>,
    /// Hero was the previous street aggressor.
    pub is_previous_street_aggressor: bool,
    /// Hero is a delayed c-bet candidate.
    pub is_delayed_cbet_candidate: bool,
    /// Big blind.
    pub big_blind: Option<f64>,
    /// Raise-grid unit.
    pub wager_unit: Option<f64>,
    /// Time budget in ms.
    pub time_budget_ms: Option<f64>,
    /// Monte Carlo samples override.
    pub mc_samples: Option<f64>,
    /// River CFR iteration override for deeper production search.
    pub river_iterations: Option<u32>,
    /// Public seats.
    pub public_players: Vec<PublicPlayer>,
}

/// Legal actions in either the array form or the object form.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum RawLegalActions {
    /// Array form, for example ["fold","call","raise","allin"].
    List(Vec<LegalAction>),
    /// Object form: {actions, toCall, canRaise, minRaiseTo, maxRaiseTo, wagerUnit}.
    Object(RawLegalActionsObject),
}

/// Object form of the legal action set.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawLegalActionsObject {
    /// Action names.
    pub actions: Vec<LegalAction>,
    /// Chips to call.
    pub to_call: Option<f64>,
    /// Raising allowed.
    pub can_raise: Option<bool>,
    /// Minimum raise-to.
    pub min_raise_to: Option<f64>,
    /// Maximum raise-to.
    pub max_raise_to: Option<f64>,
    /// Raise-grid unit.
    pub wager_unit: Option<f64>,
}

/// A live opponent seat reference (number or object form).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum SeatRef {
    /// Plain seat number.
    Number(usize),
    /// Object with seatId/id/index.
    Object(SeatRefObject),
}

impl SeatRef {
    /// Resolves the seat index.
    #[must_use]
    pub const fn seat(self) -> usize {
        match self {
            SeatRef::Number(seat) => seat,
            SeatRef::Object(object) => match object.seat_id {
                Some(seat) => seat,
                None => match object.id {
                    Some(seat) => seat,
                    None => match object.index {
                        Some(seat) => seat,
                        None => 0,
                    },
                },
            },
        }
    }
}

/// Object form of a seat reference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SeatRefObject {
    /// Seat index (camelCase seatId).
    pub seat_id: Option<usize>,
    /// Numeric id.
    pub id: Option<usize>,
    /// Index.
    pub index: Option<usize>,
}

impl RawDecisionContext {
    /// Normalises a raw context exactly like the JS `normalizedContext`.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn normalize(self) -> DecisionContext {
        let variant = if self.is_short_deck || self.variant.as_deref() == Some("shortdeck") {
            Variant::ShortDeck
        } else {
            Variant::Standard
        };
        let street = self
            .street
            .or_else(|| self.phase.as_deref().and_then(Street::from_name))
            .unwrap_or_else(|| Street::from_community_count(self.community_cards.len()));
        let hero_seat = self.hero_seat.or(self.current_player_index).unwrap_or(0);
        let opponents: Vec<usize> = self
            .active_opponent_seats
            .iter()
            .map(|seat| seat.seat())
            .collect();
        let legal = self
            .legal_actions
            .unwrap_or(RawLegalActions::List(Vec::new()));
        let (legal_names, legal_object) = match legal {
            RawLegalActions::List(names) => (names, RawLegalActionsObject::default()),
            RawLegalActions::Object(object) => (object.actions.clone(), object),
        };
        let mut legal_actions: Vec<ActionType> = legal_names
            .iter()
            .map(|name| name.to_action_type())
            .collect();
        let can_raise = self.can_raise != Some(false) && legal_object.can_raise != Some(false);
        let can_check = self.can_check == Some(true) || legal_actions.contains(&ActionType::Check);
        if legal_actions.is_empty() {
            if self.to_call.unwrap_or(0.0) > 0.0 {
                legal_actions.push(ActionType::Fold);
                legal_actions.push(ActionType::Call);
            } else {
                legal_actions.push(ActionType::Check);
            }
            let max_raise_to = self
                .max_raise_to
                .or(legal_object.max_raise_to)
                .unwrap_or(0.0);
            if can_raise && max_raise_to > self.current_bet {
                legal_actions.push(ActionType::Raise);
            }
            if self.hero_stack.or(self.stack).unwrap_or(0.0) > 0.0 {
                legal_actions.push(ActionType::AllIn);
            }
        }
        let num_opponents = self
            .num_opponents
            .or(self.num_opponents_active)
            .unwrap_or_else(|| opponents.len().max(1))
            .max(1);
        let player_count = self.player_count.unwrap_or(opponents.len() + 1).max(2);
        let table_player_count = self
            .table_player_count
            .or(self.player_count)
            .unwrap_or(opponents.len() + 1)
            .max(2);
        let big_blind = self.big_blind.unwrap_or(80.0).max(1.0);
        let wager_unit = self.wager_unit.or(legal_object.wager_unit).map_or_else(
            || (big_blind / 2.0).floor().max(1.0),
            |unit| {
                if unit.is_finite() && unit > 0.0 {
                    unit.floor().max(1.0)
                } else {
                    (big_blind / 2.0).floor().max(1.0)
                }
            },
        );
        let to_call = self
            .to_call
            .or(legal_object.to_call)
            .unwrap_or(0.0)
            .max(0.0);
        let hero_stack = self.hero_stack.or(self.stack).unwrap_or(0.0).max(0.0);
        let effective_stack = self.effective_stack.unwrap_or(hero_stack).max(0.0);
        let min_raise_to = self
            .min_raise_to
            .or(legal_object.min_raise_to)
            .unwrap_or(0.0)
            .max(0.0);
        let max_raise_to = self
            .max_raise_to
            .or(legal_object.max_raise_to)
            .unwrap_or(hero_stack)
            .max(0.0);
        let num_opponents_active = self.num_opponents_active;
        let time_budget_ms = self.time_budget_ms.unwrap_or_else(|| {
            if num_opponents_active.unwrap_or(opponents.len()) >= 4 || street == Street::River {
                150.0
            } else {
                76.0
            }
        });
        DecisionContext {
            hand_id: self.hand_id,
            decision_id: self.decision_id,
            seed: self.seed.unwrap_or_default(),
            variant,
            street,
            hero_seat,
            hole_cards: self.hole_cards,
            community_cards: self.community_cards,
            pot: self.pot.max(0.0),
            current_bet: self.current_bet.max(0.0),
            to_call,
            hero_round_bet: self
                .hero_round_bet
                .or(self.your_bet)
                .unwrap_or(0.0)
                .max(0.0),
            hero_stack,
            effective_stack,
            min_raise_to,
            max_raise_to,
            can_raise,
            can_check,
            legal_actions,
            active_opponent_seats: opponents,
            num_opponents,
            player_count,
            table_player_count,
            starting_stack: self.starting_stack.unwrap_or(0.0).max(0.0),
            position_from_button: self.position_from_button.unwrap_or(0),
            is_small_blind: self.is_small_blind,
            is_big_blind: self.is_big_blind,
            preflop_raise_count: self.preflop_raise_count.unwrap_or(0),
            is_previous_street_aggressor: self.is_previous_street_aggressor,
            is_delayed_cbet_candidate: self.is_delayed_cbet_candidate,
            big_blind,
            wager_unit,
            time_budget_ms,
            mc_samples: self.mc_samples.unwrap_or(0.0),
            river_iterations: self.river_iterations.unwrap_or(800).clamp(100, 20_000),
            public_players: self.public_players,
        }
    }
}

/// Deserialises a decision id that may be a string or a number.
fn de_decision_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl serde::de::Visitor<'_> for Visitor {
        type Value = String;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a decision id string or number")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            Ok(value.to_owned())
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<String, E> {
            Ok(value.to_string())
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<String, E> {
            Ok(value.to_string())
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<String, E> {
            Ok(value.to_string())
        }
    }
    deserializer.deserialize_any(Visitor)
}

/// Card JSON helpers ({rank:"A", suit:"spades", value:14}).
pub mod card_vec {
    use super::{Card, Rank, Suit};
    use serde::{Deserialize, Deserializer};

    /// One raw card object from the JSON corpus.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CardJson {
        rank: String,
        suit: String,
        #[serde(default)]
        #[allow(dead_code)]
        value: Option<u8>,
    }

    /// Parses a rank token and suit name into a core card.
    #[must_use]
    pub fn card_from_parts(rank: &str, suit: &str) -> Option<Card> {
        let rank = Rank::from_str_token(rank)?;
        let suit = Suit::from_name(suit)?;
        Some(Card::new(suit, rank))
    }

    /// Deserialises a list of cards.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Card>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Vec::<CardJson>::deserialize(deserializer)?;
        raw.into_iter()
            .map(|card| {
                card_from_parts(&card.rank, &card.suit).ok_or_else(|| {
                    serde::de::Error::custom(format!(
                        "invalid card rank {} suit {}",
                        card.rank, card.suit
                    ))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_action_object_form_normalises_like_js() {
        let raw: RawDecisionContext = serde_json::from_str(
            r#"{
                "handId": 3, "decisionId": "standard:flop:1:2", "variant": "standard",
                "communityCards": [{"rank":"Q","suit":"clubs","value":12},
                                   {"rank":"9","suit":"diamonds","value":9},
                                   {"rank":"4","suit":"clubs","value":4}],
                "pot": 640, "currentBet": 240, "toCall": 240, "yourBet": 0,
                "stack": 6000, "effectiveStack": 5200, "numOpponentsActive": 2,
                "activeOpponentSeats": [2,3], "playerCount": 3, "currentPlayerIndex": 1,
                "positionFromButton": 2, "canRaise": true, "canCheck": false,
                "minRaiseTo": 480, "maxRaiseTo": 6000, "bigBlind": 80,
                "tablePlayerCount": 6, "startingStack": 20000,
                "legalActions": {"actions": ["fold","call","raise","allin"], "toCall": 240,
                                 "canRaise": true, "minRaiseTo": 480, "maxRaiseTo": 6000,
                                 "wagerUnit": 40}
            }"#,
        )
        .expect("raw context parses");
        let context = raw.normalize();
        assert_eq!(context.street, Street::Flop);
        assert_eq!(context.to_call, 240.0);
        assert_eq!(context.legal_actions.len(), 4);
        assert!(context.can_raise);
        assert!(!context.can_check);
        assert_eq!(context.wager_unit, 40.0);
        assert_eq!(context.num_opponents, 2);
        assert_eq!(context.table_player_count, 6);
        assert_eq!(context.strategy_focus(), StrategyFocus::General);
    }
}
