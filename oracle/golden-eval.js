'use strict';
// Exports golden hand-evaluation corpora from the authoritative JS evaluator
// (js/probability-core.js evaluate), plus exhaustive five-card hashes for both
// variants. Output is consumed by crates/poker-core/tests/js_golden_eval.rs.
//
// Usage: node oracle/golden-eval.js
const fs = require('node:fs');
const path = require('node:path');
const { mulberry32 } = require('./lib/prng');
const { loadProbabilityCore } = require('./lib/load-js');

const probability = loadProbabilityCore();
const repoRoot = path.resolve(__dirname, '..');
const outDir = path.join(repoRoot, 'tests', 'golden', 'eval');
fs.mkdirSync(outDir, { recursive: true });

const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const SUIT_CODE = { spades: 's', hearts: 'h', diamonds: 'd', clubs: 'c' };
const STD_RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
const SHORT_RANKS = ['6','7','8','9','10','J','Q','K','A'];
const VALUES = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,J:11,Q:12,K:13,A:14 };
const RANK_TOKEN = { 2:'2',3:'3',4:'4',5:'5',6:'6',7:'7',8:'8',9:'9',10:'T',11:'J',12:'Q',13:'K',14:'A' };

function deck(variant) {
    const ranks = variant === 'shortdeck' ? SHORT_RANKS : STD_RANKS;
    const cards = [];
    for (const suit of SUITS) for (const rank of ranks) cards.push({ rank, suit, value: VALUES[rank] });
    return cards;
}

function label(card) {
    return RANK_TOKEN[VALUES[card.rank]] + SUIT_CODE[card.suit];
}

function record(variant, cards) {
    const result = probability.evaluate(cards, variant === 'shortdeck');
    return { cards: cards.map(label), rank: result.rank, score: result.score, name: result.name };
}

function sampleHand(rng, pool, count) {
    const copy = pool.slice();
    for (let i = 0; i < count; i++) {
        const j = i + Math.floor(rng() * (copy.length - i));
        [copy[i], copy[j]] = [copy[j], copy[i]];
    }
    return copy.slice(0, count);
}

const NON_ASCII_NAMES = new Set(['高牌','一对','两对','三条','顺子','同花','葫芦','四条','同花顺','皇家同花顺']);

function curated() {
    const std = [
        // Royal flushes (all four suits).
        ['As','Ks','Qs','Js','Ts'], ['Ah','Kh','Qh','Jh','Th'],
        ['Ad','Kd','Qd','Jd','Td'], ['Ac','Kc','Qc','Jc','Tc'],
        // Straight flushes, including steel wheel and Broadway-adjacent.
        ['9h','8h','7h','6h','5h'], ['5c','4c','3c','2c','Ac'],
        ['Ks','Qs','Js','Ts','9s'],
        // Quads with kickers, including seven-card quads.
        ['As','Ah','Ad','Ac','Kd'], ['2s','2h','2d','2c','3h'],
        ['As','Ah','Ad','Ac','Kd','Qd','Jd'],
        // Full houses: trips + pair, two trips, split-pot style ties.
        ['Ks','Kh','Kd','Qs','Qh'], ['As','Ah','Ad','2s','2h'],
        ['As','Ah','Ad','Ks','Kh','Kd','2c'],
        // Flushes: five and seven cards, ace-high and six-high.
        ['As','Qs','Ts','8s','6s'], ['As','Qs','Ts','8s','6s','4s','2s'],
        ['2h','3h','4h','5h','7h'], ['Jd','9d','7d','5d','3d','2d','Ad'],
        // Straights: standard wheel and top straight; seven-card two-way hands.
        ['As','2h','3d','4c','5h'], ['Ts','9h','8d','7c','6h'],
        ['As','Kh','Qd','Jc','Th','9s','8s'],
        // Trips, two pair, one pair, high card with kickers.
        ['As','Ah','Ad','Kd','Qc'], ['As','Ah','Kd','Kc','Qc'],
        ['As','Ah','Kd','Qc','Jc'], ['As','Kh','Qd','Jc','9c'],
        // Seven cards where board/hole combinations matter.
        ['As','Ah','Kd','Qc','Jc','Ts','9s'],
        ['2s','3s','4s','5s','6s','7h','8h']
    ];
    // The short-deck cards need ranks 6..A only; keep the same hand shapes.
    const sd = [
        ['As','Ks','Qs','Js','Ts'], ['Ah','Kh','Qh','Jh','Th'],
        ['Kd','Qd','Jd','Td','9d'],
        ['Th','9h','8h','7h','6h'], ['Ac','6c','7c','8c','9c'],
        ['Ts','9s','8s','7s','6s'], ['As','Ks','Qs','Js','9s'],
        ['As','Ah','Ad','Ac','Kd'], ['6s','6h','6d','6c','7h'],
        ['As','Ah','Ad','Ac','Kd','Qd','Jd'],
        ['Ks','Kh','Kd','Qs','Qh'], ['As','Ah','Ad','9s','9h'],
        ['As','Ah','Ad','Ks','Kh','Kd','9c'],
        ['As','Qs','Ts','8s','6s'], ['As','Qs','Ts','8s','6s','7s','6h'],
        ['Ah','Kh','Qh','Jh','9h'], ['Jd','9d','8d','7d','6d','Td','Ad'],
        ['As','6h','7d','8c','9h'], ['Ts','9h','8d','7c','6h'],
        ['As','Kh','Qd','Jc','Th','9s','8d'],
        ['As','Ah','Ad','Kd','Qc'], ['As','Ah','Kd','Kc','Qc'],
        ['As','Ah','Kd','Qc','Jc'], ['As','Kh','Qd','Jc','9c'],
        ['As','Ah','Kd','Qc','Jc','Ts','9s'],
        ['6s','7s','8s','9s','Ts','Jh','Qh']
    ];
    return { standard: std, shortdeck: sd };
}

