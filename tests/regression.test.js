'use strict';

/**
 * Regression tests for the defects found during the v1.5.0 architecture review.
 * Each test pins one specific failure that was reproducible before the fix.
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { ServerPokerGame, chooseServerAiAction, createServerAi } = require('../server/server-game');
const PokerGameRules = require('../js/game-rules-core');
const ProbabilityCore = require('../js/probability-core');
const serverRules = require('../server/poker-rules');
const aiCore = require('../js/ai-core');
const { AIPlayer, AI_STYLES } = require('../js/ai');

const PROFILE = {
    vpip: 0.24, pfr: 0.75, aggression: 0.55, threeBetFreq: 0.08, bluffFreq: 0.10,
    foldTo3Bet: 0.48, cbFreq: 0.55, doubleBarrel: 0.48, floatFreq: 0.18,
    checkRaiseFreq: 0.10, heroCallFreq: 0.15
};

const AI_STYLE_LIST = Object.values(AI_STYLES);

/** Seat record matching server-game's shape: aiRef is the AIPlayer itself. */
function seat(index, options = {}) {
    const stack = options.stack ?? 20000;
    const isHuman = !!options.isHuman;
    return {
        id: options.id || `seat-${index}`,
        seatId: index,
        name: options.name || `Seat${index}`,
        avatar: '👤',
        isHuman,
        connected: true,
        left: false,
        stack,
        holeCards: [],
        chipsInPot: 0,
        roundBet: 0,
        folded: false,
        isAllIn: false,
        dealtIn: false,
        lastAction: null,
        aiRef: isHuman
            ? null
            : new AIPlayer(`Bot${index}`, AI_STYLE_LIST[index % AI_STYLE_LIST.length], stack, index)
    };
}

function gameWith(humanSeats, aiSeats) {
    const game = new ServerPokerGame({
        maxPlayers: humanSeats + aiSeats,
        isShortDeck: false,
        startingStack: 20000,
        smallBlind: 40,
        bigBlind: 80,
        random: Math.random
    });
    game.players = [];
    for (let i = 0; i < humanSeats; i++) game.players.push(seat(i, { isHuman: true }));
    for (let i = humanSeats; i < humanSeats + aiSeats; i++) game.players.push(seat(i));
    return game;
}

test('a failing AI observer cannot escape action handling', () => {
    const game = gameWith(1, 3);
    for (const player of game.players) {
        if (player.aiRef) {
            player.aiRef.recordOpponentAction = () => { throw new Error('injected observer failure'); };
            player.aiRef.updateOpponentRange = () => { throw new Error('injected range failure'); };
        }
    }
    assert.equal(game.startHand(), true);
    const idx = game.currentPlayerIdx;
    const legal = game.legalActions(idx);
    assert.ok(legal.actions.length > 0);
    // Before the fix this threw out of act() and, in the desktop build, out of
    // the websocket handler - taking the whole process with it.
    const result = game.act(idx, legal.actions.includes('check') ? 'check' : 'call', 0);
    assert.equal(result.ok, true);
});

