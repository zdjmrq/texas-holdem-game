/**
 * 服务端牌局引擎
 */
const RANK_VALS = { '2':2,'3':3,'4':4,'5':5,'6':6,'7':7,'8':8,'9':9,'10':10,'J':11,'Q':12,'K':13,'A':14 };
const SUITS = ['spades','hearts','diamonds','clubs'];

class Card {
    constructor(rank, suit) { this.rank = rank; this.suit = suit; this.value = RANK_VALS[rank] || parseInt(rank); }
}

class Deck {
    constructor(ranks) {
        this.cards = [];
        const r = ranks || ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
        for (const s of SUITS) for (const rank of r) this.cards.push(new Card(rank, s));
    }
    shuffle() {
        for (let i = this.cards.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [this.cards[i], this.cards[j]] = [this.cards[j], this.cards[i]];
        }
    }
    deal() { return this.cards.pop(); }
}

class GameEngine {
    constructor(isShortDeck, useEliteAI) {
        this.isShortDeck = isShortDeck;
        this.useEliteAI = useEliteAI;
        this.players = [];
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.phase = 'idle';
        this.bettingRound = 'preflop';
        this.dealerPosition = -1;
        this.currentPlayerIdx = 0;
        this.roundBets = {};
        this.smallBlind = 40;
        this.bigBlind = 80;
        this.numHands = 0;
        this.lastHandResult = null;
    }

    addPlayer(name, id, isHuman) {
        this.players.push({ name, id, isHuman, stack: 1000, cards: [], folded: false, isAllIn: false, lastAction: null, aiStyle: isHuman ? null : ['TAG','LAG','Tight-Passive','Maniac','Calling Station','Solid'][Math.floor(Math.random()*6)] });
    }

    startNewHand() {
        this.dealerPosition = (this.dealerPosition + 1) % this.players.length;
        const active = this.players.filter(p => p.stack > 0);
        if (active.length <= 1) { this.phase = 'idle'; return; }
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.phase = 'preflop';
        this.bettingRound = 'preflop';
        this.roundBets = {};
        this.lastHandResult = null;
        for (const p of this.players) {
            if (p.stack <= 0) continue;
            p.folded = false; p.isAllIn = false; p.cards = []; p.lastAction = null;
            this.roundBets[p.id] = 0;
        }
        const ranks = this.isShortDeck ? ['6','7','8','9','10','J','Q','K','A'] : ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
        this.deck = new Deck(ranks);
        this.deck.shuffle();
        for (const p of this.players) { if (p.stack > 0) { p.cards.push(this.deck.deal()); p.cards.push(this.deck.deal()); } }
        const sbIdx = (this.dealerPosition + 1) % this.players.length;
        const bbIdx = (this.dealerPosition + 2) % this.players.length;
        if (this.players[sbIdx].stack > 0) {
            const sb = Math.min(this.smallBlind, this.players[sbIdx].stack);
            this.players[sbIdx].stack -= sb; this.roundBets[this.players[sbIdx].id] = sb; this.pot += sb;
        }
        if (this.players[bbIdx].stack > 0) {
            const bb = Math.min(this.bigBlind, this.players[bbIdx].stack);
            this.players[bbIdx].stack -= bb; this.roundBets[this.players[bbIdx].id] = bb; this.pot += bb;
        }
        this.currentBet = this.bigBlind;
        this.currentPlayerIdx = (bbIdx + 1) % this.players.length;
        this.numHands++;
    }

