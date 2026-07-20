/**
 * Card, Deck, and Hand Evaluation Logic
 */
const SUITS = ['spades', 'hearts', 'diamonds', 'clubs'];
const SUIT_SYMBOLS = { spades: '♠', hearts: '♥', diamonds: '♦', clubs: '♣' };
const SUIT_COLORS = { spades: 'black', hearts: 'red', diamonds: 'red', clubs: 'black' };
const RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
const RANK_VALUES = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,'J':11,'Q':12,'K':13,'A':14 };

class Card {
    constructor(rank, suit) {
        this.rank = rank;
        this.suit = suit;
        this.value = RANK_VALUES[rank];
    }
    get symbol() { return SUIT_SYMBOLS[this.suit]; }
    get color() { return SUIT_COLORS[this.suit]; }
    toString() { return this.rank + this.symbol; }
}

class Deck {
    constructor(ranks) {
        this.cards = [];
        const r = ranks || RANKS;
        for (const s of SUITS) {
            for (const rank of r) {
                this.cards.push(new Card(rank, s));
            }
        }
    }
    shuffle() {
        for (let i = this.cards.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [this.cards[i], this.cards[j]] = [this.cards[j], this.cards[i]];
        }
    }
    deal() { return this.cards.pop(); }
}

const HAND_NAMES = {
    9: '皇家同花顺', 8: '同花顺', 7: '四条', 6: '葫芦',
    5: '同花', 4: '顺子', 3: '三条', 2: '两对', 1: '一对', 0: '高牌'
};

function evaluateHand(cards) {
    const ranks = cards.map(c => c.value).sort((a,b) => b-a);
    const suits = cards.map(c => c.suit);
    const isFlush = suits.every(s => s === suits[0]);
    const rankCounts = {};
    for (const r of ranks) rankCounts[r] = (rankCounts[r] || 0) + 1;
    const groups = Object.entries(rankCounts).sort((a,b) => b[1]-a[1] || b[0]-a[0]);
    const unique = [...new Set(ranks)].sort((a,b) => b-a);
    let isStraight = false, straightHigh = 0;
    if (unique.length >= 5) {
        for (let i = 0; i <= unique.length - 5; i++) {
            if (unique[i] - unique[i+4] === 4) {
                isStraight = true;
                straightHigh = unique[i];
                break;
            }
        }
        if (!isStraight && unique[0] === 14 && unique[unique.length-1] === 2) {
            const low = [...unique.filter(v => v <= 5), 1].sort((a,b) => b-a);
            for (let i = 0; i <= low.length - 5; i++) {
                if (low[i] - low[i+4] === 4) {
                    isStraight = true;
                    straightHigh = low[i];
                    break;
                }
            }
        }
    }
    if (isFlush && isStraight && straightHigh === 14) return { rank: 9, name: '皇家同花顺', value: 9e10 };
    if (isFlush && isStraight) return { rank: 8, name: '同花顺', value: 8e10 + straightHigh * 1e8 };
    if (groups[0][1] === 4) return { rank: 7, name: '四条', value: 7e10 + +groups[0][0] * 1e8 };
    if (groups[0][1] === 3 && groups[1][1] === 2) return { rank: 6, name: '葫芦', value: 6e10 + +groups[0][0] * 1e8 };
    if (isFlush) return { rank: 5, name: '同花', value: 5e10 + ranks.reduce((s,v,i) => s + v * Math.pow(100, 4-i), 0) };
    if (isStraight) return { rank: 4, name: '顺子', value: 4e10 + straightHigh * 1e8 };
    if (groups[0][1] === 3) return { rank: 3, name: '三条', value: 3e10 + +groups[0][0] * 1e8 };
    if (groups[0][1] === 2 && groups[1][1] === 2) return { rank: 2, name: '两对', value: 2e10 + Math.max(+groups[0][0],+groups[1][0]) * 1e8 };
    if (groups[0][1] === 2) return { rank: 1, name: '一对', value: 1e10 + +groups[0][0] * 1e8 };
    return { rank: 0, name: '高牌', value: ranks.reduce((s,v,i) => s + v * Math.pow(100, 4-i), 0) };
}

function getBestHand(holeCards, communityCards) {
    const all = [...holeCards, ...communityCards];
    if (all.length < 5) return { rank: 0, name: '等待更多牌', value: 0 };
    let best = null;
    function combos(arr, k, start=0, chosen=[]) {
        if (chosen.length === k) {
            const result = evaluateHand(chosen);
            if (!best || result.value > best.value) best = result;
            return;
        }
        for (let i = start; i < arr.length; i++) {
            chosen.push(arr[i]);
            combos(arr, k, i+1, chosen);
            chosen.pop();
        }
    }
    combos(all, 5);
    return best;
}