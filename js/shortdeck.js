/**
 * Short Deck (6+ Hold'em) — 完整实现
 * ==========================================
 * 与标准德州扑克完全独立的代码，不修改任何现有文件。
 *
 * 核心差异：
 *   1. 36张牌（去掉2-5）
 *   2. 前注制（Ante），每人1倍前注$40，庄家2倍前注$80
 *   3. 牌型排名：同花 > 葫芦，顺子 > 三条
 *   4. 最小顺子 A-6-7-8-9（A充当5）
 *   5. 翻前翻后都是庄家下家（UTG）先行动
 *   6. 最小下注/加注 = 2倍前注 = $80
 *   7. 无大小盲、无 BB Option
 */

// ============================================================
// 1. 常量
// ============================================================

const SD_RANKS = ['6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SD_RANK_VALUES = {
    '6': 6, '7': 7, '8': 8, '9': 9, '10': 10,
    'J': 11, 'Q': 12, 'K': 13, 'A': 14
};

// 短牌牌型排名：同花(6) > 葫芦(5) > 顺子(4) > 三条(3)
const SD_HAND_TYPES = {
    HIGH_CARD: 0,
    ONE_PAIR: 1,
    TWO_PAIR: 2,
    THREE_OF_A_KIND: 3,
    STRAIGHT: 4,
    FULL_HOUSE: 5,
    FLUSH: 6,
    FOUR_OF_A_KIND: 7,
    STRAIGHT_FLUSH: 8,
    ROYAL_FLUSH: 9
};

const SD_HAND_TYPE_NAMES = {
    0: '高牌',
    1: '一对',
    2: '两对',
    3: '三条',
    4: '顺子',
    5: '葫芦',
    6: '同花',
    7: '四条',
    8: '同花顺',
    9: '皇家同花顺'
};

// ============================================================
// 2. 短牌牌堆（36张：6到A）
// ============================================================

class ShortDeckDeck {
    constructor() {
        this.cards = [];
        for (const suit of SUITS) {
            for (const rank of SD_RANKS) {
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

    deal() { return this.cards.pop(); }

    removeCards(toRemove) {
        const set = new Set(toRemove.map(c => `${c.rank}-${c.suit}`));
        this.cards = this.cards.filter(c => !set.has(`${c.rank}-${c.suit}`));
    }

    clone() {
        const d = new ShortDeckDeck();
        d.cards = this.cards.map(c => new Card(c.rank, c.suit));
        return d;
    }
}

// ============================================================
// 3. 牌型评估
// ============================================================

/**
 * 短牌顺子检测（含 A-6-7-8-9）
 * 返回 false 或 { high, isWheel }
 */
function checkSDStraight(values) {
    const unique = [...new Set(values)].sort((a, b) => b - a);
    if (unique.length < 5) return false;

    // 常规顺子
    if (unique[0] - unique[4] === 4) return { high: unique[0], isWheel: false };

    // 短牌轮盘顺：A-6-7-8-9（A=14，当5使用）
    if (unique[0] === 14 && unique[1] === 9 && unique[2] === 8 &&
        unique[3] === 7 && unique[4] === 6) {
        return { high: 9, isWheel: true };
    }

    return false;
}

/** 评估5张牌（短牌排名） */
function evaluateSD5(cards) {
    const values = cards.map(c => c.value).sort((a, b) => b - a);
    const suits = cards.map(c => c.suit);

    const isFlush = suits.every(s => s === suits[0]);
    const sr = checkSDStraight(values);
    const isStraight = sr !== false;

    const freq = {};
    for (const v of values) freq[v] = (freq[v] || 0) + 1;
    const counts = Object.entries(freq).map(([v, c]) => ({ value: parseInt(v), count: c }));
    counts.sort((a, b) => b.count - a.count || b.value - a.value);

    const isRoyal = isStraight && isFlush && sr.high === 14;
    const straightHigh = sr ? sr.high : null;

    let rank, score;

    if (isRoyal) {
        rank = SD_HAND_TYPES.ROYAL_FLUSH;
        score = rank * 10 ** 10;
    } else if (isStraight && isFlush) {
        rank = SD_HAND_TYPES.STRAIGHT_FLUSH;
        score = rank * 10 ** 10 + straightHigh * 10 ** 8;
    } else if (counts[0].count === 4) {
        rank = SD_HAND_TYPES.FOUR_OF_A_KIND;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8 +
            (counts[1] ? counts[1].value * 10 ** 6 : 0);
    } else if (isFlush) {
        // 短牌：同花 > 葫芦
        rank = SD_HAND_TYPES.FLUSH;
        score = rank * 10 ** 10;
        for (let i = 0; i < values.length; i++)
            score += values[i] * 10 ** (8 - i * 2);
    } else if (counts[0].count === 3 && counts[1] && counts[1].count === 2) {
        rank = SD_HAND_TYPES.FULL_HOUSE;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8 +
            counts[1].value * 10 ** 6;
    } else if (isStraight) {
        // 短牌：顺子 > 三条
        rank = SD_HAND_TYPES.STRAIGHT;
        score = rank * 10 ** 10 + straightHigh * 10 ** 8;
    } else if (counts[0].count === 3) {
        rank = SD_HAND_TYPES.THREE_OF_A_KIND;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8;
        const kickers = values.filter(v => v !== counts[0].value)
            .sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++)
            score += kickers[i] * 10 ** (6 - i * 2);
    } else if (counts[0].count === 2 && counts[1] && counts[1].count === 2) {
        rank = SD_HAND_TYPES.TWO_PAIR;
        const pairs = [counts[0].value, counts[1].value]
            .sort((a, b) => b - a);
        score = rank * 10 ** 10 + pairs[0] * 10 ** 8 + pairs[1] * 10 ** 6;
        const kicker = values.filter(v => v !== pairs[0] && v !== pairs[1]);
        if (kicker.length) score += kicker[0] * 10 ** 4;
    } else if (counts[0].count === 2) {
        rank = SD_HAND_TYPES.ONE_PAIR;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8;
        const kickers = values.filter(v => v !== counts[0].value)
            .sort((a, b) => b - a);
        for (let i = 0; i < kickers.length; i++)
            score += kickers[i] * 10 ** (6 - i * 2);
    } else {
        rank = SD_HAND_TYPES.HIGH_CARD;
        score = rank * 10 ** 10;
        for (let i = 0; i < values.length; i++)
            score += values[i] * 10 ** (8 - i * 2);
    }

    return { rank, score, name: SD_HAND_TYPE_NAMES[rank], cards: cards.slice() };
}

/** N张牌中取最佳5张（短牌排名） */
function evaluateSDHand(cards) {
    if (cards.length < 5) return evaluateSDPartial(cards);
    const combos = getCombinations(cards, 5);
    let best = null;
    for (const combo of combos) {
        const r = evaluateSD5(combo);
        if (!best || r.score > best.score) best = r;
    }
    return best;
}

/** 不足5张时的评估 */
function evaluateSDPartial(cards) {
    const values = cards.map(c => c.value).sort((a, b) => b - a);
    const suits = cards.map(c => c.suit);
    const isFlush = suits.every(s => s === suits[0]);

    const freq = {};
    for (const v of values) freq[v] = (freq[v] || 0) + 1;
    const counts = Object.entries(freq).map(([v, c]) => ({ value: parseInt(v), count: c }));
    counts.sort((a, b) => b.count - a.count || b.value - a.value);

    let rank, score;

    if (counts[0].count === 4) {
        rank = SD_HAND_TYPES.FOUR_OF_A_KIND;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8
            + (counts[1] ? counts[1].value * 10 ** 6 : 0);
    } else if (counts[0].count === 3 && counts.length >= 2 && counts[1].count === 2) {
        rank = SD_HAND_TYPES.FULL_HOUSE;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8 + counts[1].value * 10 ** 6;
    } else if (counts[0].count === 3) {
        rank = SD_HAND_TYPES.THREE_OF_A_KIND;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8;
        const k = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < k.length; i++) score += k[i] * 10 ** (6 - i * 2);
    } else if (counts.length >= 2 && counts[0].count === 2 && counts[1].count === 2) {
        rank = SD_HAND_TYPES.TWO_PAIR;
        const pairs = [counts[0].value, counts[1].value].sort((a, b) => b - a);
        score = rank * 10 ** 10 + pairs[0] * 10 ** 8 + pairs[1] * 10 ** 6;
        const k = values.filter(v => v !== pairs[0] && v !== pairs[1]);
        if (k.length) score += k[0] * 10 ** 4;
    } else if (counts[0].count === 2) {
        rank = SD_HAND_TYPES.ONE_PAIR;
        score = rank * 10 ** 10 + counts[0].value * 10 ** 8;
        const k = values.filter(v => v !== counts[0].value).sort((a, b) => b - a);
        for (let i = 0; i < k.length; i++) score += k[i] * 10 ** (6 - i * 2);
    } else if (cards.length === 2 && isFlush) {
        rank = SD_HAND_TYPES.HIGH_CARD;
        score = rank * 10 ** 10 + values[0] * 10 ** 8 + values[1] * 10 ** 6 + 10 ** 4;
    } else {
        rank = SD_HAND_TYPES.HIGH_CARD;
        score = rank * 10 ** 10;
        for (let i = 0; i < values.length; i++)
            score += values[i] * 10 ** (8 - i * 2);
    }

    return { rank, score, name: SD_HAND_TYPE_NAMES[rank], cards: cards.slice() };
}

function compareSDHands(h1, h2) {
    if (h1.score > h2.score) return 1;
    if (h1.score < h2.score) return -1;
    return 0;
}

/** 获取当前牌可击败的牌型列表 */
function getSDBeatableHands(currentHand) {
    const beats = [];
    for (let rank = 0; rank < currentHand.rank; rank++) {
        beats.push(SD_HAND_TYPE_NAMES[rank]);
    }
    return beats;
}

// ============================================================
// 4. 短牌概率计算器（36张牌）
// ============================================================

class SDProbability {
    static calculateWinProb(playerCards, communityCards, numOpponents = 3, numSim = 2000) {
        const known = [...playerCards, ...communityCards];
        const deck = new ShortDeckDeck();
        deck.removeCards(known);
        const total = 36;
        const unknown = total - known.length;
        const rem = 5 - communityCards.length;

        if (rem === 0 && unknown <= 12) return this.enumerate(playerCards, communityCards, deck, numOpponents);
        return this.monteCarlo(playerCards, communityCards, deck, numOpponents, numSim, rem);
    }