    handleAction(playerId, action, amount) {
        const player = this.players.find(p => p.id === playerId);
        if (!player || player.folded || player.isAllIn) return { error: '无法行动' };
        if (this.players[this.currentPlayerIdx].id !== playerId) return { error: '还没轮到你' };
        const toCall = Math.max(0, this.currentBet - (this.roundBets[playerId] || 0));
        switch (action) {
            case 'fold': player.folded = true; break;
            case 'check': break;
            case 'call':
                const callAmt = Math.min(toCall, player.stack);
                player.stack -= callAmt; this.roundBets[playerId] = (this.roundBets[playerId] || 0) + callAmt; this.pot += callAmt;
                break;
            case 'raise':
                const betAmt = Math.min(amount || this.bigBlind * 2, player.stack);
                player.stack -= betAmt; this.roundBets[playerId] = (this.roundBets[playerId] || 0) + betAmt; this.pot += betAmt;
                this.currentBet = this.roundBets[playerId];
                break;
            case 'allin':
                const allAmt = player.stack;
                player.stack = 0; this.roundBets[playerId] = (this.roundBets[playerId] || 0) + allAmt; this.pot += allAmt;
                if (this.roundBets[playerId] > this.currentBet) this.currentBet = this.roundBets[playerId];
                player.isAllIn = true;
                break;
        }
        player.lastAction = action;
        return { handEnded: false };
    }

    executeAction(playerIdx, action, amount) {
        return this.handleAction(this.players[playerIdx].id, action, amount);
    }

    advanceGame() {
        const inHand = this.players.filter(p => !p.folded && p.stack > 0);
        const canAct = inHand.filter(p => !p.isAllIn);
        if (canAct.length <= 1) { this.endHand(); return; }
        const allActed = this.players.every(p => p.folded || p.isAllIn || (this.roundBets[p.id] || 0) >= this.currentBet);
        if (allActed) { this.nextStreet(); return; }
        let next = (this.currentPlayerIdx + 1) % this.players.length;
        let attempts = 0;
        while (attempts < this.players.length) {
            const p = this.players[next];
            if (!p.folded && !p.isAllIn && p.stack > 0) { this.currentPlayerIdx = next; return; }
            next = (next + 1) % this.players.length;
            attempts++;
        }
        this.endHand();
    }

    nextStreet() {
        switch (this.bettingRound) {
            case 'preflop':
                this.bettingRound = 'flop'; this.phase = 'flop';
                this.communityCards.push(this.deck.deal()); this.communityCards.push(this.deck.deal()); this.communityCards.push(this.deck.deal());
                break;
            case 'flop': this.bettingRound = 'turn'; this.phase = 'turn'; this.communityCards.push(this.deck.deal()); break;
            case 'turn': this.bettingRound = 'river'; this.phase = 'river'; this.communityCards.push(this.deck.deal()); break;
            case 'river': this.doShowdown(); return;
        }
        this.currentBet = 0;
        for (const p of this.players) this.roundBets[p.id] = 0;
        this.currentPlayerIdx = this.players.findIndex(p => !p.folded && p.stack > 0);
    }

    doShowdown() {
        this.phase = 'showdown';
        const inHand = this.players.filter(p => !p.folded && p.stack >= 0);
        const results = inHand.map(p => ({ player: p, hand: this.evaluateHand(p.cards) }));
        results.sort((a, b) => b.hand.value - a.hand.value);
        const winner = results[0];
        const winners = results.filter(r => r.hand.value === winner.hand.value);
        const share = Math.floor(this.pot / winners.length);
        for (const w of winners) w.player.stack += share;
        this.lastHandResult = { winners: winners.map(w => ({ name: w.player.name, hand: w.hand })), pot: this.pot };
        this.pot = 0;
    }

    endHand() {
        const inHand = this.players.filter(p => !p.folded && p.stack >= 0);
        if (inHand.length === 1) {
            inHand[0].stack += this.pot;
            this.lastHandResult = { winners: [{ name: inHand[0].name, hand: null }], pot: this.pot };
            this.pot = 0;
        }
        this.phase = 'idle';
    }

