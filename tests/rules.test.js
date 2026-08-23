'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { evaluateHand: evaluateServerHand, VALUES } = require('../server/poker-rules');
const { ServerPokerGame } = require('../server/server-game');
const { PokerRoomServer } = require('../server/local-server');

function card(rank, suit = 'spades') {
    return { rank, suit, value: VALUES[rank] };
}

function player(id, seatId, stack = 1000) {
    return {
        id, seatId, name:id, avatar:'👤', isHuman:true, connected:true, left:false,
        stack, holeCards:[], chipsInPot:0, roundBet:0, folded:false,
        isAllIn:false, dealtIn:true, lastAction:null
    };
}

function loadBrowserRules() {
    const context = vm.createContext({
        console,
        setTimeout: () => 0,
        clearTimeout: () => {},
        performance: { now: () => 0 }
    });
    for (const relative of ['js/cards.js', 'js/probability.js', 'js/ai.js', 'js/game.js', 'js/shortdeck.js']) {
        const filename = path.join(__dirname, '..', relative);
        vm.runInContext(fs.readFileSync(filename, 'utf8'), context, { filename });
    }
    return vm.runInContext(`({
        Card, evaluateHand, evaluateSDHand, ProbabilityCalculator,
        PokerGame, ShortDeckGame, AIPlayer, AI_STYLES, PlayerModel,
        analyzeBoardTexture, getPositionCategoryFromButton
    })`, context);
}

const browser = loadBrowserRules();

test('standard and short-deck wheel straights use the correct high card', () => {
    const standardWheel = evaluateServerHand([
        card('A'), card('2','hearts'), card('3','diamonds'), card('4','clubs'), card('5','hearts')
    ], false);
    const sixHigh = evaluateServerHand([
        card('2'), card('3','hearts'), card('4','diamonds'), card('5','clubs'), card('6','hearts')
    ], false);
    assert.equal(standardWheel.rank, 4);
    assert.ok(sixHigh.score > standardWheel.score);

    const shortWheel = evaluateServerHand([
        card('A'), card('6','hearts'), card('7','diamonds'), card('8','clubs'), card('9','hearts')
    ], true);
    const tenHigh = evaluateServerHand([
        card('6'), card('7','hearts'), card('8','diamonds'), card('9','clubs'), card('10','hearts')
    ], true);
    assert.equal(shortWheel.rank, 4);
    assert.ok(tenHigh.score > shortWheel.score);
});

test('suited wheel is not royal and short deck keeps flush above full house', () => {
    const suitedWheel = browser.evaluateHand(['A','2','3','4','5'].map(rank => new browser.Card(rank, 'spades')));
    assert.equal(suitedWheel.rank, 8);

    const shortSuitedWheel = browser.evaluateSDHand(['A','6','7','8','9'].map(rank => new browser.Card(rank, 'hearts')));
    assert.equal(shortSuitedWheel.rank, 8);
    const flush = browser.evaluateSDHand(['A','K','J','8','6'].map(rank => new browser.Card(rank, 'clubs')));
    const fullHouse = browser.evaluateSDHand([
        new browser.Card('A','spades'), new browser.Card('A','hearts'), new browser.Card('A','diamonds'),
        new browser.Card('K','spades'), new browser.Card('K','hearts')
    ]);
    assert.ok(flush.score > fullHouse.score);
});

test('split-pot equity is divided by every tied player', () => {
    const board = ['10','J','Q','K','A'].map(rank => new browser.Card(rank, 'spades'));
    const hero = [new browser.Card('2','clubs'), new browser.Card('3','diamonds')];
    const result = browser.ProbabilityCalculator.calculateWinProb(hero, board, 3, 40);
    assert.equal(result.tieProb, 1);
    assert.equal(result.equityProb, 0.25);
});

