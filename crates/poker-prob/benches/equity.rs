//! Criterion benchmarks for exact heads-up enumeration and fixed-seed Monte
//! Carlo equity.
//!
//! The cases (hole cards, board, opponent count, seed, simulations) are shared
//! with `oracle/bench-js.js` so the JavaScript reference implementation can be
//! timed on the same machine with the same inputs.
//!
//! Run: `cargo bench -p poker-prob --bench equity` (release profile; add
//! `--offline` when the dependency cache is warm).
//!
//! Throughput is reported in showdown samples per second (exact: enumerated
//! showdowns; Monte Carlo: target sample count).

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use poker_core::{Card, Variant};
use poker_prob::{MonteCarloOptions, calculate};

/// `(id, variant, hole cards, board, opponents, enumerated showdown count)`.
const EXACT_CASES: [(&str, Variant, &str, &str, usize, u64); 4] = [
    (
        "standard-river",
        Variant::Standard,
        "As Ah",
        "Ks Qd 9c 7h 2s",
        1,
        990,
    ),
    (
        "standard-turn",
        Variant::Standard,
        "Ah Kh",
        "Qh Jh 2c 7d",
        1,
        45_540,
    ),
    (
        "shortdeck-river",
        Variant::ShortDeck,
        "As Ah",
        "Ks Qd 9c 7c 6d",
        1,
        406,
    ),
    (
        "shortdeck-turn",
        Variant::ShortDeck,
        "As Ks",
        "Qs Js 6d 7c",
        1,
        12_180,
    ),
];

/// `(id, variant, hole cards, board, opponents, seed, simulations)`.
/// 2400 samples is the JS default and keeps all cases in the millisecond
/// range.
const MONTE_CARLO_CASES: [(&str, Variant, &str, &str, usize, &str, u32); 4] = [
    (
        "standard-preflop-heads-up",
        Variant::Standard,
        "As Ah",
        "",
        1,
        "bench-mc-std-preflop-heads-up",
        2400,
    ),
    (
        "standard-flop-three-way",
        Variant::Standard,
        "Qh Qd",
        "Ah Kd 2c",
        2,
        "bench-mc-std-flop-three-way",
        2400,
    ),
    (
        "standard-preflop-nine-way",
        Variant::Standard,
        "Js Ts",
        "",
        9,
        "bench-mc-std-preflop-nine-way",
        2400,
    ),
    (
        "shortdeck-preflop-seven-way",
        Variant::ShortDeck,
        "As Ad",
        "",
        7,
        "bench-mc-sd-preflop-seven-way",
        2400,
    ),
];

fn parse_cards(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|token| token.parse().expect("card"))
        .collect()
}

fn bench_exact(c: &mut Criterion) {
    let mut group = c.benchmark_group("prob/exact-heads-up");
    for (id, variant, player, board, opponents, samples) in EXACT_CASES {
        let player = parse_cards(player);
        let board = parse_cards(board);
        // The JS reference only exposes calculate(); bench the same entry point.
        let options = MonteCarloOptions::default();
        group.throughput(Throughput::Elements(samples));
        group.bench_function(id, |b| {
            b.iter(|| {
                let result = calculate(
                    black_box(&player),
                    black_box(&board),
                    opponents,
                    variant,
                    &options,
                )
                .expect("exact calculation");
                black_box(result);
            });
        });
    }
    group.finish();
}

fn bench_monte_carlo(c: &mut Criterion) {
    let mut group = c.benchmark_group("prob/monte-carlo");
    for (id, variant, player, board, opponents, seed, simulations) in MONTE_CARLO_CASES {
        let player = parse_cards(player);
        let board = parse_cards(board);
        let options = MonteCarloOptions {
            seed: seed.to_string(),
            simulations,
            min_samples: simulations,
        };
        group.throughput(Throughput::Elements(options.target_samples()));
        group.bench_function(id, |b| {
            b.iter(|| {
                let result = calculate(
                    black_box(&player),
                    black_box(&board),
                    opponents,
                    variant,
                    &options,
                )
                .expect("monte carlo calculation");
                black_box(result);
            });
        });
    }
    group.finish();
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
    targets = bench_exact, bench_monte_carlo
}
criterion_main!(benches);
