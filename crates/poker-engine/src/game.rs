//! The betting state machine.
//!
//! This is a direct port of the original `server/server-game.js`
//! `ServerPokerGame` (a strict, headless version of the shared `js/engine.js`
//! kernel). Indices use the same `-1` sentinel convention as the JS code so the
//! control flow stays mechanically reviewable against the oracle.

use poker_core::{Card, Deck, EvaluatedHand, RandomSource, evaluate};

use crate::action::{ActError, Action, ActionType, LegalActions};
use crate::config::GameConfig;
use crate::phase::Phase;
use crate::player::{ActionLabel, LastAction, Player};
use crate::result::{HandEndReason, HandPlayerResult, HandResult, PlayerIdentity};
use crate::rules::{self, ActionMeta, ActionMetaInput, SidePot};

/// A complete poker table state machine.
#[derive(Clone, Debug)]
pub struct Game {
    config: GameConfig,
    players: Vec<Player>,
    phase: Phase,
    community_cards: Vec<Card>,
    deck: Vec<Card>,
    pot: u64,
    current_bet: u64,
    last_raise: u64,
    has_full_bet_this_round: bool,
    round_bets: Vec<u64>,
    players_acted: Vec<bool>,
    acted_at_bet: Vec<u64>,
    raise_size_at_action: Vec<u64>,
    dealer_pos: i32,
    sb_idx: i32,
    bb_idx: i32,
    current_player_idx: i32,
    preflop_raise_count: u32,
    preflop_aggressor: i32,
    last_aggressor: i32,
    last_aggressor_street: Option<Phase>,
    flop_had_aggression: bool,
    hand_id: u64,
    turn_id: u64,
    actions_this_round: u32,
    last_hand_result: Option<HandResult>,
    players_dealt_last_hand: Vec<usize>,
    last_action_meta: Option<ActionMeta>,
}

impl Game {
    /// Creates a table with the given configuration and seats.
    #[must_use]
    pub fn new(config: GameConfig, players: Vec<Player>) -> Self {
        let seat_count = players.len();
        let minimum_bet = config.minimum_bet();
        Self {
            config,
            players,
            phase: Phase::Idle,
            community_cards: Vec::new(),
            deck: Vec::new(),
            pot: 0,
            current_bet: 0,
            last_raise: minimum_bet,
            has_full_bet_this_round: false,
            round_bets: vec![0; seat_count],
            players_acted: vec![false; seat_count],
            acted_at_bet: vec![0; seat_count],
            raise_size_at_action: vec![0; seat_count],
            dealer_pos: -1,
            sb_idx: -1,
            bb_idx: -1,
            current_player_idx: -1,
            preflop_raise_count: 0,
            preflop_aggressor: -1,
            last_aggressor: -1,
            last_aggressor_street: None,
            flop_had_aggression: false,
            hand_id: 0,
            turn_id: 0,
            actions_this_round: 0,
            last_hand_result: None,
            players_dealt_last_hand: Vec::new(),
            last_action_meta: None,
        }
    }

    /// Replaces the seat list (room layer adds/removes players between hands).
    pub fn set_players(&mut self, players: Vec<Player>) {
        self.players = players;
        let seat_count = self.players.len();
        self.round_bets.clear();
        self.round_bets.resize(seat_count, 0);
        self.players_acted.clear();
        self.players_acted.resize(seat_count, false);
        self.acted_at_bet.clear();
        self.acted_at_bet.resize(seat_count, 0);
        self.raise_size_at_action.clear();
        self.raise_size_at_action.resize(seat_count, 0);
    }

