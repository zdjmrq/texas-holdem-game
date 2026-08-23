'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const core = require('../js/ai-core');
const { AIPlayer, AI_STYLES } = require('../js/ai');
const { ServerPokerGame, chooseServerAiAction, createServerAi } = require('../server/server-game');
const { evaluateHand, VALUES } = require('../server/poker-rules');

function card(rank, suit) {
    return { rank, suit, value: VALUES[rank] };
}

function evaluator(cards, variant) {
    return evaluateHand(cards, variant === 'shortdeck');
}

function postflopState(overrides = {}) {
    return {
        handId:12,
        decisionId:'flop:4:1',
        seed:'parity-table',
        variant:'standard',
        communityCards:[card('Q','clubs'), card('9','diamonds'), card('4','clubs')],
        pot:640,
        currentBet:240,
        toCall:240,
        yourBet:0,
        stack:6000,
        effectiveStack:5200,
        numOpponentsActive:2,
        activeOpponentSeats:[2,3],
        playerCount:3,
        currentPlayerIndex:1,
        positionFromButton:2,
        legalActions:['fold','call','raise','allin'],
        canRaise:true,
        canCheck:false,
        minRaiseTo:480,
        maxRaiseTo:6000,
        bigBlind:80,
        phase:'flop',
        timeBudgetMs:80,
        evaluateCards:evaluator,
        ...overrides
    };
}

test('shared strategy is deterministic for identical public information and seed', () => {
    const a = new AIPlayer('Parity', AI_STYLES.SOLID, 6000, 1);
    const b = new AIPlayer('Parity', AI_STYLES.SOLID, 6000, 1);
    a.holeCards = [card('A','spades'), card('Q','hearts')];
    b.holeCards = [card('A','spades'), card('Q','hearts')];
    const da = a.decide(postflopState());
    const db = b.decide(postflopState());
    assert.deepEqual(da, db);
    assert.equal(a.lastDecisionTrace.strategyVersion, 'unified-v1');
    assert.deepEqual(a.lastDecisionTrace.equity, b.lastDecisionTrace.equity);
});

test('online AI is an adapter around the same AIPlayer and shared core', () => {
    const players = [createServerAi(0, 20000), createServerAi(1, 20000), createServerAi(2, 20000)];
    assert.ok(players.every(player => player.aiRef instanceof AIPlayer));
    const game = new ServerPokerGame({
        startingStack:20000, smallBlind:40, bigBlind:80,
        ante:40, minBet:80, isShortDeck:false
    }, players, { random:() => 0.371 });
    assert.equal(game.startHand(), true);
    const idx = game.currentPlayerIdx;
    const decision = chooseServerAiAction(game, idx);
    assert.ok(game.legalActions(idx).actions.includes(decision.action));
    assert.equal(players[idx].aiRef.lastDecisionTrace.strategyVersion, 'unified-v1');
});

test('range-weighted equity falls against a tighter, stronger range', () => {
    const context = core.normalizedContext({
        variant:'standard',
        holeCards:[card('A','spades'), card('Q','hearts')],
        communityCards:[card('Q','clubs'), card('9','diamonds'), card('4','clubs')],
        activeOpponentSeats:[2],
        numOpponentsActive:1,
        pot:500,
        stack:5000,
        evaluateCards:evaluator,
        timeBudgetMs:180,
        mcSamples:700
    });
    const tight = new core.OpponentBelief(2);
    tight.rangeWidth = 0.10;
    tight.strengthShift = 0.15;
    const loose = new core.OpponentBelief(2);
    loose.rangeWidth = 0.75;
    loose.strengthShift = -0.05;
    const vsTight = core.estimateRangeEquity(context, new Map([['2', tight]]), new core.SeededRng('range-test'));
    const vsLoose = core.estimateRangeEquity(context, new Map([['2', loose]]), new core.SeededRng('range-test'));
    assert.ok(vsLoose.mean > vsTight.mean + 0.08, `${vsLoose.mean} should exceed ${vsTight.mean}`);
    assert.ok(vsTight.samples >= 56 && vsLoose.samples >= 56);
});

test('opponent response model bluffs less into callers and more into over-folders', () => {
    const folder = new core.UnifiedPokerAI({ style:'SOLID', seatId:1 });
    const caller = new core.UnifiedPokerAI({ style:'SOLID', seatId:1 });
    for (let hand = 1; hand <= 14; hand++) {
        folder.observeAction({ seatId:2, handId:hand, street:'flop', action:'fold',
            meta:{ toCallBefore:100, betFraction:.62 } });
        caller.observeAction({ seatId:2, handId:hand, street:'flop', action:'call',
            meta:{ toCallBefore:100, betFraction:.62 } });
    }
    folder.beginHand(99);
    caller.beginHand(99);
    const context = core.normalizedContext(postflopState({
        handId:99, currentPlayerIndex:1, activeOpponentSeats:[2], numOpponentsActive:1
    }));
    const texture = core.analyzeTexture(context.communityCards, context.holeCards, context.variant);
    const target = 720;
    const folderResponse = folder.responseForRaise(context, target, .31, texture, folder.activeReads(context));
    const callerResponse = caller.responseForRaise(context, target, .31, texture, caller.activeReads(context));
    assert.ok(folderResponse.allFold > callerResponse.allFold + .12);
    assert.equal(folder.ensureOpponent(2).model.foldsTooMuch, true);
    assert.equal(caller.ensureOpponent(2).model.callsTooMuch, true);
});

test('bounded mixing never selects a strategically dominated candidate', () => {
    const brain = new core.UnifiedPokerAI({ style:'LAG', seatId:1 });
    const context = core.normalizedContext(postflopState({ pot:1000, toCall:0, currentBet:0 }));
    for (let seed = 0; seed < 80; seed++) {
        const candidates = [
            { action:'check', amount:0, invest:0, utility:100, ev:100 },
            { action:'raise', amount:320, invest:320, utility:96, ev:101 },
            { action:'allin', amount:6000, invest:6000, utility:-900, ev:-700 }
        ];
        const choice = brain.selectCandidate(candidates, context, new core.SeededRng(seed), .5);
        assert.notEqual(choice.action, 'allin');
    }
});

test('six-way complex decisions stay inside the foreground performance envelope', () => {
    const ai = new AIPlayer('Performance', AI_STYLES.SOLID, 20000, 0);
    ai.holeCards = [card('J','spades'), card('10','spades')];
    const started = performance.now();
    const decision = ai.decide(postflopState({
        handId:31,
        decisionId:'six-way',
        seed:'performance',
        communityCards:[card('9','spades'), card('8','hearts'), card('2','clubs')],
        activeOpponentSeats:[1,2,3,4,5],
        numOpponentsActive:5,
        playerCount:6,
        currentPlayerIndex:0,
        pot:1200,
        currentBet:300,
        toCall:300,
        minRaiseTo:600,
        stack:20000,
        maxRaiseTo:20000,
        timeBudgetMs:150
    }));
    const elapsed = performance.now() - started;
    assert.ok(['fold','call','raise','allin'].includes(decision.action));
    assert.ok(elapsed < 220, `decision blocked for ${elapsed.toFixed(1)}ms`);
    assert.ok(ai.lastDecisionTrace.equity.samples >= 40);
});
