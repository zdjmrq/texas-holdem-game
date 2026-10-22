'use strict';
// Same-machine benchmark of the original JavaScript probability engine
// (reference/texas-holdem-game/js/probability-core.js, v1.5.8).
//
// Purpose: produce reference numbers for docs/benchmarks.md that are measured
// on the same machine and with the same card inputs as the Rust criterion
// benches (crates/poker-core/benches/eval.rs and
// crates/poker-prob/benches/equity.rs).
//
// Hand generation is intentionally bit-compatible with the Rust benches:
// oracle/lib/prng.js xorshift32 (same hashSeed/xorshift32 as
// probability-core) + the same full-deck Fisher-Yates shuffle order.
//
// Output: one JSON document on stdout (machine info, methodology, ns/op and
// ops/s per case). Wall-clock numbers from WSL are indicative only.
//
// Usage: node oracle/bench-js.js

const os = require('node:os');
const { loadProbabilityCore } = require('./lib/load-js');
const { xorshift32 } = require('./lib/prng');

const probability = loadProbabilityCore();

const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const STANDARD_RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SHORT_RANKS = ['6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const VALUES = { '2': 2, '3': 3, '4': 4, '5': 5, '6': 6, '7': 7, '8': 8, '9': 9, '10': 10, J: 11, Q: 12, K: 13, A: 14 };
const SUIT_NAME = { s: 'spades', h: 'hearts', d: 'diamonds', c: 'clubs' };

// Must match crates/poker-core/benches/eval.rs.
const HANDS_PER_BATCH = 10_000;
const HAND_SEED = 'bench-core-eval';

// Must match crates/poker-prob/benches/equity.rs.
const EXACT_CASES = [
    { id: 'standard-river', variant: 'standard', player: ['As', 'Ah'], board: ['Ks', 'Qd', '9c', '7h', '2s'], numOpponents: 1, expectedSamples: 990, expectedMethod: 'exact-heads-up' },
    { id: 'standard-turn', variant: 'standard', player: ['Ah', 'Kh'], board: ['Qh', 'Jh', '2c', '7d'], numOpponents: 1, expectedSamples: 45_540, expectedMethod: 'exact-heads-up' },
    { id: 'shortdeck-river', variant: 'shortdeck', player: ['As', 'Ah'], board: ['Ks', 'Qd', '9c', '7c', '6d'], numOpponents: 1, expectedSamples: 406, expectedMethod: 'exact-heads-up' },
    { id: 'shortdeck-turn', variant: 'shortdeck', player: ['As', 'Ks'], board: ['Qs', 'Js', '6d', '7c'], numOpponents: 1, expectedSamples: 12_180, expectedMethod: 'exact-heads-up' }
];
const MONTE_CARLO_CASES = [
    { id: 'standard-preflop-heads-up', variant: 'standard', player: ['As', 'Ah'], board: [], numOpponents: 1, simulations: 2400, seed: 'bench-mc-std-preflop-heads-up' },
    { id: 'standard-flop-three-way', variant: 'standard', player: ['Qh', 'Qd'], board: ['Ah', 'Kd', '2c'], numOpponents: 2, simulations: 2400, seed: 'bench-mc-std-flop-three-way' },
    { id: 'standard-preflop-nine-way', variant: 'standard', player: ['Js', 'Ts'], board: [], numOpponents: 9, simulations: 2400, seed: 'bench-mc-std-preflop-nine-way' },
    { id: 'shortdeck-preflop-seven-way', variant: 'shortdeck', player: ['As', 'Ad'], board: [], numOpponents: 7, simulations: 2400, seed: 'bench-mc-sd-preflop-seven-way' }
];

function card(token) {
    const suit = SUIT_NAME[token.slice(-1)];
    const rankToken = token.slice(0, -1);
    const rank = rankToken === 'T' ? '10' : rankToken;
    if (!suit || !VALUES[rank]) throw new Error('bad card token: ' + token);
    return { rank, suit, value: VALUES[rank] };
}
function cards(tokens) {
    return tokens.map(card);
}

function createDeck(isShortDeck) {
    const ranks = isShortDeck ? SHORT_RANKS : STANDARD_RANKS;
    const deck = [];
    for (const suit of SUITS) {
        for (const rank of ranks) deck.push({ rank, suit, value: VALUES[rank] });
    }
    return deck;
}

// Identical to poker-core Deck::shuffle: i from len-1 down to 1,
// j = floor(rng() * (i + 1)), swap.
function shuffle(rng, pool) {
    for (let i = pool.length - 1; i > 0; i--) {
        const j = Math.floor(rng() * (i + 1));
        [pool[i], pool[j]] = [pool[j], pool[i]];
    }
    return pool;
}

// Identical to the Rust bench helper: one xorshift32 stream from HAND_SEED,
// a fresh canonical deck cloned and shuffled per hand.
function generateHands(isShortDeck, cardCount) {
    const rng = xorshift32(HAND_SEED);
    const canonical = createDeck(isShortDeck);
    const hands = [];
    for (let hand = 0; hand < HANDS_PER_BATCH; hand++) {
        const pool = canonical.slice();
        shuffle(rng, pool);
        hands.push(pool.slice(0, cardCount));
    }
    return hands;
}

function machineInfo() {
    const cpus = os.cpus();
    const wslVersion = (() => {
        try {
            return require('node:fs').readFileSync('/proc/version', 'utf8').includes('microsoft') ? 'WSL2' : 'native-linux';
        } catch (_) {
            return 'unknown';
        }
    })();
    return {
        host: os.hostname(),
        platform: os.platform(),
        arch: os.arch(),
        osRelease: os.release(),
        distribution: 'Ubuntu 24.04.5 LTS',
        wsl: wslVersion,
        cpuModel: cpus.length > 0 ? cpus[0].model.trim() : 'unknown',
        logicalCpus: cpus.length,
        memoryBytes: os.totalmem(),
        nodeVersion: process.version
    };
}

function percentile(sorted, fraction) {
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
}

// Runs fn() repeatedly until the accumulated elapsed time reaches minTimeMs
// (and at least 5 iterations), returning per-iteration nanosecond statistics.
function measure(fn, options = {}) {
    const warmup = options.warmup ?? 2;
    const minTimeMs = options.minTimeMs ?? 800;
    const maxIterations = options.maxIterations ?? 5000;
    for (let index = 0; index < warmup; index++) fn();
    const targetNs = BigInt(Math.round(minTimeMs * 1e6));
    const times = [];
    let totalNs = 0n;
    while (totalNs < targetNs || times.length < 5) {
        const started = process.hrtime.bigint();
        fn();
        const elapsed = process.hrtime.bigint() - started;
        totalNs += elapsed;
        times.push(Number(elapsed));
        if (times.length >= maxIterations) break;
    }
    times.sort((left, right) => left - right);
    return {
        iterations: times.length,
        totalMs: Number(totalNs) / 1e6,
        meanNs: Number(totalNs) / times.length,
        minNs: times[0],
        p50Ns: percentile(times, 0.5),
        p95Ns: percentile(times, 0.95)
    };
}

function perOp(stats, divisor) {
    const meanNs = stats.meanNs / divisor;
    return {
        meanNs,
        opsPerSec: 1e9 / meanNs,
        minNs: stats.minNs / divisor,
        p50Ns: stats.p50Ns / divisor,
        p95Ns: stats.p95Ns / divisor
    };
}

function benchmarkEvaluate() {
    const rows = [];
    let sink = 0;
    for (const variant of ['standard', 'shortdeck']) {
        const isShortDeck = variant === 'shortdeck';
        for (const cardCount of [5, 6, 7]) {
            const hands = generateHands(isShortDeck, cardCount);
            const batch = () => {
                let score = 0;
                for (const hand of hands) score += probability.evaluate(hand, isShortDeck).score;
                sink += score;
            };
            const result = perOp(measure(batch), HANDS_PER_BATCH);
            rows.push({
                benchmark: 'evaluate/' + variant,
                cards: cardCount,
                handsPerBatch: HANDS_PER_BATCH,
                ...result
            });
        }
    }
    if (sink === Number.MIN_SAFE_INTEGER) throw new Error('unreachable sink guard');
    return rows;
}

function calculateOptions(testCase) {
    return {
        playerCards: cards(testCase.player),
        communityCards: cards(testCase.board),
        numOpponents: testCase.numOpponents,
        isShortDeck: testCase.variant === 'shortdeck',
        simulations: testCase.simulations,
        minSamples: testCase.simulations,
        seed: testCase.seed
    };
}

function benchmarkExact() {
    return EXACT_CASES.map(testCase => {
        const options = {
            playerCards: cards(testCase.player),
            communityCards: cards(testCase.board),
            numOpponents: testCase.numOpponents,
            isShortDeck: testCase.variant === 'shortdeck'
        };
        const result = probability.calculate(options);
        if (result.method !== testCase.expectedMethod) {
            throw new Error(testCase.id + ': expected method ' + testCase.expectedMethod + ', got ' + result.method);
        }
        if (result.samples !== testCase.expectedSamples) {
            throw new Error(testCase.id + ': expected ' + testCase.expectedSamples + ' samples, got ' + result.samples);
        }
        let sink = 0;
        const stats = measure(() => {
            sink += probability.calculate(options).equityProb;
        });
        if (sink < 0) throw new Error('unreachable sink guard');
        return {
            benchmark: 'calculate/exact-heads-up',
            case: testCase.id,
            variant: testCase.variant,
            samples: result.samples,
            ...perOp(stats, 1),
            msPerOp: stats.meanNs / 1e6
        };
    });
}

function benchmarkMonteCarlo() {
    return MONTE_CARLO_CASES.map(testCase => {
        const options = calculateOptions(testCase);
        const first = probability.calculate(options);
        if (first.method !== 'monte-carlo') {
            throw new Error(testCase.id + ': expected monte-carlo, got ' + first.method);
        }
        if (first.samples !== testCase.simulations) {
            throw new Error(testCase.id + ': expected ' + testCase.simulations + ' samples, got ' + first.samples);
        }
        let sink = 0;
        const stats = measure(() => {
            sink += probability.calculate(options).equityProb;
        });
        if (sink < 0) throw new Error('unreachable sink guard');
        return {
            benchmark: 'calculate/monte-carlo',
            case: testCase.id,
            variant: testCase.variant,
            samples: first.samples,
            samplesPerSec: (first.samples * 1e9) / stats.meanNs,
            ...perOp(stats, 1),
            msPerOp: stats.meanNs / 1e6
        };
    });
}

const started = process.hrtime.bigint();
const report = {
    script: 'oracle/bench-js.js',
    oracle: {
        file: 'reference/texas-holdem-game/js/probability-core.js',
        commit: 'e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90',
        appVersion: 'v1.5.8'
    },
    machine: machineInfo(),
    methodology: {
        handsPerBatch: HANDS_PER_BATCH,
        handSeed: HAND_SEED,
        warmupIterations: 2,
        minTimeMs: 800,
        note: 'Same hand inputs and cases as the Rust criterion benchmarks; WSL numbers are indicative only and do not represent Windows performance.',
        rustCommand: 'cargo bench -p poker-core --bench eval && cargo bench -p poker-prob --bench equity'
    },
    evaluate: benchmarkEvaluate(),
    calculate: [...benchmarkExact(), ...benchmarkMonteCarlo()]
};
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
report.elapsedMs = elapsedMs;
process.stdout.write(JSON.stringify(report, null, 2) + '\n');
