'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const probability = require('../js/probability-core');

const card = (rank, suit) => ({ rank, suit });

test('shared evaluator preserves standard and short-deck ranking differences', () => {
    const flush = [card('A','spades'), card('Q','spades'), card('10','spades'), card('8','spades'), card('6','spades')];
    const fullHouse = [card('K','spades'), card('K','hearts'), card('K','diamonds'), card('Q','spades'), card('Q','hearts')];
    assert.ok(probability.evaluate(fullHouse, false).score > probability.evaluate(flush, false).score);
    assert.ok(probability.evaluate(flush, true).score > probability.evaluate(fullHouse, true).score);
    assert.equal(probability.evaluate([
        card('A','spades'), card('6','hearts'), card('7','diamonds'), card('8','clubs'), card('9','spades')
    ], true).name, '顺子');
});

test('preflop equity responds to the real opponent count', () => {
    const options = {
        playerCards:[card('A','spades'), card('A','hearts')],
        communityCards:[],
        simulations:5000,
        minSamples:5000,
        timeBudgetMs:2000,
        seed:'aces-opponent-count'
    };
    const headsUp = probability.calculate({ ...options, numOpponents:1 });
    const sixWay = probability.calculate({ ...options, numOpponents:5 });
    assert.ok(headsUp.winProb > .78, `heads-up AA was ${headsUp.winProb}`);
    assert.ok(sixWay.winProb < headsUp.winProb - .25, `six-way AA was ${sixWay.winProb}`);
});

test('river heads-up calculation is exact and tie equity is split', () => {
    const result = probability.calculate({
        playerCards:[card('2','clubs'), card('3','diamonds')],
        communityCards:[card('10','spades'), card('J','spades'), card('Q','spades'), card('K','spades'), card('A','spades')],
        numOpponents:1,
        isShortDeck:false
    });
    assert.equal(result.method, 'exact-heads-up');
    assert.equal(result.tieProb, 1);
    assert.equal(result.equityProb, .5);
    assert.equal(result.samples, 990);
});

test('improvement cards are hidden preflop and ordinary high-card changes are excluded', () => {
    const hole = [card('A','spades'), card('K','spades')];
    const preflop = probability.findImprovements(hole, [], false);
    assert.equal(preflop.totalOuts, 0);

    const flop = probability.findImprovements(hole, [card('Q','spades'), card('2','spades'), card('7','diamonds')], false);
    assert.equal(flop.byHandType['高牌'], undefined);
    assert.equal(flop.byHandType['同花'].strongCount, 9);
    assert.ok(flop.strongOuts >= 9);
});

test('nine-opponent analysis stays comfortably off the foreground budget', () => {
    const started = performance.now();
    const result = probability.calculate({
        playerCards:[card('A','spades'), card('K','hearts')],
        communityCards:[card('Q','diamonds'), card('9','clubs'), card('2','spades')],
        numOpponents:9,
        simulations:1800,
        minSamples:1800,
        timeBudgetMs:2000,
        seed:'probability-performance',
        isShortDeck:false
    });
    const elapsed = performance.now() - started;
    assert.equal(result.samples, 1800);
    assert.ok(elapsed < 650, `analysis took ${elapsed.toFixed(1)}ms`);
});
