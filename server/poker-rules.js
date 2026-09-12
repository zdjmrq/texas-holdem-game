'use strict';

const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const STANDARD_RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SHORT_RANKS = ['6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const VALUES = { '2':2, '3':3, '4':4, '5':5, '6':6, '7':7, '8':8, '9':9, '10':10, J:11, Q:12, K:13, A:14 };

const STANDARD_NAMES = ['高牌', '一对', '两对', '三条', '顺子', '同花', '葫芦', '四条', '同花顺', '皇家同花顺'];
const SHORT_NAMES = ['高牌', '一对', '两对', '三条', '顺子', '葫芦', '同花', '四条', '同花顺', '皇家同花顺'];

function card(rank, suit) {
    return { rank, suit, value: VALUES[rank] };
}

function createDeck(isShortDeck = false) {
    const ranks = isShortDeck ? SHORT_RANKS : STANDARD_RANKS;
    const deck = [];
    for (const suit of SUITS) for (const rank of ranks) deck.push(card(rank, suit));
    return deck;
}

function shuffle(deck, random = Math.random) {
    for (let i = deck.length - 1; i > 0; i--) {
        const j = Math.floor(random() * (i + 1));
        [deck[i], deck[j]] = [deck[j], deck[i]];
    }
    return deck;
}

function combinations(items, size) {
    if (size === 0) return [[]];
    if (items.length < size) return [];
    const output = [];
    for (let i = 0; i <= items.length - size; i++) {
        for (const tail of combinations(items.slice(i + 1), size - 1)) output.push([items[i], ...tail]);
    }
    return output;
}

// Only defined for exactly five ranks: the first/last window test below is a
// correct straight detector for 5 cards, but for 6-7 ranks a straight can sit
// lower in the sorted list (e.g. 8,6,5,4,3,2 contains 6-5-4-3-2) and this
// implementation would miss it. evaluateHand() always passes 5-card
// combinations, so reject anything else loudly instead of answering wrongly.
function straightHigh(values, isShortDeck) {
    if (!Array.isArray(values) || values.length !== 5) return false;
    const unique = [...new Set(values)].sort((a, b) => b - a);
    if (unique.length < 5) return false;
    if (unique[0] - unique[4] === 4) return unique[0];
    if (!isShortDeck && unique.slice(0, 5).join(',') === '14,5,4,3,2') return 5;
    if (isShortDeck && unique.slice(0, 5).join(',') === '14,9,8,7,6') return 9;
    return false;
}

function scoreKickers(rank, orderedValues) {
    let score = rank * 10 ** 10;
    for (let i = 0; i < orderedValues.length; i++) score += orderedValues[i] * 10 ** (8 - i * 2);
    return score;
}

function evaluateFive(cards, isShortDeck = false) {
    const values = cards.map(c => c.value ?? VALUES[c.rank]).sort((a, b) => b - a);
    const isFlush = cards.every(c => c.suit === cards[0].suit);
    const highStraight = straightHigh(values, isShortDeck);
    const counts = new Map();
    for (const value of values) counts.set(value, (counts.get(value) || 0) + 1);
    const groups = [...counts.entries()].map(([value, count]) => ({ value, count }))
        .sort((a, b) => b.count - a.count || b.value - a.value);

    const ranks = isShortDeck
        ? { high:0, pair:1, two:2, trips:3, straight:4, full:5, flush:6, quads:7, sf:8, royal:9 }
        : { high:0, pair:1, two:2, trips:3, straight:4, flush:5, full:6, quads:7, sf:8, royal:9 };
    let rank;
    let score;

    if (isFlush && highStraight === 14) {
        rank = ranks.royal; score = rank * 10 ** 10;
    } else if (isFlush && highStraight !== false) {
        rank = ranks.sf; score = rank * 10 ** 10 + highStraight * 10 ** 8;
    } else if (groups[0].count === 4) {
        rank = ranks.quads; score = scoreKickers(rank, [groups[0].value, groups[1].value]);
    } else if (groups[0].count === 3 && groups[1]?.count === 2) {
        rank = ranks.full; score = scoreKickers(rank, [groups[0].value, groups[1].value]);
    } else if (isFlush) {
        rank = ranks.flush; score = scoreKickers(rank, values);
    } else if (highStraight !== false) {
        rank = ranks.straight; score = rank * 10 ** 10 + highStraight * 10 ** 8;
    } else if (groups[0].count === 3) {
        rank = ranks.trips;
        score = scoreKickers(rank, [groups[0].value, ...values.filter(v => v !== groups[0].value)]);
    } else if (groups[0].count === 2 && groups[1]?.count === 2) {
        rank = ranks.two;
        const pairs = [groups[0].value, groups[1].value].sort((a, b) => b - a);
        score = scoreKickers(rank, [...pairs, values.find(v => !pairs.includes(v))]);
    } else if (groups[0].count === 2) {
        rank = ranks.pair;
        score = scoreKickers(rank, [groups[0].value, ...values.filter(v => v !== groups[0].value)]);
    } else {
        rank = ranks.high; score = scoreKickers(rank, values);
    }

    return { rank, score, name: (isShortDeck ? SHORT_NAMES : STANDARD_NAMES)[rank], cards: cards.slice() };
}

function evaluateHand(cards, isShortDeck = false) {
    if (!Array.isArray(cards) || cards.length < 5) return null;
    let best = null;
    for (const five of combinations(cards, 5)) {
        const result = evaluateFive(five, isShortDeck);
        if (!best || result.score > best.score) best = result;
    }
    return best;
}

module.exports = {
    SUITS,
    STANDARD_RANKS,
    SHORT_RANKS,
    VALUES,
    STANDARD_NAMES,
    SHORT_NAMES,
    card,
    createDeck,
    shuffle,
    straightHigh,
    evaluateFive,
    evaluateHand
};
