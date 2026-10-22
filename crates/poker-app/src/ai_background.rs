//! One FIFO worker owns the table AI; long searches never borrow the GUI state.
use crate::ai::{AiPolicy, AiView};
use crate::ai_bridge::BridgeInput;
use poker_engine::Action;
use std::sync::mpsc::{self, Receiver, Sender};
type Key = (u64, u64, usize);
enum Command {
    Decide(Box<BridgeInput>),
    Observe(poker_ai::ObservedAction),
    Showdown(usize, poker_ai::ShowdownResult),
}
pub struct BackgroundAi {
    tx: Sender<Command>,
    rx: Receiver<(Key, Action)>,
    pending: Option<Key>,
}
impl BackgroundAi {
    pub fn new(mut policy: Box<dyn AiPolicy>) -> Self {
        let (tx, commands) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("poker-deep-ai".into())
            .spawn(move || {
                while let Ok(cmd) = commands.recv() {
                    match cmd {
                        Command::Observe(event) => policy.observe(&event),
                        Command::Showdown(seat, result) => policy.showdown(seat, &result),
                        Command::Decide(input) => {
                            let key = (input.hand_id, input.turn_id, input.seat);
                            let action = policy.decide(&input.view());
                            if results.send((key, action)).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("AI worker");
        Self {
            tx,
            rx,
            pending: None,
        }
    }
    fn request(&mut self, view: &AiView<'_>) -> bool {
        let key = (view.hand_id, view.turn_id, view.seat);
        if self.pending != Some(key) {
            if self
                .tx
                .send(Command::Decide(Box::new(BridgeInput::from_view(view))))
                .is_err()
            {
                return false;
            }
            self.pending = Some(key);
        }
        true
    }
}
impl AiPolicy for BackgroundAi {
    fn name(&self) -> &str {
        "深度 Rust AI"
    }
    fn decide(&mut self, view: &AiView<'_>) -> Action {
        if !self.request(view) {
            return crate::ai::fallback_action(view.legal);
        }
        let key = (view.hand_id, view.turn_id, view.seat);
        while let Ok((k, a)) = self.rx.recv() {
            if k == key {
                self.pending = None;
                return a;
            }
        }
        crate::ai::fallback_action(view.legal)
    }
    fn poll_decision(&mut self, view: &AiView<'_>) -> Option<Action> {
        if !self.request(view) {
            return Some(crate::ai::fallback_action(view.legal));
        }
        let key = (view.hand_id, view.turn_id, view.seat);
        loop {
            match self.rx.try_recv() {
                Ok((k, a)) if k == key => {
                    self.pending = None;
                    return Some(a);
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Some(crate::ai::fallback_action(view.legal));
                }
            }
        }
    }
    fn observe(&mut self, event: &poker_ai::ObservedAction) {
        let _ = self.tx.send(Command::Observe(event.clone()));
    }
    fn showdown(&mut self, seat: usize, result: &poker_ai::ShowdownResult) {
        let _ = self.tx.send(Command::Showdown(seat, *result));
    }
}
impl BridgeInput {
    fn view(&self) -> AiView<'_> {
        AiView {
            seat: self.seat,
            phase: self.phase,
            hand_id: self.hand_id,
            turn_id: self.turn_id,
            hole_cards: &self.hole_cards,
            community_cards: &self.community_cards,
            pot: self.pot,
            to_call: self.to_call,
            stack: self.stack,
            legal: &self.legal,
            variant: self.variant,
            players: &self.players,
            current_bet: self.current_bet,
            dealer_pos: self.dealer_pos,
            sb_idx: self.sb_idx,
            bb_idx: self.bb_idx,
            actions_this_round: self.actions_this_round,
            preflop_raise_count: self.preflop_raise_count,
            last_aggressor: self.last_aggressor,
            last_aggressor_street: self.last_aggressor_street,
            preflop_aggressor: self.preflop_aggressor,
            flop_had_aggression: self.flop_had_aggression,
            starting_stack: self.starting_stack,
            big_blind: self.big_blind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Blocked {
        gate: Receiver<()>,
    }
    impl AiPolicy for Blocked {
        fn name(&self) -> &str {
            "blocked test"
        }
        fn decide(&mut self, view: &AiView<'_>) -> Action {
            self.gate.recv().unwrap();
            crate::ai::fallback_action(view.legal)
        }
    }
    #[test]
    fn interactive_tick_returns_while_ai_is_waiting_and_applies_result_later() {
        let (tx, gate) = mpsc::channel();
        let policy = BackgroundAi::new(Box::new(Blocked { gate }));
        let settings = crate::settings::Settings {
            players: 2,
            ..Default::default()
        };
        let config = crate::session::TableConfig::fast(&settings, "background-test");
        let mut session = crate::session::TableSession::new(&config, "玩家", Box::new(policy));
        assert!(session.start());
        while session.is_human_turn() {
            let action = crate::table_ctrl::primary_action(&session.legal_actions()).unwrap();
            session.human_act(action).unwrap();
        }
        let turn = session.game().turn_id();
        let now = std::time::Instant::now() + std::time::Duration::from_secs(1);
        assert!(!session.tick(now));
        assert_eq!(session.game().turn_id(), turn);
        tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while session.game().turn_id() == turn {
            session.tick(std::time::Instant::now() + std::time::Duration::from_secs(2));
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
