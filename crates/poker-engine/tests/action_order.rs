//! Rule-based regressions independent of the legacy JS oracle.
use poker_core::Deck;
use poker_engine::{Action, ActionType, Game, GameConfig, Phase, Player};

fn table(stacks: &[u64], button: usize) -> Game {
    let players = stacks
        .iter()
        .enumerate()
        .map(|(i, &stack)| Player::new(format!("p{i}"), i, format!("P{i}"), stack))
        .collect();
    let mut game = Game::new(GameConfig::standard(10_000, 50, 100), players);
    assert!(game.start_hand_with_deck(Deck::standard(), Some(button)));
    game
}

fn passive(game: &mut Game, seat: usize) {
    assert_eq!(game.current_player_idx(), seat as i32, "{:?}", game.phase());
    let legal = game.legal_actions(seat);
    game.act(
        seat,
        if legal.contains(ActionType::Check) {
            Action::Check
        } else {
            Action::Call
        },
    )
    .unwrap();
}

#[test]
fn every_button_and_table_size_uses_utg_preflop_and_button_left_on_all_later_streets() {
    for count in 2..=10 {
        for button in 0..count {
            let mut game = table(&vec![10_000; count], button);
            let first = if count == 2 {
                button
            } else {
                (button + 3) % count
            };
            for offset in 0..count {
                passive(&mut game, (first + offset) % count);
            }
            for phase in [Phase::Flop, Phase::Turn, Phase::River] {
                assert_eq!(game.phase(), phase);
                // Includes the big blind's preflop option, then SB/BB/.../button.
                for offset in 1..=count {
                    passive(&mut game, (button + offset) % count);
                }
            }
            assert_eq!(game.phase(), Phase::Idle);
        }
    }
}

#[test]
fn postflop_raise_returns_clockwise_to_players_who_checked() {
    let mut game = table(&[10_000; 5], 0);
    for seat in [3, 4, 0, 1, 2] {
        passive(&mut game, seat);
    }
    passive(&mut game, 1);
    passive(&mut game, 2);
    game.act(3, Action::Raise { to: 300 }).unwrap();
    for seat in [4, 0, 1] {
        passive(&mut game, seat);
    }
    assert_eq!(game.phase(), Phase::Flop, "BB must still respond");
    passive(&mut game, 2);
    assert_eq!(game.phase(), Phase::Turn);
    assert_eq!(game.current_player_idx(), 1);
}

#[test]
fn folded_small_blind_is_skipped_but_big_blind_still_opens_each_later_street() {
    let mut game = table(&[10_000; 5], 0);
    for seat in [3, 4, 0] {
        passive(&mut game, seat);
    }
    game.act(1, Action::Fold).unwrap();
    passive(&mut game, 2);
    for phase in [Phase::Flop, Phase::Turn, Phase::River] {
        assert_eq!(game.phase(), phase);
        for seat in [2, 3, 4, 0] {
            passive(&mut game, seat);
        }
    }
}

#[test]
fn lone_live_player_can_only_call_or_fold_against_all_in_opponents() {
    let mut game = table(&[1_000, 500, 1_000], 0);
    game.act(0, Action::Call).unwrap();
    game.act(1, Action::AllIn).unwrap();
    game.act(2, Action::Fold).unwrap();
    assert_eq!(game.current_player_idx(), 0);
    let legal = game.legal_actions(0);
    assert_eq!(legal.actions, [ActionType::Fold, ActionType::Call]);
    assert!(!legal.can_raise);
    let before = format!("{game:?}");
    assert!(game.act(0, Action::Raise { to: 900 }).is_err());
    assert_eq!(format!("{game:?}"), before);
    game.act(0, Action::Call).unwrap();
    assert_eq!(game.phase(), Phase::Idle, "no phantom turns after the call");
    assert_eq!(game.community_cards().len(), 5);
}

#[test]
fn all_in_small_blind_is_skipped_without_skipping_the_live_big_blind() {
    let mut game = table(&[1_000, 50, 1_000, 1_000], 0);
    for seat in [3, 0, 2] {
        passive(&mut game, seat);
    }
    for phase in [Phase::Flop, Phase::Turn, Phase::River] {
        assert_eq!(game.phase(), phase);
        for seat in [2, 3, 0] {
            passive(&mut game, seat);
        }
    }
}

#[test]
fn street_actions_and_amounts_clear_at_each_street_without_erasing_hand_history() {
    let mut game = table(&[10_000; 3], 0);
    assert!(game.players()[1].round_action.is_some());
    for seat in [0, 1, 2] {
        passive(&mut game, seat);
    }
    for phase in [Phase::Flop, Phase::Turn, Phase::River] {
        assert_eq!(game.phase(), phase);
        for player in game.players() {
            assert_eq!(player.round_action, None, "no action yet on {phase:?}");
            assert_eq!(player.round_bet, 0);
            assert!(
                player.last_action.is_some(),
                "hand-wide modeling evidence is retained"
            );
        }
        passive(&mut game, 1);
        assert_eq!(
            game.players()[1].round_action.unwrap().action,
            poker_engine::ActionLabel::Check
        );
        assert_eq!(game.players()[0].round_action, None);
        game.act(2, Action::Raise { to: 100 }).unwrap();
        assert_eq!(game.players()[2].round_action.unwrap().amount, 100);
        assert_eq!(game.players()[2].round_bet, 100);
        passive(&mut game, 0);
        passive(&mut game, 1);
    }
}

#[test]
fn cumulative_short_all_ins_reopen_only_players_facing_a_full_raise() {
    let mut game = table(&[1_000, 250, 300, 1_000], 0);
    game.act(3, Action::Raise { to: 200 }).unwrap();
    passive(&mut game, 0);
    game.act(1, Action::AllIn).unwrap();
    game.act(2, Action::AllIn).unwrap();
    assert_eq!(game.current_player_idx(), 3);
    assert!(
        game.legal_actions(3).can_raise,
        "two short raises total a full 100"
    );
    game.act(3, Action::Raise { to: 400 }).unwrap();
    assert_eq!(game.current_player_idx(), 0);
    passive(&mut game, 0);
    assert_eq!(game.phase(), Phase::Flop);
    assert_eq!(
        game.current_player_idx(),
        3,
        "both blinds are all-in; UTG is the first live player"
    );
}
