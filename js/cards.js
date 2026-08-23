/**
 * Card, Deck, and Hand Evaluation Logic
 * Texas Hold'em Poker
 */

const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const SUIT_SYMBOLS = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
const SUIT_COLORS = { spades: 'black', hearts: 'red', diamonds: 'red', clubs: 'black' };
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const RANK_VALUES = { '2': 2, '3': 3, '4': 4, '5': 5, '6': 6, '7': 7, '8': 8, '9': 9, '10': 10, 'J': 11, 'Q': 12, 'K': 13, 'A': 14 };

/** Card class */
class Card {
    constructor(rank, suit) {
        this.rank = rank;
        this.suit = suit;
        this.value = RANK_VALUES[rank];
    }

    get symbol() {
        return SUIT_SYMBOLS[this.suit];
    }

    get color() {
        return SUIT_COLORS[this.suit];
    }

    toString() {
        return this.rank + this.symbol;
    }
}

/** Deck class */
class Deck {
    constructor() {
        this.cards = [];
        for (const suit of SUITS) {
            for (const rank of RANKS) {
                this.cards.push(new Card(rank, suit));
            }
        }
    }

    shuffle() {
        for (let i = this.cards.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [this.cards[i], this.cards[j]] = [this.cards[j], this.cards[i]];
        }
    }

    deal() {
        return this.cards.pop();
    }

    removeCards(toRemove) {
        const removeSet = new Set(toRemove.map(c => `${c.rank}-${c.suit}`));
        this.cards = this.cards.filter(c => !removeSet.has(`${c.rank}-${c.suit}`));
    }

    clone() {
        const d = new Deck();
        d.cards = this.cards.map(c => new Card(c.rank, c.suit));
        return d;
    }
}

/** Hand Evaluation */
const HAND_TYPES = {
    HIGH_CARD: 0,
    ONE_PAIR: 1,
    TWO_PAIR: 2,
    THREE_OF_A_KIND: 3,
    STRAIGHT: 4,
    FLUSH: 5,
    FULL_HOUSE: 6,
    FOUR_OF_A_KIND: 7,
    STRAIGHT_FLUSH: 8,
    ROYAL_FLUSH: 9
};

const HAND_TYPE_NAMES = {
    0: '高牌',
    1: '一对',
    2: '两对',
    3: '三条',
    4: '顺子',
    5: '同花',
    6: '葫芦',
    7: '四条',
    8: '同花顺',
    9: '皇家同花顺'
};

/**
 * Evaluate the best 5-card hand from a set of cards (up to 7)
 * Returns { rank: number (hand type), score: number (for comparison), name: string, cards: Card[] }
 */
function evaluateHand(cards) {
    if (cards.length < 5) {
        // Not enough cards - score based on what we have
        return evaluatePartialHand(cards);
    }

    // Generate all C(cards.length, 5) combinations
    const combos = getCombinations(cards, 5);
    let best = null;

    for (const combo of combos) {
        const result = evaluate5Cards(combo);
        if (!best || result.score > best.score) {
            best = result;
        }
    }

    return best;
}

/** Evaluate exactly 5 cards */
function evaluate5Cards(cards) {
    const values = cards.map(c => c.value).sort((a, b) => b - a);
    const suits = cards.map(c => c.suit);

    // Check flush
    const isFlush = suits.every(s => s === suits[0]);

    // Check straight
    const straightHigh = checkStraight(values);
    const isStraight = straightHigh !== false;

    // Count rank frequencies
    const freq = {};
    for (const v of values) {
        freq[v] = (freq[v] || 0) + 1;
    }
    const counts = Object.entries(freq).map(([v, c]) => ({ value: parseInt(v), count: c }));
    counts.sort((a, b) => b.count - a.count || b.value - a.value);

    // A royal flush is specifically 10-J-Q-K-A.  A-2-3-4-5 is the
    // five-high wheel and must never be scored as an ace-high straight.
    const isRoyal = isStraight && isFlush && straightHigh === 14;

    let rank, score;

    if (isRoyal) {
        rank = HAND_TYPES.ROYAL_FLUSH;
        score = rank * 10**10;
    } else if (isStraight && isFlush) {
        rank = HAND_TYPES.STRAIGHT_FLUSH;
        score = rank * 10**10 + straightHigh * 10**8;
    } else if (counts[0].count === 4) {
        rank = HAND_TYPES.FOUR_OF_A_KIND;
        score = rank * 10**10 + counts[0].value * 10**8 + counts[1].value * 10**6;
    } else if (counts[0].count === 3 && counts[1].count === 2) {
        rank = HAND_TYPES.FULL_HOUSE;
        score = rank * 10**10 + counts[0].value * 10**8 + counts[1].value * 10**6;
    } else if (isFlush) {
        rank = HAND_TYPES.FLUSH;
        score = rank * 10**10;
        for (let i = 0; i < values.length; i++) {
            score += values[i] * 10**(8 - i*2);
        }
    } else if (isStraight) {
        rank = HAND_TYPES.STRAIGHT;
        score = rank * 10**10 + straightHigh * 10**8;
    } else if (counts[0].count === 3) {
        rank = HAND_TYPES.THREE_OF_A_KIND;
        score = rank * 10**10 + counts[0].value * 10**8;
        const kickers = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++) {
            score += kickers[i] * 10**(6 - i*2);
        }
    } else if (counts[0].count === 2 && counts[1].count === 2) {
        rank = HAND_TYPES.TWO_PAIR;
        const pairs = [counts[0].value, counts[1].value].sort((a, b) => b - a);
        score = rank * 10**10 + pairs[0] * 10**8 + pairs[1] * 10**6;
        const kicker = values.filter(v => v !== pairs[0] && v !== pairs[1]);
        score += kicker[0] * 10**4;
    } else if (counts[0].count === 2) {
        rank = HAND_TYPES.ONE_PAIR;
        score = rank * 10**10 + counts[0].value * 10**8;
        const kickers = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++) {
            score += kickers[i] * 10**(6 - i*2);
        }
    } else {
        rank = HAND_TYPES.HIGH_CARD;
        score = rank * 10**10;
        for (let i = 0; i < values.length; i++) {
            score += values[i] * 10**(8 - i*2);
        }
    }

    return {
        rank,
        score,
        name: HAND_TYPE_NAMES[rank],
        cards: cards.slice()
    };
}

