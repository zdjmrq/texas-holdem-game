//! Broad deterministic game walks plus known-answer betting/settlement cases.
use poker_core::{Card, ChaChaAdapter, Deck, RandomSource, Variant};
use poker_engine::{Action, ActionType, Game, GameConfig, Phase, Player};

fn players(stacks: &[u64]) -> Vec<Player> {
    stacks
        .iter()
        .enumerate()
        .map(|(i, &stack)| Player::new(format!("p{i}"), i, format!("P{i}"), stack))
        .collect()
}

fn state(game: &Game) -> String {
    format!(
        "{:?}/{:?}/{:?}/{:?}/{}/{}/{}/{}/{}/{:?}",
        game.phase(),
        game.players(),
        game.community_cards(),
        game.round_bets(),
        game.pot(),
        game.current_bet(),
        game.hand_id(),
        game.turn_id(),
        game.current_player_idx(),
        game.last_hand_result()
    )
}

fn invariants(game: &Game, total: u64) {
    assert_eq!(
        game.players().iter().map(|p| p.stack).sum::<u64>() + game.pot(),
        total,
        "chip conservation: {}",
        state(game)
    );
    let mut mask = 0;
    for card in game
        .players()
        .iter()
        .flat_map(|p| &p.hole_cards)
        .chain(game.community_cards())
    {
        assert_eq!(mask & card.bit(), 0, "duplicate card: {}", state(game));
        mask |= card.bit();
        assert!(!game.is_short_deck() || card.rank_value() >= 6);
    }
    if game.phase().is_active() {
        assert_eq!(
            game.players().iter().map(|p| p.chips_in_pot).sum::<u64>(),
            game.pot()
        );
        let actor = usize::try_from(game.current_player_idx()).expect("active hand has an actor");
        assert!(!game.legal_actions(actor).actions.is_empty());
    } else {
        assert_eq!(game.phase(), Phase::Idle);
        assert_eq!(game.current_player_idx(), -1);
        let result = game.last_hand_result().unwrap();
        assert_eq!(
            result
                .winner_amounts
                .iter()
                .map(|(_, amount)| amount)
                .sum::<u64>(),
            result.pot
        );
        for (id, _) in &result.winner_amounts {
            assert!(game.players().iter().any(|p| &p.id == id && !p.folded));
        }
    }
}

#[test]
fn thousands_of_mixed_action_hands_preserve_cards_chips_and_progress() {
    let mut rng = ChaChaAdapter::from_u64(0x2026_1023);
    for variant in [Variant::Standard, Variant::ShortDeck] {
        let max_players = if variant.is_short_deck() { 8 } else { 10 };
        for count in 2..=max_players {
            for case in 0..200 {
                // Include stacks shorter than blinds/antes and off-grid odd chips.
                let stacks: Vec<_> = (0..count)
                    .map(|_| 1 + u64::from(rng.next_u32() % 2_000))
                    .collect();
                let total = stacks.iter().sum();
                let config = if variant.is_short_deck() {
                    GameConfig::short_deck(2_000, 40, 80)
                } else {
                    GameConfig::standard(2_000, 40, 80)
                };
                let mut game = Game::new(config, players(&stacks));
                assert!(game.start_hand(&mut rng));
                let mut steps = 0;
                loop {
                    invariants(&game, total);
                    if !game.phase().is_active() {
                        break;
                    }
                    steps += 1;
                    assert!(
                        steps < 500,
                        "no progress: {variant:?}/{count}/{case}: {}",
                        state(&game)
                    );
                    let actor = game.current_player_idx() as usize;
                    let legal = game.legal_actions(actor);
                    let before = state(&game);
                    assert!(game.act((actor + 1) % count, Action::AllIn).is_err());
                    assert_eq!(before, state(&game), "rejected action must be atomic");
                    assert!(!game.start_hand(&mut rng), "cannot discard an active pot");
                    assert_eq!(before, state(&game));
                    let choice = legal.actions[(rng.next_u32() as usize) % legal.actions.len()];
                    let action = match choice {
                        ActionType::Fold => Action::Fold,
                        ActionType::Check => Action::Check,
                        ActionType::Call => Action::Call,
                        ActionType::AllIn => Action::AllIn,
                        ActionType::Raise => {
                            let slots =
                                (legal.max_raise_to - legal.min_raise_to) / legal.wager_unit + 1;
                            Action::Raise {
                                to: legal.min_raise_to
                                    + u64::from(rng.next_u32()) % slots * legal.wager_unit,
                            }
                        }
                    };
                    game.act(actor, action).unwrap();
                    assert_ne!(before, state(&game));
                }
            }
        }
    }
}