test('side pots contain folded contributions', () => {
    const players = [player('A',0), player('B',1), player('C',2)];
    players[0].chipsInPot = 100;
    players[1].chipsInPot = 200;
    players[2].chipsInPot = 200;
    players[2].folded = true;
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:40, bigBlind:80 }, players);
    game.pot = 500;
    const pots = game.calculateSidePots();
    assert.deepEqual(pots.map(pot => pot.amount), [300, 200]);
    assert.deepEqual(pots.map(pot => pot.eligible.map(item => item.id)), [['A','B'], ['B']]);
});

test('cumulative short all-ins reopen A but not C', () => {
    const players = ['A','B','C','D','E'].map((id, seat) => player(id, seat));
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:50, bigBlind:100 }, players);
    game.phase = 'preflop';
    game.currentBet = 200;
    game.lastRaise = 100;
    game.hasFullBetThisRound = true;
    game.roundBets = { 0:100, 1:125, 2:125, 3:200, 4:200 };
    game.playersActed = new Set([0,1,2,3,4]);
    game.actedAtBet = { 0:100, 1:125, 2:125, 3:200, 4:200 };
    game.raiseSizeAtAction = { 0:100, 1:100, 2:100, 3:100, 4:100 };
    assert.equal(game.canRaise(0), true);
    assert.equal(game.canRaise(2), false);
    assert.equal(game.minRaiseTo(), 300);
});

test('short stack can make a sub-minimum all-in but cannot submit an impossible raise', () => {
    const players = [player('A',0,40), player('B',1,1000)];
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:40, bigBlind:80 }, players);
    game.phase = 'preflop';
    game.currentPlayerIdx = 0;
    game.currentBet = 0;
    game.roundBets = {};
    const legal = game.legalActions(0);
    assert.ok(legal.actions.includes('allin'));
    assert.ok(!legal.actions.includes('raise'));
});

test('a unique uncalled wager is returned before a fold pot is awarded', () => {
    const players = [player('A',0,900), player('B',1,900)];
    players[0].chipsInPot = 100;
    players[1].chipsInPot = 100;
    players[1].folded = true;
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:40, bigBlind:80 }, players);
    game.phase = 'flop';
    game.pot = 200;
    game.currentBet = 100;
    game.roundBets = { 0:100, 1:0 };
    game.finishByFold(players[0]);
    assert.equal(game.lastHandResult.pot, 100);
    assert.equal(players[0].stack, 1100);
    assert.equal(players[0].chipsInPot, 0);
    assert.equal(players[0].stack + players[1].stack, 2000);
});

test('AI respects real stack/raise bounds and executes a remembered check-raise plan', () => {
    const ai = new browser.AIPlayer('Test', browser.AI_STYLES.SOLID, 50, 1);
    ai.holeCards = [new browser.Card('A','spades'), new browser.Card('A','hearts')];
    let decision = ai.decide({
        communityCards:[], pot:120, currentBet:0, toCall:0, yourBet:0,
        stack:50, effectiveStack:50, numOpponentsActive:1, bigBlind:80,
        minRaiseTo:80, maxRaiseTo:50, canRaise:true, canCheck:true,
        positionFromButton:0, preflopRaiseCount:0, phase:'preflop'
    });
    assert.ok(decision.amount <= 50);
    const partialAllInTargets = ai._getRaiseSizes(300, 100, 150, false, 3, {
        ownBet:0, currentBet:100, minRaiseTo:200, maxRaiseTo:150,
        communityCards:[new browser.Card('A','clubs'), new browser.Card('10','clubs'), new browser.Card('9','hearts')]
    });
    assert.deepEqual(Array.from(partialAllInTargets), [150]);

    ai.stack = 1000;
    ai._estimateEquity = () => 0.82;
    ai.streetPlan = { type:'check_raise', street:3, minEquity:0.6 };
    decision = ai.decide({
        communityCards:[new browser.Card('A','clubs'), new browser.Card('10','clubs'), new browser.Card('9','hearts')],
        pot:300, currentBet:100, toCall:100, yourBet:0, stack:1000, effectiveStack:1000,
        numOpponentsActive:1, bigBlind:80, minRaiseTo:200, maxRaiseTo:1000,
        canRaise:true, canCheck:false, positionFromButton:0, phase:'flop'
    });
    assert.ok(['raise','allin'].includes(decision.action));
    if (decision.action === 'raise') assert.ok(decision.amount >= 200 && decision.amount <= 1000);
});