    /// Table configuration.
    #[must_use]
    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    /// Seats.
    #[must_use]
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// Mutable seats (room layer only; engine state should stay consistent).
    pub fn players_mut(&mut self) -> &mut [Player] {
        &mut self.players
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Total pot.
    #[must_use]
    pub fn pot(&self) -> u64 {
        self.pot
    }

    /// Current bet to match this round.
    #[must_use]
    pub fn current_bet(&self) -> u64 {
        self.current_bet
    }

    /// Size of the last full raise this round.
    #[must_use]
    pub fn last_raise(&self) -> u64 {
        self.last_raise
    }

    /// Community cards dealt so far.
    #[must_use]
    pub fn community_cards(&self) -> &[Card] {
        &self.community_cards
    }

    /// Cards remaining in the deck.
    #[must_use]
    pub fn deck_len(&self) -> usize {
        self.deck.len()
    }

    /// Dealer button seat (-1 before the first hand).
    #[must_use]
    pub const fn dealer_pos(&self) -> i32 {
        self.dealer_pos
    }

    /// Small blind seat (-1 when there is no small blind).
    #[must_use]
    pub const fn sb_idx(&self) -> i32 {
        self.sb_idx
    }

    /// Big blind seat (-1 when there is no big blind).
    #[must_use]
    pub const fn bb_idx(&self) -> i32 {
        self.bb_idx
    }

    /// Seat to act, or -1.
    #[must_use]
    pub const fn current_player_idx(&self) -> i32 {
        self.current_player_idx
    }

    /// Chips committed by each seat this betting round.
    #[must_use]
    pub fn round_bets(&self) -> &[u64] {
        &self.round_bets
    }

    /// True after a full bet/raise this round.
    #[must_use]
    pub const fn has_full_bet_this_round(&self) -> bool {
        self.has_full_bet_this_round
    }

    /// Monotonic hand counter (incremented only for hands that start).
    #[must_use]
    pub const fn hand_id(&self) -> u64 {
        self.hand_id
    }

    /// Turn counter used by the protocol layer to reject stale actions.
    #[must_use]
    pub const fn turn_id(&self) -> u64 {
        self.turn_id
    }

    /// Actions taken in the current betting round.
    #[must_use]
    pub const fn actions_this_round(&self) -> u32 {
        self.actions_this_round
    }

    /// Pre-flop raise count.
    #[must_use]
    pub const fn preflop_raise_count(&self) -> u32 {
        self.preflop_raise_count
    }

    /// Seat of the pre-flop aggressor, or -1.
    #[must_use]
    pub const fn preflop_aggressor(&self) -> i32 {
        self.preflop_aggressor
    }

    /// Seat of the last aggressor, or -1.
    #[must_use]
    pub const fn last_aggressor(&self) -> i32 {
        self.last_aggressor
    }

    /// Street of the last aggressor.
    #[must_use]
    pub const fn last_aggressor_street(&self) -> Option<Phase> {
        self.last_aggressor_street
    }

    /// True when the flop saw aggression.
    #[must_use]
    pub const fn flop_had_aggression(&self) -> bool {
        self.flop_had_aggression
    }

    /// Result of the most recently settled hand.
    #[must_use]
    pub fn last_hand_result(&self) -> Option<&HandResult> {
        self.last_hand_result.as_ref()
    }

    /// Metadata of the most recent action (for opponent modeling).
    #[must_use]
    pub const fn last_action_meta(&self) -> Option<ActionMeta> {
        self.last_action_meta
    }

    /// Seats dealt into the previous hand.
    #[must_use]
    pub fn players_dealt_last_hand(&self) -> &[usize] {
        &self.players_dealt_last_hand
    }

    /// True for the short-deck variant.
    #[must_use]
    pub const fn is_short_deck(&self) -> bool {
        self.config.is_short_deck()
    }

    /// Minimum bet unit for this variant.
    #[must_use]
    pub const fn minimum_bet(&self) -> u64 {
        self.config.minimum_bet()
    }

    /// Raise-grid unit for this variant.
    #[must_use]
    pub const fn wager_unit(&self) -> u64 {
        self.config.wager_unit()
    }

    /// Seats that can be dealt into the next hand.
    fn eligible_seats(&self) -> Vec<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, player)| player.active_for_new_hand())
            .map(|(index, _)| index)
            .collect()
    }

    /// Seats still in the hand (not folded, dealt in).
    #[must_use]
    pub fn in_hand(&self) -> Vec<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, player)| !player.folded && player.dealt_in)
            .map(|(index, _)| index)
            .collect()
    }

    /// In-hand seats that can still act (not all-in, chips left).
    #[must_use]
    pub fn live_to_act(&self) -> Vec<usize> {
        self.in_hand()
            .into_iter()
            .filter(|seat| !self.players[*seat].is_all_in && self.players[*seat].stack > 0)
            .collect()
    }

    fn next_index(&self, start: i32, predicate: impl Fn(&Player, usize) -> bool) -> i32 {
        let n = self.players.len() as i32;
        if n <= 1 {
            return -1;
        }
        for step in 1..n {
            let index = (start + step + n) % n;
            let seat = index as usize;
            if predicate(&self.players[seat], seat) {
                return index;
            }
        }
        -1
    }

    fn next_dealt(&self, start: i32) -> i32 {
        self.next_index(start, |player, _| player.dealt_in && !player.folded)
    }

    fn next_to_act(&self, start: i32) -> i32 {
        self.next_index(start, |player, seat| {
            player.dealt_in
                && !player.folded
                && !player.is_all_in
                && player.stack > 0
                && (self.round_bets[seat] < self.current_bet || !self.players_acted[seat])
        })
    }

    /// Position relative to the button among seats dealt into the hand.
    #[must_use]
    pub fn hand_position_info(&self, seat_id: usize) -> (u32, u32) {
        let n = self.players.len();
        let mut seats: Vec<usize> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, player)| player.dealt_in)
            .map(|(index, _)| index)
            .collect();
        let dealer = self.dealer_pos;
        seats.sort_by_key(|seat| {
            let seat_i = *seat as i32;
            ((seat_i - dealer + n as i32) % n as i32) as u32
        });
        let position = seats.iter().position(|candidate| *candidate == seat_id);
        #[allow(clippy::cast_possible_truncation)]
        let position = position.map_or(0, |index| index as u32);
        #[allow(clippy::cast_possible_truncation)]
        let player_count = (seats.len().max(2)) as u32;
        (position, player_count)
    }

    fn reset_round_state(&mut self) {
        self.round_bets.fill(0);
        self.players_acted.fill(false);
        self.acted_at_bet.fill(0);
        self.raise_size_at_action.fill(0);
        self.current_bet = 0;
        self.last_raise = self.minimum_bet();
        self.has_full_bet_this_round = false;
        self.actions_this_round = 0;
        for player in &mut self.players {
            player.round_bet = 0;
        }
    }

    /// Starts a hand with a freshly shuffled deck from the supplied RNG.
    pub fn start_hand<R: RandomSource + ?Sized>(&mut self, rng: &mut R) -> bool {
        let eligible = self.eligible_seats();
        let dealer_override = if self.dealer_pos < 0 && eligible.len() >= 2 {
            let raw = (rng.next_f64() * eligible.len() as f64) as usize;
            Some(eligible[raw.min(eligible.len() - 1)])
        } else {
            None
        };
        let mut deck = Deck::new(self.config.variant);
        deck.shuffle(rng);
        self.start_hand_with_deck(deck, dealer_override)
    }

    /// Starts a hand from an explicit deck order (golden replay / deterministic tests).
    pub fn start_hand_with_deck(&mut self, deck: Deck, dealer_override: Option<usize>) -> bool {
        let previous_bb = self.bb_idx;
        let previous_dealt_count = self.players_dealt_last_hand.len();
        let eligible = self.eligible_seats();

        self.community_cards.clear();
        self.pot = 0;
        self.current_bet = 0;
        self.round_bets.fill(0);
        self.players_acted.fill(false);
        self.acted_at_bet.fill(0);
        self.raise_size_at_action.fill(0);

        for (index, player) in self.players.iter_mut().enumerate() {
            let dealt = eligible.contains(&index);
            player.hole_cards.clear();
            player.chips_in_pot = 0;
            player.round_bet = 0;
            player.folded = !dealt;
            player.dealt_in = dealt;
            player.is_all_in = false;
            player.last_action = None;
        }
        if eligible.len() < 2 {
            self.phase = Phase::Idle;
            return false;
        }

        self.last_hand_result = None;
        self.hand_id += 1;
        self.turn_id = 0;
        self.actions_this_round = 0;
        self.phase = Phase::Preflop;
        self.preflop_raise_count = 0;
        self.preflop_aggressor = -1;
        self.last_aggressor = -1;
        self.last_aggressor_street = None;
        self.flop_had_aggression = false;
        self.last_raise = self.minimum_bet();
        self.has_full_bet_this_round = false;
        self.sb_idx = -1;
        self.bb_idx = -1;

        let n = self.players.len() as i32;
        let entering_heads_up = !self.is_short_deck()
            && eligible.len() == 2
            && previous_dealt_count > 2
            && previous_bb >= 0
            && eligible.contains(&(previous_bb as usize));
        if let Some(seat) = dealer_override {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            {
                self.dealer_pos = seat as i32;
            }
        } else if entering_heads_up {
            self.dealer_pos = previous_bb;
        } else if self.dealer_pos < 0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            {
                self.dealer_pos = eligible[0] as i32;
            }
        } else {
            let next_dealer = self.next_index(self.dealer_pos, |_, seat| eligible.contains(&seat));
            if next_dealer >= 0 {
                self.dealer_pos = next_dealer;
            }
        }
        let _ = n;
        self.players_dealt_last_hand = eligible.clone();

        self.deck = deck.cards().to_vec();

        // Two rounds of hole cards, clockwise from the dealer.
        let mut deal_pos = self.dealer_pos;
        for _round in 0..2 {
            for _ in 0..eligible.len() {
                deal_pos = self.next_index(deal_pos, |_, seat| eligible.contains(&seat));
                if deal_pos < 0 {
                    break;
                }
                let card = self
                    .deck
                    .pop()
                    .expect("deck exhausted while dealing hole cards");
                self.players[deal_pos as usize].hole_cards.push(card);
            }
        }

        if self.is_short_deck() {
            self.post_antes();
        } else {
            self.post_blinds();
        }

        let action_anchor = if self.is_short_deck() {
            self.dealer_pos
        } else {
            self.bb_idx
        };
        self.current_player_idx = self.next_to_act(action_anchor);
        self.auto_close_no_decision();
        self.bump_turn();
        if self.is_round_complete() {
            self.advance_after_action();
        }
        true
    }

    fn commit(&mut self, seat: usize, amount: u64, label: ActionLabel) -> u64 {
        let paid = amount.min(self.players[seat].stack);
        self.players[seat].stack -= paid;
        self.players[seat].chips_in_pot += paid;
        self.pot += paid;
        if self.players[seat].stack == 0 {
            self.players[seat].is_all_in = true;
        }
        self.players[seat].last_action = Some(LastAction {
            action: label,
            amount: paid,
        });
        paid
    }

    fn post_blinds(&mut self) {
        let dealt = self.in_hand();
        if dealt.len() == 2 {
            self.sb_idx = self.dealer_pos;
            self.bb_idx = self.next_dealt(self.dealer_pos);
        } else {
            self.sb_idx = self.next_dealt(self.dealer_pos);
            self.bb_idx = self.next_dealt(self.sb_idx);
        }
        if self.sb_idx < 0 || self.bb_idx < 0 {
            return;
        }
        let small_blind_seat = self.sb_idx as usize;
        let big_blind_seat = self.bb_idx as usize;
        let small_blind_paid = self.commit(
            small_blind_seat,
            self.config.small_blind,
            ActionLabel::SmallBlind,
        );
        self.round_bets[small_blind_seat] = small_blind_paid;
        self.players[small_blind_seat].round_bet = self.round_bets[small_blind_seat];
        let big_blind_paid =
            self.commit(big_blind_seat, self.config.big_blind, ActionLabel::BigBlind);
        self.round_bets[big_blind_seat] = big_blind_paid;
        self.players[big_blind_seat].round_bet = self.round_bets[big_blind_seat];
        let can_bet = self.live_to_act().len();
        self.current_bet = if can_bet >= 2 {
            self.config.big_blind
        } else {
            self.round_bets[small_blind_seat].max(self.round_bets[big_blind_seat])
        };
        self.last_raise = self.config.big_blind;
        self.has_full_bet_this_round =
            can_bet >= 2 || self.round_bets[big_blind_seat] >= self.config.big_blind;
    }

    fn post_antes(&mut self) {
        for seat in 0..self.players.len() {
            if self.players[seat].dealt_in {
                self.commit(seat, self.config.ante, ActionLabel::Ante);
            }
        }
        let dealer = self.dealer_pos;
        if dealer >= 0 {
            let dealer_seat = dealer as usize;
            if self.players[dealer_seat].dealt_in && self.players[dealer_seat].stack > 0 {
                self.commit(dealer_seat, self.config.ante, ActionLabel::DealerAnte);
            }
        }
        // Antes are dead money and never enter round bets.
        self.current_bet = 0;
        self.last_raise = self.config.min_bet;
    }

    /// Whether this seat may still make a voluntary raise this round.
    #[must_use]
    pub fn can_raise(&self, seat: usize) -> bool {
        let Some(player) = self.players.get(seat) else {
            return false;
        };
        if player.folded || player.is_all_in || player.stack == 0 {
            return false;
        }
        if !self.players_acted[seat] {
            return true;
        }
        let faced = self.acted_at_bet[seat];
        let required = self.raise_size_at_action[seat].max(self.minimum_bet());
        self.current_bet.saturating_sub(faced) >= required
    }

    /// Smallest legal raise total.
    #[must_use]
    pub fn min_raise_to(&self) -> u64 {
        if self.current_bet == 0 {
            return self.minimum_bet();
        }
        if !self.has_full_bet_this_round && self.current_bet < self.minimum_bet() {
            return self.minimum_bet();
        }
        self.current_bet + self.last_raise.max(self.minimum_bet())
    }

    fn record_action(&mut self, seat: usize) {
        self.players_acted[seat] = true;
        self.acted_at_bet[seat] = self.current_bet;
        self.raise_size_at_action[seat] = self.last_raise.max(self.minimum_bet());
    }

    fn clear_players_acted(&mut self) {
        self.players_acted.fill(false);
    }

    /// Legal actions for a seat (empty unless it is that seat's turn).
    #[must_use]
    pub fn legal_actions(&self, seat: usize) -> LegalActions {
        let empty = LegalActions {
            actions: Vec::new(),
            to_call: 0,
            can_raise: false,
            min_raise_to: 0,
            max_raise_to: 0,
            wager_unit: self.wager_unit(),
        };
        let index = i32::try_from(seat).unwrap_or(-1);
        if index != self.current_player_idx || self.phase == Phase::Idle {
            return empty;
        }
        let Some(player) = self.players.get(seat) else {
            return empty;
        };
        if player.folded || player.is_all_in {
            return empty;
        }
        let own_bet = self.round_bets[seat];
        let to_call = self.current_bet.saturating_sub(own_bet);
        let can_raise = self.can_raise(seat);
        let max_raise_to = own_bet + player.stack;
        let bounds = rules::raise_bounds(self.min_raise_to(), max_raise_to, self.wager_unit());
        let mut actions = Vec::with_capacity(4);
        if to_call == 0 {
            actions.push(ActionType::Check);
        } else {
            actions.push(ActionType::Fold);
            actions.push(ActionType::Call);
        }
        if player.stack > 0 && can_raise && bounds.min <= bounds.max && bounds.min < max_raise_to {
            actions.push(ActionType::Raise);
        }
        if player.stack > 0 && (max_raise_to <= self.current_bet || can_raise) {
            actions.push(ActionType::AllIn);
        }
        LegalActions {
            actions,
            to_call: to_call.min(player.stack),
            can_raise,
            min_raise_to: bounds.min,
            max_raise_to,
            wager_unit: bounds.unit,
        }
    }

    /// Applies an action for a seat and advances the game.
    ///
    /// # Errors
    ///
    /// Returns [`ActError`] when the action is illegal (out of turn, not in the
    /// legal set, raise off the grid, raise out of range).
    pub fn act(&mut self, seat: usize, action: Action) -> Result<ActionMeta, ActError> {
        let legal = self.legal_actions(seat);
        let action_type = action.action_type();
        if !legal.contains(action_type) {
            return Err(ActError::IllegalAction(action_type));
        }
        let own_bet = self.round_bets[seat];
        let to_call = self.current_bet.saturating_sub(own_bet);
        let pot_before = self.pot;
        let current_bet_before = self.current_bet;
        let preflop_raise_count_before = self.preflop_raise_count;

        match action {
            Action::Fold => {
                self.players[seat].folded = true;
                self.players[seat].last_action = Some(LastAction {
                    action: ActionLabel::Fold,
                    amount: 0,
                });
            }
            Action::Check => {
                self.players[seat].last_action = Some(LastAction {
                    action: ActionLabel::Check,
                    amount: 0,
                });
                self.record_action(seat);
            }
            Action::Call => {
                let paid = self.commit(
                    seat,
                    to_call.min(self.players[seat].stack),
                    ActionLabel::Call,
                );
                self.round_bets[seat] = own_bet + paid;
                self.players[seat].round_bet = self.round_bets[seat];
                self.record_action(seat);
            }
            Action::Raise { to } => {
                let wager_unit = legal.wager_unit;
                if wager_unit == 0 || to % wager_unit != 0 {
                    return Err(ActError::RaiseNotIntegerMultiple {
                        amount: to,
                        wager_unit,
                    });
                }
                if to < legal.min_raise_to || to > legal.max_raise_to {
                    return Err(ActError::RaiseOutOfRange {
                        amount: to,
                        minimum: legal.min_raise_to,
                        maximum: legal.max_raise_to,
                    });
                }
                let previous = self.current_bet;
                let paid = self.commit(seat, to - own_bet, ActionLabel::Raise);
                self.round_bets[seat] = own_bet + paid;
                self.players[seat].round_bet = self.round_bets[seat];
                self.current_bet = self.round_bets[seat];
                self.last_raise = self.current_bet - previous;
                self.has_full_bet_this_round = true;
                self.clear_players_acted();
                self.record_action(seat);
                if self.phase == Phase::Preflop {
                    self.preflop_raise_count += 1;
                }
            }
            Action::AllIn => {
                let all_in_amount = self.players[seat].stack;
                let target = own_bet + all_in_amount;
                let previous = self.current_bet;
                let paid = self.commit(seat, all_in_amount, ActionLabel::AllIn);
                self.round_bets[seat] = own_bet + paid;
                self.players[seat].round_bet = self.round_bets[seat];
                if target > previous {
                    let full_raise = target >= self.min_raise_to();
                    self.current_bet = target;
                    if full_raise {
                        self.last_raise = target - previous;
                        self.has_full_bet_this_round = true;
                        self.clear_players_acted();
                        if self.phase == Phase::Preflop {
                            self.preflop_raise_count += 1;
                        }
                    } else if !self.has_full_bet_this_round && target >= self.minimum_bet() {
                        self.has_full_bet_this_round = true;
                        self.last_raise = self.minimum_bet();
                    }
                }
                self.record_action(seat);
            }
        }

        let aggressive = self.current_bet > current_bet_before;
        if aggressive {
            self.last_aggressor = seat as i32;
            self.last_aggressor_street = Some(self.phase);
            if self.phase == Phase::Preflop {
                self.preflop_aggressor = seat as i32;
            }
            if self.phase == Phase::Flop {
                self.flop_had_aggression = true;
            }
        }
        let additional_paid = self.players[seat].last_action.map_or(0, |last| last.amount);
        let raise_to = self.round_bets[seat];
        let meta = rules::build_action_meta(ActionMetaInput {
            call_cost: to_call,
            additional_paid,
            raise_to,
            current_bet_before,
            pot_before,
            pot_after: self.pot,
            minimum_bet: self.minimum_bet(),
            aggressive,
            faced_3bet: self.phase == Phase::Preflop
                && to_call > 0
                && preflop_raise_count_before >= 2,
            preflop_raise_count_before,
            ..ActionMetaInput::default()
        });
        self.actions_this_round += 1;
        self.last_action_meta = Some(meta);
        self.advance_after_action();
        Ok(meta)
    }

    fn auto_close_no_decision(&mut self) -> bool {
        let live = self.live_to_act();
        if live.is_empty() {
            return true;
        }
        if live.len() != 1 {
            return false;
        }
        let seat = live[0];
        if self.current_bet.saturating_sub(self.round_bets[seat]) > 0 {
            self.current_player_idx = seat as i32;
            return false;
        }
        self.record_action(seat);
        true
    }

    fn is_round_complete(&self) -> bool {
        let in_hand = self.in_hand();
        if in_hand.len() <= 1 {
            return true;
        }
        for seat in in_hand {
            if self.players[seat].is_all_in {
                continue;
            }
            if self.round_bets[seat] < self.current_bet || !self.players_acted[seat] {
                return false;
            }
        }
        true
    }

    fn refund_uncalled(&mut self) -> u64 {
        let Some(uncalled) = rules::find_uncalled_refund(&self.round_bets) else {
            return 0;
        };
        let Some(player) = self.players.get_mut(uncalled.index) else {
            return 0;
        };
        let refund = uncalled.refund;
        player.stack += refund;
        player.chips_in_pot -= refund;
        self.pot -= refund;
        self.round_bets[uncalled.index] -= refund;
        player.round_bet = self.round_bets[uncalled.index];
        if player.stack > 0 {
            player.is_all_in = false;
        }
        let live_bets: Vec<u64> = self
            .in_hand()
            .into_iter()
            .map(|seat| self.round_bets[seat])
            .collect();
        self.current_bet = live_bets.into_iter().max().unwrap_or(0);
        refund
    }

    fn bump_turn(&mut self) {
        self.turn_id += 1;
    }

    /// Advances after an action: finish the hand, close the street or move the
    /// action to the next seat.
    pub fn advance_after_action(&mut self) {
        let in_hand = self.in_hand();
        if in_hand.len() <= 1 {
            self.finish_by_fold(in_hand.first().copied());
            return;
        }
        if self.is_round_complete() {
            self.refund_uncalled();
            if self.live_to_act().len() <= 1 {
                while self.phase != Phase::Showdown {
                    self.advance_street();
                }
                self.showdown();
                return;
            }
            self.advance_street();
            if self.phase == Phase::Showdown {
                self.showdown();
            } else {
                self.auto_close_no_decision();
                if self.is_round_complete() {
                    self.advance_after_action();
                } else {
                    self.bump_turn();
                }
            }
            return;
        }
        let next = self.next_to_act(self.current_player_idx);
        if next >= 0 {
            self.current_player_idx = next;
        }
        self.bump_turn();
    }

    /// Deals the next street and resets per-round state.
    pub fn advance_street(&mut self) {
        self.reset_round_state();
        for player in &mut self.players {
            player.round_bet = 0;
        }
        match self.phase {
            Phase::Preflop => {
                self.deck.pop();
                let first = self.deck.pop().expect("deck");
                let second = self.deck.pop().expect("deck");
                let third = self.deck.pop().expect("deck");
                self.community_cards.extend([first, second, third]);
                self.phase = Phase::Flop;
            }
            Phase::Flop => {
                self.deck.pop();
                let card = self.deck.pop().expect("deck");
                self.community_cards.push(card);
                self.phase = Phase::Turn;
            }
            Phase::Turn => {
                self.deck.pop();
                let card = self.deck.pop().expect("deck");
                self.community_cards.push(card);
                self.phase = Phase::River;
            }
            Phase::River => {
                self.phase = Phase::Showdown;
                self.current_player_idx = -1;
                return;
            }
            Phase::Idle | Phase::Showdown => return,
        }
        self.current_player_idx = self.next_to_act(self.dealer_pos);
    }

    /// Main/side pots for the current hand.
    #[must_use]
    pub fn calculate_side_pots(&self) -> Vec<SidePot> {
        let eligible = self.in_hand();
        rules::calculate_side_pots(&self.players, self.pot, &eligible)
    }

    /// Sorts seats clockwise from the left of the dealer (odd chips go first).
    #[must_use]
    pub fn order_left_of_dealer(&self, seats: &[usize]) -> Vec<usize> {
        let n = self.players.len() as i32;
        let mut ordered = seats.to_vec();
        ordered.sort_by_key(|seat| {
            let distance = (*seat as i32 - self.dealer_pos + n) % n;
            if distance == 0 { n } else { distance }
        });
        ordered
    }

    /// Runs the showdown and settles the pot.
    pub fn showdown(&mut self) {
        let live = self.in_hand();
        if live.len() <= 1 {
            self.finish_by_fold(live.first().copied());
            return;
        }
        let mut evaluations: Vec<Option<EvaluatedHand>> = vec![None; self.players.len()];
        for &seat in &live {
            let mut cards = self.players[seat].hole_cards.clone();
            cards.extend_from_slice(&self.community_cards);
            evaluations[seat] = Some(evaluate(&cards, self.config.variant));
        }

        let side_pots = self.calculate_side_pots();
        let mut winner_amounts: Vec<(usize, u64)> = Vec::new();
        let mut all_winners: Vec<usize> = Vec::new();
        for side_pot in side_pots {
            let mut best: Option<u64> = None;
            let mut winners: Vec<usize> = Vec::new();
            for &seat in &side_pot.eligible {
                let Some(hand) = evaluations[seat] else {
                    continue;
                };
                let score = hand.score();
                match best {
                    None => {
                        best = Some(score);
                        winners = vec![seat];
                    }
                    Some(current) if score > current => {
                        best = Some(score);
                        winners = vec![seat];
                    }
                    Some(current) if score == current => {
                        if !winners.contains(&seat) {
                            winners.push(seat);
                        }
                    }
                    Some(_) => {}
                }
            }
            let ordered = self.order_left_of_dealer(&winners);
            if ordered.is_empty() {
                continue;
            }
            let share = side_pot.amount / ordered.len() as u64;
            let remainder = side_pot.amount - share * ordered.len() as u64;
            for (index, &seat) in ordered.iter().enumerate() {
                let award = share + u64::from((index as u64) < remainder);
                self.players[seat].stack += award;
                if let Some(entry) = winner_amounts
                    .iter_mut()
                    .find(|(winner, _)| *winner == seat)
                {
                    entry.1 += award;
                } else {
                    winner_amounts.push((seat, award));
                }
            }
            all_winners.extend(ordered);
        }

        let mut unique_winners: Vec<usize> = Vec::new();
        for seat in all_winners {
            if !unique_winners.contains(&seat) {
                unique_winners.push(seat);
            }
        }
        let strongest = evaluations
            .iter()
            .flatten()
            .map(|hand| hand.rank_index())
            .max()
            .unwrap_or(0);
        self.last_hand_result = Some(HandResult {
            reason: HandEndReason::Showdown,
            is_short_deck: self.is_short_deck(),
            pot: self.pot,
            hand_rank: strongest,
            winners: unique_winners
                .iter()
                .map(|seat| self.identity(*seat))
                .collect(),
            winner_amounts: winner_amounts
                .iter()
                .map(|(seat, amount)| (self.players[*seat].id.clone(), *amount))
                .collect(),
            results: live
                .iter()
                .map(|seat| HandPlayerResult {
                    identity: self.identity(*seat),
                    cards: self.players[*seat].hole_cards.clone(),
                    hand_rank: evaluations[*seat].map_or(0, EvaluatedHand::rank_index),
                })
                .collect(),
        });
        self.finish_hand();
    }

    fn identity(&self, seat: usize) -> PlayerIdentity {
        let player = &self.players[seat];
        PlayerIdentity {
            id: player.id.clone(),
            seat_id: player.seat_id,
            name: player.name.clone(),
            is_human: player.is_human,
        }
    }

    fn finish_by_fold(&mut self, winner: Option<usize>) {
        let Some(winner_seat) = winner else {
            self.finish_hand();
            return;
        };
        self.refund_uncalled();
        let settled_pot = self.pot;
        self.players[winner_seat].stack += settled_pot;
        self.last_hand_result = Some(HandResult {
            reason: HandEndReason::Fold,
            is_short_deck: self.is_short_deck(),
            pot: settled_pot,
            hand_rank: 0,
            winners: vec![self.identity(winner_seat)],
            winner_amounts: vec![(self.players[winner_seat].id.clone(), settled_pot)],
            results: (0..self.players.len())
                .filter(|seat| self.players[*seat].dealt_in)
                .map(|seat| HandPlayerResult {
                    identity: self.identity(seat),
                    cards: Vec::new(),
                    hand_rank: 0,
                })
                .collect(),
        });
        self.finish_hand();
    }

    fn finish_hand(&mut self) {
        self.phase = Phase::Idle;
        self.current_player_idx = -1;
        self.pot = 0;
        self.turn_id += 1;
    }

    /// Resets the table to an empty idle state (used when leaving a room).
    pub fn reset(&mut self) {
        self.hand_id = 0;
        self.turn_id = 0;
        self.phase = Phase::Idle;
        self.community_cards.clear();
        self.deck.clear();
        self.pot = 0;
        self.current_bet = 0;
        self.last_raise = self.minimum_bet();
        self.has_full_bet_this_round = false;
        self.round_bets.fill(0);
        self.players_acted.fill(false);
        self.acted_at_bet.fill(0);
        self.raise_size_at_action.fill(0);
        self.dealer_pos = -1;
        self.sb_idx = -1;
        self.bb_idx = -1;
        self.current_player_idx = -1;
        self.preflop_raise_count = 0;
        self.preflop_aggressor = -1;
        self.last_aggressor = -1;
        self.last_aggressor_street = None;
        self.flop_had_aggression = false;
        self.actions_this_round = 0;
        self.last_hand_result = None;
        self.players_dealt_last_hand.clear();
        self.last_action_meta = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::{Card, Rank, Suit};

    fn players(count: usize, stack: u64) -> Vec<Player> {
        (0..count)
            .map(|seat| Player::new(format!("p{seat}"), seat, format!("P{seat}"), stack))
            .collect()
    }

    fn game_with_players(count: usize, stack: u64) -> Game {
        Game::new(GameConfig::standard(stack, 50, 100), players(count, stack))
    }

    #[test]
    fn heads_up_blinds_close_and_current_bet() {
        let mut game = game_with_players(2, 1000);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        assert_eq!(game.dealer_pos(), 0);
        assert_eq!(game.sb_idx(), 0, "heads-up dealer posts the small blind");
        assert_eq!(game.bb_idx(), 1);
        assert_eq!(game.pot(), 150);
        assert_eq!(game.current_bet(), 100);
        assert_eq!(game.current_player_idx(), 0);
        assert_eq!(game.phase(), Phase::Preflop);
    }

    #[test]
    fn short_deck_antes_include_doubled_dealer_ante() {
        let config = GameConfig::short_deck(1000, 40, 80);
        let mut game = Game::new(config, players(6, 1000));
        assert!(game.start_hand_with_deck(Deck::short_deck(), Some(0)));
        assert_eq!(game.pot(), 6 * 40 + 40);
        assert_eq!(game.current_bet(), 0);
        assert_eq!(
            game.current_player_idx(),
            1,
            "action starts left of the dealer"
        );
        assert_eq!(game.round_bets(), &[0, 0, 0, 0, 0, 0]);
        assert_eq!(game.last_raise(), 80);
    }

    #[test]
    fn fold_win_refunds_the_uncalled_wager() {
        let mut game = game_with_players(3, 1000);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        // Dealer 0, SB 1, BB 2, first to act seat 0.
        assert_eq!(game.current_player_idx(), 0);
        game.act(0, Action::Fold).expect("fold");
        game.act(1, Action::Fold).expect("fold");
        let result = game.last_hand_result().expect("settled");
        assert_eq!(result.reason, HandEndReason::Fold);
        assert_eq!(result.pot, 100);
        assert_eq!(result.winners[0].seat_id, 2);
        assert_eq!(game.players()[2].stack, 1050);
        assert_eq!(game.players()[1].stack, 950);
        assert_eq!(game.players()[0].stack, 1000);
        assert_eq!(game.phase(), Phase::Idle);
    }

    #[test]
    fn heads_up_all_in_runs_out_the_board_and_settles() {
        let mut game = game_with_players(2, 100);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        assert_eq!(game.current_player_idx(), 0);
        game.act(0, Action::AllIn).expect("all-in");
        let result = game.last_hand_result().expect("settled");
        assert_eq!(result.reason, HandEndReason::Showdown);
        assert_eq!(result.pot, 200);
        assert_eq!(game.community_cards().len(), 5);
        let total: u64 = game.players().iter().map(|player| player.stack).sum();
        assert_eq!(total, 200);
    }

    #[test]
    fn raise_off_grid_is_rejected_with_js_message() {
        let mut game = game_with_players(3, 1000);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        let error = game
            .act(0, Action::Raise { to: 275 })
            .expect_err("off-grid");
        assert_eq!(
            error.to_string(),
            "加注金额必须是 50 的整数倍；零头筹码可使用全下"
        );
        let error = game
            .act(0, Action::Raise { to: 50 })
            .expect_err("out of range");
        assert_eq!(error.to_string(), "加注金额不在合法范围内");
    }

    #[test]
    fn out_of_turn_action_is_rejected() {
        let mut game = game_with_players(3, 1000);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        let error = game.act(1, Action::Call).expect_err("not your turn");
        assert_eq!(error.to_string(), "当前不能执行这个行动");
    }

    fn play_out_with_checks(game: &mut Game) {
        let mut guard = 0;
        while game.phase() != Phase::Idle {
            guard += 1;
            assert!(guard < 100, "hand did not finish");
            let seat = game.current_player_idx() as usize;
            let legal = game.legal_actions(seat);
            assert!(
                !legal.actions.is_empty(),
                "no legal actions at guard {guard}"
            );
            let action = if legal.contains(ActionType::Check) {
                Action::Check
            } else if legal.contains(ActionType::Call) {
                Action::Call
            } else {
                panic!("unexpected decision point: {legal:?}");
            };
            game.act(seat, action).expect("auto action");
        }
    }

    #[test]
    fn side_pot_settlement_requires_correct_eligibility() {
        // Three players: seat 0 has 100 (all-in), seats 1 and 2 have 300.
        let mut seats = players(3, 300);
        seats[0].stack = 100;
        let config = GameConfig::standard(300, 50, 100);
        let mut game = Game::new(config, seats);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        // Button 0, SB 1, BB 2; seat 0 acts first (shoves its short stack).
        game.act(0, Action::AllIn).expect("all-in");
        game.act(1, Action::Raise { to: 300 }).expect("raise");
        game.act(2, Action::Call).expect("call");
        play_out_with_checks(&mut game);
        let result = game.last_hand_result().expect("settled");
        assert_eq!(result.reason, HandEndReason::Showdown);
        assert_eq!(result.pot, 700);
        let total: u64 = game.players().iter().map(|player| player.stack).sum();
        assert_eq!(total, 700);
        assert!(!result.winner_amounts.is_empty());
        // Seat 0 is only eligible for the 300 main pot.
        assert!(
            result
                .winner_amounts
                .iter()
                .map(|(_, amount)| *amount)
                .sum::<u64>()
                == 700
        );
    }

    #[test]
    fn blind_and_ante_labels_match_js() {
        let mut standard = game_with_players(3, 1000);
        standard.start_hand_with_deck(Deck::standard(), Some(0));
        let labels: Vec<Option<LastAction>> = standard
            .players()
            .iter()
            .map(|player| player.last_action)
            .collect();
        assert_eq!(
            labels[1],
            Some(LastAction {
                action: ActionLabel::SmallBlind,
                amount: 50
            })
        );
        assert_eq!(
            labels[2],
            Some(LastAction {
                action: ActionLabel::BigBlind,
                amount: 100
            })
        );

        let mut short = Game::new(GameConfig::short_deck(1000, 40, 80), players(4, 1000));
        short.start_hand_with_deck(Deck::short_deck(), Some(0));
        assert_eq!(
            short.players()[0].last_action,
            Some(LastAction {
                action: ActionLabel::DealerAnte,
                amount: 40
            })
        );
        assert_eq!(
            short.players()[1].last_action,
            Some(LastAction {
                action: ActionLabel::Ante,
                amount: 40
            })
        );
    }

    #[test]
    fn deal_order_matches_clockwise_js_dealing() {
        let mut game = Game::new(GameConfig::standard(1000, 50, 100), players(3, 1000));
        let deck = Deck::standard();
        let mut expected = deck.cards().to_vec();
        game.start_hand_with_deck(deck, Some(0));
        // Dealer 0: deal order is seat 1, 2, 0 then again.
        let mut expected_holes = vec![Vec::new(); 3];
        for _round in 0..2 {
            for seat in [1usize, 2, 0] {
                expected_holes[seat].push(expected.pop().expect("deck"));
            }
        }
        for (seat, cards) in expected_holes.iter().enumerate() {
            assert_eq!(&game.players()[seat].hole_cards, cards);
        }
        assert_eq!(game.deck_len(), expected.len());
    }

    #[test]
    fn card_constants_are_constructible_for_tests() {
        let card = Card::new(Suit::Spades, Rank::Ace);
        assert_eq!(card.rank_value(), 14);
    }
}
