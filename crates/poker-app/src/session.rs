//! Local table session: owns the poker-engine Game, the seats, the AI policy
//! and the hand-to-hand flow (settlement, next hand, dealer rotation).
//!
//! The session is pure UI-independent logic (only std::time::Instant for the AI
//! pacing), so it can be driven from tests and from the screenshot scenarios
//! without an egui context.

use std::time::{Duration, Instant};

use poker_core::{Card, EvaluatedHand, RuntimeRng, Variant, evaluate};
use poker_engine::{ActError, Action, Game, GameConfig, HandResult, LegalActions, Phase, Player};

use crate::ai::{AiPolicy, AiView};
use crate::settings::Settings;

/// Why the local game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOver {
    /// The human lost their whole stack.
    HumanBusted,
    /// Every AI opponent busted: the human won the table.
    HumanWins,
    /// Fewer than two players can be dealt in.
    TableBroken,
}

impl GameOver {
    pub fn label(self) -> &'static str {
        match self {
            Self::HumanBusted => "你的筹码输光了",
            Self::HumanWins => "你赢下了整张牌桌",
            Self::TableBroken => "牌桌人数不足，对局结束",
        }
    }
}

/// Static configuration for one local session.
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub variant: Variant,
    pub players: usize,
    pub starting_stack: u64,
    pub small_blind: u64,
    pub big_blind: u64,
    pub ante: u64,
    pub ai_delay: Duration,
    /// None for normal play; Some only for an explicitly reproducible game.
    pub seed: Option<String>,
}

impl TableConfig {
    pub fn from_settings(settings: &Settings, seed: Option<String>) -> Self {
        Self {
            variant: settings.variant(),
            players: settings.players,
            starting_stack: settings.starting_stack,
            small_blind: settings.small_blind,
            big_blind: settings.big_blind,
            ante: settings.ante,
            ai_delay: settings.ai_delay(),
            seed,
        }
    }

    /// Fast deterministic configuration for scenarios and tests.
    pub fn fast(settings: &Settings, seed: impl Into<String>) -> Self {
        Self {
            ai_delay: Duration::ZERO,
            ..Self::from_settings(settings, Some(seed.into()))
        }
    }

    pub fn game_config(&self) -> GameConfig {
        if self.variant.is_short_deck() {
            GameConfig::short_deck(self.starting_stack, self.ante, self.ante.saturating_mul(2))
                .with_max_players(8)
        } else {
            GameConfig::standard(self.starting_stack, self.small_blind, self.big_blind)
                .with_max_players(10)
        }
    }
}

/// One revealed hand at showdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowdownEntry {
    pub seat: usize,
    pub name: String,
    pub cards: Vec<Card>,
    pub hand_name: &'static str,
}

/// Settlement summary shown by the result dialog.
#[derive(Debug, Clone)]
pub struct Settlement {
    pub hand_id: u64,
    pub result: HandResult,
    pub winners: Vec<(String, u64)>,
    pub human_delta: i64,
    pub stacks_before: Vec<u64>,
    pub showdown: Vec<ShowdownEntry>,
    pub board: Vec<Card>,
    pub game_over: Option<GameOver>,
}

impl Settlement {
    pub fn pot(&self) -> u64 {
        self.result.pot
    }

    pub fn human_won(&self) -> bool {
        self.human_delta > 0
    }
}

/// A playable local table.
pub struct TableSession {
    game: Game,
    rng: RuntimeRng,
    human_seat: usize,
    ai: Box<dyn AiPolicy + Send>,
    ai_delay: Duration,
    next_ai_at: Option<Instant>,
    last_settlement: Option<Settlement>,
    game_over: Option<GameOver>,
    stacks_at_hand_start: Vec<u64>,
    events: Vec<String>,
}

