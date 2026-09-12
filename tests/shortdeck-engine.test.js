'use strict';

/**
 * Shared betting kernel + short-deck AI parity.
 *
 * Covers the task-1 acceptance list: side pots, uncalled-bet refunds,
 * flush ordering and the short-deck wheel must behave identically for both
 * variants where the rules agree, and must keep their real differences where
 * they do not.
 */

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const nodeEngine = require('../js/engine');
const nodeRules = require('../js/game-rules-core');

function loadBrowser() {
    const context = vm.createContext({
        console,
        setTimeout: () => 0,
        clearTimeout: () => {},
        performance: { now: () => 0 }
    });
    for (const relative of [
        'js/cards.js', 'js/game-rules-core.js', 'js/engine.js', 'js/probability-core.js',
        'js/probability.js', 'js/ai-core.js', 'js/ai.js', 'js/game.js', 'js/shortdeck.js'
    ]) {
        const filename = path.join(__dirname, '..', relative);
        vm.runInContext(fs.readFileSync(filename, 'utf8'), context, { filename });
    }
    return vm.runInContext(`({
        Card, evaluateHand, evaluateSDHand, compareSDHands, PokerEngine,
        PokerGame, ShortDeckGame, AIPlayer, ShortDeckAIPlayer,
        AI_STYLES, STYLE_CONFIGS, SD_STYLE_CONFIGS,
        PokerAICore, PokerProbabilityCore
    })`, context);
}

const browser = loadBrowser();

const STANDARD_OPTIONS = { startingStack: 5000, smallBlind: 40, bigBlind: 80 };
const SHORTDECK_OPTIONS = { startingStack: 5000, ante: 40, minBet: 80 };

const card = (rank, suit) => new browser.Card(rank, suit);

function seat(name, stack, holeCards, overrides = {}) {
    return {
        name, stack, holeCards, chipsInPot: 0, folded: false, isAllIn: false, hasActed: false,
        isHuman: false, lastAction: null, avatar: '🤖', ...overrides
    };
}

function gameOf(Game, options, players, overrides = {}) {
    const game = new Game(options);
    game.players = players;
    game.aiPlayers = players.filter(player => !player.isHuman);
    game.humanPlayer = players.find(player => player.isHuman) || null;
    game.phase = 'river';
    Object.assign(game, overrides);
    return game;
}

// Cross-realm safe: objects returned from a vm context carry that realm's
// prototypes, so compare plain JSON copies instead of live objects.
const potShape = pots => JSON.parse(JSON.stringify(
    pots.map(pot => ({
        amount: pot.amount,
        eligible: pot.eligible.map(player => player.name).sort()
    }))
));

test('js/engine.js is usable from Node and from the browser and matches game-rules-core', () => {
    assert.equal(typeof nodeEngine.executeAction, 'function');
    assert.equal(typeof browser.PokerEngine.executeAction, 'function');
    assert.equal(nodeEngine.minBetUnit({ bigBlind: 100 }), 100);      // standard table
    assert.equal(nodeEngine.minBetUnit({ minBet: 80, bigBlind: 80 }), 80); // short deck

    const contributors = [
        { name: 'A', chipsInPot: 100 },
        { name: 'B', chipsInPot: 200 },
        { name: 'C', chipsInPot: 200, folded: true }
    ];
    const plainGame = {
        players: contributors,
        pot: 500,
        getPlayersInHand() { return contributors.filter(player => !player.folded); }
    };
    const expected = potShape(nodeRules.calculateSidePots(contributors, 500, plainGame.getPlayersInHand()));
    assert.deepEqual(potShape(nodeEngine.calculateSidePots(plainGame)), expected);
    assert.deepEqual(potShape(browser.PokerEngine.calculateSidePots(plainGame)), expected);
});

