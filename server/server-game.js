'use strict';

const { createDeck, shuffle, evaluateHand, VALUES } = require('./poker-rules');
const { AIPlayer, AI_STYLES } = require('../js/ai');

const AI_NAMES = ['Alex', 'Blake', 'Casey', 'Drew', 'Emma', 'Finn', 'Grace', 'Hayes', 'Ivy', 'Jade'];
const AI_AVATARS = ['😎', '🤠', '🕶️', '🎩', '👑', '🦊', '🐺', '🦅', '🐯', '🐉'];

function cloneCard(card) {
    return card ? { rank: card.rank, suit: card.suit, value: card.value ?? VALUES[card.rank] } : null;
}

class ServerPokerGame {
    constructor(config, players, options = {}) {
        this.config = { ...config };
        this.players = players;
        this.random = options.random || Math.random;
        this.phase = 'idle';
        this.communityCards = [];
        this.deck = [];
        this.pot = 0;
        this.currentBet = 0;
        this.lastRaise = this.minimumBet;
        this.hasFullBetThisRound = false;
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.dealerPos = -1;
        this.sbIdx = -1;
        this.bbIdx = -1;
        this.currentPlayerIdx = -1;
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.flopHadAggression = false;
        this.handId = 0;
        this.turnId = 0;
        this.lastHandResult = null;
        this.playersDealtLastHand = [];
        this.onChange = null;
        this.onHandEnd = null;
    }

    get isShortDeck() { return !!this.config.isShortDeck; }
    get minimumBet() { return this.isShortDeck ? this.config.minBet : this.config.bigBlind; }

    activeForNewHand(player) {
        return player.stack > 0 && !player.left && (!player.isHuman || player.connected);
    }

    inHand() { return this.players.filter(player => !player.folded && player.dealtIn); }
    liveToAct() { return this.inHand().filter(player => !player.isAllIn && player.stack > 0); }

    nextIndex(start, predicate) {
        const n = this.players.length;
        for (let step = 1; step < n; step++) {
            const idx = (start + step + n) % n;
            if (predicate(this.players[idx], idx)) return idx;
        }
        return -1;
    }

    nextDealt(start) {
        return this.nextIndex(start, player => player.dealtIn && !player.folded);
    }

    nextToAct(start) {
        return this.nextIndex(start, (player, idx) => {
            if (!player.dealtIn || player.folded || player.isAllIn || player.stack <= 0) return false;
            return (this.roundBets[idx] || 0) < this.currentBet || !this.playersActed.has(idx);
        });
    }

    startHand() {
        const previousBb = this.bbIdx;
        const previousDealtCount = this.playersDealtLastHand.length;
        const eligible = this.players.filter(player => this.activeForNewHand(player));
        this.communityCards = [];
        this.pot = 0;
        this.currentBet = 0;
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};

        for (const player of this.players) {
            player.holeCards = [];
            player.chipsInPot = 0;
            player.roundBet = 0;
            player.folded = !eligible.includes(player);
            player.dealtIn = eligible.includes(player);
            player.isAllIn = false;
            player.lastAction = null;
        }
        if (eligible.length < 2) {
            this.phase = 'idle';
            this.emitChange();
            return false;
        }

        this.lastHandResult = null;
        this.handId++;
        this.turnId = 0;
        this.phase = 'preflop';
        this.preflopRaiseCount = 0;
        this.preflopAggressor = -1;
        this.lastAggressor = -1;
        this.lastAggressorStreet = null;
        this.flopHadAggression = false;
        this.lastRaise = this.minimumBet;
        this.hasFullBetThisRound = false;

        const enteringHeadsUp = !this.isShortDeck && eligible.length === 2 && previousDealtCount > 2 &&
            previousBb >= 0 && eligible.includes(this.players[previousBb]);
        if (enteringHeadsUp) this.dealerPos = previousBb;
        else if (this.dealerPos < 0) this.dealerPos = this.players.indexOf(eligible[Math.floor(this.random() * eligible.length)]);
        else {
            const nextDealer = this.nextIndex(this.dealerPos, player => eligible.includes(player));
            if (nextDealer >= 0) this.dealerPos = nextDealer;
        }
        this.playersDealtLastHand = [...eligible];

        this.deck = shuffle(createDeck(this.isShortDeck), this.random);
        let dealPos = this.dealerPos;
        for (let round = 0; round < 2; round++) {
            for (let count = 0; count < eligible.length; count++) {
                dealPos = this.nextIndex(dealPos, player => eligible.includes(player));
                this.players[dealPos].holeCards.push(this.deck.pop());
            }
        }

