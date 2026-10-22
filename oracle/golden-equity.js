'use strict';
// Exports golden equity/probability corpora from the authoritative JS
// implementation (reference/texas-holdem-game/js/probability-core.js,
// commit e0c6c6f8 / v1.5.8). Output is consumed by
// crates/poker-prob/tests/js_golden_prob.rs.
//
// Design notes:
// 1. Only deterministic fields are recorded. The JS result's elapsedMs field is
//    a wall-clock reading, so it is intentionally dropped: re-running this
//    script must produce byte-identical files (the oracle README requires
//    repeatable corpora). Rust measures its own elapsed time separately.
// 2. Monte Carlo rows pass minSamples = effective target sample count (and a
//    large time budget) so probability-core.js can never take its time-budget
//    early exit. For a fixed seed the sample stream and therefore every
//    recorded field is fully deterministic.
// 3. Cards are stored as compact tokens ("As", "Th") and converted to the JS
//    card objects that probability-core.js expects.
// 4. Every Monte Carlo row is calculated twice and the serialized results must
//    be identical; exact rows verify method + the closed-form sample count.
//
// Usage: node oracle/golden-equity.js
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { loadProbabilityCore } = require('./lib/load-js');

const probability = loadProbabilityCore();
const repoRoot = path.resolve(__dirname, '..');
const outDir = path.join(repoRoot, 'tests', 'golden', 'prob');
fs.mkdirSync(outDir, { recursive: true });

const VALUES = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,J:11,Q:12,K:13,A:14 };
const SUIT_NAME = { s: 'spades', h: 'hearts', d: 'diamonds', c: 'clubs' };
const REFERENCE_COMMIT = 'e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90';

function fail(message) {
    throw new Error(message);
}
function assertEqual(actual, expected, message) {
    if (actual !== expected) fail(message + ': expected ' + String(expected) + ', got ' + String(actual));
}

function jsCard(token) {
    const suit = SUIT_NAME[token.slice(-1)];
    const rankToken = token.slice(0, -1);
    const rank = rankToken === 'T' ? '10' : rankToken;
    if (!suit || !VALUES[rank]) fail('bad compact card token: ' + token);
    return { rank, suit, value: VALUES[rank] };
}

// The evaluator's decode() emits a full rank label and suit name; keep exactly
// that shape in the corpus.
function serializeImprovementCard(card) {
    assertEqual(typeof card.rank, 'string', 'improvement card rank');
    return { rank: card.rank, suit: card.suit, value: card.value, quality: card.quality };
}

function serializeImprovements(improvements) {
    if (!improvements || typeof improvements !== 'object') fail('calculate must attach outs');
    const byHandType = {};
    for (const [name, group] of Object.entries(improvements.byHandType)) {
        byHandType[name] = {
            cards: group.cards.map(serializeImprovementCard),
            count: group.count,
            strongCount: group.strongCount,
            thinCount: group.thinCount
        };
    }
    return {
        totalOuts: improvements.totalOuts,
        strongOuts: improvements.strongOuts,
        thinOuts: improvements.thinOuts,
        byHandType,
        note: improvements.note
    };
}

// Drop elapsedMs (nondeterministic) and keep every other JS field, including
// stats.bestHandCounts and the outs report calculate() attaches.
function serializeResult(result) {
    return {
        winProb: result.winProb,
        tieProb: result.tieProb,
        loseProb: result.loseProb,
        equityProb: result.equityProb,
        samples: result.samples,
        method: result.method,
        stats: { bestHandCounts: result.stats.bestHandCounts },
        outs: serializeImprovements(result.outs)
    };
}

// ---------------------------------------------------------------------------
// Exact heads-up (calculate selects it for numOpponents === 1 && board >= 4).
// ---------------------------------------------------------------------------
const EXACT_CASES = [
    {
        id: 'standard-river-royal-board-tie',
        variant: 'standard',
        player: ['2c', '3d'],
        board: ['Ts', 'Js', 'Qs', 'Ks', 'As'],
        numOpponents: 1
    },
    {
        id: 'standard-river-pair-loses-to-board-flush',
        variant: 'standard',
        player: ['As', 'Ah'],
        board: ['Ks', 'Qd', '9c', '7h', '2s'],
        numOpponents: 1
    },
    {
        id: 'standard-turn-flush-draw',
        variant: 'standard',
        player: ['Ah', 'Kh'],
        board: ['Qh', 'Jh', '2c', '7d'],
        numOpponents: 1
    },
    {
        id: 'standard-turn-made-straight',
        variant: 'standard',
        player: ['Ts', '9s'],
        board: ['8h', '7d', '6c', '2s'],
        numOpponents: 1
    },
    {
        id: 'shortdeck-river-broadway',
        variant: 'shortdeck',
        player: ['As', 'Ah'],
        board: ['Ks', 'Qd', '9c', '7c', '6d'],
        numOpponents: 1
    },
    {
        id: 'shortdeck-turn-flush-draw',
        variant: 'shortdeck',
        player: ['As', 'Ks'],
        board: ['Qs', 'Js', '6d', '7c'],
        numOpponents: 1
    }
];