test('ShortDeckGame delegates its betting kernel to the shared engine', () => {
    const engine = browser.PokerEngine;
    const calls = [];
    const original = engine.canPlayerRaise;
    engine.canPlayerRaise = (game, playerIndex) => {
        calls.push({ game, playerIndex });
        return original(game, playerIndex);
    };
    try {
        const players = [seat('A', 1000, []), seat('B', 1000, [])];
        const game = gameOf(browser.ShortDeckGame, SHORTDECK_OPTIONS, players, { phase: 'flop' });
        assert.equal(game.canPlayerRaise(1), true);
        assert.equal(calls.length, 1);
        assert.equal(calls[0].game, game);
        assert.equal(calls[0].playerIndex, 1);
    } finally {
        engine.canPlayerRaise = original;
    }
});

test('side pots include folded chips and are identical for both variants', () => {
    const build = (Game, options) => {
        const players = [
            seat('A', 0, [], { chipsInPot: 100 }),
            seat('B', 0, [], { chipsInPot: 200 }),
            seat('C', 0, [], { chipsInPot: 200, folded: true })
        ];
        return gameOf(Game, options, players, { pot: 500, phase: 'flop', communityCards: [] });
    };
    const standard = build(browser.PokerGame, STANDARD_OPTIONS).calculateSidePots();
    const shortDeck = build(browser.ShortDeckGame, SHORTDECK_OPTIONS).calculateSidePots();
    const expected = [
        { amount: 300, eligible: ['A', 'B'] },
        { amount: 200, eligible: ['B'] }
    ];
    assert.deepEqual(potShape(standard), expected);
    assert.deepEqual(potShape(shortDeck), expected);
});

test('short-deck showdown pays the main pot and the side pot from all-in stacks', () => {
    const board = [
        card('9', 'spades'), card('6', 'spades'), card('7', 'spades'),
        card('9', 'hearts'), card('6', 'hearts')
    ];
    const players = [
        seat('Flush', 0, [card('A', 'spades'), card('Q', 'spades')], { chipsInPot: 100, isAllIn: true }),
        seat('FullHouse', 0, [card('9', 'diamonds'), card('6', 'diamonds')], { chipsInPot: 300, isAllIn: true }),
        seat('TwoPair', 0, [card('K', 'clubs'), card('K', 'diamonds')], { chipsInPot: 300, isAllIn: true })
    ];
    const game = gameOf(browser.ShortDeckGame, SHORTDECK_OPTIONS, players, {
        pot: 700, communityCards: board, currentBet: 0
    });
    let announced = null;
    game.onHandEnd = result => { announced = result; };
    game.showdown();

    // Main pot 300 (100 × 3) goes to the flush, side pot 400 (200 × 2) to the
    // full house: short deck ranks the flush above the full house.
    assert.equal(players[0].stack, 300);
    assert.equal(players[1].stack, 400);
    assert.equal(players[2].stack, 0);
    assert.equal(announced.handName, '同花');
    assert.equal(announced.pot, 700);
    assert.equal(game.pot, 0);
    assert.equal(game.phase, 'idle');
});

test('an uncalled wager is refunded identically by both variants', () => {
    const build = (Game, options) => {
        const players = [
            seat('A', 200, [], { chipsInPot: 300 }),
            seat('B', 400, [], { chipsInPot: 100 }),
            seat('C', 400, [], { chipsInPot: 100, folded: true })
        ];
        const game = gameOf(Game, options, players, {
            phase: 'river', pot: 500, currentBet: 300, roundBets: { 0: 300, 1: 100, 2: 100 }
        });
        return { game, players };
    };
    const standard = build(browser.PokerGame, STANDARD_OPTIONS);
    const shortDeck = build(browser.ShortDeckGame, SHORTDECK_OPTIONS);
    const standardRefund = standard.game.refundUncalledBet();
    const shortDeckRefund = shortDeck.game.refundUncalledBet();

    assert.equal(standardRefund, 200);
    assert.equal(shortDeckRefund, 200);
    assert.deepEqual(
        shortDeck.players.map(player => [player.stack, player.chipsInPot]),
        standard.players.map(player => [player.stack, player.chipsInPot])
    );
    assert.deepEqual(shortDeck.game.roundBets, standard.game.roundBets);
    assert.equal(shortDeck.game.pot, standard.game.pot);
    assert.equal(shortDeck.game.pot, 300);
    assert.equal(shortDeck.game.currentBet, standard.game.currentBet);
    assert.equal(shortDeck.game.currentBet, 100);
});