        if (this.isShortDeck) this.postAntes();
        else this.postBlinds();

        const actionAnchor = this.isShortDeck ? this.dealerPos : this.bbIdx;
        this.currentPlayerIdx = this.nextToAct(actionAnchor);
        this.autoCloseNoDecision();
        this.bumpTurn();
        this.emitChange();
        if (this.isRoundComplete()) this.advanceAfterAction();
        return true;
    }

    commit(idx, amount, label) {
        const player = this.players[idx];
        const paid = Math.max(0, Math.min(Math.floor(amount), player.stack));
        player.stack -= paid;
        player.chipsInPot += paid;
        this.pot += paid;
        if (player.stack === 0) player.isAllIn = true;
        player.lastAction = { action: label, amount: paid };
        return paid;
    }

    postBlinds() {
        const dealt = this.inHand();
        if (dealt.length === 2) {
            this.sbIdx = this.dealerPos;
            this.bbIdx = this.nextDealt(this.dealerPos);
        } else {
            this.sbIdx = this.nextDealt(this.dealerPos);
            this.bbIdx = this.nextDealt(this.sbIdx);
        }
        this.roundBets[this.sbIdx] = this.commit(this.sbIdx, this.config.smallBlind, 'small blind');
        this.roundBets[this.bbIdx] = this.commit(this.bbIdx, this.config.bigBlind, 'big blind');
        this.players[this.sbIdx].roundBet = this.roundBets[this.sbIdx];
        this.players[this.bbIdx].roundBet = this.roundBets[this.bbIdx];
        const canBet = this.liveToAct().length;
        this.currentBet = canBet >= 2
            ? this.config.bigBlind
            : Math.max(this.roundBets[this.sbIdx] || 0, this.roundBets[this.bbIdx] || 0);
        this.lastRaise = this.config.bigBlind;
        this.hasFullBetThisRound = canBet >= 2 || (this.roundBets[this.bbIdx] || 0) >= this.config.bigBlind;
    }

    postAntes() {
        for (let idx = 0; idx < this.players.length; idx++) {
            const player = this.players[idx];
            if (!player.dealtIn) continue;
            this.commit(idx, this.config.ante, 'ante');
        }
        const dealer = this.players[this.dealerPos];
        if (dealer?.dealtIn && dealer.stack > 0) this.commit(this.dealerPos, this.config.ante, 'dealer ante');
        // Antes are dead money and deliberately never enter roundBets.
        this.currentBet = 0;
        this.lastRaise = this.config.minBet;
    }

    canRaise(idx) {
        const player = this.players[idx];
        if (!player || player.folded || player.isAllIn || player.stack <= 0) return false;
        if (!this.playersActed.has(idx)) return true;
        const faced = this.actedAtBet[idx] || 0;
        const required = this.raiseSizeAtAction[idx] || this.minimumBet;
        return this.currentBet - faced >= required;
    }

    minRaiseTo() {
        if (this.currentBet === 0) return this.minimumBet;
        if (!this.hasFullBetThisRound && this.currentBet < this.minimumBet) return this.minimumBet;
        return this.currentBet + Math.max(this.lastRaise, this.minimumBet);
    }

    recordAction(idx) {
        this.playersActed.add(idx);
        this.actedAtBet[idx] = this.currentBet;
        this.raiseSizeAtAction[idx] = Math.max(this.lastRaise, this.minimumBet);
    }

    legalActions(idx) {
        const player = this.players[idx];
        if (!player || idx !== this.currentPlayerIdx || player.folded || player.isAllIn || this.phase === 'idle') {
            return { actions: [], toCall: 0, canRaise: false, minRaiseTo: 0, maxRaiseTo: 0 };
        }
        const ownBet = this.roundBets[idx] || 0;
        const toCall = Math.max(0, this.currentBet - ownBet);
        const canRaise = this.canRaise(idx);
        const maxRaiseTo = ownBet + player.stack;
        const actions = [];
        if (toCall === 0) actions.push('check');
        else actions.push('fold', 'call');
        if (player.stack > 0 && canRaise && maxRaiseTo > this.minRaiseTo()) actions.push('raise');
        if (player.stack > 0 && (maxRaiseTo <= this.currentBet || canRaise)) actions.push('allin');
        return {
            actions,
            toCall: Math.min(toCall, player.stack),
            canRaise,
            minRaiseTo: this.minRaiseTo(),
            maxRaiseTo
        };
    }

    getHandPositionInfo(playerIndex) {
        const seats = this.players.map((player, index) => player.dealtIn ? index : -1)
            .filter(index => index >= 0)
            .sort((a, b) => ((a - this.dealerPos + this.players.length) % this.players.length) -
                ((b - this.dealerPos + this.players.length) % this.players.length));
        return {
            positionFromButton: Math.max(0, seats.indexOf(playerIndex)),
            playerCount: Math.max(2, seats.length)
        };
    }

    act(idx, action, amount = 0) {
        const legal = this.legalActions(idx);
        action = String(action || '').toLowerCase();
        if (!legal.actions.includes(action)) return { ok: false, error: '当前不能执行这个行动' };
        const player = this.players[idx];
        const ownBet = this.roundBets[idx] || 0;
        const toCall = Math.max(0, this.currentBet - ownBet);
        const potBefore = this.pot;
        const currentBetBefore = this.currentBet;
        const preflopRaiseCountBefore = this.preflopRaiseCount;

        if (action === 'fold') {
            player.folded = true;
            player.lastAction = { action: 'fold', amount: 0 };
        } else if (action === 'check') {
            player.lastAction = { action: 'check', amount: 0 };
            this.recordAction(idx);
        } else if (action === 'call') {
            const paid = this.commit(idx, Math.min(toCall, player.stack), 'call');
            this.roundBets[idx] = ownBet + paid;
            player.roundBet = this.roundBets[idx];
            this.recordAction(idx);
        } else if (action === 'raise') {
            let target = Math.floor(Number(amount) || 0);
            if (target < legal.minRaiseTo || target > legal.maxRaiseTo) return { ok: false, error: '加注金额不在合法范围内' };
            const previous = this.currentBet;
            const paid = this.commit(idx, target - ownBet, 'raise');
            this.roundBets[idx] = ownBet + paid;
            player.roundBet = this.roundBets[idx];
            this.currentBet = this.roundBets[idx];
            this.lastRaise = this.currentBet - previous;
            this.hasFullBetThisRound = true;
            this.playersActed.clear();
            this.recordAction(idx);
            if (this.phase === 'preflop') this.preflopRaiseCount++;
        } else if (action === 'allin') {
            const target = ownBet + player.stack;
            const previous = this.currentBet;
            const paid = this.commit(idx, player.stack, 'allin');
            this.roundBets[idx] = ownBet + paid;
            player.roundBet = this.roundBets[idx];
            if (target > previous) {
                const fullRaise = target >= this.minRaiseTo();
                this.currentBet = target;
                if (fullRaise) {
                    this.lastRaise = target - previous;
                    this.hasFullBetThisRound = true;
                    this.playersActed.clear();
                    if (this.phase === 'preflop') this.preflopRaiseCount++;
                } else if (!this.hasFullBetThisRound && target >= this.minimumBet) {
                    this.hasFullBetThisRound = true;
                    this.lastRaise = this.minimumBet;
                }
            }
            this.recordAction(idx);
        }

        const resolvedAction = player.lastAction?.action || action;
        const aggressive = this.currentBet > currentBetBefore;
        if (aggressive) {
            this.lastAggressor = idx;
            this.lastAggressorStreet = this.phase;
            if (this.phase === 'preflop') this.preflopAggressor = idx;
            if (this.phase === 'flop') this.flopHadAggression = true;
        }
        this.notifyAIsOfAction(idx, resolvedAction, {
            toCallBefore: toCall,
            amount: Math.max(0, Number(player.lastAction?.amount) || 0),
            betFraction: Math.max(0, Number(player.lastAction?.amount) || 0) /
                Math.max(this.minimumBet, potBefore + toCall),
            aggressive,
            preflopRaiseCountBefore,
            faced3bet: this.phase === 'preflop' && toCall > 0 && preflopRaiseCountBefore >= 2
        });

        this.advanceAfterAction();
        return { ok: true };
    }

    notifyAIsOfAction(actorIdx, action, meta = {}) {
        for (let idx = 0; idx < this.players.length; idx++) {
            if (idx === actorIdx) continue;
            const ai = this.players[idx]?.aiRef;
            if (!ai) continue;
            ai.position = idx;
            if (typeof ai.recordOpponentAction === 'function') {
                ai.recordOpponentAction(actorIdx, action, this.phase, this.handId, meta);
            }
            if (typeof ai.updateOpponentRange === 'function') {
                let rangeAction = action;
                if (this.phase === 'preflop' && meta.aggressive) {
                    if (meta.preflopRaiseCountBefore >= 2) rangeAction = '4bet';
                    else if (meta.preflopRaiseCountBefore >= 1) rangeAction = '3bet';
                    else rangeAction = 'raise';
                } else if (this.phase === 'preflop' && action === 'call' && meta.preflopRaiseCountBefore >= 2) {
                    rangeAction = 'call3bet';
                }
                ai.updateOpponentRange(actorIdx, rangeAction, false);
            }
        }
    }

    autoCloseNoDecision() {
        const live = this.liveToAct();
        if (live.length === 0) return true;
        if (live.length !== 1) return false;
        const idx = this.players.indexOf(live[0]);
        if (Math.max(0, this.currentBet - (this.roundBets[idx] || 0)) > 0) {
            this.currentPlayerIdx = idx;
            return false;
        }
        this.recordAction(idx);
        return true;
    }

    isRoundComplete() {
        if (this.inHand().length <= 1) return true;
        for (const player of this.inHand()) {
            if (player.isAllIn) continue;
            const idx = this.players.indexOf(player);
            if ((this.roundBets[idx] || 0) < this.currentBet || !this.playersActed.has(idx)) return false;
        }
        return true;
    }

    refundUncalled() {
        const entries = this.players.map((player, idx) => ({ player, idx, amount: this.roundBets[idx] || 0 }))
            .sort((a, b) => b.amount - a.amount);
        if (entries.length < 2 || entries[0].amount === entries[1].amount) return 0;
        const top = entries[0];
        const refund = top.amount - entries[1].amount;
        top.player.stack += refund;
        top.player.chipsInPot -= refund;
        this.pot -= refund;
        this.roundBets[top.idx] -= refund;
        top.player.roundBet = this.roundBets[top.idx];
        if (top.player.stack > 0) top.player.isAllIn = false;
        const liveBets = this.inHand().map(player => this.roundBets[this.players.indexOf(player)] || 0);
        this.currentBet = liveBets.length ? Math.max(...liveBets) : 0;
        return refund;
    }

    advanceAfterAction() {
        if (this.inHand().length <= 1) {
            this.finishByFold(this.inHand()[0]);
            return;
        }
        if (this.isRoundComplete()) {
            this.refundUncalled();
            if (this.liveToAct().length <= 1) {
                while (this.phase !== 'showdown') this.advanceStreet();
                this.showdown();
                return;
            }
            this.advanceStreet();
            if (this.phase === 'showdown') this.showdown();
            else {
                this.autoCloseNoDecision();
                if (this.isRoundComplete()) this.advanceAfterAction();
                else { this.bumpTurn(); this.emitChange(); }
            }
            return;
        }
        const next = this.nextToAct(this.currentPlayerIdx);
        if (next >= 0) this.currentPlayerIdx = next;
        this.bumpTurn();
        this.emitChange();
    }

    advanceStreet() {
        this.roundBets = {};
        this.playersActed = new Set();
        this.actedAtBet = {};
        this.raiseSizeAtAction = {};
        this.currentBet = 0;
        this.lastRaise = this.minimumBet;
        this.hasFullBetThisRound = false;
        for (const player of this.players) player.roundBet = 0;

        if (this.phase === 'preflop') {
            this.deck.pop();
            this.communityCards.push(this.deck.pop(), this.deck.pop(), this.deck.pop());
            this.phase = 'flop';
        } else if (this.phase === 'flop') {
            this.deck.pop(); this.communityCards.push(this.deck.pop()); this.phase = 'turn';
        } else if (this.phase === 'turn') {
            this.deck.pop(); this.communityCards.push(this.deck.pop()); this.phase = 'river';
        } else if (this.phase === 'river') {
            this.phase = 'showdown';
            this.currentPlayerIdx = -1;
            return;
        }
        this.currentPlayerIdx = this.nextToAct(this.dealerPos);
    }

    calculateSidePots() {
        const eligiblePlayers = this.inHand();
        const levels = [...new Set(this.players.map(p => p.chipsInPot || 0).filter(Boolean))].sort((a, b) => a - b);
        const pots = [];
        let previous = 0;
        for (const level of levels) {
            const contributors = this.players.filter(player => (player.chipsInPot || 0) >= level);
            const eligible = eligiblePlayers.filter(player => (player.chipsInPot || 0) >= level);
            const amount = (level - previous) * contributors.length;
            if (amount > 0 && eligible.length) pots.push({ amount, eligible });
            previous = level;
        }
        const accounted = pots.reduce((sum, item) => sum + item.amount, 0);
        if (this.pot > accounted && pots.length) pots[0].amount += this.pot - accounted;
        return pots;
    }

    orderLeftOfDealer(players) {
        const n = this.players.length;
        return [...players].sort((a, b) => {
            const ad = ((this.players.indexOf(a) - this.dealerPos + n) % n) || n;
            const bd = ((this.players.indexOf(b) - this.dealerPos + n) % n) || n;
            return ad - bd;
        });
    }

    showdown() {
        const live = this.inHand();
        const evaluations = new Map();
        for (const player of live) evaluations.set(player, evaluateHand([...player.holeCards, ...this.communityCards], this.isShortDeck));
        const winnerAmounts = new Map();
        const allWinners = [];
        for (const sidePot of this.calculateSidePots()) {
            let best = -Infinity;
            let winners = [];
            for (const player of sidePot.eligible) {
                const score = evaluations.get(player)?.score ?? -Infinity;
                if (score > best) { best = score; winners = [player]; }
                else if (score === best) winners.push(player);
            }
            winners = this.orderLeftOfDealer(winners);
            const share = Math.floor(sidePot.amount / winners.length);
            let remainder = sidePot.amount - share * winners.length;
            for (const player of winners) {
                const award = share + (remainder-- > 0 ? 1 : 0);
                player.stack += award;
                winnerAmounts.set(player, (winnerAmounts.get(player) || 0) + award);
            }
            allWinners.push(...winners);
        }
        const winners = [...new Set(allWinners)];
        const strongest = [...evaluations.values()].sort((a, b) => b.score - a.score)[0];
        this.notifyAIsOfShowdown(live, evaluations, winners);
        const settledPot = this.pot;
        this.lastHandResult = {
            reason: 'showdown',
            isShortDeck: this.isShortDeck,
            pot: settledPot,
            handRank: strongest?.rank ?? 0,
            winners: winners.map(player => this.publicIdentity(player)),
            winnerAmounts: Object.fromEntries([...winnerAmounts].map(([player, value]) => [player.id, value])),
            results: live.map(player => ({
                ...this.publicIdentity(player),
                cards: player.holeCards.map(cloneCard),
                handRank: evaluations.get(player)?.rank ?? 0
            }))
        };
        this.finishHand();
    }

    notifyAIsOfShowdown(live, evaluations, winners) {
        const winnerSet = new Set(winners);
        for (let observerIdx = 0; observerIdx < this.players.length; observerIdx++) {
            const ai = this.players[observerIdx]?.aiRef;
            if (!ai || typeof ai.recordShowdown !== 'function') continue;
            ai.position = observerIdx;
            for (const player of live) {
                const seatIdx = this.players.indexOf(player);
                const hand = evaluations.get(player);
                const action = player.lastAction?.action;
                const aggressive = action === 'raise' || action === 'allin';
                const passive = action === 'check' || action === 'call';
                ai.recordShowdown(seatIdx, {
                    won:winnerSet.has(player),
                    handRank:hand?.rank ?? 0,
                    wasBluff:aggressive && !winnerSet.has(player) && (hand?.rank ?? 0) <= 2,
                    wasTrap:passive && winnerSet.has(player) && (hand?.rank ?? 0) >= 4
                });
            }
        }
    }

    finishByFold(winner) {
        if (!winner) { this.finishHand(); return; }
        this.refundUncalled();
        const settledPot = this.pot;
        winner.stack += settledPot;
        this.lastHandResult = {
            reason: 'fold',
            isShortDeck: this.isShortDeck,
            pot: settledPot,
            handRank: 0,
            winners: [this.publicIdentity(winner)],
            winnerAmounts: { [winner.id]: settledPot },
            results: this.players.filter(player => player.dealtIn).map(player => ({
                ...this.publicIdentity(player), cards: [], handRank: 0
            }))
        };
        this.finishHand();
    }

    finishHand() {
        this.phase = 'idle';
        this.currentPlayerIdx = -1;
        this.pot = 0;
        this.turnId++;
        this.emitChange();
        if (this.onHandEnd) this.onHandEnd(this.lastHandResult);
    }

    publicIdentity(player) {
        return { id: player.id, seatId: player.seatId, name: player.name, isHuman: player.isHuman };
    }

    bumpTurn() { this.turnId++; }
    emitChange() { if (this.onChange) this.onChange(); }
}