    getAIDecision(playerIdx) {
        const player = this.players[playerIdx];
        if (!player || player.isHuman) return { action: 'check', amount: 0 };
        const style = player.aiStyle || 'TAG';
        const toCall = Math.max(0, this.currentBet - (this.roundBets[player.id] || 0));
        const handStrength = this.evaluateHandStrength(player.cards);
        const potOdds = toCall > 0 ? toCall / (this.pot + toCall) : 0;
        const pRandom = Math.random();
        if (toCall === 0) {
            if (handStrength > 0.6 + pRandom * 0.2) return { action: 'raise', amount: Math.min(Math.floor(player.stack * (0.3 + Math.random() * 0.4)), player.stack) };
            return { action: 'check', amount: 0 };
        }
        if (handStrength > 0.7) return { action: 'raise', amount: Math.min(Math.floor(player.stack * 0.4), player.stack) };
        if (handStrength > 0.4 || potOdds < 0.25) return { action: 'call', amount: toCall };
        if (handStrength < 0.15 && toCall > player.stack * 0.15) return { action: 'fold', amount: 0 };
        if (pRandom < 0.08 && toCall < player.stack * 0.1) return { action: 'raise', amount: Math.min(toCall * 3, player.stack) };
        return { action: 'call', amount: toCall };
    }

    evaluateHandStrength(cards) {
        if (!cards || cards.length < 2) return 0;
        const v1 = cards[0].value, v2 = cards[1].value;
        const paired = v1 === v2;
        return paired ? Math.min(1, v1 / 14 * 0.8) : Math.max(v1, v2) / 14 * 0.5 + Math.min(v1, v2) / 14 * 0.15;
    }

    evaluateHand(cards) {
        const rankCounts = {};
        for (const c of cards) rankCounts[c.value] = (rankCounts[c.value] || 0) + 1;
        const groups = Object.entries(rankCounts).sort((a, b) => b[1] - a[1] || b[0] - a[0]);
        const vals = cards.map(c => c.value).sort((a, b) => b - a);
        let score = groups.reduce((s, g, i) => s + parseInt(g[0]) * Math.pow(14, 5 - i), 0);
        if (groups[0][1] === 4) score += 7e6;
        else if (groups[0][1] === 3 && groups[1] && groups[1][1] >= 2) score += 6e6;
        else if (cards.length >= 5 && new Set(cards.map(c => c.suit)).size === 1) score += 5e6;
        else if (groups[0][1] === 3) score += 3e6;
        else if (groups[0][1] === 2 && groups[1] && groups[1][1] === 2) score += 2e6;
        else if (groups[0][1] === 2) score += 1e6;
        return { rank: Math.floor(score / 1e6), name: '手牌', value: score };
    }

    getPublicState() {
        return {
            players: this.players.map(p => ({ name: p.name, id: p.id, stack: p.stack, folded: p.folded, isAllIn: p.isAllIn, isHuman: p.isHuman, lastAction: p.lastAction })),
            communityCards: this.communityCards.map(c => ({ rank: c.rank, suit: c.suit })),
            pot: this.pot, currentBet: this.currentBet, phase: this.phase, bettingRound: this.bettingRound,
            dealerPosition: this.dealerPosition, currentPlayerIdx: this.currentPlayerIdx,
            smallBlind: this.smallBlind, bigBlind: this.bigBlind, numHands: this.numHands
        };
    }

    getPlayerState(playerId) {
        const p = this.players.find(p => p.id === playerId);
        if (!p) return { cards: [], stack: 0, bet: 0, position: 0, isYourTurn: false, availableActions: [] };
        return {
            cards: p.cards.map(c => ({ rank: c.rank, suit: c.suit })),
            stack: p.stack, bet: this.roundBets[playerId] || 0,
            isYourTurn: this.players[this.currentPlayerIdx].id === playerId && !p.folded && !p.isAllIn && this.phase !== 'idle',
            availableActions: this.getAvailableActions(playerId)
        };
    }

    getAvailableActions(playerId) {
        const p = this.players.find(p => p.id === playerId);
        if (!p || p.folded || p.isAllIn || this.phase === 'idle') return [];
        const toCall = Math.max(0, this.currentBet - (this.roundBets[playerId] || 0));
        const actions = [];
        if (toCall === 0) actions.push('check'); else actions.push('call', 'fold');
        if (p.stack > toCall + this.bigBlind) actions.push('raise');
        if (p.stack > 0) actions.push('allin');
        return actions;
    }
}

module.exports = { GameEngine, Card, Deck };