    static monteCarlo(playerCards, communityCards, deck, opponents, sims, rem) {
        let w = 0, t = 0, l = 0, equity = 0;
        const cards = deck.cards;
        for (let s = 0; s < sims; s++) {
            cards.sort(() => Math.random() - 0.5);
            let idx = 0;
            const com = [...communityCards];
            for (let i = 0; i < rem; i++) com.push(cards[idx++]);
            const ph = evaluateSDHand([...playerCards, ...com]);
            let best = -1;
            const scores = [];
            for (let o = 0; o < opponents; o++) {
                const oh = evaluateSDHand([cards[idx++], cards[idx++], ...com]);
                if (oh && oh.score > best) best = oh.score;
                if (oh) scores.push(oh.score);
            }
            if (ph && ph.score > best) { w++; equity += 1; }
            else if (ph && ph.score === best) {
                t++;
                equity += 1 / (scores.filter(score => score === ph.score).length + 1);
            }
            else l++;
        }
        return { winProb: w / sims, tieProb: t / sims, loseProb: l / sims, equityProb: equity / sims };
    }

    static enumerate(playerCards, communityCards, deck, opponents) {
        let w = 0, t = 0, l = 0, equity = 0;
        const rem = 5 - communityCards.length;
        const combs = rem > 0 ? getCombinations(deck.cards, rem) : [[]];
        for (const g of combs.slice(0, 5000)) {
            const com = [...communityCards, ...g];
            const ph = evaluateSDHand([...playerCards, ...com]);
            const rest = deck.cards.filter(c => !g.includes(c));
            const opps = opponents > 0 ? getCombinations(rest, 2) : [[]];
            for (const og of opps.slice(0, 50)) {
                const oh = evaluateSDHand([og[0], og[1], ...com]);
                if (ph && oh && ph.score > oh.score) { w++; equity += 1; }
                else if (ph && oh && ph.score === oh.score) { t++; equity += 0.5; }
                else l++;
            }
        }
        const tot = w + t + l;
        return tot ? { winProb: w / tot, tieProb: t / tot, loseProb: l / tot, equityProb: equity / tot } :
            { winProb: 0, tieProb: 0, loseProb: 0, equityProb: 0 };
    }

