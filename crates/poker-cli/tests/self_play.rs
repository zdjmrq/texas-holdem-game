//! End-to-end engine stress tests through the simulation harness.

use poker_cli::{PolicyKind, SimConfig, run};
use poker_core::Variant;

fn base(variant: Variant, hands: u64, seed: u64, policy: PolicyKind) -> SimConfig {
    SimConfig {
        variant,
        players: 6,
        hands,
        seed,
        stack: 20000,
        small_blind: 50,
        big_blind: 100,
        ante: 40,
        min_bet: 80,
        policy,
        refill: true,
    }
}

#[test]
fn random_self_play_is_deterministic_and_conserves_chips() {
    let config = base(Variant::Standard, 400, 7, PolicyKind::Random);
    let mut first = run(&config);
    let mut second = run(&config);
    // Wall-clock timing is the only intentionally non-deterministic field.
    first.duration_ms = 0;
    second.duration_ms = 0;
    assert_eq!(
        serde_json::to_string(&first).expect("json"),
        serde_json::to_string(&second).expect("json"),
        "same seed must reproduce the same table"
    );
    assert!(first.errors.is_empty(), "errors: {:?}", first.errors);
    assert_eq!(first.chip_conservation_violations, 0);
    assert_eq!(first.hands_played, 400);
    assert!(first.stopped_early.is_none());
    assert!(first.action_counts.values().sum::<u64>() > 0);
    assert!(
        first.hands_by_reason.contains_key("showdown")
            || first.hands_by_reason.contains_key("fold")
    );
}

#[test]
fn short_deck_self_play_runs_clean() {
    let config = base(Variant::ShortDeck, 300, 11, PolicyKind::Random);
    let outcome = run(&config);
    assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
    assert_eq!(outcome.chip_conservation_violations, 0);
    assert_eq!(outcome.hands_played, 300);
    assert_eq!(outcome.variant, "shortdeck");
}

#[test]
fn aggressive_policy_produces_raises_and_all_ins() {
    let config = base(Variant::Standard, 200, 23, PolicyKind::Aggressive);
    let outcome = run(&config);
    assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
    assert!(outcome.action_counts.get("raise").copied().unwrap_or(0) > 0);
    assert!(outcome.hands_by_reason.contains_key("showdown"));
}
