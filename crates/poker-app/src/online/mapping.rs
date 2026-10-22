//! Conversions between protocol v2 wire values and the online UI/engine types.
//!
//! All functions are pure so they can be unit-tested without a socket or an
//! egui context (docs/specs/online-ui-plan.md section 7.1).

use poker_core::{Card, Rank, Suit, Variant};
use poker_engine::LegalActions as EngineLegalActions;
use poker_protocol::cards::{CardDto, RevealedCard};
use poker_protocol::common::{
    ActionType as WireActionType, LastHandResult, LegalActions as WireLegalActions, Phase,
    PublicPlayer, RoomConfig,
};
use poker_protocol::{GameState, RequestId, RoomOptionsInput};
use serde_json::Value;

use super::commands::OnlineAction;
use super::text;

/// Stable string key of a request id (used to match ack/error replies to the
/// pending request). Non-string request ids fall back to their JSON text.
pub fn request_id_key(id: &RequestId) -> String {
    id.as_str()
        .map_or_else(|| id.as_value().to_string(), str::to_owned)
}

/// Converts one face-up wire card; invalid tokens return None.
pub fn card_from_dto(card: &CardDto) -> Option<Card> {
    let suit = Suit::from_name(&card.suit)?;
    let rank = Rank::from_str_token(&card.rank)?;
    Some(Card::new(suit, rank))
}

/// Converts one showdown-revealed card (same tokens as CardDto).
pub fn card_from_revealed(card: &RevealedCard) -> Option<Card> {
    let suit = Suit::from_name(&card.suit)?;
    let rank = Rank::from_str_token(&card.rank)?;
    Some(Card::new(suit, rank))
}

/// Converts a list of wire cards, silently skipping invalid tokens (a broken
/// server must never panic the UI).
pub fn cards_from_dtos(cards: &[CardDto]) -> Vec<Card> {
    cards.iter().filter_map(card_from_dto).collect()
}

/// Converts showdown-revealed cards.
pub fn cards_from_revealed(cards: &[RevealedCard]) -> Vec<Card> {
    cards.iter().filter_map(card_from_revealed).collect()
}

/// Variant enum for poker-core helpers.
pub fn variant(is_short_deck: bool) -> Variant {
    if is_short_deck {
        Variant::ShortDeck
    } else {
        Variant::Standard
    }
}

/// Chinese phase label (reference ui.md section 8.3).
pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Preflop => text::PHASE_PREFLOP,
        Phase::Flop => text::PHASE_FLOP,
        Phase::Turn => text::PHASE_TURN,
        Phase::River => text::PHASE_RIVER,
        Phase::Showdown => text::PHASE_SHOWDOWN,
        Phase::Idle => text::PHASE_IDLE,
    }
}

/// Chinese hand-type name for a hand rank index (0..=9); rank tables follow
/// the variant order (short deck swaps flush/full house).
pub fn hand_name(is_short_deck: bool, rank: i64) -> String {
    let names = variant(is_short_deck).hand_type_names();
    usize::try_from(rank)
        .ok()
        .and_then(|index| names.get(index))
        .map_or_else(|| "-".to_owned(), |name| (*name).to_owned())
}

/// Converts wire legal actions into the engine type used by the shared table
/// widgets and RaisePanel. The wager unit falls back to the room config when
/// the server omits it (non-turn snapshots).
pub fn legal_from_wire(legal: &WireLegalActions, config: &RoomConfig) -> EngineLegalActions {
    let mut actions = Vec::with_capacity(legal.actions.len());
    for action in &legal.actions {
        actions.push(match action {
            WireActionType::Fold => poker_engine::ActionType::Fold,
            WireActionType::Check => poker_engine::ActionType::Check,
            WireActionType::Call => poker_engine::ActionType::Call,
            WireActionType::Raise => poker_engine::ActionType::Raise,
            WireActionType::Allin => poker_engine::ActionType::AllIn,
        });
    }
    let fallback_unit = if config.is_short_deck {
        config.ante
    } else {
        config.small_blind
    };
    let wager_unit = legal
        .wager_unit
        .unwrap_or(fallback_unit)
        .max(1)
        .unsigned_abs();
    EngineLegalActions {
        actions,
        to_call: nonzero_u64(legal.to_call),
        can_raise: legal.can_raise,
        min_raise_to: nonzero_u64(legal.min_raise_to),
        max_raise_to: nonzero_u64(legal.max_raise_to),
        wager_unit,
    }
}