    /** 找补牌 */
    static findOuts(playerCards, communityCards, numOpponents = 3) {
        const deck = new ShortDeckDeck();
        deck.removeCards([...playerCards, ...communityCards]);
        const rem = 5 - communityCards.length;
        if (rem <= 0) return { outs: [], byHandType: {} };

        const current = evaluateSDHand([...playerCards, ...communityCards]);
        const baseRank = current ? current.rank : -1;
        const baseScore = current ? current.score : 0;
        const byType = {};

        for (const card of deck.cards) {
            const nh = evaluateSDHand([...playerCards, ...communityCards, card]);
            if (!nh) continue;
            const better = nh.rank > baseRank ||
                (nh.rank === baseRank && nh.score > baseScore + 10 ** 4);
            if (!better) continue;
            if (!byType[nh.name]) byType[nh.name] = { cards: [], count: 0 };
            byType[nh.name].cards.push(card);
            byType[nh.name].count++;
        }

        return { outs: deck.cards, totalOuts: Object.values(byType)
            .reduce((s, v) => s + v.count, 0), byHandType: byType };
    }
}

// ============================================================
// 5. 短牌AI
// ============================================================

const SD_STYLE_CONFIGS = {
    [AI_STYLES.TAG]: {
        vpip: 0.20, pfr: 0.82, aggression: 0.55, bluffFreq: 0.07,
        foldToRaise: 0.55, foldTo3Bet: 0.55, raiseRatio: 2.2, cbFreq: 0.50,
        threeBetFreq: 0.05, doubleBarrel: 0.42, floatFreq: 0.14,
        checkRaiseFreq: 0.07, heroCallFreq: 0.10,
        description: 'Tight-Aggressive'
    },
    [AI_STYLES.LAG]: {
        vpip: 0.30, pfr: 0.78, aggression: 0.50, bluffFreq: 0.12,
        foldToRaise: 0.38, foldTo3Bet: 0.42, raiseRatio: 2.5, cbFreq: 0.45,
        threeBetFreq: 0.11, doubleBarrel: 0.50, floatFreq: 0.24,
        checkRaiseFreq: 0.11, heroCallFreq: 0.20,
        description: 'Loose-Aggressive'
    },
    [AI_STYLES.TAGFISH]: {
        vpip: 0.18, pfr: 0.50, aggression: 0.35, bluffFreq: 0.04,
        foldToRaise: 0.45, foldTo3Bet: 0.52, raiseRatio: 1.8, cbFreq: 0.30,
        threeBetFreq: 0.03, doubleBarrel: 0.22, floatFreq: 0.07,
        checkRaiseFreq: 0.04, heroCallFreq: 0.12,
        description: 'Tight-Passive'
    },
    [AI_STYLES.MANIAC]: {
        vpip: 0.38, pfr: 0.68, aggression: 0.60, bluffFreq: 0.20,
        foldToRaise: 0.25, foldTo3Bet: 0.30, raiseRatio: 3.0, cbFreq: 0.50,
        threeBetFreq: 0.17, doubleBarrel: 0.58, floatFreq: 0.28,
        checkRaiseFreq: 0.17, heroCallFreq: 0.25,
        description: 'Maniac'
    },
    [AI_STYLES.CALLING]: {
        vpip: 0.38, pfr: 0.14, aggression: 0.20, bluffFreq: 0.03,
        foldToRaise: 0.10, foldTo3Bet: 0.18, raiseRatio: 1.3, cbFreq: 0.20,
        threeBetFreq: 0.02, doubleBarrel: 0.10, floatFreq: 0.05,
        checkRaiseFreq: 0.03, heroCallFreq: 0.30,
        description: 'Calling Station'
    },
    [AI_STYLES.SOLID]: {
        vpip: 0.23, pfr: 0.72, aggression: 0.45, bluffFreq: 0.08,
        foldToRaise: 0.45, foldTo3Bet: 0.48, raiseRatio: 2.0, cbFreq: 0.40,
        threeBetFreq: 0.07, doubleBarrel: 0.42, floatFreq: 0.16,
        checkRaiseFreq: 0.09, heroCallFreq: 0.15,
        description: 'Solid'
    }
};

class ShortDeckAIPlayer extends AIPlayer {
    constructor(name, style, stack = 20000, index = 0) {
        super(name, style, stack, index);
        this.config = SD_STYLE_CONFIGS[style] || SD_STYLE_CONFIGS[AI_STYLES.SOLID];
        this.minBet = 80; // 2× ante
        this.isShortDeck = true;
    }

    // 翻后评估：用短牌概率
    evaluateHandStrength(communityCards, numOpponentsActive) {
        if (communityCards.length === 0) return this.evaluatePreFlop();

        const r = SDProbability.calculateWinProb(
            this.holeCards, communityCards, Math.max(1, numOpponentsActive), 500
        );
        const wp = Number.isFinite(r.equityProb) ? r.equityProb : r.winProb;
        return { winProb: Math.min(0.95, Math.max(0.02, wp)), raw: r };
    }

    // 短牌翻前牌力（36张牌的胜率校准）
    evaluatePreFlop() {
        const c1 = this.holeCards[0], c2 = this.holeCards[1];
        const hv = Math.max(c1.value, c2.value);
        const lv = Math.min(c1.value, c2.value);
        const suited = c1.suit === c2.suit;
        const gap = hv - lv;

        let s = 0;
        if (c1.rank === c2.rank) {
            const pv = c1.value;
            if (pv >= 12) s = 0.72;       // QQ+
            else if (pv >= 10) s = 0.65;  // TT-JJ
            else if (pv >= 8) s = 0.58;   // 88-99
            else s = 0.50;                // 66-77
        } else if (hv === 14 && lv >= 12) s = 0.68;
        else if (hv === 14 && lv >= 11) s = 0.58;
        else if (hv === 14 && lv >= 10) s = 0.47;
        else if (hv >= 12 && lv >= 11) s = 0.52;
        else if (hv >= 12 && lv >= 10) s = 0.42;
        else if (suited && gap <= 2 && hv >= 11) s = 0.42;
        else if (suited && gap <= 2 && hv >= 9) s = 0.34;
        else if (suited && gap <= 1 && hv >= 8) s = 0.28;
        else if (suited && hv >= 13) s = 0.30;
        else if (suited && hv >= 11) s = 0.24;
        else if (hv >= 14 && lv >= 8) s = 0.26;
        else if (hv >= 13 && lv >= 8) s = 0.22;
        else if (hv >= 12 && lv >= 8) s = 0.20;
        else if (suited && hv >= 10) s = 0.18;
        else if (suited) s = 0.14;
        else if (gap <= 3 && hv >= 10) s = 0.20;
        else if (gap <= 2 && hv >= 8) s = 0.16;
        else if (hv >= 14) s = 0.24;
        else if (hv >= 12) s = 0.18;
        else s = 0.08;

        return { winProb: s, strength: s };
    }
}

function createSDAIPlayers(count, startingStack = 20000) {
    const styles = [...Object.values(AI_STYLES)];
    for (let i = styles.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [styles[i], styles[j]] = [styles[j], styles[i]];
    }
    return Array.from({ length: count }, (_, i) => {
        const stack = Math.max(1, Math.floor(Number(startingStack) || 20000));
        const p = new ShortDeckAIPlayer(AI_NAMES[i % AI_NAMES.length], styles[i % styles.length],
            stack, i);
        p.avatar = AI_AVATARS[i % AI_AVATARS.length];
        return p;
    });
}

// ============================================================
// 6. 短牌游戏主类
// ============================================================

class ShortDeckGame {
    constructor(options = {}) {
        const configuredStack = Number(options.startingStack);
        const configuredAnte = Number(options.ante);
        const configuredMinBet = Number(options.minBet);
        this.startingStack = Number.isFinite(configuredStack) && configuredStack > 0
            ? Math.floor(configuredStack) : 20000;
        this.players = [];
        this.humanPlayer = null;
        this.aiPlayers = [];
        this.deck = null;
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.dealerPosition = -1;
        this.currentPlayerIndex = 0;

        // 短牌：前注制（无盲注）
        this.ante = Number.isFinite(configuredAnte) && configuredAnte > 0
            ? Math.floor(configuredAnte) : 40;          // 基础前注单位
        this.minBet = Number.isFinite(configuredMinBet) && configuredMinBet >= this.ante
            ? Math.floor(configuredMinBet) : this.ante * 2; // 最小下注/加注

        this.phase = 'idle';
        this.bettingRound = 'preflop';

        this.handHistory = [];
        this.numHands = 0;
        this.handGeneration = 0;
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.flopHadAggression = false;
        this.flopCbetResolved = false;
        this.playersDealtThisHand = [];

        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.lastRaiser = -1;
        this.hasFullBetThisRound = false;

        // UI兼容：短牌无SB/BB
        this.sbIndex = -1;
        this.bbIndex = -1;
        this.bbNeedsOption = false;
        // UI兼容：bigBlind用作最小下注参考
        this.bigBlind = this.minBet;

        this.onUpdate = null;
        this.onPlayerAction = null;
        this.onHandEnd = null;
        this.onAIThinking = null;
        this.onAIAction = null;

        this.probabilityResult = null;
        this.aiDelay = 1800;
        this.isProcessing = false;
        this.humanStack = this.startingStack;
    }