test('a short-deck fold win returns the uncalled portion of the last bet', () => {
    const players = [seat('A', 1000, [], { isHuman: true }), seat('B', 1000, []), seat('C', 1000, [])];
    const game = gameOf(browser.ShortDeckGame, SHORTDECK_OPTIONS, players, {
        phase: 'flop', pot: 0, currentBet: 0, roundBets: {}
    });
    let announced = null;
    game.onHandEnd = result => { announced = result; };

    game.executeAction(2, 'raise', 400); // C bets 400, nobody calls
    game.executeAction(1, 'fold');
    game.executeAction(0, 'fold');
    assert.equal(game.pot, 400);

    assert.equal(game.checkHandEnd(), true);
    assert.equal(players[2].stack, 1000);   // the uncalled 400 came back
    assert.equal(players[2].chipsInPot, 0);
    assert.equal(game.pot, 0);
    assert.equal(announced.pot, 0);
    assert.equal(announced.reason, 'all folded');
});

test('flush versus full house: short deck flips the winner, standard does not', () => {
    const board = [
        card('9', 'spades'), card('6', 'spades'), card('7', 'spades'),
        card('9', 'hearts'), card('6', 'hearts')
    ];
    const holeFlush = [card('A', 'spades'), card('Q', 'spades')];
    const holeFullHouse = [card('9', 'diamonds'), card('6', 'diamonds')];
    const build = (Game, options) => {
        const players = [
            seat('Flush', 0, holeFlush, { chipsInPot: 500, isAllIn: true }),
            seat('FullHouse', 0, holeFullHouse, { chipsInPot: 500, isAllIn: true })
        ];
        const game = gameOf(Game, options, players, { pot: 1000, communityCards: board, currentBet: 0 });
        let announced = null;
        game.onHandEnd = result => { announced = result; };
        game.showdown();
        return { players, announced };
    };
    const standard = build(browser.PokerGame, STANDARD_OPTIONS);
    const shortDeck = build(browser.ShortDeckGame, SHORTDECK_OPTIONS);

    assert.deepEqual([standard.players[0].stack, standard.players[1].stack], [0, 1000]);
    assert.equal(standard.announced.handName, '葫芦');
    assert.deepEqual([shortDeck.players[0].stack, shortDeck.players[1].stack], [1000, 0]);
    assert.equal(shortDeck.announced.handName, '同花');
});

test('A-6-7-8-9 is a short-deck wheel but only a high card in standard poker', () => {
    const wheel = [card('A', 'spades'), card('6', 'clubs'), card('7', 'diamonds'), card('8', 'hearts'), card('9', 'spades')];
    const tenHigh = [card('6', 'clubs'), card('7', 'diamonds'), card('8', 'hearts'), card('9', 'spades'), card('10', 'clubs')];

    const standardWheel = browser.evaluateHand(wheel);
    const shortDeckWheel = browser.evaluateSDHand(wheel);
    assert.equal(standardWheel.rank, 0);          // high card — A-6-7-8-9 is not consecutive
    assert.equal(shortDeckWheel.rank, 4);         // straight in short deck
    assert.equal(shortDeckWheel.name, '顺子');
    assert.equal(browser.compareSDHands(shortDeckWheel, browser.evaluateSDHand(tenHigh)), -1);
});

