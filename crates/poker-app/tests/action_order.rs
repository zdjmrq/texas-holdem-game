//! Exercise the normal session/tick path, including visible AI pacing.
use poker_app::{
    ai::{AiPolicy, AiView},
    session::{TableConfig, TableSession},
    settings::Settings,
};
use poker_engine::{Action, Phase};
use std::time::{Duration, Instant};

struct Passive;
impl AiPolicy for Passive {
    fn name(&self) -> &str {
        "passive regression"
    }
    fn decide(&mut self, view: &AiView<'_>) -> Action {
        poker_app::table_ctrl::primary_action(view.legal).unwrap()
    }
}

fn step(table: &mut TableSession, expected: usize, clock: &mut Instant) {
    assert_eq!(table.current_actor(), Some(expected));
    let old_turn = table.game().turn_id();
    if table.is_human_turn() {
        table
            .human_act(poker_app::table_ctrl::primary_action(&table.legal_actions()).unwrap())
            .unwrap();
    } else {
        assert!(
            !table.tick(Instant::now()),
            "AI action must remain visible for its delay"
        );
        assert_eq!(table.game().turn_id(), old_turn);
        *clock += Duration::from_secs(5);
        assert!(table.tick(*clock));
        assert_eq!(
            table.game().turn_id(),
            old_turn + 1,
            "one tick must apply only one action"
        );
    }
}

#[test]
fn normal_session_never_fast_forwards_the_blinds_on_postflop_streets() {
    for count in 2..=10 {
        for seed in 0..6 {
            let settings = Settings {
                players: count,
                ai_delay_ms: 1800,
                ..Default::default()
            };
            let config = TableConfig::from_settings(&settings, Some(format!("order-{seed}")));
            let mut table = TableSession::new(&config, "Human", Box::new(Passive));
            assert!(table.start());
            let mut clock = Instant::now();
            let button = table.game().dealer_pos() as usize;
            let first = if count == 2 {
                button
            } else {
                (button + 3) % count
            };
            for offset in 0..count {
                step(&mut table, (first + offset) % count, &mut clock);
            }
            for phase in [Phase::Flop, Phase::Turn, Phase::River] {
                assert_eq!(table.game().phase(), phase);
                assert!(
                    table
                        .game()
                        .players()
                        .iter()
                        .all(|p| p.round_action.is_none() && p.round_bet == 0)
                );
                for offset in 1..=count {
                    step(&mut table, (button + offset) % count, &mut clock);
                }
            }
            assert!(table.last_settlement().is_some());
        }
    }
}