    // ── 初始化 ──

    init(numAiPlayers = 5) {
        this.players = [];
        this.aiPlayers = [];

        this.humanPlayer = {
            name: 'You', stack: this.startingStack, holeCards: [], chipsInPot: 0,
            folded: false, isAllIn: false, hasActed: false,
            isHuman: true, lastAction: null, avatar: '😤'
        };
        this.players.push(this.humanPlayer);

        const ais = createSDAIPlayers(numAiPlayers, this.startingStack);
        for (const ai of ais) {
            const obj = {
                name: ai.name, stack: ai.stack, holeCards: [],
                chipsInPot: 0, folded: false, isAllIn: false,
                hasActed: false, isHuman: false, lastAction: null,
                avatar: ai.avatar, aiRef: ai
            };
            this.players.push(obj);
            this.aiPlayers.push(obj);
        }

        for (let i = 0; i < this.aiPlayers.length; i++) {
            this.aiPlayers[i].aiRef.position = i + 1;
            this.aiPlayers[i].aiRef.numPlayers = this.players.length;
            this.aiPlayers[i].aiRef.bigBlind = this.minBet;
            this.aiPlayers[i].aiRef.minBet = this.minBet;
        }

        this.numHands = 0;
        const active = this.getActivePlayers();
        if (active.length > 0) {
            const indices = active.map(p => this.players.indexOf(p));
            this.dealerPosition = indices[Math.floor(Math.random() * indices.length)];
        } else {
            this.dealerPosition = 0;
        }
    }

    // ── 新一局 ──

    startNewHand() {
        this.handGeneration++;
        this.isProcessing = false;

        for (const p of this.players) {
            p.holeCards = [];
            p.chipsInPot = 0;
            p.folded = p.stack <= 0;
            p.isAllIn = false;
            p.hasActed = false;
            p.lastAction = null;
        }

        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};

        const active = this.getActivePlayers();
        if (active.length < 2) {
            this.phase = 'idle';
            if (this.onUpdate) this.onUpdate();
            if (this.onHandEnd) {
                if (active.length === 1) this.onHandEnd({ winner: active[0], pot: this.pot, reason: 'Not enough players' });
                else { this.phase = 'idle'; }
            }
            return;
        }

        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = 0;
        this.phase = 'preflop';
        this.bettingRound = 'preflop';
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.lastRaiser = -1;
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.flopHadAggression = false;
        this.flopCbetResolved = false;
        this.hasFullBetThisRound = false;
        this.probabilityResult = null;

        if (this.numHands > 0) {
            this.dealerPosition = this.getNextActivePlayer(this.dealerPosition);
            if (this.dealerPosition === -1)
                this.dealerPosition = this.players.indexOf(active[0]);
        }

        // 36张牌
        this.deck = new ShortDeckDeck();
        this.deck.shuffle();

        for (const p of active) p.holeCards = [this.deck.deal(), this.deck.deal()];
        this.playersDealtThisHand = [...active];

        // 收前注
        this.postAntes();

        // 短牌翻前：庄家下家先行动
        this.currentPlayerIndex = this.getNextActivePlayer(this.dealerPosition);
        const forcedRunout = this.markRoundCompleteIfNoDecision();
        this.isProcessing = false;
        this.calculateProbability();
        if (this.onUpdate) this.onUpdate();

        if (forcedRunout) {
            const generation = this.handGeneration;
            setTimeout(() => {
                if (generation === this.handGeneration && this.phase !== 'idle') this.advanceGame();
            }, 350);
        } else if (!this.isPlayerTurn())
            setTimeout(() => this.processAITurns(), 500);
    }

    // ── 收取前注 ──

    postAntes() {
        const active = this.getActivePlayers();
        // 每人1倍前注
        for (const p of active) {
            const amt = Math.min(this.ante, p.stack);
            p.stack -= amt;
            p.chipsInPot += amt;
            this.pot += amt;
            if (p.stack === 0) p.isAllIn = true;
        }
        // 庄家额外1倍前注（共2倍）
        const dealer = this.players[this.dealerPosition];
        if (dealer && !dealer.folded && dealer.stack > 0) {
            const amt = Math.min(this.ante, dealer.stack);
            dealer.stack -= amt;
            dealer.chipsInPot += amt;
            this.pot += amt;
            if (dealer.stack === 0) dealer.isAllIn = true;
            dealer.lastAction = { action: 'dealer ante', amount: amt };
        }
        // Antes are dead money. chipsInPot drives the UI, while roundBets must
        // remain zero so every player faces the full opening bet.
        this.lastRaise = this.minBet; // 最小加注额 = 80
    }

    // ── 辅助方法 ──

    isPlayerTurn() {
        return this.currentPlayerIndex === 0 && this.players[0] &&
            !this.players[0].folded && !this.players[0].isAllIn;
    }

    get currentPlayer() { return this.players[this.currentPlayerIndex]; }

    getActivePlayers() { return this.players.filter(p => !p.folded && p.stack > 0); }

    getPlayersInHand() { return this.players.filter(p => !p.folded); }