fn fixed_deck(deal_order: &str) -> Deck {
    let mut cards: Vec<Card> = deal_order
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    cards.reverse();
    Deck::from_cards(cards).unwrap()
}

#[test]
fn short_stack_wins_only_main_pot_and_second_best_wins_side_pot() {
    let mut game = Game::new(
        GameConfig::standard(300, 50, 100),
        players(&[100, 300, 300]),
    );
    // Seat 0 AA, seat 1 KK, seat 2 QQ; board 2 3 7 8 9, with explicit burns.
    let deck = fixed_deck("Ks Qs As Kh Qh Ah Jc 2c 3d 7h Tc 8s 4c 9d");
    assert!(game.start_hand_with_deck(deck, Some(0)));
    game.act(0, Action::AllIn).unwrap();
    game.act(1, Action::AllIn).unwrap();
    game.act(2, Action::Call).unwrap();
    assert_eq!(game.phase(), Phase::Idle);
    assert_eq!(
        game.players().iter().map(|p| p.stack).collect::<Vec<_>>(),
        [300, 400, 0]
    );
    assert_eq!(
        game.last_hand_result().unwrap().winner_amounts,
        [("p0".into(), 300), ("p1".into(), 400)]
    );
}

#[test]
fn tied_board_splits_odd_chip_to_first_winner_left_of_button() {
    let mut game = Game::new(GameConfig::standard(200, 1, 2), players(&[200, 200, 200]));
    let deck = fixed_deck("2c 3c 4c 5d 6d 7d 8c As Ks Qs 9c Js Tc Ts");
    assert!(game.start_hand_with_deck(deck, Some(0)));
    game.act(0, Action::Raise { to: 5 }).unwrap();
    game.act(1, Action::Call).unwrap();
    game.act(2, Action::Call).unwrap();
    game.act(1, Action::Check).unwrap();
    game.act(2, Action::Check).unwrap();
    game.act(0, Action::Raise { to: 2 }).unwrap();
    game.act(1, Action::Fold).unwrap();
    game.act(2, Action::Call).unwrap();
    while game.phase().is_active() {
        let actor = game.current_player_idx() as usize;
        game.act(actor, Action::Check).unwrap();
    }
    // Pot 19, both play the royal flush on the board. Seat 2 gets the odd chip.
    assert_eq!(
        game.players().iter().map(|p| p.stack).collect::<Vec<_>>(),
        [202, 195, 203]
    );
    invariants(&game, 600);
}

#[test]
fn short_all_in_does_not_reopen_raising_for_player_who_already_acted() {
    let mut game = Game::new(
        GameConfig::standard(1_000, 50, 100),
        players(&[1_000, 250, 1_000]),
    );
    assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
    game.act(0, Action::Raise { to: 200 }).unwrap();
    game.act(1, Action::AllIn).unwrap();
    game.act(2, Action::Call).unwrap();
    let legal = game.legal_actions(0);
    assert_eq!(legal.to_call, 50);
    assert_eq!(legal.actions, [ActionType::Fold, ActionType::Call]);
    let before = state(&game);
    assert!(game.act(0, Action::AllIn).is_err());
    assert_eq!(before, state(&game));
    game.act(0, Action::Call).unwrap();
    assert_eq!(game.phase(), Phase::Flop);
}