test('blind-position AI still evaluates postflop value bets', () => {
    const ai = new browser.AIPlayer('Blind', browser.AI_STYLES.SOLID, 1000, 1);
    ai.holeCards = [new browser.Card('A','spades'), new browser.Card('A','hearts')];
    ai._estimateEquity = () => 0.95;
    let reachedPostflopStrategy = false;
    ai._applyHeuristics = candidates => {
        reachedPostflopStrategy = true;
        return candidates.find(candidate => candidate.action === 'raise');
    };
    const decision = ai.decide({
        communityCards:[new browser.Card('A','clubs'), new browser.Card('7','diamonds'), new browser.Card('2','hearts')],
        pot:300, currentBet:0, toCall:0, yourBet:0, stack:1000, effectiveStack:1000,
        numOpponentsActive:1, bigBlind:80, minRaiseTo:80, maxRaiseTo:1000,
        canRaise:true, canCheck:true, isBigBlind:true, positionFromButton:1, phase:'flop'
    });
    assert.equal(reachedPostflopStrategy, true);
    assert.equal(decision.action, 'raise');
});

test('AI recognizes short-deck wheel draws and heads-up button position', () => {
    const hole = [new browser.Card('A','spades'), new browser.Card('6','hearts')];
    const board = [new browser.Card('7','clubs'), new browser.Card('8','diamonds'), new browser.Card('K','hearts')];
    assert.equal(browser.analyzeBoardTexture(board, hole, false).hasStraightDraw, false);
    assert.equal(browser.analyzeBoardTexture(board, hole, true).hasStraightDraw, true);
    assert.equal(browser.getPositionCategoryFromButton(0, 2, true, false, false), 'LP');
    assert.equal(browser.getPositionCategoryFromButton(1, 2, false, true, false), 'BL');
});

test('AI can make a delayed turn c-bet after the flop checks through', () => {
    const ai = new browser.AIPlayer('Delayed', browser.AI_STYLES.SOLID, 1000, 1);
    ai.holeCards = [new browser.Card('Q','spades'), new browser.Card('J','spades')];
    ai._estimateEquity = () => 0.55;
    ai.config = { ...ai.config, cbFreq:3 };
    ai._applyHeuristics = candidates => candidates.find(candidate => candidate.action === 'check');
    const decision = ai.decide({
        communityCards:[
            new browser.Card('2','clubs'), new browser.Card('7','diamonds'),
            new browser.Card('K','hearts'), new browser.Card('A','clubs')
        ],
        pot:300, currentBet:0, toCall:0, yourBet:0, stack:1000, effectiveStack:1000,
        numOpponentsActive:1, bigBlind:80, minRaiseTo:80, maxRaiseTo:1000,
        canRaise:true, canCheck:true, positionFromButton:0, phase:'turn',
        isPreviousStreetAggressor:false, isDelayedCBetCandidate:true
    });
    assert.ok(['raise','allin'].includes(decision.action));
});