    getNextActivePlayer(startPos) {
        const n = this.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!this.players[idx].folded && this.players[idx].stack > 0 && !this.players[idx].isAllIn)
                return idx;
        }
        return -1;
    }

    getNextPlayerInHand(startPos) {
        const n = this.players.length;
        for (let i = 1; i < n; i++) {
            const idx = (startPos + i) % n;
            if (!this.players[idx].folded) return idx;
        }
        return -1;
    }

    getHandPositionInfo(playerIndex) {
        let seats = this.playersDealtThisHand.map(player => this.players.indexOf(player))
            .filter(index => index >= 0);
        if (!seats.length) {
            seats = this.players.map((player, index) => player.holeCards?.length === 2 ? index : -1)
                .filter(index => index >= 0);
        }
        seats.sort((a, b) => ((a - this.dealerPosition + this.players.length) % this.players.length) -
            ((b - this.dealerPosition + this.players.length) % this.players.length));
        return {
            positionFromButton: Math.max(0, seats.indexOf(playerIndex)),
            playerCount: Math.max(2, seats.length)
        };
    }

    getNextPlayerNeedingAction(startPos) {
        const n = this.players.length;
        for (let step = 1; step <= n; step++) {
            const idx = (startPos + step) % n;
            const player = this.players[idx];
            if (!player || player.folded || player.isAllIn || player.stack <= 0) continue;
            const matched = (this.roundBets[idx] || 0) >= this.currentBet;
            if (!matched || !this.playersActed.has(idx)) return idx;
        }
        return -1;
    }

    canPlayerRaise(playerIndex) {
        const player = this.players[playerIndex];
        if (!player || player.folded || player.isAllIn || player.stack <= 0) return false;
        if (!this.playersActed.has(playerIndex)) return true;
        const facedBefore = this.actedAtBet[playerIndex] || 0;
        const required = this.raiseSizeAtAction[playerIndex] || this.minBet;
        return this.currentBet - facedBefore >= required;
    }

    recordRoundAction(playerIndex) {
        this.playersActed.add(playerIndex);
        this.actedAtBet[playerIndex] = this.currentBet;
        this.raiseSizeAtAction[playerIndex] = Math.max(this.lastRaise, this.minBet);
    }

    getMinRaiseTo() {
        if (this.currentBet === 0) return this.minBet;
        if (!this.hasFullBetThisRound && this.currentBet < this.minBet) return this.minBet;
        return this.currentBet + Math.max(this.lastRaise, this.minBet);
    }

    markRoundCompleteIfNoDecision() {
        const live = this.getPlayersInHand().filter(p => !p.isAllIn && p.stack > 0);
        if (live.length === 0) return true;
        if (live.length !== 1) return false;
        const idx = this.players.indexOf(live[0]);
        if (Math.max(0, this.currentBet - (this.roundBets[idx] || 0)) > 0) return false;
        this.recordRoundAction(idx);
        return true;
    }

    refundUncalledBet() {
        const entries = this.players.map((player, index) => ({
            player, index, amount: Math.max(0, this.roundBets[index] || 0)
        })).sort((a, b) => b.amount - a.amount);
        if (entries.length < 2 || entries[0].amount === entries[1].amount) return 0;
        const top = entries[0];
        const refund = top.amount - entries[1].amount;
        top.player.stack += refund;
        top.player.chipsInPot -= refund;
        this.pot -= refund;
        this.roundBets[top.index] -= refund;
        if (top.player.isAllIn && top.player.stack > 0) top.player.isAllIn = false;
        const liveBets = this.getPlayersInHand().map(p => this.roundBets[this.players.indexOf(p)] || 0);
        this.currentBet = liveBets.length ? Math.max(...liveBets) : 0;
        return refund;
    }

    // ── 下注轮次检查 ──

    isBettingRoundComplete() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) return true;

        for (const p of inHand) {
            if (p.isAllIn) continue;
            const idx = this.players.indexOf(p);
            const myBet = this.roundBets[idx] || 0;
            if (myBet < this.currentBet) return false;
            if (!this.playersActed.has(idx)) return false;
        }

        // 短牌无BB Option，直接返回
        return true;
    }

    // ── 玩家行动 ──

    playerAction(action, amount) {
        if (this.isProcessing || !this.isPlayerTurn()) return;
        this.isProcessing = true;
        this.executeAction(0, action, amount);
        this.isProcessing = false;
        if (this.onUpdate) this.onUpdate();
        if (this.checkHandEnd()) return;

        for (let safety = 0; safety < 5; safety++) {
            const prev = this.phase;
            this.advanceGame();
            if (this.phase === 'showdown' || this.phase === 'idle') break;
            if (this.phase !== prev) break;
            const gi = this.currentPlayerIndex;
            const gp = this.players[gi];
            if (gp.folded || gp.isAllIn) continue;
            if ((this.roundBets[gi] || 0) >= this.currentBet && this.playersActed.has(gi)) continue;
            break;
        }

        if (this.currentPlayerIndex !== 0 && this.phase !== 'showdown' && this.phase !== 'idle')
            setTimeout(() => this.processAITurns(), 500);
    }

    // ── 执行行动 ──

    executeAction(playerIndex, action, amount) {
        const player = this.players[playerIndex];
        if (!player) {
            console.warn(`executeAction: player ${playerIndex} not found (action=${action})`);
            return;
        }
        const a = action.toLowerCase();
        const contributed = this.roundBets[playerIndex] || 0;
        const toCall = Math.max(0, this.currentBet - contributed);
        const currentBetBefore = this.currentBet;
        const lastAggressorBefore = this.lastAggressor;
        const lastAggressorStreetBefore = this.lastAggressorStreet;
        const preflopRaiseCountBefore = this.preflopRaiseCount;

        switch (a) {
            case 'fold':
                player.folded = true;
                player.lastAction = { action: 'fold', amount: 0 };
                break;

            case 'check':
                if (toCall > 0) { this.executeAction(playerIndex, 'call', 0); return; }
                player.lastAction = { action: 'check', amount: 0 };
                this.recordRoundAction(playerIndex);
                break;

            case 'call':
                if (toCall === 0) { this.executeAction(playerIndex, 'check', 0); return; }
                const callAmt = Math.min(toCall, player.stack);
                player.stack -= callAmt;
                player.chipsInPot += callAmt;
                this.pot += callAmt;
                if (player.stack === 0) player.isAllIn = true;
                player.lastAction = { action: 'call', amount: callAmt };
                this.roundBets[playerIndex] = (this.roundBets[playerIndex] || 0) + callAmt;
                this.recordRoundAction(playerIndex);
                break;

            case 'raise':
            case 'bet':
                const contrib = this.roundBets[playerIndex] || 0;
                const total = Math.min(amount, player.stack + contrib);
                const add = total - contrib;
                if (!this.canPlayerRaise(playerIndex)) {
                    this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }
                if (add <= 0) { this.executeAction(playerIndex, 'call', 0); return; }

                // 最小加注验证
                const minRaiseSize = Math.max(this.lastRaise, this.minBet);
                const minTotal = this.getMinRaiseTo();

                if (total < minTotal) {
                    if (total === player.stack + contrib && total > this.currentBet) {
                        this.executeAction(playerIndex, 'allin', 0);
                    } else this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }

                const actualAdd = Math.min(add, player.stack);
                player.stack -= actualAdd;
                player.chipsInPot += actualAdd;
                this.pot += actualAdd;

                const prevCurrent = this.currentBet;
                this.currentBet = total;
                this.lastRaise = this.currentBet - prevCurrent;
                this.hasFullBetThisRound = true;
                player.lastAction = { action: 'raise', amount: actualAdd };
                this.roundBets[playerIndex] = total;
                this.playersActed.clear();
                this.recordRoundAction(playerIndex);
                this.lastRaiser = playerIndex;
                this.lastAggressor = playerIndex;
                this.lastAggressorStreet = this.phase;
                if (this.phase === 'preflop') this.preflopRaiseCount++;
                if (player.stack === 0) player.isAllIn = true;
                break;

            case 'allin':
                const aiAmt = player.stack;
                const beforeAI = this.roundBets[playerIndex] || 0;
                const totalAI = beforeAI + aiAmt;
                if (totalAI > this.currentBet && !this.canPlayerRaise(playerIndex)) {
                    this.executeAction(playerIndex, toCall > 0 ? 'call' : 'check', 0);
                    return;
                }
                player.chipsInPot += aiAmt;
                this.pot += aiAmt;

                if (totalAI > this.currentBet) {
                    const mrs = Math.max(this.lastRaise, this.minBet);
                    if (totalAI >= this.currentBet + mrs) {
                        this.lastRaise = totalAI - this.currentBet;
                        this.currentBet = totalAI;
                        this.playersActed.clear();
                        this.lastRaiser = playerIndex;
                        this.lastAggressor = playerIndex;
                        this.lastAggressorStreet = this.phase;
                        if (this.phase === 'preflop') this.preflopRaiseCount++;
                        this.hasFullBetThisRound = true;
                    } else {
                        this.currentBet = totalAI;
                        if (!this.hasFullBetThisRound && this.currentBet >= this.minBet) {
                            this.hasFullBetThisRound = true;
                            this.lastRaise = this.minBet;
                        }
                    }
                }

                player.stack = 0;
                player.isAllIn = true;
                this.roundBets[playerIndex] = totalAI;
                this.recordRoundAction(playerIndex);
                player.lastAction = { action: 'allin', amount: aiAmt };
                break;
        }

        this.actionsThisRound++;
        const resolvedAction = player.lastAction?.action || a;
        const raisedCurrentBet = this.currentBet > currentBetBefore;
        if (raisedCurrentBet) {
            this.lastAggressor = playerIndex;
            this.lastAggressorStreet = this.phase;
            if (this.phase === 'preflop') {
                this.preflopAggressor = playerIndex;
                if (this.preflopRaiseCount === preflopRaiseCountBefore) this.preflopRaiseCount++;
            }
            if (this.phase === 'flop') this.flopHadAggression = true;
        }
        const isCbetOpportunity = this.phase === 'flop' && playerIndex === this.preflopAggressor &&
            !this.flopCbetResolved && currentBetBefore === 0;
        if (this.phase === 'flop' && playerIndex === this.preflopAggressor && !this.flopCbetResolved) {
            this.flopCbetResolved = true;
        }
        this.notifyAIsOfAction(playerIndex, resolvedAction, {
            toCallBefore: toCall,
            aggressive: raisedCurrentBet,
            isCbetOpportunity,
            facedCbet: this.phase === 'flop' && toCall > 0 &&
                lastAggressorBefore === this.preflopAggressor && lastAggressorStreetBefore === 'flop',
            faced3bet: this.phase === 'preflop' && toCall > 0 && preflopRaiseCountBefore >= 2,
            preflopRaiseCountBefore
        });
    }

    notifyAIsOfAction(playerIndex, action, actionMeta = {}) {
        const street = this.phase;
        for (let i = 0; i < this.players.length; i++) {
            if (i === playerIndex) continue;
            const ai = this.players[i]?.aiRef;
            if (!ai) continue;
            if (typeof ai.recordOpponentAction === 'function')
                ai.recordOpponentAction(playerIndex, action, street, this.handGeneration, actionMeta);
            if (typeof ai.updateOpponentRange === 'function') {
                let rangeAction = action;
                if (street === 'preflop' && actionMeta.aggressive) {
                    if (actionMeta.preflopRaiseCountBefore >= 2) rangeAction = '4bet';
                    else if (actionMeta.preflopRaiseCountBefore >= 1) rangeAction = '3bet';
                    else rangeAction = 'raise';
                } else if (street === 'preflop' && action === 'call' && actionMeta.preflopRaiseCountBefore >= 2) {
                    rangeAction = 'call3bet';
                }
                ai.updateOpponentRange(playerIndex, rangeAction, false, null);
            }
        }
    }

    // ── AI 流程 ──

    async processAITurns() {
        const gen = this.handGeneration;

        while (!this.isPlayerTurn() && !this.isProcessing && this.handGeneration === gen && this.phase !== 'idle') {
            if (this.checkHandEnd()) return;

            const idx = this.currentPlayerIndex;
            const player = this.players[idx];

            // Safety: player not found — skip
            if (!player) { this.advanceGame(); continue; }

            if (player.folded || player.isAllIn) { this.advanceGame(); continue; }

            this.isProcessing = true;

            const active = this.getPlayersInHand();
            const tablePosition = this.getHandPositionInfo(idx);
            const opponentStacks = active.filter(p => p !== player)
                .map(p => p.stack + (this.roundBets[this.players.indexOf(p)] || 0));
            const gameState = {
                communityCards: this.communityCards,
                pot: this.pot,
                currentBet: this.currentBet,
                toCall: Math.max(0, this.currentBet - (this.roundBets[idx] || 0)),
                yourBet: this.roundBets[idx] || 0,
                stack: player.stack,
                effectiveStack: Math.min(player.stack, Math.max(0, ...opponentStacks)),
                numOpponentsActive: active.length - 1,
                dealerPosition: this.dealerPosition,
                currentPlayerIndex: idx,
                positionFromButton: tablePosition.positionFromButton,
                canCheck: (this.roundBets[idx] || 0) >= this.currentBet,
                canRaise: this.canPlayerRaise(idx),
                minRaiseTo: this.getMinRaiseTo(),
                maxRaiseTo: (this.roundBets[idx] || 0) + player.stack,
                preflopRaiseCount: this.preflopRaiseCount,
                isPreviousStreetAggressor: this.lastAggressor === idx &&
                    this.lastAggressorStreet === ({ flop:'preflop', turn:'flop', river:'turn' }[this.phase] || null),
                isDelayedCBetCandidate: this.phase === 'turn' && this.preflopAggressor === idx &&
                    !this.flopHadAggression,
                isSmallBlind: false,
                isBigBlind: false,
                bigBlind: this.minBet,
                phase: this.phase,
                isShortDeck: true
            };

            if (this.onAIThinking) this.onAIThinking(idx);
            await new Promise(r => setTimeout(r, Math.max(120, this.aiDelay * (0.34 + Math.random() * 0.08))));
            if (this.handGeneration !== gen) return;

            const ai = player.aiRef;
            ai.holeCards = player.holeCards;
            ai.stack = player.stack;
            ai.chipsInPot = player.chipsInPot;
            ai.position = idx;
            ai.numPlayers = tablePosition.playerCount;

            await new Promise(r => setTimeout(r, Math.max(100, this.aiDelay * (0.14 + Math.random() * 0.05))));
            if (this.handGeneration !== gen) return;

            const decision = ai.decide(gameState);
            this.executeAction(idx, decision.action, decision.amount);
            this.isProcessing = false;

            if (this.onAIAction) this.onAIAction(idx, decision);

            for (let safety = 0; safety < 5; safety++) {
                const p = this.phase;
                this.advanceGame();
                if (this.phase === 'showdown' || this.phase === 'idle') break;
                if (this.phase !== p) break;
                const gi = this.currentPlayerIndex, gp = this.players[gi];
                if (gp.folded || gp.isAllIn) continue;
                if ((this.roundBets[gi] || 0) >= this.currentBet && this.playersActed.has(gi)) continue;
                break;
            }

            if (!(this.phase === 'showdown' || this.phase === 'idle')) {
                await new Promise(r => setTimeout(r, Math.max(120, this.aiDelay * (0.38 + Math.random() * 0.10))));
                if (this.handGeneration !== gen) return;
            }

            if (this.checkHandEnd()) return;
        }
    }

    // ── 游戏推进 ──

    advanceGame() {
        if (this.phase === 'idle') return false;
        if (this.checkHandEnd()) return false;

        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) { this.endHand(inHand[0]); return false; }

        if (this.isBettingRoundComplete()) {
            this.refundUncalledBet();
            if (inHand.filter(p => !p.isAllIn).length <= 1) {
                this.advancePhase();
                while (this.phase !== 'showdown') this.advancePhase();
                this.showdown();
                if (this.onUpdate) this.onUpdate();
                return false;
            }
            this.advancePhase();
            if (this.phase === 'showdown') { this.showdown(); if (this.onUpdate) this.onUpdate(); return false; }
            return true;
        }

        const next = this.getNextPlayerNeedingAction(this.currentPlayerIndex);
        if (next === -1) {
            if (this.getPlayersInHand().filter(p => !p.isAllIn).length <= 1) {
                this.advancePhase();
                while (this.phase !== 'showdown') this.advancePhase();
                this.showdown();
                if (this.onUpdate) this.onUpdate();
                return false;
            }
            return false;
        }

        this.currentPlayerIndex = next;

        if (this.onUpdate) this.onUpdate();
        return true;
    }

    // ── 阶段推进 ──

    advancePhase() {
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.actionsThisRound = 0;
        this.currentBet = 0;
        this.lastRaise = this.minBet;
        this.hasFullBetThisRound = false;

        switch (this.phase) {
            case 'preflop':
                this.dealCommunityCards();
                this.phase = 'flop';
                this.bettingRound = 'flop';
                break;
            case 'flop':
                this.dealCommunityCards();
                this.phase = 'turn';
                this.bettingRound = 'turn';
                break;
            case 'turn':
                this.dealCommunityCards();
                this.phase = 'river';
                this.bettingRound = 'river';
                break;
            case 'river':
                this.phase = 'showdown';
                this.bettingRound = 'showdown';
                return;
        }

        this.currentPlayerIndex = this.getNextActivePlayer(this.dealerPosition);
        this.calculateProbability();
        if (this.onUpdate) this.onUpdate();
    }

    // ── 发公共牌 ──

    dealCommunityCards() {
        this.deck.deal(); // 烧牌
        this.communityCards.push(this.deck.deal());
        if (this.phase === 'preflop') {
            this.communityCards.push(this.deck.deal());
            this.communityCards.push(this.deck.deal());
        }
    }

    // ── 概率计算 ──

    calculateProbability() {
        if (!this.humanPlayer || this.humanPlayer.folded) return;

        const community = [...this.communityCards];
        const opponents = this.getPlayersInHand().filter(p => !p.isHuman).length;

        if (this.phase === 'preflop' && community.length === 0) {
            // 简化的翻前评估
            const c1 = this.humanPlayer.holeCards[0];
            const c2 = this.humanPlayer.holeCards[1];
            let strength = 0;

            if (c1.rank === c2.rank) {
                const v = c1.value;
                strength = v >= 12 ? 0.72 : v >= 10 ? 0.62 : v >= 8 ? 0.55 : 0.48;
            } else {
                const h = Math.max(c1.value, c2.value);
                const l = Math.min(c1.value, c2.value);
                const suited = c1.suit === c2.suit;
                const gap = h - l;

                if (h === 14 && l >= 12) strength = 0.68;
                else if (h === 14 && l >= 11) strength = 0.58;
                else if (h >= 12 && l >= 11) strength = 0.52;
                else if (h >= 12 && suited) strength = 0.38;
                else if (h >= 14) strength = 0.32;
                else if (suited && gap <= 2) strength = 0.30;
                else strength = 0.22;
            }

            this.probabilityResult = { winProb: strength, tieProb: 0.05, loseProb: 1 - strength - 0.05 };
            return;
        }

        try {
            const r = SDProbability.calculateWinProb(
                this.humanPlayer.holeCards, community,
                Math.max(1, opponents),
                community.length >= 3 ? 2000 : 1500
            );
            this.probabilityResult = r;
        } catch (e) {
            console.error('SD probability error:', e);
            this.probabilityResult = { winProb: 0.5, tieProb: 0.05, loseProb: 0.45 };
        }
    }

    // ── 检查牌局结束 ──

    checkHandEnd() {
        // An outer action loop can observe the hand once more after settlement.
        // Do not announce the already-cleared pot a second time.
        if (this.phase === 'idle') return true;
        const ih = this.getPlayersInHand();
        if (ih.length <= 1) { this.endHand(ih.length === 1 ? ih[0] : null); return true; }
        return false;
    }

    endHand(winner) {
        if (this.phase === 'idle') return;
        this.refundUncalledBet();
        const settledPot = this.pot;
        if (winner) {
            winner.stack += settledPot;
            if (this.onHandEnd) {
                this.onHandEnd({
                    winner, pot: settledPot,
                    reason: winner.folded ? 'fold' : 'all folded',
                    name: winner.name
                });
            }
        }
        this.phase = 'idle';
        this.numHands++;
        this.pot = 0;
        if (this.onUpdate) this.onUpdate();
    }

    // ── 边池计算 ──

    calculateSidePots() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) return [{ amount: this.pot, eligible: [...inHand] }];

        const levels = [...new Set(this.players.map(p => Math.max(0, p.chipsInPot || 0)).filter(Boolean))]
            .sort((a, b) => a - b);
        const pots = [];
        let previous = 0;
        for (const level of levels) {
            const contributors = this.players.filter(p => (p.chipsInPot || 0) >= level);
            const eligible = inHand.filter(p => (p.chipsInPot || 0) >= level);
            const amount = (level - previous) * contributors.length;
            if (amount > 0 && eligible.length > 0) pots.push({ amount, eligible });
            previous = level;
        }
        const accounted = pots.reduce((sum, pot) => sum + pot.amount, 0);
        if (this.pot > accounted && pots.length) pots[0].amount += this.pot - accounted;
        return pots;
    }

    orderFromLeftOfDealer(players) {
        const n = this.players.length;
        return [...players].sort((a, b) => {
            const ai = this.players.indexOf(a), bi = this.players.indexOf(b);
            const ad = ((ai - this.dealerPosition + n) % n) || n;
            const bd = ((bi - this.dealerPosition + n) % n) || n;
            return ad - bd;
        });
    }

    // ── 摊牌 ──

    showdown() {
        const inHand = this.getPlayersInHand();
        if (inHand.length <= 1) { this.endHand(inHand[0]); return; }

        const results = [];
        const handMap = new Map();
        for (const p of inHand) {
            const hand = evaluateSDHand([...p.holeCards, ...this.communityCards]);
            results.push({ player: p, hand });
            handMap.set(p, hand);
        }

        const sidePots = this.calculateSidePots();
        const allWinners = [];
        let totalAwarded = 0;
        const winnerAmounts = new Map();

        for (const pot of sidePots) {
            if (pot.amount <= 0) continue;

            let bestHand = null, potWinners = [];
            for (const p of pot.eligible) {
                const hand = handMap.get(p);
                if (!hand) continue;
                if (!bestHand || compareSDHands(hand, bestHand) > 0) {
                    bestHand = hand;
                    potWinners = [p];
                } else if (compareSDHands(hand, bestHand) === 0) {
                    if (!potWinners.includes(p)) potWinners.push(p);
                }
            }

            if (potWinners.length === 0) {
                const share = Math.floor(pot.amount / pot.eligible.length);
                for (const p of pot.eligible) {
                    p.stack += share;
                    winnerAmounts.set(p, (winnerAmounts.get(p) || 0) + share);
                    totalAwarded += share;
                }
                allWinners.push(...pot.eligible);
                continue;
            }

            potWinners = this.orderFromLeftOfDealer(potWinners);
            const share = Math.floor(pot.amount / potWinners.length);
            let rem = pot.amount - share * potWinners.length;
            for (const w of potWinners) {
                const award = share + (rem > 0 ? 1 : 0);
                if (rem > 0) rem--;
                w.stack += award;
                winnerAmounts.set(w, (winnerAmounts.get(w) || 0) + award);
                totalAwarded += award;
            }
            allWinners.push(...potWinners);
        }

        if (this.pot > totalAwarded && allWinners.length > 0) {
            const leftover = this.pot - totalAwarded;
            const firstLeft = this.orderFromLeftOfDealer([...new Set(allWinners)])[0];
            firstLeft.stack += leftover;
            winnerAmounts.set(firstLeft, (winnerAmounts.get(firstLeft) || 0) + leftover);
        }

        const uniqueWinners = [...new Set(allWinners)];
        const bestResult = results.reduce((best, r) =>
            (!best || (r.hand && compareSDHands(r.hand, best.hand) > 0)) ? r : best, null);

        const settledPot = this.pot;
        if (this.onHandEnd) {
            this.onHandEnd({
                winners: uniqueWinners,
                winnerAmounts,
                pot: settledPot,
                hand: bestResult?.hand,
                handName: bestResult?.hand?.name || 'Unknown',
                results,
                reason: 'showdown',
                share: uniqueWinners.length > 0 ? Math.floor(this.pot / uniqueWinners.length) : 0
            });
        }

        this.phase = 'idle';
        this.numHands++;
        this.pot = 0;
        if (this.onUpdate) this.onUpdate();
    }

    // ── 手牌分析 ──

    getPlayerWinHands() {
        if (!this.humanPlayer || this.humanPlayer.folded || this.communityCards.length === 0) return [];

        const allCards = [...this.humanPlayer.holeCards, ...this.communityCards];
        const hand = evaluateSDHand(allCards);
        if (!hand) return [];

        return {
            currentHand: hand.name,
            beats: getSDBeatableHands(hand),
            allHandTypes: Object.values(SD_HAND_TYPE_NAMES)
        };
    }

    getOuts() {
        if (!this.humanPlayer || this.humanPlayer.folded || this.communityCards.length >= 5) return null;
        return SDProbability.findOuts(
            this.humanPlayer.holeCards, this.communityCards,
            this.getPlayersInHand().filter(p => !p.isHuman).length
        );
    }

    // ── 可用行动 ──

    getAvailableActions() {
        if (!this.isPlayerTurn()) return [];

        const toCall = Math.max(0, this.currentBet - (this.roundBets[0] || 0));
        const player = this.humanPlayer;
        const actions = [];

        if (toCall === 0) {
            actions.push({ type: 'check', label: 'Check', amount: 0 });
        } else {
            actions.push({ type: 'call', label: `Call ${Math.min(toCall, player.stack)}`, amount: Math.min(toCall, player.stack) });
        }

        if (player.stack > 0) {
            if (toCall > 0) actions.push({ type: 'fold', label: 'Fold', amount: 0 });

            const canRaise = this.canPlayerRaise(0);
            if (canRaise) {
                const minRaiseTotal = this.getMinRaiseTo();
                const raiseAmount = minRaiseTotal - (this.roundBets[0] || 0);

                if (raiseAmount < player.stack) {
                    actions.push({ type: 'raise', label: `Raise to ${minRaiseTotal}`, amount: minRaiseTotal });
                }

            }

            const allInTotal = (this.roundBets[0] || 0) + player.stack;
            if (allInTotal <= this.currentBet || canRaise)
                actions.push({ type: 'allin', label: 'All-in', amount: player.stack });
        }

        return actions;
    }

    // ── 重置 ──

    resetGame() {
        this.players = [];
        this.humanPlayer = null;
        this.aiPlayers = [];
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.phase = 'idle';
        this.numHands = 0;
        this.dealerPosition = -1;
        this.sbIndex = -1;
        this.bbIndex = -1;
        if (this.onUpdate) this.onUpdate();
    }

    // ── 卡牌格式化（兼容UI） ──

    static formatCard(card) {
        if (!card) return '';
        const sym = SUIT_SYMBOLS[card.suit];
        return { rank: card.rank, suit: card.suit, symbol: sym, red: card.suit === 'hearts' || card.suit === 'diamonds' };
    }
}