function casesForVariant(variant, seed) {
    const pool = deck(variant);
    const rng = mulberry32(seed);
    const rows = [];
    for (const hand of curated()[variant]) rows.push(record(variant, hand.map(codeToCard)));
    for (const count of [5, 6, 7]) {
        for (let i = 0; i < 2000; i++) rows.push(record(variant, sampleHand(rng, pool, count)));
    }
    for (const count of [2, 3, 4]) {
        for (let i = 0; i < 500; i++) rows.push(record(variant, sampleHand(rng, pool, count)));
    }
    return rows;
}

function codeToCard(text) {
    const suitChar = text.slice(-1);
    const rankToken = text.slice(0, -1);
    const suit = Object.keys(SUIT_CODE).find(key => SUIT_CODE[key] === suitChar);
    return { rank: rankToken === 'T' ? '10' : rankToken, suit, value: VALUES[rankToken === 'T' ? '10' : rankToken] };
}

const FNV_OFFSET = 14695981039346656037n;
const FNV_PRIME = 1099511628211n;
const MASK64 = (1n << 64n) - 1n;

function exhaustiveHash(variant) {
    const pool = deck(variant);
    let hash = FNV_OFFSET;
    const histogram = new Array(10).fill(0);
    let combinations = 0;
    const n = pool.length;
    const hand = new Array(5);
    for (let a = 0; a < n - 4; a++) {
        hand[0] = pool[a];
        for (let b = a + 1; b < n - 3; b++) {
            hand[1] = pool[b];
            for (let c = b + 1; c < n - 2; c++) {
                hand[2] = pool[c];
                for (let d = c + 1; d < n - 1; d++) {
                    hand[3] = pool[d];
                    for (let e = d + 1; e < n; e++) {
                        hand[4] = pool[e];
                        const result = probability.evaluate(hand, variant === 'shortdeck');
                        hash ^= BigInt(result.rank);
                        hash = (hash * FNV_PRIME) & MASK64;
                        hash ^= BigInt(result.score);
                        hash = (hash * FNV_PRIME) & MASK64;
                        histogram[result.rank]++;
                        combinations++;
                    }
                }
            }
        }
    }
    return { variant, cardsPerHand: 5, combinations, scoreHash: hash.toString(), rankHistogram: histogram };
}

const started = Date.now();
const standard = casesForVariant('standard', 0x5e1ec7ed);
const shortdeck = casesForVariant('shortdeck', 0x5e1ec7ee);
for (const [name, rows] of [['standard', standard], ['shortdeck', shortdeck]]) {
    const file = path.join(outDir, name + '-cases.jsonl');
    fs.writeFileSync(file, rows.map(row => JSON.stringify(row)).join('\n') + '\n');
    console.log(name, 'cases:', rows.length, '->', file);
}
for (const variant of ['standard', 'shortdeck']) {
    const hash = exhaustiveHash(variant);
    const file = path.join(outDir, variant + '-five-card-hash.json');
    fs.writeFileSync(file, JSON.stringify(hash) + '\n');
    console.log(variant, 'combinations:', hash.combinations, 'histogram:', JSON.stringify(hash.rankHistogram));
}
console.log('elapsedMs:', Date.now() - started);