function deckSize(variant) {
    return variant === 'shortdeck' ? 36 : 52;
}

// probability-core exactHeadsUp enumerates every (river, opponent pair) when
// the board has four cards and every opponent pair when the board is complete.
function expectedExactSamples(testCase) {
    const remaining = deckSize(testCase.variant) - testCase.player.length - testCase.board.length;
    if (testCase.board.length === 5) return (remaining * (remaining - 1)) / 2;
    if (testCase.board.length === 4) return (remaining * (remaining - 1) * (remaining - 2)) / 2;
    return fail('exact corpus only covers board lengths 4 and 5');
}

function calculateOptions(testCase) {
    return {
        playerCards: testCase.player.map(jsCard),
        communityCards: testCase.board.map(jsCard),
        numOpponents: testCase.numOpponents,
        isShortDeck: testCase.variant === 'shortdeck'
    };
}

function generateExact() {
    return EXACT_CASES.map(testCase => {
        const result = probability.calculate(calculateOptions(testCase));
        assertEqual(result.method, 'exact-heads-up', testCase.id + ' method');
        assertEqual(result.samples, expectedExactSamples(testCase), testCase.id + ' exhaustive sample count');
        assertEqual(result.outs.note.length > 0, true, testCase.id + ' outs note');
        return {
            id: testCase.id,
            variant: testCase.variant,
            player: testCase.player,
            board: testCase.board,
            numOpponents: testCase.numOpponents,
            result: serializeResult(result)
        };
    });
}

// ---------------------------------------------------------------------------
// Fixed-seed Monte Carlo (calculate routes here unless heads-up turn/river).
// ---------------------------------------------------------------------------
const MONTE_CARLO_CASES = [
    // Standard deck, preflop and postflop.
    { id: 'standard-preflop-aces-heads-up', variant: 'standard', player: ['As', 'Ah'], board: [], numOpponents: 1, simulations: 400, seed: 'golden-std-preflop-aces-heads-up' },
    { id: 'standard-preflop-ak-three-way', variant: 'standard', player: ['As', 'Ks'], board: [], numOpponents: 3, simulations: 500, seed: 'golden-std-preflop-ak-three-way' },
    { id: 'standard-preflop-jts-nine-way', variant: 'standard', player: ['Js', 'Ts'], board: [], numOpponents: 9, simulations: 600, seed: 'golden-std-preflop-jts-nine-way' },
    { id: 'standard-preflop-aces-zero-opponents-clamp', variant: 'standard', player: ['As', 'Ah'], board: [], numOpponents: 0, simulations: 400, seed: 'golden-std-preflop-aces-zero-opponents' },
    { id: 'standard-flop-flush-draw-heads-up', variant: 'standard', player: ['As', 'Ks'], board: ['Qs', '2s', '7d'], numOpponents: 1, simulations: 500, seed: 'golden-std-flop-flush-draw-heads-up' },
    { id: 'standard-flop-queens-three-way', variant: 'standard', player: ['Qh', 'Qd'], board: ['Ah', 'Kd', '2c'], numOpponents: 2, simulations: 600, seed: 'golden-std-flop-queens-three-way' },
    { id: 'standard-preflop-lower-simulation-clamp', variant: 'standard', player: ['As', 'Ah'], board: [], numOpponents: 1, simulations: 10, seed: 'golden-std-preflop-lower-simulation-clamp' },
    { id: 'standard-flop-upper-simulation-clamp', variant: 'standard', player: ['As', 'Ks'], board: ['Qs', '2s', '7d'], numOpponents: 1, simulations: 20000, seed: 'golden-std-flop-upper-simulation-clamp' },
    { id: 'standard-turn-qsjs-three-way', variant: 'standard', player: ['Qs', 'Js'], board: ['Ah', 'Kh', '2c', '7d'], numOpponents: 2, simulations: 600, seed: 'golden-std-turn-qsjs-three-way' },
    { id: 'standard-river-oesd-three-way', variant: 'standard', player: ['9h', '8h'], board: ['Ah', 'Kh', '2c', '3d', '4s'], numOpponents: 2, simulations: 700, seed: 'golden-std-river-oesd-three-way' },
    { id: 'standard-preflop-empty-seed', variant: 'standard', player: ['As', 'Ad'], board: [], numOpponents: 1, simulations: 400, seed: '' },
    { id: 'standard-preflop-unicode-seed', variant: 'standard', player: ['As', 'Kd'], board: [], numOpponents: 2, simulations: 400, seed: '德州扑克-🎲-seed' },
    // Short deck (36 cards), including flush-ranked-above-full-house spots.
    { id: 'shortdeck-preflop-broadway-heads-up', variant: 'shortdeck', player: ['As', 'Ks'], board: [], numOpponents: 1, simulations: 400, seed: 'golden-sd-preflop-broadway-heads-up' },
    { id: 'shortdeck-preflop-aces-eight-way', variant: 'shortdeck', player: ['As', 'Ad'], board: [], numOpponents: 7, simulations: 500, seed: 'golden-sd-preflop-aces-eight-way' },
    { id: 'shortdeck-flop-flush-draw-heads-up', variant: 'shortdeck', player: ['As', 'Ks'], board: ['Qs', '8s', '7d'], numOpponents: 1, simulations: 500, seed: 'golden-sd-flop-flush-draw-heads-up' },
    { id: 'shortdeck-turn-three-way', variant: 'shortdeck', player: ['Ah', 'Kh'], board: ['Qh', 'Jh', '2c', '7d'], numOpponents: 3, simulations: 500, seed: 'golden-sd-turn-three-way' },
    { id: 'shortdeck-river-three-way', variant: 'shortdeck', player: ['As', 'Ah'], board: ['Ks', 'Qd', '9c', '7c', '6d'], numOpponents: 2, simulations: 400, seed: 'golden-sd-river-three-way' }
];

