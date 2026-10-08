'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');
const rules = require('../js/game-rules-core');
const aiCore = require('../js/ai-core');
const { ServerPokerGame, createServerAi } = require('../server/server-game');
const { AIPlayer, AI_STYLES, PlayerModel } = require('../js/ai');
const engine = require('../js/engine');

test('desktop service discovery rejects old-version services while accepting the matching release', async () => {
    const {WebSocket,WebSocketServer}=require('ws');
    const version=require('../package.json').version;
    const source=fs.readFileSync(path.join(__dirname,'..','main.js'),'utf8');
    const start=source.indexOf('function probePokerServer(');
    const end=source.indexOf('\nasync function ensureLocalServer',start);
    assert.ok(start>=0&&end>start);
    const context=vm.createContext({WebSocket,setTimeout,clearTimeout,APP_VERSION:version});
    vm.runInContext(source.slice(start,end),context);
    for(const [appVersion,serverId,expected] of [[version,'texas-holdem-game',true],['1.5.7','texas-holdem-game',false],[undefined,'texas-holdem-game',false],[version,'other-service',false]]){
        const server=new WebSocketServer({port:0,host:'127.0.0.1'});
        await new Promise(resolve=>server.once('listening',resolve));
        server.on('connection',socket=>socket.send(JSON.stringify({type:'hello',protocolVersion:2,appVersion,serverId})));
        try {assert.equal(await context.probePokerServer(`ws://127.0.0.1:${server.address().port}`),expected);}
        finally {for(const socket of server.clients)socket.terminate();await new Promise(resolve=>server.close(resolve));}
    }
});

test('raise grid rounds up the minimum and down the maximum without rounding all-in stacks', () => {
    assert.deepEqual(rules.raiseBounds(203, 999, 40), {min:240, max:960, unit:40});
    assert.equal(rules.snapRaise(333, 203, 999, 40), 320);
    assert.equal(rules.snapRaise(999, 203, 999, 40), 960);
    assert.equal(rules.snapRaise(210, 203, 239, 40), null);
    assert.equal(rules.wagerUnit({isShortDeck:true, config:{smallBlind:40, ante:20}}), 20);
});

test('authoritative server rejects fractional/off-grid raises but preserves odd all-ins and calls', () => {
    const players = Array.from({length:3}, (_,i) => createServerAi(i, 1000));
    players.forEach(p => { p.isHuman = true; });
    const g = new ServerPokerGame({smallBlind:40, bigBlind:80, isShortDeck:false}, players);
    g.startHand();
    const idx = g.currentPlayerIdx;
    const originalPot = g.pot;
    assert.equal(g.act(idx, 'raise', 201).ok, false);
    assert.equal(g.act(idx, 'raise', 200.5).ok, false);
    assert.equal(g.pot, originalPot);
    assert.equal(g.act(idx, 'raise', 200).ok, true);
    const next = g.currentPlayerIdx;
    players[next].stack = 263;
    const exact = (g.roundBets[next] || 0) + 263;
    assert.equal(g.act(next, 'allin', 263).ok, true);
    assert.equal(g.currentBet, exact);
    const legal = g.legalActions(g.currentPlayerIdx);
    assert.equal(legal.minRaiseTo % 40, 0);
    assert.equal(g.act(g.currentPlayerIdx, 'call').ok, true);
});

test('local betting kernel snaps raises and disposes AI state while cancelling suspended turns', () => {
    const ai = { disposed:0, dispose() { this.disposed++; } };
    const players = [{stack:1000, chipsInPot:0, isHuman:true}, {stack:1000, chipsInPot:0}];
    const g = { players, aiPlayers:[{aiRef:ai}], humanPlayer:players[0], smallBlind:40, bigBlind:80,
        roundBets:{0:0,1:80}, currentBet:80, pot:80, lastRaise:80, hasFullBetThisRound:true,
        playersActed:new Set(), actedAtBet:{}, raiseSizeAtAction:{}, phase:'preflop',
        currentPlayerIndex:0, preflopRaiseCount:0, handGeneration:3, isProcessing:true,
        notifyAIsOfAction() {}, onUpdate:null };
    engine.executeAction(g, 0, 'raise', 213);
    assert.equal(g.roundBets[0], 200);
    assert.equal(g.pot, 280);
    engine.resetGame(g);
    assert.equal(g.handGeneration, 4);
    assert.equal(g.isProcessing, false);
    assert.equal(ai.disposed, 1);
});

