'use strict';

const assert = require('node:assert/strict');
const { ServerPokerGame, chooseServerAiAction, createServerAi } = require('../server/server-game');

function seededRandom(seed) {
    let state = seed >>> 0;
    return () => {
        state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
        return state / 4294967296;
    };
}

function percentile(values, ratio) {
    const ordered = [...values].sort((a, b) => a - b);
    return ordered[Math.min(ordered.length - 1, Math.floor(ordered.length * ratio))] || 0;
}

function runVariant(isShortDeck, hands = 12) {
    const stack = 200000;
    const players = Array.from({ length:4 }, (_, seat) => createServerAi(seat, stack));
    const game = new ServerPokerGame({
        isShortDeck,
        startingStack:stack,
        smallBlind:40,
        bigBlind:80,
        ante:40,
        minBet:80
    }, players, { random:seededRandom(isShortDeck ? 0x6badcafe : 0x51a7d00d) });
    const initialChips = players.length * stack;
    const timings = [];
    let actions = 0;

    for (let hand = 0; hand < hands; hand++) {
        assert.equal(game.startHand(), true, `hand ${hand + 1} could not start`);
        let guard = 0;
        while (game.phase !== 'idle') {
            assert.ok(++guard < 300, `hand ${hand + 1} stalled`);
            const idx = game.currentPlayerIdx;
            assert.ok(idx >= 0, `hand ${hand + 1} has no current player`);
            const legalBefore = game.legalActions(idx);
            const started = performance.now();
            const decision = chooseServerAiAction(game, idx);
            timings.push(performance.now() - started);
            assert.ok(decision, `AI seat ${idx} returned no decision`);
            assert.ok(legalBefore.actions.includes(decision.action),
                `${decision.action} is not in ${legalBefore.actions.join(',')}`);
            const result = game.act(idx, decision.action, decision.amount);
            assert.equal(result.ok, true, result.error);
            actions++;
        }
        assert.equal(players.reduce((sum, player) => sum + player.stack, 0), initialChips,
            `chip conservation failed after hand ${hand + 1}`);
    }

    return {
        variant:isShortDeck ? 'shortdeck' : 'standard',
        hands,
        actions,
        averageMs:timings.reduce((sum, value) => sum + value, 0) / timings.length,
        p95Ms:percentile(timings, .95),
        maxMs:Math.max(...timings)
    };
}

const reports = [runVariant(false), runVariant(true)];
for (const report of reports) {
    assert.ok(report.maxMs < 220, `${report.variant} blocked for ${report.maxMs.toFixed(1)}ms`);
    console.log(`${report.variant}: ${report.hands} hands, ${report.actions} actions, ` +
        `avg ${report.averageMs.toFixed(1)}ms, p95 ${report.p95Ms.toFixed(1)}ms, max ${report.maxMs.toFixed(1)}ms`);
}