/** Evaluate fewer than 5 cards (partial hand for pre-flop evaluation) */
function evaluatePartialHand(cards) {
    const values = cards.map(c => c.value).sort((a, b) => b - a);
    const suits = cards.map(c => c.suit);

    // Count rank frequencies (same as full eval)
    const freq = {};
    for (const v of values) {
        freq[v] = (freq[v] || 0) + 1;
    }
    const counts = Object.entries(freq).map(([v, c]) => ({ value: parseInt(v), count: c }));
    counts.sort((a, b) => b.count - a.count || b.value - a.value);

    // Check flush potential (all same suit)
    const isFlush = suits.every(s => s === suits[0]);

    let rank, score;

    // Detect made hands based on frequencies (works for any number of cards)
    if (counts[0].count === 4) {
        rank = HAND_TYPES.FOUR_OF_A_KIND;
        score = rank * 10**10 + counts[0].value * 10**8;
        if (counts[1]) score += counts[1].value * 10**6;
    } else if (counts[0].count === 3 && counts.length >= 2 && counts[1].count === 2) {
        rank = HAND_TYPES.FULL_HOUSE;
        score = rank * 10**10 + counts[0].value * 10**8 + counts[1].value * 10**6;
    } else if (counts[0].count === 3) {
        rank = HAND_TYPES.THREE_OF_A_KIND;
        score = rank * 10**10 + counts[0].value * 10**8;
        const kickers = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++) {
            score += kickers[i] * 10**(6 - i*2);
        }
    } else if (counts.length >= 2 && counts[0].count === 2 && counts[1].count === 2) {
        rank = HAND_TYPES.TWO_PAIR;
        const pairs = [counts[0].value, counts[1].value].sort((a, b) => b - a);
        score = rank * 10**10 + pairs[0] * 10**8 + pairs[1] * 10**6;
        const kicker = values.filter(v => v !== pairs[0] && v !== pairs[1]);
        if (kicker.length > 0) score += kicker[0] * 10**4;
    } else if (counts[0].count === 2) {
        rank = HAND_TYPES.ONE_PAIR;
        score = rank * 10**10 + counts[0].value * 10**8;
        const kickers = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++) {
            score += kickers[i] * 10**(6 - i*2);
        }
    } else if (cards.length === 2 && isFlush) {
        // Suited high cards pre-flop - small bonus
        rank = HAND_TYPES.HIGH_CARD;
        score = rank * 10**10 + values[0] * 10**8 + values[1] * 10**6 + 10**4;
    } else {
        rank = HAND_TYPES.HIGH_CARD;
        score = rank * 10**10;
        for (let i = 0; i < values.length; i++) {
            score += values[i] * 10**(8 - i*2);
        }
    }

    return {
        rank,
        score,
        name: HAND_TYPE_NAMES[rank],
        cards: cards.slice()
    };
}

/** Return the straight's high card, or false when the values are not a straight. */
function checkStraight(values) {
    const unique = [...new Set(values)].sort((a, b) => b - a);
    if (unique.length < 5) return false;

    // Normal straight
    if (unique[0] - unique[4] === 4) return unique[0];

    // Wheel (A-2-3-4-5)
    if (unique[0] === 14 && unique[1] === 5 && unique[2] === 4 && unique[3] === 3 && unique[4] === 2) {
        return 5;
    }

    return false;
}

/** Get all combinations of size k from array */
function getCombinations(arr, k) {
    if (k === 0) return [[]];
    if (arr.length < k) return [];

    const result = [];
    const first = arr[0];
    const rest = arr.slice(1);

    // Combinations that include first
    const withFirst = getCombinations(rest, k - 1);
    for (const combo of withFirst) {
        result.push([first, ...combo]);
    }

    // Combinations that exclude first
    const withoutFirst = getCombinations(rest, k);
    for (const combo of withoutFirst) {
        result.push(combo);
    }

    return result;
}

/** Compare two evaluated hands. Returns 1 if h1 wins, -1 if h2 wins, 0 if tie */
function compareHands(h1, h2) {
    if (h1.score > h2.score) return 1;
    if (h1.score < h2.score) return -1;
    return 0;
}