// Mirrors probability-core.js: Math.max(400, Math.min(12000, floor(Number(x) || 2400))).
function effectiveTargetSamples(simulations) {
    const requested = Math.floor(Number(simulations) || 2400);
    return Math.max(400, Math.min(12000, requested));
}

function monteCarloOptions(testCase, targetSamples) {
    return {
        playerCards: testCase.player.map(jsCard),
        communityCards: testCase.board.map(jsCard),
        numOpponents: testCase.numOpponents,
        // minSamples === target disables the time-budget early exit; the sample
        // count then depends only on the seed and the explicit target.
        simulations: testCase.simulations,
        minSamples: targetSamples,
        timeBudgetMs: 2000,
        seed: testCase.seed,
        isShortDeck: testCase.variant === 'shortdeck'
    };
}

function generateMonteCarlo() {
    return MONTE_CARLO_CASES.map(testCase => {
        const targetSamples = effectiveTargetSamples(testCase.simulations);
        const options = monteCarloOptions(testCase, targetSamples);
        const first = probability.calculate(options);
        const second = probability.calculate(options);
        assertEqual(first.method, 'monte-carlo', testCase.id + ' method');
        assertEqual(first.samples, targetSamples, testCase.id + ' deterministic sample count');
        const firstJson = JSON.stringify(serializeResult(first));
        const secondJson = JSON.stringify(serializeResult(second));
        assertEqual(firstJson, secondJson, testCase.id + ' repeated calculation');
        return {
            id: testCase.id,
            variant: testCase.variant,
            player: testCase.player,
            board: testCase.board,
            numOpponents: testCase.numOpponents,
            simulations: testCase.simulations,
            minSamples: targetSamples,
            seed: testCase.seed,
            result: serializeResult(first)
        };
    });
}