test('opponent model counts hands and betting opportunities without action inflation', () => {
    const model = new browser.PlayerModel(1);
    model.recordAction('call', 'preflop', 1, { toCallBefore:80, aggressive:false });
    model.recordAction('raise', 'preflop', 1, { toCallBefore:160, aggressive:true });
    model.recordAction('fold', 'flop', 1, { toCallBefore:100, aggressive:false, facedCbet:true });
    assert.equal(model.handsSeen, 1);
    assert.equal(model.vpipHands, 1);
    assert.equal(model.pfrHands, 1);
    assert.deepEqual({ ...model.pf }, { fold:0, call:0, raise:1 });
    assert.equal(model.postflopFacedBet, 1);
    assert.equal(model.postflopFolds, 1);
    assert.equal(model.foldedToCbet, 1);

    model.recordAction('bet', 'turn', 2, {
        toCallBefore:0, aggressive:true, isCbetOpportunity:true
    });
    assert.equal(model.handsSeen, 2);
    assert.equal(model.postflopFacedBet, 1);
    assert.equal(model.cbetOpp, 1);
    assert.equal(model.cbetMade, 1);

    model.recordAction('allin', 'river', 3, { toCallBefore:200, aggressive:false });
    assert.equal(model.postflopCalls, 1);
    assert.equal(model.postflopAggressive, 1);
});

test('table position ignores busted empty seats', () => {
    const game = new browser.PokerGame({ startingStack:1000, smallBlind:40, bigBlind:80 });
    game.players = [
        { holeCards:[1,2] }, { holeCards:[] }, { holeCards:[1,2] }, { holeCards:[] }
    ];
    game.playersDealtThisHand = [game.players[0], game.players[2]];
    game.dealerPosition = 0;
    assert.deepEqual({ ...game.getHandPositionInfo(0) }, { positionFromButton:0, playerCount:2 });
    assert.deepEqual({ ...game.getHandPositionInfo(2) }, { positionFromButton:1, playerCount:2 });
});

test('disconnect folding never skips a different current actor or leaves a free-check player in the hand', () => {
    const roomServer = new PokerRoomServer({ disconnectGraceMs:100 });
    const players = [player('A',0), player('B',1), player('C',2)];
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:40, bigBlind:80 }, players);
    game.phase = 'flop';
    game.currentPlayerIdx = 0;
    game.currentBet = 0;
    game.roundBets = {};
    game.playersActed = new Set();
    const room = { game, players };

    roomServer.foldDisconnectedPlayer(room, players[1]);
    assert.equal(players[1].folded, true);
    assert.equal(game.currentPlayerIdx, 0);
    assert.equal(game.playersActed.has(0), false);

    roomServer.foldDisconnectedPlayer(room, players[0]);
    assert.equal(players[0].folded, true);
    assert.ok(game.currentPlayerIdx !== 0 || game.phase === 'idle');
});

test('a failed next-hand attempt preserves the result so reconnect can resume readiness', () => {
    const players = [player('A',0), player('B',1)];
    const game = new ServerPokerGame({ isShortDeck:false, smallBlind:40, bigBlind:80 }, players);
    game.lastHandResult = { reason:'fold', pot:120 };
    game.communityCards = [card('A')];
    players[1].connected = false;
    assert.equal(game.startHand(), false);
    assert.deepEqual(game.lastHandResult, { reason:'fold', pot:120 });
    assert.deepEqual(game.communityCards, []);
});

test('a player leaving during the ready countdown is replaced without stalling', async () => {
    const players = [player('A',0), player('B',1)];
    const config = {
        maxPlayers:2, isShortDeck:false, startingStack:1000,
        smallBlind:40, bigBlind:80, ante:40, minBet:80
    };
    const game = new ServerPokerGame(config, players, { random:() => 0.25 });
    game.phase = 'idle';
    game.lastHandResult = { reason:'fold', pot:120 };
    const room = {
        code:'5', config, players, game, ready:new Set(['A','B']),
        nextHandTimer:null, stateVersion:0
    };
    const roomServer = new PokerRoomServer();
    roomServer.rooms.set(room.code, room);
    assert.equal(roomServer.tryStartNextHand(room), true);
    players[0].left = true;
    players[0].connected = false;
    await new Promise(resolve => setTimeout(resolve, 320));
    assert.equal(game.handId, 1);
    assert.equal(game.phase, 'preflop');
    assert.equal(room.players.filter(item => !item.isHuman).length, 1);
});
