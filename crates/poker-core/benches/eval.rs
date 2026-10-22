//! Criterion benchmarks for the standard / short-deck hand evaluator and deck
//! shuffling.
//!
//! Hand inputs are generated deterministically (XorShift32Rng + canonical deck
//! shuffle) so `oracle/bench-js.js` can generate the exact same card sets in
//! JavaScript and produce same-machine comparisons.
//!
//! Run: `cargo bench -p poker-core --bench eval` (release profile; use
//! `--offline` when the dependency cache is warm).
//!
//! Each reported throughput element is one evaluated hand; the deck shuffle
//! benchmark reports one shuffle per iteration.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use poker_core::{Card, Deck, Variant, XorShift32Rng, evaluate};

/// Number of deterministic hands per benchmark batch. Must match
/// `oracle/bench-js.js` (`HANDS_PER_BATCH`).
const HANDS_PER_BATCH: usize = 10_000;

/// Card generation seed shared with `oracle/bench-js.js`.
const HAND_SEED: &str = "bench-core-eval";

fn generate_hands(variant: Variant, card_count: usize, hand_count: usize) -> Vec<Vec<Card>> {
    let mut rng = XorShift32Rng::from_seed(HAND_SEED);
    let canonical = Deck::new(variant);
    let mut hands = Vec::with_capacity(hand_count);
    for _ in 0..hand_count {
        let mut deck = canonical.clone();
        deck.shuffle(&mut rng);
        hands.push(deck.cards()[..card_count].to_vec());
    }
    hands
}

fn bench_evaluate(c: &mut Criterion) {
    for (variant_name, variant) in [
        ("standard", Variant::Standard),
        ("shortdeck", Variant::ShortDeck),
    ] {
        for cards in [5usize, 6, 7] {
            let hands = generate_hands(variant, cards, HANDS_PER_BATCH);
            let mut group = c.benchmark_group(format!("eval/{variant_name}"));
            group.throughput(Throughput::Elements(hands.len() as u64));
            group.bench_with_input(BenchmarkId::from_parameter(cards), &hands, |b, hands| {
                b.iter(|| {
                    for hand in hands {
                        black_box(evaluate(black_box(hand), variant));
                    }
                });
            });
            group.finish();
        }
    }
}

fn bench_shuffle(c: &mut Criterion) {
    let mut rng = XorShift32Rng::from_seed("bench-core-shuffle");
    let mut deck = Deck::new(Variant::Standard);
    c.bench_function("deck/shuffle-standard", |b| {
        b.iter(|| {
            deck.shuffle(&mut rng);
            black_box(&deck);
        });
    });
}

// Defaults are tuned for the release-profile baseline. The environment
// overrides keep the slower dev-profile run (used for diagnostic numbers) to a
// few minutes; they do not change the measured code path.
fn bench_config() -> Criterion {
    let sample_size = env_parse("POKER_BENCH_SAMPLE_SIZE", 20usize).max(10);
    let warm_up_ms = env_parse("POKER_BENCH_WARMUP_MS", 300u64);
    let measurement_ms = env_parse("POKER_BENCH_MEASUREMENT_MS", 1500u64).max(100);
    Criterion::default()
        .sample_size(sample_size)
        .warm_up_time(Duration::from_millis(warm_up_ms))
        .measurement_time(Duration::from_millis(measurement_ms))
}

fn env_parse<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

criterion_group! {
    name = benches;
    config = bench_config();
    targets = bench_evaluate, bench_shuffle
}
criterion_main!(benches);