test('AI candidates use the blind grid before evaluating value, with exact all-in exceptions', () => {
    const brain = new aiCore.UnifiedPokerAI({seatId:0});
    const ctx = aiCore.normalizedContext({bigBlind:100, wagerUnit:50, pot:477, currentBet:103,
        toCall:103, minRaiseTo:206, maxRaiseTo:999, stack:999, phase:'river',
        legalActions:['fold','call','raise','allin'], canRaise:true});
    const targets = brain.raiseTargets(ctx, {wetness:.2});
    assert.ok(targets.length > 1);
    assert.ok(targets.every(t => t === 999 || (t % 50 === 0 && t >= 206 && t < 999)));
    assert.equal(brain.legalize({action:'raise', amount:333}, ctx).amount, 350);
    const bot = new AIPlayer('Grid', AI_STYLES.SOLID, 999, 0);
    assert.equal(bot._legalize({action:'raise', amount:333}, {...ctx, yourBet:0, stack:999, smallBlind:50}).amount, 350);
    assert.equal(bot._legalize({action:'allin', amount:999}, {...ctx, stack:999}).amount, 999);
});

test('response branches sum to one and a heads-up called branch has exactly one caller', () => {
    const brain = new aiCore.UnifiedPokerAI({seatId:0});
    const ctx = aiCore.normalizedContext({phase:'river', pot:500, bigBlind:80, stack:5000, effectiveStack:5000,
        toCall:0, currentBet:0, numOpponents:1, activeOpponentSeats:[1]});
    const reads = brain.activeReads(ctx);
    const texture = { wetness:0, hasStrongDraw:false };
    const response = brain.responseForRaise(ctx, 320, .5, texture, reads);
    assert.ok(Math.abs(response.allFold + response.anyCall + response.anyRaise - 1) < 1e-12);
    assert.ok(Math.abs(response.expectedCallers - 1) < 1e-12);
    assert.equal(brain.realizationFactor(ctx, texture, reads), 1);
    const multi = {...ctx, numOpponents:3, activeOpponentSeats:[1,2,3]};
    const mr = brain.responseForRaise(multi, 320, .5, texture, brain.activeReads(multi));
    assert.ok(Math.abs(mr.allFold + mr.anyCall + mr.anyRaise - 1) < 1e-12);
    assert.ok(mr.expectedCallers >= 1 && mr.expectedCallers <= 3);
});

test('opponent histories stay bounded over 2000 hands and probabilities remain consistent', () => {
    const shared = new aiCore.BehavioralModel(1);
    const legacy = new PlayerModel(1);
    const self = new aiCore.SelfImage();
    for (let id=0; id<2000; id++) {
        const event = {handId:id, action:id % 2 ? 'raise' : 'fold', street:'preflop'};
        shared.observe(event);
        legacy.recordAction(event.action, event.street, id);
        self.record({action:event.action, amount:0}, {handId:id, street:'preflop'}, 'value');
    }
    assert.equal(shared.hands.size, 256);
    assert.equal(shared.vpipHands.size, 128);
    assert.equal(shared.pfrHands.size, 128);
    assert.equal(legacy.seenHandIds.size, 256);
    assert.equal(legacy.handFlags.size, 256);
    assert.equal(legacy.handsSeen, 2000);
    assert.equal(self.hands.size, 256);
    assert.ok(shared.vpip > .45 && shared.vpip < .55);
});

test('AI worker release removes the old brain instead of creating a cleanup brain', () => {
    const context = vm.createContext({ console, self:{}, Map });
    context.self = context;
    context.postMessage = () => {};
    context.importScripts = (...files) => {
        for (const file of files) vm.runInContext(fs.readFileSync(path.join(__dirname, '..', 'js', file), 'utf8'), context);
    };
    vm.runInContext(fs.readFileSync(path.join(__dirname, '..', 'js', 'ai-worker.js'), 'utf8'), context);
    for (let i=0; i<50; i++) {
        context.onmessage({data:{type:'observe', key:`bot-${i}`, event:{handId:1, seatId:1, action:'call', street:'preflop'}}});
        context.onmessage({data:{type:'release', key:`bot-${i}`}});
    }
    context.onmessage({data:{type:'release', key:'never-created'}});
    assert.equal(vm.runInContext('brains.size', context), 0);
});