test('the short-deck wheel beats three of a kind at showdown, standard high card does not', () => {
    const board = [card('7', 'clubs'), card('8', 'diamonds'), card('9', 'spades'), card('K', 'hearts'), card('6', 'diamonds')];
    const holeWheel = [card('A', 'spades'), card('Q', 'clubs')];
    const holeTrips = [card('K', 'spades'), card('K', 'diamonds')];
    const build = (Game, options) => {
        const players = [
            seat('Wheel', 0, holeWheel, { chipsInPot: 400, isAllIn: true }),
            seat('Trips', 0, holeTrips, { chipsInPot: 400, isAllIn: true })
        ];
        const game = gameOf(Game, options, players, { pot: 800, communityCards: board, currentBet: 0 });
        let announced = null;
        game.onHandEnd = result => { announced = result; };
        game.showdown();
        return { players, announced };
    };
    const shortDeck = build(browser.ShortDeckGame, SHORTDECK_OPTIONS);
    assert.deepEqual([shortDeck.players[0].stack, shortDeck.players[1].stack], [800, 0]);
    assert.equal(shortDeck.announced.handName, '顺子');

    const standard = build(browser.PokerGame, STANDARD_OPTIONS);
    assert.deepEqual([standard.players[0].stack, standard.players[1].stack], [0, 800]);
    assert.equal(standard.announced.handName, '三条');
});

test('ShortDeckAIPlayer feeds one config source to the worker and to the in-process brain', () => {
    for (const style of Object.values(browser.AI_STYLES)) {
        const ai = new browser.ShortDeckAIPlayer('SD', style, 20000, 1);
        assert.equal(ai.config, browser.SD_STYLE_CONFIGS[style]);
        assert.equal(ai.sharedBrain.profile.vpip, ai.config.vpip);
        assert.equal(ai.sharedBrain.style, style);
        assert.equal(ai.isShortDeck, true);
    }
    // The fork this test pins: the standard table keeps its own table, and the
    // two tables really do differ for several styles.
    const standard = new browser.AIPlayer('STD', browser.AI_STYLES.MANIAC, 20000, 1);
    assert.equal(standard.config, browser.STYLE_CONFIGS[browser.AI_STYLES.MANIAC]);
    assert.notEqual(browser.SD_STYLE_CONFIGS[browser.AI_STYLES.MANIAC].vpip, standard.config.vpip);
    assert.equal(new browser.ShortDeckAIPlayer('SD', browser.AI_STYLES.MANIAC, 20000, 1).config.vpip, 0.38);
});

test('ShortDeckAIPlayer always evaluates with the short-deck ranking', () => {
    const state = {
        handId: 3, decisionId: 'shortdeck:flop:7:1', seed: 'poker-table:shortdeck',
        variant: 'shortdeck',
        communityCards: [card('9', 'spades'), card('6', 'spades'), card('7', 'spades')],
        pot: 640, currentBet: 160, toCall: 160, yourBet: 0, stack: 6000, effectiveStack: 6000,
        numOpponentsActive: 2, activeOpponentSeats: [2, 3], playerCount: 3, currentPlayerIndex: 1,
        positionFromButton: 2, legalActions: ['fold', 'call', 'raise', 'allin'], canRaise: true,
        canCheck: false, minRaiseTo: 320, maxRaiseTo: 6000, bigBlind: 80, phase: 'flop', timeBudgetMs: 80
    };
    const hole = [card('A', 'spades'), card('K', 'spades')];
    const bogusEvaluator = () => ({ rank: 0, score: 0, name: '高牌' });
    const decide = (AIClass, extra) => {
        const ai = new AIClass('Parity', browser.AI_STYLES.SOLID, 6000, 1);
        ai.holeCards = hole;
        const decision = ai.decide({ ...state, ...extra });
        return { decision, equity: ai.lastDecisionTrace.equity };
    };

    // A standard evaluator handed in by an adapter must not win over the
    // short-deck ranking the class owns.
    const real = decide(browser.ShortDeckAIPlayer, {});
    const withBogus = decide(browser.ShortDeckAIPlayer, { evaluateCards: bogusEvaluator });
    assert.deepEqual(withBogus.decision, real.decision);
    assert.deepEqual(withBogus.equity, real.equity);
    assert.ok(real.equity.samples > 0);

    // Control: the same bogus evaluator really does change a plain AIPlayer's
    // equity, so the assertion above is able to detect the regression.
    const control = decide(browser.AIPlayer, { evaluateCards: bogusEvaluator });
    assert.notEqual(control.equity.mean, real.equity.mean);
});

