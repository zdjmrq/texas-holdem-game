'use strict';
// Verifies that all hand evaluators in the v1.5.8 code base agree, so the
// golden corpus has a single authoritative oracle.
//
// 1. Exhaustive: every 5-card combination, standard (2,598,960) and short deck
//    (376,992), comparing rank+score of four implementations.
// 2. Sampled: random 6- and 7-card hands (best-of-five selection paths).
// 3. Partial hands (<5 cards): documents known intentional differences
//    (cards.js/shortdeck.js have a suited preflop bonus that probability-core
//    does not, and server/poker-rules returns null by design).
const { mulberry32 } = require('./lib/prng');
const { loadBrowserModules, loadProbabilityCore, loadServerRules } = require('./lib/load-js');

const browser = loadBrowserModules().modules;
const probability = loadProbabilityCore();
const serverRules = loadServerRules();

const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const STD_RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
const SHORT_RANKS = ['6','7','8','9','10','J','Q','K','A'];
const VALUES = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,J:11,Q:12,K:13,A:14 };

function deckOf(shortDeck) {
    const ranks = shortDeck ? SHORT_RANKS : STD_RANKS;
    const deck = [];
    for (const suit of SUITS) for (const rank of ranks) deck.push({ rank, suit, value: VALUES[rank] });
    return deck;
}

function key(hand) {
    return hand.map(c => c.rank + '|' + c.suit).join(' ');
}

function combos(arr, k, visit) {
    const idx = new Array(k);
    (function rec(start, depth) {
        if (depth === k) {
            const sel = new Array(k);
            for (let i = 0; i < k; i++) sel[i] = arr[idx[i]];
            visit(sel);
            return;
        }
        for (let i = start; i <= arr.length - (k - depth); i++) {
            idx[depth] = i;
            rec(i + 1, depth + 1);
        }
    })(0, 0);
}

function mismatch(store, where, hand, a, b, labelA, labelB) {
    if (store.length >= 25) return;
    store.push({ where, hand: key(hand), [labelA]: { rank: a && a.rank, score: a && a.score }, [labelB]: { rank: b && b.rank, score: b && b.score } });
}

function exhaustive5(shortDeck) {
    const deck = deckOf(shortDeck);
    const st = { where: shortDeck ? 'shortdeck-5card' : 'standard-5card', total: 0, vsServer: 0, vsLegacy: 0, examples: [] };
    combos(deck, 5, (hand) => {
        const p = probability.evaluate(hand, shortDeck);
        const s = serverRules.evaluateHand(hand, shortDeck);
        const legacy = shortDeck ? browser.evaluateSDHand(hand) : browser.evaluateHand(hand);
        st.total++;
        if (p.rank !== s.rank || p.score !== s.score) { st.vsServer++; mismatch(st.examples, 'vs-server', hand, p, s, 'probability', 'server'); }
        if (p.rank !== legacy.rank || p.score !== legacy.score) { st.vsLegacy++; mismatch(st.examples, 'vs-legacy', hand, p, legacy, 'probability', 'legacy'); }
    });
    return st;
}

function sampledN(shortDeck, n, count, seed) {
    const deck = deckOf(shortDeck);
    const rng = mulberry32(seed);
    const st = { where: (shortDeck ? 'shortdeck-' : 'standard-') + n + 'card', total: 0, vsServer: 0, vsLegacy: 0, examples: [] };
    for (let i = 0; i < count; i++) {
        const copy = deck.slice();
        for (let j = 0; j < n; j++) {
            const k = j + Math.floor(rng() * (copy.length - j));
            [copy[j], copy[k]] = [copy[k], copy[j]];
        }
        const hand = copy.slice(0, n);
        const p = probability.evaluate(hand, shortDeck);
        const legacy = shortDeck ? browser.evaluateSDHand(hand) : browser.evaluateHand(hand);
        const s = n >= 5 ? serverRules.evaluateHand(hand, shortDeck) : null;
        st.total++;
        if (n >= 5 && (p.rank !== s.rank || p.score !== s.score)) { st.vsServer++; mismatch(st.examples, 'vs-server', hand, p, s, 'probability', 'server'); }
        if (p.rank !== legacy.rank || p.score !== legacy.score) { st.vsLegacy++; mismatch(st.examples, 'vs-legacy', hand, p, legacy, 'probability', 'legacy'); }
    }
    return st;
}

function partialComparison(shortDeck, count, seed) {
    const deck = deckOf(shortDeck);
    const rng = mulberry32(seed);
    const out = {};
    for (const n of [2, 3, 4]) {
        const copy = deck.slice();
        let differ = 0; let equal = 0; const examples = [];
        for (let i = 0; i < count; i++) {
            const pool = copy.slice();
            const hand = [];
            for (let j = 0; j < n; j++) hand.push(pool[Math.floor(rng() * pool.length)]);
            // ensure distinct
            const uniq = [...new Map(hand.map(c => [c.rank + c.suit, c])).values()];
            if (uniq.length < n) { i--; continue; }
            const p = probability.evaluate(uniq, shortDeck);
            const legacy = shortDeck ? browser.evaluateSDHand(uniq) : browser.evaluateHand(uniq);
            if (p.rank === legacy.rank && p.score === legacy.score) equal++;
            else { differ++; if (examples.length < 8) examples.push({ hand: key(uniq), probability: { rank: p.rank, score: p.score }, legacy: { rank: legacy.rank, score: legacy.score } }); }
        }
        out['cards-' + n] = { equal, differ, examples };
    }
    return out;
}

const started = Date.now();
const report = {
    generatedAt: new Date().toISOString(),
    reference: require('./lib/load-js').referenceRepo(),
    exhaustive5: [exhaustive5(false), exhaustive5(true)],
    sampled: [
        sampledN(false, 6, 20000, 0x51a7e1), sampledN(false, 7, 20000, 0x51a7e2),
        sampledN(true, 6, 20000, 0x51a7e3), sampledN(true, 7, 20000, 0x51a7e4)
    ],
    partial: { standard: partialComparison(false, 3000, 0x9e37), shortdeck: partialComparison(true, 3000, 0x9e38) }
};
report.elapsedMs = Date.now() - started;
console.log(JSON.stringify(report, null, 2));
