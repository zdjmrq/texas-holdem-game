//! Shared host adapter: captures public evidence before the engine advances streets.
/// Apply an action and return public observation metadata, never private cards.
pub fn act(
    game: &mut poker_engine::Game,
    seat: usize,
    action: poker_engine::Action,
) -> Result<crate::input::ObservedAction, poker_engine::ActError> {
    let street = crate::input::Street::from_name(game.phase().as_str());
    let hand_id = game.hand_id();
    let board = game.community_cards().to_vec();
    let (position, count) = game.hand_position_info(seat);
    let sb = game.sb_idx() == seat as i32;
    let bb = game.bb_idx() == seat as i32;
    let meta = game.act(seat, action)?;
    let street = street.unwrap_or(crate::input::Street::Preflop);
    {
        let event = crate::input::ObservedAction {
            seat_id: seat,
            action: action.action_type().as_str().to_owned(),
            street,
            hand_id,
            meta: crate::input::ObservedActionMeta {
                to_call_before: meta.to_call_before as f64,
                additional_paid: meta.additional_paid as f64,
                raise_to: meta.raise_to as f64,
                current_bet_before: meta.current_bet_before as f64,
                pot_before: meta.pot_before as f64,
                pot_after: meta.pot_after as f64,
                minimum_bet: meta.minimum_bet as f64,
                aggressive: meta.aggressive,
                bet_fraction: meta.bet_fraction,
                preflop_raise_count_before: meta.preflop_raise_count_before,
                position_from_button: Some(position as i32),
                player_count: Some(count as usize),
                is_small_blind: sb,
                is_big_blind: bb,
                community_cards: board,
                is_cbet_opportunity: meta.is_cbet_opportunity,
                faced_cbet: meta.faced_cbet,
                faced_3bet: meta.faced_3bet,
            },
        };
        Ok(event)
    }
}

/// Publicly revealed showdown information; fold wins reveal no private cards.
pub fn showdown(game: &poker_engine::Game) -> Vec<(usize, crate::ShowdownResult)> {
    let Some(result) = game.last_hand_result() else {
        return Vec::new();
    };
    result
        .results
        .iter()
        .filter(|p| !p.cards.is_empty())
        .filter_map(|p| {
            let index = game
                .players()
                .iter()
                .position(|player| player.id == p.identity.id)?;
            let won = result
                .winner_amounts
                .iter()
                .any(|(id, amount)| *id == p.identity.id && *amount > 0);
            let aggressive = game.players()[index].last_action.is_some_and(|a| {
                matches!(
                    a.action,
                    poker_engine::ActionLabel::Raise | poker_engine::ActionLabel::AllIn
                )
            });
            Some((
                index,
                crate::ShowdownResult {
                    won,
                    hand_rank: p.hand_rank,
                    was_bluff: aggressive && !won && p.hand_rank <= 2,
                    was_trap: !aggressive && won && p.hand_rank >= 4,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use poker_core::Deck;
    use poker_engine::{Action, Game, GameConfig, Player};

    #[test]
    fn showdown_routes_sparse_seat_ids_by_player_identity() {
        let players = vec![
            Player::new("a", 8, "A", 100),
            Player::new("b", 42, "B", 100),
        ];
        let mut game = Game::new(GameConfig::standard(100, 50, 100), players);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        game.act(0, Action::AllIn).unwrap();
        let observations = super::showdown(&game);
        assert_eq!(
            observations
                .iter()
                .map(|(seat, _)| *seat)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn fold_win_does_not_invent_private_showdown_evidence() {
        let players = vec![
            Player::new("a", 0, "A", 1000),
            Player::new("b", 1, "B", 1000),
        ];
        let mut game = Game::new(GameConfig::standard(1000, 50, 100), players);
        assert!(game.start_hand_with_deck(Deck::standard(), Some(0)));
        super::act(&mut game, 0, Action::Fold).unwrap();
        assert!(super::showdown(&game).is_empty());
    }
}