test('short-deck AI decisions match the worker brain for the same public state', () => {
    const state = {
        handId: 5, decisionId: 'shortdeck:turn:9:1', seed: 'poker-table:shortdeck',
        variant: 'shortdeck',
        communityCards: [
            card('9', 'spades'), card('6', 'spades'), card('7', 'spades'), card('K', 'hearts')
        ],
        pot: 900, currentBet: 0, toCall: 0, yourBet: 0, stack: 4600, effectiveStack: 4600,
        numOpponentsActive: 2, activeOpponentSeats: [2, 3], playerCount: 3, currentPlayerIndex: 1,
        positionFromButton: 2, legalActions: ['check', 'raise', 'allin'], canRaise: true,
        canCheck: true, minRaiseTo: 160, maxRaiseTo: 4600, bigBlind: 80, phase: 'turn', timeBudgetMs: 80
    };
    const hole = [card('A', 'spades'), card('K', 'spades')];

    // Main-thread / server path: AIPlayer.decide -> _decideShared.
    const ai = new browser.ShortDeckAIPlayer('Parity', browser.AI_STYLES.SOLID, 6000, 1);
    ai.holeCards = hole;
    const mainThread = ai.decide(state);

    // Worker path replica: js/ai-worker.js builds the brain from meta
    // {name, style, seatId, profile: config} and injects the probability core.
    const brain = new browser.PokerAICore.UnifiedPokerAI({
        name: ai.name, style: ai.style, seatId: state.currentPlayerIndex, profile: ai.config
    });
    const worker = brain.decide({
        ...state,
        variant: 'shortdeck',
        heroSeat: state.currentPlayerIndex,
        holeCards: hole,
        heroRoundBet: state.yourBet,
        heroStack: state.stack,
        evaluateCards: (cards, variant) => browser.PokerProbabilityCore.evaluate(cards, variant === 'shortdeck')
    });

    assert.equal(mainThread.action, worker.action);
    assert.equal(mainThread.amount, worker.amount);
    assert.deepEqual(
        JSON.parse(JSON.stringify(ai.lastDecisionTrace.equity)),
        JSON.parse(JSON.stringify(worker.trace.equity))
    );
});

test('a page that forgot js/engine.js fails with a precise message', () => {
    const context = vm.createContext({
        console, setTimeout: () => 0, clearTimeout: () => {}, performance: { now: () => 0 }
    });
    for (const relative of [
        'js/cards.js', 'js/game-rules-core.js', 'js/probability-core.js',
        'js/probability.js', 'js/ai-core.js', 'js/ai.js', 'js/game.js', 'js/shortdeck.js'
    ]) {
        const filename = path.join(__dirname, '..', relative);
        vm.runInContext(fs.readFileSync(filename, 'utf8'), context, { filename });
    }
    const error = vm.runInContext(`(() => {
        const game = new ShortDeckGame({ startingStack: 1000, ante: 40, minBet: 80 });
        game.players = [
            { name: 'A', stack: 1000, holeCards: [], chipsInPot: 0, folded: false, isAllIn: false, isHuman: true },
            { name: 'B', stack: 1000, holeCards: [], chipsInPot: 0, folded: false, isAllIn: false, isHuman: false }
        ];
        try { game.canPlayerRaise(1); } catch (thrown) { return thrown.message; }
        return null;
    })()`, context);
    assert.match(error || '', /js\/engine\.js must be loaded before js\/shortdeck\.js/);
});