function estimateEquity(game, idx, simulations = 120) {
    const hero = game.players[idx];
    const opponents = game.inHand().filter(player => player !== hero).length;
    const knownKeys = new Set([...hero.holeCards, ...game.communityCards].map(c => `${c.rank}-${c.suit}`));
    const source = createDeck(game.isShortDeck).filter(c => !knownKeys.has(`${c.rank}-${c.suit}`));
    let equity = 0;
    for (let run = 0; run < simulations; run++) {
        const deck = shuffle(source.slice(), game.random);
        let cursor = 0;
        const board = game.communityCards.map(cloneCard);
        while (board.length < 5) board.push(deck[cursor++]);
        const heroHand = evaluateHand([...hero.holeCards, ...board], game.isShortDeck);
        let beaten = false;
        let ties = 0;
        for (let opponent = 0; opponent < opponents; opponent++) {
            const hand = evaluateHand([deck[cursor++], deck[cursor++], ...board], game.isShortDeck);
            if (hand.score > heroHand.score) { beaten = true; break; }
            if (hand.score === heroHand.score) ties++;
        }
        if (!beaten) equity += 1 / (ties + 1);
    }
    return equity / simulations;
}

function chooseServerAiAction(game, idx) {
    const legal = game.legalActions(idx);
    if (!legal.actions.length) return null;
    const player = game.players[idx];
    if (!player.aiRef) return null;
    const ai = player.aiRef;
    const active = game.inHand();
    const position = game.getHandPositionInfo(idx);
    const opponentStacks = active.filter(item => item !== player)
        .map(item => item.stack + (game.roundBets[game.players.indexOf(item)] || 0));
    ai.holeCards = player.holeCards;
    ai.stack = player.stack;
    ai.chipsInPot = player.chipsInPot;
    ai.position = idx;
    ai.numPlayers = position.playerCount;
    ai.bigBlind = game.minimumBet;
    ai.isShortDeck = game.isShortDeck;
    return ai.decide({
        handId: game.handId,
        decisionId: `${game.phase}:${game.turnId}:${idx}`,
        seed: `online-${game.isShortDeck ? 'shortdeck' : 'standard'}:${game.handId}`,
        variant: game.isShortDeck ? 'shortdeck' : 'standard',
        communityCards: game.communityCards,
        pot: game.pot,
        currentBet: game.currentBet,
        toCall: legal.toCall,
        yourBet: game.roundBets[idx] || 0,
        stack: player.stack,
        effectiveStack: Math.min(player.stack, Math.max(0, ...opponentStacks)),
        numOpponentsActive: active.length - 1,
        activeOpponentSeats: active.filter(item => item !== player).map(item => game.players.indexOf(item)),
        playerCount: position.playerCount,
        dealerPosition: game.dealerPos,
        currentPlayerIndex: idx,
        positionFromButton: position.positionFromButton,
        legalActions: legal,
        canCheck: legal.actions.includes('check'),
        canRaise: legal.canRaise,
        minRaiseTo: legal.minRaiseTo,
        maxRaiseTo: legal.maxRaiseTo,
        preflopRaiseCount: game.preflopRaiseCount,
        isPreviousStreetAggressor: game.lastAggressor === idx &&
            game.lastAggressorStreet === ({ flop:'preflop', turn:'flop', river:'turn' }[game.phase] || null),
        isDelayedCBetCandidate: game.phase === 'turn' && game.preflopAggressor === idx && !game.flopHadAggression,
        isSmallBlind: idx === game.sbIdx,
        isBigBlind: idx === game.bbIdx,
        bigBlind: game.minimumBet,
        phase: game.phase,
        timeBudgetMs: game.phase === 'river' || active.length >= 5 ? 150 : 80,
        evaluateCards: (cards, variant) => evaluateHand(cards, variant === 'shortdeck')
    });
}

function createServerAi(seatId, startingStack) {
    const offset = Math.max(0, seatId - 1) % AI_NAMES.length;
    const styles = Object.values(AI_STYLES);
    const aiRef = new AIPlayer(AI_NAMES[offset], styles[offset % styles.length], startingStack, seatId);
    return {
        id: `ai-${seatId}`,
        seatId,
        name: AI_NAMES[offset],
        avatar: AI_AVATARS[offset],
        isHuman: false,
        connected: true,
        left: false,
        stack: startingStack,
        holeCards: [],
        chipsInPot: 0,
        roundBet: 0,
        folded: false,
        isAllIn: false,
        dealtIn: false,
        lastAction: null,
        aiRef
    };
}

module.exports = { ServerPokerGame, chooseServerAiAction, estimateEquity, createServerAi };