impl TableSession {
    pub fn new(config: &TableConfig, human_name: &str, ai: Box<dyn AiPolicy + Send>) -> Self {
        let max_seats = if config.variant.is_short_deck() {
            8
        } else {
            10
        };
        let seat_count = config.players.clamp(2, max_seats);
        let players: Vec<Player> = (0..seat_count)
            .map(|seat| {
                if seat == 0 {
                    Player::new("human", 0, human_name, config.starting_stack).with_human(true)
                } else {
                    Player::new(
                        format!("ai-{seat}"),
                        seat,
                        [
                            "Alex", "Blake", "Casey", "Drew", "Emma", "Finn", "Grace", "Hayes",
                            "Ivy",
                        ][seat - 1],
                        config.starting_stack,
                    )
                    .with_human(false)
                }
            })
            .collect();
        let game = Game::new(config.game_config(), players);
        Self {
            game,
            rng: RuntimeRng::from_optional_seed(config.seed.as_deref()),
            human_seat: 0,
            ai,
            ai_delay: config.ai_delay,
            next_ai_at: None,
            last_settlement: None,
            game_over: None,
            stacks_at_hand_start: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Start the first hand. Returns false when the table cannot be dealt.
    pub fn start(&mut self) -> bool {
        if self.game.hand_id() != 0 {
            return false;
        }
        self.start_next_hand(Instant::now())
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn human_seat(&self) -> usize {
        self.human_seat
    }

    pub fn ai_name(&self) -> &str {
        self.ai.name()
    }

    pub fn human(&self) -> &Player {
        &self.game.players()[self.human_seat]
    }

    pub fn current_actor(&self) -> Option<usize> {
        let index = self.game.current_player_idx();
        if index < 0 {
            None
        } else {
            usize::try_from(index).ok()
        }
    }

    pub fn is_human_turn(&self) -> bool {
        self.game.phase().is_active() && self.current_actor() == Some(self.human_seat)
    }

    pub fn legal_actions(&self) -> LegalActions {
        self.game.legal_actions(self.human_seat)
    }

    /// Number of opponents still in the hand (at least 1 for analysis).
    pub fn live_opponents(&self) -> usize {
        self.game
            .in_hand()
            .into_iter()
            .filter(|seat| *seat != self.human_seat)
            .count()
            .max(1)
    }

    /// The human hand evaluated against the current board.
    pub fn human_hand(&self) -> Option<EvaluatedHand> {
        let hole = &self.human().hole_cards;
        if hole.is_empty() {
            return None;
        }
        let mut cards = hole.clone();
        cards.extend_from_slice(self.game.community_cards());
        if (2..=7).contains(&cards.len()) {
            Some(evaluate(&cards, self.game.config().variant))
        } else {
            None
        }
    }

    pub fn big_blind(&self) -> u64 {
        self.game.config().minimum_bet().max(1)
    }

    pub fn last_settlement(&self) -> Option<&Settlement> {
        self.last_settlement.as_ref()
    }

    pub fn game_over(&self) -> Option<GameOver> {
        self.game_over
    }

    pub fn events(&self) -> &[String] {
        &self.events
    }

    /// Apply a human action. Returns a user-facing error when the action is
    /// illegal or it is not the human's turn.
    pub fn human_act(&mut self, action: Action) -> Result<(), String> {
        if !self.is_human_turn() {
            return Err("现在不是你的行动".to_owned());
        }
        let event = poker_ai::host::act(&mut self.game, self.human_seat, action)
            .map_err(|error: ActError| error.to_string())?;
        self.ai.observe(&event);
        self.push_event(format!(
            "你：{}",
            action_label(&action, self.game.config().variant)
        ));
        self.after_action(Instant::now());
        Ok(())
    }

    /// Advance the AI when its scheduled delay has elapsed. Executes at most one
    /// action per call so the UI stays responsive and shows AI pacing.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.tick_impl(now, false)
    }

    fn tick_impl(&mut self, now: Instant, blocking: bool) -> bool {
        let Some(due) = self.next_ai_at else {
            return false;
        };
        if now < due {
            return false;
        }
        if !self.game.phase().is_active() {
            self.next_ai_at = None;
            return false;
        }
        let Some(actor) = self.current_actor() else {
            self.next_ai_at = None;
            return false;
        };
        if actor == self.human_seat {
            self.next_ai_at = None;
            return false;
        }
        self.next_ai_at = None;
        let Some(action) = self.decide_ai_action(actor, blocking) else {
            self.next_ai_at = Some(now + Duration::from_millis(10));
            return false;
        };
        let action = if self
            .game
            .legal_actions(actor)
            .contains(action.action_type())
        {
            action
        } else {
            crate::ai::fallback_action(&self.game.legal_actions(actor))
        };
        let (action, event) = match poker_ai::host::act(&mut self.game, actor, action) {
            Ok(event) => (action, event),
            Err(_) => {
                // Safety net: never let a policy error wedge the table.
                let fallback = crate::ai::fallback_action(&self.game.legal_actions(actor));
                match poker_ai::host::act(&mut self.game, actor, fallback) {
                    Ok(event) => (fallback, event),
                    Err(_) => return false,
                }
            }
        };
        self.ai.observe(&event);
        self.push_event(format!(
            "{}：{}",
            self.game.players()[actor].name,
            action_label(&action, self.game.config().variant)
        ));
        self.after_action(now);
        true
    }

    /// Immediately run the next AI action (tests / deterministic scenarios).
    pub fn advance_ai_immediately(&mut self) -> bool {
        let now = Instant::now();
        self.next_ai_at = Some(now);
        self.tick_impl(now, true)
    }

    /// Start the next hand after a settlement.
    pub fn next_hand(&mut self) -> bool {
        if self.game_over.is_some() || self.last_settlement.is_none() {
            return false;
        }
        self.last_settlement = None;
        self.start_next_hand(Instant::now())
    }

    fn start_next_hand(&mut self, now: Instant) -> bool {
        self.stacks_at_hand_start = self
            .game
            .players()
            .iter()
            .map(|player| player.stack)
            .collect();
        let started = self.game.start_hand(&mut self.rng);
        if !started {
            self.game_over = Some(GameOver::TableBroken);
            return false;
        }
        self.push_event(format!("第 {} 手开始", self.game.hand_id()));
        self.next_ai_at = None;
        self.after_action(now);
        true
    }

    fn decide_ai_action(&mut self, actor: usize, blocking: bool) -> Option<Action> {
        let legal = self.game.legal_actions(actor);
        let players = self.game.players();
        let view = AiView {
            seat: actor,
            phase: self.game.phase(),
            hand_id: self.game.hand_id(),
            turn_id: self.game.turn_id(),
            hole_cards: &players[actor].hole_cards,
            community_cards: self.game.community_cards(),
            pot: self.game.pot(),
            to_call: legal.to_call,
            stack: players[actor].stack,
            legal: &legal,
            variant: self.game.config().variant,
            players,
            current_bet: self.game.current_bet(),
            dealer_pos: self.game.dealer_pos(),
            sb_idx: self.game.sb_idx(),
            bb_idx: self.game.bb_idx(),
            actions_this_round: self.game.actions_this_round(),
            preflop_raise_count: self.game.preflop_raise_count(),
            last_aggressor: self.game.last_aggressor(),
            last_aggressor_street: self.game.last_aggressor_street(),
            preflop_aggressor: self.game.preflop_aggressor(),
            flop_had_aggression: self.game.flop_had_aggression(),
            starting_stack: self.game.config().starting_stack,
            big_blind: self.game.minimum_bet(),
        };
        if blocking {
            Some(self.ai.decide(&view))
        } else {
            self.ai.poll_decision(&view)
        }
    }

    fn after_action(&mut self, now: Instant) {
        self.next_ai_at = None;
        if self.game.phase() == Phase::Idle {
            if let Some(result) = self.game.last_hand_result().cloned() {
                self.settle(result);
            }
            return;
        }
        if let Some(actor) = self.current_actor()
            && actor != self.human_seat
        {
            self.next_ai_at = Some(now + self.ai_delay);
        }
    }

    fn settle(&mut self, result: HandResult) {
        for (seat, result) in poker_ai::host::showdown(&self.game) {
            self.ai.showdown(seat, &result);
        }
        let players = self.game.players();
        let human = self.human_seat;
        let human_stack = players[human].stack;
        let human_before = self
            .stacks_at_hand_start
            .get(human)
            .copied()
            .unwrap_or(human_stack);
        let human_delta = i64::try_from(human_stack).unwrap_or(i64::MAX)
            - i64::try_from(human_before).unwrap_or(i64::MAX);

        let winners: Vec<(String, u64)> = result
            .winner_amounts
            .iter()
            .map(|(id, amount)| {
                let name = players
                    .iter()
                    .find(|player| &player.id == id)
                    .map_or_else(|| id.clone(), |player| player.name.clone());
                (name, *amount)
            })
            .collect();

        let variant = self.game.config().variant;
        let showdown: Vec<ShowdownEntry> = result
            .results
            .iter()
            .filter(|entry| !entry.cards.is_empty())
            .map(|entry| ShowdownEntry {
                seat: entry.identity.seat_id,
                name: entry.identity.name.clone(),
                cards: entry.cards.clone(),
                hand_name: variant
                    .hand_type_names()
                    .get(usize::from(entry.hand_rank))
                    .copied()
                    .unwrap_or("高牌"),
            })
            .collect();

        let alive = players.iter().filter(|player| player.stack > 0).count();
        let game_over = if human_stack == 0 {
            Some(GameOver::HumanBusted)
        } else if alive <= 1 {
            Some(GameOver::HumanWins)
        } else {
            None
        };
        self.game_over = game_over;
        self.last_settlement = Some(Settlement {
            hand_id: self.game.hand_id(),
            result,
            winners,
            human_delta,
            stacks_before: self.stacks_at_hand_start.clone(),
            showdown,
            board: self.game.community_cards().to_vec(),
            game_over,
        });
        let pot = self
            .last_settlement
            .as_ref()
            .map(|settlement| settlement.result.pot)
            .unwrap_or_default();
        self.push_event(format!("第 {} 手结束，底池 {}", self.game.hand_id(), pot));
    }

    fn push_event(&mut self, event: String) {
        self.events.push(event);
        if self.events.len() > 60 {
            self.events.remove(0);
        }
    }
}

/// Human readable action description for the status/log lines.
#[must_use]
pub fn action_label(action: &Action, variant: Variant) -> String {
    match action {
        Action::Fold => "弃牌".to_owned(),
        Action::Check => "过牌".to_owned(),
        Action::Call => "跟注".to_owned(),
        Action::Raise { to } => {
            let unit = if variant.is_short_deck() {
                "前注"
            } else {
                "BB"
            };
            format!("加注到 {to}（{unit} 显示见桌面）")
        }
        Action::AllIn => "全下".to_owned(),
    }
}

/// Drive a session until the human must act, a settlement appears or the hand
/// ends (used by deterministic screenshot scenarios and tests).
pub fn advance_ai_steps(session: &mut TableSession, max_steps: usize) -> usize {
    let mut steps = 0;
    while steps < max_steps && session.game().phase().is_active() && !session.is_human_turn() {
        if !session.advance_ai_immediately() {
            break;
        }
        steps += 1;
    }
    steps
}

/// Play the human's primary action (check/call) until the hand settles.
pub fn autoplay_hand(session: &mut TableSession) -> bool {
    for _ in 0..200 {
        if session.last_settlement().is_some() {
            return true;
        }
        if session.is_human_turn() {
            let legal = session.legal_actions();
            let Some(action) = crate::table_ctrl::primary_action(&legal) else {
                return false;
            };
            if session.human_act(action).is_err() {
                return false;
            }
        } else if !session.advance_ai_immediately() {
            return false;
        }
    }
    session.last_settlement().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::DeterministicAi;
    use poker_engine::ActionType;

    fn fast_config(players: usize, variant: Variant) -> TableConfig {
        TableConfig {
            variant,
            players,
            starting_stack: 2_000,
            small_blind: 50,
            big_blind: 100,
            ante: 40,
            ai_delay: Duration::ZERO,
            seed: Some("session-test".to_owned()),
        }
    }

    fn session(players: usize, variant: Variant) -> TableSession {
        TableSession::new(
            &fast_config(players, variant),
            "测试玩家",
            Box::new(DeterministicAi),
        )
    }

    #[test]
    fn session_starts_and_reaches_a_human_decision() {
        let mut table = session(4, Variant::Standard);
        assert!(table.start());
        advance_ai_steps(&mut table, 50);
        assert!(
            table.is_human_turn() || table.last_settlement().is_some(),
            "AI should either reach the human or finish the hand"
        );
        if table.is_human_turn() {
            assert!(!table.legal_actions().actions.is_empty());
        }
    }

    #[test]
    fn full_hand_settles_and_chips_are_conserved() {
        let mut table = session(4, Variant::Standard);
        let total_before: u64 = table.game().players().iter().map(|p| p.stack).sum();
        assert!(table.start());
        assert!(autoplay_hand(&mut table), "hand should settle");
        let settlement = table.last_settlement().expect("settlement");
        assert!(settlement.pot() > 0);
        let total_after: u64 = table.game().players().iter().map(|p| p.stack).sum();
        assert_eq!(total_before, total_after, "chips must be conserved");
        assert!(!settlement.winners.is_empty());
    }

    #[test]
    fn next_hand_rotates_the_dealer() {
        let mut table = session(4, Variant::Standard);
        assert!(table.start());
        let first_dealer = table.game().dealer_pos();
        assert!(autoplay_hand(&mut table));
        let second_dealer_before = table.game().dealer_pos();
        assert!(table.next_hand(), "next hand should start");
        assert_ne!(first_dealer, table.game().dealer_pos());
        assert_ne!(second_dealer_before, table.game().dealer_pos());
        assert_eq!(table.game().hand_id(), 2);
    }

    #[test]
    fn short_deck_session_uses_only_6_to_ace() {
        let mut table = session(4, Variant::ShortDeck);
        assert!(table.start());
        assert!(autoplay_hand(&mut table));
        for card in table.game().community_cards() {
            assert!(
                card.rank_value() >= 6,
                "short deck must not contain low cards: {card:?}"
            );
        }
    }

    #[test]
    fn human_hand_is_evaluable_after_the_flop() {
        let mut table = session(4, Variant::Standard);
        assert!(table.start());
        let mut guard = 0;
        while table.game().community_cards().len() < 3 && guard < 400 {
            guard += 1;
            if table.is_human_turn() {
                let legal = table.legal_actions();
                if let Some(action) = crate::table_ctrl::primary_action(&legal) {
                    let _ = table.human_act(action);
                }
            } else if !table.advance_ai_immediately() {
                break;
            }
            if table.last_settlement().is_some() {
                break;
            }
        }
        if table.game().community_cards().len() >= 3 {
            let hand = table.human_hand().expect("evaluated hand");
            assert!(hand.rank_index() <= 9);
            assert!(!hand.name().is_empty());
        }
    }

    #[test]
    fn autoplay_ten_hands_never_panics() {
        let mut table = session(3, Variant::Standard);
        assert!(table.start());
        for hand in 0..10 {
            assert!(autoplay_hand(&mut table), "hand {hand} should settle");
            if table.game_over().is_some() {
                break;
            }
            assert!(table.next_hand(), "hand {hand} should start the next one");
        }
    }

    #[test]
    fn invalid_ai_raise_falls_back_and_logs_the_action_actually_applied() {
        struct BadRaise;
        impl AiPolicy for BadRaise {
            fn name(&self) -> &str {
                "invalid raise test"
            }
            fn decide(&mut self, _: &AiView<'_>) -> Action {
                Action::Raise { to: 1 }
            }
        }
        let config = fast_config(4, Variant::Standard);
        let mut table = TableSession::new(&config, "Human", Box::new(BadRaise));
        assert!(table.start());
        for _ in 0..4 {
            if !table.is_human_turn() {
                break;
            }
            table
                .human_act(crate::ai::fallback_action(&table.legal_actions()))
                .unwrap();
        }
        assert!(!table.is_human_turn());
        let actor = table.current_actor().unwrap();
        assert!(
            table
                .game()
                .legal_actions(actor)
                .contains(ActionType::Raise)
        );
        let fallback = crate::ai::fallback_action(&table.game().legal_actions(actor));
        assert!(table.advance_ai_immediately());
        assert_eq!(
            table.events().last().unwrap(),
            &format!(
                "{}：{}",
                table.game().players()[actor].name,
                action_label(&fallback, config.variant)
            )
        );
        assert_eq!(
            table.game().players().iter().map(|p| p.stack).sum::<u64>() + table.game().pot(),
            8_000
        );
    }
}