test('equity sampling terminates when an opponent belief is folded', () => {
    const brain = new aiCore.UnifiedPokerAI({ name: 'T', style: 'SOLID', seatId: 1, profile: PROFILE });
    brain.observeAction({ seatId: 3, action: 'fold', street: 'preflop', handId: 1, meta: {} });
    const started = Date.now();
    const decision = brain.decide({
        handId: 1,
        decisionId: 'standard:flop:1:1',
        seed: 'regression',
        variant: 'standard',
        holeCards: [{ rank: 'A', suit: 'spades', value: 14 }, { rank: 'K', suit: 'spades', value: 13 }],
        communityCards: [
            { rank: 'Q', suit: 'hearts', value: 12 },
            { rank: 'J', suit: 'spades', value: 11 },
            { rank: '2', suit: 'clubs', value: 2 }
        ],
        pot: 1000, currentBet: 0, toCall: 0, heroRoundBet: 0, heroStack: 19700,
        effectiveStack: 19700, numOpponents: 3,
        // Seat 3 folds yet is still listed as active: this used to spin forever
        // because an empty sampler never advanced the sample counter.
        activeOpponentSeats: [2, 3, 4],
        playerCount: 4, positionFromButton: 1, isSmallBlind: false, isBigBlind: false,
        preflopRaiseCount: 1, legalActions: ['check', 'raise'], canCheck: true, canRaise: true,
        minRaiseTo: 100, maxRaiseTo: 19700, bigBlind: 100, timeBudgetMs: 150,
        evaluateCards: (cards, variant) => ProbabilityCore.evaluate(cards, variant === 'shortdeck')
    });
    assert.ok(Date.now() - started < 5000, 'decision must not hang');
    assert.ok(['check', 'raise', 'allin', 'fold', 'call'].includes(decision.action));
});

test('the decision is reproducible and does not depend on the sampling budget', () => {
    const decide = (mcSamples, timeBudgetMs) => {
        const brain = new aiCore.UnifiedPokerAI({ name: 'T', style: 'SOLID', seatId: 1, profile: PROFILE });
        return brain.decide({
            handId: 1,
            decisionId: 'standard:flop:1:1',
            seed: 'regression',
            variant: 'standard',
            holeCards: [{ rank: 'Q', suit: 'hearts', value: 12 }, { rank: 'J', suit: 'diamonds', value: 11 }],
            communityCards: [
                { rank: 'Q', suit: 'spades', value: 12 },
                { rank: '7', suit: 'hearts', value: 7 },
                { rank: '2', suit: 'clubs', value: 2 }
            ],
            pot: 1000, currentBet: 0, toCall: 0, heroRoundBet: 0, heroStack: 19800,
            effectiveStack: 19800, numOpponents: 2, activeOpponentSeats: [2, 4], playerCount: 3,
            positionFromButton: 1, isSmallBlind: false, isBigBlind: false, preflopRaiseCount: 1,
            legalActions: ['check', 'raise'], canCheck: true, canRaise: true,
            minRaiseTo: 100, maxRaiseTo: 19800, bigBlind: 100,
            timeBudgetMs, mcSamples,
            evaluateCards: (cards, variant) => ProbabilityCore.evaluate(cards, variant === 'shortdeck')
        });
    };
    const fast = decide(240, 150);
    const squeezed = decide(240, 20);
    const repeated = decide(240, 150);
    // Identical inputs must reproduce identical decisions...
    assert.deepEqual(repeated, fast);
    // ...and the same holds regardless of the wall-clock budget, because the
    // intended sample count now runs to completion instead of being cut short.
    assert.equal(squeezed.trace.equity.samples, fast.trace.equity.samples);
    assert.equal(`${squeezed.action}:${squeezed.amount}`, `${fast.action}:${fast.amount}`);
});

