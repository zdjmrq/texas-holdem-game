//! Conversions between poker-engine state and protocol v2 DTOs.
//!
//! Every server message is built here so the wire format stays in lockstep with
//! docs/specs/protocol-v2.md and the golden corpus. Hidden information rules
//! live in game_state(): only the recipient's hole cards and showdown-revealed
//! cards are ever emitted.

use std::collections::BTreeMap;

use poker_core::Card;
use poker_engine::{
    HandResult as EngineHandResult, LastAction, LegalActions as EngineLegalActions,
    Phase as EnginePhase, PlayerIdentity,
};
use poker_protocol as proto;

use crate::room::{Room, Seat};

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Standard card DTO (rank token + suit name).
pub(crate) fn card(card: Card) -> proto::CardDto {
    proto::CardDto::new(card.rank().as_str(), card.suit().as_str())
}

/// Showdown-revealed card DTO (also carries the numeric rank value).
pub(crate) fn revealed_card(card: Card) -> proto::RevealedCard {
    proto::RevealedCard::new(
        card.rank().as_str(),
        card.suit().as_str(),
        Some(i64::from(card.rank().value())),
    )
}

/// Engine phase to protocol phase.
pub(crate) fn phase(phase: EnginePhase) -> proto::Phase {
    match phase {
        EnginePhase::Idle => proto::Phase::Idle,
        EnginePhase::Preflop => proto::Phase::Preflop,
        EnginePhase::Flop => proto::Phase::Flop,
        EnginePhase::Turn => proto::Phase::Turn,
        EnginePhase::River => proto::Phase::River,
        EnginePhase::Showdown => proto::Phase::Showdown,
    }
}

/// Engine action type to protocol action type.
pub(crate) fn action_type(action: poker_engine::ActionType) -> proto::ActionType {
    match action {
        poker_engine::ActionType::Fold => proto::ActionType::Fold,
        poker_engine::ActionType::Check => proto::ActionType::Check,
        poker_engine::ActionType::Call => proto::ActionType::Call,
        poker_engine::ActionType::Raise => proto::ActionType::Raise,
        poker_engine::ActionType::AllIn => proto::ActionType::Allin,
    }
}

/// Last-action record for public player entries.
pub(crate) fn last_action(last: Option<LastAction>) -> Option<proto::PlayerActionRecord> {
    last.map(|last| proto::PlayerActionRecord {
        action: last.action.as_str().to_owned(),
        amount: to_i64(last.amount),
    })
}

/// Legal action snapshot; wagerUnit is omitted when it is not the player's turn
/// (the reference server early-returns a wagerUnit-less object in that case).
pub(crate) fn legal_actions(legal: &EngineLegalActions) -> proto::LegalActions {
    let actions: Vec<proto::ActionType> = legal.actions.iter().copied().map(action_type).collect();
    proto::LegalActions {
        wager_unit: if actions.is_empty() {
            None
        } else {
            Some(to_i64(legal.wager_unit))
        },
        actions,
        to_call: to_i64(legal.to_call),
        can_raise: legal.can_raise,
        min_raise_to: to_i64(legal.min_raise_to),
        max_raise_to: to_i64(legal.max_raise_to),
    }
}

/// Public lobby/table player entry.
pub(crate) fn public_player(seat: &Seat) -> proto::PublicPlayer {
    proto::PublicPlayer {
        id: seat.id.clone(),
        seat_id: to_i64(seat.seat_id as u64),
        name: seat.name.clone(),
        avatar: seat.avatar.clone(),
        is_human: seat.is_human,
        connected: seat.connected,
        stack: to_i64(seat.stack),
        chips_in_pot: to_i64(seat.chips_in_pot),
        round_bet: to_i64(seat.round_bet),
        folded: seat.folded,
        is_all_in: seat.is_all_in,
        last_action: last_action(seat.last_action),
    }
}

/// Public player list sorted by seat id.
pub(crate) fn public_players(room: &Room) -> Vec<proto::PublicPlayer> {
    room.seats.iter().map(public_player).collect()
}

/// Public identity used in settlement payloads.
pub(crate) fn public_identity(identity: &PlayerIdentity) -> proto::PublicIdentity {
    proto::PublicIdentity {
        id: identity.id.clone(),
        seat_id: to_i64(identity.seat_id as u64),
        name: identity.name.clone(),
        is_human: identity.is_human,
    }
}

/// Settlement payload of the hand that just ended.
pub(crate) fn last_hand_result(result: &EngineHandResult) -> proto::LastHandResult {
    let mut winner_amounts: BTreeMap<String, i64> = BTreeMap::new();
    for (id, amount) in &result.winner_amounts {
        winner_amounts.insert(id.clone(), to_i64(*amount));
    }
    proto::LastHandResult {
        reason: match result.reason {
            poker_engine::HandEndReason::Showdown => proto::HandEndReason::Showdown,
            poker_engine::HandEndReason::Fold => proto::HandEndReason::Fold,
        },
        is_short_deck: result.is_short_deck,
        pot: to_i64(result.pot),
        hand_rank: i64::from(result.hand_rank),
        winners: result.winners.iter().map(public_identity).collect(),
        winner_amounts,
        results: result
            .results
            .iter()
            .map(|entry| proto::HandResultEntry {
                id: entry.identity.id.clone(),
                seat_id: to_i64(entry.identity.seat_id as u64),
                name: entry.identity.name.clone(),
                is_human: entry.identity.is_human,
                cards: entry.cards.iter().copied().map(revealed_card).collect(),
                hand_rank: i64::from(entry.hand_rank),
            })
            .collect(),
    }
}