fn nonzero_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The wire action plus amount for a player_action command. Wire semantics:
/// raise carries the raise-to total; other actions ignore the amount (the
/// reference client sends toCall for calls and the stack for all-ins).
pub fn wire_action(action: OnlineAction, to_call: u64, stack: u64) -> (WireActionType, i64) {
    match action {
        OnlineAction::Fold => (WireActionType::Fold, 0),
        OnlineAction::Check => (WireActionType::Check, 0),
        OnlineAction::Call => (
            WireActionType::Call,
            i64::try_from(to_call).unwrap_or(i64::MAX),
        ),
        OnlineAction::AllIn => (
            WireActionType::Allin,
            i64::try_from(stack).unwrap_or(i64::MAX),
        ),
        OnlineAction::RaiseTo(amount) => (
            WireActionType::Raise,
            i64::try_from(amount).unwrap_or(i64::MAX),
        ),
    }
}

/// Builds room options from app settings parts (the reference whitelists are
/// enforced server-side by poker-protocol).
#[allow(clippy::too_many_arguments)]
pub fn room_options(
    players: usize,
    starting_stack: u64,
    small_blind: u64,
    big_blind: u64,
    ante: u64,
    is_short_deck: bool,
    ai_delay_ms: u64,
    player_name: &str,
) -> (String, RoomOptionsInput) {
    let options = RoomOptionsInput {
        max_players: Some(Value::from(players)),
        is_short_deck: Some(Value::from(is_short_deck)),
        starting_stack: Some(Value::from(starting_stack)),
        small_blind: Some(Value::from(small_blind)),
        big_blind: Some(Value::from(big_blind)),
        ante: Some(Value::from(ante)),
        min_bet: Some(Value::from(ante.saturating_mul(2))),
        ai_delay: Some(Value::from(ai_delay_ms)),
    };
    (player_name.to_owned(), options)
}

/// Finds the recipient player in a game snapshot.
pub fn my_player<'a>(state: &'a GameState, my_id: Option<&str>) -> Option<&'a PublicPlayer> {
    let my_id = my_id?;
    state.players.iter().find(|player| player.id == my_id)
}

/// Name of the player to act (None for idle/showdown snapshots).
pub fn current_actor_name(state: &GameState) -> Option<String> {
    usize::try_from(state.current_player_idx)
        .ok()
        .and_then(|index| state.players.get(index))
        .map(|player| player.name.clone())
}

/// Chip amount of a walk bet/commit.
pub fn chips(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Seat index as usize, None for negative values.
pub fn seat_index(value: i64) -> Option<usize> {
    usize::try_from(value).ok()
}

/// One settlement row for the online result overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementRow {
    pub player_id: String,
    pub name: String,
    pub is_human: bool,
    pub is_winner: bool,
    pub amount: u64,
    pub cards: Vec<Card>,
    pub hand_name: String,
}

/// The online settlement overlay model built from lastHandResult plus the
/// board of the last snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementView {
    pub hand_id: u64,
    pub pot: u64,
    pub is_fold: bool,
    pub hand_rank_name: String,
    pub board: Vec<Card>,
    pub rows: Vec<SettlementRow>,
}

/// Builds the settlement overlay model; the hand id comes from the caller
/// because lastHandResult does not carry it.
pub fn settlement_view(hand_id: u64, result: &LastHandResult, board: &[CardDto]) -> SettlementView {
    let is_fold = matches!(result.reason, poker_protocol::HandEndReason::Fold);
    let rows = result
        .results
        .iter()
        .map(|row| SettlementRow {
            player_id: row.id.clone(),
            name: row.name.clone(),
            is_human: row.is_human,
            is_winner: result.winners.iter().any(|winner| winner.id == row.id),
            amount: u64::try_from(result.winner_amounts.get(&row.id).copied().unwrap_or(0))
                .unwrap_or(0),
            cards: cards_from_revealed(&row.cards),
            hand_name: hand_name(result.is_short_deck, row.hand_rank),
        })
        .collect();
    SettlementView {
        hand_id,
        pot: u64::try_from(result.pot).unwrap_or(0),
        is_fold,
        hand_rank_name: hand_name(result.is_short_deck, result.hand_rank),
        board: cards_from_dtos(board),
        rows,
    }
}