test('the sampler and the mixing roll share the shipped decision distribution', () => {
    // Pins the decision outcome, not just its determinism: an earlier attempt to
    // give the mixing roll its own random stream re-seeded every decision enough
    // to change ~4% of choices, and a paired benchmark against a fixed opponent
    // showed that stream losing across every alternative prefix.
    const decide = () => {
        const brain = new aiCore.UnifiedPokerAI({ name: 'T', style: 'SOLID', seatId: 1, profile: PROFILE });
        return brain.decide({
            handId: 1,
            decisionId: 'standard:flop:1:1',
            seed: 'distribution-pin',
            variant: 'standard',
            holeCards: [{ rank: 'Q', suit: 'hearts', value: 12 }, { rank: 'J', suit: 'diamonds', value: 11 }],
            communityCards: [
                { rank: 'Q', suit: 'spades', value: 12 },
                { rank: '7', suit: 'hearts', value: 7 },
                { rank: '2', suit: 'clubs', value: 2 }
            ],
            pot: 1000, currentBet: 0, toCall: 0, heroRoundBet: 0, heroStack: 19800,
            effectiveStack: 19800, numOpponents: 2, activeOpponentSeats: [2, 4], playerCount: 3,
            positionFromButton: 1, isSmallBlind: false, isBigBlind: false, preflopRaiseCount: 1,
            legalActions: ['check', 'raise'], canCheck: true, canRaise: true,
            minRaiseTo: 100, maxRaiseTo: 19800, bigBlind: 100, timeBudgetMs: 150,
            evaluateCards: (cards, variant) => ProbabilityCore.evaluate(cards, variant === 'shortdeck')
        });
    };
    const first = decide();
    const second = decide();
    assert.deepEqual(second, first, 'identical inputs must reproduce the identical decision');
    assert.equal(first.trace.equity.samples, 240, 'the intended sample count must complete');
    // A frozen fingerprint: changing any random-stream seeding without intending
    // to change behaviour breaks this with a readable diff.
    assert.equal(`${first.action}:${first.amount}:s${first.trace.equity.samples}`, 'raise:550:s240');
});

test('server AI equities use short-deck hand ranking without an injected global', () => {
    const evaluate = (cards) => ProbabilityCore.evaluate(cards, true);
    const flush = [
        { rank: 'A', suit: 'spades', value: 14 }, { rank: 'J', suit: 'spades', value: 11 },
        { rank: '9', suit: 'spades', value: 9 }, { rank: '7', suit: 'spades', value: 7 },
        { rank: '6', suit: 'spades', value: 6 }, { rank: '3', suit: 'spades', value: 3 },
        { rank: '2', suit: 'spades', value: 2 }
    ];
    const fullHouse = [
        { rank: 'K', suit: 'spades', value: 13 }, { rank: 'K', suit: 'hearts', value: 13 },
        { rank: 'K', suit: 'clubs', value: 13 }, { rank: 'Q', suit: 'spades', value: 12 },
        { rank: 'Q', suit: 'hearts', value: 12 }, { rank: '4', suit: 'clubs', value: 4 },
        { rank: '2', suit: 'diamonds', value: 2 }
    ];
    assert.ok(evaluate(flush).rank > evaluate(fullHouse).rank, 'flush must beat a full house in short deck');

    // The Node fallback used to reach for a standard-deck evaluator, which made
    // online short-deck AI decisions differ from the local game.
    const serverGame = new ServerPokerGame({
        maxPlayers: 2, isShortDeck: true, startingStack: 20000,
        smallBlind: 50, bigBlind: 100, random: Math.random
    });
    serverGame.players = [seat(0, { isHuman: true }), seat(1)];
    const ai = serverGame.players[1].aiRef;
    ai.isShortDeck = true;
    ai.bigBlind = 100;
    ai.holeCards = [{ rank: 'A', suit: 'spades', value: 14 }, { rank: 'K', suit: 'spades', value: 13 }];
    const baseState = {
        handId: 1, decisionId: 'shortdeck:flop:0:1', seed: 'regression', variant: 'shortdeck',
        communityCards: [
            { rank: 'Q', suit: 'spades', value: 12 },
            { rank: '9', suit: 'spades', value: 9 },
            { rank: '2', suit: 'clubs', value: 2 }
        ],
        pot: 600, currentBet: 0, toCall: 0, yourBet: 0, stack: 19800, effectiveStack: 19800,
        numOpponentsActive: 1, activeOpponentSeats: [2], playerCount: 2, dealerPosition: 0,
        currentPlayerIndex: 1, positionFromButton: 1, legalActions: ['check', 'raise'],
        canCheck: true, canRaise: true, minRaiseTo: 100, maxRaiseTo: 19800, preflopRaiseCount: 1,
        isPreviousStreetAggressor: true, isDelayedCBetCandidate: false,
        isSmallBlind: false, isBigBlind: false, bigBlind: 100, phase: 'flop', timeBudgetMs: 150
    };
    const withShared = ai.decide({ ...baseState, evaluateCards: (cards, variant) => serverRules.evaluateHand(cards, variant === 'shortdeck') });
    const withoutInjection = ai.decide(baseState);
    assert.deepEqual(withoutInjection, withShared);
});