// ---------------------------------------------------------------------------
// findImprovements (outs) direct cases. Boards outside 3..4 cards early-return.
// ---------------------------------------------------------------------------
const IMPROVEMENT_CASES = [
    { id: 'standard-preflop-hidden', variant: 'standard', player: ['As', 'Ks'], board: [] },
    { id: 'standard-river-hidden', variant: 'standard', player: ['As', 'Ks'], board: ['Qs', '2s', '7d', '9h', '3c'] },
    { id: 'standard-flop-flush-draw', variant: 'standard', player: ['As', 'Ks'], board: ['Qs', '2s', '7d'] },
    { id: 'standard-flop-trips-kicker-outs', variant: 'standard', player: ['As', 'Ah'], board: ['Ad', '5c', '2s'] },
    { id: 'standard-flop-mixed-suit-groups', variant: 'standard', player: ['As', 'Qh'], board: ['Kd', 'Jc', '5s'] },
    { id: 'standard-turn-flush-draw', variant: 'standard', player: ['Ah', 'Kh'], board: ['Qh', 'Jh', '2c', '7d'] },
    { id: 'standard-turn-pair-kickers', variant: 'standard', player: ['As', 'Ad'], board: ['Kc', '8h', '3s', '2d'] },
    { id: 'standard-turn-quads-kicker-outs', variant: 'standard', player: ['As', 'Ah'], board: ['Ad', 'Ac', '2s', '3d'] },
    { id: 'standard-turn-straight-draw', variant: 'standard', player: ['9h', '8h'], board: ['7s', '6d', '2c', 'Kd'] },
    { id: 'shortdeck-preflop-hidden', variant: 'shortdeck', player: ['As', 'Ks'], board: [] },
    { id: 'shortdeck-flop-flush-draw', variant: 'shortdeck', player: ['As', 'Ks'], board: ['Qs', '8s', '7d'] },
    { id: 'shortdeck-flop-straight-draw', variant: 'shortdeck', player: ['6h', '7h'], board: ['8s', '9s', 'Ad'] },
    { id: 'shortdeck-turn-flush-vs-full-house', variant: 'shortdeck', player: ['Ah', 'Kh'], board: ['Qh', 'Jh', '2c', '7d'] },
    { id: 'shortdeck-turn-made-straight', variant: 'shortdeck', player: ['As', '6h'], board: ['7d', '8c', '9h', 'Kd'] }
];

function generateImprovements() {
    return IMPROVEMENT_CASES.map(testCase => {
        const result = probability.findImprovements(
            testCase.player.map(jsCard),
            testCase.board.map(jsCard),
            testCase.variant === 'shortdeck'
        );
        // Repeat to guard against accidental nondeterminism.
        const repeated = probability.findImprovements(
            testCase.player.map(jsCard),
            testCase.board.map(jsCard),
            testCase.variant === 'shortdeck'
        );
        assertEqual(JSON.stringify(serializeImprovements(result)), JSON.stringify(serializeImprovements(repeated)), testCase.id + ' repeated call');
        return {
            id: testCase.id,
            variant: testCase.variant,
            player: testCase.player,
            board: testCase.board,
            result: serializeImprovements(result)
        };
    });
}

// ---------------------------------------------------------------------------
// Write corpora.
// ---------------------------------------------------------------------------
function writeJsonl(fileName, rows) {
    const text = rows.map(row => JSON.stringify(row)).join('\n') + '\n';
    const file = path.join(outDir, fileName);
    fs.writeFileSync(file, text);
    const hash = crypto.createHash('sha256').update(text).digest('hex');
    return { file: fileName, rows: rows.length, sha256: hash, bytes: Buffer.byteLength(text) };
}

const started = Date.now();
const exactRows = generateExact();
const monteCarloRows = generateMonteCarlo();
const improvementRows = generateImprovements();

const files = [
    writeJsonl('exact-heads-up.jsonl', exactRows),
    writeJsonl('monte-carlo.jsonl', monteCarloRows),
    writeJsonl('improvements.jsonl', improvementRows)
];

const totalMonteCarloSamples = monteCarloRows.reduce((sum, row) => sum + row.result.samples, 0);
const manifest = {
    generator: 'oracle/golden-equity.js',
    authoritativeOracle: 'reference/texas-holdem-game/js/probability-core.js',
    referenceCommit: REFERENCE_COMMIT,
    variants: ['standard', 'shortdeck'],
    omittedFields: ['elapsedMs'],
    notes: [
        'Exact heads-up rows use calculate() with numOpponents = 1 and a board of 4 or 5 cards.',
        'Monte Carlo rows pass minSamples equal to the effective target sample count, disabling the JS time-budget early exit.',
        'All probabilities/counts are deterministic; only elapsedMs was dropped when exporting.'
    ],
    exactSampleTotal: exactRows.reduce((sum, row) => sum + row.result.samples, 0),
    monteCarloSampleTotal: totalMonteCarloSamples,
    files
};
fs.writeFileSync(path.join(outDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

for (const file of files) {
    console.log(file.file + ': ' + file.rows + ' rows, ' + file.bytes + ' bytes, sha256=' + file.sha256);
}
console.log('exact samples:', manifest.exactSampleTotal, 'monte carlo samples:', manifest.monteCarloSampleTotal);
console.log('elapsedMs:', Date.now() - started);
