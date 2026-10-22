//! Exercise the same configuration/session path used by the normal Start button.
use poker_app::{
    ai::DeterministicAi,
    session::{TableConfig, TableSession, autoplay_hand},
    settings::Settings,
};
use std::collections::HashSet;

fn table(config: &TableConfig) -> TableSession {
    let mut table = TableSession::new(config, "玩家", Box::new(DeterministicAi));
    assert!(table.start());
    table
}

fn deal(table: &TableSession) -> Vec<u8> {
    table
        .game()
        .players()
        .iter()
        .flat_map(|p| p.hole_cards.iter().map(|c| c.index()))
        .collect()
}

#[test]
fn repeated_normal_starts_and_restarts_get_independent_deals() {
    let config = TableConfig::from_settings(&Settings::default(), None);
    let deals: HashSet<_> = (0..128).map(|_| deal(&table(&config))).collect();
    assert_eq!(
        deals.len(),
        128,
        "normal Start must not replay a fixed test seed"
    );
}

#[test]
fn explicit_seed_replays_but_next_hand_advances_the_deck_stream() {
    let settings = Settings {
        players: 3,
        ..Settings::default()
    };
    let config = TableConfig::from_settings(&settings, Some("reproducible-game".into()));
    let mut first = table(&config);
    let mut replay = table(&config);
    let before = deal(&first);
    assert_eq!(before, deal(&replay));
    assert!(!first.next_hand(), "next hand cannot skip a live hand");
    assert!(!first.start(), "double start cannot reset the live pot");
    assert_eq!(before, deal(&first));
    assert_eq!(first.game().hand_id(), 1);
    for _ in 0..8 {
        assert!(autoplay_hand(&mut first));
        assert!(autoplay_hand(&mut replay));
        let settled_chips: Vec<_> = first.game().players().iter().map(|p| p.stack).collect();
        assert!(
            !first.start(),
            "start is only for the first hand, even after settlement"
        );
        assert_eq!(
            settled_chips,
            first
                .game()
                .players()
                .iter()
                .map(|p| p.stack)
                .collect::<Vec<_>>()
        );
        if first.game_over().is_some() {
            break;
        }
        assert!(first.next_hand());
        assert!(replay.next_hand());
        assert_ne!(before, deal(&first));
        assert_eq!(deal(&first), deal(&replay));
    }
}