test('a non 5-card straightHigh input is rejected instead of answered wrongly', () => {
    // 8,6,5,4,3,2 contains 6-5-4-3-2 but the 5-card window test cannot see it.
    const six = [8, 6, 5, 4, 3, 2];
    assert.equal(serverRules.straightHigh(six, false), false);
    assert.equal(serverRules.straightHigh([2, 3, 4, 5, 6], false), 6);
    assert.equal(serverRules.straightHigh([14, 2, 3, 4, 5], false), 5);
    assert.equal(serverRules.straightHigh([6, 7, 8, 9, 14], true), 9);
});

test('the two hand evaluators stay in agreement', () => {
    const deck = serverRules.createDeck(false);
    const rankOf = (card) => card.value;
    let compared = 0;
    for (let i = 0; i < deck.length - 6; i += 3) {
        const hand = deck.slice(i, i + 7);
        const a = serverRules.evaluateHand(hand, false);
        const b = ProbabilityCore.evaluate(hand, false);
        const c = ProbabilityCore.evaluate(hand, false);
        assert.equal(a.rank, b.rank, JSON.stringify(hand.map(rankOf)));
        assert.equal(a.score, b.score, JSON.stringify(hand.map(rankOf)));
        assert.deepEqual(c, b);
        compared++;
    }
    assert.ok(compared > 10);
});

test('side pot amounts always equal the total pot', () => {
    const players = [{ chipsInPot: 200 }, { chipsInPot: 500 }, { chipsInPot: 500 }, { chipsInPot: 100 }];
    const pots = PokerGameRules.calculateSidePots(players, 1300, players.slice(1));
    const total = pots.reduce((sum, pot) => sum + pot.amount, 0);
    assert.equal(total, 1300);
});

test('an uncalled wager is refunded before the pot is settled', () => {
    const refund = PokerGameRules.findUncalledRefund({ 0: 500, 1: 200, 2: 200 });
    assert.deepEqual(refund, { index: 0, refund: 300 });
    assert.equal(PokerGameRules.findUncalledRefund({ 0: 200, 1: 200 }), null);
});

test('server AI decisions stay inside a tight event-loop budget', () => {
    const build = (count) => {
        const game = new ServerPokerGame({
            maxPlayers: count, isShortDeck: false, startingStack: 20000,
            smallBlind: 40, bigBlind: 80, random: Math.random
        });
        game.players = [];
        game.players.push(seat(0, { isHuman: true }));
        for (let i = 1; i < count; i++) game.players.push(seat(i));
        const deck = serverRules.createDeck(false);
        let cursor = 0;
        for (const player of game.players) player.holeCards = [deck[cursor++], deck[cursor++]];
        game.communityCards = [deck[cursor++], deck[cursor++], deck[cursor++]];
        game.pot = 8000;
        game.phase = 'flop';
        game.currentBet = 0;
        game.dealerPos = 0;
        game.sbIdx = 1;
        game.bbIdx = 2;
        game.currentPlayerIdx = 1;
        game.roundBets = {};
        for (let i = 0; i < count; i++) game.roundBets[i] = 0;
        return game;
    };
    const samples = [];
    for (let i = 0; i < 8; i++) {
        const game = build(10);
        const started = process.hrtime.bigint();
        chooseServerAiAction(game, 1);
        samples.push(Number(process.hrtime.bigint() - started) / 1e6);
    }
    const worst = Math.max(...samples);
    // The enumeration evaluator needed 17-63ms per decision; the shared
    // bit-counting evaluator keeps the worst case an order of magnitude lower.
    assert.ok(worst < 25, `worst decision took ${worst.toFixed(1)}ms`);
});