/// Per-recipient game snapshot.
pub(crate) fn game_state(room: &Room, seat_index: usize) -> Option<proto::GameState> {
    let game = room.game.as_ref()?;
    let seat = room.seats.get(seat_index)?;
    let player_count = game.players().len();
    let your_cards = if seat.is_human || true {
        seat.hole_cards.iter().copied().map(card).collect()
    } else {
        Vec::new()
    };
    let current_player_idx = game.current_player_idx();
    let is_your_turn = game.phase() != EnginePhase::Idle
        && i32::try_from(seat_index).ok() == Some(current_player_idx);
    let legal = if is_your_turn {
        legal_actions(&game.legal_actions(seat_index))
    } else {
        legal_actions(&EngineLegalActions {
            actions: Vec::new(),
            to_call: 0,
            can_raise: false,
            min_raise_to: 0,
            max_raise_to: 0,
            wager_unit: 0,
        })
    };
    let _ = player_count;
    Some(proto::GameState {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        state_version: room.state_version,
        hand_id: room.hand_id(),
        turn_id: room.turn_id(),
        host_id: room.host_id.clone(),
        is_host: room.host_id.as_deref() == Some(seat.id.as_str()),
        room_config: room.config.clone(),
        is_short_deck: room.config.is_short_deck,
        phase: phase(game.phase()),
        pot: to_i64(game.pot()),
        community_cards: game.community_cards().iter().copied().map(card).collect(),
        players: public_players(room),
        dealer_pos: i64::from(game.dealer_pos()),
        sb_idx: i64::from(game.sb_idx()),
        bb_idx: i64::from(game.bb_idx()),
        current_player_idx: i64::from(current_player_idx),
        current_bet: to_i64(game.current_bet()),
        last_raise: to_i64(game.last_raise()),
        is_your_turn,
        your_cards,
        your_stack: to_i64(seat.stack),
        your_bet: to_i64(seat.round_bet),
        legal_actions: legal,
        last_hand_result: game.last_hand_result().map(last_hand_result),
    })
}

/// room_created / room_joined payload.
pub(crate) fn room_info(room: &Room, seat_index: usize, resumed: bool) -> Option<proto::RoomInfo> {
    let seat = room.seats.get(seat_index)?;
    Some(proto::RoomInfo {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        player_id: seat.id.clone(),
        seat_id: to_i64(seat.seat_id as u64),
        is_host: room.host_id.as_deref() == Some(seat.id.as_str()),
        host_id: room.host_id.clone(),
        resume_token: seat.resume_token.clone(),
        resumed,
        room_config: room.config.clone(),
        players: public_players(room),
    })
}

/// player_joined / player_left payload.
pub(crate) fn lobby_update(room: &Room) -> proto::LobbyUpdate {
    proto::LobbyUpdate {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        host_id: room.host_id.clone(),
        room_config: room.config.clone(),
        players: public_players(room),
    }
}

/// host_changed payload.
pub(crate) fn host_changed(room: &Room, seat_index: usize) -> Option<proto::HostChanged> {
    let seat = room.seats.get(seat_index)?;
    Some(proto::HostChanged {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        host_id: room.host_id.clone(),
        is_host: room.host_id.as_deref() == Some(seat.id.as_str()),
        state_version: room.state_version,
        players: public_players(room),
    })
}

/// game_starting payload.
pub(crate) fn game_starting(room: &Room) -> proto::GameStarting {
    let human_count = room.seats.iter().filter(|seat| seat.is_human).count();
    let ai_count = room.seats.iter().filter(|seat| !seat.is_human).count();
    proto::GameStarting {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        player_count: to_i64(room.seats.len() as u64),
        human_count: to_i64(human_count as u64),
        ai_count: to_i64(ai_count as u64),
        room_config: room.config.clone(),
    }
}

/// ready_status payload.
pub(crate) fn ready_status(room: &Room, all_ready: bool) -> proto::ReadyStatus {
    let humans: Vec<usize> = room
        .seats
        .iter()
        .enumerate()
        .filter(|(_, seat)| seat.is_human && seat.connected && !seat.left && seat.stack > 0)
        .map(|(index, _)| index)
        .collect();
    let players = humans
        .iter()
        .map(|index| proto::ReadyEntry {
            id: room.seats[*index].id.clone(),
            ready: room.ready.contains(&room.seats[*index].id),
        })
        .collect();
    let waiting_for = humans
        .iter()
        .filter(|index| !room.ready.contains(&room.seats[**index].id))
        .count();
    proto::ReadyStatus {
        protocol_version: proto::PROTOCOL_VERSION,
        room_code: room.code.clone(),
        hand_id: room.hand_id(),
        all_ready,
        waiting_for: to_i64(waiting_for as u64),
        players,
    }
}

/// Generic ack payload.
pub(crate) fn ack(
    request_id: &proto::RequestId,
    accepted: bool,
    room: Option<&Room>,
) -> proto::Ack {
    proto::Ack {
        protocol_version: proto::PROTOCOL_VERSION,
        request_id: request_id.clone(),
        accepted,
        room_code: room.map(|room| room.code.clone()),
        state_version: room.map_or(0, |room| room.state_version),
        hand_id: room.map_or(0, Room::hand_id),
    }
}

/// action_ack payload.
pub(crate) fn action_ack(
    request_id: &proto::RequestId,
    accepted: bool,
    message: String,
    room: &Room,
) -> proto::ActionAck {
    proto::ActionAck {
        protocol_version: proto::PROTOCOL_VERSION,
        request_id: request_id.clone(),
        accepted,
        message,
        room_code: room.code.clone(),
        hand_id: room.hand_id(),
        turn_id: room.turn_id(),
        state_version: room.state_version,
    }
}
